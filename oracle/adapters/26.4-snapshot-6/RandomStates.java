package spyglass.oracle;

import net.minecraft.core.HolderGetter;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

/**
 * 26.4-snapshot-6's {@code RandomState} factories. The state now carries the vertical anchors that
 * a {@code gradient}'s anchor coordinates resolve against.
 */
final class RandomStates {
    private RandomStates() {}

    /** As {@code ChunkMap} creates it, with anchors from the level's height. */
    static RandomState forGenerator(HolderGetter<NormalNoise> noises, long seed, NoiseBasedChunkGenerator generator,
            LevelHeightAccessor level) {
        return RandomState.createForGenerator(noises, seed, generator, level);
    }

    /** From the noise settings alone: anchors from the settings' own height. */
    static RandomState forSettings(HolderGetter<NormalNoise> noises, long seed, NoiseGeneratorSettings settings) {
        return RandomState.createForTesting(noises, seed, settings);
    }
}
