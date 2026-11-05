package spyglass.oracle;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Proxy;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.function.Predicate;
import java.util.function.Supplier;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.Registries;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.GenerationChunkHolder;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.WorldGenRegion;
import net.minecraft.util.StaticCache2D;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.status.ChunkPyramid;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.chunk.status.ChunkStep;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.feature.Feature;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.util.datafix.DataFixers;

/**
 * Decoration without a server, for feature golden files ({@code docs/specs/oracle.md}): the
 * target features wrapped to report their calls, and a real {@code WorldGenRegion} over
 * stand-ins for the server level, its chunk cache and the chunk holders ({@link StandIns}).
 */
final class Decoration {
    /** The data packs' structure templates, for fossils; made by {@link #record}. */
    private static StructureTemplateManager templates;

    private Decoration() {}

    /**
     * 26.3's {@code Feature} is an interface: a proxy reports {@code place} and delegates. Each
     * placed feature's feature is a registry entry, whose value is swapped for the proxy.
     */
    static void record(Game game, Set<String> placed) throws Exception {
        // As Structures makes its own: the storage stays open while templates are read.
        var storage = LevelStorageSource.createDefault(java.nio.file.Path.of("oracle-levels")).createAccess("features");
        templates = new StructureTemplateManager(game.resources, storage, DataFixers.getDataFixer(),
                game.access.lookupOrThrow(Registries.BLOCK));
        Field value = Holder.Reference.class.getDeclaredField("value");
        value.setAccessible(true);
        var registry = game.access.lookupOrThrow(Registries.PLACED_FEATURE);
        Set<Holder<Feature>> wrapped = new java.util.HashSet<>();
        for (String id : placed) {
            var entry = registry.get(net.minecraft.resources.Identifier.parse(id)).orElseThrow(
                    () -> new IllegalArgumentException("no placed feature " + id));
            Holder<Feature> holder = entry.value().feature();
            if (!(holder instanceof Holder.Reference<Feature> reference)) {
                throw new IllegalArgumentException(id + "'s feature is inline, so it can't be wrapped");
            }
            if (wrapped.add(holder)) {
                value.set(reference, wrap(reference.value()));
            }
        }
    }

    private static Feature wrap(Feature feature) {
        return (Feature) Proxy.newProxyInstance(Feature.class.getClassLoader(), new Class<?>[] {Feature.class},
                (proxy, method, args) -> {
                    boolean place = method.getName().equals("place") && args != null && args.length == 4
                            && args[0] instanceof WorldGenLevel && args[3] instanceof BlockPos;
                    var before = place ? Features.containers((WorldGenLevel) args[0], (BlockPos) args[3]) : null;
                    Object result;
                    try {
                        result = method.invoke(feature, args);
                    } catch (InvocationTargetException e) {
                        throw e.getCause();
                    }
                    if (place) {
                        Features.record((WorldGenLevel) args[0], (BlockPos) args[3], (Boolean) result, before);
                    }
                    return result;
                });
    }

