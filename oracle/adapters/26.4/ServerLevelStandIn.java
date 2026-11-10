package spyglass.oracle;

import net.minecraft.server.level.ServerLevel;

/**
 * The server level's constructor, apart from {@link FeatureStep.Server} because its parameters
 * change between versions and only this file then differs. 26.4's takes nine.
 */
abstract class ServerLevelStandIn extends ServerLevel {
    /** Never run: instances are made by {@link StandIns#allocate}. */
    ServerLevelStandIn() {
        super(null, null, null, null, null, null, false, null, false);
    }
}
