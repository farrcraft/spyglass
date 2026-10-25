//! What every random source and positional factory provides.

/// `net.minecraft.util.RandomSource`: the operations worldgen calls on a random source.
///
/// Each implementation ports its game class exactly, including how many values each call
/// consumes, since skipping or adding one shifts everything after it.
pub trait RandomSource {
    /// The factory [`fork_positional`](Self::fork_positional) returns.
    type Positional: PositionalRandomFactory;

    /// `fork()`: a new, independent source seeded from this one.
    fn fork(&mut self) -> Self
    where
        Self: Sized;
    /// `forkPositional()`: a factory of per-position and per-name sources seeded from this one.
    fn fork_positional(&mut self) -> Self::Positional;
    /// `nextInt()`.
    fn next_int(&mut self) -> i32;
    /// `nextInt(int)`: uniform in `0..bound`.
    ///
    /// # Panics
    ///
    /// If `bound` isn't positive, where the game throws `IllegalArgumentException`.
    fn next_int_bounded(&mut self, bound: i32) -> i32;
    /// `nextLong()`.
    fn next_long(&mut self) -> i64;
    /// `nextBoolean()`.
    fn next_boolean(&mut self) -> bool;
    /// `nextFloat()`: uniform in `[0, 1)`, 24 bits.
    fn next_float(&mut self) -> f32;
    /// `nextDouble()`: uniform in `[0, 1)`, 53 bits.
    fn next_double(&mut self) -> f64;
    /// `nextGaussian()`: Marsaglia's polar method (`MarsagliaPolarGaussian`).
    fn next_gaussian(&mut self) -> f64;
    /// `consumeCount(int)`: skip `count` values, as the game does.
    fn consume_count(&mut self, count: i32);
}

/// `PositionalRandomFactory`: random sources derived from a fixed seed and a position or name.
#[allow(
    clippy::wrong_self_convention,
    reason = "named after the game's fromHashOf and fromSeed"
)]
pub trait PositionalRandomFactory {
    /// The sources this factory makes.
    type Source: RandomSource;

    /// `at(int, int, int)`: a source for a block position.
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Source;
    /// `fromHashOf(String)`: a source for a name, such as a noise id.
    fn from_hash_of(&self, name: &str) -> Self::Source;
    /// `fromSeed(long)`.
    fn from_seed(&self, seed: i64) -> Self::Source;
}
