package spyglass.oracle;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
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
import net.minecraft.network.chat.Component;
import net.minecraft.server.packs.PackLocationInfo;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.PathPackResources;
import net.minecraft.server.packs.repository.PackSource;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseSpawnFinder;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.SpawnTargetPoint;
import net.minecraft.world.level.levelgen.densityfunction.DensityBuffer;
import net.minecraft.world.level.levelgen.densityfunction.DensitySampler;
import net.minecraft.world.level.levelgen.densityfunction.DensitySamplerSet;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

/** {@link GameApi} for 26.3's game API: its density samplers, volumes and sampler contexts. */
final class Adapter implements GameApi {
    static final Adapter INSTANCE = new Adapter();

    private static final Constructor<NoiseSpawnFinder> SPAWN_FINDER;
    private static final Field SPAWN_RESULT;
    private static final Method RADIAL_SEARCH;
    private static final Method SPAWN_FITNESS;
    private static final Method SPAWN_LOCATION;
    private static final Method SPAWN_RESULT_FITNESS;
    private static final Method FIND_GENERATION_POINT;

    static {
        try {
            SPAWN_FINDER = NoiseSpawnFinder.class.getDeclaredConstructor(List.class, DensitySamplerSet.class);
            SPAWN_RESULT = NoiseSpawnFinder.class.getDeclaredField("result");
            RADIAL_SEARCH = NoiseSpawnFinder.class.getDeclaredMethod("radialSearch", DensitySamplerSet.class,
                    List.class, float.class, float.class);
            SPAWN_FITNESS = NoiseSpawnFinder.class.getDeclaredMethod("getSpawnPositionAndFitness",
                    DensitySamplerSet.class, List.class, int.class, int.class);
            Class<?> result = SPAWN_RESULT.getType();
            SPAWN_LOCATION = result.getDeclaredMethod("location");
            SPAWN_RESULT_FITNESS = result.getDeclaredMethod("fitness");
            FIND_GENERATION_POINT =
                    Structure.class.getDeclaredMethod("findGenerationPoint", Structure.GenerationContext.class);
            for (var m : List.of(SPAWN_FINDER, SPAWN_RESULT, RADIAL_SEARCH, SPAWN_FITNESS, SPAWN_LOCATION,
                    SPAWN_RESULT_FITNESS, FIND_GENERATION_POINT)) {
                m.setAccessible(true);
            }
        } catch (ReflectiveOperationException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    private Adapter() {}

    @Override
    public Game load(List<Path> packs) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // WorldLoader.load: static registries first, then tags from the data packs, then the
        // worldgen registries from the data packs into the WORLD layer.
        LayeredRegistryAccess<RegistryLayer> layers = RegistryLayer.createRegistryAccess();
        var all = new ArrayList<PackResources>();
        all.add(ServerPacksSource.createVanillaPackSource().fullResources());
        for (Path pack : packs) {
            String name = pack.getFileName().toString();
            all.add(new PathPackResources(
                    new PackLocationInfo("file/" + name, Component.literal(name), PackSource.DEFAULT, Optional.empty()),
                    pack));
        }
        var resources = new MultiPackResourceManager(PackType.SERVER_DATA, all);
        var tags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
        List<HolderLookup.RegistryLookup<?>> lookups =
                TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), tags);
        RegistryAccess.Frozen world =
                RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        RegistryAccess.Frozen access = layers.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
        // ReloadableServerResources.updateStaticRegistryTags: bind the static registries' tags
        // (blocks, items), which structure pieces read.
        tags.forEach(net.minecraft.core.Registry.PendingTags::apply);

