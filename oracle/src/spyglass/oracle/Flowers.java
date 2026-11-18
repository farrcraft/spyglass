package spyglass.oracle;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.QuartPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;

/**
 * What bone meal grows on grass, from the game: each bone meal feature's provider at fixed
 * positions with fixed random seeds ({@code providers.json}), and for each seed, the biome
 * {@code ServerLevel.getBiome} gives at the blocks of fixed and found patches ({@code
 * <seed>.json}), with each biome's bone meal features. See {@code docs/specs/flowers.md} and
 * {@code docs/specs/oracle.md#flower-files}.
 */
final class Flowers {
    /** Bump when a flower file's content changes. */
    static final int FORMAT = 1;

    /** The heights providers are sampled at, and the fixed patches' heights. */
    private static final int[] HEIGHTS = {-60, -31, 0, 30, 63, 64, 90, 141, 200, 300};

    /** The random sources' seeds each provider is drawn with. */
    private static final long[] RANDOM_SEEDS = {0, 1, 2, 3, 42, -7, 123456789, Long.MIN_VALUE};

    /** How many positions each provider is sampled at. */
    private static final int PROVIDER_POSITIONS = 400;

    /** The scan for patches: every this many chunks, out to this many from the origin. */
    private static final int SCAN_STEP = 24;
    private static final int SCAN_RADIUS = 480;

    /** At most this many found patches per biome with bone meal features. */
    private static final int PATCHES_PER_BIOME = 4;

    private Flowers() {}

    static int run(Game game, Path out, String dimension, List<String> seeds, int threads) throws Exception {
        Files.createDirectories(out);
        LevelStem stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        var api = GameApi.get();
        // Every bone meal feature any biome of the dimension can pick, by id.
        Map<String, Object> features = new LinkedHashMap<>();
        Map<Holder<Biome>, List<GameApi.BoneMealFeature>> byBiome = new HashMap<>();
        for (Holder<Biome> biome : generator.getBiomeSource().possibleBiomes()) {
            var list = api.boneMealFeatures(game, biome);
            byBiome.put(biome, list);
            for (var f : list) {
                features.putIfAbsent(f.id(), f.feature());
            }
        }
        write(out.resolve("providers.json"), providers(game, features, dimension));
        System.out.printf("%s: %d bone meal features%n", out.resolve("providers.json"), features.size());

        try (ExecutorService pool = Executors.newFixedThreadPool(Math.max(1, Math.min(threads, seeds.size())))) {
            List<Future<?>> futures = new ArrayList<>();
            for (String text : seeds) {
                futures.add(pool.submit(() -> {
                    long seed = WorldOptions.parseSeed(text).orElseThrow();
                    long t0 = System.nanoTime();
                    Path file = out.resolve(text + ".json");
                    var root = seedFile(game, stem, generator, byBiome, dimension, seed);
                    write(file, root);
                    System.out.printf("%s: %d patches, %.1f s%n", file, root.getAsJsonArray("patches").size(),
                            (System.nanoTime() - t0) / 1e9);
                    return null;
                }));
            }
            for (Future<?> f : futures) {
                f.get();
            }
        }
        return 0;
    }

    private static JsonObject header(Game game, String dimension) {
        var root = new JsonObject();
        root.addProperty("format", FORMAT);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("dimension", dimension);
        return root;
    }

    /**
     * Each feature's provider at fixed positions across the build height, drawn with each of
     * {@link #RANDOM_SEEDS}: the state, and the source's next {@code nextInt()} after it, which
     * shows how many draws the provider took.
     */
    private static JsonObject providers(Game game, Map<String, Object> features, String dimension) {
        var root = header(game, dimension);
        var seeds = new JsonArray();
        for (long s : RANDOM_SEEDS) {
            seeds.add(s);
        }
        root.add("random_seeds", seeds);
        var positions = new ArrayList<BlockPos>();
        var random = new Random(0x5eed_f10e_7L);
        for (int i = 0; i < PROVIDER_POSITIONS; i++) {
            // Far apart, near each other and at the axes, so the noises' whole range shows.
            int spread = i % 4 == 0 ? 64 : i % 4 == 1 ? 4096 : 3_000_000;
            positions.add(new BlockPos(random.nextInt(2 * spread + 1) - spread, HEIGHTS[i % HEIGHTS.length],
                    random.nextInt(2 * spread + 1) - spread));
        }
        var list = new JsonArray();
        for (var entry : features.entrySet()) {
            var feature = new JsonObject();
            feature.addProperty("feature", entry.getKey());
            var cases = new JsonArray();
            for (BlockPos pos : positions) {
                var states = new JsonArray();
                var next = new JsonArray();
                boolean simple = true;
                for (long s : RANDOM_SEEDS) {
                    var source = new LegacyRandomSource(s);
                    var state = GameApi.get().boneMealState(entry.getValue(), source, pos);
                    if (state == null) {
                        simple = false;
                        break;
                    }
                    states.add(Blocks.name(state));
                    next.add(source.nextInt());
                }
                if (!simple) {
                    break;
                }
                var c = new JsonObject();
                c.add("pos", ints(pos.getX(), pos.getY(), pos.getZ()));
                c.add("states", states);
                c.add("next", next);
                cases.add(c);
            }
            feature.add("cases", cases);
            list.add(feature);
        }
        root.add("features", list);
        return root;
    }

