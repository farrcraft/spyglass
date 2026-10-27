package spyglass.oracle;

import static spyglass.oracle.Probe.*;

import java.util.Comparator;
import java.util.List;

import com.google.gson.JsonArray;

import net.minecraft.core.QuartPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.Noises;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.densityfunction.DensityBuffer;
import net.minecraft.world.level.levelgen.densityfunction.DensitySampler;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.synth.Noise;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.synth.PerlinNoise;

/**
 * The probe's worldgen cases for 26.3's density API: noises, density functions (point, column,
 * volume and chunk digests), the Nether's and End's routers, and the aquifer. The cases, their
 * positions and their format are the core's ({@link Probe}); this class only asks the game.
 */
final class ProbeWorldgen {
    /** The noises the overworld's climate density functions use. */
    private static final List<ResourceKey<NormalNoise>> CLIMATE_NOISES = List.of(Noises.TEMPERATURE,
            Noises.VEGETATION, Noises.CONTINENTALNESS, Noises.EROSION, Noises.RIDGE, Noises.SHIFT);
    private ProbeWorldgen() {}

    static JsonArray density(Game game, List<String> seeds) {
        var density = densityOverworld(game, seeds);
        for (String[] dimension : OTHER_DIMENSIONS) {
            density.addAll(otherDimension(game, seeds, dimension[0], dimension[1]));
        }
        return density;
    }

    // ---- Noise ----

