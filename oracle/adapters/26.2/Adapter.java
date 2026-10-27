package spyglass.oracle;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.WeakHashMap;
import java.util.function.Predicate;

import com.google.gson.JsonArray;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.QuartPos;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.NoiseColumn;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.DensityFunctions;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

/**
 * {@link GameApi} for 26.2's game API, the 1.21 family: density functions computed one point at a time in
 * {@code f64} ({@code DensityFunction.compute}), with caching and interpolation only inside a {@code NoiseChunk},
 * climate samplers that hold the spawn targets, and {@code Climate$SpawnFinder}.
 */
final class Adapter implements GameApi {
    static final Adapter INSTANCE = new Adapter();

    private static final MethodHandle FITNESS;
    private static final Constructor<?> SPAWN_FINDER;
    private static final Field SPAWN_RESULT;
    private static final Method RADIAL_SEARCH;
    private static final Method SPAWN_FITNESS;
    private static final Method SPAWN_LOCATION;
    private static final Method SPAWN_RESULT_FITNESS;
    private static final Method FIND_GENERATION_POINT;
    private static final Method IS_VALID_BIOME;
    private static final Method CREATE_FLUID_PICKER;
    private static final Method CACHED_CLIMATE_SAMPLER;
    private static final Method INTERPOLATED_DENSITY;
    private static final Constructor<?> WIRING_HELPER;

    static {
        try {
            Method fitness = Climate.ParameterPoint.class.getDeclaredMethod("fitness", Climate.TargetPoint.class);
            fitness.setAccessible(true);
            FITNESS = MethodHandles.lookup().unreflect(fitness);
            Class<?> finder = Class.forName("net.minecraft.world.level.biome.Climate$SpawnFinder", false,
                    Adapter.class.getClassLoader());
            SPAWN_FINDER = finder.getDeclaredConstructor(List.class, Climate.Sampler.class);
            SPAWN_RESULT = finder.getDeclaredField("result");
            RADIAL_SEARCH = finder.getDeclaredMethod("radialSearch", List.class, Climate.Sampler.class, float.class,
                    float.class);
            SPAWN_FITNESS = finder.getDeclaredMethod("getSpawnPositionAndFitness", List.class, Climate.Sampler.class,
                    int.class, int.class);
            Class<?> result = SPAWN_RESULT.getType();
            SPAWN_LOCATION = result.getDeclaredMethod("location");
            SPAWN_RESULT_FITNESS = result.getDeclaredMethod("fitness");
            FIND_GENERATION_POINT =
                    Structure.class.getDeclaredMethod("findGenerationPoint", Structure.GenerationContext.class);
            IS_VALID_BIOME = Structure.class.getDeclaredMethod("isValidBiome", Structure.GenerationStub.class,
                    Structure.GenerationContext.class);
            CREATE_FLUID_PICKER = NoiseBasedChunkGenerator.class.getDeclaredMethod("createFluidPicker",
                    NoiseGeneratorSettings.class);
            CACHED_CLIMATE_SAMPLER = NoiseChunk.class.getDeclaredMethod("cachedClimateSampler",
                    net.minecraft.world.level.levelgen.NoiseRouter.class, List.class);
            INTERPOLATED_DENSITY = NoiseChunk.class.getDeclaredMethod("getInterpolatedDensity");
            WIRING_HELPER = Class.forName("net.minecraft.world.level.levelgen.RandomState$1NoiseWiringHelper",
                    false, Adapter.class.getClassLoader()).getDeclaredConstructor(RandomState.class, long.class, boolean.class);
            for (var m : List.of(SPAWN_FINDER, SPAWN_RESULT, RADIAL_SEARCH, SPAWN_FITNESS, SPAWN_LOCATION,
                    SPAWN_RESULT_FITNESS, FIND_GENERATION_POINT, IS_VALID_BIOME, CREATE_FLUID_PICKER,
                    CACHED_CLIMATE_SAMPLER, INTERPOLATED_DENSITY, WIRING_HELPER)) {
                m.setAccessible(true);
            }
        } catch (ReflectiveOperationException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    /**
     * {@code DensityFunctions.BeardifierMarker.INSTANCE}, no structures: package-private in 26.2, and only
     * loadable once the game is bootstrapped.
     */
    static DensityFunctions.BeardifierOrMarker noBeardifier() {
        return NoBeardifier.INSTANCE;
    }

    private static final class NoBeardifier {
        static final DensityFunctions.BeardifierOrMarker INSTANCE;

        static {
            try {
                Field marker = Class.forName("net.minecraft.world.level.levelgen.DensityFunctions$BeardifierMarker")
                        .getDeclaredField("INSTANCE");
                marker.setAccessible(true);
                INSTANCE = (DensityFunctions.BeardifierOrMarker) marker.get(null);
            } catch (ReflectiveOperationException e) {
                throw new ExceptionInInitializerError(e);
            }
        }
    }

    /**
     * Each random state's generator settings, for the noise chunks {@link #chunkBiomes} builds: in 26.2 they need
     * the settings, which a {@code RandomState} doesn't keep.
     */
    private final Map<RandomState, NoiseGeneratorSettings> settingsOf =
            Collections.synchronizedMap(new WeakHashMap<>());

    private Adapter() {}

    @Override
    public Game load() {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // WorldLoader.load: static registries first, then tags from the data packs, then the
        // worldgen registries from the data packs into the WORLDGEN layer. (Its next step loads the
        // DIMENSIONS layer, which a new world fills from the preset below instead.)
        LayeredRegistryAccess<RegistryLayer> layers = RegistryLayer.createRegistryAccess();
        var resources = new MultiPackResourceManager(PackType.SERVER_DATA,
                List.of(ServerPacksSource.createVanillaPackSource()));
        var tags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
        List<HolderLookup.RegistryLookup<?>> lookups =
                TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLDGEN), tags);
        RegistryAccess.Frozen worldgen = RegistryDataLoader.load(resources, lookups,
                RegistryDataLoader.WORLDGEN_REGISTRIES, Runnable::run).join();
        RegistryAccess.Frozen access = layers.replaceFrom(RegistryLayer.WORLDGEN, worldgen).compositeAccess();
        // ReloadableServerResources.updateComponentsAndStaticRegistryTags: bind the static registries'
        // tags (blocks, items), which structure pieces read.
        tags.forEach(net.minecraft.core.Registry.PendingTags::apply);

        // What CreateWorldScreen and the dedicated server give a world with the Default preset.
        return new Game(access, resources, WorldPresets.createNormalWorldDimensions(access));
    }

