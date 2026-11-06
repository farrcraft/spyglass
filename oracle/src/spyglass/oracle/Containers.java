package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.function.Supplier;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;
import com.mojang.serialization.JsonOps;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.world.Container;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.RandomizableContainer;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.storage.loot.LootParams;
import net.minecraft.world.level.storage.loot.parameters.LootContextParamSets;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.phys.Vec3;

/**
 * Container golden files: for one seed, the nearest start of each structure to the origin, and
 * every chunk those starts reach decorated on its own with structures, recording each container
 * with a loot table that decoration made and what made it. See
 * {@code docs/specs/oracle.md#container-golden-files}.
 *
 * <p>Starts and references come from the generator's own {@code createStructures} and
 * {@code createReferences}, on fresh proto chunks, through a real {@code StructureManager} whose
 * level is a stand-in handing out those chunks. Decoration then runs as for feature golden files,
 * with that manager.
 *
 * <p>Each new container is filled at once from its table and seed by the game's own
 * {@code LootTable.fill}, at its block, with the region's biomes and no structure for any map,
 * as loot golden files are ({@link Loot}).
 */
final class Containers {
    /** How far from the origin, in chunks on each axis, starts are looked for. */
    static final int RADIUS = 64;

    /** How far a start reaches from its chunk (`ChunkGenerator.createReferences`). */
    static final int REACH = 8;

    /**
     * Structure types whose piece generators keep state in static fields: a fortress's piece
     * weights, an end city's ship. Their starts race when made at once on several threads.
     */
    private static final Set<String> STATIC_STATE = Set.of("minecraft:fortress", "minecraft:end_city");

    /**
     * A container decoration made: what made it, where, the block or entity, the table and seed,
     * its slots, and what the table fills them with (null until filled, or where the region can't
     * give its biomes).
     */
    record Made(String by, BlockPos pos, String block, String entity, String table, long seed, int slots,
            JsonArray items) {
        Made filled(JsonArray items) {
            return new Made(by, pos, block, entity, table, seed, slots, items);
        }
    }

    /** The server's loot tables, and the ops that encode the stacks they make. */
    private static ReloadableServerRegistries.Holder loot;
    private static RegistryOps<JsonElement> ops;

    /** {@code LevelReader.getBiome}: of a {@code BlockPos} to 26.3, of x, y and z from 26.4. */
    private static final Method GET_BIOME = getBiome();

    /** What one chunk's decoration is recording: the label generating, and the containers so far. */
    private static final class Recording {
        final ChunkPos center;
        /** The level fills run in. */
        final Loot.Level level;
        String by = "";
        Map<String, Made> seen = new HashMap<>();
        int entities;
        final List<Made> made = new ArrayList<>();

        Recording(ChunkPos center, Loot.Level level) {
            this.center = center;
            this.level = level;
        }
    }

    private static final ThreadLocal<Recording> RECORDING = new ThreadLocal<>();

    private Containers() {}

    static int run(Game game, Path outDir, String dimension, List<String> seeds, int threads) throws Exception {
        Files.createDirectories(outDir);
        // Decoration's stand-ins, with each thread's structure templates; and the items' components
        // bound, for the mobs structures spawn with their equipment.
        GameApi.get().recordFeatures(game, Features.TARGETS);
        loot = Loot.holder(game);
        ops = RegistryOps.create(JsonOps.INSTANCE, loot.lookup());
        try (var pool = Executors.newFixedThreadPool(Math.max(1, threads))) {
            for (String text : seeds) {
                long t0 = System.nanoTime();
                long seed = WorldOptions.parseSeed(text).orElseThrow();
                Path out = outDir.resolve(text + ".json");
                write(out, seed(game, dimension, seed, pool));
                System.out.printf("%s: %.1f s%n", out, (System.nanoTime() - t0) / 1e9);
            }
        }
        return 0;
    }

