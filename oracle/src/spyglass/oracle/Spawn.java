package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseSpawnFinder;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.SpawnTargetPoint;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.densityfunction.DensitySamplerSet;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;

/**
 * Spawn golden files: for one seed, the chunk the game's spawn search starts from
 * ({@code ChunkGenerator.getOrigin}), the climate search's result after each of its phases, and
 * the search's fitness at fixed positions. See {@code docs/specs/oracle.md}.
 *
 * <p>The phases replay {@code NoiseSpawnFinder}'s constructor through its own private methods, by
 * reflection: the fitness at (0, 0), then {@code radialSearch} with (2048, 512) and (512, 32).
 * The final spawn block needs a live level ({@code PlayerSpawnFinder}), so it isn't written.
 */
final class Spawn {
    /** Block (x, z) positions the fitness is written at: negatives, quart boundaries, the border. */
    static final int[][] SAMPLES = {
            {0, 0}, {1, 1}, {3, -1}, {-1, -4}, {-5, 7}, {512, 0}, {-517, 1203}, {2048, -2048},
            {12_345, -6_789}, {-100_003, 99_998}, {29_999_984, -29_999_984}, {-30_000_000, 30_000_000}};

    private static final Constructor<NoiseSpawnFinder> CONSTRUCTOR;
    private static final Field RESULT;
    private static final Method RADIAL_SEARCH;
    private static final Method FITNESS;
    private static final Method LOCATION;
    private static final Method RESULT_FITNESS;

    static {
        try {
            CONSTRUCTOR = NoiseSpawnFinder.class.getDeclaredConstructor(List.class, DensitySamplerSet.class);
            RESULT = NoiseSpawnFinder.class.getDeclaredField("result");
            RADIAL_SEARCH = NoiseSpawnFinder.class.getDeclaredMethod("radialSearch", DensitySamplerSet.class,
                    List.class, float.class, float.class);
            FITNESS = NoiseSpawnFinder.class.getDeclaredMethod("getSpawnPositionAndFitness",
                    DensitySamplerSet.class, List.class, int.class, int.class);
            Class<?> result = RESULT.getType();
            LOCATION = result.getDeclaredMethod("location");
            RESULT_FITNESS = result.getDeclaredMethod("fitness");
            for (var m : List.of(CONSTRUCTOR, RESULT, RADIAL_SEARCH, FITNESS, LOCATION, RESULT_FITNESS)) {
                m.setAccessible(true);
            }
        } catch (ReflectiveOperationException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    private Spawn() {}

    static int run(Game game, Path outDir, String dimension, List<String> seeds) throws Exception {
        Files.createDirectories(outDir);
        for (String text : seeds) {
            long seed = WorldOptions.parseSeed(text).orElseThrow();
            Path out = outDir.resolve(text + ".json");
            write(out, seed(game, dimension, seed));
            System.out.printf("%s%n", out);
        }
        return 0;
    }

    private static JsonObject seed(Game game, String dimension, long seed) throws ReflectiveOperationException {
        if (!(game.stem(dimension).generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        var settings = generator.generatorSettings().value();
        // As ServerChunkCache and ChunkMap create it.
        RandomState random = RandomState.create(game.access.lookupOrThrow(Registries.NOISE), seed, settings);

        var root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("seed", seed);
        root.addProperty("dimension", dimension);

        ChunkPos origin = generator.getOrigin(random);
        root.add("origin", pair(origin.x(), origin.z()));

        List<SpawnTargetPoint> targets = settings.spawnTarget();
        if (targets.isEmpty()) {
            return root;
        }
        // NoiseBasedChunkGenerator.getOrigin's sampler set.
        DensitySamplerSet samplers = random.samplersWithContext(SamplerContext.builder().enableCaches().build());

        // The constructor runs the whole search; rerun it phase by phase on the same instance.
        NoiseSpawnFinder finder = CONSTRUCTOR.newInstance(targets, samplers);
        BlockPos found = (BlockPos) LOCATION.invoke(RESULT.get(finder));
        RESULT.set(finder, FITNESS.invoke(null, samplers, targets, 0, 0));
        var phases = new JsonArray();
        phases.add(result(RESULT.get(finder)));
        RADIAL_SEARCH.invoke(finder, samplers, targets, 2048.0f, 512.0f);
        phases.add(result(RESULT.get(finder)));
        RADIAL_SEARCH.invoke(finder, samplers, targets, 512.0f, 32.0f);
        Object last = RESULT.get(finder);
        phases.add(result(last));
        if (!LOCATION.invoke(last).equals(found) || !ChunkPos.containing(found).equals(origin)) {
            throw new IllegalStateException("the replayed search disagrees with the game's: " + found);
        }
        root.add("phases", phases);

        var samples = new JsonArray();
        for (int[] p : SAMPLES) {
            var o = result(FITNESS.invoke(null, samplers, targets, p[0], p[1]));
            samples.add(o);
        }
        root.add("samples", samples);
        return root;
    }

    /** A {@code NoiseSpawnFinder.Result}: block x and z (y is always 0) and fitness. */
    private static JsonObject result(Object result) throws ReflectiveOperationException {
        var pos = (BlockPos) LOCATION.invoke(result);
        var o = new JsonObject();
        o.add("position", pair(pos.getX(), pos.getZ()));
        o.addProperty("fitness", (long) RESULT_FITNESS.invoke(result));
        return o;
    }

    private static JsonArray pair(int a, int b) {
        var arr = new JsonArray();
        arr.add(a);
        arr.add(b);
        return arr;
    }

    private static void write(Path path, JsonObject root) throws IOException {
        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            new Gson().toJson(root, writer);
        }
        Files.writeString(path, text + "\n");
    }
}
