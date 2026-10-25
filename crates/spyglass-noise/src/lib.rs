//! Bit-exact ports of Minecraft's noise: Perlin, noise stacks and normal noise.
//!
//! 26.3 samples noise in `f32` from `f64` coordinates (`net.minecraft.world.level.levelgen.synth`:
//! `GradientNoise`, `PerlinNoise`, `NoiseStack`, `NormalNoise`). Each item cites the class and
//! method it ports, and floating-point operations follow the game's order exactly. See
//! `docs/specs/noise.md`.

pub mod mth;
mod normal;
mod perlin;
mod stack;

pub use normal::{NoiseError, NormalNoise, NormalNoiseParameters, Normalization, OctaveInfo};
pub use perlin::PerlinNoise;
pub use stack::{Layer, NoiseStack};