    @Override
    public RandomState randomState(Game game, NoiseBasedChunkGenerator generator, long seed) {
        // As ChunkMap creates it for the ServerChunkCache.
        NoiseGeneratorSettings settings = generator.generatorSettings().value();
        RandomState random = RandomState.create(settings, game.access.lookupOrThrow(Registries.NOISE), seed);
        settingsOf.put(random, settings);
        return random;
    }

    @Override
    public ChunkBiomes chunkBiomes(RandomState random, BiomeSource source, int qx0, int qy0, int qz0, int width,
            int height, boolean targets) {
        NoiseGeneratorSettings settings = settingsOf.get(random);
        if (settings == null) {
            throw new IllegalArgumentException("a random state this adapter didn't create");
        }
        // NoiseBasedChunkGenerator.doCreateBiomes: the chunk's NoiseChunk (NoiseChunk.forChunk, here with no
        // structures and nothing blending), its cached climate sampler, and ChunkAccess.fillBiomesFromNoise asking
        // the source (the blender and below-zero retrogen leave it as is) for each quart with that sampler.
        var level = LevelHeightAccessor.create(QuartPos.toBlock(qy0), QuartPos.toBlock(height));
        NoiseChunk chunk = noiseChunk(random, settings, level, QuartPos.toBlock(qx0), QuartPos.toBlock(qz0));
        Climate.Sampler sampler = cachedClimateSampler(chunk, random, settings);
        return new ChunkBiomes() {
            @Override
            public Holder<Biome> biome(int qx, int qy, int qz) {
                return source.getNoiseBiome(qx, qy, qz, sampler);
            }

            @Override
            public Climate.TargetPoint target(int qx, int qy, int qz) {
                if (!targets) {
                    throw new UnsupportedOperationException("targets weren't asked for");
                }
                // What MultiNoiseBiomeSource.getNoiseBiome(qx, qy, qz, sampler) looks up.
                return sampler.sample(qx, qy, qz);
            }

            @Override
            public void close() {}
        };
    }