    static JsonArray noise(Game game, List<String> seedArgs) {
        var cases = new JsonArray();
        var stem = game.stem("minecraft:overworld");
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalStateException("the overworld isn't noise-based");
        }
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        for (String seedArg : seedArgs) {
            long seed = WorldOptions.parseSeed(seedArg)
                    .orElseThrow(() -> new IllegalArgumentException("empty seed"));

            // A bare Perlin noise straight from a seeded random: Perlin on its own.
            var perlin = new PerlinNoise(new XoroshiroRandomSource(seed));
            var parity = new StringBuilder();
            perlin.parityConfigString(parity);
            cases.add(kase("perlin_parity").seed(seed).text(parity.toString()).json);
            samples(cases, kase("perlin").seed(seed), perlin);

            // The climate noises as a world creates them (RandomState.create, Noises.instantiate).
            var state = RandomState.create(noises, seed, generator.generatorSettings().value());
            // RandomState's own factory: the overworld uses Xoroshiro (legacy_random_source false).
            PositionalRandomFactory factory = new XoroshiroRandomSource(seed).forkPositional();
            for (var key : CLIMATE_NOISES) {
                String id = key.identifier().toString();
                Noise noise = state.getOrCreateNoise(key);
                NormalNoise normal = noises.getOrThrow(key).value();
                // The same noise rebuilt from our copy of RandomState's factory must agree, or
                // the parity string below wouldn't describe the noise that was sampled.
                Noise rebuilt = normal.create(factory.fromHashOf(key.identifier()));
                for (double[] p : POINTS) {
                    if (Float.floatToRawIntBits(noise.get(p[0], p[1], p[2]))
                            != Float.floatToRawIntBits(rebuilt.get(p[0], p[1], p[2]))) {
                        throw new IllegalStateException("rebuilt " + id + " differs from RandomState's");
                    }
                }
                var noiseParity = new StringBuilder();
                normal.parityConfigString(noiseParity, factory.fromHashOf(key.identifier()));
                cases.add(kase("normal_noise_parity").seed(seed).name(id).text(noiseParity.toString()).json);
                samples(cases, kase("normal_noise").seed(seed).name(id), noise);
            }
        }
        return cases;
    }

    // ---- Density functions and the climate sampler ----

    private static JsonArray densityOverworld(Game game, List<String> seedArgs) {
        var cases = new JsonArray();
        var stem = game.stem("minecraft:overworld");
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalStateException("the overworld isn't noise-based");
        }
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        var functions = game.access.lookupOrThrow(Registries.DENSITY_FUNCTION);
        int quartMinY = QuartPos.fromBlock(stem.type().value().minY());
        int quartHeight = QuartPos.fromBlock(stem.type().value().height());
        for (String seedArg : seedArgs) {
            long seed = WorldOptions.parseSeed(seedArg)
                    .orElseThrow(() -> new IllegalArgumentException("empty seed"));
            var state = RandomState.create(noises, seed, generator.generatorSettings().value());

            // Every named function on its own, in id order, as RandomState evaluates a single block.
            var named = functions.listElements()
                    .sorted(Comparator.comparing(h -> h.key().identifier().toString()))
                    .toList();
            for (var holder : named) {
                String id = holder.key().identifier().toString();
                var function = holder.value();
                for (int[] b : BLOCKS) {
                    var c = kase("density").seed(seed).name(id);
                    c.json.add("pos", ints(b));
                    cases.add(c.floats(state.sampleBlockValueUncached(function, b[0], b[1], b[2])).json);
                }
            }

            // Every named function through the volume path, as NoiseBasedChunkGenerator.iterateNoiseColumn
            // samples final_density: a fresh caching context per column (its NoiseChunk's, with no beardifier and
            // nothing blending), over the noise settings' whole y range. Then final_density over whole chunks, as
            // chunk generation's NoiseChunk samples it.
            var noiseSettings = generator.generatorSettings().value().noiseSettings();
            int minY = noiseSettings.minY();
            int height = noiseSettings.height();
            var columnPool = state.acquireDensityBufferPool();
            try {
                for (var holder : named) {
                    String id = holder.key().identifier().toString();
                    for (int[] col : COLUMNS) {
                        var context = SamplerContext.builder().enableCaches().useBufferArena(columnPool).build();
                        var volume = new DensityVolume(1, height, 1, col[0], minY, col[1]);
                        var buffer = DensityBuffer.createUnpooled(volume.size());
                        state.samplersWithContext(context).get(holder.value()).sampleVolume(buffer, volume);
                        var c = kase("density_column").seed(seed).name(id);
                        c.json.add("pos", ints(new int[] {col[0], minY, col[1]}));
                        c.json.addProperty("height", height);
                        cases.add(c.longs(digest(buffer, volume.size())).json);
                        if (id.equals(FINAL_DENSITY)) {
                            float[] values = new float[volume.size()];
                            for (int i = 0; i < values.length; i++) {
                                values[i] = buffer.get(i);
                            }
                            var cv = kase("density_column_values").seed(seed).name(id);
                            cv.json.add("pos", ints(new int[] {col[0], minY, col[1]}));
                            cv.json.addProperty("height", height);
                            cases.add(cv.floats(values).json);
                        }
                    }
                }
                var finalDensity = functions.getOrThrow(
                        ResourceKey.create(Registries.DENSITY_FUNCTION, Identifier.parse(FINAL_DENSITY))).value();
                for (int[] chunk : CHUNKS) {
                    var context = SamplerContext.builder().enableCaches().useBufferArena(columnPool).build();
                    var volume = new DensityVolume(16, height, 16, chunk[0] * 16, minY, chunk[1] * 16);
                    var buffer = DensityBuffer.createUnpooled(volume.size());
                    state.samplersWithContext(context).get(finalDensity).sampleVolume(buffer, volume);
                    var c = kase("density_chunk").seed(seed).name(FINAL_DENSITY);
                    c.json.add("pos", ints(new int[] {chunk[0] * 16, minY, chunk[1] * 16}));
                    c.json.addProperty("height", height);
                    cases.add(c.longs(digest(buffer, volume.size())).json);
                }
            } finally {
                state.releaseDensityBufferPool(columnPool);
            }

            // The climate sampler as chunk generation builds it (ChunkGenerator.doCreateBiomes).
            var pool = state.acquireDensityBufferPool();
            try {
                var context = SamplerContext.builder().enableCaches().useBufferArena(pool).build();
                Climate.Sampler sampler = state.createClimateSampler(context);
                DensitySampler.Bound[] six = {sampler.temperature(), sampler.humidity(), sampler.continentalness(),
                    sampler.erosion(), sampler.depth(), sampler.weirdness()};
                for (int[] q : QUARTS) {
                    int bx = QuartPos.toBlock(q[0]);
                    int by = QuartPos.toBlock(q[1]);
                    int bz = QuartPos.toBlock(q[2]);
                    float[] values = new float[6];
                    for (int i = 0; i < 6; i++) {
                        values[i] = six[i].sampleValue(bx, by, bz);
                    }
                    var c = kase("climate").seed(seed).method("floats");
                    c.json.add("pos", ints(q));
                    cases.add(c.floats(values).json);
                    Climate.TargetPoint t = sampler.sample(q[0], q[1], q[2]);
                    var ct = kase("climate").seed(seed).method("target");
                    ct.json.add("pos", ints(q));
                    cases.add(ct.longs(t.temperature(), t.humidity(), t.continentalness(), t.erosion(), t.depth(),
                            t.weirdness()).json);
                }

                // Does the buffered volume path, which chunk generation uses, give the same floats as the
                // point path? Recorded as the number of differing cells per chunk, over all six functions.
                int size = QuartPos.fromBlock(16);
                for (int[] chunk : CHUNKS) {
                    int qx0 = QuartPos.fromBlock(chunk[0] * 16);
                    int qz0 = QuartPos.fromBlock(chunk[1] * 16);
                    var volume = new DensityVolume(size, quartHeight, size, QuartPos.toBlock(qx0),
                            QuartPos.toBlock(quartMinY), QuartPos.toBlock(qz0), 4, 4, 4);
                    long differing = 0;
                    for (DensitySampler.Bound bound : six) {
                        DensityBuffer buffer = DensityBuffer.createUnpooled(volume.size());
                        bound.sampleVolume(buffer, volume);
                        for (int qy = 0; qy < quartHeight; qy++) {
                            for (int qz = 0; qz < size; qz++) {
                                for (int qx = 0; qx < size; qx++) {
                                    float point = bound.sampleValue(QuartPos.toBlock(qx0 + qx),
                                            QuartPos.toBlock(quartMinY + qy), QuartPos.toBlock(qz0 + qz));
                                    float buffered = buffer.get(volume.indexUnchecked(qx, qy, qz));
                                    if (Float.floatToRawIntBits(point) != Float.floatToRawIntBits(buffered)) {
                                        differing++;
                                    }
                                }
                            }
                        }
                    }
                    var c = kase("climate_volume_vs_point").seed(seed);
                    c.json.add("pos", ints(new int[] {chunk[0], 0, chunk[1]}));
                    cases.add(c.longs(differing).json);

                    // Every climate float in the chunk, condensed: catches a one-ulp difference
                    // anywhere without writing ~9,000 values. h = h * 1000003 + unsigned bits, over
                    // cells y, then z, then x (outermost first), and the six values in order.
                    long digest = 1;
                    for (int qy = 0; qy < quartHeight; qy++) {
                        for (int qz = 0; qz < size; qz++) {
                            for (int qx = 0; qx < size; qx++) {
                                int bx = QuartPos.toBlock(qx0 + qx);
                                int by = QuartPos.toBlock(quartMinY + qy);
                                int bz = QuartPos.toBlock(qz0 + qz);
                                for (DensitySampler.Bound bound : six) {
                                    float v = bound.sampleValue(bx, by, bz);
                                    digest = digest * 1000003L + (Float.floatToRawIntBits(v) & 0xFFFFFFFFL);
                                }
                            }
                        }
                    }
                    var cd = kase("climate_digest").seed(seed);
                    cd.json.add("pos", ints(new int[] {chunk[0], 0, chunk[1]}));
                    cases.add(cd.longs(digest).json);
                }
            } finally {
                state.releaseDensityBufferPool(pool);
            }
        }
        return cases;
    }

    /** A router entry's function. */
    private static net.minecraft.world.level.levelgen.densityfunction.DensityFunction routerEntry(
            net.minecraft.world.level.levelgen.NoiseRouter router, String entry) {
        return switch (entry) {
            case "temperature" -> router.temperature();
            case "vegetation" -> router.vegetation();
            case "continents" -> router.continents();
            case "erosion" -> router.erosion();
            case "depth" -> router.depth();
            case "ridges" -> router.ridges();
            case "final_density" -> router.finalDensity();
            default -> throw new IllegalArgumentException(entry);
        };
    }

    /**
     * The Nether's or the End's density, as {@link #density} probes the overworld's: the dimension's own named
     * functions (those under {@code prefix}) and its router entries ({@code noise_router.<entry>}) at the probe's
     * blocks and over column volumes, final density over chunks, and the climate sampler at quarts and as chunk
     * digests over the dimension's height. Every case has the dimension.
     */
    private static JsonArray otherDimension(Game game, List<String> seedArgs, String dimension, String prefix) {
        var cases = new JsonArray();
        var stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalStateException(dimension + " isn't noise-based");
        }
        var settings = generator.generatorSettings().value();
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        var functions = game.access.lookupOrThrow(Registries.DENSITY_FUNCTION);
        var named = functions.listElements()
                .filter(h -> h.key().identifier().toString().startsWith(prefix))
                .sorted(Comparator.comparing(h -> h.key().identifier().toString()))
                .toList();
        var all = new java.util.LinkedHashMap<String, net.minecraft.world.level.levelgen.densityfunction.DensityFunction>();
        for (var holder : named) {
            all.put(holder.key().identifier().toString(), holder.value());
        }
        for (String entry : ROUTER) {
            all.put("noise_router." + entry, routerEntry(settings.noiseRouter(), entry));
        }
        int quartMinY = QuartPos.fromBlock(stem.type().value().minY());
        int quartHeight = QuartPos.fromBlock(stem.type().value().height());
        int minY = settings.noiseSettings().minY();
        int height = settings.noiseSettings().height();
        for (String seedArg : seedArgs) {
            long seed = WorldOptions.parseSeed(seedArg)
                    .orElseThrow(() -> new IllegalArgumentException("empty seed"));
            var state = RandomState.create(noises, seed, settings);
            for (var e : all.entrySet()) {
                for (int[] b : BLOCKS) {
                    var c = kase("density").seed(seed).name(e.getKey());
                    c.json.addProperty("dimension", dimension);
                    c.json.add("pos", ints(b));
                    cases.add(c.floats(state.sampleBlockValueUncached(e.getValue(), b[0], b[1], b[2])).json);
                }
            }
            var pool = state.acquireDensityBufferPool();
            try {
                for (var e : all.entrySet()) {
                    for (int[] col : COLUMNS) {
                        var context = SamplerContext.builder().enableCaches().useBufferArena(pool).build();
                        var volume = new DensityVolume(1, height, 1, col[0], minY, col[1]);
                        var buffer = DensityBuffer.createUnpooled(volume.size());
                        state.samplersWithContext(context).get(e.getValue()).sampleVolume(buffer, volume);
                        var c = kase("density_column").seed(seed).name(e.getKey());
                        c.json.addProperty("dimension", dimension);
                        c.json.add("pos", ints(new int[] {col[0], minY, col[1]}));
                        c.json.addProperty("height", height);
                        cases.add(c.longs(digest(buffer, volume.size())).json);
                    }
                }
                for (int[] chunk : CHUNKS) {
                    var context = SamplerContext.builder().enableCaches().useBufferArena(pool).build();
                    var volume = new DensityVolume(16, height, 16, chunk[0] * 16, minY, chunk[1] * 16);
                    var buffer = DensityBuffer.createUnpooled(volume.size());
                    state.samplersWithContext(context).get(settings.noiseRouter().finalDensity())
                            .sampleVolume(buffer, volume);
                    var c = kase("density_chunk").seed(seed).name("noise_router.final_density");
                    c.json.addProperty("dimension", dimension);
                    c.json.add("pos", ints(new int[] {chunk[0] * 16, minY, chunk[1] * 16}));
                    c.json.addProperty("height", height);
                    cases.add(c.longs(digest(buffer, volume.size())).json);
                }

                var context = SamplerContext.builder().enableCaches().useBufferArena(pool).build();
                Climate.Sampler sampler = state.createClimateSampler(context);
                DensitySampler.Bound[] six = {sampler.temperature(), sampler.humidity(), sampler.continentalness(),
                    sampler.erosion(), sampler.depth(), sampler.weirdness()};
                for (int[] q : QUARTS) {
                    float[] values = new float[6];
                    for (int i = 0; i < 6; i++) {
                        values[i] = six[i].sampleValue(QuartPos.toBlock(q[0]), QuartPos.toBlock(q[1]),
                                QuartPos.toBlock(q[2]));
                    }
                    var c = kase("climate").seed(seed).method("floats");
                    c.json.addProperty("dimension", dimension);
                    c.json.add("pos", ints(q));
                    cases.add(c.floats(values).json);
                }
                int size = QuartPos.fromBlock(16);
                for (int[] chunk : CHUNKS) {
                    int qx0 = QuartPos.fromBlock(chunk[0] * 16);
                    int qz0 = QuartPos.fromBlock(chunk[1] * 16);
                    long digest = 1;
                    for (int qy = 0; qy < quartHeight; qy++) {
                        for (int qz = 0; qz < size; qz++) {
                            for (int qx = 0; qx < size; qx++) {
                                for (DensitySampler.Bound bound : six) {
                                    float v = bound.sampleValue(QuartPos.toBlock(qx0 + qx),
                                            QuartPos.toBlock(quartMinY + qy), QuartPos.toBlock(qz0 + qz));
                                    digest = digest * 1000003L + (Float.floatToRawIntBits(v) & 0xFFFFFFFFL);
                                }
                            }
                        }
                    }
                    var cd = kase("climate_digest").seed(seed);
                    cd.json.addProperty("dimension", dimension);
                    cd.json.add("pos", ints(new int[] {chunk[0], quartMinY, chunk[1]}));
                    cd.json.addProperty("height", quartHeight);
                    cases.add(cd.longs(digest).json);
                }
            } finally {
                state.releaseDensityBufferPool(pool);
            }
        }
        return cases;
    }

    // ---- Terrain: the aquifer ----

    /**
     * Densities the aquifer is asked about besides each column's own: well below zero (fluid levels and
     * statuses), slightly below (where barriers decide), and zero.
     */
    private static final double[] AQUIFER_DENSITIES = {-1.0, -0.02, 0.0};

    /**
     * The aquifer as NoiseBasedChunkGenerator.iterateNoiseColumn builds it, for one column: a NoiseChunk over a
     * 1 × height × 1 volume, with no beardifier and nothing blending. Each case asks computeSubstance about
     * every block from the top down, as the column does, with the column's own final_density or with one fixed
     * density, and records each block bottom-up as one character.
     */
    static JsonArray terrain(Game game, List<String> seedArgs) {
        var cases = new JsonArray();
        var stem = game.stem("minecraft:overworld");
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalStateException("the overworld isn't noise-based");
        }
        NoiseGeneratorSettings settings = generator.generatorSettings().value();
        Aquifer.FluidPicker picker;
        try {
            var create = NoiseBasedChunkGenerator.class.getDeclaredMethod("createFluidPicker",
                    NoiseGeneratorSettings.class);
            create.setAccessible(true);
            picker = (Aquifer.FluidPicker) create.invoke(null, settings);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("can't build the generator's fluid picker", e);
        }
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        int minY = settings.noiseSettings().minY();
        int height = settings.noiseSettings().height();
        for (String seedArg : seedArgs) {
            long seed = WorldOptions.parseSeed(seedArg)
                    .orElseThrow(() -> new IllegalArgumentException("empty seed"));
            var state = RandomState.create(noises, seed, settings);
            for (int[] col : COLUMNS) {
                var volume = new DensityVolume(1, height, 1, col[0], minY, col[1]);
                for (int d = -1; d < AQUIFER_DENSITIES.length; d++) {
                    try (var chunk = Calls.probeChunk(state, settings, picker, volume)) {
                        Aquifer aquifer = chunk.aquifer();
                        float[] density = null;
                        if (d < 0) {
                            var sampler = chunk.cachingSamplers().get(settings.noiseRouter().finalDensity());
                            try (var buffer = sampler.sampleVolume(volume)) {
                                density = new float[height];
                                for (int i = 0; i < height; i++) {
                                    density[i] = buffer.get(i);
                                }
                            }
                        }
                        char[] blocks = new char[height];
                        for (int i = height - 1; i >= 0; i--) {
                            int y = minY + i;
                            double value = d < 0 ? density[i] : AQUIFER_DENSITIES[d];
                            blocks[i] = blockChar(aquifer.computeSubstance(col[0], y, col[1], value));
                        }
                        var c = kase("aquifer_column").seed(seed)
                                .method(d < 0 ? "final_density" : "fixed");
                        c.json.add("pos", ints(new int[] {col[0], minY, col[1]}));
                        c.json.addProperty("height", height);
                        if (d >= 0) {
                            c.json.addProperty("density", hex(Double.doubleToRawLongBits(AQUIFER_DENSITIES[d])));
                        }
                        cases.add(c.text(new String(blocks)).json);
                    }
                }
            }
        }
        return cases;
    }

    /** {@code s} for nothing (the default block), else air, water or lava's first letter. */
    private static char blockChar(BlockState state) {
        if (state == null) {
            return 's';
        }
        if (state.isAir()) {
            return 'a';
        }
        if (state.is(Blocks.WATER)) {
            return 'w';
        }
        if (state.is(Blocks.LAVA)) {
            return 'l';
        }
        throw new IllegalStateException("unexpected aquifer block " + state);
    }

    /**
     * A buffer's floats in index order (y fastest, then x, then z), condensed as the climate digest is:
     * {@code h = h * 1000003 + unsigned bits}, from 1.
     */
    private static long digest(DensityBuffer buffer, int size) {
        long digest = 1;
        for (int i = 0; i < size; i++) {
            digest = digest * 1000003L + (Float.floatToRawIntBits(buffer.get(i)) & 0xFFFFFFFFL);
        }
        return digest;
    }

    /** {@code get(x, y, z)} and {@code get(x, z)} at every probe point, one case each. */
    private static void samples(JsonArray cases, Case template, Noise noise) {
        for (double[] p : POINTS) {
            var c3 = template.copy("get_3d");
            var point = new JsonArray();
            for (double v : p) {
                point.add(hex(Double.doubleToRawLongBits(v)));
            }
            c3.json.add("point", point);
            cases.add(c3.floats(noise.get(p[0], p[1], p[2])).json);

            var c2 = template.copy("get_2d");
            var point2 = new JsonArray();
            point2.add(hex(Double.doubleToRawLongBits(p[0])));
            point2.add(hex(Double.doubleToRawLongBits(p[2])));
            c2.json.add("point", point2);
            cases.add(c2.floats(noise.get(p[0], p[2])).json);
        }
    }
}
