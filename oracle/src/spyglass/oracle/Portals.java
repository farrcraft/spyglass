package spyglass.oracle;

import java.io.StringWriter;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.stream.JsonWriter;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.BlockUtil;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.ai.village.poi.PoiType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.NetherPortalBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.portal.PortalForcer;
import net.minecraft.world.level.portal.TeleportTransition;

/**
 * The game's own portal code, run on its dedicated server in this JVM, for a list of cases. Run
 * by {@code cargo xtask oracle portals}; see
 * {@code docs/adr/0076-the-oracle-drives-the-game-s-portal-code.md} and
 * {@code docs/specs/oracle.md#portal-files}.
 *
 * <pre>
 * Portals &lt;cases file&gt; &lt;out file&gt; -- &lt;net.minecraft.server.Main arguments&gt;
 * </pre>
 *
 * Each layout of the cases file is played on the server thread, one after another:
 *
 * <ol>
 *   <li>each dimension's border it names is set, with {@code WorldBorder.setCenter} and
 *       {@code setSize};
 *   <li>its portals are placed with {@code Level.setBlock}, which adds their points of interest
 *       as play does, and its stale records are added with {@code PoiManager.add} where the block
 *       isn't a portal;
 *   <li>each search is the target level's {@code PortalForcer.findClosestPortalPosition}, with
 *       the exit block given;
 *   <li>each trip is {@code NetherPortalBlock.getPortalDestination} for a marker entity standing
 *       at the position given, inside the portal it names. It records the exit block the game
 *       searched from and the block it found, through a {@code PortalForcer} that calls the
 *       game's own and notes its arguments and answer. A trip that finds nothing makes the game
 *       build a portal; its portal blocks are set to air before the next case;
 *   <li>everything is undone: the portal blocks and stale records removed and the borders put
 *       back.
 * </ol>
 *
 * The out file is the cases file with each search's and trip's answer added. Progress goes to
 * stdout as lines starting {@code spyglass portals:}.
 */
public final class Portals {
    /** Bump when the portal file's content changes. */
    static final int FORMAT = 1;

    /**
     * {@code Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE}: no neighbour updates, so a portal
     * placed without a frame isn't broken by its neighbours' shape updates.
     */
    private static final int FLAGS = Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE;

    /**
     * The portal's point-of-interest type, by key: {@code PoiTypes.NETHER_PORTAL} is a key in
     * 26.2 and 26.3 and a holder in 26.4.
     */
    private static final ResourceKey<PoiType> NETHER_PORTAL =
            ResourceKey.create(Registries.POINT_OF_INTEREST_TYPE, Identifier.withDefaultNamespace("nether_portal"));

    private Portals() {}

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
        int split = Arrays.asList(args).indexOf("--");
        if (split != 2) {
            throw new IllegalArgumentException("usage: Portals <cases file> <out file> -- <server args>");
        }
        Path casesFile = Path.of(args[0]);
        Path out = Path.of(args[1]).toAbsolutePath();
        String[] serverArgs = Arrays.copyOfRange(args, split + 1, args.length);
        JsonObject cases = JsonParser.parseString(Files.readString(casesFile)).getAsJsonObject();

        // Main returns once the server thread is started.
        net.minecraft.server.Main.main(serverArgs);
        MinecraftServer server = Join.findServer();
        Join.waitFor("the server to be ready", server::isReady, () -> {});
        report("ready");

        Map<ResourceKey<Level>, Recorder> recorders = server.submit(() -> {
            try {
                return install(server);
            } catch (ReflectiveOperationException e) {
                throw new IllegalStateException(e);
            }
        }).join();
        JsonArray layouts = cases.getAsJsonArray("layouts");
        for (int i = 0; i < layouts.size(); i++) {
            JsonObject layout = layouts.get(i).getAsJsonObject();
            play(server, recorders, layout);
            if ((i + 1) % 20 == 0 || i + 1 == layouts.size()) {
                report("played %d of %d layouts", i + 1, layouts.size());
            }
        }

