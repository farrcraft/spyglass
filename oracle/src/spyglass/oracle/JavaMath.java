package spyglass.oracle;

import java.io.IOException;
import java.math.BigDecimal;
import java.math.MathContext;
import java.math.RoundingMode;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.SplittableRandom;
import java.util.function.DoubleUnaryOperator;
import java.util.stream.IntStream;

/**
 * {@code math <out dir>}: what {@code java.lang.Math} returns on this machine, beside
 * {@code StrictMath} and the correctly rounded result, for the functions worldgen calls ({@code sin},
 * {@code cos}, {@code log}, {@code pow}) and the arguments spyglass's call sites take
 * ({@code docs/specs/java-math.md}). These are the JDK's values, not the game's, and don't depend
 * on the game version; the game is never loaded.
 *
 * <p>Each set is a file {@code <function>-<set>.bin} of little-endian doubles, one record per case:
 * the arguments, then {@code Math}'s, {@code StrictMath}'s and the correctly rounded result.
 * {@code math.json} lists the sets and the JVM. The correctly rounded result is computed in
 * {@code BigDecimal} to {@link #DIGITS} digits and rounded once to a double, so it's wrong only for
 * an argument whose true result lies within about 10<sup>-60</sup> of a rounding boundary.
 */
final class JavaMath {
    /** Digits the exact results are carried to. */
    private static final int DIGITS = 60;
    private static final MathContext MC = new MathContext(DIGITS + 20, RoundingMode.HALF_EVEN);
    /** Wider, for range reduction, so that |x| up to 10^6 loses nothing. */
    private static final MathContext WIDE = new MathContext(DIGITS + 60, RoundingMode.HALF_EVEN);
    private static final BigDecimal TWO = BigDecimal.valueOf(2);
    /** Below this a series term no longer changes the result. */
    private static final BigDecimal EPSILON = BigDecimal.ONE.movePointLeft(DIGITS + 15);
    private static final BigDecimal PI = new BigDecimal(
            "3.14159265358979323846264338327950288419716939937510582097494459230781640628620899"
                    + "8628034825342117067982148086513282306647093844609550582231725359408128481117450");
    private static final BigDecimal HALF_PI = PI.divide(TWO, WIDE);
    private static final BigDecimal LN2 = atanh2(BigDecimal.ONE.divide(BigDecimal.valueOf(3), MC));

    private JavaMath() {}

    /** A set of arguments for one function, and how they're computed. */
    private record Set(String function, String name, double[] x, double[] y) {
        int arity() {
            return y == null ? 1 : 2;
        }
    }

    /**
     * Runs on its own, with only a JDK: {@code java oracle/src/spyglass/oracle/JavaMath.java <out dir>},
     * as the Math workflow does on each CI system.
     */
    public static void main(String[] args) throws IOException {
        if (args.length != 1) {
            System.err.println("usage: java JavaMath.java <out dir>");
            System.exit(2);
        }
        System.exit(run(Path.of(args[0])));
    }

    static int run(Path out) throws IOException {
        Files.createDirectories(out);
        var sets = new ArrayList<Set>();
        for (String function : List.of("sin", "cos")) {
            sets.add(new Set(function, "table", sineTable(), null));
            sets.add(new Set(function, "angles", angles(1, 200_000), null));
            sets.add(new Set(function, "float-angles", floatAngles(2, 200_000), null));
            sets.add(new Set(function, "broad", broadTrig(3, 1_000_000), null));
            sets.add(new Set(function, "edges", trigEdges(), null));
        }
        sets.add(new Set("log", "gaussian", gaussianS(4, 500_000), null));
        sets.add(new Set("log", "dripstone", uniform(5, 200_000, 0.0, 0.384), null));
        sets.add(new Set("log", "broad", broadPositive(6, 1_000_000), null));
        sets.add(new Set("log", "edges", logEdges(), null));
        double[] drip = uniform(7, 200_000, 0.0, 0.384);
        sets.add(new Set("pow", "dripstone-4/3", drip, filled(drip.length, 1.3333333333333333)));
        sets.add(new Set("pow", "dripstone-2/3", drip, filled(drip.length, 0.6666666666666666)));
        double[][] powers = integerPowers();
        sets.add(new Set("pow", "integer", powers[0], powers[1]));
        double[][] floats = floatPowers(8, 200_000);
        sets.add(new Set("pow", "float", floats[0], floats[1]));
        double[][] broad = broadPowers(9, 300_000);
        sets.add(new Set("pow", "broad", broad[0], broad[1]));

        var meta = new StringBuilder();
        meta.append(String.format(Locale.ROOT,
                "{\n  \"os\": \"%s\",\n  \"arch\": \"%s\",\n  \"vm\": \"%s %s\",\n  \"vendor\": \"%s\",\n  \"sets\": [\n",
                System.getProperty("os.name"), System.getProperty("os.arch"), System.getProperty("java.vm.name"),
                System.getProperty("java.vm.version"), System.getProperty("java.vendor")));
        for (int i = 0; i < sets.size(); i++) {
            Set set = sets.get(i);
            long t0 = System.nanoTime();
            write(out, set);
            System.out.printf(Locale.ROOT, "math: %s %s, %d cases, %.1f s%n", set.function, set.name, set.x.length,
                    (System.nanoTime() - t0) / 1e9);
            meta.append(String.format(Locale.ROOT,
                    "    {\"function\": \"%s\", \"set\": \"%s\", \"args\": %d, \"count\": %d}%s\n", set.function,
                    set.name, set.arity(), set.x.length, i + 1 < sets.size() ? "," : ""));
        }
        meta.append("  ]\n}\n");
        Files.writeString(out.resolve("math.json"), meta);
        return 0;
    }

