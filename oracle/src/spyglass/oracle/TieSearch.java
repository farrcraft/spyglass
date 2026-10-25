package spyglass.oracle;

import java.util.Arrays;
import java.util.Comparator;
import java.util.List;

import net.minecraft.world.level.biome.Climate;

/**
 * Every parameter point at the minimum fitness for a target, not just one of them.
 *
 * <p>{@code Climate.RTree.search} returns one point at the minimum and, when several tie, which one
 * depends on the thread's previous result. This search is exact instead: a bounding-box tree over
 * the same seven dimensions, pruning only boxes strictly farther than the best found so far, and
 * collecting every point that equals it.
 *
 * <p>Distances are computed on flat arrays for speed, with the same arithmetic as the game: per
 * dimension {@code Climate.Parameter.distance}, squared as {@code Mth.square} does, summed in
 * {@code long}. The seventh dimension is the point's offset, against a target of 0, as in
 * {@code Climate.ParameterPoint.parameterSpace} and {@code Climate.TargetPoint.toParameterArray}.
 * For a point that is exactly {@code Climate.ParameterPoint.fitness}; {@link #bruteForce} uses the
 * game's own {@code fitness} to check it.
 */
final class TieSearch {
    private static final int LEAF_SIZE = 8;
    private static final int DIMENSIONS = 7;

    private final Climate.ParameterPoint[] points;
    /** The value (e.g. biome index) each point maps to, in tree order. */
    private final int[] values;

    /** Point bounds, {@code DIMENSIONS} per point, in tree order. */
    private final long[] pointMin, pointMax;
    /** Original index of each point in tree order. */
    private final int[] order;

    /** Node bounds, {@code DIMENSIONS} per node. */
    private long[] nodeMin = new long[0], nodeMax = new long[0];
    /** Children of inner nodes, or -1 for a leaf. */
    private int[] left = new int[0], right = new int[0];
    /** A leaf's range in tree order. */
    private int[] start = new int[0], end = new int[0];
    private int nodes;

    /** Per-search scratch. A {@code TieSearch} is used by one thread. */
    private long best;
    private int[] hits = new int[16];
    private int hitCount;
    private final long[] target = new long[DIMENSIONS];

    TieSearch(List<Climate.ParameterPoint> points, int[] values) {
        this.points = points.toArray(Climate.ParameterPoint[]::new);
        int n = this.points.length;
        Integer[] ids = new Integer[n];
        Arrays.setAll(ids, i -> i);
        long[][] bounds = new long[n][];
        for (int i = 0; i < n; i++) {
            bounds[i] = bounds(this.points[i]);
        }
        build(ids, 0, n, bounds);
        this.order = Arrays.stream(ids).mapToInt(Integer::intValue).toArray();
        this.values = Arrays.stream(order).map(i -> values[i]).toArray();
        this.pointMin = new long[n * DIMENSIONS];
        this.pointMax = new long[n * DIMENSIONS];
        for (int i = 0; i < n; i++) {
            for (int d = 0; d < DIMENSIONS; d++) {
                pointMin[i * DIMENSIONS + d] = bounds[order[i]][d];
                pointMax[i * DIMENSIONS + d] = bounds[order[i]][DIMENSIONS + d];
            }
        }
    }

    /** Min then max per dimension, as in {@code Climate.ParameterPoint.parameterSpace}. */
    private static long[] bounds(Climate.ParameterPoint p) {
        Climate.Parameter[] space = {p.temperature(), p.humidity(), p.continentalness(), p.erosion(), p.depth(),
            p.weirdness(), new Climate.Parameter(p.offset(), p.offset())};
        long[] b = new long[2 * DIMENSIONS];
        for (int d = 0; d < DIMENSIONS; d++) {
            b[d] = space[d].min();
            b[DIMENSIONS + d] = space[d].max();
        }
        return b;
    }

