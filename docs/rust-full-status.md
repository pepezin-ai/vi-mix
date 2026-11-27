# State of the `rust/full` branch

French version: [fr/rust-full-status.md](fr/rust-full-status.md). Plan: [architecture-rust.md](architecture-rust.md).

## Base: main (Neptune VIctory)

This branch now contains `origin/main` (merge of PR #277 and everything before it): `third_party/victory` is main's Neptune tree, with two of our patches re-applied on top (VICTORY_KEYBOARD and VICTORY_WINDOW_MODE=borderless, commit b27dbae4) and the rust/full changes to xmem (EraFields) and xasync (commit following it). `anyxboxx run` gives VIctory main's launcher set: the release build, the Neptune switch set (46 variables: 39 measured on 2026-11-25, plus the three switches of PR #358, the three of PR #383 and `VICTORY_HEAP_SCAN_ALT`, measured on 2026-11-26), `-Quality` (`--quality`), `--override`, `--auto-a`, `--frametime-dump`, `--warm`, `--precompile`, `--diag`/`--dred`. The golden copy of main's script is `tests/fixtures/lancer_victory_neptune.ps1`; `tests/test_anyxboxx_run.py` and `tests/test_lanceur_victory.py` check the Rust settings against it. The graphical launcher starts the game detached (its log under `%LOCALAPPDATA%\AnyXboxX\logs`), closes once the game is up unless "Keep the launcher open" is on, and warms the shader cache automatically when the marker of this build and environment is missing or stale (`warm-state.txt`).

Ported since: the packed f16, mixed-precision FMA and `v_dot2_f32_f16` forms of VOP3P, and `v_pk_fmac_f16` (VOP2 and VOP3), are in `crates/anyxboxx-rdna2-ref` (`half.rs`, `pkf16.rs`, `pkf16_mix.rs`); the parity tests of `tests/test_rdna2_ref_parity.py` pass again (38 of 38; 1808 pytest tests passed on 2026-11-26).

## Test it locally (Windows)

```powershell
git fetch origin
git checkout rust/full
git submodule update --init
cargo build --release --workspace

# Command line (replaced tools\lancer_victory.ps1: same options, same environment):
.\target\release\anyxboxx.exe run --game-dir "C:\path\to\your\game" --xg-dll "C:\path\to\your\game\dll\xg_xs.dll" --precompile

# Graphical launcher (double-click target\release\anyxboxx-launcher.exe, or):
.\target\release\anyxboxx-launcher.exe
```

- `anyxboxx run` first builds VIctory (`cargo build --profile diag -p victory --features diagnostics`), like the script, so the first launch takes several minutes. `--precompile` fills the shader cache before the game (about 4.5 minutes the first time, measured on 2026-11-25). The first cold launch may crash while pipelines compile; launch again once the cache is full.
- The PowerShell option names are accepted too (`--GameDir`, `--XgDll`, `--XgMode`, `--Precompile`, `--Dred`, `--NoTunedSettings`, `--SkipPipelines`, `--BatchCardRehash`). `anyxboxx run --help` lists them.
- One game at a time: close any running game first. The graphical launcher checks this itself.

### Linux (Proton)

Native Linux builds are not supported: VIctory, which the root workspace uses through `victory-hle`, only builds for Windows (`victory-gpu` uses its `#[cfg(windows)]` modules unconditionally). Cross-build both workspaces for Windows instead, with an MSVC CRT and SDK on Linux (for example [xwin](https://github.com/Jake-Shadle/xwin)) and `lld-link`:

```bash
cargo build --release --workspace --target x86_64-pc-windows-msvc \
    --exclude anyxboxx-kernelx --exclude anyxboxx-d3d12-xs
(cd third_party/victory && cargo build --profile diag -p victory --features diagnostics --target x86_64-pc-windows-msvc)
```

- `kernelx` and `d3d12_xs` need MSVC's `lib.exe` to make the `.exp` of their forwarder table (`anyxboxx-forwarders::link`); `llvm-lib /DEF` writes only the `.lib`. Not blocking: the game runs through VIctory, which does not load these DLLs.
- There is no cargo inside Proton, so `anyxboxx.exe run` takes the cross-built VIctory with `--victory-exe` and skips the build (the repository is then not needed). Run it with Proton's `wine` and the Wine prefix of your choice, paths written as `Z:\…`:

```bash
VKD3D_CONFIG=descriptor_heap "$PROTON/files/bin/wine" target/x86_64-pc-windows-msvc/release/anyxboxx.exe run \
    --victory-exe 'Z:\…\third_party\victory\target\x86_64-pc-windows-msvc\diag\victory.exe' \
    --game-dir 'Z:\…\your game folder'
```

- `anyxboxx run` sets the Windows tuned set (`TUNED`), not the `linux-amd` profile, which only the graphical launcher reads. Wine passes the Linux environment on, so export `VKD3D_CONFIG=descriptor_heap` (the title's view heap is larger than vkd3d-proton's default cap) and, for the `linux-amd` values, add `--no-tuned-settings` and export them yourself.
- The graphical launcher also accepts `--victory-exe <path>` (or `--victory-exe=<path>`) to run a prebuilt `victory.exe` on its Play and Shaders pages without a cargo build or a local repository checkout.

### Graphical launcher (`crates/anyxboxx-launcher`)

Install and use: [launcher.md](launcher.md). Prebuilt executable: [launcher releases](https://github.com/LooperSalty/AnyXboxX/releases?q=launcher&expanded=true).

Style: a dusk gradient with translucent cards (`theme.rs`, colour maths tested for WCAG AA, including text over the picture overlay blended in linear light), one 8-point spacing scale, one type scale and one set of radii (`tokens.rs`), and hand-painted controls with hover, press and keyboard focus states (`widgets.rs`). The splash pictures are never committed: they are read from `ANYXBOXX_LAUNCHER_SPLASH` (a file or a folder), else the PNG and JPEG files of `%LOCALAPPDATA%\AnyXboxX\launcher\splash\` (several rotate as a slideshow with a cross-fade), else `%LOCALAPPDATA%\AnyXboxX\launcher\splash.png`; without one, the gradient and the title are drawn. Home shows it across a hero with a slow zoom and a pointer parallax; when PLAY builds VIctory or warms the shaders, a full-window launch screen (`ui_launch.rs`) shows the stages (Building VIctory, Preloading shaders, Starting the game), a bar fed by VIctory's progress lines, the elapsed time and the time left (`launch_progress.rs`, tested), and Cancel, which kills the running step (`job.rs`). The About lines give the version and the git commit (embedded by `build.rs`). `--page <home|setup|folder|shaders|play>` opens a page; debug builds also take `--preview-launch` to show the launch screen without running anything.

It is built with egui through eframe 0.34: pure Rust, no C++ binding, no installer. The window has four pages with progress bars, a log and a few dialogs, which immediate-mode egui draws in a few hundred lines. Raw Win32 would have meant hand-written message loops and layout for the same pages. The logic lives in plain modules that the window only calls, so the 190 tests never open a window.

| Page | What it does | Limits |
| --- | --- | --- |
| Setup | Checks the Rust toolchain, `d3d12.dll`, the VC++ runtime (present, version not checked), `dxc` from the Windows SDK, and the GPU through DXGI. Links to official pages only; nothing is downloaded. | Driver version shows "unknown": the repository records no minimum yet. |
| Files | Game folder (typed path): presence, empty files, PE section bounds (a truncated exe or DLL), RPF7 header table bounds, and a SHA-256 manifest of the required files and top-level `.rpf`, computed locally. Nothing is copied or uploaded. | There are no reference hashes, so the manifest is a record, not proof. An archive cut after intact tables is not detected. The RPF7 layout was not checked against a real file. |
| Shaders | `victory translate --cache --pipelines --used-pipelines … --jobs N` on `shaders.rpf`, the exe and its two DLLs only, with the game's environment (the cache key depends on it). Progress is read from VIctory's "N of M containers (x%)" lines. It refuses to run while `victory.exe` or the game is running. | |
| Play | Profile chosen by OS and GPU (`profiles.toml`), overrides in the advanced panel, optional `xg_xs.dll` (real or compare), build then launch through the same steps as `anyxboxx run`, per-title saves location (`<data>\<TitleId>\saves`), clean shader cache (asks for confirmation; only a folder named `cache` can be deleted). | VIctory prints fps only in its window title, so there is no fps line to show. |

Profiles (`crates/anyxboxx-launcher/profiles.toml`, one test each):

- **Windows + NVIDIA:** your working set (the six `TUNED` values of `anyxboxx run`, no DRED, no relay). Measured: 2-3 fps on `main` after PR #199, 5-7 fps on the fork at `80d58f8`. Setting PR #199's switches to `0` did not change the `main` figure, so the profile leaves them at VIctory's defaults.
- **Linux (Proton) + AMD:** the values of PR #199's run (RX 9070 XT, RADV, GE-Proton11-12):
  - `SHADER_LOOP_LIMIT=256`, `LATE_WAIT=0`, `WIDE_BACK=0`, `PIPELINE_WAIT_MS=50`
  - `TEXTURE_BUDGET_MB=8192` (sized for a 16 GB card), `TEXTURE_NO_WAIT=1`
  - `CARD_BUFFERS_MIN_MB=1`, `CARD_BUFFERS_MB=1024`
  - `VKD3D_CONFIG=descriptor_heap`
- **Windows + AMD, Intel, others:** VIctory's defaults, marked "unmeasured".

## Component by component

| Component (before) | Now on `rust/full` | Parity check | Old code |
| --- | --- | --- | --- |
| `anyxboxx run` (PowerShell) | `anyxboxx run` (`crates/anyxboxx`) | same calls, `VICTORY_*` environment and exit codes as the script, with a fake cargo and victory.exe | **deleted**; `tests/test_anyxboxx_run.py` keeps the expectations |
| `emu/loader.py` (PE loader) | `crates/anyxboxx-pe` (on `victory-pe`) | `tests/test_rust_pe_parity.py`: 13 images, same JSON and page protections | kept (the Python sandbox still uses it) |
| `--rapport` (`emu/report.py`, `config.py`) | `anyxboxx rapport` (`crates/anyxboxx-report`) | `tests/test_rust_rapport_parity.py`: same text on 7 targets, same refusals on 5 | kept (used by the Python CLI and tools) |
| `--traduction` | `anyxboxx traduction` | `tests/test_rust_traduction_parity.py`: same table, byte-identical SVG on 6 setups | kept |
| `--carte` | `anyxboxx carte` | `tests/test_rust_carte_parity.py`: byte-identical SVG and badges on 5 cases plus 3 synthetic GDK tables | kept |
| `tools/jour_j.py` | `anyxboxx jour-j` | `tests/test_rust_jour_j_parity.py`: static part of `rapport.md` byte-identical on 5 cases | kept: the Unicorn and native launch sections are stated as not available in Rust (D2, D4), so the port is partial |
| `--banc` | `anyxboxx banc` | `tests/test_rust_banc_parity.py`: Module, Imports and Resolved columns and the footer | kept: the load/init columns need the Unicorn sandbox (D2) and show `n/d` |
| `rust/` (texture, audio, RDNA 2 bridge) | `crates/` (one workspace) | existing tests unchanged | `rust/Cargo.toml` deleted |
| `runtime/xcrdapi` | `crates/anyxboxx-xcrdapi` | `test_xcrdapi.py`, identical export table | **deleted** |
| `runtime/kernelx` | `crates/anyxboxx-kernelx` (2,661 forwarders) | `test_kernelx.py`, identical export table | **deleted** (the forwarder table moved into the crate) |
| `runtime/d3d12_xs` | `crates/anyxboxx-d3d12-xs` | `test_d3d12_virtual_memory.py`, `test_runtime_cpp.py`, identical export table | **deleted** |
| `runtime/tls` (`anyxboxx_tls.dll`) | `crates/anyxboxx-tls`, a `no_std` cdylib | `test_native_tls.py`, `test_native_tls_callbacks.py`, identical export table | **deleted** |
| `runtime/xmem` | `crates/anyxboxx-xmem` (ERA attributes shared with VIctory's xmem module) | `test_xmem_era.py`, `test_xmem_virtual.py`, `test_runtime_cpp.py`, identical export table | **deleted** |
| `runtime/xgameruntime` | `crates/anyxboxx-xgameruntime` and VIctory `xasync` | 17 PE exports and 26 provider/interface pairs checked against C++ | Native runtime ships the Rust DLL. Inventory and commands below. |
| `runtime/launcher` | `crates/anyxboxx-native-launcher` | `tests/test_rust_native_launcher_parity.py` runs the synthetic native launcher suite against both implementations | kept as `anyxboxx_cpp.exe` for parity; TomBox-dependent cases require its DLLs |
| Graphical launcher | `crates/anyxboxx-launcher` (new) | 190 Rust tests | |

How the Rust runtime DLLs and native launcher reach the runtime folder: `cargo build --release` builds them, and `runtime/CMakeLists.txt` copies them from `target/release` into `runtime/build/bin` (`rust_dlls.cmake`; a missing one stops the build with the command to run). The native launcher is `runtime/build/bin/anyxboxx.exe`; the separate `target/release/anyxboxx.exe` is the VIctory CLI. So `cargo build --release` now comes before `cmake` (CI, AGENTS.md, READMEs).

TLS carrier finding: as a `no_std` cdylib, its TLS block holds exactly the carrier (262,208 bytes, no Rust std thread-locals). The game's template therefore shares it as with the C++ DLL. `no_std` needs `panic = "abort"`, which Cargo only accepts for the whole release profile, so the root workspace's release profile now aborts on panic.

### Windows x64 xgameruntime inventory

The native launcher loads `runtime/build/bin/xgameruntime.dll`, byte-identical to the Cargo artifact. The `xgameruntime_cpp` target builds the oracle in `runtime/build/oracle/xgameruntime.dll`. The measured PE tables match by ordinal and name. Both contain zero forwarders.

| Ordinal | Export |
| ---: | --- |
| 1 | `AnyXboxX_CreateDXGIFactory` |
| 2 | `AnyXboxX_CreateDXGIFactory1` |
| 3 | `AnyXboxX_CreateDXGIFactory2` |
| 4 | `AnyXboxX_NormalizeHdrLuminance` |
| 5 | `AnyXboxX_RegCloseKey` |
| 6 | `AnyXboxX_RegOpenKeyExW` |
| 7 | `AnyXboxX_RegOpenKeyW` |
| 8 | `AnyXboxX_RegQueryValueExW` |
| 9 | `InitializeApiImpl` |
| 10 | `InitializeApiImplEx` |
| 11 | `InitializeApiImplEx2` |
| 12 | `QueryApiImpl` |
| 13 | `UninitializeApiImpl` |
| 14 | `XErrorReport` |
| 15 | `XGameRuntimeInitialize` |
| 16 | `XGameRuntimeUninitialize` |
| 17 | `XSystemGetDeviceType` |

The GDK table names 396 functions. Of these, 262 belong to the 26 pairs served by `QueryApiImpl` in local mode. Aliases share 19 objects. The table lists the 210 slots wired to specific C++ handlers after the three IUnknown methods. Some handlers return errors. The other slots in those tables use the generic refusal. The two `.inc` files give the GUIDs and method names. Of the 262 mapped functions on these objects, 52 use the generic refusal and 12 have explicit offline or uninstall refusals. The reports count the other 198 functions (198/396 overall).

| Object (without the `Interface` suffix) | C++ slots |
| --- | --- |
| TaskQueue | 3–7, 9–30 |
| User | 3–14, 16–38, 42–49 |
| Gamertag, Analytics, Feature | 3 |
| System | 3–8 |
| Game, Display, Error | 3–5 |
| Invite, Activation, Protocol | 3–4 |
| PendingInvite | 5–6 |
| HdrDisplay | 4 |
| Networking | 15–17 |
| GameSave | 3–32 |
| LocalStorage | 3–8 |
| Package | 3–19, 22, 24–26, 30, 32–34, 37–41 |
| Store | 3–16, 19–21, 28–29, 49–54, 58–64, 66–67, 71–78, 82–83 |

`test_rust_xgameruntime_parity.py` compares the two DLLs directly in separate processes. It calls the initializers and all three DXGI factories and checks aliases, IUnknown results, stub placement, all four profile pictures, the bytes of an accented gamertag and callbacks from a Windows event. The Rust queue now handles wait registration at slots 20 and 21, rearming and removal. It preserves waits owned by another queue sharing the port. Slot 0 answers IUnknown and the queue IID. Slot 29 traces a blocking call on a time-sensitive thread.

`test_rust_dlls.py` runs the same scenarios on each DLL, including synthetic PE startup, registry, tasks, users, persistent saves, packages, events, offline Store, HDR, connectivity and DXGI redirection. For XPackage, C++ returns `S_OK` for chunk uninstall without removing chunks. Rust logs `E_NOTIMPL`. Both return `E_NOTIMPL` for UWP and `false` for package uninstall. Rust logs all three with tracing disabled. Unsupported Xbox Live and Store methods retain their offline limitations.

Rust uses VIctory’s `xasync` queue. The tests cover the scenarios named here. Full Microsoft Task scheduling, including callback order and thread affinity during concurrent termination, remains implementation-specific. The Rust config reader accepts multiline attributes and retains its overflow check on version components.

**Building and retaining the oracles**

The first three build commands below produce and stage the Rust runtime. The `rust_dlls` target suffices for distribution. The following CMake command builds the three test oracles. C++ still needs the MIT Task module and `runtime/common`. Rust embeds `gdk_api_table.inc` and `served_interfaces.inc`. The Python and Rust reports read those tables and `unsupported.txt`. A parity test checks that inventory against both DLLs. Only reports on older checkouts without the inventory fall back to C++ comments. The C++ source files and the MIT Task implementation remain inputs to the oracle build. Distribution needs only the staged Rust runtime binaries from `bin`. The C++ `launcher` and `gpu_runner` directories supply the comparison executables for launcher and GPU tests.

Signatures checked offline in the locally installed GDK 261000 headers, including `XTaskQueue.h`, `XThread.h`, `XUser.h` and `XPackage.h`. [Microsoft GDK public reference](https://learn.microsoft.com/gaming/gdk/)

```powershell
$env:CARGO_BUILD_JOBS = "4"
$env:CARGO_TARGET_DIR = Join-Path $PWD "target-codex"
cargo build --offline --release --workspace
cmake -S runtime -B runtime/build -A x64 "-DANYXBOXX_RUST_RELEASE=$env:CARGO_TARGET_DIR/release"
cmake --build runtime/build --config Release --target rust_dlls
cmake --build runtime/build --config Release --target xgameruntime_cpp anyxboxx_cpp anyxboxx_gpu_run_cpp
$env:ANYXBOXX_REQUIRE_RUNTIME = "1"
python -m pytest -q tests/test_rust_dlls.py tests/test_rust_xgameruntime_parity.py
```

**Still Python:**

- The Unicorn sandbox and its HLE handlers (dropped in the end state, D2).
- `emu/modules.py`, `emu/loader.py`, `report.py`, `status_map.py`, `translation.py`, `bench.py`, `tools/jour_j.py`: they have Rust ports with parity, but other Python code (the CLI, the GUI window, the sandbox) still imports them. They go together with the sandbox.
- RDNA 2 (`emu/rdna2*.py`, phase 4). The reference interpreter is in Rust, crate `anyxboxx-rdna2-ref` (decoder, mnemonic table generated by `tools/rdna2_ref_tables.py`, wave state, scalar instructions, branches, VOP1, VOP2, VOPC, and VOP3 with its native table, the older encodings, carry, lanes, 64-bit multiply, SAD, division, 64-bit compares, f64 operations, movrel; scalar memory (SMEM: loads, stores, atomics) over a sparse global memory, with the `mem` directive and page output; SDWA and DPP on VOP1 and VOP2; VOP3P integer dot products, packed f16, `v_fma_mix_*`, `v_dot2_f32_f16` and `v_pk_fmac_f16` on VOP2 and VOP3). Also in Rust since: the LDS (DS reads and writes, atomics, `_d16` halves, lane exchanges), FLAT, GLOBAL and SCRATCH, MUBUF untyped accesses, the typed buffer formats of MUBUF and MTBUF (`buffer_*_format`, `tbuffer_*_format`, with the format table `format_table.rs` generated by the same tool from `emu/data/gfx10_formats.json`), the 2D MIMG instructions (image_load, image_store, image_get_resinfo, image_sample, image_sample_c and image_gather4 with their _lz, _l and _o forms, refusing what the Python refuses), the float atomics (fcmpswap, fmin, fmax and their `_x2` forms on FLAT and GLOBAL; fadd, fmin, fmax and fcmpst in f32 and f64 on the LDS; `atomic_float.rs`), and SDWA on the VOPC compares (`ext_compare.rs`). Two float-atomic rules are unverified assumptions carried from the Python: fmin and fmax keep the other value when only one is a NaN (taken from the V_MIN/V_MAX description, not measured), and the NaN fadd produces is what the Python's x86-64 arithmetic gives (first NaN operand, old value first, quieted; the default NaN with its sign set for infinities of opposite signs), a property of the model's host rather than a documented RDNA 2 result. The Python model itself refuses, and the Rust refuses the same, with parity cases for each: the VOP3P operations other than the integer dot products, packed f16 and the mixed-precision and f16 dot forms (the integer dot products with op_sel, neg or clamp; `v_fma_mix_*` with op_sel on an f32 source; `v_dot2_f32_f16` with op_sel[2] or neg_hi[2]; `v_pk_fmac_f16` in SDWA or DPP, and with VOP3 modifiers other than clamp), the op_sel and clamp forms of the one 16-bit-result VOP3 operation (`v_sat_pk_u8_i16`; op_sel is not read on 32-bit results), DPP16 and DPP8 on VOPC, and the `_d16` forms of the typed buffer and image accesses. Since then also in Rust: the graphics instructions, EXP exports (`export.rs`) and VINTRP interpolation (`interp.rs`; the CLI takes `primitive N` and `primitives L:P ...` and prints `exp TARGET LANE X Y Z W` lines), the program counter instructions (s_getpc_b64, s_setpc_b64, s_swappc_b64, s_call_b64; `pc.rs`) and the MUBUF cache controls (buffer_gl0_inv, buffer_gl1_inv). No RDNA 2 instruction family of the interpreter is Python-only any more: a sweep of every encoding the Python executes (1,060 of the 1,490 LLVM encodings plus the five ISA-only ones, zero operands, wave32 and wave64, run on 2026-11-25) finds none that the Python runs and the Rust refuses. The Rust refuses what the Python refuses there too: EXP with COMPR (16-bit exports), VINTRP attributes past param7 and VINTRP opcode 3, and the VOP3 forms of `v_interp_*`. The NaN results of v_interp_p1_f32 and v_interp_p2_f32 are, like those of fadd, the ones the Python's x86-64 arithmetic gives (the left NaN operand first, quieted; the default NaN with its sign set for ∞ × 0 or ∞ − ∞): a property of the model's host, kept as an unverified assumption. The typed-format rules (UNORM and SNORM rounding, DST_SEL handling, V# FORMAT INVALID disabling MTBUF, store restrictions) follow the Python model: what AMD's RDNA 2 ISA states (section 8.1.4, table 31, p. 59; section 8.1.5, p. 62) is cited there, and the rest (UNORM/SNORM rounding to nearest, 16-bit FLOAT NaN widening, UINT/SINT store saturation, 1.0 against 1 for DST_SEL = 1) is labelled assumed, not measured, until LLVM's tests or our own measurement settle it. Division uses `num-bigint` (pure Rust, crates.io) for exact rounding. Parity with the Python model: `tests/test_rdna2_ref_parity.py` (fixed programs, 300 seeded random programs and 200 for packed f16 and `v_pk_fmac_f16`, compared line by line). Until the HLSL side moves, `anyxboxx traduction` reads the RDNA 2 coverage from `crates/anyxboxx-report/data/rdna2-coverage.txt`; a test fails when that table drifts from the Python measure, and `python tests/test_rust_traduction_parity.py` regenerates it.
- `tools/*.py` and the pytest suite (the oracle).

## Checks on this branch (2026-11-25, Windows 11, Rust 1.98.1, Python 3.12)

```
python -m pytest                       → 1251 passed, 11 skipped
python -m ruff check --select W605,F,E9 PrototypeScarlett tests tools → All checks passed!
cargo test --release --workspace       → 198 passed, 0 failed
cargo clippy --release --workspace --all-targets → 0 warnings
cargo fmt --check                      → clean
cargo test --release -p anyxboxx-xgameruntime → 65 passed (including XAsync cleanup/status race regression)
cargo test -p anyxboxx-rdna2-ref      → 122 passed (decoder, wave, memory, integer and float atomics, scalar, vector, SDWA/DPP, VOPC SDWA, compares, VOP3, SAD, division, f64, movrel, dot products, LDS, vector memory, typed formats, images, exports, interpolation, program counter, cache controls, step loop)
python -m pytest tests/test_rdna2_ref_parity.py → 38 passed of 38 (packed f16 and `v_pk_fmac_f16`: 200 seeded programs, fixed programs, EXP (120 seeded programs), VINTRP (150 seeded programs), the program counter instructions, the MUBUF cache controls, 300 seeded random programs, division and f64 dedicated tests, scalar memory, LDS, FLAT/GLOBAL/SCRATCH, MUBUF, typed formats and images, float atomics, VOPC SDWA and DPP (200 seeded programs), and the refusals shared by both models: typed and image `_d16`, VOP3P beyond the dot products, VOP3 16-bit op_sel; Python model vs Rust binary). `test_wide_operations_match_the_python_model` passes since `ea9fee97` (canonical NaN for f64 sqrt and rsq of negative inputs).
```

On 2026-11-26, `cargo build --release --workspace`, `cargo test --release --workspace`, crate Clippy with `-D warnings`, `cargo fmt --check`, and the documented Ruff check passed. CMake built `runtime/build-ninja` with Ninja. The 89-case pytest run used the Rust xgameruntime in a mixed runtime; `tests/test_rust_dlls.py` also compared its exports against the C++ DLL. No game was run.

On 2026-11-27, the release workspace passed 566 Cargo tests. All 1,969 pytest cases passed with the runtime required, the three C++ oracles built and the GPU tests using WARP. The launcher tests used their synthetic recorder under the filename `axg-recorder.exe`. No game or VIctory executable ran. The documented Ruff check, 17 VIctory xasync unit tests and `cargo xtask policy` also passed.

The runtime was built from this branch (cargo, then CMake). With `ANYXBOXX_REQUIRE_RUNTIME=1`, the TomBox DLL tests also need `python tools/build_tombox.py`.

## Findings along the way

- VIctory does not compile without its `diagnostics` feature at this commit (`victory_gpu::queue::presented` is gated by it but used outside it). The root workspace depends on `victory-hle` with that feature, the way VIctory is always built.
- VIctory writes fps only to its window title. A stdout fps line in VIctory would let both launchers show it.
- VIctory's test `a_read_in_place_never_meets_the_gap_of_a_remapping` (victory-hle, xmem) is flaky: it fails about 3 times in 8 on the unmodified file.
- The Windows+NVIDIA profile was corrected after the maintainer's test: PR #199's switches at `0` made no difference (2-3 fps on `main`, 5-7 fps on the fork at `80d58f8`).
- Rust-side port bug, fixed: five places of `crates/anyxboxx-rdna2-ref` indexed a VGPR group without checking that it ends at v255, and the binary aborted on an out-of-bounds index (`wide.rs`, `div.rs`, `vop3_forms.rs`, twice in `sad.rs`): a 64-bit result written at v255 by the f64 operations and conversions to f64, the 64-bit shifts, `v_div_*_f64`, `v_mad_u64_u32` and the packed SAD forms, the 64-bit S2 pair of the packed SAD forms at v255, and the 128-bit source and result of `v_mqsad_u32_u8` at v253 and up (29 mnemonics, found by trying every VOP1, VOP2, VOP3, VOP3P and VOPC opcode with registers at the top of the range; Linux, 2026-11-26). They now return the operand error that `read64` already returned for a source pair at v255, before anything is written (`Wave::vgpr_group`). The Python model raises `IndexError` in the same cases, after writing the low half in the first active lane; whether it should raise `Untranslated` instead, with the Rust following, is left open.
- CI on `rust/full` (first run through PR #228's triggers): the pytest job failed at the Rust DLL copy, because `file(COPY_FILE)` does not create `runtime/build/bin` on a clean build. Fixed in `7bc532ba` (`file(MAKE_DIRECTORY)` first). A dispatched `tests.yml` run on `rust/full` then passed in all three jobs.
- Rust-side port bug, fixed: `crates/anyxboxx-rdna2-ref/src/scalar.rs` zero-extended a 32-bit literal read as S0 by `s_ashr_i64`, `s_bfe_i64` and `s_flbit_i32_i64`, where the Python model sign-extends it (`signed_literal_s0` in `emu/rdna2_exec_bits.py`, from AMD's RDNA 2 ISA, section 6.2.1, p. 43: "64-bit signed integer: sign extended to 64 bits"). With the literal 0xFFFFFFF0, `s_flbit_i32_i64` gave 32 instead of 60. The Rust now sign-extends for these three; the unsigned and bitwise 64-bit sources keep zero-extending. Found by running the parity generator on 48,000 more seeds (Linux, 2026-11-26): the 300 seeded programs did not pair one of these three instructions with a negative literal.
- The xmem test `graphics_blocks_get_their_own_region_flagged_by_the_query` failed now and then: the tests share one address space, and a region freed by one test was reused by another running at the same time. The xmem tests now take turns on one lock (`7b69e6fb`); 15 runs in a row pass.
- Rust-side port bug, fixed: `crates/anyxboxx-rdna2-ref/src/movrel.rs` added the register number and M0 in 32 bits. M0 is a full 32-bit value, so with M0 = 0xFFFFFFFD `s_movrels_b32 s102, s47` read s44 in a release build (the sum wrapped) and panicked in a debug build (`attempt to add with overflow`), where the Python model refuses the index 0x10000002C as outside the model. The sum is now taken in 64 bits and refused like the Python. Same for `s_movrels_b64`, `s_movreld_b32/b64`, `v_movrels_b32`, `v_movreld_b32` and `v_movrelsd_b32`; the `_2` forms and `v_swaprel_b32` take 10-bit fields of M0 and could not overflow. Found by running the parity generator on 6,000 more seeds with special values in the scalar registers (Linux, 2026-11-26); the 300 seeded programs keep M0 below 256 unless an instruction rewrites it.
- The RDNA 2 parity generator used to send branches into data words, which decode as memory instructions (outside the model). Branches now land on instruction starts.
- Python-side model bug (not fixed here, worth an issue): the VOP3 forms of `v_madmk_f32`, `v_madak_f32`, `v_fmamk_f32` and `v_fmaak_f32` (VOP3 opcodes 0x120, 0x121, 0x12C, 0x12D) without a literal operand raise `TypeError` in `emu/rdna2_exec_vop3.py` (the missing literal K reaches the operation as `None`) instead of being refused or run. The Rust reads K as 0. Neither result is a refusal, so the parity tests leave these forms out; found by the encoding sweep above.
- Python-side model bug (not fixed here, worth an issue): `emu/rdna2_exec_wide.py` `_fma` computes `float(Fraction(a) * Fraction(b) + Fraction(c))` for finite operands, and `float()` raises `OverflowError` when the exact result exceeds the double range, instead of returning ±infinity as IEEE fma does. The Rust port returns ±infinity, so the generic fuzz leaves `v_fma_f64` out and a dedicated test covers it with operands that cannot overflow.
- Rust-side port bug, fixed: `v_mac_f32`, `v_madmk_f32` and `v_madak_f32` in `crates/anyxboxx-rdna2-ref/src/vector.rs` computed `a * b + c` as a plain expression, so the compiler chose which NaN came out, and it swapped the operands of the addition: with a NaN factor and a NaN addend, the Rust returned the addend's NaN where the Python model (`_mul_add`, x86-64 arithmetic) returns the factor's (for example `v_madak_f32` with S1 = `0x7FC00B0B` and K = `0x7FC00C0C`: Rust `0x7FC00C0C`, Python `0x7FC00B0B`). `float::mul_add` now writes the model's order out: the first NaN factor, or the NaN the product makes (∞ × 0), before a NaN addend. `v_mad_f32` uses it too; it gave the model's NaN in this build, but only by the compiler's choice. Like the other NaN rules of the port, this follows the model's host arithmetic, not a measured RDNA 2 result. Found by running the parity generator on 60,000 more seeds (Linux, 2026-11-27): five programs ended with a different NaN in a register, the three traced all at a `v_madak_f32` with a NaN K; with this change the same 60,000 programs end with the same values in both models.
- Python-side model inconsistency, fixed: the VOP3 form of `v_cndmask_b32` read its lane mask (S2) inside the loop over active lanes (`emu/rdna2_exec_vop3.py`), so with EXEC = 0 a mask operand the model refuses (a VGPR, a reserved code) was never read and the instruction ran, while the Rust port reads the mask once and refused it. The Python now reads it once before the lanes, like its other VOP3 checks and the Rust; nothing changes for a readable mask. Found by the parity generator: in 120,000 more seeds on 2026-11-27 it was the only difference left between the two models apart from the register groups past v255 (#437). (A sweep of every opcode also shows the half-precision instructions still being ported, #503 and #505; the generator does not draw those yet.)
