//! Bit-exact ports of Minecraft's noise: Perlin, noise stacks and normal noise.
//!
//! 26.3 samples noise in `f32` from `f64` coordinates (`net.minecraft.world.level.levelgen.synth`:
//! `GradientNoise`, `PerlinNoise`, `SimplexNoise`, `NoiseStack`, `NormalNoise`). Each item cites the class and
//! method it ports, and floating-point operations follow the game's order exactly. See
//! `docs/specs/noise.md`.
//!
//! 26.2's noise is another algorithm family, all in `f64`, in [`v1_21`] (ADR 0026). Noise
//! parameters say which family they're for ([`NormalNoiseParameters`]), and 26.3's
//! [`NormalNoise`] refuses 26.2's.

pub mod mth;
mod normal;
mod perlin;
mod simplex;
mod stack;
pub mod v1_21;
mod volume;

pub use normal::{
    NoiseError, NormalNoise, NormalNoiseParameters, Normalization, OctaveInfo, Parameters,
};
pub use perlin::{PerlinNoise, SmearedPerlinNoise};
pub use simplex::SimplexNoise;
pub use stack::{Layer, Noise, NoiseStack};
pub use volume::Volume;
