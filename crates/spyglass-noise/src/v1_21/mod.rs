//! The 1.21 family's noise, which 26.2 has: `ImprovedNoise`, the octave set `PerlinNoise`,
//! `NormalNoise`, `SimplexNoise` and `BlendedNoise`, all in `f64` (ADR 0026, an algorithm family
//! of its own).
//!
//! Construction draws, seeding and the gradient table are 26.3's; the arithmetic isn't, and
//! nothing here is narrowed to `f32`. Ported from the 26.2 bytecode
//! (`net.minecraft.world.level.levelgen.synth`); see `docs/specs/noise.md`, section "26.2".

mod blended;
mod improved;
mod normal;
mod perlin;
mod simplex;

pub use blended::BlendedNoise;
pub use improved::ImprovedNoise;
pub use normal::{NoiseParameters, NormalNoise};
pub use perlin::{PerlinNoise, wrap};
pub use simplex::SimplexNoise;