    private static JsonObject seed(Game game, String dimension, long seed, ExecutorService pool)
            throws Exception {
        LevelStem stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        var api = GameApi.get();
        var type = stem.type().value();
        LevelHeightAccessor height = LevelHeightAccessor.create(type.minY(), type.height());
        RandomState random = api.randomState(game, generator, height, seed);
        ChunkGeneratorStructureState state =
                generator.createState(game.access.lookupOrThrow(Registries.STRUCTURE_SET), random, seed);
        state.ensureStructuresGenerated();
        ResourceKey<Level> level = ResourceKey.create(Registries.DIMENSION,
                net.minecraft.resources.Identifier.parse(dimension));
        var world = new StartWorld(game, generator, state, level, height);

        // Every chunk's starts within the radius, then the nearest start of each structure whose
        // reach stays inside what's been made.
        var futures = new ArrayList<Future<?>>();
        for (int z = -RADIUS - REACH; z <= RADIUS + REACH; z++) {
            for (int x = -RADIUS - REACH; x <= RADIUS + REACH; x++) {
                ChunkPos pos = new ChunkPos(x, z);
                futures.add(pool.submit(() -> world.chunk(pos)));
            }
        }
        join(futures);
        var registry = game.access.lookupOrThrow(Registries.STRUCTURE);
        Map<String, StructureStart> nearest = new java.util.TreeMap<>();
        Map<String, Long> distance = new HashMap<>();
        for (int z = -RADIUS; z <= RADIUS; z++) {
            for (int x = -RADIUS; x <= RADIUS; x++) {
                for (var entry : world.chunk(new ChunkPos(x, z)).getAllStarts().entrySet()) {
                    StructureStart start = entry.getValue();
                    if (start.getPieces().isEmpty() || !inside(start.getBoundingBox())) {
                        continue;
                    }
                    String id = registry.getKey(entry.getKey()).toString();
                    long d = (long) x * x + (long) z * z;
                    if (d < distance.getOrDefault(id, Long.MAX_VALUE)) {
                        distance.put(id, d);
                        nearest.put(id, start);
                    }
                }
            }
        }

        // The chunks those starts reach, each decorated on its own.
        var chunks = new TreeSet<ChunkPos>(Comparator.comparingInt(ChunkPos::z).thenComparingInt(ChunkPos::x));
        for (StructureStart start : nearest.values()) {
            BoundingBox box = start.getBoundingBox();
            for (int z = box.minZ() >> 4; z <= box.maxZ() >> 4; z++) {
                for (int x = box.minX() >> 4; x <= box.maxX() >> 4; x++) {
                    chunks.add(new ChunkPos(x, z));
                }
            }
        }
        for (ChunkPos pos : chunks) {
            world.references(pos);
        }
        var decorated = new ArrayList<Future<JsonObject>>();
        for (ChunkPos pos : chunks) {
            decorated.add(pool.submit(() -> {
                var recording = new Recording(pos, Loot.level(game, loot));
                RECORDING.set(recording);
                try {
                    api.decorate(game, generator, height, random, seed, pos, false, world.managerFor(pos));
                } finally {
                    RECORDING.remove();
                }
                var o = new JsonObject();
                o.add("chunk", ints(pos.x(), pos.z()));
                var list = new JsonArray();
                for (Made c : recording.made) {
                    list.add(json(c));
                }
                o.add("containers", list);
                return o;
            }));
        }

        var root = new JsonObject();
        root.addProperty("format", 2);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("seed", seed);
        root.addProperty("dimension", dimension);
        var starts = new JsonArray();
        for (var entry : nearest.entrySet()) {
            StructureStart start = entry.getValue();
            var o = new JsonObject();
            o.addProperty("structure", entry.getKey());
            o.add("chunk", ints(start.getChunkPos().x(), start.getChunkPos().z()));
            o.add("bounds", box(start.getBoundingBox()));
            var pieces = new JsonArray();
            for (var piece : start.getPieces()) {
                pieces.add(box(piece.getBoundingBox()));
            }
            o.add("pieces", pieces);
            starts.add(o);
        }
        root.add("starts", starts);
        var out = new JsonArray();
        for (Future<JsonObject> f : decorated) {
            try {
                out.add(f.get());
            } catch (ExecutionException e) {
                throw new RuntimeException(e.getCause());
            }
        }
        root.add("chunks", out);
        return root;
    }

    /**
     * Whether every chunk a start with this box decorates is within the radius, so the starts of
     * every chunk within reach of it are made.
     */
    private static boolean inside(BoundingBox box) {
        return (box.minX() >> 4) >= -RADIUS && (box.maxX() >> 4) <= RADIUS && (box.minZ() >> 4) >= -RADIUS
                && (box.maxZ() >> 4) <= RADIUS;
    }

    private static void join(List<Future<?>> futures) throws InterruptedException {
        for (Future<?> f : futures) {
            try {
                f.get();
            } catch (ExecutionException e) {
                throw new RuntimeException(e.getCause());
            }
        }
    }

