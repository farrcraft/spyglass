package spyglass.oracle;

import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.WorldDimensions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;

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

    private Game(RegistryAccess.Frozen access, MultiPackResourceManager resources, WorldDimensions dimensions) {
        this.access = access;
        this.resources = resources;
        this.dimensions = dimensions;
        var current = SharedConstants.getCurrentVersion();
        this.version = current.id();
        this.dataVersion = current.dataVersion().version();
    }

    static Game load() {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // WorldLoader.load: static registries first, then tags from the data packs, then the
        // worldgen registries from the data packs into the WORLD layer.
        LayeredRegistryAccess<RegistryLayer> layers = RegistryLayer.createRegistryAccess();
        var resources = new MultiPackResourceManager(PackType.SERVER_DATA,
                List.of(ServerPacksSource.createVanillaPackSource().fullResources()));
        var tags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
        List<HolderLookup.RegistryLookup<?>> lookups =
                TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), tags);
        RegistryAccess.Frozen world =
                RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        RegistryAccess.Frozen access = layers.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
        // ReloadableServerResources.updateStaticRegistryTags: bind the static registries' tags
        // (blocks, items), which structure pieces read.
        tags.forEach(net.minecraft.core.Registry.PendingTags::apply);

        // What CreateWorldScreen and the dedicated server give a world with the Default preset.
        return new Game(access, resources, WorldPresets.createNormalWorldDimensions(access));
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
