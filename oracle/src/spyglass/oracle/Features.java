package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.function.Supplier;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;
import net.minecraft.world.level.levelgen.placement.RarityFilter;

/**
 * Feature golden files: for one seed, each chunk decorated on its own, and every call the
 * decoration makes to a target feature, with where and whether it placed. See
 * {@code docs/specs/oracle.md#feature-golden-files}.
 */
final class Features {
    /**
     * The placed features whose features' calls are recorded: dungeons, geodes, the desert well,
     * fossils and lava lakes. Every call to those features is recorded, under the placed feature
     * running at the time.
     */
    static final Set<String> TARGETS = Set.of(
            "minecraft:monster_room", "minecraft:monster_room_deep", "minecraft:amethyst_geode",
            "minecraft:desert_well", "minecraft:fossil_upper", "minecraft:fossil_lower",
            "minecraft:lake_lava_underground", "minecraft:lake_lava_surface");

    /**
     * The last decoration step decorated: {@code surface_structures}, the last with a target. A
     * later step can't change what earlier ones did in the chunk, so decoration stops there.
     */
    static final int LAST_STEP = 4;

    /** The square of chunks decorated around the origin: its minimum chunk and side. */
    static final int MIN_CHUNK = -8;
    static final int SIDE = 16;

    /**
     * Placed features too rare for the square, with how many chunks to find for each: chunks
     * near the origin where its rarity filter passes and the biome at the chunk's middle lists it.
     */
    static final Map<String, Integer> RARE = Map.of(
            "minecraft:desert_well", 8, "minecraft:fossil_upper", 4, "minecraft:fossil_lower", 4);

    /** How far from the origin, in chunks, rare features are looked for. */
    static final int RARE_RADIUS = 256;

    /** A call to a target feature: the placed feature running, the origin, and the result. */
    record Try(String feature, int x, int y, int z, boolean placed) {}

    private static final ThreadLocal<List<Try>> TRIES = new ThreadLocal<>();
    private static final ThreadLocal<Supplier<String>> GENERATING = new ThreadLocal<>();
    /** The placed features of the steps decorated. */
    private static final ThreadLocal<Set<String>> STEPS = new ThreadLocal<>();

    /** Thrown to stop decorating at the first feature past {@link #LAST_STEP}. */
    private static final class Stop extends RuntimeException {
        private static final long serialVersionUID = 1L;

        Stop() {
            // With a stack trace: the crash report that wraps it reads one.
            super("past the last step decorated");
        }
    }

    private Features() {}

    /** What the region is generating now, from {@code WorldGenLevel.setCurrentlyGenerating}. */
    static void generating(Supplier<String> what) {
        GENERATING.set(what);
        Set<String> steps = STEPS.get();
        if (what != null && steps != null && !steps.contains(placedId(what.get()))) {
            throw new Stop();
        }
    }

    /** Whether {@code e}, or what caused it, is a {@link Stop}. */
    private static boolean stopped(Throwable e) {
        for (Throwable t = e; t != null; t = t.getCause()) {
            if (t instanceof Stop) {
                return true;
            }
        }
        return false;
    }

    /** A target feature was called at {@code pos}, and placed or not. */
    static void record(BlockPos pos, boolean placed) {
        List<Try> tries = TRIES.get();
        if (tries == null) {
            return;
        }
        Supplier<String> what = GENERATING.get();
        tries.add(new Try(placedId(what == null ? "" : what.get()), pos.getX(), pos.getY(), pos.getZ(), placed));
    }

    /**
     * The placed feature's id from the region's description of it, a {@code ResourceKey}'s
     * {@code ResourceKey[minecraft:worldgen/placed_feature / minecraft:monster_room]}; the text
     * itself if it isn't one.
     */
    static String placedId(String text) {
        int slash = text.lastIndexOf(" / ");
        if (text.startsWith("ResourceKey[") && slash >= 0 && text.endsWith("]")) {
            return text.substring(slash + 3, text.length() - 1);
        }
        return text;
    }

