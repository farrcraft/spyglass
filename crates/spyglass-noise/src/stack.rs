//! `NoiseStack`: a weighted sum of noise layers at different frequencies.

use crate::perlin::PerlinNoise;

/// `NoiseStack.Layer`: one noise, sampled at `frequency` times the input, weighted by
/// `amplitude`.
#[derive(Debug, Clone)]
pub struct Layer {
    /// The noise.
    pub noise: PerlinNoise,
    /// Input scale.
    pub frequency: f64,
    /// Output weight.
    pub amplitude: f32,
}

/// `NoiseStack` (built as `NoiseStack.Perlin` when every layer is Perlin, which is all this port
/// supports so far).
#[derive(Debug, Clone, Default)]
pub struct NoiseStack {
    layers: Vec<Layer>,
}

impl NoiseStack {
    /// A stack of `layers`, in order. Order matters: the sum is accumulated in `f32`.
    pub fn new(layers: Vec<Layer>) -> Self {
        NoiseStack { layers }
    }

    /// The layers, in order.
    pub fn layers(&self) -> &[Layer] {
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

    /// `get(double, double)`: each layer's 2D noise.
    pub fn get_2d(&self, x: f64, z: f64) -> f32 {
        self.layers.iter().fold(0.0, |sum, layer| {
            let f = layer.frequency;
            sum + layer.amplitude * layer.noise.get_2d(x * f, z * f)
        })
    }
}
