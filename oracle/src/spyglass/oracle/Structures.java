package spyglass.oracle;

import java.io.IOException;
import java.io.StringWriter;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.function.Predicate;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.QuartPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * Structure golden files: for one seed, every placement chunk of every structure set the
 * dimension can generate, in a few chunk regions, with what the game tries there; the
 * concentric-ring positions; and the slime chunks. See {@code docs/specs/oracle.md}.
 *
 * <p>The choice among a set's structures copies {@code ChunkGenerator.createStructures}. Each try
 * records {@code Structure.findGenerationPoint}'s stub (the position the biome check samples),
 * the biome there through the context's resolver, {@code GenerationContext.isValidBiome}, and
 * whether {@code Structure.generate} produced a valid start, which decides the game's choice.
 */
final class Structures {
    /** A chunk region, inclusive. */
    record Region(String name, int minX, int minZ, int maxX, int maxZ) {}

    static final List<Region> REGIONS = List.of(
            new Region("origin", -64, -64, 63, 63),
            new Region("far-pos", 62_500, 62_500, 62_531, 62_531),
            new Region("far-neg", -62_532, -62_532, -62_501, -62_501),
            // The world border is at block ±29,999,984: chunk 1,874,999 is the last inside.
            new Region("border-pos", 1_874_968, 1_874_968, 1_874_999, 1_874_999),
            new Region("border-neg", -1_875_000, -1_875_000, -1_874_969, -1_874_969));

    private static final Method FIND_GENERATION_POINT;

