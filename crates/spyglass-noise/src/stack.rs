//! `NoiseStack`: a weighted sum of noise layers at different frequencies.

use crate::perlin::{PerlinNoise, SmearedPerlinNoise};
use crate::volume::Volume;

/// `Noise`: one octave a stack can hold.
pub trait Noise {
    /// `get(double, double, double)`.
    fn get(&self, x: f64, y: f64, z: f64) -> f32;

    /// `addToVolume(DensityBuffer, DensityVolume, double, double, float)`: adds
    /// `factor · noise` at each of `volume`'s positions, scaled, to `buffer`.
    fn add_to_volume(
        &self,
        buffer: &mut [f32],
        volume: &Volume,
        xz_scale: f64,
        y_scale: f64,
        factor: f32,
    );
}

impl Noise for PerlinNoise {
    fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        PerlinNoise::get(self, x, y, z)
    }

    fn add_to_volume(&self, b: &mut [f32], v: &Volume, xz: f64, y: f64, factor: f32) {
        PerlinNoise::add_to_volume(self, b, v, xz, y, factor);
    }
}

impl Noise for SmearedPerlinNoise {
    fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        SmearedPerlinNoise::get(self, x, y, z)
    }

    fn add_to_volume(&self, b: &mut [f32], v: &Volume, xz: f64, y: f64, factor: f32) {
        SmearedPerlinNoise::add_to_volume(self, b, v, xz, y, factor);
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

    /// `addToVolume`: each layer adds its noise at `scale · frequency`, weighted by
    /// `factor · amplitude`, in layer order. The scales and weights are multiplied before
    /// sampling, unlike [`get`](Self::get)'s, so the two paths can differ in the low bits.
    pub fn add_to_volume(
        &self,
        buffer: &mut [f32],
        volume: &Volume,
        xz_scale: f64,
        y_scale: f64,
        factor: f32,
    ) {
        for layer in &self.layers {
            let f = layer.frequency;
            layer.noise.add_to_volume(
                buffer,
                volume,
                xz_scale * f,
                y_scale * f,
                factor * layer.amplitude,
            );
        }
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
