package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.NoiseColumn;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;

/**
 * Height golden files: for one seed, what the generator answers when a structure asks about
 * terrain before any chunk exists. {@code ChunkGenerator.getBaseHeight} for the two worldgen
 * heightmaps at every column of a few block regions, and {@code getBaseColumn} at a sample of
 * columns. See {@code docs/specs/oracle.md}.
 */
final class Heights {
    /** A block region: minimum x and z, and the side. */
    record Region(String name, int minX, int minZ, int size) {}

    static final List<Region> REGIONS = List.of(
            new Region("origin", -128, -128, 256),
            new Region("far-pos", 1_000_000, 1_000_000, 64),
            new Region("far-neg", -1_000_064, -1_000_064, 64),
            new Region("border-pos", 29_999_920, 29_999_920, 64),
            new Region("border-neg", -29_999_984, -29_999_984, 64));

    static final List<Heightmap.Types> HEIGHTMAPS =
            List.of(Heightmap.Types.WORLD_SURFACE_WG, Heightmap.Types.OCEAN_FLOOR_WG);

    private Heights() {}

    static int run(Game game, Path outDir, String dimension, List<String> seeds, int threads) throws Exception {
        Files.createDirectories(outDir);
        try (var pool = Executors.newFixedThreadPool(Math.max(1, Math.min(threads, seeds.size())))) {
            List<Future<?>> futures = new ArrayList<>();
            for (String text : seeds) {
                futures.add(pool.submit(() -> {
                    long t0 = System.nanoTime();
                    long seed = WorldOptions.parseSeed(text).orElseThrow();
                    Path out = outDir.resolve(text + ".json");
                    write(out, seed(game, dimension, seed));
                    System.out.printf("%s: %.1f s%n", out, (System.nanoTime() - t0) / 1e9);
                    return null;
                }));
            }
            for (Future<?> f : futures) {
                try {
                    f.get();
                } catch (ExecutionException e) {
                    pool.shutdownNow();
                    throw new RuntimeException(e.getCause());
                }
            }
        }
        return 0;
    }

    /** The sampled columns: an 8 × 8 grid over the origin region, at varying offsets in their chunks. */
    static List<int[]> columns() {
        var out = new ArrayList<int[]>();
        for (int j = 0; j < 8; j++) {
            for (int i = 0; i < 8; i++) {
                out.add(new int[] {-128 + 32 * i + (7 * i) % 16, -128 + 32 * j + (11 * j) % 16});
            }
        }
        for (Region r : REGIONS.subList(1, REGIONS.size())) {
            out.add(new int[] {r.minX(), r.minZ()});
            out.add(new int[] {r.minX() + r.size() - 1, r.minZ() + r.size() - 1});
        }
        return out;
    }

    private static JsonObject seed(Game game, String dimension, long seed) {
        LevelStem stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        // As ServerChunkCache and ChunkMap create them.
        RandomState random = RandomState.create(game.access.lookupOrThrow(Registries.NOISE), seed,
                generator.generatorSettings().value());
        var type = stem.type().value();
        LevelHeightAccessor height = LevelHeightAccessor.create(type.minY(), type.height());

        var root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("seed", seed);
        root.addProperty("dimension", dimension);
        root.addProperty("min_y", type.minY());
        root.addProperty("height", type.height());

        var regions = new JsonArray();
        for (Region r : REGIONS) {
            var o = new JsonObject();
            o.addProperty("name", r.name());
            o.add("min", pair(r.minX(), r.minZ()));
            o.addProperty("size", r.size());
            for (Heightmap.Types heightmap : HEIGHTMAPS) {
                // z outer, x inner.
                var values = new JsonArray();
                for (int z = r.minZ(); z < r.minZ() + r.size(); z++) {
                    for (int x = r.minX(); x < r.minX() + r.size(); x++) {
                        values.add(generator.getBaseHeight(x, z, heightmap, height, random));
                    }
                }
                o.add(heightmap.getSerializationKey(), values);
            }
            regions.add(o);
        }
        root.add("regions", regions);

        var columns = new JsonArray();
        for (int[] c : columns()) {
            NoiseColumn column = generator.getBaseColumn(c[0], c[1], height, random);
            // Runs of one block, bottom up.
            var runs = new JsonArray();
            String last = null;
            int count = 0;
            for (int y = type.minY(); y < type.minY() + type.height(); y++) {
                String id = BuiltInRegistries.BLOCK.getKey(column.getBlock(y).getBlock())
                        .toString();
                if (id.equals(last)) {
                    count++;
                    continue;
                }
                if (last != null) {
                    runs.add(run(last, count));
                }
                last = id;
                count = 1;
            }
            runs.add(run(last, count));
            var o = new JsonObject();
            o.add("pos", pair(c[0], c[1]));
            o.add("blocks", runs);
            columns.add(o);
        }
        root.add("columns", columns);
        return root;
    }

    private static JsonArray run(String id, int count) {
        var arr = new JsonArray();
        arr.add(id);
        arr.add(count);
        return arr;
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
