# GPU and SIMD

Whether a GPU or SIMD would speed up spyglass's seed searches and queries without giving up bit-exact answers. Assessed for milestone 21 ([roadmap](../roadmap/README.md)); sources checked on 2026-11-08.

**In short:** not now. The stage a GPU does well, integer placement checks, is already the cheap part of a search on the CPU. The expensive part, climate and terrain in floating point, can't run on a GPU and still match the game bit for bit. SIMD is where [ADR 0031](../adr/0031-flattened-density-evaluation.md) left it.

## Where a search's time goes

- **The placement prefilter** rejects seeds 65,536 at a time from Java's 48-bit LCG, about 394 billion seeds a second on 16 threads ([ADR 0021](../adr/0021-search-engine.md)). It's integer work, the kind GPU seed finders run (below), and it's not the bottleneck.
- **Each seed that survives** costs milliseconds of floating-point work: a biome-presence check is 6 ms on 26.3, 55% of it climate sampling and 20% the parameter search. Two dungeons near spawn take 1.5 to 2 seconds on 16 threads, about half of it density and noise.

So a GPU would have to take over the floating-point stages to matter.

## GPU

### No `f64` on the portable path

- **WGSL has no `f64`.** Its scalar types are `bool`, `AbstractInt`, `AbstractFloat`, `i32`, `u32`, `f32` and `f16` (WGSL, Candidate Recommendation Draft of 6 November 2026, §6.2.5).
- **wgpu offers `f64` only natively, and only on Vulkan:** `Features::SHADER_F64` "Enables 64-bit floating point types in SPIR-V shaders", is listed for Vulkan alone, and is "a native only feature" (wgpu-types 30.0.1). Metal and DX12 builds, and the web, wouldn't have it.
- **Consumer GPUs run `f64` slowly.** NVIDIA's throughput table gives 2 results per clock per multiprocessor for 64-bit add, multiply and multiply-add on compute capabilities 8.6 and 8.9 (GeForce RTX 30 and 40), against 128 for 32-bit: 1/64 the rate. Data-centre parts are faster (64 on 9.0, the H100). AMD's consumer spec pages list FP32 and FP16 rates but not FP64.

The game's noise needs `f64`: Perlin coordinates are `f64` and wrapped in `f64`, and 26.2's density functions are evaluated in `f64` throughout ([noise.md](../specs/noise.md), [density-functions.md](../specs/density-functions.md)).

### `f32` isn't exact either

26.3 sums noise in `f32`, which a GPU has. But WGSL doesn't promise `f32` results that match Java's:

- division is accurate to 2.5 ULP, and `sqrt` inherits the error of `1.0 / inverseSqrt(x)` (§15.7.4.1);
- "No rounding mode is specified. An implementation may round an intermediate result up or down" (§15.7.2);
- "An implementation may reassociate operations. An implementation may fuse operations if the transformed expression is at least as accurate as the original formulation" (§15.7.5);
- subnormal inputs and outputs of those operations "may be flushed to zero" (§15.7.1).

Java rounds every `float` and `double` operation to nearest, in source order. One ULP of difference is enough to change an answer: climate values are quantized by truncating `f * 10000.0f` ([biome-lookup.md](../specs/biome-lookup.md)), lattice cells are chosen by `floor`, and density is compared with 0. A last-bit difference near any of those boundaries changes a biome, a block or a structure's height. Emulating `f64` from `f32` pairs needs correctly rounded `f32` primitives, which these rules don't guarantee either.

### The rest of the work branches

Jigsaw assembly, feature placement and structure checks loop on the RNG's draws and branch on data, so threads of one GPU warp would diverge and run one after another.

### A second implementation of everything

A WGSL port of the noise, density functions, biome search and terrain would be a second code path for every algorithm, which [ADR 0026](../adr/0026-how-game-versions-coexist.md) rules out, kept equal to the first by the oracle comparisons on every version.

### What GPU seed finders do instead

Minecraft@Home, a BOINC project, runs its seed filters as OpenCL kernels, and community crackers such as `tree-cracker` and `vein-cracker` use CUDA. They search Java's `Random`, the 48-bit integer LCG behind structure placement and older features, which a GPU runs exactly. That's the stage spyglass's prefilter already makes cheap.

## SIMD

[ADR 0031](../adr/0031-flattened-density-evaluation.md)'s bit-identical prototype of 26.3's octave loop, batching 8 heights at a time, was 1.2 to 1.3 times faster at the lowest frequencies and 0.7 times elsewhere: the loop is bound by per-value `f64` work and permutation-table lookups, not arithmetic width. Milestone 21's profiles show the same shape: `Node::fill` and `add_to_volume` lead 1:4 height maps (27% and 25% self), and chunk work splits between the point path, volumes and biome lookups, none of them a wide arithmetic kernel. `std::simd` is still unstable. [ADR 0032](../adr/0032-density-programs-only-where-they-pay.md) stands.

## When to look again

- WGSL or wgpu gains `f64` on every backend, with correctly rounded operations and a way to forbid reassociation and fusion.
- A search stage turns out to be integer-only and the bottleneck, for example a new placement or slime-chunk filter.
- A profile shows a kernel bound by arithmetic width.

## Sources

- W3C, [WebGPU Shading Language](https://www.w3.org/TR/WGSL/), Candidate Recommendation Draft, 6 November 2026: §6.2.5 Scalar Types, §15.7.1 to §15.7.5.
- wgpu, [`wgpu_types::Features`](https://docs.rs/wgpu-types/latest/wgpu_types/struct.Features.html), wgpu-types 30.0.1: `SHADER_F64`.
- NVIDIA, [CUDA C++ Programming Guide](https://docs.nvidia.com/cuda/archive/12.8.0/cuda-c-programming-guide/index.html) 12.8, §5.4.1 Arithmetic Instructions, table "Throughput of Native Arithmetic Instructions".
- AMD, [Radeon RX 7900 XTX specifications](https://www.amd.com/en/products/graphics/desktops/radeon/7000-series/amd-radeon-rx-7900xtx.html).
- [Minecraft@Home](https://boincsynergy.ca/wiki/Minecraft@Home) (BOINC Synergy wiki); [Nel-S/tree-cracker](https://github.com/Nel-S/tree-cracker).
