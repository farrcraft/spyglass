package spyglass.oracle;

import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Predicate;

import it.unimi.dsi.fastutil.objects.ObjectOpenHashSet;

import net.minecraft.core.Holder;
import net.minecraft.core.QuartPos;
import net.minecraft.server.level.WorldGenRegion;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.NoiseBiomeResolver;
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
 * 26.4's chunk steps, as the server's {@code ChunkStatusTasks} runs them: noise biomes for each
 * chunk ({@code generateNoiseBiomes}), its biomes upscaled from its neighbours' noise biomes
 * ({@code upscaleBiomes}), then {@code buildTerrain}, which no longer takes a biome manager and
 * collects the possible biomes from the chunk alone. See 26.3's {@code Chunks} for the stand-ins.
 */
final class Chunks {
    private Chunks() {}

    /** The neighbourhood of one chunk: fresh proto chunks, each step done once when first needed. */
    static final class Neighbourhood {
        private final NoiseBasedChunkGenerator generator;
        private final LevelHeightAccessor height;
        private final RandomState random;
        private final PalettedContainerFactory containers;
        private final StructureManager structures;
        private final Map<ChunkPos, ProtoChunk> chunks = new HashMap<>();
        private final Set<ChunkPos> withNoiseBiomes = new HashSet<>();
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

        /** {@code ChunkStatusTasks.generateNoiseBiomes}. */
        ProtoChunk withNoiseBiomes(int x, int z) {
            ProtoChunk chunk = chunk(x, z);
            if (withNoiseBiomes.add(chunk.getPos())) {
                generator.createNoiseBiomes(random, Blender.empty(), chunk, containers).join();
                chunk.setPersistedStatus(ChunkStatus.NOISE_BIOMES);
            }
            return chunk;
        }

        /**
         * {@code ChunkStatusTasks.upscaleBiomes}: the resolver of {@code makeNoiseBiomeResolver},
         * each quart's noise biome from the chunk it's in, within one chunk.
         */
        ProtoChunk withBiomes(int x, int z) {
            ProtoChunk chunk = withNoiseBiomes(x, z);
            if (withBiomes.add(chunk.getPos())) {
                NoiseBiomeResolver resolver = (qx, qy, qz) -> {
                    int cx = QuartPos.toSection(qx);
                    int cz = QuartPos.toSection(qz);
                    if (Math.abs(cx - x) > 1 || Math.abs(cz - z) > 1) {
                        throw new IllegalStateException("upscaling asked outside its neighbours");
                    }
                    return withNoiseBiomes(cx, cz).getNoiseBiomeChunk().getNoiseBiome(qx, qy, qz);
                };
                generator.createBiomes(resolver, random, chunk).join();
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
        Set<Holder<Biome>> biomes = chunk.collectBiomesInPalette(new ObjectOpenHashSet<>());
        Region region = StandIns.allocate(Region.class);
        region.neighbourhood = n;
        region.center = pos;
        return generator.buildTerrain(chunk, Blender.empty(), random, n.structures, region, biomes).join();
    }

    /** A {@code WorldGenRegion} that only hands out the neighbourhood's chunks; see 26.3's. */
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