    static {
        try {
            FIND_GENERATION_POINT =
                    Structure.class.getDeclaredMethod("findGenerationPoint", Structure.GenerationContext.class);
            FIND_GENERATION_POINT.setAccessible(true);
        } catch (NoSuchMethodException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    private Structures() {}

    static int run(Game game, Path outDir, String dimension, List<String> seeds) throws Exception {
        Files.createDirectories(outDir);
        var storage = LevelStorageSource.createDefault(Path.of("oracle-levels")).createAccess("structures");
        var templates = new StructureTemplateManager(game.resources, storage, DataFixers.getDataFixer(),
                game.access.lookupOrThrow(Registries.BLOCK));
        for (String text : seeds) {
            long t0 = System.nanoTime();
            long seed = net.minecraft.world.level.levelgen.WorldOptions.parseSeed(text).orElseThrow();
            JsonObject root = seed(game, templates, dimension, seed);
            Path out = outDir.resolve(text + ".json");
            write(out, root);
            System.out.printf("%s: %.1f s%n", out, (System.nanoTime() - t0) / 1e9);
        }
        return 0;
    }

    private static JsonObject seed(Game game, StructureTemplateManager templates, String dimension, long seed)
            throws ReflectiveOperationException {
        LevelStem stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        // As ServerChunkCache and ChunkMap create them.
        RandomState random = RandomState.create(game.access.lookupOrThrow(Registries.NOISE), seed,
                generator.generatorSettings().value());
        ChunkGeneratorStructureState state =
                generator.createState(game.access.lookupOrThrow(Registries.STRUCTURE_SET), random, seed);
        state.ensureStructuresGenerated();
        var type = stem.type().value();
        LevelHeightAccessor height = LevelHeightAccessor.create(type.minY(), type.height());
        ResourceKey<net.minecraft.world.level.Level> level = ResourceKey.create(Registries.DIMENSION,
                net.minecraft.resources.Identifier.parse(dimension));

        var root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("harness", Main.HARNESS_VERSION);
        root.addProperty("version", game.version);
        root.addProperty("data_version", game.dataVersion);
        root.addProperty("seed", seed);
        root.addProperty("dimension", dimension);

        var regions = new JsonArray();
        for (Region r : REGIONS) {
            var o = new JsonObject();
            o.addProperty("name", r.name());
            o.add("min", pair(r.minX(), r.minZ()));
            o.add("max", pair(r.maxX(), r.maxZ()));
            regions.add(o);
        }
        root.add("regions", regions);

        var rings = new JsonArray();
        var sets = new JsonArray();
        for (Holder<StructureSet> holder : state.possibleStructureSets()) {
            String id = holder.unwrapKey().orElseThrow().identifier().toString();
            StructureSet set = holder.value();
            if (set.placement() instanceof ConcentricRingsStructurePlacement rp) {
                var ring = new JsonObject();
                ring.addProperty("set", id);
                var chunks = new JsonArray();
                for (ChunkPos p : state.getRingPositionsFor(rp)) {
                    chunks.add(pair(p.x(), p.z()));
                }
                ring.add("chunks", chunks);
                rings.add(ring);
            }
            var chunks = new JsonArray();
            for (Region r : REGIONS) {
                for (int z = r.minZ(); z <= r.maxZ(); z++) {
                    for (int x = r.minX(); x <= r.maxX(); x++) {
                        if (!set.placement().isStructureChunk(state, x, z)) {
                            continue;
                        }
                        var chunk = new JsonObject();
                        chunk.add("chunk", pair(x, z));
                        chunk.add("tries", tries(game, generator, random, templates, height, level, seed, set,
                                new ChunkPos(x, z)));
                        chunks.add(chunk);
                    }
                }
            }
            var s = new JsonObject();
            s.addProperty("set", id);
            s.add("chunks", chunks);
            sets.add(s);
        }
        root.add("rings", rings);
        root.add("sets", sets);

        var slime = new JsonArray();
        for (Region r : REGIONS) {
            var o = new JsonObject();
            o.addProperty("region", r.name());
            var chunks = new JsonArray();
            for (int z = r.minZ(); z <= r.maxZ(); z++) {
                for (int x = r.minX(); x <= r.maxX(); x++) {
                    // Slime.checkSlimeSpawnRules.
                    if (WorldgenRandom.seedSlimeChunk(x, z, seed, 987234911L).nextInt(10) == 0) {
                        chunks.add(pair(x, z));
                    }
                }
            }
            o.add("chunks", chunks);
            slime.add(o);
        }
        root.add("slime", slime);
        return root;
    }

    /** ChunkGenerator.createStructures' choice in one placement chunk, recording every try. */
    private static JsonArray tries(Game game, ChunkGenerator generator, RandomState random,
            StructureTemplateManager templates, LevelHeightAccessor height,
            ResourceKey<net.minecraft.world.level.Level> level, long seed, StructureSet set, ChunkPos chunk)
            throws ReflectiveOperationException {
        Climate.Sampler sampler = random.createClimateSampler(SamplerContext.builder().enableCaches().build());
        var out = new JsonArray();
        List<StructureSet.StructureSelectionEntry> entries = set.structures();
        if (entries.size() == 1) {
            out.add(attempt(game, generator, random, sampler, templates, height, level, seed, entries.get(0), chunk));
            return out;
        }
        var list = new ArrayList<>(entries);
        var pick = new WorldgenRandom(new LegacyRandomSource(0L));
        pick.setLargeFeatureSeed(seed, chunk.x(), chunk.z());
        int total = 0;
        for (var e : list) {
            total += e.weight();
        }
        while (!list.isEmpty()) {
            int n = pick.nextInt(total);
            int i = 0;
            for (var e : list) {
                n -= e.weight();
                if (n < 0) {
                    break;
                }
                i++;
            }
            var entry = list.get(i);
            JsonObject t = attempt(game, generator, random, sampler, templates, height, level, seed, entry, chunk);
            out.add(t);
            if (t.get("start").getAsBoolean()) {
                break;
            }
            list.remove(i);
            total -= entry.weight();
        }
        return out;
    }

    private static JsonObject attempt(Game game, ChunkGenerator generator, RandomState random,
            Climate.Sampler sampler, StructureTemplateManager templates, LevelHeightAccessor height,
            ResourceKey<net.minecraft.world.level.Level> level, long seed,
            StructureSet.StructureSelectionEntry entry, ChunkPos chunk) throws ReflectiveOperationException {
        Holder<Structure> holder = entry.structure();
        Structure structure = holder.value();
        Predicate<Holder<Biome>> valid = structure.biomes()::contains;
        var t = new JsonObject();
        t.addProperty("structure", holder.unwrapKey().orElseThrow().identifier().toString());

        // findGenerationPoint on its own context, then the biome check on the stub.
        var context = new Structure.GenerationContext(game.access, generator, generator.getBiomeSource(), sampler,
                random, templates, seed, chunk, height, valid);
        @SuppressWarnings("unchecked")
        var stub = (Optional<Structure.GenerationStub>) FIND_GENERATION_POINT.invoke(structure, context);
        if (stub.isPresent()) {
            BlockPos pos = stub.get().position();
            t.add("stub", triple(pos.getX(), pos.getY(), pos.getZ()));
            Holder<Biome> biome = context.biomeResolver().getNoiseBiome(QuartPos.fromBlock(pos.getX()),
                    QuartPos.fromBlock(pos.getY()), QuartPos.fromBlock(pos.getZ()));
            t.addProperty("biome", biome.unwrapKey().orElseThrow().identifier().toString());
            t.addProperty("valid_biome", context.isValidBiome(stub.get()));
        }

        // What the game itself does: ChunkGenerator.tryGenerateStructure.
        var start = structure.generate(holder, level, game.access, generator, generator.getBiomeSource(), sampler,
                random, templates, seed, chunk, 0, height, valid);
        t.addProperty("start", start.isValid());
        return t;
    }

    private static JsonArray pair(int a, int b) {
        var arr = new JsonArray();
        arr.add(a);
        arr.add(b);
        return arr;
    }

    private static JsonArray triple(int a, int b, int c) {
        var arr = new JsonArray();
        arr.add(a);
        arr.add(b);
        arr.add(c);
        return arr;
    }

    private static void write(Path path, JsonObject root) throws IOException {
        var text = new StringWriter();
        try (var writer = new JsonWriter(text)) {
            new Gson().toJson(root, writer);
        }
        Files.writeString(path, text + "\n");
    }
}