    static int run(Game game, Path outDir, String dimension, List<String> seeds, int threads) throws Exception {
        Files.createDirectories(outDir);
        GameApi.get().recordFeatures(game, TARGETS);
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
            throws InterruptedException, ReflectiveOperationException {
        LevelStem stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        var api = GameApi.get();
        var type = stem.type().value();
        LevelHeightAccessor height = LevelHeightAccessor.create(type.minY(), type.height());
        RandomState random = api.randomState(game, generator, height, seed);

        var root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("seed", seed);
        root.addProperty("dimension", dimension);
        JsonArray order = order(game, generator);
        root.add("order", order);
        Set<String> early = new java.util.HashSet<>();
        for (int step = 0; step <= LAST_STEP && step < order.size(); step++) {
            for (var id : order.get(step).getAsJsonArray()) {
                if (!id.isJsonNull()) {
                    early.add(id.getAsString());
                }
            }
        }

        var futures = new ArrayList<Future<JsonObject>>();
        for (ChunkPos pos : chunks(game, generator, random, seed, order)) {
            futures.add(pool.submit(() -> {
                var tries = new ArrayList<Try>();
                TRIES.set(tries);
                STEPS.set(early);
                try {
                    api.decorate(game, generator, height, random, seed, pos);
                } catch (RuntimeException e) {
                    if (!stopped(e)) {
                        throw e;
                    }
                } finally {
                    TRIES.remove();
                    GENERATING.remove();
                    STEPS.remove();
                }
                var o = new JsonObject();
                o.add("chunk", ints(pos.x(), pos.z()));
                var list = new JsonArray();
                for (Try t : tries) {
                    var j = new JsonObject();
                    j.addProperty("feature", t.feature());
                    j.add("pos", ints(t.x(), t.y(), t.z()));
                    j.addProperty("placed", t.placed());
                    list.add(j);
                }
                o.add("tries", list);
                return o;
            }));
        }
        var chunks = new JsonArray();
        for (Future<JsonObject> f : futures) {
            try {
                chunks.add(f.get());
            } catch (ExecutionException e) {
                throw new RuntimeException(e.getCause());
            }
        }
        root.add("chunks", chunks);
        return root;
    }

    /**
     * The chunks decorated: none where no target feature runs (the Nether and the End), else the
     * square around the origin, then the chunks found for each rare feature.
     */
    private static Set<ChunkPos> chunks(Game game, NoiseBasedChunkGenerator generator, RandomState random,
            long seed, JsonArray order) {
        Set<ChunkPos> chunks = new LinkedHashSet<>();
        boolean any = false;
        for (var step : order) {
            for (var id : step.getAsJsonArray()) {
                any |= !id.isJsonNull() && TARGETS.contains(id.getAsString());
            }
        }
        if (!any) {
            return chunks;
        }
        for (int z = MIN_CHUNK; z < MIN_CHUNK + SIDE; z++) {
            for (int x = MIN_CHUNK; x < MIN_CHUNK + SIDE; x++) {
                chunks.add(new ChunkPos(x, z));
            }
        }
        var registry = game.access.lookupOrThrow(Registries.PLACED_FEATURE);
        for (int step = 0; step < order.size(); step++) {
            var ids = order.get(step).getAsJsonArray();
            for (int index = 0; index < ids.size(); index++) {
                if (ids.get(index).isJsonNull() || !RARE.containsKey(ids.get(index).getAsString())) {
                    continue;
                }
                String id = ids.get(index).getAsString();
                var placed = registry.getValue(net.minecraft.resources.Identifier.parse(id));
                if (placed == null || placed.placement().isEmpty()
                        || !(placed.placement().get(0) instanceof RarityFilter rarity)) {
                    continue;
                }
                int found = 0;
                // Rings of chunks outwards from the origin.
                for (int r = 0; r <= RARE_RADIUS && found < RARE.get(id); r++) {
                    for (int z = -r; z <= r && found < RARE.get(id); z++) {
                        for (int x = -r; x <= r && found < RARE.get(id); x++) {
                            if (Math.max(Math.abs(x), Math.abs(z)) != r) {
                                continue;
                            }
                            var rng = new WorldgenRandom(new XoroshiroRandomSource(0L));
                            long decoration = rng.setDecorationSeed(seed, x << 4, z << 4);
                            rng.setFeatureSeed(decoration, index, step);
                            if (!(rng.nextFloat() < 1.0f / rarity.chance())) {
                                continue;
                            }
                            // The biome at quart (2, 16, 2) of the chunk: y 64, near its middle.
                            int qx = (x << 2) + 2;
                            int qz = (z << 2) + 2;
                            try (var biomes = GameApi.get().chunkBiomes(random, generator.getBiomeSource(), qx, 16,
                                    qz, 1, 1, false)) {
                                if (biomes.biome(qx, 16, qz).value().getGenerationSettings().hasFeature(placed)
                                        && chunks.add(new ChunkPos(x, z))) {
                                    found++;
                                }
                            }
                        }
                    }
                }
            }
        }
        return chunks;
    }

    /**
     * The generator's {@code featuresPerStep}: each step's placed features in decoration order, by
     * id, or {@code null} for one written inline in a biome.
     */
    private static JsonArray order(Game game, NoiseBasedChunkGenerator generator)
            throws ReflectiveOperationException {
        Field field = ChunkGenerator.class.getDeclaredField("featuresPerStep");
        field.setAccessible(true);
        @SuppressWarnings("unchecked")
        var steps = ((Supplier<List<FeatureSorter.StepFeatureData>>) field.get(generator)).get();
        var registry = game.access.lookupOrThrow(Registries.PLACED_FEATURE);
        var order = new JsonArray();
        for (FeatureSorter.StepFeatureData step : steps) {
            var list = new JsonArray();
            for (PlacedFeature placed : step.features()) {
                var key = registry.getKey(placed);
                list.add(key == null ? null : key.toString());
            }
            order.add(list);
        }
        return order;
    }

    private static JsonArray ints(int... values) {
        var arr = new JsonArray();
        for (int v : values) {
            arr.add(v);
        }
        return arr;
    }

    private static void write(Path path, JsonObject root) throws IOException {
        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            writer.setSerializeNulls(true);
            new Gson().toJson(root, writer);
        }
        Files.writeString(path, text + "\n");
    }
}