    /** A patch: one chunk's blocks at one height. */
    private record Patch(int cx, int cz, int y) {}

    private static JsonObject seedFile(Game game, LevelStem stem, NoiseBasedChunkGenerator generator,
            Map<Holder<Biome>, List<GameApi.BoneMealFeature>> byBiome, String dimension, long seed) {
        int minY = stem.type().value().minY();
        int height = stem.type().value().height();
        int quartMinY = QuartPos.fromBlock(minY);
        int quartHeight = QuartPos.fromBlock(height);
        var api = GameApi.get();
        RandomState random = api.randomState(game, generator, LevelHeightAccessor.create(minY, height), seed);
        BiomeSource source = generator.getBiomeSource();

        var patches = new ArrayList<Patch>();
        for (int y : HEIGHTS) {
            patches.add(new Patch(0, 0, y));
        }
        // Patches where each biome with bone meal features is, found on a coarse scan of whole
        // chunk columns.
        Map<Holder<Biome>, Integer> found = new HashMap<>();
        for (int cz = -SCAN_RADIUS; cz <= SCAN_RADIUS; cz += SCAN_STEP) {
            for (int cx = -SCAN_RADIUS; cx <= SCAN_RADIUS; cx += SCAN_STEP) {
                try (var chunk = api.chunkBiomes(random, source, cx * 4, quartMinY, cz * 4, 4, quartHeight, false)) {
                    var here = new HashMap<Holder<Biome>, Integer>();
                    for (int qy = quartMinY; qy < quartMinY + quartHeight; qy++) {
                        Holder<Biome> b = chunk.biome(cx * 4 + 1, qy, cz * 4 + 1);
                        if (!byBiome.getOrDefault(b, List.of()).isEmpty()) {
                            here.putIfAbsent(b, qy);
                        }
                    }
                    for (var e : here.entrySet()) {
                        int n = found.getOrDefault(e.getKey(), 0);
                        if (n < PATCHES_PER_BIOME) {
                            found.put(e.getKey(), n + 1);
                            patches.add(new Patch(cx, cz, e.getValue() * 4 + 1));
                        }
                    }
                }
            }
        }

        var root = header(game, dimension);
        root.addProperty("seed", seed);
        var biomeFeatures = new JsonObject();
        var palette = new LinkedHashMap<Holder<Biome>, Integer>();
        var list = new JsonArray();
        for (Patch patch : patches) {
            int qy = QuartPos.fromBlock(patch.y);
            int lo = Math.clamp(qy - 1, quartMinY, quartMinY + quartHeight - 1);
            int hi = Math.clamp(qy + 1, quartMinY, quartMinY + quartHeight - 1);
            int qx0 = patch.cx * 4 - 1;
            int qz0 = patch.cz * 4 - 1;
            var cells = new JsonArray();
            try (var window = api.chunkBiomes(random, source, qx0, lo, qz0, 6, hi - lo + 1, false)) {
                // The level's chunks clamp a quart's y to the level (ChunkAccess.getNoiseBiome).
                GameApi.QuartLookup quarts = (x, y, z) -> window.biome(x,
                        Math.clamp(y, quartMinY, quartMinY + quartHeight - 1), z);
                for (int z = patch.cz * 16; z < patch.cz * 16 + 16; z++) {
                    for (int x = patch.cx * 16; x < patch.cx * 16 + 16; x++) {
                        Holder<Biome> b = api.zoomedBiome(seed, quarts, x, patch.y, z);
                        Integer i = palette.get(b);
                        if (i == null) {
                            i = palette.size();
                            palette.put(b, i);
                        }
                        cells.add(i);
                    }
                }
            }
            var p = new JsonObject();
            p.add("chunk", ints(patch.cx, patch.cz));
            p.addProperty("y", patch.y);
            p.add("biomes", cells);
            list.add(p);
        }
        var names = new JsonArray();
        for (Holder<Biome> b : palette.keySet()) {
            String name = b.unwrapKey().orElseThrow().identifier().toString();
            names.add(name);
            var ids = new JsonArray();
            for (var f : byBiome.getOrDefault(b, List.of())) {
                ids.add(f.id());
            }
            biomeFeatures.add(name, ids);
        }
        root.add("biome_palette", names);
        root.add("bone_meal_features", biomeFeatures);
        root.add("patches", list);
        return root;
    }

    private static void write(Path file, JsonObject root) throws java.io.IOException {
        Files.writeString(file, new Gson().toJson(root) + "\n");
    }

    private static JsonArray ints(int... values) {
        var array = new JsonArray();
        for (int v : values) {
            array.add(v);
        }
        return array;
    }
}
