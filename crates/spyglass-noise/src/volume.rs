//! `DensityVolume`: a box of block positions on a regular grid, which the volume paths of noises
//! and density functions fill in one call.

/// `DensityVolume`: `size` positions along each axis, from `min`, `step` blocks apart, all in
/// x, y, z order. Values are laid out with y fastest, then x, then z, as
/// [`index`](Volume::index) gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Volume {
    /// Positions along x, y and z, each at least 1.
    pub size: [i32; 3],
    /// The first block along each axis.
    pub min: [i32; 3],
    /// Blocks between positions along each axis, each at least 1.
    pub step: [i32; 3],
}

impl Volume {
    /// A volume of `size` positions from `min`, `step` apart.
    ///
    /// # Panics
    ///
    /// If a size or step isn't positive, as the game's constructor throws.
    pub fn new(size: [i32; 3], min: [i32; 3], step: [i32; 3]) -> Self {
        assert!(
            size.iter().all(|&s| s > 0),
            "volume sizes must be positive: {size:?}"
        );
        assert!(
            step.iter().all(|&s| s > 0),
            "volume steps must be positive: {step:?}"
        );
        Volume { size, min, step }
    }

    /// A volume of every block in a box: steps of 1.
    pub fn blocks(size: [i32; 3], min: [i32; 3]) -> Self {
        Volume::new(size, min, [1, 1, 1])
    }

    /// The number of positions (`size()`).
    pub fn len(&self) -> usize {
        self.size.iter().map(|&s| s as usize).product()
    }

    /// Always false: every axis holds at least one position.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// `indexUnchecked(x, y, z)`: where position `(x, y, z)`, counted along each axis, lies in a
    /// buffer.
    pub fn index(&self, x: i32, y: i32, z: i32) -> usize {
        (y + (x + z * self.size[0]) * self.size[1]) as usize
    }

    /// `blockX(i)`: the block at position `i` along x. Wraps like the game's `int` arithmetic.
    pub fn block_x(&self, i: i32) -> i32 {
        self.min[0].wrapping_add(i.wrapping_mul(self.step[0]))
    }

    /// `blockY(i)`.
    pub fn block_y(&self, i: i32) -> i32 {
        self.min[1].wrapping_add(i.wrapping_mul(self.step[1]))
    }

    /// `blockZ(i)`.
    pub fn block_z(&self, i: i32) -> i32 {
        self.min[2].wrapping_add(i.wrapping_mul(self.step[2]))
    }

    /// `maxBlockX()`, `maxBlockY()` and `maxBlockZ()`: the last block the volume covers along
    /// each axis, `min + size · step − 1`. A step above 1 makes that past the last position.
    pub fn max_block(&self) -> [i32; 3] {
        std::array::from_fn(|a| {
            self.min[a]
                .wrapping_add(self.size[a].wrapping_mul(self.step[a]))
                .wrapping_sub(1)
        })
    }
}
