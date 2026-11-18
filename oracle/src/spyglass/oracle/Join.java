package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicReference;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;
import com.mojang.authlib.GameProfile;

import io.netty.channel.embedded.EmbeddedChannel;
import jdk.jfr.consumer.RecordedEvent;
import jdk.jfr.consumer.RecordingFile;
import net.minecraft.SharedConstants;
import net.minecraft.network.Connection;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.network.protocol.common.ClientboundKeepAlivePacket;
import net.minecraft.network.protocol.common.ServerboundKeepAlivePacket;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.CommonListenerCookie;
import net.minecraft.server.network.config.PrepareSpawnTask;
import net.minecraft.server.players.NameAndId;
import net.minecraft.util.profiling.jfr.JvmProfiler;
import net.minecraft.world.level.ChunkPos;

/**
 * The game's own dedicated server, started in this JVM, with one player joined at the world
 * spawn the way a client's join does, so that the chunks a first join decorates are generated
 * by the game's own tickets and scheduler. Run by {@code cargo xtask oracle order}; see
 * {@code docs/adr/0073-the-oracle-joins-a-stand-in-player.md} and
 * {@code docs/specs/oracle.md#order-files}.
 *
 * <pre>
 * Join &lt;out file&gt; [--no-join] [--hold SECONDS] -- &lt;net.minecraft.server.Main arguments&gt;
 * </pre>
 *
 * The server arguments must include {@code --jfrProfile}. The join is the configuration
 * phase's {@code PrepareSpawnTask} (the spawn search and its chunks), then its
 * {@code spawnPlayer}, which places the player with {@code PlayerList}. Only the network
 * connection is a stand-in: a {@code Connection} on Netty's {@code EmbeddedChannel}, as the
 * game's own gametest mock player has. What the server sends it is dropped, except that
 * keep-alives are answered, as a client would, so the server doesn't drop the player.
 *
 * Once every chunk within the view distance of the player is loaded in full, it waits
 * {@code --hold} more seconds (default 2) for generation to settle, then stops the server,
 * and its profiler, which writes the JFR recording, and writes the recording's {@code FEATURES}
 * steps to the out file. With {@code --no-join} it stops as soon as the server is ready, to record startup alone.
 * Progress goes to stdout as lines starting {@code spyglass join:}.
 */
public final class Join {
    /** Bump when the order file's content changes. */
    static final int FORMAT = 1;

    /** Longest wait for any one stage, so a stuck server fails the run. */
    private static final long STAGE_TIMEOUT_MS = 300_000;

    /** The game's JFR event for a generation step ({@code ChunkGenerationEvent}). */
    private static final String CHUNK_EVENT = "minecraft.ChunkGeneration";

    private Join() {}

    public static void main(String[] args) {
        try {
            run(args);
            System.exit(0);
        } catch (Throwable e) {
            e.printStackTrace();
            // The server's threads aren't daemons, so only exit ends the JVM.
            Runtime.getRuntime().halt(1);
        }
    }

