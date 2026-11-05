package spyglass.oracle;

import net.minecraft.server.level.ServerLevel;

/**
 * 26.4-snapshot-7's server level constructor: eight parameters, without the debug-world flag
 * 26.4's took before its spawners.
 */
abstract class ServerLevelStandIn extends ServerLevel {
    /** Never run: instances are made by {@link StandIns#allocate}. */
    ServerLevelStandIn() {
        super(null, null, null, null, null, null, null, false);
    }
}