    /**
     * {@code ChunkStatusTasks.generateFeatures} for the chunk at {@code pos}, with its neighbours
     * through {@code buildTerrain} and its heightmap priming. With {@code neighboursFirst}, the 5
     * x 5 chunks around it are generated and each of its 8 neighbours is decorated first, z then
     * x ascending, recording nothing; otherwise none is.
     */
    static void decorate(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor height,
            RandomState random, long seed, ChunkPos pos, boolean neighboursFirst) {
        int reach = neighboursFirst ? 2 : 1;
        Map<ChunkPos, ChunkAccess> chunks = new HashMap<>();
        for (int dz = -reach; dz <= reach; dz++) {
            for (int dx = -reach; dx <= reach; dx++) {
                ChunkPos at = new ChunkPos(pos.x() + dx, pos.z() + dz);
                ChunkAccess chunk = Chunks.generate(game, generator, height, random, seed, at);
                FeatureStep.afterTerrain(chunk);
                chunks.put(at, chunk);
            }
        }
        FeatureStep.Server level = StandIns.allocate(FeatureStep.Server.class);
        level.seed = seed;
        level.generator = generator;
        level.random = random;
        level.type = typeOf(game, generator);
        level.game = game;
        level.height = height;
        Cache cache = StandIns.allocate(Cache.class);
        cache.level = level;
        level.cache = cache;
        if (neighboursFirst) {
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    if (dx != 0 || dz != 0) {
                        ChunkPos neighbour = new ChunkPos(pos.x() + dx, pos.z() + dz);
                        Features.quiet(() -> decorateOne(level, generator, chunks, neighbour));
                    }
                }
            }
        }
        decorateOne(level, generator, chunks, pos);
    }

    /** One chunk's {@code generateFeatures}, over the chunks around it in {@code chunks}. */
    private static void decorateOne(FeatureStep.Server level, NoiseBasedChunkGenerator generator,
            Map<ChunkPos, ChunkAccess> chunks, ChunkPos pos) {
        ChunkStep step = ChunkPyramid.GENERATION_PYRAMID.getStepTo(ChunkStatus.FEATURES);
        var deps = step.directDependencies();
        for (int dz = -1; dz <= 1; dz++) {
            for (int dx = -1; dx <= 1; dx++) {
                ChunkAccess chunk = chunks.get(new ChunkPos(pos.x() + dx, pos.z() + dz));
                int distance = Math.max(Math.abs(dx), Math.abs(dz));
                ((ProtoChunk) chunk).setPersistedStatus(deps.get(Math.min(distance, deps.size() - 1)));
            }
        }
        StaticCache2D<GenerationChunkHolder> holders = StaticCache2D.create(pos.x(), pos.z(), 1, (x, z) -> {
            ChunkHolder holder = StandIns.allocate(ChunkHolder.class);
            holder.chunk = chunks.get(new ChunkPos(x, z));
            return holder;
        });
        ChunkAccess center = chunks.get(pos);
        FeatureStep.beforeFeatures(center);
        WorldGenRegion region = new Region(level, holders, step, center);
        Manager structures = StandIns.allocate(Manager.class);
        generator.applyBiomeDecoration(region, center, structures);
        Blender.generateBorderTicks(region, center);
    }

    /** The dimension type of the dimension {@code generator} is. */
    private static DimensionType typeOf(Game game, ChunkGenerator generator) {
        for (var stem : game.dimensions.dimensions().values()) {
            if (stem.generator() == generator) {
                return stem.type().value();
            }
        }
        throw new IllegalStateException("no dimension has this generator");
    }

    /** A region that reports what it's generating. */
    static final class Region extends WorldGenRegion {
        Region(ServerLevel level, StaticCache2D<GenerationChunkHolder> holders, ChunkStep step, ChunkAccess center) {
            super(level, holders, step, center);
        }

        @Override
        public void setCurrentlyGenerating(Supplier<String> what) {
            super.setCurrentlyGenerating(what);
            Features.generating(what);
        }
    }

    /** A server with only structure templates. */
    static class GameServer extends DedicatedServer {
        /** Never run: instances are made by {@link StandIns#allocate}. */
        GameServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        @Override
        public StructureTemplateManager getStructureTemplateManager() {
            return templates;
        }
    }

    /** The level's chunk source: only its random state and generator. */
    static class Cache extends ServerChunkCache {
        FeatureStep.Server level;

        /** Never run: instances are made by {@link StandIns#allocate}. */
        Cache() {
            super(null, null, null, null, null, null, 0, 0, false, null);
        }

        @Override
        public RandomState randomState() {
            return level.random;
        }

        @Override
        public ChunkGenerator getGenerator() {
            return level.generator;
        }
    }

    /** A chunk holder that hands out one chunk, whatever status is asked for. */
    static class ChunkHolder extends GenerationChunkHolder {
        ChunkAccess chunk;

        /** Never run: instances are made by {@link StandIns#allocate}. */
        ChunkHolder() {
            super(null);
        }

        @Override
        public ChunkAccess getChunkIfPresentUnchecked(ChunkStatus status) {
            return chunk;
        }

        @Override
        public ChunkAccess getChunkIfPresent(ChunkStatus status) {
            return chunk;
        }

        @Override
        protected void addSaveDependency(CompletableFuture<?> future) {}

        @Override
        public int getTicketLevel() {
            return 0;
        }

        @Override
        public int getQueueLevel() {
            return 0;
        }
    }

    /** No structures: none start or are referenced, so none place in any step. */
    static class Manager extends StructureManager {
        /** Never run: instances are made by {@link StandIns#allocate}. */
        Manager() {
            super(null, null, null);
        }

        @Override
        public boolean shouldGenerateStructures() {
            return false;
        }

        @Override
        public List<StructureStart> startsForStructure(int x, int z, Predicate<Structure> matches) {
            return List.of();
        }

        @Override
        public List<StructureStart> startsForStructure(int x, int z, Structure structure) {
            return List.of();
        }
    }
}
