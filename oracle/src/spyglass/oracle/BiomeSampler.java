package spyglass.oracle;

import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import net.minecraft.core.Holder;
import net.minecraft.core.QuartPos;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.RandomState;

/**
 * Biomes for one seed and dimension, a chunk at a time, through the path chunk generation uses
 * ({@code ChunkGenerator.doCreateBiomes}), and with ties reported.
 *
 * <p>For a multi-noise biome source, each chunk is resolved twice: by the game's own
 * {@code BiomeSource.createResolverForChunk}, and by a copy of {@code
 * MultiNoiseBiomeSource.createResolverForChunk} that also exposes each cell's climate target. The
 * target goes to {@link TieSearch} for every biome at the minimum fitness. Every cell is checked:
 * both the game's resolver and the game's {@code getNoiseBiome(target)} must return a member of
 * the tie set, and a sample of cells is checked against a brute-force search. Any failure is a hard
 * error, since it means the copy or the tie search no longer matches the game.
 */
final class BiomeSampler {
    /** One cell in every this many, and every tied cell, is also checked against brute force. */
    private static final int BRUTE_FORCE_EVERY = 1000;

    /** Receives each cell of a chunk. {@code tied} is {@code null} unless several biomes tie. */
    interface Sink {
        void cell(int qx, int qy, int qz, Holder<Biome> biome, Holder<Biome>[] tied);
    }

    final String dimension;
    final int quartMinY;
    final int quartHeight;
    final long seed;
    private final RandomState random;
    private final BiomeSource source;
    private final MultiNoiseBiomeSource multiNoise;
    private final TieSearch ties;
    private final List<Holder<Biome>> biomes = new ArrayList<>();
    private long cells;
    private long bruteForceChecks;

    BiomeSampler(Game game, String dimension, long seed) {
        LevelStem stem = game.stem(dimension);
        if (!(stem.generator() instanceof NoiseBasedChunkGenerator generator)) {
            throw new IllegalArgumentException(dimension + " doesn't use a noise-based generator");
        }
        this.dimension = dimension;
        this.seed = seed;
        // ChunkAccess.getMinY / getHeight come from the dimension type.
        this.quartMinY = QuartPos.fromBlock(stem.type().value().minY());
        this.quartHeight = QuartPos.fromBlock(stem.type().value().height());
        this.random = GameApi.get().randomState(game, generator, seed);
        this.source = generator.getBiomeSource();
        if (source instanceof MultiNoiseBiomeSource m) {
            this.multiNoise = m;
            var values = parameters(m).values();
            Map<Holder<Biome>, Integer> index = new HashMap<>();
            int[] ids = new int[values.size()];
            for (int i = 0; i < ids.length; i++) {
                ids[i] = index.computeIfAbsent(values.get(i).getSecond(), b -> {
                    biomes.add(b);
                    return biomes.size() - 1;
                });
            }
            this.ties = new TieSearch(values.stream().map(p -> p.getFirst()).toList(), ids);
        } else {
            this.multiNoise = null;
            this.ties = null;
        }
    }

    /** The source's parameter list. {@code MultiNoiseBiomeSource.parameters()} is private. */
    @SuppressWarnings("unchecked")
    private static Climate.ParameterList<Holder<Biome>> parameters(MultiNoiseBiomeSource source) {
        try {
            Method m = MultiNoiseBiomeSource.class.getDeclaredMethod("parameters");
            m.setAccessible(true);
            return (Climate.ParameterList<Holder<Biome>>) m.invoke(source);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("MultiNoiseBiomeSource.parameters() not found; the game changed", e);
        }
    }

    long cells() {
        return cells;
    }

    long bruteForceChecks() {
        return bruteForceChecks;
    }

    /** Every cell of chunk ({@code cx}, {@code cz}), full height, y outermost then z then x. */
    void sampleChunk(int cx, int cz, Sink sink) {
        int qx0 = QuartPos.fromBlock(cx * 16);
        int qz0 = QuartPos.fromBlock(cz * 16);
        int size = QuartPos.fromBlock(16);
        try (var chunk = GameApi.get().chunkBiomes(random, source, qx0, quartMinY, qz0, size, quartHeight,
                multiNoise != null)) {
            if (multiNoise == null) {
                forEachCell(qx0, qz0, size, (qx, qy, qz) -> sink.cell(qx, qy, qz, chunk.biome(qx, qy, qz), null));
                return;
            }
            forEachCell(qx0, qz0, size, (qx, qy, qz) -> {
                var target = chunk.target(qx, qy, qz);
                int[] tied = ties.search(target);
                Holder<Biome> game = chunk.biome(qx, qy, qz);
                Holder<Biome> copy = multiNoise.getNoiseBiome(target);
                check(tied, game, "createResolverForChunk", qx, qy, qz);
                check(tied, copy, "getNoiseBiome(target)", qx, qy, qz);
                if (tied.length > 1 || cells % BRUTE_FORCE_EVERY == 0) {
                    int[] brute = ties.bruteForce(target);
                    if (!java.util.Arrays.equals(tied, brute)) {
                        throw new IllegalStateException(String.format(
                                "tie search disagrees with brute force at quart %d %d %d: %s vs %s",
                                qx, qy, qz, names(tied), names(brute)));
                    }
                    bruteForceChecks++;
                }
                sink.cell(qx, qy, qz, game, tied.length > 1 ? holders(tied) : null);
            });
        }
    }

    private interface CellVisitor {
        void visit(int qx, int qy, int qz);
    }

    private void forEachCell(int qx0, int qz0, int size, CellVisitor visitor) {
        for (int qy = quartMinY; qy < quartMinY + quartHeight; qy++) {
            for (int qz = qz0; qz < qz0 + size; qz++) {
                for (int qx = qx0; qx < qx0 + size; qx++) {
                    visitor.visit(qx, qy, qz);
                    cells++;
                }
            }
        }
    }

    private void check(int[] tied, Holder<Biome> biome, String path, int qx, int qy, int qz) {
        for (int id : tied) {
            if (biomes.get(id) == biome) {
                return;
            }
        }
        throw new IllegalStateException(String.format("%s gave %s at quart %d %d %d, outside the minimum set %s",
                path, biome.getRegisteredName(), qx, qy, qz, names(tied)));
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private Holder<Biome>[] holders(int[] ids) {
        Holder<Biome>[] out = new Holder[ids.length];
        for (int i = 0; i < ids.length; i++) {
            out[i] = biomes.get(ids[i]);
        }
        return out;
    }

    private List<String> names(int[] ids) {
        return java.util.Arrays.stream(ids).mapToObj(i -> biomes.get(i).getRegisteredName()).toList();
    }
}
