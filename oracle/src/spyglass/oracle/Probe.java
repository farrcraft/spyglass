package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.function.Function;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomSupport;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;

/**
 * Raw values from the game's random sources and noises, for the Rust ports' ignored tests. The
 * format is specified in {@code docs/specs/oracle.md} ("Probe files").
 *
 * <p>Each case records everything needed to reproduce it (kind, seed, bound, point), so the Rust
 * side replays the cases it finds rather than keeping its own copy of these lists. Floating-point
 * values are written as the hex of their raw bits.
 */
final class Probe {
    static final long[] RNG_SEEDS = {
        0L, 1L, -1L, Long.MIN_VALUE, Long.MAX_VALUE, 42L, 0x5DEECE66DL, -4893098694207956070L, 123456789L,
    };
    static final String[] NAMES = {
        "", "a", "octave_0", "octave_-10", "minecraft:temperature", "minecraft:offset",
        "minecraft:overworld/continents", "café",
    };
    static final int[] BOUNDS = {1, 2, 3, 5, 7, 10, 16, 100, 1000, (1 << 30) + 1, Integer.MAX_VALUE};
    static final int[][] POSITIONS = {
        {0, 0, 0}, {1, 2, 3}, {-1, -2, -3}, {1000, -64, -1000}, {30000000, 320, -30000000},
        {Integer.MIN_VALUE, Integer.MAX_VALUE, 0},
    };
    static final double[][] POINTS = {
        {0, 0, 0}, {0.5, 0.25, -0.75}, {1, 1, 1}, {-1, -1, -1}, {12.34, -56.78, 90.12},
        {-123.5, 64, 4567.25}, {1e6, 0, -1e6}, {-1000064.75, 100.5, 1000000.25},
        {29999984, 320, -29999984}, {33554431.5, -33554432.25, 16777216.5}, {1e9, -1e9, 5e8},
        {0.001, -0.001, 1e-9}, {-7.25e6, -64, 7.5e6}, {256.125, 0, -0.0},
    };
    static final int SEQUENCE = 8;
    /**
     * Block positions for density functions; the y sweep at the end exercises depth's gradient. Most aren't on
     * the 4 × 8 × 4 cell corners of interpolated functions.
     */
    static final int[][] BLOCKS = {
        {0, 0, 0}, {1, -1, 1}, {-1, 64, -1}, {123, -64, -456}, {-1000, 320, 1000}, {1000000, 70, -1000000},
        {-29999984, 0, 29999984}, {29999983, -63, -29999983}, {7, 100, -13}, {4096, -30, -4096},
        {-517, -65, 3}, {-517, -40, 3}, {-517, -1, 3}, {-517, 62, 3}, {-517, 63, 3}, {-517, 200, 3},
        {-517, 319, 3}, {-517, 321, 3},
    };
    /** Quart positions for the climate sampler. */
    static final int[][] QUARTS = {
        {0, 0, 0}, {-1, -16, -1}, {31, -5, -77}, {1, 79, 1}, {250000, 16, -250000}, {-7499996, 0, 7499996},
        {7499995, -16, -7499995}, {-129, 40, 128},
    };
    /** Chunks where the climate sampler's volume path is compared with its point path. */
    static final int[][] CHUNKS = {{0, 0}, {-1, -1}, {62500, -62500}, {-1874999, 1874999}};
    /**
     * Block columns {@code (x, z)} sampled as the generator samples them: one 1 × height × 1 volume. Some x and z
     * are on interpolated cell corners and some aren't, which changes how many corners a column needs.
     */
    static final int[][] COLUMNS = {
        {0, 0}, {1, -1}, {-517, 3}, {123, -456}, {4096, -4096}, {1000000, -1000000}, {-29999984, 29999984},
        {29999983, -29999983},
    };
    static final String FINAL_DENSITY = "minecraft:overworld/final_density";
    /** Dimensions probed besides the overworld, with the prefix of their named density functions. */
    static final String[][] OTHER_DIMENSIONS = {
        {"minecraft:the_nether", "minecraft:nether/"}, {"minecraft:the_end", "minecraft:end/"},
    };
    /** The noise router's entries, which the Nether and End write inline. */
    static final String[] ROUTER = {
        "temperature", "vegetation", "continents", "erosion", "depth", "ridges", "final_density",
    };

