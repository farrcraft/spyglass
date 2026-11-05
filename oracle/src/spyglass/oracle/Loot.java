package spyglass.oracle;

import java.io.IOException;
import java.io.Writer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Random;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.permissions.PermissionSet;
import net.minecraft.commands.Commands;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.tags.TagKey;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.storage.loot.LootParams;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.level.storage.loot.parameters.LootContextParamSets;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.phys.Vec3;

/**
 * Loot golden files: every container loot table of the version, filled by the game's own
 * {@code LootTable.fill} for a fixed list of seeds, slot by slot. See {@code docs/specs/oracle.md}.
 *
 * <p>Tables load as the server loads them ({@code ReloadableServerRegistries.reload}). A fill
 * needs a level only for its server's registries, the container's biome
 * ({@code location_check}) and exploration maps' searches: a stand-in gives each fill one biome,
 * cycling through the registry, and finds no structure for any map.
 */
final class Loot {
    /** The loot table folders whose tables fill containers, and their containers' slots. */
    private static final String[][] CONTAINERS = {{"chests/", "27"}, {"barrels/", "27"}, {"dispensers/", "9"},
            {"pots/", "1"}};

    /** Fills a table: the edge seeds, then the rest from a fixed random. Seed 0 isn't a seed. */
    static final int SEEDS = 500;

    /** The container's block: its centre is the fill's origin. */
    private static final BlockPos AT = new BlockPos(0, 64, 0);

    private Loot() {}

    static int run(Game game, Path outDir) throws IOException {
        Files.createDirectories(outDir);
        ReloadableServerRegistries.Holder registries = holder(game);
        var ops = RegistryOps.create(JsonOps.INSTANCE, registries.lookup());
        List<Holder.Reference<Biome>> biomes = new ArrayList<>(
                game.access.lookupOrThrow(Registries.BIOME).listElements().toList());
        biomes.sort((a, b) -> a.key().identifier().compareTo(b.key().identifier()));
        long[] seeds = seeds();

        var server = StandIns.allocate(Server.class);
        server.registries = registries;
        var level = StandIns.allocate(Level.class);
        // Enchanting reads the enchantments from the level's registries.
        level.game = game;
        level.server = server;
        level.structures = StandIns.allocate(NoStructures.class);

        List<Identifier> tables = registries.lookup().lookupOrThrow(Registries.LOOT_TABLE).listElementIds()
                .map(ResourceKey::identifier).filter(id -> slots(id) > 0).sorted().toList();
        for (Identifier id : tables) {
            LootTable table = registries.getLootTable(ResourceKey.create(Registries.LOOT_TABLE, id));
            var root = new JsonObject();
            root.addProperty("format", 1);
            root.addProperty("harness", Main.HARNESS_VERSION);
            root.addProperty("version", game.version);
            root.addProperty("data_version", game.dataVersion);
            root.addProperty("table", id.toString());
            root.addProperty("slots", slots(id));
            root.add("at", pos(AT));
            var fills = new JsonArray();
            for (int i = 0; i < seeds.length; i++) {
                Holder.Reference<Biome> biome = biomes.get(i % biomes.size());
                level.biome = biome;
                var container = new SimpleContainer(slots(id));
                LootParams params = new LootParams.Builder(level)
                        .withParameter(LootContextParams.ORIGIN, Vec3.atCenterOf(AT))
                        .create(LootContextParamSets.CHEST);
                table.fill(container, params, seeds[i]);
                var fill = new JsonObject();
                fill.addProperty("seed", seeds[i]);
                fill.addProperty("biome", biome.key().identifier().toString());
                var slots = new JsonArray();
                for (int slot = 0; slot < container.getContainerSize(); slot++) {
                    ItemStack stack = container.getItem(slot);
                    if (stack.isEmpty()) {
                        continue;
                    }
                    JsonObject encoded = ItemStack.CODEC.encodeStart(ops, stack).getOrThrow().getAsJsonObject();
                    encoded.addProperty("slot", slot);
                    slots.add(encoded);
                }
                fill.add("slots", slots);
                fills.add(fill);
            }
            root.add("fills", fills);
            Path out = outDir.resolve(id.getPath() + ".json");
            Files.createDirectories(out.getParent());
            try (Writer writer = Files.newBufferedWriter(out)) {
                // One line a fill keeps the files small and still diffable.
                writer.write(compact(root));
            }
            System.out.println(out);
        }
        return 0;
    }

