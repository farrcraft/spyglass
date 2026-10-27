package spyglass.oracle;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

import net.minecraft.world.level.levelgen.WorldOptions;

/**
 * The spyglass oracle: asks the game itself for worldgen results. Built and run by
 * {@code cargo xtask oracle}; see {@code docs/adr/0004-oracle-standalone-harness.md} and
 * {@code docs/specs/oracle.md}.
 *
 * <pre>
 * biomes &lt;jobs file&gt; [--threads N]
 *     One golden biome file per job line: "seed dimension qx qz sizeX sizeZ out-path".
 *     The seed is parsed as the game parses the world-creation seed field (WorldOptions.parseSeed),
 *     so text seeds work. Regions are chunk-aligned quart rectangles at full height.
 * check-save &lt;world dir&gt; [--dimension ID]
 *     Compare against the biomes stored in a save. Exits 1 on any unexplained mismatch.
 * probe &lt;out dir&gt; &lt;seed&gt;...
 *     Raw values from the random sources and noises (rng.json, noise.json), for the Rust ports'
 *     tests. Noise cases are written for each seed; random-source cases use a fixed seed list.
 * structures &lt;out dir&gt; [--dimension ID] &lt;seed&gt;...
 *     One structure golden file per seed (&lt;seed&gt;.json): the dimension's placement chunks, what
 *     the game tries in each, ring positions and slime chunks, in fixed chunk regions.
 * spawn &lt;out dir&gt; [--dimension ID] &lt;seed&gt;...
 *     One spawn golden file per seed (&lt;seed&gt;.json): the dimension's spawn origin, the climate
 *     search's phases and its fitness at fixed positions.
 * heights &lt;out dir&gt; [--dimension ID] [--threads N] &lt;seed&gt;...
 *     One height golden file per seed (&lt;seed&gt;.json): the generator's worldgen heightmaps at
 *     every column of fixed block regions, and its block columns at a sample of positions.
 * </pre>
 *
 * Any command may be preceded by {@code --pack <dir>} (repeatable): a data pack folder loaded
 * over the vanilla one. The outputs then name the version {@code <version>+<folder name>}.
 */
public final class Main {
    /** Bump when output for the same game version changes. Recorded in golden-file headers. */
    static final int HARNESS_VERSION = 1;

    private Main() {}

    public static void main(String[] args) throws Exception {
        var packs = new ArrayList<Path>();
        while (args.length >= 2 && args[0].equals("--pack")) {
            packs.add(Path.of(args[1]).toAbsolutePath());
            args = java.util.Arrays.copyOfRange(args, 2, args.length);
        }
        if (args.length < 2) {
            usage();
        }
        long t0 = System.nanoTime();
        Game game = Game.load(packs);
        System.out.printf("spyglass oracle %s: Minecraft %s (data version %d), loaded in %.1f s%n", HARNESS_VERSION,
                game.version, game.dataVersion, (System.nanoTime() - t0) / 1e9);
        int status;
        try {
            status = switch (args[0]) {
                case "biomes" -> biomes(game, Path.of(args[1]), intOption(args, "--threads",
                        Runtime.getRuntime().availableProcessors()));
                case "check-save" -> CheckSave.run(game, Path.of(args[1]), dimension(args)) == 0 ? 0 : 1;
                case "probe" -> Probe.run(game, Path.of(args[1]), List.of(args).subList(2, args.length));
                case "structures" -> Structures.run(game, Path.of(args[1]), dimension(args), positional(args));
                case "spawn" -> Spawn.run(game, Path.of(args[1]), dimension(args), positional(args));
                case "heights" -> Heights.run(game, Path.of(args[1]), dimension(args), positional(args),
                        intOption(args, "--threads", Runtime.getRuntime().availableProcessors()));
                default -> {
                    usage();
                    yield 2;
                }
            };
        } catch (Throwable e) {
            // The game routes System.out and System.err through its logger, which drops stack
            // traces; write to the process's stdout directly.
            e.printStackTrace(new java.io.PrintStream(new java.io.FileOutputStream(java.io.FileDescriptor.out), true));
            status = 1;
        }
        // The game leaves non-daemon worker threads running.
        System.exit(status);
    }

    private record Job(long seed, String dimension, int qx, int qz, int sizeX, int sizeZ, Path out) {
        static Job parse(String line) {
            String[] f = line.trim().split("\\s+", 7);
            if (f.length != 7) {
                throw new IllegalArgumentException("bad job line: " + line);
            }
            long seed = WorldOptions.parseSeed(f[0])
                    .orElseThrow(() -> new IllegalArgumentException("empty seed in: " + line));
            return new Job(seed, f[1], Integer.parseInt(f[2]), Integer.parseInt(f[3]), Integer.parseInt(f[4]),
                    Integer.parseInt(f[5]), Path.of(f[6]));
        }
    }

    private static int biomes(Game game, Path jobsFile, int threads) throws Exception {
        List<Job> jobs = Files.readAllLines(jobsFile).stream()
                .filter(l -> !l.isBlank() && !l.startsWith("#"))
                .map(Job::parse)
                .toList();
        try (var pool = Executors.newFixedThreadPool(Math.max(1, Math.min(threads, jobs.size())))) {
            List<Future<?>> futures = new ArrayList<>();
            for (Job job : jobs) {
                futures.add(pool.submit(() -> {
                    long t0 = System.nanoTime();
                    var sampler = new BiomeSampler(game, job.dimension, job.seed);
                    var grid = GoldenGrid.sample(sampler, job.qx, job.qz, job.sizeX, job.sizeZ);
                    grid.write(job.out, game.version, game.dataVersion, job.seed, job.dimension);
                    System.out.printf("%s: %d cells, %d tied, %.1f s%n", job.out, sampler.cells(), grid.tieCount(),
                            (System.nanoTime() - t0) / 1e9);
                    return null;
                }));
            }
            for (Future<?> f : futures) {
                try {
                    f.get();
                } catch (ExecutionException e) {
                    pool.shutdownNow();
                    throw new RuntimeException(e.getCause());
                }
            }
        }
        return 0;
    }

    /** Arguments after the first two that aren't options or their values. */
    private static List<String> positional(String[] args) {
        var out = new ArrayList<String>();
        for (int i = 2; i < args.length; i++) {
            if (args[i].startsWith("--")) {
                i++;
            } else {
                out.add(args[i]);
            }
        }
        return out;
    }

    /** {@code --dimension}, the overworld by default. */
    private static String dimension(String[] args) {
        return option(args, "--dimension", "minecraft:overworld");
    }

    private static String option(String[] args, String name, String fallback) {
        for (int i = 2; i + 1 < args.length; i++) {
            if (args[i].equals(name)) {
                return args[i + 1];
            }
        }
        return fallback;
    }

    private static int intOption(String[] args, String name, int fallback) {
        return Integer.parseInt(option(args, name, Integer.toString(fallback)));
    }

    private static void usage() {
        System.err.println("usage: biomes <jobs file> [--threads N] | check-save <world dir> [--dimension ID]"
                + " | probe <out dir> <seed>... | structures <out dir> [--dimension ID] <seed>..."
                + " | spawn <out dir> [--dimension ID] <seed>... | heights <out dir> [--dimension ID] [--threads N]"
                + " <seed>...");
        System.exit(2);
    }
}
