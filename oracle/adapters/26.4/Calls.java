package spyglass.oracle;

import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.ResourceKey;
import net.minecraft.util.context.ContextMap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.NoiseBiomeResolver;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseColumn;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

/**
 * 26.4's forms of the calls the 26.3 adapter keeps in its {@code Calls} (ADR 0029). The rest of
 * the 26.3 adapter compiles unchanged against 26.4.
 */
final class Calls {
    private Calls() {}

    /** {@code Climate.Parameter.distance} adds 1 above the range from 26.4 on. */
    static final boolean HALF_OPEN_CLIMATE = true;

    /** A chunk's biome resolver, as a function of quart position. */
    interface QuartBiomes {
        Holder<Biome> at(int qx, int qy, int qz);
    }

    static QuartBiomes resolver(BiomeSource source, Climate.Sampler sampler, int qx0, int qy0, int qz0, int width,
            int height) {
        NoiseBiomeResolver resolver = source.createResolverForChunk(sampler, qx0, qy0, qz0, width, height, width);
        return resolver::getNoiseBiome;
    }

    /** 26.4 has no {@code getBaseHeight}; {@code getFirstFreeHeight} is what callers use instead. */
    static int baseHeight(NoiseBasedChunkGenerator generator, int x, int z, Heightmap.Types type,
            LevelHeightAccessor height, RandomState random) {
        return generator.getFirstFreeHeight(x, z, type, height, random);
    }

    /**
     * The column bottom up; null where the block is solid, which 26.4's column leaves to the
     * material rule ({@code NoiseColumn.SOLID}).
     */
    static BlockState[] baseColumn(NoiseBasedChunkGenerator generator, int x, int z, LevelHeightAccessor height,
            RandomState random) {
        NoiseColumn column = generator.getBaseColumn(x, z, height, random);
        var blocks = new BlockState[height.getHeight()];
        for (int i = 0; i < blocks.length; i++) {
            int y = height.getMinY() + i;
            blocks[i] = column.isSolid(y) ? null : column.getBlock(y);
        }
        return blocks;
    }

    /** The biome a structure's generation context sees at a quart. */
    static Holder<Biome> contextBiome(Structure.GenerationContext context, int qx, int qy, int qz) {
        return context.noiseBiomeResolver().getNoiseBiome(qx, qy, qz);
    }

    /** {@code Structure.generate} as {@code StructureSet.tryGenerateStartInChunk} calls it; null when no start. */
    static StructureStart generate(Structure structure, Holder<Structure> holder, ResourceKey<Level> level,
            RegistryAccess access, ChunkGenerator generator, Climate.Sampler sampler, RandomState random,
            StructureTemplateManager templates, long seed, ChunkPos chunk, LevelHeightAccessor height,
            java.util.function.Predicate<Holder<Biome>> valid) {
        return structure.generate(holder, level, access, generator, generator.getBiomeSource(), sampler, random,
                templates, seed, chunk, height, valid);
    }

    /**
     * A noise chunk over {@code volume} with no structures and no blending, for the aquifer probe:
     * {@code NoiseBasedChunkGenerator.createNoiseChunk} with an empty context, as its base column does.
     */
    static NoiseChunk probeChunk(RandomState state, NoiseGeneratorSettings settings, Aquifer.FluidPicker picker,
            DensityVolume volume) {
        return new NoiseChunk(state, volume, ContextMap.EMPTY, settings.noiseRouter().finalDensity(),
                settings.aquifers().orElse(null), picker);
    }
}
