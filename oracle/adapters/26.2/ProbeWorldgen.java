package spyglass.oracle;

import static spyglass.oracle.Probe.*;

import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.IntStream;

import com.google.gson.JsonArray;

import net.minecraft.core.QuartPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.util.Mth;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseRouter;
import net.minecraft.world.level.levelgen.NoiseSettings;
import net.minecraft.world.level.levelgen.Noises;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.synth.BlendedNoise;
import net.minecraft.world.level.levelgen.synth.ImprovedNoise;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.synth.PerlinNoise;
import net.minecraft.world.level.levelgen.synth.SimplexNoise;

/**
 * The probe's worldgen cases for 26.2's game API, where everything is {@code f64}: the noises, density functions
 * at single points, and what a {@code NoiseChunk} computes (cached climate, interpolated final density, the
 * aquifer), in every dimension. The positions and the format are the core's ({@link Probe}); the case kinds are
 * 26.2's own and specified in {@code docs/specs/oracle.md} ("Probe files").
 */
final class ProbeWorldgen {
    /** The noises the overworld's climate density functions use. */
    private static final List<ResourceKey<NormalNoise.NoiseParameters>> CLIMATE_NOISES = List.of(Noises.TEMPERATURE,
            Noises.VEGETATION, Noises.CONTINENTALNESS, Noises.EROSION, Noises.RIDGE, Noises.SHIFT);
    /** {@code (yScale, yMax)} pairs for the smeared ("broken") Perlin samples. */
    private static final double[][] SMEARS = {{1.0, 0.5}, {0.05, -1.0}, {2.5, 1e9}, {1.0e-3, 0.0}};
    /** The octave set {@code perlin_noise} kind {@code new} builds: first octave, then the amplitudes. */
    private static final int PERLIN_FIRST_OCTAVE = -4;
    private static final double[] PERLIN_AMPLITUDES = {1.0, 0.5, 0.0, 2.0};
    /** The {@code old_blended_noise} parameters the noise cases use: the overworld's. */
    private static final double[] BLENDED = {0.25, 0.125, 80.0, 160.0, 8.0};
    /** 26.2's noise router: every entry, by its name in the settings. */
    private static final String[] ROUTER_26_2 = {
        "barrier", "fluid_level_floodedness", "fluid_level_spread", "lava", "temperature", "vegetation",
        "continents", "erosion", "depth", "ridges", "preliminary_surface_level", "final_density", "vein_toggle",
        "vein_ridged", "vein_gap",
    };
    private static final String OVERWORLD = "minecraft:overworld";
    private static final String FINAL = "noise_router.final_density";

    private ProbeWorldgen() {}

    private static long seed(String arg) {
        return WorldOptions.parseSeed(arg).orElseThrow(() -> new IllegalArgumentException("empty seed"));
    }

