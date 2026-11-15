package spyglass.oracle;

import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicReference;

import com.mojang.authlib.GameProfile;

import io.netty.channel.embedded.EmbeddedChannel;
import net.minecraft.network.Connection;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.CommonListenerCookie;
import net.minecraft.server.network.config.PrepareSpawnTask;
import net.minecraft.server.players.NameAndId;
import net.minecraft.world.level.ChunkPos;

/**
 * The game's own dedicated server, started in this JVM, with one player joined at the world
 * spawn the way a client's join does, so that the chunks a first join decorates are generated
 * by the game's own tickets and scheduler. Run by {@code cargo xtask oracle order}; see
 * {@code docs/adr/0073-the-oracle-joins-a-stand-in-player.md}.
 *
 * <pre>
 * Join [--no-join] [--hold SECONDS] -- &lt;net.minecraft.server.Main arguments&gt;
 * </pre>
 *
 * The join is the configuration phase's {@code PrepareSpawnTask} (the spawn search and its
 * chunks), then its {@code spawnPlayer}, which places the player with {@code PlayerList}. Only
 * the network connection is a stand-in: a {@code Connection} on Netty's {@code EmbeddedChannel},
 * as the game's own gametest mock player has. Once every chunk within the view distance of the
 * player is loaded in full, it waits {@code --hold} more seconds (default 2) for generation to
 * settle, then stops the server, which writes the JFR recording that {@code --jfrProfile}
 * started. With {@code --no-join} it stops as soon as the server is ready, to record startup
 * alone.
 *
 * Progress goes to stdout as lines starting {@code spyglass join:}, for the xtask to read.
 */
public final class Join {
    /** Longest wait for any one stage, so a stuck server fails the run. */
    private static final long STAGE_TIMEOUT_MS = 300_000;

    private Join() {}

    public static void main(String[] args) throws Exception {
        boolean join = true;
        double holdSeconds = 2;
        int i = 0;
        for (; i < args.length && !args[i].equals("--"); i++) {
            switch (args[i]) {
                case "--no-join" -> join = false;
                case "--hold" -> holdSeconds = Double.parseDouble(args[++i]);
                default -> throw new IllegalArgumentException("unknown option " + args[i]);
            }
        }
        String[] serverArgs = i < args.length ? Arrays.copyOfRange(args, i + 1, args.length) : new String[0];

        // Main returns once the server thread is started.
        net.minecraft.server.Main.main(serverArgs);
        MinecraftServer server = findServer();
        waitFor("the server to be ready", server::isReady);
        report("ready");
        if (join) {
            ServerPlayer player = joinPlayer(server);
            ChunkPos centre = player.chunkPosition();
            int radius = server.getPlayerList().getViewDistance();
            report("joined at chunk %d %d, view distance %d", centre.x(), centre.z(), radius);
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
            }).join());
            report("loaded");
            Thread.sleep((long) (holdSeconds * 1000));
        }
        report("stopping");
        server.halt(true);
        report("stopped");
        System.exit(0);
    }

    /**
     * Joins one player as the configuration phase would: the spawn task started, ticked once a
     * tick until it's ready, then asked to spawn the player on a stand-in connection.
     */
    private static ServerPlayer joinPlayer(MinecraftServer server) throws Exception {
        String name = "spyglass";
        var profile = new GameProfile(
                UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(StandardCharsets.UTF_8)), name);
        var task = new PrepareSpawnTask(server, new NameAndId(profile));
        server.submit(() -> task.start(packet -> {})).join();
        waitFor("the spawn to be prepared", () -> server.submit(task::tick).join());
        return server.submit(() -> {
            var connection = new Connection(PacketFlow.SERVERBOUND);
            new EmbeddedChannel(connection);
            return task.spawnPlayer(connection, CommonListenerCookie.createInitial(profile, false));
        }).join();
    }

    /**
     * The server {@code Main} started. Nothing in the game holds it statically: the server
     * thread's task, from {@code MinecraftServer.spin}, holds it in an {@code AtomicReference}.
     * Needs {@code --add-opens java.base/java.lang=ALL-UNNAMED} to read the thread's task.
     */
    private static MinecraftServer findServer() throws ReflectiveOperationException {
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

    private interface Check {
        boolean done() throws Exception;
    }

    /** Polls {@code check} every 50 ms (a tick) until it's done, or fails after the timeout. */
    private static void waitFor(String what, Check check) throws Exception {
        long deadline = System.currentTimeMillis() + STAGE_TIMEOUT_MS;
        while (!check.done()) {
            if (System.currentTimeMillis() > deadline) {
                throw new IllegalStateException("timed out waiting for " + what);
            }
            Thread.sleep(50);
        }
    }

    private static void report(String format, Object... args) {
        System.out.println("spyglass join: " + String.format(format, args));
        System.out.flush();
    }
}