    /**
     * The server's loot registries, loaded as {@code WorldLoader} loads a dedicated server's
     * resources, then with the items' default components bound, which every stack reads. The
     * static registries' tags are bound already ({@link Game}).
     */
    private static ReloadableServerRegistries.Holder holder(Game game) {
        ReloadableServerResources resources = ReloadableServerResources.loadResources(game.resources, game.layers,
                List.of(), FeatureFlags.DEFAULT_FLAGS, Commands.CommandSelection.DEDICATED,
                PermissionSet.NO_PERMISSIONS, Runnable::run, Runnable::run).join();
        resources.updateComponentsAndStaticRegistryTags();
        return resources.fullRegistries();
    }

    /** The slots of the container a table fills, or 0 for a table that fills none. */
    private static int slots(Identifier id) {
        if (!id.getNamespace().equals("minecraft")) {
            return 0;
        }
        for (String[] container : CONTAINERS) {
            if (id.getPath().startsWith(container[0])) {
                return Integer.parseInt(container[1]);
            }
        }
        return 0;
    }

    /** The seeds every table is filled with: the edges, then a fixed random's. */
    static long[] seeds() {
        long[] edges = {1, -1, 2, Long.MIN_VALUE, Long.MAX_VALUE, 12_345, -6_789_012_345L};
        long[] seeds = new long[SEEDS];
        System.arraycopy(edges, 0, seeds, 0, edges.length);
        var random = new Random(20_261_104L);
        for (int i = edges.length; i < SEEDS; i++) {
            long seed;
            do {
                seed = random.nextLong();
            } while (seed == 0);
            seeds[i] = seed;
        }
        return seeds;
    }

    private static JsonArray pos(BlockPos pos) {
        var array = new JsonArray();
        array.add(pos.getX());
        array.add(pos.getY());
        array.add(pos.getZ());
        return array;
    }

    /** The file: its header on one line, then one fill a line. */
    private static String compact(JsonObject root) {
        var gson = new com.google.gson.Gson();
        var fills = root.remove("fills").getAsJsonArray();
        String header = gson.toJson(root);
        var out = new StringBuilder(header.substring(0, header.length() - 1)).append(",\"fills\":[\n");
        for (int i = 0; i < fills.size(); i++) {
            out.append(gson.toJson(fills.get(i))).append(i + 1 < fills.size() ? ",\n" : "\n");
        }
        return out.append("]}\n").toString();
    }

    /** The server: only its loot registries, which a fill's context reads. */
    static class Server extends Decoration.GameServer {
        ReloadableServerRegistries.Holder registries;

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return registries;
        }
    }

    /**
     * The level a fill runs in: its server, one biome everywhere, every chunk loaded, and no
     * structure for any exploration map. The overloads below cover every supported version's
     * methods; one a version lacks is only a method of this class there.
     */
    static class Level extends FeatureStep.Server {
        MinecraftServer server;
        Holder<Biome> biome;
        StructureManager structures;

        @Override
        public MinecraftServer getServer() {
            return server;
        }

        public Holder<Biome> getBiome(BlockPos pos) {
            return biome;
        }

        public Holder<Biome> getBiome(int x, int y, int z) {
            return biome;
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            return true;
        }

        @Override
        public StructureManager structureManager() {
            return structures;
        }

        /** 26.2's search. */
        public BlockPos findNearestMapStructure(TagKey<Structure> destination, BlockPos from, int radius,
                boolean skipKnown) {
            return null;
        }

        /** 26.3's. */
        public BlockPos findNearestMapStructure(HolderSet<Structure> destination, BlockPos from, int radius,
                boolean skipKnown) {
            return null;
        }
    }

    /** No structure anywhere, for 26.3's claim of the structure a map's container stands in. */
    static class NoStructures extends StructureManager {
        /**
         * What {@code getStructureAt} gives for no structure: 26.3's {@code INVALID_START}, and
         * from 26.4 {@code null}, the field gone.
         */
        static final StructureStart NONE = none();

        /** Never run: instances are made by {@link StandIns#allocate}. */
        NoStructures() {
            super(null, null, null);
        }

        @Override
        public StructureStart getStructureAt(BlockPos pos, Structure structure) {
            return NONE;
        }

        public StructureStart getStructureAt(BlockPos pos, HolderSet<Structure> structures) {
            return NONE;
        }

        @Override
        public void addReference(StructureStart start) {}

        private static StructureStart none() {
            try {
                return (StructureStart) StructureStart.class.getField("INVALID_START").get(null);
            } catch (NoSuchFieldException e) {
                return null;
            } catch (IllegalAccessException e) {
                throw new IllegalStateException(e);
            }
        }
    }
}