    private static NoiseBasedChunkGenerator generator(Game game, String dimension) {
        if (!(game.stem(dimension).generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalStateException(dimension + " isn't noise-based");
        }
        return generator;
    }

    // ---- Noise ----

    // The legacy constructors and the smeared ("broken") samples are deprecated, but they're what the game
    // itself calls (BlendedNoise, the Nether's biome noises).
    @SuppressWarnings("deprecation")
    static JsonArray noise(Game game, List<String> seedArgs) {
        var cases = new JsonArray();
        NoiseGeneratorSettings settings = generator(game, OVERWORLD).generatorSettings().value();
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        for (String seedArg : seedArgs) {
            long seed = seed(seedArg);

            // One Perlin lattice straight from a seeded random.
            var improved = new ImprovedNoise(new XoroshiroRandomSource(seed));
            var parity = new StringBuilder();
            improved.parityConfigString(parity);
            cases.add(kase("improved_noise_parity").seed(seed).text(parity.toString()).json);
            for (double[] p : POINTS) {
                var c = kase("improved_noise").seed(seed).method("noise");
                c.json.add("point", hexes(p[0], p[1], p[2]));
                cases.add(c.doubles(improved.noise(p[0], p[1], p[2])).json);
                for (double[] s : SMEARS) {
                    var cs = kase("improved_noise").seed(seed).method("noise_smeared");
                    cs.json.add("point", hexes(p[0], p[1], p[2], s[0], s[1]));
                    cases.add(cs.doubles(improved.noise(p[0], p[1], p[2], s[0], s[1])).json);
                }
            }

            // Octave sets, both initializations: new (NormalNoise's) and legacy (BlendedNoise's).
            var perlins = new LinkedHashMap<String, PerlinNoise>();
            perlins.put("new", PerlinNoise.create(new XoroshiroRandomSource(seed), PERLIN_FIRST_OCTAVE,
                    PERLIN_AMPLITUDES[0], PERLIN_AMPLITUDES[1], PERLIN_AMPLITUDES[2], PERLIN_AMPLITUDES[3]));
            perlins.put("legacy", PerlinNoise.createLegacyForBlendedNoise(new LegacyRandomSource(seed),
                    IntStream.rangeClosed(-7, 0)));
            for (var e : perlins.entrySet()) {
                var perlinParity = new StringBuilder();
                e.getValue().parityConfigString(perlinParity);
                cases.add(kase("perlin_noise_parity").kind(e.getKey()).seed(seed).text(perlinParity.toString()).json);
                for (double[] p : POINTS) {
                    var c = kase("perlin_noise").kind(e.getKey()).seed(seed).method("get_3d");
                    c.json.add("point", hexes(p[0], p[1], p[2]));
                    cases.add(c.doubles(e.getValue().getValue(p[0], p[1], p[2])).json);
                    for (double[] s : SMEARS) {
                        var cs = kase("perlin_noise").kind(e.getKey()).seed(seed).method("get_smeared");
                        cs.json.add("point", hexes(p[0], p[1], p[2], s[0], s[1]));
                        cases.add(cs.doubles(e.getValue().getValue(p[0], p[1], p[2], s[0], s[1])).json);
                    }
                }
            }

            // The climate noises as a world creates them (RandomState.getOrCreateNoise: NormalNoise.create from
            // the settings' positional factory, hashed by the noise's id).
            var state = RandomState.create(settings, noises, seed);
            PositionalRandomFactory factory = settings.getRandomSource().newInstance(seed).forkPositional();
            for (var key : CLIMATE_NOISES) {
                String id = key.identifier().toString();
                NormalNoise noise = state.getOrCreateNoise(key);
                NormalNoise.NoiseParameters parameters = noises.getOrThrow(key).value();
                // The same noise rebuilt from our copy of RandomState's factory must agree, or the parity string
                // below wouldn't describe the noise that was sampled.
                NormalNoise rebuilt = NormalNoise.create(factory.fromHashOf(key.identifier()), parameters);
                for (double[] p : POINTS) {
                    if (Double.doubleToRawLongBits(noise.getValue(p[0], p[1], p[2]))
                            != Double.doubleToRawLongBits(rebuilt.getValue(p[0], p[1], p[2]))) {
                        throw new IllegalStateException("rebuilt " + id + " differs from RandomState's");
                    }
                }
                normalNoise(cases, "create", seed, id, noise);
            }

            // The Nether's biome noises, as RandomState's noise wiring makes them for any dimension:
            // createLegacyNetherBiome from a LegacyRandomSource at seed + 0 (temperature) and seed + 1 (vegetation).
            List<ResourceKey<NormalNoise.NoiseParameters>> nether =
                    List.of(Noises.TEMPERATURE_NETHER, Noises.VEGETATION_NETHER);
            for (int i = 0; i < nether.size(); i++) {
                var key = nether.get(i);
                NormalNoise noise = NormalNoise.createLegacyNetherBiome(new LegacyRandomSource(seed + i),
                        noises.getOrThrow(key).value());
                normalNoise(cases, "legacy_nether_biome", seed, key.identifier().toString(), noise);
            }

            // The End islands' simplex noise (DensityFunctions.EndIslandDensityFunction): a LegacyRandomSource at
            // the seed, 17292 values skipped.
            var islands = new LegacyRandomSource(seed);
            islands.consumeCount(17292);
            var simplex = new SimplexNoise(islands);
            for (double[] p : POINTS) {
                var c2 = kase("simplex_noise").seed(seed).method("get_2d");
                c2.json.add("point", hexes(p[0], p[2]));
                cases.add(c2.doubles(simplex.getValue(p[0], p[2])).json);
                var c3 = kase("simplex_noise").seed(seed).method("get_3d");
                c3.json.add("point", hexes(p[0], p[1], p[2]));
                cases.add(c3.doubles(simplex.getValue(p[0], p[1], p[2])).json);
            }

            // old_blended_noise, seeded both ways RandomState's wiring seeds it: from the factory's
            // "minecraft:terrain" (xoroshiro) and from a LegacyRandomSource at the seed (legacy).
            var blended = new LinkedHashMap<String, BlendedNoise>();
            blended.put("xoroshiro", new BlendedNoise(new XoroshiroRandomSource(seed).forkPositional()
                    .fromHashOf(Identifier.withDefaultNamespace("terrain")), BLENDED[0], BLENDED[1], BLENDED[2],
                    BLENDED[3], BLENDED[4]));
            blended.put("legacy", new BlendedNoise(new LegacyRandomSource(seed), BLENDED[0], BLENDED[1], BLENDED[2],
                    BLENDED[3], BLENDED[4]));
            for (var e : blended.entrySet()) {
                for (int[] b : BLOCKS) {
                    var c = kase("blended_noise").kind(e.getKey()).seed(seed);
                    c.json.add("pos", ints(b));
                    cases.add(c.doubles(e.getValue().compute(new DensityFunction.SinglePointContext(b[0], b[1],
                            b[2]))).json);
                }
            }
        }
        return cases;
    }

    /** A normal noise's parity string and {@code getValue} at every probe point. */
    private static void normalNoise(JsonArray cases, String kind, long seed, String id, NormalNoise noise) {
        var parity = new StringBuilder();
        noise.parityConfigString(parity);
        cases.add(kase("normal_noise_parity").kind(kind).seed(seed).name(id).text(parity.toString()).json);
        for (double[] p : POINTS) {
            var c = kase("normal_noise").kind(kind).seed(seed).name(id).method("get_3d");
            c.json.add("point", hexes(p[0], p[1], p[2]));
            cases.add(c.doubles(noise.getValue(p[0], p[1], p[2])).json);
        }
    }

    // ---- Density functions, the climate sampler, and the NoiseChunk ----

    static JsonArray density(Game game, List<String> seedArgs) {
        var cases = densityDimension(game, seedArgs, OVERWORLD, "");
        for (String[] dimension : OTHER_DIMENSIONS) {
            cases.addAll(densityDimension(game, seedArgs, dimension[0], dimension[1]));
        }
        return cases;
    }

    /** A router entry by its name in the settings. */
    private static DensityFunction routerEntry(NoiseRouter router, String entry) {
        return switch (entry) {
            case "barrier" -> router.barrierNoise();
            case "fluid_level_floodedness" -> router.fluidLevelFloodednessNoise();
            case "fluid_level_spread" -> router.fluidLevelSpreadNoise();
            case "lava" -> router.lavaNoise();
            case "temperature" -> router.temperature();
            case "vegetation" -> router.vegetation();
            case "continents" -> router.continents();
            case "erosion" -> router.erosion();
            case "depth" -> router.depth();
            case "ridges" -> router.ridges();
            case "preliminary_surface_level" -> router.preliminarySurfaceLevel();
            case "final_density" -> router.finalDensity();
            case "vein_toggle" -> router.veinToggle();
            case "vein_ridged" -> router.veinRidged();
            case "vein_gap" -> router.veinGap();
            default -> throw new IllegalArgumentException(entry);
        };
    }

    /** The six climate functions of a 26.2 sampler, in {@code Climate.Sampler.sample}'s order. */
    private static DensityFunction[] six(Climate.Sampler sampler) {
        return new DensityFunction[] {sampler.temperature(), sampler.humidity(), sampler.continentalness(),
            sampler.erosion(), sampler.depth(), sampler.weirdness()};
    }

    private static LevelHeightAccessor level(LevelStem stem) {
        var type = stem.type().value();
        return LevelHeightAccessor.create(type.minY(), type.height());
    }

    /**
     * One dimension's density cases. The overworld's are every named function in the registry; the Nether's and
     * End's are their own (ids starting with {@code prefix}) and carry the dimension. Every dimension also gets its
     * router's fifteen entries, wired by the random state.
     */
    private static JsonArray densityDimension(Game game, List<String> seedArgs, String dimension, String prefix) {
        var cases = new JsonArray();
        boolean overworld = dimension.equals(OVERWORLD);
        LevelStem stem = game.stem(dimension);
        NoiseGeneratorSettings settings = generator(game, dimension).generatorSettings().value();
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        var functions = game.access.lookupOrThrow(Registries.DENSITY_FUNCTION);
        var named = functions.listElements()
                .filter(h -> h.key().identifier().toString().startsWith(prefix))
                .sorted(Comparator.comparing(h -> h.key().identifier().toString()))
                .toList();
        LevelHeightAccessor level = level(stem);
        int quartMinY = QuartPos.fromBlock(level.getMinY());
        int quartHeight = QuartPos.fromBlock(level.getHeight());
        for (String seedArg : seedArgs) {
            long seed = seed(seedArg);
            var state = RandomState.create(settings, noises, seed);

            // Every function wired to the seed as RandomState wires its router (RandomState$1NoiseWiringHelper),
            // evaluated at single blocks outside any NoiseChunk, where every cache and interpolation marker is
            // transparent.
            DensityFunction.Visitor wiring = Adapter.wiring(state, seed, settings);
            Map<String, DensityFunction> all = new LinkedHashMap<>();
            for (var holder : named) {
                all.put(holder.key().identifier().toString(), holder.value().mapAll(wiring));
            }
            for (String entry : ROUTER_26_2) {
                all.put("noise_router." + entry, routerEntry(state.router(), entry));
            }
            for (var e : all.entrySet()) {
                for (int[] b : BLOCKS) {
                    var c = dimension(kase("density").seed(seed).name(e.getKey()), overworld, dimension);
                    c.json.add("pos", ints(b));
                    cases.add(c.doubles(e.getValue().compute(new DensityFunction.SinglePointContext(b[0], b[1],
                            b[2]))).json);
                }
            }

            // The random state's own climate sampler (markers stripped, no NoiseChunk): what the spawn search and
            // structures sample. The six f64 values before Climate.Sampler.sample narrows them, then its target.
            Climate.Sampler point = state.sampler();
            DensityFunction[] pointSix = six(point);
            for (int[] q : QUARTS) {
                var ctx = new DensityFunction.SinglePointContext(QuartPos.toBlock(q[0]), QuartPos.toBlock(q[1]),
                        QuartPos.toBlock(q[2]));
                double[] values = new double[6];
                for (int i = 0; i < 6; i++) {
                    values[i] = pointSix[i].compute(ctx);
                }
                var c = dimension(kase("climate").seed(seed).method("doubles"), overworld, dimension);
                c.json.add("pos", ints(q));
                cases.add(c.doubles(values).json);
                Climate.TargetPoint t = point.sample(q[0], q[1], q[2]);
                var ct = dimension(kase("climate").seed(seed).method("target"), overworld, dimension);
                ct.json.add("pos", ints(q));
                cases.add(ct.longs(t.temperature(), t.humidity(), t.continentalness(), t.erosion(), t.depth(),
                        t.weirdness()).json);
            }

            for (int[] chunk : CHUNKS) {
                // The climate sampler chunk generation uses (doCreateBiomes: the chunk's NoiseChunk's
                // cachedClimateSampler), over every quart of the chunk, as f64 before narrowing.
                NoiseChunk noiseChunk = Adapter.noiseChunk(state, settings, level, chunk[0] * 16, chunk[1] * 16);
                DensityFunction[] cached = six(Adapter.cachedClimateSampler(noiseChunk, state, settings));
                int size = QuartPos.fromBlock(16);
                int qx0 = QuartPos.fromBlock(chunk[0] * 16);
                int qz0 = QuartPos.fromBlock(chunk[1] * 16);
                long digest = 1;
                long differing = 0;
                for (int qy = 0; qy < quartHeight; qy++) {
                    for (int qz = 0; qz < size; qz++) {
                        for (int qx = 0; qx < size; qx++) {
                            var ctx = new DensityFunction.SinglePointContext(QuartPos.toBlock(qx0 + qx),
                                    QuartPos.toBlock(quartMinY + qy), QuartPos.toBlock(qz0 + qz));
                            for (int i = 0; i < 6; i++) {
                                long bits = Double.doubleToRawLongBits(cached[i].compute(ctx));
                                digest = digest * 1000003L + bits;
                                if (bits != Double.doubleToRawLongBits(pointSix[i].compute(ctx))) {
                                    differing++;
                                }
                            }
                        }
                    }
                }
                var cd = dimension(kase("noise_chunk_climate_digest").seed(seed), overworld, dimension);
                cd.json.add("pos", ints(new int[] {chunk[0], quartMinY, chunk[1]}));
                cd.json.addProperty("height", quartHeight);
                cases.add(cd.longs(digest).json);
                var cv = dimension(kase("noise_chunk_climate_vs_point").seed(seed), overworld, dimension);
                cv.json.add("pos", ints(new int[] {chunk[0], quartMinY, chunk[1]}));
                cv.json.addProperty("height", quartHeight);
                cases.add(cv.longs(differing).json);
            }

            // Final density as the generator interpolates it: per column as iterateNoiseColumn walks it (every
            // value), and per chunk as doFill walks it (a digest).
            for (int[] col : COLUMNS) {
                var walk = new ColumnWalk(state, settings, level, col[0], col[1]);
                double[] values = new double[walk.height()];
                walk.run((noiseChunk, y, i) -> values[i] = Adapter.interpolatedDensity(noiseChunk));
                var c = dimension(kase("noise_chunk_density_column").seed(seed).name(FINAL), overworld, dimension);
                c.json.add("pos", ints(new int[] {col[0], walk.minY(), col[1]}));
                c.json.addProperty("height", walk.height());
                cases.add(c.doubles(values).json);
            }
            for (int[] chunk : CHUNKS) {
                var c = dimension(kase("noise_chunk_density_chunk").seed(seed).name(FINAL), overworld, dimension);
                cases.add(chunkDensity(c, state, settings, level, chunk[0], chunk[1]).json);
            }
        }
        return cases;
    }

    private static Case dimension(Case c, boolean overworld, String dimension) {
        if (!overworld) {
            c.json.addProperty("dimension", dimension);
        }
        return c;
    }

    /**
     * Final density over a chunk as {@code NoiseBasedChunkGenerator.doFill} walks its NoiseChunk (with no
     * structures and nothing blending): cells x, then z, then y from the top, blocks y from the top, then x, then
     * z, reading {@code getInterpolatedDensity} at each block. Digested in index order y fastest, then x, then z.
     */
    private static Case chunkDensity(Case c, RandomState state, NoiseGeneratorSettings settings,
            LevelHeightAccessor level, int cx, int cz) {
        int x0 = cx * 16;
        int z0 = cz * 16;
        // fillFromNoise's range, from the noise settings clamped to the chunk.
        NoiseSettings noise = settings.noiseSettings().clampToHeightAccessor(level);
        int cellHeight = noise.getCellHeight();
        int cellWidth = noise.getCellWidth();
        int minCellY = Mth.floorDiv(noise.minY(), cellHeight);
        int cellCountY = Mth.floorDiv(noise.height(), cellHeight);
        int height = cellCountY * cellHeight;
        int cellCountXZ = 16 / cellWidth;
        double[] values = new double[16 * height * 16];
        NoiseChunk chunk = Adapter.noiseChunk(state, settings, level, x0, z0);
        chunk.initializeForFirstCellX();
        for (int cellX = 0; cellX < cellCountXZ; cellX++) {
            chunk.advanceCellX(cellX);
            for (int cellZ = 0; cellZ < cellCountXZ; cellZ++) {
                for (int cellY = cellCountY - 1; cellY >= 0; cellY--) {
                    chunk.selectCellYZ(cellY, cellZ);
                    for (int dy = cellHeight - 1; dy >= 0; dy--) {
                        int y = (minCellY + cellY) * cellHeight + dy;
                        chunk.updateForY(y, (double) dy / (double) cellHeight);
                        for (int dx = 0; dx < cellWidth; dx++) {
                            int x = x0 + cellX * cellWidth + dx;
                            chunk.updateForX(x, (double) dx / (double) cellWidth);
                            for (int dz = 0; dz < cellWidth; dz++) {
                                int z = z0 + cellZ * cellWidth + dz;
                                chunk.updateForZ(z, (double) dz / (double) cellWidth);
                                int iy = cellY * cellHeight + dy;
                                values[((z - z0) * 16 + (x - x0)) * height + iy] = Adapter.interpolatedDensity(chunk);
                            }
                        }
                    }
                }
            }
            chunk.swapSlices();
        }
        chunk.stopInterpolation();
        long digest = 1;
        for (double v : values) {
            digest = digest * 1000003L + Double.doubleToRawLongBits(v);
        }
        c.json.add("pos", ints(new int[] {x0, minCellY * cellHeight, z0}));
        c.json.addProperty("height", height);
        return c.longs(digest);
    }

    /** Receives each block of a column walk: the NoiseChunk positioned at it, its y, its index from the bottom. */
    private interface BlockVisitor {
        void visit(NoiseChunk chunk, int y, int index);
    }

    /**
     * A block column walked as {@code NoiseBasedChunkGenerator.iterateNoiseColumn} walks it: a NoiseChunk one cell
     * wide at the column's cell (no structures, nothing blending), cells and blocks from the top down, positioned
     * with {@code updateForY}, {@code updateForX} and {@code updateForZ} at each block.
     */
    private static final class ColumnWalk {
        private final RandomState state;
        private final NoiseGeneratorSettings settings;
        private final NoiseSettings noise;
        private final int x;
        private final int z;
        private final int cellHeight;
        private final int cellMinY;
        private final int cellCountY;

        ColumnWalk(RandomState state, NoiseGeneratorSettings settings, LevelHeightAccessor level, int x, int z) {
            this.state = state;
            this.settings = settings;
            this.noise = settings.noiseSettings().clampToHeightAccessor(level);
            this.x = x;
            this.z = z;
            this.cellHeight = noise.getCellHeight();
            this.cellMinY = Mth.floorDiv(noise.minY(), cellHeight);
            this.cellCountY = Mth.floorDiv(noise.height(), cellHeight);
        }

        int minY() {
            return cellMinY * cellHeight;
        }

        int height() {
            return Math.max(cellCountY, 0) * cellHeight;
        }

        void run(BlockVisitor visitor) {
            if (cellCountY <= 0) {
                return;
            }
            int cellWidth = noise.getCellWidth();
            int cellX = Math.floorDiv(x, cellWidth);
            int cellZ = Math.floorDiv(z, cellWidth);
            double fx = (double) Math.floorMod(x, cellWidth) / (double) cellWidth;
            double fz = (double) Math.floorMod(z, cellWidth) / (double) cellWidth;
            var chunk = new NoiseChunk(1, state, cellX * cellWidth, cellZ * cellWidth, noise,
                    Adapter.noBeardifier(), settings, Adapter.fluidPicker(settings),
                    Blender.empty());
            chunk.initializeForFirstCellX();
            chunk.advanceCellX(0);
            for (int cellY = cellCountY - 1; cellY >= 0; cellY--) {
                chunk.selectCellYZ(cellY, 0);
                for (int dy = cellHeight - 1; dy >= 0; dy--) {
                    int y = (cellMinY + cellY) * cellHeight + dy;
                    chunk.updateForY(y, (double) dy / (double) cellHeight);
                    chunk.updateForX(x, fx);
                    chunk.updateForZ(z, fz);
                    visitor.visit(chunk, y, cellY * cellHeight + dy);
                }
            }
            chunk.stopInterpolation();
        }
    }

    // ---- Terrain: the aquifer ----

    /**
     * Densities the aquifer is asked about besides each column's own: well below zero (fluid levels and
     * statuses), slightly below (where barriers decide), and zero.
     */
    private static final double[] AQUIFER_DENSITIES = {-1.0, -0.02, 0.0};

    /**
     * The overworld's aquifer as {@code iterateNoiseColumn} asks it, one column at a time: a fresh column walk per
     * case, {@code computeSubstance} at every block from the top down with the NoiseChunk itself as the context,
     * given the column's interpolated final density or one fixed density.
     */
    static JsonArray terrain(Game game, List<String> seedArgs) {
        var cases = new JsonArray();
        LevelStem stem = game.stem(OVERWORLD);
        NoiseGeneratorSettings settings = generator(game, OVERWORLD).generatorSettings().value();
        var noises = game.access.lookupOrThrow(Registries.NOISE);
        LevelHeightAccessor level = level(stem);
        for (String seedArg : seedArgs) {
            long seed = seed(seedArg);
            var state = RandomState.create(settings, noises, seed);
            for (int[] col : COLUMNS) {
                for (int d = -1; d < AQUIFER_DENSITIES.length; d++) {
                    var walk = new ColumnWalk(state, settings, level, col[0], col[1]);
                    char[] blocks = new char[walk.height()];
                    int fixed = d;
                    walk.run((chunk, y, i) -> {
                        double density = fixed < 0 ? Adapter.interpolatedDensity(chunk) : AQUIFER_DENSITIES[fixed];
                        blocks[i] = blockChar(chunk.aquifer().computeSubstance(chunk, density));
                    });
                    var c = kase("noise_chunk_aquifer_column").seed(seed).method(d < 0 ? "final_density" : "fixed");
                    c.json.add("pos", ints(new int[] {col[0], walk.minY(), col[1]}));
                    c.json.addProperty("height", walk.height());
                    if (d >= 0) {
                        c.json.addProperty("density", hex(Double.doubleToRawLongBits(AQUIFER_DENSITIES[d])));
                    }
                    cases.add(c.text(new String(blocks)).json);
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

    /** Doubles as hex bits, for a case's {@code point}. */
    private static JsonArray hexes(double... values) {
        var a = new JsonArray();
        for (double v : values) {
            a.add(hex(Double.doubleToRawLongBits(v)));
        }
        return a;
    }
}
