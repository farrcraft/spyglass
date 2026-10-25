package spyglass.oracle;

import java.io.DataInputStream;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import net.minecraft.core.Holder;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.chunk.storage.RegionFile;
import net.minecraft.world.level.chunk.storage.RegionStorageInfo;

/**
 * Compares the harness with the biomes stored in a real world save, as an independent check of the
 * harness. See "Independent check: real world saves" in {@code docs/specs/oracle.md}.
 *
 * <p>Every chunk at or past the biomes step stores the biomes the game generated, at quart
 * resolution. A cell passes if it equals the harness's biome where the minimum is unique, or is
 * any member of the tie set where it isn't.
 */
final class CheckSave {
    /** Chunk statuses at or past {@code ChunkStatus.BIOMES}, whose biomes are final. */
    private static final Set<String> GENERATED = Set.of("minecraft:biomes", "minecraft:noise", "minecraft:surface",
            "minecraft:carvers", "minecraft:features", "minecraft:initialize_light", "minecraft:light",
            "minecraft:spawn", "minecraft:full");
    private static final Pattern REGION = Pattern.compile("r\\.(-?\\d+)\\.(-?\\d+)\\.mca");
    private static final int MAX_REPORTED = 10;

    private CheckSave() {}

    /** Returns the number of unexplained mismatches. */
    static long run(Game game, Path world, String dimension) throws IOException {
        // From 26.x the seed lives in data/minecraft/world_gen_settings.dat, not level.dat.
        CompoundTag settings = NbtIo.readCompressed(world.resolve("data/minecraft/world_gen_settings.dat"),
                NbtAccounter.unlimitedHeap());
        long seed = settings.getCompoundOrEmpty("data").getLong("seed")
                .orElseThrow(() -> new IOException("no data.seed in world_gen_settings.dat"));
        var id = Identifier.parse(dimension);
        Path regions = world.resolve("dimensions").resolve(id.getNamespace()).resolve(id.getPath()).resolve("region");
        System.out.printf("world %s, seed %d, %s%n", world.getFileName(), seed, dimension);

        var sampler = new BiomeSampler(game, dimension, seed);
        var lookup = game.access.lookupOrThrow(Registries.BIOME);
        Map<String, Holder<Biome>> byName = new HashMap<>();
        var info = new RegionStorageInfo(world.getFileName().toString(),
                ResourceKey.create(Registries.DIMENSION, id), "chunk");

        long chunks = 0, cells = 0, tied = 0, tiedDifferent = 0, unexplained = 0;
        long t0 = System.nanoTime();
        List<Path> files;
        try (var list = Files.list(regions)) {
            files = list.filter(p -> REGION.matcher(p.getFileName().toString()).matches()).sorted().toList();
        }
        for (Path file : files) {
            Matcher m = REGION.matcher(file.getFileName().toString());
            m.matches();
            int rx = Integer.parseInt(m.group(1)), rz = Integer.parseInt(m.group(2));
            try (var region = new RegionFile(info, file, regions, false)) {
                for (int i = 0; i < 32 * 32; i++) {
                    var pos = new ChunkPos(rx * 32 + (i & 31), rz * 32 + (i >> 5));
                    CompoundTag chunk;
                    try (DataInputStream in = region.getChunkDataInputStream(pos)) {
                        if (in == null) {
                            continue;
                        }
                        chunk = NbtIo.read(in);
                    }
                    if (!GENERATED.contains(chunk.getStringOr("Status", ""))) {
                        continue;
                    }
                    var stored = storedBiomes(chunk, sampler, lookup, byName);
                    long[] counts = new long[4];
                    sampler.sampleChunk(pos.x(), pos.z(), (qx, qy, qz, biome, ties) -> {
                        Holder<Biome> actual = stored.get(cellKey(qx, qy, qz));
                        if (actual == null) {
                            return;
                        }
                        counts[0]++;
                        if (ties == null) {
                            if (actual != biome && counts[3]++ < MAX_REPORTED) {
                                System.out.printf("UNEXPLAINED at quart %d %d %d: save %s, harness %s%n", qx, qy, qz,
                                        actual.getRegisteredName(), biome.getRegisteredName());
                            }
                            return;
                        }
                        counts[1]++;
                        for (Holder<Biome> t : ties) {
                            if (t == actual) {
                                if (actual != biome) {
                                    counts[2]++;
                                }
                                return;
                            }
                        }
                        if (counts[3]++ < MAX_REPORTED) {
                            System.out.printf("UNEXPLAINED at quart %d %d %d: save %s, outside the tie set%n", qx, qy,
                                    qz, actual.getRegisteredName());
                        }
                    });
                    chunks++;
                    cells += counts[0];
                    tied += counts[1];
                    tiedDifferent += counts[2];
                    unexplained += counts[3];
                }
            }
        }
        System.out.printf("compared %d cells in %d chunks in %.1f s%n", cells, chunks, (System.nanoTime() - t0) / 1e9);
        System.out.printf("tied cells: %d (save picked a different tied biome in %d)%n", tied, tiedDifferent);
        System.out.printf("tie search checked against brute force on %d cells%n", sampler.bruteForceChecks());
        System.out.printf("unexplained mismatches: %d%n", unexplained);
        return unexplained;
    }