    /** {@code NoiseChunk.forChunk} for the chunk at block ({@code x0}, {@code z0}): no structures, nothing blending. */
    static NoiseChunk noiseChunk(RandomState random, NoiseGeneratorSettings settings, LevelHeightAccessor level,
            int x0, int z0) {
        var noise = settings.noiseSettings().clampToHeightAccessor(level);
        return new NoiseChunk(16 / noise.getCellWidth(), random, x0, z0, noise,
                noBeardifier(), settings, fluidPicker(settings), Blender.empty());
    }

    /** {@code NoiseChunk.cachedClimateSampler(router, spawnTarget)}, as doCreateBiomes asks for it. */
    static Climate.Sampler cachedClimateSampler(NoiseChunk chunk, RandomState random,
            NoiseGeneratorSettings settings) {
        try {
            return (Climate.Sampler) CACHED_CLIMATE_SAMPLER.invoke(chunk, random.router(), settings.spawnTarget());
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("NoiseChunk.cachedClimateSampler failed", e);
        }
    }

    /** {@code NoiseChunk.getInterpolatedDensity}: the cell-interpolated final density at the chunk's block. */
    static double interpolatedDensity(NoiseChunk chunk) {
        try {
            return (double) INTERPOLATED_DENSITY.invoke(chunk);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("NoiseChunk.getInterpolatedDensity failed", e);
        }
    }

    /** The generator's fluid picker ({@code NoiseBasedChunkGenerator.createFluidPicker}). */
    static Aquifer.FluidPicker fluidPicker(NoiseGeneratorSettings settings) {
        try {
            return (Aquifer.FluidPicker) CREATE_FLUID_PICKER.invoke(null, settings);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("can't build the generator's fluid picker", e);
        }
    }

    /**
     * A new {@code RandomState$1NoiseWiringHelper}, the visitor {@code RandomState}'s constructor maps the router
     * with, for wiring other functions to the same seed. It memoizes, so one per random state.
     */
    static DensityFunction.Visitor wiring(RandomState random, long seed, NoiseGeneratorSettings settings) {
        try {
            return (DensityFunction.Visitor) WIRING_HELPER.newInstance(random, seed,
                    settings.useLegacyRandomSource());
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("can't build RandomState's noise wiring", e);
        }
    }

    @Override
    public boolean halfOpenClimate() {
        return false;
    }

    @Override
    public long fitness(Climate.ParameterPoint point, Climate.TargetPoint target) {
        // Private in 26.2.
        try {
            return (long) FITNESS.invokeExact(point, target);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException(e);
        }
    }

    @Override
    public int baseHeight(NoiseBasedChunkGenerator generator, int x, int z, Heightmap.Types type,
            LevelHeightAccessor height, RandomState random) {
        return generator.getBaseHeight(x, z, type, height, random);
    }

    @Override
    public BlockState[] baseColumn(NoiseBasedChunkGenerator generator, int x, int z, LevelHeightAccessor height,
            RandomState random) {
        NoiseColumn column = generator.getBaseColumn(x, z, height, random);
        var blocks = new BlockState[height.getHeight()];
        for (int i = 0; i < blocks.length; i++) {
            blocks[i] = column.getBlock(height.getMinY() + i);
        }
        return blocks;
    }

