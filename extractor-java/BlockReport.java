// The extractor's block behaviour report (ADR 0054): each block state's flags, read from the
// running game, by the state ids reports/blocks.json lists. It calls the game only through
// reflection, so one compiled class works for every version. Rebuild BlockReport.class with
// `cargo xtask java-report` after changing this file.

import java.io.IOException;
import java.io.Writer;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

public final class BlockReport {
    /** The report's format; bump it when its shape changes. */
    private static final int FORMAT = 2;

    /** Flags every version has: the name in the report, then the `BlockState` method. */
    private static final String[][] FLAGS = {
        {"air", "isAir"},
        {"liquid", "liquid"},
        {"solid", "isSolid"},
        {"replaceable", "canBeReplaced"},
        {"solid_render", "isSolidRender"},
    };

    /** Flags only some versions have: written where the method exists. */
    private static final String[][] OPTIONAL_FLAGS = {
        {"blocks_motion", "blocksMotion"},
    };

    private BlockReport() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            System.err.println("usage: BlockReport <output file>");
            System.exit(2);
        }
        Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
        Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);

        Class<?> blockClass = Class.forName("net.minecraft.world.level.block.Block");
        Object states = blockClass.getField("BLOCK_STATE_REGISTRY").get(null);
        Method getId = states.getClass().getMethod("getId", Object.class);
        Class<?> leaves = Class.forName("net.minecraft.world.level.block.LeavesBlock");
        Class<?> behaviour = Class.forName("net.minecraft.world.level.block.state.BlockBehaviour");
        Object fluids =
                Class.forName("net.minecraft.core.registries.BuiltInRegistries").getField("FLUID").get(null);
        Method fluidKey = fluids.getClass().getMethod("getKey", Object.class);
        Class<?> entityBlock = Class.forName("net.minecraft.world.level.block.EntityBlock");
        Class<?> blockPos = Class.forName("net.minecraft.core.BlockPos");
        Class<?> blockState = Class.forName("net.minecraft.world.level.block.state.BlockState");
        Method newBlockEntity = entityBlock.getMethod("newBlockEntity", blockPos, blockState);
        Object origin = blockPos.getField("ZERO").get(null);
        Class<?> randomizable = Class.forName("net.minecraft.world.RandomizableContainer");
        java.util.Set<Object> unmade = new java.util.HashSet<>();

        StringBuilder out = new StringBuilder();
        out.append("{\"format\":").append(FORMAT).append(",\"states\":[");
        boolean first = true;
        for (Object state : (Iterable<?>) states) {
            Class<?> c = state.getClass();
            out.append(first ? "\n" : ",\n");
            first = false;
            out.append("{\"id\":").append(getId.invoke(states, state));
            for (String[] flag : FLAGS) {
                flag(out, flag[0], call(state, c.getMethod(flag[1])));
            }
            for (String[] flag : OPTIONAL_FLAGS) {
                Method m = method(c, flag[1]);
                if (m != null) {
                    flag(out, flag[0], call(state, m));
                }
            }
            Object block = c.getMethod("getBlock").invoke(state);
            flag(out, "leaves", leaves.isInstance(block));
            // Whether its block entity takes a loot table and seed: what a template's block draws
            // a seed for. A block entity that can't be made outside a world (a vault, whose
            // config makes items) is named on stderr and taken as no container.
            boolean container = false;
            if (entityBlock.isInstance(block)) {
                try {
                    container = randomizable.isInstance(newBlockEntity.invoke(block, origin, state));
                } catch (InvocationTargetException e) {
                    if (unmade.add(block)) {
                        System.err.println("no block entity for " + block + ": " + e.getCause());
                    }
                }
            }
            flag(out, "loot_container", container);
            String updater = declaring(block.getClass(), behaviour, "updateShape");
            if (updater != null) {
                out.append(",\"shape_updater\":\"").append(updater).append('"');
            }
            Object fluid = c.getMethod("getFluidState").invoke(state);
            Class<?> f = fluid.getClass();
            if (!(Boolean) f.getMethod("isEmpty").invoke(fluid)) {
                Object type = f.getMethod("getType").invoke(fluid);
                out.append(",\"fluid\":{\"id\":\"")
                        .append(fluidKey.invoke(fluids, type))
                        .append("\",\"amount\":")
                        .append(f.getMethod("getAmount").invoke(fluid))
                        .append(",\"source\":")
                        .append(f.getMethod("isSource").invoke(fluid))
                        .append('}');
            }
            out.append('}');
        }
        out.append("\n]}\n");
        try (Writer w = Files.newBufferedWriter(Path.of(args[0]), StandardCharsets.UTF_8)) {
            w.write(out.toString());
        } catch (IOException e) {
            System.err.println("couldn't write " + args[0] + ": " + e);
            System.exit(1);
        }
    }

    /** Appends a flag only when it's set, so the report lists what's true. */
    private static void flag(StringBuilder out, String name, boolean value) {
        if (value) {
            out.append(",\"").append(name).append("\":true");
        }
    }

    private static boolean call(Object state, Method m)
            throws IllegalAccessException, InvocationTargetException {
        return (Boolean) m.invoke(state);
    }

    /**
     * The simple name of the first class from {@code c} up to {@code base} (not {@code base}
     * itself) that declares a method named {@code name}, or null: which class's override of
     * {@code BlockBehaviour}'s a block runs.
     */
    private static String declaring(Class<?> c, Class<?> base, String name) {
        for (; c != null && c != base; c = c.getSuperclass()) {
            for (Method m : c.getDeclaredMethods()) {
                if (m.getName().equals(name)) {
                    return c.getSimpleName();
                }
            }
        }
        return null;
    }

    /** A public no-argument method, or null if this version doesn't have it. */
    private static Method method(Class<?> c, String name) {
        try {
            return c.getMethod(name);
        } catch (NoSuchMethodException e) {
            return null;
        }
    }
}
