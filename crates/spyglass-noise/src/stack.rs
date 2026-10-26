//! `NoiseStack`: a weighted sum of noise layers at different frequencies.

use crate::perlin::{PerlinNoise, SmearedPerlinNoise};

/// `Noise`: one octave a stack can hold.
pub trait Noise {
    /// `get(double, double, double)`.
    fn get(&self, x: f64, y: f64, z: f64) -> f32;
}

impl Noise for PerlinNoise {
    fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        PerlinNoise::get(self, x, y, z)
    }
}

impl Noise for SmearedPerlinNoise {
    fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        SmearedPerlinNoise::get(self, x, y, z)
    }
}

/// `NoiseStack.Layer`: one noise, sampled at `frequency` times the input, weighted by
/// `amplitude`.
#[derive(Debug, Clone)]
pub struct Layer<N = PerlinNoise> {
    /// The noise.
    pub noise: N,
    /// Input scale.
    pub frequency: f64,
    /// Output weight.
    pub amplitude: f32,
}

/// `NoiseStack`, built as `NoiseStack.Perlin` or `NoiseStack.SmearedPerlin` when every layer is
/// of one kind, which is all this port supports. Both sum the same way.
#[derive(Debug, Clone)]
pub struct NoiseStack<N = PerlinNoise> {
    layers: Vec<Layer<N>>,
}

impl<N> Default for NoiseStack<N> {
    fn default() -> Self {
        NoiseStack { layers: Vec::new() }
    }
}

impl<N: Noise> NoiseStack<N> {
    /// A stack of `layers`, in order. Order matters: the sum is accumulated in `f32`.
    pub fn new(layers: Vec<Layer<N>>) -> Self {
        NoiseStack { layers }
    }

    /// The layers, in order.
    pub fn layers(&self) -> &[Layer<N>] {
        &self.layers
    }

    /// `get(double, double, double)`: `sum += amplitude * noise.get(x * f, y * f, z * f)` in
    /// layer order.
    pub fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        self.layers.iter().fold(0.0, |sum, layer| {
            let f = layer.frequency;
            sum + layer.amplitude * layer.noise.get(x * f, y * f, z * f)
        })
    }
}

impl NoiseStack<PerlinNoise> {
    /// `get(double, double)`: each layer's 2D noise.
    pub fn get_2d(&self, x: f64, z: f64) -> f32 {
        self.layers.iter().fold(0.0, |sum, layer| {
            let f = layer.frequency;
            sum + layer.amplitude * layer.noise.get_2d(x * f, z * f)
        })
    }
}
