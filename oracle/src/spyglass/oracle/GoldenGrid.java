package spyglass.oracle;

import java.io.BufferedOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Arrays;
import java.util.IdentityHashMap;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

import net.minecraft.core.Holder;
import net.minecraft.world.level.biome.Biome;

/**
 * A golden biome grid for a chunk-aligned, full-height region, and its file format. The format is
 * specified in {@code docs/specs/oracle.md}; keep the two in step and bump {@link #FORMAT} on any
 * change.
 */
final class GoldenGrid {
    static final int FORMAT = 1;
    private static final byte[] MAGIC = "SPYBIOME".getBytes(StandardCharsets.US_ASCII);

    private final int qx0, qy0, qz0, sizeX, sizeY, sizeZ;
    /** Biome per cell, in file order. For a tied cell, any member of the tie set. */
    private final Holder<Biome>[] cells;
    /** Tie sets by cell index. */
    private final Map<Integer, Holder<Biome>[]> ties = new TreeMap<>();

    @SuppressWarnings({"unchecked", "rawtypes"})
    GoldenGrid(int qx0, int qy0, int qz0, int sizeX, int sizeY, int sizeZ) {
        this.qx0 = qx0;
        this.qy0 = qy0;
        this.qz0 = qz0;
        this.sizeX = sizeX;
        this.sizeY = sizeY;
        this.sizeZ = sizeZ;
        this.cells = new Holder[Math.multiplyExact(Math.multiplyExact(sizeX, sizeY), sizeZ)];
    }

    /** Fills the grid from the sampler, chunk by chunk. */
    static GoldenGrid sample(BiomeSampler sampler, int qx0, int qz0, int sizeX, int sizeZ) {
        if (Math.floorMod(qx0, 4) != 0 || Math.floorMod(qz0, 4) != 0 || sizeX % 4 != 0 || sizeZ % 4 != 0) {
            throw new IllegalArgumentException("regions must be chunk-aligned: quart x, z and sizes multiples of 4");
        }
        var grid = new GoldenGrid(qx0, sampler.quartMinY, qz0, sizeX, sampler.quartHeight, sizeZ);
        for (int cz = qz0 >> 2; cz < (qz0 + sizeZ) >> 2; cz++) {
            for (int cx = qx0 >> 2; cx < (qx0 + sizeX) >> 2; cx++) {
                sampler.sampleChunk(cx, cz, grid::set);
            }
        }
        return grid;
    }

    private void set(int qx, int qy, int qz, Holder<Biome> biome, Holder<Biome>[] tied) {
        // x fastest, then z, then y: horizontal slices are contiguous.
        int index = ((qy - qy0) * sizeZ + (qz - qz0)) * sizeX + (qx - qx0);
        cells[index] = biome;
        if (tied != null) {
            ties.put(index, tied);
        }
    }

    int tieCount() {
        return ties.size();
    }

    /**
     * Writes the file atomically. Tied cells store the tie set's lowest palette index, so the output
     * doesn't depend on which tied biome the game happened to pick.
     */
    void write(Path out, String version, int dataVersion, long seed, String dimension) throws IOException {
        // Palette: every biome present, sorted by id.
        var present = new IdentityHashMap<Holder<Biome>, String>();
        for (Holder<Biome> cell : cells) {
            present.computeIfAbsent(cell, Holder::getRegisteredName);
        }
        ties.values().forEach(t -> Arrays.stream(t).forEach(b -> present.computeIfAbsent(b, Holder::getRegisteredName)));
        String[] palette = new TreeSet<>(present.values()).toArray(String[]::new);
        if (palette.length > 0xFFFF) {
            throw new IllegalStateException("palette too large");
        }
        var index = new IdentityHashMap<Holder<Biome>, Integer>();
        present.forEach((holder, name) -> index.put(holder, Arrays.binarySearch(palette, name)));

        var header = new StringBuilder("{");
        header.append("\"format\":").append(FORMAT);
        header.append(",\"harness\":").append(Main.HARNESS_VERSION);
        header.append(",\"version\":").append(jsonString(version));
        header.append(",\"dataVersion\":").append(dataVersion);
        header.append(",\"seed\":").append(seed);
        header.append(",\"dimension\":").append(jsonString(dimension));
        header.append(",\"min\":[").append(qx0).append(',').append(qy0).append(',').append(qz0).append(']');
        header.append(",\"size\":[").append(sizeX).append(',').append(sizeY).append(',').append(sizeZ).append(']');
        header.append(",\"palette\":[");
        for (int i = 0; i < palette.length; i++) {
            header.append(i > 0 ? "," : "").append(jsonString(palette[i]));
        }
        header.append("],\"ties\":").append(ties.size()).append('}');
        byte[] headerBytes = header.toString().getBytes(StandardCharsets.UTF_8);

        Files.createDirectories(out.toAbsolutePath().getParent());
        Path partial = out.resolveSibling(out.getFileName() + ".part");
        try (OutputStream os = new BufferedOutputStream(Files.newOutputStream(partial), 1 << 16)) {
            os.write(MAGIC);
            os.write(le(4).putInt(headerBytes.length).array());
            os.write(headerBytes);
            ByteBuffer cellBytes = le(2 * cells.length);
            for (int i = 0; i < cells.length; i++) {
                Holder<Biome>[] tied = ties.get(i);
                int stored = tied == null ? index.get(cells[i]) : Arrays.stream(tied).mapToInt(index::get).min().getAsInt();
                cellBytes.putShort((short) stored);
            }
            os.write(cellBytes.array());
            for (var tie : ties.entrySet()) {
                int[] ids = Arrays.stream(tie.getValue()).mapToInt(index::get).sorted().toArray();
                ByteBuffer entry = le(4 + 2 + 2 * ids.length);
                entry.putInt(tie.getKey()).putShort((short) ids.length);
                for (int id : ids) {
                    entry.putShort((short) id);
                }
                os.write(entry.array());
            }
        }
        Files.move(partial, out, StandardCopyOption.REPLACE_EXISTING);
    }

    private static ByteBuffer le(int size) {
        return ByteBuffer.allocate(size).order(ByteOrder.LITTLE_ENDIAN);
    }

    /** Ids and dimension names are plain ASCII identifiers; anything else is rejected, not escaped. */
    private static String jsonString(String s) {
        if (!s.matches("[A-Za-z0-9_.:/\\-]*")) {
            throw new IllegalArgumentException("unexpected characters in " + s);
        }
        return '"' + s + '"';
    }
}
