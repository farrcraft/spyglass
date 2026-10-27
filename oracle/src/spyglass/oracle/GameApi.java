package spyglass.oracle;

import java.util.List;

import com.google.gson.JsonArray;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.resources.ResourceKey;
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
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

/**
 * Everything the harness asks the game that differs between versions (ADR 0027). The core calls
 * only this, and game types every supported version has; each adapter in {@code oracle/adapters/}
 * implements it as {@code Adapter} for its range of data versions, and {@code cargo xtask oracle}
 * compiles the core with the one adapter whose ranges hold the jar's data version.
 */
interface GameApi {
    /** The adapter compiled in. */
    static GameApi get() {
        return Adapter.INSTANCE;
    }

    /**
     * {@code WorldLoader.load}'s registries, and the Default preset's dimensions, from the vanilla
     * data pack and then each of {@code packs}, data pack folders.
     */
    Game load(List<java.nio.file.Path> packs);

    /**
     * The generator's random state for {@code seed}, as {@code ChunkMap} creates it for a level of
     * {@code level}'s height.
     */
    RandomState randomState(Game game, NoiseBasedChunkGenerator generator, LevelHeightAccessor level, long seed);

    /**
     * A chunk's quart biomes as the game resolves them for chunk generation, and, for a
     * multi-noise source, the climate target behind each. Close it when done.
     */
    ChunkBiomes chunkBiomes(RandomState random, BiomeSource source, int qx0, int qy0, int qz0, int width,
            int height, boolean targets);

    /** A chunk's biomes; see {@link #chunkBiomes}. Cells are asked for in the core's order. */
    interface ChunkBiomes extends AutoCloseable {
        /** The game's biome at a quart, from the chunk's resolver. */
        Holder<Biome> biome(int qx, int qy, int qz);

        /** The climate target at a quart; only when asked for. */
        Climate.TargetPoint target(int qx, int qy, int qz);

        @Override
        void close();
    }

    /**
     * Whether {@code Climate.Parameter.distance} treats ranges as half-open, adding 1 above the
     * range (26.4 on; {@code docs/specs/biome-lookup.md#fitness}).
     */
    boolean halfOpenClimate();

    /** {@code ParameterPoint.fitness}: the game's own distance from a point to a target. */
    long fitness(Climate.ParameterPoint point, Climate.TargetPoint target);

    /** The generator's height for a heightmap before any chunk exists ({@code getBaseHeight}). */
    int baseHeight(NoiseBasedChunkGenerator generator, int x, int z, Heightmap.Types type,
            LevelHeightAccessor height, RandomState random);

    /** The generator's block column at (x, z), bottom up, one block per level. */
    BlockState[] baseColumn(NoiseBasedChunkGenerator generator, int x, int z, LevelHeightAccessor height,
            RandomState random);

    /**
     * The spawn search replayed phase by phase: the origin chunk, then, when the dimension has spawn
     * targets, the best result after each phase and the fitness at each of {@code samples}.
     */
    SpawnRun spawn(NoiseBasedChunkGenerator generator, RandomState random, int[][] samples)
            throws ReflectiveOperationException;

    /** A spawn search; {@code phases} and {@code samples} are null without targets. */
    record SpawnRun(ChunkPos origin, List<SpawnResult> phases, List<SpawnResult> samples) {}

    /** A position the spawn search scored, at block (x, z), and its fitness. */
    record SpawnResult(int x, int z, long fitness) {}

    /** The climate sampler {@code ChunkGenerator.createStructures} gives structures. */
    Climate.Sampler structureSampler(RandomState random);

    /**
     * One structure tried in a placement chunk: {@code findGenerationPoint} and the biome check on
     * its own context, then what {@code ChunkGenerator.tryGenerateStructure} does.
     */
    StructureTry tryStructure(Game game, ChunkGenerator generator, RandomState random, Climate.Sampler sampler,
            StructureTemplateManager templates, LevelHeightAccessor height, ResourceKey<Level> level, long seed,
            Holder<Structure> structure, ChunkPos chunk) throws ReflectiveOperationException;

    /**
     * A structure try: its stub's position, biome and biome check (all null without a stub), and the
     * start, null when the game made none.
     */
    record StructureTry(BlockPos stub, Holder<Biome> biome, Boolean validBiome, StructureStart start) {}

    /** The probe's noise cases ({@code docs/specs/oracle.md}, "Probe files"). */
    JsonArray probeNoise(Game game, List<String> seeds);

    /** The probe's density cases, every dimension. */
    JsonArray probeDensity(Game game, List<String> seeds);

    /** The probe's terrain (aquifer) cases. */
    JsonArray probeTerrain(Game game, List<String> seeds);
}
