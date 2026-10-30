package spyglass.oracle;

import java.lang.reflect.Constructor;

/**
 * Instances of game classes made without running their constructors, as stand-ins for the parts
 * of a server the generator's chunk steps are handed but barely use. A stand-in is a subclass that
 * overrides what the steps call; everything else reads fields that were never set, and fails.
 */
final class StandIns {
    private StandIns() {}

    /**
     * A new {@code type}, with no constructor of its own or its superclasses run: the JDK's
     * serialization constructor, through {@code sun.reflect.ReflectionFactory} (module
     * {@code jdk.unsupported}). Called reflectively, so compiling doesn't warn about it.
     */
    static <T> T allocate(Class<T> type) {
        try {
            Class<?> factoryType = Class.forName("sun.reflect.ReflectionFactory");
            Object factory = factoryType.getMethod("getReflectionFactory").invoke(null);
            var constructor = (Constructor<?>) factoryType
                    .getMethod("newConstructorForSerialization", Class.class, Constructor.class)
                    .invoke(factory, type, Object.class.getDeclaredConstructor());
            return type.cast(constructor.newInstance());
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("can't make a stand-in " + type.getName(), e);
        }
    }
}
