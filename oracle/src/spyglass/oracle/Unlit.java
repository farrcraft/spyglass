package spyglass.oracle;

import net.minecraft.core.BlockPos;
import net.minecraft.world.level.lighting.LevelLightEngine;

/**
 * The light engine as features see it: no chunk is lit until after them, and for a column it has
 * no light data the sky engine answers 15 ({@code SkyLightSectionStorage.getLightValue}) and the
 * block engine 0. Made by {@link StandIns#allocate}; only {@link #getRawBrightness} answers.
 */
class Unlit extends LevelLightEngine {
    /** Whether the dimension has sky light. */
    boolean sky;

    /** Never run: instances are made by {@link StandIns#allocate}. */
    Unlit() {
        super(null, false, false);
    }

    @Override
    public int getRawBrightness(BlockPos pos, int darkening) {
        return Math.max(0, sky ? 15 - darkening : 0);
    }
}
