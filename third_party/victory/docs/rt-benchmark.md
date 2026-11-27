# Synthetic ray-tracing benchmark

Run from the AnyXboxX root on Windows with an SM 6.6 adapter and Rust.

```powershell
.\tools\bench_victory_rt.ps1 -Adapter 'RTX 5070 Ti' -Label baseline
.\tools\bench_victory_rt.ps1 -Warp -Label warp
```

Use `-Offline` if the locked Cargo dependencies are cached. The runner builds
the `victory-shader` test binary in release mode before taking the GPU turn.
The adapter and driver probe, exact build and executable commands, Git revision,
working tree status, fixture hashes, raw samples, and results go to
`%LOCALAPPDATA%\AnyXboxX-RT`. Use the same registry from each worktree. The
runner holds an exclusive handle to `gpu.lock` and creates `gpu-lock.txt`
with the owner and PID. An existing marker blocks the turn. The runner deletes
its marker after the run. `gpu.lock` can remain after the turn. For other GPU
tests, hold the same lock or claim `gpu-lock.txt` with exclusive creation for
the duration of the test, then release it. If a process is interrupted, check
the owner before removing its stale marker.

## Workloads and oracle

Each frame dispatches 262,144 rays on a 512 × 512 logical grid. Each enabled
ray tests one node through the translated 64-bit `image_bvh64_intersect_ray`
instruction and writes four dwords. RT off uses the same DXIL, pipeline, ray
records, geometry and dispatch dimensions with the instruction masked out.

| Scene | Input |
| --- | --- |
| Static | 128 pre-uploaded 16- and 32-bit box and triangle nodes. Lanes in a group use the same node. |
| Dynamic | The same 128 nodes alternate between two placements. A node-region copy and barrier precede each dispatch. Rays and pointers stay fixed. |
| Heavy | 262,144 node records tiled from the same geometry. An odd stride permutes the node indices so adjacent lanes fetch distant nodes and run different node tests. |

Each geometry phase is read back in separate verification frames with RT on
and off. The CPU reference in `victory-shader/src/bvh.rs` computes the four
expected words for every enabled ray. RT off must return the fixture's four
sentinel words for every ray. A mismatch reports the scene, phase and ray.
The input and result BLAKE3 hashes help check comparisons across builds.

## Timing and comparisons

- Shader translation, pipeline creation, initial uploads and 24 warmup frames
  per mode happen before measurement. The 120 measured frames per mode use
  the same sequence of geometry phases. The order of the two modes alternates
  across pairs so both modes run first for each phase equally often.
- Frame time covers command recording through fence completion, including the
  timestamp query resolve. CPU wait is the interval from submission and signal
  to fence completion. D3D12 queue timestamps cover the geometry copy and
  barrier on the dynamic scene, the dispatch, and the total of those commands.
  Output readbacks and CPU oracle checks happen outside measured frames.
- The median averages the middle two samples. The p95 uses nearest rank.
  RT overhead is the RT-on percentile minus the corresponding RT-off percentile.
  The reported DXGI peaks are the largest process-usage samples after measured
  fences, in local and nonlocal segments. The CPU peak is the process working
  set high-water mark. A dash marks an unavailable measurement.
- For an optimization, apply the same benchmark commit to the base and candidate
  in separate worktrees. Run both with the same adapter, driver, Rust version,
  release profile settings and `VICTORY_*` switches. Compare their RT-on samples
  after checking that the fixture inputs and outputs match. Repeat in reversed
  build order if the difference is close to the variation between runs.
- The benchmark exercises the translated node test over a descriptor-backed
  raw buffer. Game timings for the BVH builder/refit, traversal and lighting
  resolve require a game run. The first capture with a newly working pass and a
  verified output should be retained as that pass's baseline. Keep WARP timings
  with their own adapter results.

The quick correctness checks are available without collecting timings.

```powershell
cargo test --manifest-path third_party/victory/Cargo.toml --locked --release -p victory-shader --lib intersect_tests -- --test-threads=1
```
