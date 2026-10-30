package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;

/**
 * Block golden files: for one seed, the blocks of whole chunks as generation leaves them before
 * features, with no structures. See {@code docs/specs/oracle.md#block-golden-files}.
 */
final class Blocks {
    /** A square of chunks: its minimum chunk and side. */
    record Area(String name, int minX, int minZ, int size) {}

    static final List<Area> AREAS = List.of(
            new Area("origin", -2, -2, 4),
            new Area("far-pos", 62_500, 62_500, 2),
            new Area("far-neg", -62_504, -62_504, 2),
            new Area("border-pos", 1_874_995, 1_874_995, 2),
            new Area("border-neg", -1_874_999, -1_874_999, 2));

    private Blocks() {}

    static int run(Game game, Path outDir, String dimension, List<String> seeds, int threads) throws Exception {
        Files.createDirectories(outDir);
        // One task per chunk: a chunk costs the game far more than a seed's setup.
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

    private static JsonObject seed(Game game, String dimension, long seed, java.util.concurrent.ExecutorService pool)
            throws InterruptedException {
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
        root.addProperty("min_y", type.minY());
        root.addProperty("height", type.height());

        var positions = new ArrayList<ChunkPos>();
        for (Area a : AREAS) {
            for (int z = a.minZ(); z < a.minZ() + a.size(); z++) {
                for (int x = a.minX(); x < a.minX() + a.size(); x++) {
                    positions.add(new ChunkPos(x, z));
                }
            }
        }
        var futures = new ArrayList<Future<JsonObject>>();
        for (ChunkPos pos : positions) {
            futures.add(pool.submit(() -> {
                ChunkAccess chunk = api.generateChunk(game, generator, height, random, seed, pos);
                var o = new JsonObject();
                o.add("chunk", pair(pos.x(), pos.z()));
                o.add("columns", columns(chunk, type.minY(), type.height()));
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
     * Prints, for one column, the generator's {@code getBaseColumn} beside the generated chunk's
     * blocks, where they differ in kind: for finding where the two disagree.
     */
    static int column(Game game, String seedText, String dimension, int x, int z) {
        LevelStem stem = game.stem(dimension);
        var generator = (NoiseBasedChunkGenerator) stem.generator();
        var api = GameApi.get();
        var type = stem.type().value();
        LevelHeightAccessor height = LevelHeightAccessor.create(type.minY(), type.height());
        long seed = WorldOptions.parseSeed(seedText).orElseThrow();
        RandomState random = api.randomState(game, generator, height, seed);
        BlockState[] column = api.baseColumn(generator, x, z, height, random);
        ChunkAccess chunk = api.generateChunk(game, generator, height, random, seed, new ChunkPos(x >> 4, z >> 4));
        var pos = new BlockPos.MutableBlockPos();
        for (int y = type.minY(); y < type.minY() + type.height(); y++) {
            BlockState base = column[y - type.minY()];
            String a = base == null ? "solid" : name(base);
            String b = name(chunk.getBlockState(pos.set(x, y, z)));
            System.out.printf("%d %s %s%s%n", y, a, b, a.equals(b) ? "" : "  <-");
        }
        return 0;
    }

    /** The chunk's 256 columns, x fastest within z, each as runs of one state from the bottom up. */
    private static JsonArray columns(ChunkAccess chunk, int minY, int height) {
        var columns = new JsonArray();
        var pos = new BlockPos.MutableBlockPos();
        int x0 = chunk.getPos().getMinBlockX();
        int z0 = chunk.getPos().getMinBlockZ();
        for (int dz = 0; dz < 16; dz++) {
            for (int dx = 0; dx < 16; dx++) {
                var runs = new JsonArray();
                String last = null;
                int count = 0;
                for (int y = minY; y < minY + height; y++) {
                    String state = name(chunk.getBlockState(pos.set(x0 + dx, y, z0 + dz)));
                    if (state.equals(last)) {
                        count++;
                        continue;
                    }
                    if (last != null) {
                        runs.add(run(last, count));
                    }
                    last = state;
                    count = 1;
                }
                runs.add(run(last, count));
                columns.add(runs);
            }
        }
        return columns;
    }

    /** A state's id, then its non-default properties in brackets, by name: {@code id[a=b,c=d]}. */
    static String name(BlockState state) {
        var id = new StringBuilder(BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
        BlockState plain = state.getBlock().defaultBlockState();
        Map<String, String> changed = new TreeMap<>();
        for (Property<?> p : state.getProperties()) {
            if (!state.getValue(p).equals(plain.getValue(p))) {
                changed.put(p.getName(), value(state, p));
            }
        }
        if (!changed.isEmpty()) {
            id.append('[');
            changed.forEach((k, v) -> id.append(id.charAt(id.length() - 1) == '[' ? "" : ",").append(k)
                    .append('=').append(v));
            id.append(']');
        }
        return id.toString();
    }

    private static <T extends Comparable<T>> String value(BlockState state, Property<T> property) {
        return property.getName(state.getValue(property));
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
