package spyglass.oracle;

import java.nio.file.Path;
import java.util.List;

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
 * with the vanilla data pack and any test packs.
 */
final class Game {
    final RegistryAccess.Frozen access;
    /** The data packs, for structure templates. */
    final MultiPackResourceManager resources;
    final WorldDimensions dimensions;
    final String version;
    final int dataVersion;

    /**
     * {@code packs} are the test packs loaded over the vanilla one, which the version's name
     * records: {@code <version>+<folder name>} for each.
     */
    Game(RegistryAccess.Frozen access, MultiPackResourceManager resources, WorldDimensions dimensions,
            List<Path> packs) {
        this.access = access;
        this.resources = resources;
        this.dimensions = dimensions;
        var current = SharedConstants.getCurrentVersion();
        var version = new StringBuilder(current.id());
        for (Path pack : packs) {
            version.append('+').append(pack.getFileName());
        }
        this.version = version.toString();
        this.dataVersion = current.dataVersion().version();
    }

    /** The registries and dimensions, loaded as this version's server loads them, with {@code packs}. */
    static Game load(List<Path> packs) {
        return GameApi.get().load(packs);
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