    private static void run(String[] args) throws Exception {
        if (args.length < 1) {
            throw new IllegalArgumentException("usage: Join <out file> [--no-join] [--hold SECONDS] -- <server args>");
        }
        Path out = Path.of(args[0]).toAbsolutePath();
        boolean join = true;
        double holdSeconds = 2;
        int i = 1;
        for (; i < args.length && !args[i].equals("--"); i++) {
            switch (args[i]) {
                case "--no-join" -> join = false;
                case "--hold" -> holdSeconds = Double.parseDouble(args[++i]);
                default -> throw new IllegalArgumentException("unknown option " + args[i]);
            }
        }
        String[] serverArgs = i < args.length ? Arrays.copyOfRange(args, i + 1, args.length) : new String[0];
        if (!Arrays.asList(serverArgs).contains("--jfrProfile")) {
            throw new IllegalArgumentException("the server arguments need --jfrProfile");
        }

        // Main returns once the server thread is started.
        net.minecraft.server.Main.main(serverArgs);
        MinecraftServer server = findServer();
        waitFor("the server to be ready", server::isReady, () -> {});
        Instant ready = Instant.now();
        report("ready");

        var root = new JsonObject();
        root.addProperty("format", FORMAT);
        root.addProperty("harness", Main.HARNESS_VERSION);
        var current = SharedConstants.getCurrentVersion();
        root.addProperty("version", System.getProperty("spyglass.oracle.version", current.id()));
        root.addProperty("data_version", current.dataVersion().version());
        root.addProperty("seed", server.overworld().getSeed());
        root.addProperty("processors", Runtime.getRuntime().availableProcessors());

        if (join) {
            var joined = joinPlayer(server);
            ServerPlayer player = joined.player;
            ChunkPos centre = player.chunkPosition();
            int radius = server.getPlayerList().getViewDistance();
            report("joined at chunk %d %d, view distance %d", centre.x(), centre.z(), radius);
            var playerJson = new JsonObject();
            playerJson.add("chunk", ints(centre.x(), centre.z()));
            playerJson.add("pos", ints(player.getBlockX(), player.getBlockY(), player.getBlockZ()));
            root.add("player", playerJson);
            root.addProperty("view_distance", radius);
            waitFor("the chunks within the view distance", () -> server.submit(() -> {
                if (player.isRemoved()) {
                    throw new IllegalStateException("the player was removed before its chunks loaded");
                }
                var chunks = player.level().getChunkSource();
                for (int dz = -radius; dz <= radius; dz++) {
                    for (int dx = -radius; dx <= radius; dx++) {
                        if (chunks.getChunkNow(centre.x() + dx, centre.z() + dz) == null) {
                            return false;
                        }
                    }
                }
                return true;
            }).join(), joined::answer);
            report("loaded");
            long holdUntil = System.currentTimeMillis() + (long) (holdSeconds * 1000);
            waitFor("the hold", () -> System.currentTimeMillis() >= holdUntil, joined::answer);
            if (server.submit(player::isRemoved).join()) {
                throw new IllegalStateException("the player was removed during the hold");
            }
        }
        report("stopping");
        server.halt(true);
        report("stopped");

        // Nothing in the server stops a profiler started by --jfrProfile before the JVM exits;
        // this is what /jfr stop does.
        Path recording = JvmProfiler.INSTANCE.stop();
        root.add("features", features(recording, ready));
        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            new Gson().toJson(root, writer);
        }
        Files.createDirectories(out.getParent());
        Files.writeString(out, text + "\n");
        report("wrote %s", out);
    }

    /**
     * The Overworld's {@code FEATURES} steps in the recording, in start order: each chunk, when
     * the step started (nanoseconds after the first), how long it took, its thread, and whether
     * it started before the server was ready (at startup, before any join).
     */
    private static JsonArray features(Path recording, Instant ready) throws IOException {
        var steps = new ArrayList<RecordedEvent>();
        for (RecordedEvent event : RecordingFile.readAllEvents(recording)) {
            if (event.getEventType().getName().equals(CHUNK_EVENT)
                    && event.getString("status").equals("minecraft:features")
                    && event.getString("level").equals("minecraft:overworld")) {
                steps.add(event);
            }
        }
        if (steps.isEmpty()) {
            throw new IllegalStateException("the recording has no Overworld features steps");
        }
        steps.sort(Comparator.comparing(RecordedEvent::getStartTime));
        Instant first = steps.get(0).getStartTime();
        var array = new JsonArray();
        for (RecordedEvent step : steps) {
            var o = new JsonObject();
            o.add("chunk", ints(step.getInt("chunkPosX"), step.getInt("chunkPosZ")));
            o.addProperty("start_ns", java.time.Duration.between(first, step.getStartTime()).toNanos());
            o.addProperty("duration_ns", step.getDuration().toNanos());
            o.addProperty("thread", step.getThread() == null ? "" : step.getThread().getJavaName());
            o.addProperty("startup", step.getStartTime().isBefore(ready));
            array.add(o);
        }
        return array;
    }

    /** The joined player, and the stand-in client's side of its connection. */
    private record Joined(MinecraftServer server, ServerPlayer player, EmbeddedChannel channel) {
        /**
         * Drops what the server sent, answering each keep-alive as a client would. Runs on
         * this thread; the answer is handled on the server's.
         */
        void answer() {
            for (Object packet; (packet = channel.readOutbound()) != null;) {
                if (packet instanceof ClientboundKeepAlivePacket keepAlive) {
                    long id = keepAlive.getId();
                    server.execute(() -> player.connection.handleKeepAlive(new ServerboundKeepAlivePacket(id)));
                }
                io.netty.util.ReferenceCountUtil.release(packet);
            }
        }
    }

    /**
     * Joins one player as the configuration phase would: the spawn task started, ticked once a
     * tick until it's ready, then asked to spawn the player on a stand-in connection.
     */
    private static Joined joinPlayer(MinecraftServer server) throws Exception {
        String name = "spyglass";
        var profile = new GameProfile(
                UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(StandardCharsets.UTF_8)), name);
        var task = new PrepareSpawnTask(server, new NameAndId(profile));
        server.submit(() -> task.start(packet -> {})).join();
        waitFor("the spawn to be prepared", () -> server.submit(task::tick).join(), () -> {});
        return server.submit(() -> {
            var connection = new Connection(PacketFlow.SERVERBOUND);
            var channel = new EmbeddedChannel(connection);
            var player = task.spawnPlayer(connection, CommonListenerCookie.createInitial(profile, false));
            return new Joined(server, player, channel);
        }).join();
    }

    /**
     * The server {@code Main} started. Nothing in the game holds it statically: the server
     * thread's task, from {@code MinecraftServer.spin}, holds it in an {@code AtomicReference}.
     * Needs {@code --add-opens java.base/java.lang=ALL-UNNAMED} to read the thread's task.
     */
    static MinecraftServer findServer() throws ReflectiveOperationException {
        for (Thread thread : Thread.getAllStackTraces().keySet()) {
            if (!thread.getName().equals("Server thread")) {
                continue;
            }
            Field holderField = Thread.class.getDeclaredField("holder");
            holderField.setAccessible(true);
            Object holder = holderField.get(thread);
            Field taskField = holder.getClass().getDeclaredField("task");
            taskField.setAccessible(true);
            Object task = taskField.get(holder);
            for (Field field : task.getClass().getDeclaredFields()) {
                field.setAccessible(true);
                if (field.get(task) instanceof AtomicReference<?> ref && ref.get() instanceof MinecraftServer server) {
                    return server;
                }
            }
        }
        throw new IllegalStateException("no server thread holding a MinecraftServer");
    }

    interface Check {
        boolean done() throws Exception;
    }

    /**
     * Polls {@code check} every 50 ms (a tick), running {@code between} each time, until it's
     * done, or fails after the timeout.
     */
    static void waitFor(String what, Check check, Runnable between) throws Exception {
        long deadline = System.currentTimeMillis() + STAGE_TIMEOUT_MS;
        while (true) {
            between.run();
            if (check.done()) {
                return;
            }
            if (System.currentTimeMillis() > deadline) {
                throw new IllegalStateException("timed out waiting for " + what);
            }
            Thread.sleep(50);
        }
    }

    private static JsonArray ints(int... values) {
        var array = new JsonArray();
        for (int v : values) {
            array.add(v);
        }
        return array;
    }

    private static void report(String format, Object... args) {
        System.out.println("spyglass join: " + String.format(format, args));
        System.out.flush();
    }
}