    /**
     * The region is now generating {@code what}: the containers made since the last call are the
     * previous one's. Called by the decoration region's {@code setCurrentlyGenerating}.
     */
    static void generating(WorldGenLevel region, Supplier<String> what) {
        Recording recording = RECORDING.get();
        if (recording == null) {
            return;
        }
        collect(region, recording);
        recording.by = what == null ? "" : Features.placedId(what.get());
    }

    /** Decoration of the chunk is done: the containers made since the last label are its. */
    static void finish(WorldGenLevel region) {
        Recording recording = RECORDING.get();
        if (recording != null) {
            collect(region, recording);
            recording.by = "";
        }
    }

    /**
     * Adds the containers with a loot table that are new or changed since the last look, in the
     * region's 3 × 3 chunks, to the recording under its current label: block entities by
     * position, then the center chunk's entities (chest minecarts) added since.
     */
    private static void collect(WorldGenLevel region, Recording recording) {
        Map<String, Made> now = new HashMap<>();
        var fresh = new ArrayList<Made>();
        ChunkPos center = recording.center;
        for (int dz = -1; dz <= 1; dz++) {
            for (int dx = -1; dx <= 1; dx++) {
                ChunkAccess chunk = region.getChunk(center.x() + dx, center.z() + dz);
                for (BlockPos pos : chunk.getBlockEntitiesPos()) {
                    if (chunk.getBlockEntity(pos) instanceof RandomizableContainer container
                            && container.getLootTable() != null) {
                        var c = new Made(recording.by, pos.immutable(),
                                BuiltInRegistries.BLOCK
                                        .getKey(chunk.getBlockState(pos).getBlock()).toString(),
                                null, container.getLootTable().identifier().toString(),
                                container.getLootTableSeed(), container.getContainerSize(), null);
                        String key = pos.toShortString();
                        now.put(key, c);
                        Made before = recording.seen.get(key);
                        if (before == null || !before.table().equals(c.table()) || before.seed() != c.seed()) {
                            fresh.add(c);
                        }
                    }
                }
            }
        }
        recording.seen = now;
        fresh.sort(Comparator.comparing(Made::pos));
        for (Made c : fresh) {
            recording.made.add(c.filled(fill(region, recording.level, c.table(), c.seed(), Vec3.atCenterOf(c.pos()),
                    c.slots())));
        }
        if (region.getChunk(center.x(), center.z()) instanceof ProtoChunk proto) {
            List<CompoundTag> entities = proto.getEntities();
            for (int i = recording.entities; i < entities.size(); i++) {
                CompoundTag tag = entities.get(i);
                String table = tag.getStringOr("LootTable", "");
                if (table.isEmpty()) {
                    continue;
                }
                var position = tag.getListOrEmpty("Pos");
                var origin = new Vec3(position.getDoubleOr(0, 0), position.getDoubleOr(1, 0),
                        position.getDoubleOr(2, 0));
                String id = tag.getStringOr("id", "");
                long seed = tag.getLongOr("LootTableSeed", 0);
                int slots = entitySlots(region, id);
                recording.made.add(new Made(recording.by, BlockPos.containing(origin), null, id, table, seed, slots,
                        fill(region, recording.level, table, seed, origin, slots)));
            }
            recording.entities = entities.size();
        }
    }

    /**
     * The stacks table {@code table} fills a container of {@code slots} slots with, from loot seed
     * {@code seed}, as {@code RandomizableContainer.unpackLootTable} fills it from {@code origin}
     * (an entity's {@code ContainerEntity.unpackChestVehicleLootTable} likewise): each encoded
     * by {@code ItemStack.CODEC}, with its slot. Biomes are the region's, so null where a block
     * the table asks about is outside it.
     */
    private static JsonArray fill(WorldGenLevel region, Loot.Level level, String table, long seed, Vec3 origin,
            int slots) {
        var key = ResourceKey.create(Registries.LOOT_TABLE, Identifier.parse(table));
        level.biomes = pos -> biomeAt(region, pos);
        var container = new SimpleContainer(slots);
        try {
            LootParams params = new LootParams.Builder(level).withParameter(LootContextParams.ORIGIN, origin)
                    .create(LootContextParamSets.CHEST);
            loot.getLootTable(key).fill(container, params, seed);
        } catch (OutsideRegion e) {
            return null;
        }
        var items = new JsonArray();
        for (int slot = 0; slot < container.getContainerSize(); slot++) {
            ItemStack stack = container.getItem(slot);
            if (stack.isEmpty()) {
                continue;
            }
            JsonObject encoded = ItemStack.CODEC.encodeStart(ops, stack).getOrThrow().getAsJsonObject();
            encoded.addProperty("slot", slot);
            items.add(encoded);
        }
        return items;
    }