    /** Computes a set's three results for every case and writes its file. */
    private static void write(Path out, Set set) throws IOException {
        int n = set.x.length;
        int width = set.arity() + 3;
        double[] records = new double[n * width];
        IntStream.range(0, n).parallel().forEach(i -> {
            double x = set.x[i];
            int at = i * width;
            records[at] = x;
            if (set.y == null) {
                DoubleUnaryOperator math = switch (set.function) {
                    case "sin" -> Math::sin;
                    case "cos" -> Math::cos;
                    default -> Math::log;
                };
                DoubleUnaryOperator strict = switch (set.function) {
                    case "sin" -> StrictMath::sin;
                    case "cos" -> StrictMath::cos;
                    default -> StrictMath::log;
                };
                DoubleUnaryOperator exact = switch (set.function) {
                    case "sin" -> JavaMath::sinExact;
                    case "cos" -> JavaMath::cosExact;
                    default -> JavaMath::logExact;
                };
                records[at + 1] = math.applyAsDouble(x);
                records[at + 2] = strict.applyAsDouble(x);
                records[at + 3] = exact.applyAsDouble(x);
            } else {
                double y = set.y[i];
                records[at + 1] = y;
                records[at + 2] = Math.pow(x, y);
                records[at + 3] = StrictMath.pow(x, y);
                records[at + 4] = powExact(x, y);
            }
        });
        var buffer = ByteBuffer.allocate(records.length * Double.BYTES).order(ByteOrder.LITTLE_ENDIAN);
        buffer.asDoubleBuffer().put(records);
        Files.write(out.resolve(set.function + "-" + set.name.replace('/', '_') + ".bin"), buffer.array());
    }

    // ---- Argument sets ----

    /** The carvers' sine table: {@code i / 10430.378350470453}, as {@code Mth} builds it. */
    private static double[] sineTable() {
        double[] x = new double[65_536];
        for (int i = 0; i < x.length; i++) {
            x[i] = i / 10430.378350470453;
        }
        return x;
    }

    /** Stronghold ring angles: {@code nextDouble() * Math.PI * 2.0}. */
    private static double[] angles(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[] x = new double[n];
        for (int i = 0; i < n; i++) {
            x[i] = random.nextDouble() * Math.PI * 2.0;
        }
        return x;
    }

    /** The spawn search's angles: {@code float}s in [0, 2π), widened. */
    private static double[] floatAngles(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[] x = new double[n];
        for (int i = 0; i < n; i++) {
            x[i] = (float) (random.nextDouble() * Math.PI * 2.0);
        }
        return x;
    }

    /** Half uniform in [-1000, 1000], half with magnitudes from 2^-30 to 2^19, either sign. */
    private static double[] broadTrig(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[] x = new double[n];
        for (int i = 0; i < n; i++) {
            x[i] = i % 2 == 0 ? random.nextDouble(-1000.0, 1000.0)
                    : Math.copySign(Math.pow(2.0, random.nextDouble(-30.0, 19.0)), random.nextBoolean() ? 1 : -1);
        }
        return x;
    }

    /** Near multiples of π/2 up to 20,000 of them, eight ulps either side, and tiny arguments. */
    private static double[] trigEdges() {
        var x = new ArrayList<Double>();
        for (int k = 1; k <= 20_000; k++) {
            double centre = k * (Math.PI / 2.0);
            double below = centre;
            double above = centre;
            x.add(centre);
            for (int u = 0; u < 8; u++) {
                below = Math.nextDown(below);
                above = Math.nextUp(above);
                x.add(below);
                x.add(above);
            }
        }
        for (int e = -1074; e <= -20; e++) {
            x.add(Math.scalb(1.0, e));
            x.add(Math.scalb(1.75, e));
        }
        return x.stream().mapToDouble(Double::doubleValue).toArray();
    }