    /** Builds the node for {@code ids[from, to)}, reordering that range, and returns its index. */
    private int build(Integer[] ids, int from, int to, long[][] bounds) {
        int node = nodes++;
        if (node >= left.length) {
            int size = Math.max(16, left.length * 2);
            nodeMin = Arrays.copyOf(nodeMin, size * DIMENSIONS);
            nodeMax = Arrays.copyOf(nodeMax, size * DIMENSIONS);
            left = Arrays.copyOf(left, size);
            right = Arrays.copyOf(right, size);
            start = Arrays.copyOf(start, size);
            end = Arrays.copyOf(end, size);
        }
        for (int d = 0; d < DIMENSIONS; d++) {
            long min = Long.MAX_VALUE, max = Long.MIN_VALUE;
            for (int i = from; i < to; i++) {
                min = Math.min(min, bounds[ids[i]][d]);
                max = Math.max(max, bounds[ids[i]][DIMENSIONS + d]);
            }
            nodeMin[node * DIMENSIONS + d] = min;
            nodeMax[node * DIMENSIONS + d] = max;
        }
        start[node] = from;
        end[node] = to;
        if (to - from <= LEAF_SIZE) {
            left[node] = right[node] = -1;
            return node;
        }
        // Split at the median along the dimension that minimises the children's total box span,
        // the cost Climate.RTree.build minimises. Any split is correct; this one prunes well.
        int mid = (from + to) >>> 1;
        int bestDim = 0;
        long bestCost = Long.MAX_VALUE;
        for (int d = 0; d < DIMENSIONS; d++) {
            sortByCenter(ids, from, to, bounds, d);
            long cost = cost(ids, from, mid, bounds) + cost(ids, mid, to, bounds);
            if (cost < bestCost) {
                bestCost = cost;
                bestDim = d;
            }
        }
        sortByCenter(ids, from, to, bounds, bestDim);
        int l = build(ids, from, mid, bounds);
        int r = build(ids, mid, to, bounds);
        left[node] = l;
        right[node] = r;
        return node;
    }

    private static void sortByCenter(Integer[] ids, int from, int to, long[][] bounds, int d) {
        Arrays.sort(ids, from, to, Comparator.comparingLong(id -> bounds[id][d] + bounds[id][DIMENSIONS + d]));
    }

    /** Total span of the box around {@code ids[from, to)}, over all dimensions. */
    private static long cost(Integer[] ids, int from, int to, long[][] bounds) {
        long cost = 0;
        for (int d = 0; d < DIMENSIONS; d++) {
            long min = Long.MAX_VALUE, max = Long.MIN_VALUE;
            for (int i = from; i < to; i++) {
                min = Math.min(min, bounds[ids[i]][d]);
                max = Math.max(max, bounds[ids[i]][DIMENSIONS + d]);
            }
            cost += max - min;
        }
        return cost;
    }

    /** Sum over dimensions of the squared {@code Climate.Parameter.distance} to [min, max]. */
    private long distance(long[] min, long[] max, int at) {
        long sum = 0;
        for (int d = 0; d < DIMENSIONS; d++) {
            long v = target[d];
            long above = v - max[at + d];
            long below = min[at + d] - v;
            long dist = above > 0 ? above : Math.max(below, 0);
            sum += dist * dist;
        }
        return sum;
    }

    /** The distinct values of every point at the minimum fitness, sorted ascending. */
    int[] search(Climate.TargetPoint t) {
        target[0] = t.temperature();
        target[1] = t.humidity();
        target[2] = t.continentalness();
        target[3] = t.erosion();
        target[4] = t.depth();
        target[5] = t.weirdness();
        target[6] = 0;
        best = Long.MAX_VALUE;
        hitCount = 0;
        visit(0);
        return distinctValues();
    }

    private void visit(int node) {
        if (left[node] < 0) {
            for (int i = start[node]; i < end[node]; i++) {
                offer(i, distance(pointMin, pointMax, i * DIMENSIONS));
            }
            return;
        }
        long dl = distance(nodeMin, nodeMax, left[node] * DIMENSIONS);
        long dr = distance(nodeMin, nodeMax, right[node] * DIMENSIONS);
        int first = dl <= dr ? left[node] : right[node];
        int second = dl <= dr ? right[node] : left[node];
        // Prune only strictly farther boxes: a box at exactly the best distance can hold a tie.
        if (Math.min(dl, dr) <= best) {
            visit(first);
        }
        if (Math.max(dl, dr) <= best) {
            visit(second);
        }
    }

    /** Records a point, by tree-order index, if it's at or below the best fitness so far. */
    private void offer(int point, long fitness) {
        if (fitness < best) {
            best = fitness;
            hitCount = 0;
        }
        if (fitness == best) {
            if (hitCount == hits.length) {
                hits = Arrays.copyOf(hits, hits.length * 2);
            }
            hits[hitCount++] = point;
        }
    }

    private int[] distinctValues() {
        if (hitCount == 1) {
            return new int[] {values[hits[0]]};
        }
        return Arrays.stream(hits, 0, hitCount).map(i -> values[i]).distinct().sorted().toArray();
    }

    /** The same answer as {@link #search}, using the game's own fitness on every point. */
    int[] bruteForce(Climate.TargetPoint t) {
        best = Long.MAX_VALUE;
        hitCount = 0;
        for (int i = 0; i < order.length; i++) {
            offer(i, points[order[i]].fitness(t));
        }
        return distinctValues();
    }
}
