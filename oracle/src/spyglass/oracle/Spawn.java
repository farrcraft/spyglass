package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;

/**
 * Spawn golden files: for one seed, the chunk the game's spawn search starts from
 * ({@code ChunkGenerator.getOrigin}), the climate search's result after each of its phases, and
 * the search's fitness at fixed positions. See {@code docs/specs/oracle.md}.
 *
 * <p>The phases replay the game's search phase by phase ({@link GameApi#spawn}): the fitness at
 * (0, 0), then the radial search with (2048, 512) and (512, 32). The final spawn block needs a live
 * level ({@code PlayerSpawnFinder}), so it isn't written.
 */
final class Spawn {
    /** Block (x, z) positions the fitness is written at: negatives, quart boundaries, the border. */
    static final int[][] SAMPLES = {
            {0, 0}, {1, 1}, {3, -1}, {-1, -4}, {-5, 7}, {512, 0}, {-517, 1203}, {2048, -2048},
            {12_345, -6_789}, {-100_003, 99_998}, {29_999_984, -29_999_984}, {-30_000_000, 30_000_000}};

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
        var api = GameApi.get();
        RandomState random = api.randomState(game, generator, seed);

        var root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("seed", seed);
        root.addProperty("dimension", dimension);

        var run = api.spawn(generator, random, SAMPLES);
        root.add("origin", pair(run.origin().x(), run.origin().z()));
        if (run.phases() == null) {
            return root;
        }
        var phases = new JsonArray();
        for (var r : run.phases()) {
            phases.add(result(r));
        }
        root.add("phases", phases);
        var samples = new JsonArray();
        for (var r : run.samples()) {
            samples.add(result(r));
        }
        root.add("samples", samples);
        return root;
    }

    /** A scored position: block x and z (y is always 0) and fitness. */
    private static JsonObject result(GameApi.SpawnResult result) {
        var o = new JsonObject();
        o.add("position", pair(result.x(), result.z()));
        o.addProperty("fitness", result.fitness());
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