    /** The Gaussian's {@code s = v1² + v2²} for uniform {@code v} in (-1, 1), kept in (0, 1). */
    private static double[] gaussianS(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[] x = new double[n];
        int i = 0;
        while (i < n) {
            double v1 = 2.0 * random.nextDouble() - 1.0;
            double v2 = 2.0 * random.nextDouble() - 1.0;
            double s = v1 * v1 + v2 * v2;
            if (s < 1.0 && s != 0.0) {
                x[i++] = s;
            }
        }
        return x;
    }

    /** Uniform in (low, high]. */
    private static double[] uniform(long seed, int n, double low, double high) {
        var random = new SplittableRandom(seed);
        double[] x = new double[n];
        for (int i = 0; i < n; i++) {
            x[i] = high - random.nextDouble() * (high - low);
        }
        return x;
    }

    /** Positive normal doubles with uniformly random exponents and mantissas. */
    private static double[] broadPositive(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[] x = new double[n];
        for (int i = 0; i < n; i++) {
            long exponent = random.nextLong(1, 2047);
            long mantissa = random.nextLong(1L << 52);
            x[i] = Double.longBitsToDouble(exponent << 52 | mantissa);
        }
        return x;
    }

    /** Near 1, every power of two, and a sample of subnormals. */
    private static double[] logEdges() {
        var x = new ArrayList<Double>();
        double below = 1.0;
        double above = 1.0;
        for (int u = 0; u < 1000; u++) {
            below = Math.nextDown(below);
            above = Math.nextUp(above);
            x.add(below);
            x.add(above);
        }
        for (int k = 1; k <= 1000; k++) {
            x.add(1.0 + k * 0x1p-20);
            x.add(1.0 - k * 0x1p-21);
        }
        for (int e = -1074; e <= 1023; e++) {
            x.add(Math.scalb(1.0, e));
        }
        var random = new SplittableRandom(10);
        for (int i = 0; i < 2000; i++) {
            x.add(Double.longBitsToDouble(random.nextLong(1, 1L << 52)));
        }
        return x.stream().mapToDouble(Double::doubleValue).toArray();
    }

    private static double[] filled(int n, double value) {
        double[] y = new double[n];
        java.util.Arrays.fill(y, value);
        return y;
    }

    /** {@code 2^n} and {@code 0.5^n} for every n whose result is a normal double. */
    private static double[][] integerPowers() {
        var x = new ArrayList<Double>();
        var y = new ArrayList<Double>();
        for (int n = -1022; n <= 1023; n++) {
            x.add(2.0);
            y.add((double) n);
            x.add(0.5);
            y.add((double) -n);
        }
        return new double[][] {x.stream().mapToDouble(Double::doubleValue).toArray(),
                y.stream().mapToDouble(Double::doubleValue).toArray()};
    }

    /** Density functions' {@code pow} on {@code float}s: bases in (0, 10] and exponents in [-4, 4]. */
    private static double[][] floatPowers(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[][] xy = new double[2][n];
        for (int i = 0; i < n; i++) {
            xy[0][i] = (float) (10.0 - random.nextDouble() * 10.0);
            xy[1][i] = (float) random.nextDouble(-4.0, 4.0);
        }
        return xy;
    }

    /** Bases in (0, 100] and exponents in [-10, 10]. */
    private static double[][] broadPowers(long seed, int n) {
        var random = new SplittableRandom(seed);
        double[][] xy = new double[2][n];
        for (int i = 0; i < n; i++) {
            xy[0][i] = 100.0 - random.nextDouble() * 100.0;
            xy[1][i] = random.nextDouble(-10.0, 10.0);
        }
        return xy;
    }

    // ---- Correctly rounded results ----

    /** {@code value} rounded once to the nearest double. */
    private static double round(BigDecimal value) {
        return Double.parseDouble(value.round(MC).toString());
    }

    /** {@code x} reduced by π/2: the remainder in [-π/4, π/4] and the quotient mod 4. */
    private record Reduced(BigDecimal r, int quadrant) {
        static Reduced of(double x) {
            BigDecimal exact = new BigDecimal(x);
            BigDecimal quotient = exact.divide(HALF_PI, WIDE).setScale(0, RoundingMode.HALF_EVEN);
            BigDecimal r = exact.subtract(quotient.multiply(HALF_PI, WIDE), WIDE);
            return new Reduced(r.round(MC), quotient.toBigInteger().mod(java.math.BigInteger.valueOf(4)).intValue());
        }
    }