    private static long cellKey(int qx, int qy, int qz) {
        return ((long) qy << 48) ^ ((long) (qz & 0xFFFFFF) << 24) ^ (qx & 0xFFFFFF);
    }

    /**
     * The chunk's stored biomes by cell. Each section holds a 4×4×4 paletted container, serialized
     * as {@code PalettedContainer} does: a palette of ids and, if it has more than one entry,
     * indices packed into longs at {@code max(1, ceil(log2(size)))} bits, not spanning longs, in
     * y-z-x order.
     */
    private static Map<Long, Holder<Biome>> storedBiomes(CompoundTag chunk, BiomeSampler sampler,
            net.minecraft.core.HolderGetter<Biome> lookup, Map<String, Holder<Biome>> byName) {
        Map<Long, Holder<Biome>> out = new HashMap<>();
        int cx = chunk.getIntOr("xPos", 0), cz = chunk.getIntOr("zPos", 0);
        var sections = chunk.getListOrEmpty("sections");
        for (int s = 0; s < sections.size(); s++) {
            CompoundTag section = sections.getCompoundOrEmpty(s);
            int sy = section.getByteOr("Y", (byte) 0);
            CompoundTag biomes = section.getCompoundOrEmpty("biomes");
            var palette = biomes.getListOrEmpty("palette");
            if (palette.isEmpty()) {
                continue;
            }
            long[] data = biomes.getLongArray("data").orElse(new long[0]);
            int bits = palette.size() == 1 ? 0 : Math.max(1, 32 - Integer.numberOfLeadingZeros(palette.size() - 1));
            int perLong = bits == 0 ? 1 : 64 / bits;
            for (int i = 0; i < 64; i++) {
                int idx = bits == 0 ? 0 : (int) ((data[i / perLong] >>> ((i % perLong) * bits)) & ((1L << bits) - 1));
                String name = palette.getStringOr(idx, "");
                Holder<Biome> biome = byName.computeIfAbsent(name, n -> lookup
                        .get(ResourceKey.create(Registries.BIOME, Identifier.parse(n)))
                        .orElseThrow(() -> new IllegalStateException("unknown biome " + n + " in save")));
                int qx = cx * 4 + (i & 3), qz = cz * 4 + ((i >> 2) & 3), qy = sy * 4 + (i >> 4);
                if (qy >= sampler.quartMinY && qy < sampler.quartMinY + sampler.quartHeight) {
                    out.put(cellKey(qx, qy, qz), biome);
                }
            }
        }
        return out;
    }
}
