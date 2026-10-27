package spyglass.oracle;

import net.minecraft.SharedConstants;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.WorldDimensions;

/**
 * The registries and dimensions a new world gets, built the way the dedicated server builds them,
 * with only the vanilla data pack.
 */
final class Game {
    final RegistryAccess.Frozen access;
    /** The vanilla data pack, for structure templates. */
    final MultiPackResourceManager resources;
    final WorldDimensions dimensions;
    final String version;
    final int dataVersion;

    Game(RegistryAccess.Frozen access, MultiPackResourceManager resources, WorldDimensions dimensions) {
        this.access = access;
        this.resources = resources;
        this.dimensions = dimensions;
        var current = SharedConstants.getCurrentVersion();
        this.version = current.id();
        this.dataVersion = current.dataVersion().version();
    }

    /** The registries and dimensions, loaded as this version's server loads them. */
    static Game load() {
        return GameApi.get().load();
    }

    /** A dimension by id, e.g. {@code minecraft:overworld}. */
    LevelStem stem(String id) {
        var key = ResourceKey.create(Registries.LEVEL_STEM, Identifier.parse(id));
        var stem = dimensions.dimensions().get(key);
        if (stem == null) {
            throw new IllegalArgumentException("unknown dimension " + id + "; known: "
                    + dimensions.dimensions().keySet());
        }
        return stem;
    }
}