    static double sinExact(double x) {
        if (x == 0.0) {
            return x;
        }
        var reduced = Reduced.of(x);
        return round(switch (reduced.quadrant) {
            case 0 -> sinSeries(reduced.r);
            case 1 -> cosSeries(reduced.r);
            case 2 -> sinSeries(reduced.r).negate();
            default -> cosSeries(reduced.r).negate();
        });
    }

    static double cosExact(double x) {
        var reduced = Reduced.of(x);
        return round(switch (reduced.quadrant) {
            case 0 -> cosSeries(reduced.r);
            case 1 -> sinSeries(reduced.r).negate();
            case 2 -> cosSeries(reduced.r).negate();
            default -> sinSeries(reduced.r);
        });
    }

    /** sin r = r − r³/3! + r⁵/5! − ... */
    private static BigDecimal sinSeries(BigDecimal r) {
        BigDecimal r2 = r.multiply(r, MC);
        BigDecimal term = r;
        BigDecimal sum = r;
        for (int n = 1; term.abs().compareTo(EPSILON) > 0; n++) {
            term = term.multiply(r2, MC).divide(BigDecimal.valueOf((2L * n) * (2L * n + 1)), MC).negate();
            sum = sum.add(term, MC);
        }
        return sum;
    }

    /** cos r = 1 − r²/2! + r⁴/4! − ... */
    private static BigDecimal cosSeries(BigDecimal r) {
        BigDecimal r2 = r.multiply(r, MC);
        BigDecimal term = BigDecimal.ONE;
        BigDecimal sum = BigDecimal.ONE;
        for (int n = 1; term.abs().compareTo(EPSILON) > 0; n++) {
            term = term.multiply(r2, MC).divide(BigDecimal.valueOf((2L * n - 1) * (2L * n)), MC).negate();
            sum = sum.add(term, MC);
        }
        return sum;
    }

    static double logExact(double x) {
        if (x == 1.0) {
            return 0.0;
        }
        return round(ln(x));
    }

    /** ln x for positive finite x: x = m·2^e with m in [√½, √2), ln x = e·ln 2 + 2·atanh((m − 1)/(m + 1)). */
    private static BigDecimal ln(double x) {
        int scaled = 0;
        if (x < Double.MIN_NORMAL) {
            x *= 0x1p54;
            scaled = 54;
        }
        int e = Math.getExponent(x);
        double m = Math.scalb(x, -e);
        if (m > Math.sqrt(2.0)) {
            m /= 2.0;
            e += 1;
        }
        BigDecimal mantissa = new BigDecimal(m);
        BigDecimal z = mantissa.subtract(BigDecimal.ONE).divide(mantissa.add(BigDecimal.ONE), MC);
        return BigDecimal.valueOf(e - scaled).multiply(LN2, MC).add(atanh2(z), MC);
    }

    /** 2·atanh z = 2(z + z³/3 + z⁵/5 + ...), for |z| well below 1. */
    private static BigDecimal atanh2(BigDecimal z) {
        BigDecimal z2 = z.multiply(z, MC);
        BigDecimal power = z;
        BigDecimal sum = z;
        for (int n = 1; power.abs().compareTo(EPSILON) > 0; n++) {
            power = power.multiply(z2, MC);
            sum = sum.add(power.divide(BigDecimal.valueOf(2L * n + 1), MC), MC);
        }
        return sum.multiply(TWO, MC);
    }

    /** x^y for positive x: exactly when y is a small integer, else exp(y·ln x). */
    static double powExact(double x, double y) {
        if (y == 0.0 || x == 1.0) {
            return 1.0;
        }
        if (y == Math.rint(y) && Math.abs(y) <= 1100) {
            BigDecimal power = new BigDecimal(x).pow((int) Math.abs(y));
            return round(y > 0 ? power : BigDecimal.ONE.divide(power, MC));
        }
        BigDecimal t = new BigDecimal(y).multiply(ln(x), MC);
        // exp t = 2^k · exp s, with t = k·ln 2 + s and |s| ≤ ln 2 / 2.
        BigDecimal k = t.divide(LN2, MC).setScale(0, RoundingMode.HALF_EVEN);
        BigDecimal s = t.subtract(k.multiply(LN2, MC), MC);
        BigDecimal term = BigDecimal.ONE;
        BigDecimal sum = BigDecimal.ONE;
        for (int n = 1; term.abs().compareTo(EPSILON) > 0; n++) {
            term = term.multiply(s, MC).divide(BigDecimal.valueOf(n), MC);
            sum = sum.add(term, MC);
        }
        int exponent = k.intValueExact();
        BigDecimal scale = TWO.pow(Math.abs(exponent));
        return round(exponent >= 0 ? sum.multiply(scale, MC) : sum.divide(scale, MC));
    }
}