        var root = new JsonObject();
        root.addProperty("format", FORMAT);
        root.addProperty("harness", Main.HARNESS_VERSION);
        var current = SharedConstants.getCurrentVersion();
        root.addProperty("version", System.getProperty("spyglass.oracle.version", current.id()));
        root.addProperty("data_version", current.dataVersion().version());
        root.add("layouts", layouts);
        report("stopping");
        server.halt(true);
        report("stopped");

        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            new GsonBuilder().serializeNulls().create().toJson(root, writer);
        }
        Files.createDirectories(out.getParent());
        Files.writeString(out, text + "\n");
        report("wrote %s", out);
    }

    /**
     * The game's {@code PortalForcer}, noting what {@code getPortalDestination} asks it: the exit
     * block it searched from, what it found, and the portals it built.
     */
    static final class Recorder extends PortalForcer {
        final ServerLevel level;
        BlockPos exit;
        Optional<BlockPos> found;
        final List<BlockUtil.FoundRectangle> built = new ArrayList<>();

        Recorder(ServerLevel level) {
            super(level);
            this.level = level;
        }

        @Override
        public Optional<BlockPos> findClosestPortalPosition(BlockPos exit, boolean toNether, WorldBorder border) {
            Optional<BlockPos> answer = super.findClosestPortalPosition(exit, toNether, border);
            this.exit = exit.immutable();
            this.found = answer.map(BlockPos::immutable);
            return answer;
        }

        @Override
        public Optional<BlockUtil.FoundRectangle> createPortal(BlockPos pos, Direction.Axis axis) {
            Optional<BlockUtil.FoundRectangle> made = super.createPortal(pos, axis);
            made.ifPresent(built::add);
            return made;
        }

        void clear() {
            exit = null;
            found = null;
            built.clear();
        }
    }

    /**
     * Puts a {@link Recorder} in each level's {@code portalForcer}, a private final field the
     * level's {@code getPortalForcer} returns. Each still runs the game's code.
     */
    private static Map<ResourceKey<Level>, Recorder> install(MinecraftServer server) throws ReflectiveOperationException {
        Field field = ServerLevel.class.getDeclaredField("portalForcer");
        field.setAccessible(true);
        var recorders = new HashMap<ResourceKey<Level>, Recorder>();
        for (ServerLevel level : server.getAllLevels()) {
            var recorder = new Recorder(level);
            field.set(level, recorder);
            if (level.getPortalForcer() != recorder) {
                throw new IllegalStateException("couldn't replace " + level.dimension() + "'s portal forcer");
            }
            recorders.put(level.dimension(), recorder);
        }
        return recorders;
    }

    /**
     * One layout: set up, each search and trip answered into its object, then undone, each as
     * its own server task. The game adds and removes a block's record in a task of its own
     * ({@code ServerLevel.updatePOIOnBlockStateChange} hands it to {@code MinecraftServer.execute}),
     * which runs before the next task submitted after it.
     */
    private static void play(MinecraftServer server, Map<ResourceKey<Level>, Recorder> recorders, JsonObject layout) {
        var played = new Played(server, layout);
        try {
            server.submit(played::setUp).join();
            server.submit(() -> played.answer(recorders)).join();
        } finally {
            server.submit(played::undo).join();
            // The records' removals, queued by undo.
            server.submit(() -> {}).join();
        }
    }

    /** What a layout changed, to undo. */
    private static final class Played {
        final MinecraftServer server;
        final JsonObject layout;
        final String name;
        final List<Runnable> borders = new ArrayList<>();
        final List<Placed> placed = new ArrayList<>();

        Played(MinecraftServer server, JsonObject layout) {
            this.server = server;
            this.layout = layout;
            this.name = layout.get("name").getAsString();
        }

        /** The borders set, the portals placed and the stale records added. */
        void setUp() {
            for (JsonElement element : array(layout, "borders")) {
                JsonObject border = element.getAsJsonObject();
                WorldBorder worldBorder = level(server, border).getWorldBorder();
                double centreX = worldBorder.getCenterX();
                double centreZ = worldBorder.getCenterZ();
                double size = worldBorder.getSize();
                borders.add(() -> {
                    worldBorder.setCenter(centreX, centreZ);
                    worldBorder.setSize(size);
                });
                JsonArray centre = border.getAsJsonArray("center");
                worldBorder.setCenter(centre.get(0).getAsDouble(), centre.get(1).getAsDouble());
                worldBorder.setSize(border.get("size").getAsDouble());
            }
            for (JsonElement element : array(layout, "portals")) {
                JsonObject portal = element.getAsJsonObject();
                ServerLevel level = level(server, portal);
                Direction.Axis axis = portal.get("axis").getAsString().equals("x") ? Direction.Axis.X : Direction.Axis.Z;
                BlockPos corner = pos(portal.getAsJsonArray("corner"));
                BlockState state = net.minecraft.world.level.block.Blocks.NETHER_PORTAL.defaultBlockState()
                        .setValue(NetherPortalBlock.AXIS, axis);
                int width = portal.get("width").getAsInt();
                int height = portal.get("height").getAsInt();
                for (int along = 0; along < width; along++) {
                    for (int up = 0; up < height; up++) {
                        BlockPos block = axis == Direction.Axis.X ? corner.offset(along, up, 0) : corner.offset(0, up, along);
                        placed.add(new Placed(level, block, false));
                        if (!level.setBlock(block, state, FLAGS)) {
                            throw new IllegalStateException(name + ": couldn't place a portal at " + block);
                        }
                    }
                }
            }
            for (JsonElement element : array(layout, "stale")) {
                JsonObject stale = element.getAsJsonObject();
                ServerLevel level = level(server, stale);
                BlockPos block = pos(stale.getAsJsonArray("block"));
                placed.add(new Placed(level, block, true));
                level.setBlock(block, net.minecraft.world.level.block.Blocks.STONE.defaultBlockState(), FLAGS);
                level.getPoiManager().add(block,
                        level.registryAccess().lookupOrThrow(Registries.POINT_OF_INTEREST_TYPE).getOrThrow(NETHER_PORTAL));
            }
        }

        /** Every placed block has its record by now; then the searches and trips. */
        void answer(Map<ResourceKey<Level>, Recorder> recorders) {
            for (Placed block : placed) {
                if (block.level.getPoiManager().getType(block.pos).filter(type -> type.is(NETHER_PORTAL)).isEmpty()) {
                    throw new IllegalStateException(name + ": no portal record at " + block.pos);
                }
            }
            for (JsonElement element : array(layout, "searches")) {
                JsonObject search = element.getAsJsonObject();
                ServerLevel level = level(server, search);
                Optional<BlockPos> found = level.getPortalForcer().findClosestPortalPosition(
                        pos(search.getAsJsonArray("exit")), level.dimension() == Level.NETHER, level.getWorldBorder());
                search.add("found", found.<JsonElement>map(Portals::ints).orElse(JsonNull.INSTANCE));
            }
            for (JsonElement element : array(layout, "trips")) {
                trip(server, recorders, layout, element.getAsJsonObject());
            }
        }

        /** The placed blocks set to air, the stale records removed and the borders put back. */
        void undo() {
            for (Placed block : placed) {
                if (block.stale) {
                    block.level.getPoiManager().remove(block.pos);
                }
                block.level.setBlock(block.pos, net.minecraft.world.level.block.Blocks.AIR.defaultBlockState(), FLAGS);
            }
            borders.forEach(Runnable::run);
        }
    }

    /** A block a layout changed, and whether its record is a stale one it added. */
    private record Placed(ServerLevel level, BlockPos pos, boolean stale) {}

    /**
     * One trip: a marker at the trip's position in its dimension, inside the portal it names,
     * through {@code getPortalDestination}. Adds the dimension it led to, the exit block, the
     * block found (or null) and whether the game built a portal.
     */
    private static void trip(MinecraftServer server, Map<ResourceKey<Level>, Recorder> recorders, JsonObject layout, JsonObject trip) {
        ServerLevel from = level(server, trip);
        JsonArray at = trip.getAsJsonArray("pos");
        double x = at.get(0).getAsDouble();
        double y = at.get(1).getAsDouble();
        double z = at.get(2).getAsDouble();
        JsonObject portal = layout.getAsJsonArray("portals").get(trip.get("portal").getAsInt()).getAsJsonObject();
        BlockPos entry = entryBlock(portal, x, y, z);
        if (!from.getBlockState(entry).is(net.minecraft.world.level.block.Blocks.NETHER_PORTAL)) {
            throw new IllegalStateException(layout.get("name").getAsString() + ": the trip's portal isn't at " + entry);
        }
        Entity marker = EntityTypes.MARKER.create(from, EntitySpawnReason.COMMAND);
        marker.setPos(x, y, z);
        recorders.values().forEach(Recorder::clear);
        TeleportTransition transition = ((NetherPortalBlock) net.minecraft.world.level.block.Blocks.NETHER_PORTAL)
                .getPortalDestination(from, marker, entry);
        Recorder searched = null;
        for (Recorder recorder : recorders.values()) {
            if (recorder.exit != null) {
                if (searched != null) {
                    throw new IllegalStateException("one trip searched two dimensions");
                }
                searched = recorder;
            }
        }
        if (searched == null) {
            throw new IllegalStateException(layout.get("name").getAsString() + ": the trip searched no dimension");
        }
        trip.addProperty("to", searched.level.dimension().identifier().toString());
        trip.add("exit", ints(searched.exit));
        trip.add("found", searched.found.<JsonElement>map(Portals::ints).orElse(JsonNull.INSTANCE));
        trip.addProperty("built", !searched.built.isEmpty());
        trip.addProperty("transition", transition != null);
        for (BlockUtil.FoundRectangle built : searched.built) {
            removePortalBlocks(searched.level, built);
        }
    }

    /**
     * The block of the portal the marker is in: its position floored, then moved into the
     * portal's blocks. Only where the entity lands in the found portal depends on it.
     */
    private static BlockPos entryBlock(JsonObject portal, double x, double y, double z) {
        BlockPos corner = pos(portal.getAsJsonArray("corner"));
        boolean alongX = portal.get("axis").getAsString().equals("x");
        int width = portal.get("width").getAsInt();
        int height = portal.get("height").getAsInt();
        int bx = alongX ? clamp((int) Math.floor(x), corner.getX(), corner.getX() + width - 1) : corner.getX();
        int bz = alongX ? corner.getZ() : clamp((int) Math.floor(z), corner.getZ(), corner.getZ() + width - 1);
        int by = clamp((int) Math.floor(y), corner.getY(), corner.getY() + height - 1);
        return new BlockPos(bx, by, bz);
    }

    /**
     * Sets to air every portal block in the box a built portal's rectangle can cover, along
     * either axis, which removes their records too. The frame and platform stay; they hold no
     * records.
     */
    private static void removePortalBlocks(ServerLevel level, BlockUtil.FoundRectangle built) {
        BlockPos min = built.minCorner;
        for (int along = 0; along < built.axis1Size; along++) {
            for (int up = 0; up < built.axis2Size; up++) {
                for (BlockPos block : new BlockPos[] {min.offset(along, up, 0), min.offset(0, up, along)}) {
                    if (level.getBlockState(block).is(net.minecraft.world.level.block.Blocks.NETHER_PORTAL)) {
                        level.setBlock(block, net.minecraft.world.level.block.Blocks.AIR.defaultBlockState(), FLAGS);
                    }
                }
            }
        }
    }

    private static ServerLevel level(MinecraftServer server, JsonObject object) {
        String dimension = object.get("dimension").getAsString();
        ServerLevel level = server.getLevel(ResourceKey.create(Registries.DIMENSION, Identifier.parse(dimension)));
        if (level == null) {
            throw new IllegalArgumentException("no dimension " + dimension);
        }
        return level;
    }

    private static JsonArray array(JsonObject object, String key) {
        return object.has(key) ? object.getAsJsonArray(key) : new JsonArray();
    }

    private static BlockPos pos(JsonArray array) {
        return new BlockPos(array.get(0).getAsInt(), array.get(1).getAsInt(), array.get(2).getAsInt());
    }

    private static JsonArray ints(BlockPos pos) {
        var array = new JsonArray();
        array.add(pos.getX());
        array.add(pos.getY());
        array.add(pos.getZ());
        return array;
    }

    private static int clamp(int value, int min, int max) {
        return Math.max(min, Math.min(max, value));
    }

    private static void report(String format, Object... args) {
        System.out.println("spyglass portals: " + String.format(format, args));
        System.out.flush();
    }
}