    /** A fill asked for the biome of a block the region can't give. */
    private static final class OutsideRegion extends RuntimeException {
        private static final long serialVersionUID = 1L;

        OutsideRegion(Throwable cause) {
            super(cause);
        }
    }

    /** The region's biome at {@code pos}, as the level's {@code getBiome} gives it: the zoomed one. */
    private static Holder<Biome> biomeAt(WorldGenLevel region, BlockPos pos) {
        try {
            Object biome = GET_BIOME.getParameterCount() == 1 ? GET_BIOME.invoke(region, pos)
                    : GET_BIOME.invoke(region, pos.getX(), pos.getY(), pos.getZ());
            @SuppressWarnings("unchecked")
            var holder = (Holder<Biome>) biome;
            return holder;
        } catch (InvocationTargetException e) {
            throw new OutsideRegion(e.getCause());
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    private static Method getBiome() {
        try {
            return LevelReader.class.getMethod("getBiome", BlockPos.class);
        } catch (NoSuchMethodException e) {
            try {
                return LevelReader.class.getMethod("getBiome", int.class, int.class, int.class);
            } catch (NoSuchMethodException none) {
                throw new IllegalStateException("LevelReader has no getBiome", none);
            }
        }
    }

    /**
     * The slots of the container entity {@code id}: {@code getContainerSize()} of one made on the
     * region's level, as structures make their mobs.
     */
    private static int entitySlots(WorldGenLevel region, String id) {
        Object entity = BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(id)).create(region.getLevel(),
                EntitySpawnReason.STRUCTURE);
        if (!(entity instanceof Container container)) {
            throw new IllegalStateException(id + " with a loot table isn't a container");
        }
        return container.getContainerSize();
    }

    /**
     * Chunks with structure starts and references, made as the {@code STRUCTURE_STARTS} and
     * {@code STRUCTURE_REFERENCES} steps make them, and the structure manager decoration reads
     * them through.
     */
    static final class StartWorld {
        private final Game game;
        private final NoiseBasedChunkGenerator generator;
        private final ChunkGeneratorStructureState state;
        private final ResourceKey<Level> level;
        private final LevelHeightAccessor height;
        private final PalettedContainerFactory containers;
        private final Map<ChunkPos, ProtoChunk> chunks = new ConcurrentHashMap<>();
        /** Whether starts are made one at a time: a structure with static state can generate. */
        private final boolean oneAtATime;
        private final WorldGenLevel view;
        private final StructureManager manager;

        StartWorld(Game game, NoiseBasedChunkGenerator generator, ChunkGeneratorStructureState state,
                ResourceKey<Level> level, LevelHeightAccessor height)
                throws ReflectiveOperationException {
            this.game = game;
            this.generator = generator;
            this.state = state;
            this.level = level;
            this.height = height;
            this.containers = PalettedContainerFactory.create(game.access);
            this.oneAtATime = state.possibleStructureSets().stream()
                    .flatMap(set -> set.value().structures().stream())
                    .map(entry -> BuiltInRegistries.STRUCTURE_TYPE.getKey(entry.structure().value().type()))
                    .anyMatch(type -> STATIC_STATE.contains(type.toString()));
            this.view = view(this::existing);
            this.manager = manager(view);
        }

        /** A structure manager reading through {@code view}. */
        private static StructureManager manager(WorldGenLevel view) throws ReflectiveOperationException {
            StructureManager manager = StandIns.allocate(Manager.class);
            Field field = StructureManager.class.getDeclaredField("level");
            field.setAccessible(true);
            field.set(manager, view);
            return manager;
        }

        /** The chunk at {@code pos} with its starts, made the first time it's wanted. */
        ProtoChunk chunk(ChunkPos pos) {
            return chunks.computeIfAbsent(pos, this::fresh);
        }

        /**
         * A new proto chunk at {@code pos} with its starts: one at a time where a structure with
         * static state could be among them.
         */
        private ProtoChunk fresh(ChunkPos pos) {
            var chunk = new ProtoChunk(pos, UpgradeData.EMPTY, height, containers, null);
            if (oneAtATime) {
                synchronized (StartWorld.class) {
                    createStructures(chunk);
                }
            } else {
                createStructures(chunk);
            }
            return chunk;
        }

        private void createStructures(ProtoChunk chunk) {
            GameApi.get().createStructures(game, generator, state, manager, chunk, Decoration.templates(), level);
        }

        /** The chunk at {@code pos}, whose starts must have been made. */
        private ProtoChunk existing(ChunkPos pos) {
            ProtoChunk chunk = chunks.get(pos);
            if (chunk == null) {
                throw new IllegalStateException("no starts made for chunk " + pos);
            }
            return chunk;
        }

        /**
         * A structure manager for decorating the chunk at {@code pos} on its own: its references
         * as made, and starts made again for it alone. Some pieces settle their state in the
         * first chunk that places them (an ocean ruin its height), so each chunk sees its starts
         * as made, as if it were the first. Only for one thread.
         */
        StructureManager managerFor(ChunkPos pos) throws ReflectiveOperationException {
            Map<ChunkPos, ProtoChunk> own = new HashMap<>();
            return manager(view(p -> own.computeIfAbsent(p, this::fresh)));
        }

        /** {@code createReferences} for the chunk at {@code pos}, whose neighbours' starts are made. */
        void references(ChunkPos pos) {
            for (int z = pos.z() - REACH; z <= pos.z() + REACH; z++) {
                for (int x = pos.x() - REACH; x <= pos.x() + REACH; x++) {
                    chunk(new ChunkPos(x, z));
                }
            }
            generator.createReferences(view, manager, chunk(pos));
        }

        /**
         * A level that only hands out these chunks, their heights and the registries; anything
         * else fails, so a use that needs more is noticed. A chunk asked for at
         * {@code STRUCTURE_STARTS} comes from {@code starts}, any other from these chunks.
         */
        private WorldGenLevel view(java.util.function.Function<ChunkPos, ProtoChunk> starts) {
            InvocationHandler handler = (proxy, method, args) -> {
                switch (method.getName()) {
                    case "getChunk":
                        if (args.length >= 2 && args[0] instanceof Integer x && args[1] instanceof Integer z) {
                            ChunkPos pos = new ChunkPos(x, z);
                            boolean forStarts = args.length >= 3 && args[2] == ChunkStatus.STRUCTURE_STARTS;
                            return forStarts ? starts.apply(pos) : existing(pos);
                        }
                        break;
                    case "registryAccess":
                        return game.access;
                    case "getMinY":
                        return height.getMinY();
                    case "getHeight":
                        if (args == null) {
                            return height.getHeight();
                        }
                        break;
                    default:
                        if (method.isDefault()) {
                            return InvocationHandler.invokeDefault(proxy, method, args);
                        }
                }
                throw new UnsupportedOperationException("the start level has no " + method);
            };
            return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class}, handler);
        }
    }

    /** A structure manager that generates structures, its level set by {@link StartWorld}. */
    static class Manager extends StructureManager {
        /** Never run: instances are made by {@link StandIns#allocate}. */
        Manager() {
            super(null, null, null);
        }

        @Override
        public boolean shouldGenerateStructures() {
            return true;
        }
    }

    private static JsonObject json(Made c) {
        var o = new JsonObject();
        o.addProperty("by", c.by());
        o.add("pos", ints(c.pos().getX(), c.pos().getY(), c.pos().getZ()));
        if (c.block() != null) {
            o.addProperty("block", c.block());
        }
        if (c.entity() != null) {
            o.addProperty("entity", c.entity());
        }
        o.addProperty("table", c.table());
        o.addProperty("seed", c.seed());
        o.addProperty("slots", c.slots());
        if (c.items() != null) {
            o.add("items", c.items());
        }
        return o;
    }

    private static JsonArray box(BoundingBox box) {
        return ints(box.minX(), box.minY(), box.minZ(), box.maxX(), box.maxY(), box.maxZ());
    }

    private static JsonArray ints(int... values) {
        var array = new JsonArray();
        for (int v : values) {
            array.add(v);
        }
        return array;
    }

    private static void write(Path path, JsonObject root) throws IOException {
        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            new Gson().toJson(root, writer);
        }
        Files.writeString(path, text + "\n");
    }
}
