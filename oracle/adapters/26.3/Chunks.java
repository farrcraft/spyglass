package spyglass.oracle;

import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Predicate;

import it.unimi.dsi.fastutil.objects.ObjectOpenHashSet;

import net.minecraft.core.Holder;
import net.minecraft.server.level.WorldGenRegion;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.Biome;
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
 * A chunk generated as the server's {@code ChunkStatusTasks.buildTerrain} generates it (noise,
 * the material rule and carvers in one step), without a server: the generator's own
 * {@code buildTerrain}, given a stand-in region and structure manager. See
 * {@code docs/specs/oracle.md#block-golden-files}.
 */
final class Chunks {
    private Chunks() {}

    /**
     * The neighbourhood of one chunk: fresh proto chunks, with biomes made the first time a
     * chunk's biomes are wanted, as the BIOMES status does.
     */
    static final class Neighbourhood {
        private final Game game;
        private final NoiseBasedChunkGenerator generator;
        private final LevelHeightAccessor height;
        private final RandomState random;
        private final PalettedContainerFactory containers;
        private final StructureManager structures;
        private final Map<ChunkPos, ProtoChunk> chunks = new HashMap<>();
        private final Set<ChunkPos> withBiomes = new java.util.HashSet<>();

        Neighbourhood(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor height,
                RandomState random) {
            this.game = game;
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
                // As the BIOMES step leaves it: its biomes can be read from now on.
                chunk.setPersistedStatus(ChunkStatus.BIOMES);
            }
            return chunk;
        }
    }

    /** Generates the chunk at {@code pos} through {@code buildTerrain}, and returns it. */
    static ChunkAccess generate(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor height,
            RandomState random, long seed, ChunkPos pos) {
        var n = new Neighbourhood(game, generator, height, random);
        ProtoChunk chunk = n.withBiomes(pos.x(), pos.z());
        // ChunkStatusTasks.collectPossibleBiomes(region, 1).
        Set<Holder<Biome>> biomes = new ObjectOpenHashSet<>();
        for (int z = pos.z() - 1; z <= pos.z() + 1; z++) {
            for (int x = pos.x() - 1; x <= pos.x() + 1; x++) {
                n.withBiomes(x, z).collectBiomesInPalette(biomes);
            }
        }
        // WorldGenRegion's biome manager: the region's noise biomes, with the obfuscated seed.
        var biomeManager = new BiomeManager(
                (qx, qy, qz) -> n.withBiomes(qx >> 2, qz >> 2).getNoiseBiome(qx, qy, qz),
                BiomeManager.obfuscateSeed(seed));
        Region region = StandIns.allocate(Region.class);
        region.neighbourhood = n;
        region.center = pos;
        return generator.buildTerrain(chunk, Blender.empty(), random, n.structures, biomeManager, region, biomes)
                .join();
    }

    /**
     * A {@code WorldGenRegion} that only hands out the neighbourhood's chunks. Made without its
     * constructor, which needs a server level; any method it doesn't override that reads the
     * region's own state fails, so a step that starts using more of it is noticed.
     */
    static class Region extends WorldGenRegion {
        Neighbourhood neighbourhood;
        ChunkPos center;

        /** Never run: instances are made by {@link StandIns#allocate}. */
        Region() {
            super(null, null, null, null);
        }

        @Override
        public ChunkAccess getChunk(int x, int z) {
            return neighbourhood.chunk(x, z);
        }

        @Override
        public ChunkAccess getChunk(int x, int z, ChunkStatus status, boolean required) {
            return neighbourhood.chunk(x, z);
        }

        @Override
        public ChunkPos getCenter() {
            return center;
        }
    }

    /** A {@code StructureManager} with no structures: the chunks have no starts or references. */
    static class NoStructures extends StructureManager {
        /** Never run: instances are made by {@link StandIns#allocate}. */
        NoStructures() {
            super(null, null, null);
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