        // What CreateWorldScreen and the dedicated server give a world with the Default preset.
        return new Game(access, resources, WorldPresets.createNormalWorldDimensions(access), packs);
    }

    @Override
    public RandomState randomState(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor level,
            long seed) {
        return RandomStates.forGenerator(game.access.lookupOrThrow(Registries.NOISE), seed, generator, level);
    }

    @Override
    public ChunkBiomes chunkBiomes(RandomState random, BiomeSource source, int qx0, int qy0, int qz0, int width,
            int height, boolean targets) {
        // ChunkGenerator.doCreateBiomes: a cached sampler on a pooled buffer arena per chunk.
        var pool = random.acquireDensityBufferPool();
        try {
            var context = SamplerContext.builder().enableCaches().useBufferArena(pool).build();
            Climate.Sampler sampler = random.createClimateSampler(context);
            Calls.QuartBiomes resolver = Calls.resolver(source, sampler, qx0, qy0, qz0, width, height);
            if (!targets) {
                return new ChunkBiomes() {
                    @Override
                    public Holder<Biome> biome(int qx, int qy, int qz) {
                        return resolver.at(qx, qy, qz);
                    }

                    @Override
                    public Climate.TargetPoint target(int qx, int qy, int qz) {
                        throw new UnsupportedOperationException("targets weren't asked for");
                    }

                    @Override
                    public void close() {
                        random.releaseDensityBufferPool(pool);
                    }
                };
            }
            // MultiNoiseBiomeSource.createResolverForChunk, keeping the targets.
            var volume = new DensityVolume(width, height, width, QuartPos.toBlock(qx0), QuartPos.toBlock(qy0),
                    QuartPos.toBlock(qz0), 4, 4, 4);
            DensityBuffer t = sampled(sampler.temperature(), volume);
            DensityBuffer h = sampled(sampler.humidity(), volume);
            DensityBuffer c = sampled(sampler.continentalness(), volume);
            DensityBuffer e = sampled(sampler.erosion(), volume);
            DensityBuffer d = sampled(sampler.depth(), volume);
            DensityBuffer w = sampled(sampler.weirdness(), volume);
            return new ChunkBiomes() {
                @Override
                public Holder<Biome> biome(int qx, int qy, int qz) {
                    return resolver.at(qx, qy, qz);
                }

                @Override
                public Climate.TargetPoint target(int qx, int qy, int qz) {
                    int i = volume.indexUnchecked(qx - qx0, qy - qy0, qz - qz0);
                    return Climate.target(t.get(i), h.get(i), c.get(i), e.get(i), d.get(i), w.get(i));
                }

                @Override
                public void close() {
                    random.releaseDensityBufferPool(pool);
                }
            };
        } catch (RuntimeException | Error e) {
            random.releaseDensityBufferPool(pool);
            throw e;
        }
    }

    private static DensityBuffer sampled(DensitySampler.Bound sampler, DensityVolume volume) {
        DensityBuffer buffer = DensityBuffer.createUnpooled(volume.size());
        sampler.sampleVolume(buffer, volume);
        return buffer;
    }

    @Override
    public boolean halfOpenClimate() {
        return Calls.HALF_OPEN_CLIMATE;
    }

    @Override
    public long fitness(Climate.ParameterPoint point, Climate.TargetPoint target) {
        return point.fitness(target);
    }

    @Override
    public int baseHeight(NoiseBasedChunkGenerator generator, int x, int z, Heightmap.Types type,
            LevelHeightAccessor height, RandomState random) {
        return Calls.baseHeight(generator, x, z, type, height, random);
    }

    @Override
    public BlockState[] baseColumn(NoiseBasedChunkGenerator generator, int x, int z, LevelHeightAccessor height,
            RandomState random) {
        return Calls.baseColumn(generator, x, z, height, random);
    }

    @Override
    public SpawnRun spawn(NoiseBasedChunkGenerator generator, RandomState random, int[][] samples)
            throws ReflectiveOperationException {
        ChunkPos origin = generator.getOrigin(random);
        List<SpawnTargetPoint> targets = generator.generatorSettings().value().spawnTarget();
        if (targets.isEmpty()) {
            return new SpawnRun(origin, null, null);
        }
        // NoiseBasedChunkGenerator.getOrigin's sampler set.
        DensitySamplerSet samplers = random.samplersWithContext(SamplerContext.builder().enableCaches().build());

        // The constructor runs the whole search; rerun it phase by phase on the same instance, through
        // NoiseSpawnFinder's own private methods: the fitness at (0, 0), then radialSearch with
        // (2048, 512) and (512, 32).
        NoiseSpawnFinder finder = SPAWN_FINDER.newInstance(targets, samplers);
        BlockPos found = (BlockPos) SPAWN_LOCATION.invoke(SPAWN_RESULT.get(finder));
        SPAWN_RESULT.set(finder, SPAWN_FITNESS.invoke(null, samplers, targets, 0, 0));
        var phases = new ArrayList<SpawnResult>();
        phases.add(spawnResult(SPAWN_RESULT.get(finder)));
        RADIAL_SEARCH.invoke(finder, samplers, targets, 2048.0f, 512.0f);
        phases.add(spawnResult(SPAWN_RESULT.get(finder)));
        RADIAL_SEARCH.invoke(finder, samplers, targets, 512.0f, 32.0f);
        Object last = SPAWN_RESULT.get(finder);
        phases.add(spawnResult(last));
        if (!SPAWN_LOCATION.invoke(last).equals(found) || !ChunkPos.containing(found).equals(origin)) {
            throw new IllegalStateException("the replayed search disagrees with the game's: " + found);
        }

        var scored = new ArrayList<SpawnResult>();
        for (int[] p : samples) {
            scored.add(spawnResult(SPAWN_FITNESS.invoke(null, samplers, targets, p[0], p[1])));
        }
        return new SpawnRun(origin, phases, scored);
    }

    /** A {@code NoiseSpawnFinder.Result}. */
    private static SpawnResult spawnResult(Object result) throws ReflectiveOperationException {
        var pos = (BlockPos) SPAWN_LOCATION.invoke(result);
        return new SpawnResult(pos.getX(), pos.getZ(), (long) SPAWN_RESULT_FITNESS.invoke(result));
    }

    @Override
    public Climate.Sampler structureSampler(RandomState random) {
        return random.createClimateSampler(SamplerContext.builder().enableCaches().build());
    }

    @Override
    public StructureTry tryStructure(Game game, ChunkGenerator generator, RandomState random,
            Climate.Sampler sampler, StructureTemplateManager templates, LevelHeightAccessor height,
            ResourceKey<Level> level, long seed, Holder<Structure> holder, ChunkPos chunk)
            throws ReflectiveOperationException {
        Structure structure = holder.value();
        Predicate<Holder<Biome>> valid = structure.biomes()::contains;

        // findGenerationPoint on its own context, then the biome check on the stub.
        var context = new Structure.GenerationContext(game.access, generator, generator.getBiomeSource(), sampler,
                random, templates, seed, chunk, height, valid);
        @SuppressWarnings("unchecked")
        var stub = (Optional<Structure.GenerationStub>) FIND_GENERATION_POINT.invoke(structure, context);
        BlockPos pos = null;
        Holder<Biome> biome = null;
        Boolean validBiome = null;
        if (stub.isPresent()) {
            pos = stub.get().position();
            biome = Calls.contextBiome(context, QuartPos.fromBlock(pos.getX()), QuartPos.fromBlock(pos.getY()),
                    QuartPos.fromBlock(pos.getZ()));
            validBiome = context.isValidBiome(stub.get());
        }

        // What the game itself does: ChunkGenerator.tryGenerateStructure.
        var start = Calls.generate(structure, holder, level, game.access, generator, sampler, random, templates, seed,
                chunk, height, valid);
        return new StructureTry(pos, biome, validBiome, start);
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