    private Probe() {}

    static int run(Game game, Path outDir, List<String> seeds) throws IOException {
        Files.createDirectories(outDir);
        write(outDir.resolve("rng.json"), game, rng());
        var api = GameApi.get();
        write(outDir.resolve("noise.json"), game, api.probeNoise(game, seeds));
        write(outDir.resolve("density.json"), game, api.probeDensity(game, seeds));
        write(outDir.resolve("terrain.json"), game, api.probeTerrain(game, seeds));
        return 0;
    }

    private static void write(Path path, Game game, JsonArray cases) throws IOException {
        var root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.add("cases", cases);
        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            writer.setIndent("  ");
            writer.setHtmlSafe(false);
            new Gson().toJson(root, writer);
        }
        Files.writeString(path, text + "\n", StandardCharsets.UTF_8);
        System.out.printf("%s: %d cases%n", path, cases.size());
    }

    // ---- Random sources ----

    private static JsonArray rng() {
        var cases = new JsonArray();
        for (long seed : RNG_SEEDS) {
            cases.add(kase("mix_stafford13").seed(seed).longs(RandomSupport.mixStafford13(seed)).json);
            var mixed = RandomSupport.upgradeSeedTo128bit(seed);
            cases.add(kase("upgrade_seed_to_128bit").seed(seed).longs(mixed.seedLo(), mixed.seedHi()).json);
            var unmixed = RandomSupport.upgradeSeedTo128bitUnmixed(seed);
            cases.add(kase("upgrade_seed_to_128bit_unmixed").seed(seed)
                    .longs(unmixed.seedLo(), unmixed.seedHi()).json);
        }
        for (String name : NAMES) {
            var hash = RandomSupport.seedFromHashOf(name);
            cases.add(kase("seed_from_hash_of").name(name).longs(hash.seedLo(), hash.seedHi()).json);
        }
        for (String kind : List.of("xoroshiro", "legacy")) {
            Function<Long, RandomSource> make = kind.equals("xoroshiro")
                    ? XoroshiroRandomSource::new
                    : LegacyRandomSource::new;
            for (long seed : RNG_SEEDS) {
                sequences(cases, kind, seed, make);
            }
        }
        return cases;
    }

    private static void sequences(JsonArray cases, String kind, long seed, Function<Long, RandomSource> make) {
        RandomSource r = make.apply(seed);
        long[] longs = new long[SEQUENCE];
        for (int i = 0; i < SEQUENCE; i++) {
            longs[i] = r.nextLong();
        }
        cases.add(kase("next_long").kind(kind).seed(seed).longs(longs).json);

        r = make.apply(seed);
        long[] ints = new long[SEQUENCE];
        for (int i = 0; i < SEQUENCE; i++) {
            ints[i] = r.nextInt();
        }
        cases.add(kase("next_int").kind(kind).seed(seed).longs(ints).json);

        for (int bound : BOUNDS) {
            r = make.apply(seed);
            long[] values = new long[SEQUENCE];
            for (int i = 0; i < SEQUENCE; i++) {
                values[i] = r.nextInt(bound);
            }
            var c = kase("next_int_bounded").kind(kind).seed(seed);
            c.json.addProperty("bound", bound);
            cases.add(c.longs(values).json);
        }

        r = make.apply(seed);
        float[] floats = new float[SEQUENCE];
        for (int i = 0; i < SEQUENCE; i++) {
            floats[i] = r.nextFloat();
        }
        cases.add(kase("next_float").kind(kind).seed(seed).floats(floats).json);

        r = make.apply(seed);
        double[] doubles = new double[SEQUENCE];
        for (int i = 0; i < SEQUENCE; i++) {
            doubles[i] = r.nextDouble();
        }
        cases.add(kase("next_double").kind(kind).seed(seed).doubles(doubles).json);

        r = make.apply(seed);
        double[] gaussians = new double[SEQUENCE];
        for (int i = 0; i < SEQUENCE; i++) {
            gaussians[i] = r.nextGaussian();
        }
        cases.add(kase("next_gaussian").kind(kind).seed(seed).doubles(gaussians).json);

        r = make.apply(seed);
        long[] booleans = new long[SEQUENCE];
        for (int i = 0; i < SEQUENCE; i++) {
            booleans[i] = r.nextBoolean() ? 1 : 0;
        }
        cases.add(kase("next_boolean").kind(kind).seed(seed).longs(booleans).json);

        // A Gaussian caches its second value; interleaving shows whether other calls disturb it.
        r = make.apply(seed);
        long[] interleaved = {
            Double.doubleToRawLongBits(r.nextGaussian()), r.nextLong(), Double.doubleToRawLongBits(r.nextGaussian()),
            Double.doubleToRawLongBits(r.nextGaussian()), r.nextInt(10),
        };
        cases.add(kase("gaussian_interleaved").kind(kind).seed(seed).longs(interleaved).json);

        r = make.apply(seed);
        r.consumeCount(5);
        cases.add(kase("consume_count_5").kind(kind).seed(seed).longs(r.nextLong(), r.nextLong()).json);

        r = make.apply(seed);
        RandomSource fork = r.fork();
        cases.add(kase("fork").kind(kind).seed(seed)
                .longs(fork.nextLong(), fork.nextLong(), r.nextLong(), r.nextLong()).json);

        PositionalRandomFactory positional = make.apply(seed).forkPositional();
        var parity = new StringBuilder();
        positional.parityConfigString(parity);
        cases.add(kase("positional_parity").kind(kind).seed(seed).text(parity.toString()).json);
        for (int[] p : POSITIONS) {
            RandomSource at = positional.at(p[0], p[1], p[2]);
            var c = kase("positional_at").kind(kind).seed(seed);
            c.json.add("pos", ints(p));
            cases.add(c.longs(at.nextLong(), at.nextLong()).json);
        }
        for (String name : NAMES) {
            RandomSource hashed = positional.fromHashOf(name);
            cases.add(kase("positional_from_hash_of").kind(kind).seed(seed).name(name)
                    .longs(hashed.nextLong(), hashed.nextLong()).json);
        }
    }

    // ---- Case building ----

    static Case kase(String test) {
        return new Case(test);
    }

    /** One case: {@code {"test": ..., parameters..., "type": ..., "values": [...]}}. */
    static final class Case {
        final JsonObject json = new JsonObject();

        Case(String test) {
            json.addProperty("test", test);
        }

        /** A copy of this case's parameters, with {@code method} added. */
        Case copy(String method) {
            var c = new Case(json.get("test").getAsString());
            for (var e : json.entrySet()) {
                c.json.add(e.getKey(), e.getValue().deepCopy());
            }
            c.json.addProperty("method", method);
            return c;
        }

        Case kind(String kind) {
            json.addProperty("kind", kind);
            return this;
        }

        Case seed(long seed) {
            json.addProperty("seed", seed);
            return this;
        }

        Case method(String method) {
            json.addProperty("method", method);
            return this;
        }

        Case name(String name) {
            json.addProperty("name", name);
            return this;
        }

        Case longs(long... values) {
            json.addProperty("type", "i64");
            var a = new JsonArray();
            for (long v : values) {
                a.add(v);
            }
            json.add("values", a);
            return this;
        }

        Case floats(float... values) {
            json.addProperty("type", "f32");
            var a = new JsonArray();
            for (float v : values) {
                a.add(String.format("%08x", Float.floatToRawIntBits(v)));
            }
            json.add("values", a);
            return this;
        }

        Case doubles(double... values) {
            json.addProperty("type", "f64");
            var a = new JsonArray();
            for (double v : values) {
                a.add(hex(Double.doubleToRawLongBits(v)));
            }
            json.add("values", a);
            return this;
        }

        Case text(String value) {
            json.addProperty("type", "text");
            var a = new JsonArray();
            a.add(value);
            json.add("values", a);
            return this;
        }
    }

    static String hex(long bits) {
        return String.format("%016x", bits);
    }

    static JsonArray ints(int[] values) {
        var a = new JsonArray();
        for (int v : values) {
            a.add(v);
        }
        return a;
    }
}
