package spyglass.oracle;

import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Predicate;

import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.server.level.WorldGenRegion;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;

/**
 * 26.2's chunk steps, as the server's {@code ChunkStatusTasks} runs them: biomes, then
 * {@code fillFromNoise}, {@code buildSurface} and {@code applyCarvers}, each the generator's own
 * public step, given a stand-in region and structure manager ({@link StandIns}).
 */
final class Chunks {
    private Chunks() {}

    /** The neighbourhood of one chunk: fresh proto chunks, with biomes made when first wanted. */
    static final class Neighbourhood {
        private final NoiseBasedChunkGenerator generator;
        private final LevelHeightAccessor height;
        private final RandomState random;
        private final PalettedContainerFactory containers;
        private final StructureManager structures;
        private final Map<ChunkPos, ProtoChunk> chunks = new HashMap<>();
        private final Set<ChunkPos> withBiomes = new HashSet<>();

        Neighbourhood(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor height,
                RandomState random) {
            this.generator = generator;
            this.height = height;
            this.random = random;
            this.containers = PalettedContainerFactory.create(game.access);
            this.structures = StandIns.allocate(NoStructures.class);
        }

        ProtoChunk chunk(int x, int z) {
            return chunks.computeIfAbsent(new ChunkPos(x, z),
                    pos -> new ProtoChunk(pos, UpgradeData.EMPTY, height, containers, null));
        }

        ProtoChunk withBiomes(int x, int z) {
            ProtoChunk chunk = chunk(x, z);
            if (withBiomes.add(chunk.getPos())) {
                generator.createBiomes(random, Blender.empty(), structures, chunk).join();
                chunk.setPersistedStatus(ChunkStatus.BIOMES);
            }
            return chunk;
        }
    }

    /** Generates the chunk at {@code pos} through its three steps, and returns it. */
    static ChunkAccess generate(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor height,
            RandomState random, long seed, ChunkPos pos) {
        var n = new Neighbourhood(game, generator, height, random);
        Region region = StandIns.allocate(Region.class);
        region.neighbourhood = n;
        region.center = pos;
        region.access = game.access;
        region.height = height;
        // WorldGenRegion's biome manager: the region's noise biomes, with the obfuscated seed.
        region.biomes = new BiomeManager(
                (qx, qy, qz) -> n.withBiomes(qx >> 2, qz >> 2).getNoiseBiome(qx, qy, qz),
                BiomeManager.obfuscateSeed(seed));
        ProtoChunk chunk = n.withBiomes(pos.x(), pos.z());
        generator.fillFromNoise(Blender.empty(), random, n.structures, chunk).join();
        chunk.setPersistedStatus(ChunkStatus.NOISE);
        generator.buildSurface(region, n.structures, random, chunk);
        chunk.setPersistedStatus(ChunkStatus.SURFACE);
        // The carver step is given the level's biome manager; carving replaces its source, so only
        // the seed it was made with matters.
        var levelBiomes = new BiomeManager((qx, qy, qz) -> {
            throw new IllegalStateException("the carvers read the level's biomes");
        }, BiomeManager.obfuscateSeed(seed));
        generator.applyCarvers(region, seed, random, levelBiomes, n.structures, chunk);
        chunk.setPersistedStatus(ChunkStatus.CARVERS);
        return chunk;
    }

    /**
     * A {@code WorldGenRegion} that only hands out the neighbourhood's chunks; see 26.3's. The
     * surface step collects biomes from the chunks next to the centre, so those come with biomes.
     */
    static class Region extends WorldGenRegion {
        Neighbourhood neighbourhood;
        ChunkPos center;
        RegistryAccess access;
        BiomeManager biomes;
        /** The level's heights, which a region reports as its own. */
        LevelHeightAccessor height;

        /** Never run: instances are made by {@link StandIns#allocate}. */
        Region() {
            super(null, null, null, null);
        }

        @Override
        public ChunkAccess getChunk(int x, int z) {
            boolean near = Math.abs(x - center.x()) <= 1 && Math.abs(z - center.z()) <= 1;
            return near ? neighbourhood.withBiomes(x, z) : neighbourhood.chunk(x, z);
        }

        @Override
        public ChunkAccess getChunk(int x, int z, ChunkStatus status, boolean required) {
            return getChunk(x, z);
        }

        @Override
        public ChunkPos getCenter() {
            return center;
        }

        @Override
        public BiomeManager getBiomeManager() {
            return biomes;
        }

        @Override
        public RegistryAccess registryAccess() {
            return access;
        }

        @Override
        public int getMinY() {
            return height.getMinY();
        }

        @Override
        public int getHeight() {
            return height.getHeight();
        }

        /** A new world: no chunks from an older version to blend with. */
        @Override
        public boolean isOldChunkAround(ChunkPos pos, int radius) {
            return false;
        }
    }

    /** A {@code StructureManager} with no structures: the chunks have no starts or references. */
    static class NoStructures extends StructureManager {
        /** Never run: instances are made by {@link StandIns#allocate}. */
        NoStructures() {
            super(null, null, null);
        }

        @Override
        public List<StructureStart> startsForStructure(ChunkPos pos, Predicate<Structure> matches) {
            return List.of();
        }

        @Override
        public List<StructureStart> startsForStructure(SectionPos pos, Structure structure) {
            return List.of();
        }
    }
}
