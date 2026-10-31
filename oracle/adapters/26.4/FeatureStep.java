package spyglass.oracle;

import java.lang.reflect.Proxy;
import java.util.EnumSet;

import net.minecraft.core.BlockPos;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.core.Holder;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.storage.LevelData;

/**
 * 26.4's {@code ChunkStatusTasks} around decoration, and the server level the region is built
 * over. See 26.3's for the rest: 26.4 drops the chunk's noise biomes before decorating, its level
 * no longer takes the seed in its constructor, and the region asks it for uncached biomes by
 * {@code getUncachedBiome}.
 */
final class FeatureStep {
    private FeatureStep() {}

    /**
     * The rest of {@code ChunkStatusTasks.buildTerrain}, after the generator's: no retrogen in a
     * new chunk, then the heightmaps features read.
     */
    static void afterTerrain(ChunkAccess chunk) {
        Heightmap.primeHeightmaps(chunk, EnumSet.of(Heightmap.Types.MOTION_BLOCKING,
                Heightmap.Types.MOTION_BLOCKING_NO_LEAVES, Heightmap.Types.OCEAN_FLOOR, Heightmap.Types.WORLD_SURFACE));
    }

    /** {@code generateFeatures}' {@code dropNoiseBiomes}: the noise biomes aren't kept past it. */
    static void beforeFeatures(ChunkAccess center) {
        if (center instanceof ProtoChunk proto) {
            proto.setNoiseBiomeChunk(null);
        }
    }

    /** Zero, false or null, as a value of {@code type}. */
    static Object zero(Class<?> type) {
        if (type == long.class) {
            return 0L;
        } else if (type == int.class) {
            return 0;
        } else if (type == boolean.class) {
            return false;
        } else if (type == float.class) {
            return 0f;
        } else if (type == double.class) {
            return 0.0;
        }
        return null;
    }

    /** The server level, for what the region asks of it. */
    static class Server extends ServerLevel {
        long seed;
        NoiseBasedChunkGenerator generator;
        RandomState random;
        DimensionType type;
        Game game;
        LevelHeightAccessor height;
        Decoration.Cache cache;
        int entities;

        /** Never run: instances are made by {@link StandIns#allocate}. */
        Server() {
            super(null, null, null, null, null, null, false, null, false);
        }

        @Override
        public long getSeed() {
            return seed;
        }

        /** A new world's: game time 0, as ticks scheduled during generation read it. */
        @Override
        public LevelData getLevelData() {
            return (LevelData) Proxy.newProxyInstance(LevelData.class.getClassLoader(),
                    new Class<?>[] {LevelData.class}, (proxy, method, args) -> zero(method.getReturnType()));
        }

        @Override
        public ServerChunkCache getChunkSource() {
            return cache;
        }

        @Override
        public DimensionType dimensionType() {
            return type;
        }

        /** For chunks outside the region, which decoration doesn't read: fails if it's asked. */
        @Override
        public Holder<Biome> getUncachedBiome(int x, int y, int z) {
            throw new UnsupportedOperationException("a biome outside the region, at quart " + x + ", " + z);
        }

        @Override
        public FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public net.minecraft.core.RegistryAccess registryAccess() {
            return game.access;
        }

        @Override
        public int getMinY() {
            return height.getMinY();
        }

        @Override
        public int getHeight() {
            return height.getHeight();
        }

        @Override
        public int getSeaLevel() {
            return generator.getSeaLevel();
        }

        @Override
        public void updatePOIOnBlockStateChange(BlockPos pos, BlockState before, BlockState after) {}

        /** Only for its structure templates, which fossils place. */
        @Override
        public MinecraftServer getServer() {
            return StandIns.allocate(Decoration.GameServer.class);
        }

        /** No chunk is lit yet: {@link Unlit}. */
        @Override
        public net.minecraft.world.level.lighting.LevelLightEngine getLightEngine() {
            Unlit light = StandIns.allocate(Unlit.class);
            light.sky = type.hasSkyLight();
            return light;
        }

        /** Entities placed by features (the End's crystals) are numbered from 1. */
        @Override
        public int getNextEntityId() {
            return ++entities;
        }
    }

}