    @Override
    public SpawnRun spawn(NoiseBasedChunkGenerator generator, RandomState random, int[][] samples)
            throws ReflectiveOperationException {
        // MinecraftServer.setInitialSpawn: the random state's own sampler (markers stripped, no NoiseChunk).
        Climate.Sampler sampler = random.sampler();
        ChunkPos origin = ChunkPos.containing(sampler.findSpawnPosition());
        List<Climate.ParameterPoint> targets = sampler.spawnTarget();
        if (targets.isEmpty()) {
            return new SpawnRun(origin, null, null);
        }

        // Climate.findSpawnPosition: the SpawnFinder constructor runs the whole search; rerun it phase by phase
        // on the same instance through its own private methods: the fitness at (0, 0), then radialSearch with
        // (2048, 512) and (512, 32).
        Object finder = SPAWN_FINDER.newInstance(targets, sampler);
        BlockPos found = (BlockPos) SPAWN_LOCATION.invoke(SPAWN_RESULT.get(finder));
        SPAWN_RESULT.set(finder, SPAWN_FITNESS.invoke(null, targets, sampler, 0, 0));
        var phases = new ArrayList<SpawnResult>();
        phases.add(spawnResult(SPAWN_RESULT.get(finder)));
        RADIAL_SEARCH.invoke(finder, targets, sampler, 2048.0f, 512.0f);
        phases.add(spawnResult(SPAWN_RESULT.get(finder)));
        RADIAL_SEARCH.invoke(finder, targets, sampler, 512.0f, 32.0f);
        Object last = SPAWN_RESULT.get(finder);
        phases.add(spawnResult(last));
        if (!SPAWN_LOCATION.invoke(last).equals(found) || !ChunkPos.containing(found).equals(origin)) {
            throw new IllegalStateException("the replayed search disagrees with the game's: " + found);
        }

        var scored = new ArrayList<SpawnResult>();
        for (int[] p : samples) {
            scored.add(spawnResult(SPAWN_FITNESS.invoke(null, targets, sampler, p[0], p[1])));
        }
        return new SpawnRun(origin, phases, scored);
    }

    /** A {@code Climate$SpawnFinder$Result}. */
    private static SpawnResult spawnResult(Object result) throws ReflectiveOperationException {
        var pos = (BlockPos) SPAWN_LOCATION.invoke(result);
        return new SpawnResult(pos.getX(), pos.getZ(), (long) SPAWN_RESULT_FITNESS.invoke(result));
    }

    @Override
    public Climate.Sampler structureSampler(RandomState random) {
        // 26.2's structure code reads the random state's sampler itself; this is that sampler.
        return random.sampler();
    }

    @Override
    public StructureTry tryStructure(Game game, ChunkGenerator generator, RandomState random,
            Climate.Sampler sampler, StructureTemplateManager templates, LevelHeightAccessor height,
            ResourceKey<Level> level, long seed, Holder<Structure> holder, ChunkPos chunk)
            throws ReflectiveOperationException {
        Structure structure = holder.value();
        Predicate<Holder<Biome>> valid = structure.biomes()::contains;

        // findGenerationPoint on its own context, then the biome check on the stub.
        var context = new Structure.GenerationContext(game.access, generator, generator.getBiomeSource(), random,
                templates, seed, chunk, height, valid);
        @SuppressWarnings("unchecked")
        var stub = (Optional<Structure.GenerationStub>) FIND_GENERATION_POINT.invoke(structure, context);
        BlockPos pos = null;
        Holder<Biome> biome = null;
        Boolean validBiome = null;
        if (stub.isPresent()) {
            pos = stub.get().position();
            // Structure.isValidBiome's lookup.
            biome = generator.getBiomeSource().getNoiseBiome(QuartPos.fromBlock(pos.getX()),
                    QuartPos.fromBlock(pos.getY()), QuartPos.fromBlock(pos.getZ()), random.sampler());
            validBiome = (Boolean) IS_VALID_BIOME.invoke(null, stub.get(), context);
        }

        // What the game itself does: ChunkGenerator.tryGenerateStructure, with no references yet.
        StructureStart start = structure.generate(holder, level, game.access, generator, generator.getBiomeSource(),
                random, templates, seed, chunk, 0, height, valid);
        return new StructureTry(pos, biome, validBiome, start.isValid() ? start : null);
    }

    @Override
    public JsonArray probeNoise(Game game, List<String> seeds) {
        return ProbeWorldgen.noise(game, seeds);
    }

    @Override
    public JsonArray probeDensity(Game game, List<String> seeds) {
        return ProbeWorldgen.density(game, seeds);
    }

    @Override
    public JsonArray probeTerrain(Game game, List<String> seeds) {
        return ProbeWorldgen.terrain(game, seeds);
    }
}
