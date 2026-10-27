package spyglass.oracle;

import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.NoiseColumn;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

/**
 * The game calls whose signatures change between versions, kept apart so an adapter that extends
 * this one replaces only this file (ADR 0029).
 */
final class Calls {
    private Calls() {}

    /** Whether {@code Climate.Parameter.distance} is half-open: not until 26.4. */
    static final boolean HALF_OPEN_CLIMATE = false;

    /** A chunk's biome resolver, as a function of quart position. */
    interface QuartBiomes {
        Holder<Biome> at(int qx, int qy, int qz);
    }

    static QuartBiomes resolver(BiomeSource source, Climate.Sampler sampler, int qx0, int qy0, int qz0, int width,
            int height) {
        BiomeResolver resolver = source.createResolverForChunk(sampler, qx0, qy0, qz0, width, height, width);
        return resolver::getNoiseBiome;
    }

    static int baseHeight(NoiseBasedChunkGenerator generator, int x, int z, Heightmap.Types type,
            LevelHeightAccessor height, RandomState random) {
        return generator.getBaseHeight(x, z, type, height, random);
    }

    /** The column bottom up; null where a version leaves a solid block undecided. */
    static BlockState[] baseColumn(NoiseBasedChunkGenerator generator, int x, int z, LevelHeightAccessor height,
            RandomState random) {
        NoiseColumn column = generator.getBaseColumn(x, z, height, random);
        var blocks = new BlockState[height.getHeight()];
        for (int i = 0; i < blocks.length; i++) {
            blocks[i] = column.getBlock(height.getMinY() + i);
        }
        return blocks;
    }

    /** The biome a structure's generation context sees at a quart. */
    static Holder<Biome> contextBiome(Structure.GenerationContext context, int qx, int qy, int qz) {
        return context.biomeResolver().getNoiseBiome(qx, qy, qz);
    }

    /** {@code Structure.generate} as {@code tryGenerateStructure} calls it; null when no start. */
    static StructureStart generate(Structure structure, Holder<Structure> holder, ResourceKey<Level> level,
            RegistryAccess access, ChunkGenerator generator, Climate.Sampler sampler, RandomState random,
            StructureTemplateManager templates, long seed, ChunkPos chunk, LevelHeightAccessor height,
            java.util.function.Predicate<Holder<Biome>> valid) {
        var start = structure.generate(holder, level, access, generator, generator.getBiomeSource(), sampler,
                random, templates, seed, chunk, 0, height, valid);
        return start.isValid() ? start : null;
    }

    /** A noise chunk over {@code volume} with no structures and no blending, for the aquifer probe. */
    static NoiseChunk probeChunk(RandomState state, NoiseGeneratorSettings settings, Aquifer.FluidPicker picker,
            DensityVolume volume) {
        return new NoiseChunk(state, null, settings, picker, Blender.empty(), volume);
    }
}
