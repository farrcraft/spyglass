package spyglass.oracle;

import net.minecraft.core.HolderGetter;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

/**
 * How a {@code RandomState} is created, apart from {@code Calls} because it changes on its own
 * (ADR 0029).
 */
final class RandomStates {
    private RandomStates() {}

    /** As {@code ChunkMap} creates it for a level. 26.3 doesn't use the level's height. */
    static RandomState forGenerator(HolderGetter<NormalNoise> noises, long seed, NoiseBasedChunkGenerator generator,
            LevelHeightAccessor level) {
        return RandomState.create(noises, seed, generator.generatorSettings().value());
    }

    /** From the noise settings alone, for probing one function. */
    static RandomState forSettings(HolderGetter<NormalNoise> noises, long seed, NoiseGeneratorSettings settings) {
        return RandomState.create(noises, seed, settings);
    }
}
