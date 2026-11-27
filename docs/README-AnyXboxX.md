# AnyXboxX — Xbox Series X emulator (prototype)

[![Discord](https://img.shields.io/badge/Discord-AnyXboxX-5865F2?logo=discord&logoColor=white)](https://discord.gg/TcX42vRyPU)

**English** · [Français](docs/i18n/README.fr.md)

> [!NOTE]
> **Where we are (2026-11-27)**
>
> - ✅ **The game runs.** GTA VI 1.0.2.1 (Xbox Series build) starts, plays its cutscenes and reaches the open world through VIctory, the copy in `third_party/victory`.
> - 📈 **About 24 fps median in the world in Full HD, 30 fps (the game's own cap) in scenes of up to about 4,000 draws** (Series X mode, RTX 4070 SUPER and i7-13700KF, warm cache, a 14-minute session through the launcher on 2026-11-27), about 20 in 4K, up from 14.2 on the previous base. `third_party/victory` builds on [Vice-Tay](https://github.com/TaynanGT/Vice-Tay), which keeps the title's buffers on the card, prepares the next frame while the card works and spreads the work over several threads; on 16 or more logical CPUs the launcher gives those threads more room (19.6 -> 23.8 fps median). Heavy scenes (7,000-11,000 draws) still fall to 10-18 fps. Earlier measurements: [docs/progress.md](docs/progress.md).
> - 🖥️ **Full HD, not 4K, to gain fps.** The launcher now plays at 1920x1080 by default: the game renders at 1280x720 inside instead of the 2560x1440 of the 4K a Series X wrote in its settings. With `anyxboxx run`, add `--override VICTORY_OUTPUT=1920x1080`.
> - 🎮 **Playable with mouse and keyboard**: click the game's picture to move the camera with the mouse (F9 gives it back), right button aims, left button fires. The overlay (F10) shows fps, CPU per core, GPU, RAM, VRAM and how busy each of VIctory's worker pools is.
> - ⛔ **Known limits**: ray tracing is off, so indoor and shadowed areas are dark; some graphics are missing; the game caps itself at 30 fps; it uses 20–25 GB of RAM, so 32 GB is recommended.

AnyXboxX runs Xbox GDK executables (PE32+ x86-64) on Windows. It is specialised on one build, **Grand Theft Auto VI for Xbox Series, version 1.0.2.1**, and only translates what that game needs: the console's system functions, the RDNA 2 shaders and the Direct3D 12.x calls. The graphics come from [VIctory](https://github.com/Marvisak/VIctory) (Marvisak, GPL-3.0 or later), copied into `third_party/victory` with the pull requests of its contributors, credited in [THIRD_PARTY.md](THIRD_PARTY.md).

This repository contains no game files, keys or executables, and does not explain how to get them. Live status page, with the disk space to plan for: **[loopersalty.github.io/AnyXboxX](https://loopersalty.github.io/AnyXboxX/)**.

## Requirements

- Windows 10 or 11, 64-bit, with an AVX2 processor.
- A Direct3D 12 graphics card with shader model 6.6 and mesh shaders: NVIDIA RTX 20 series or newer, or AMD RDNA 2 or newer. **The project is focused on NVIDIA**: every measurement and the launcher's tuned settings are for NVIDIA cards (RTX 4070 SUPER). AMD cards work too, but less well (fewer fps, more graphics issues), and get less attention. A 12 GB card runs short of texture memory; the launcher has a preset for 8 and 12 GB cards.
- 32 GB of RAM recommended, and more than 30 GB free on the system drive (the shader cache grows as you play).
- Your own copy of the Xbox Series 1.0.2.1 build: a folder that contains `game_scarlett_master.exe`. It is not provided.
- [Rust](https://rustup.rs/) and Visual Studio 2022 or 2026 Build Tools with the "Desktop development with C++" workload. VIctory is built from source the first time you play.

## Run the game

### With the graphical launcher (recommended)

1. Clone the repository: `git clone https://github.com/LooperSalty/AnyXboxX.git`. VIctory is already in `third_party/victory`.
2. Get `anyxboxx-launcher.exe` from the [launcher-v0.2.0 release](https://github.com/LooperSalty/AnyXboxX/releases/tag/launcher-v0.2.0) and put it in the cloned folder, or build it with `cargo build --release -p anyxboxx-launcher`.
3. Double-click the launcher. On the **Game folder** page, choose the folder that holds `game_scarlett_master.exe`, then press **PLAY**. The first start builds VIctory and warms the shader cache; judge the speed from the second start.

The full guide, with the options, the files written and troubleshooting, is in [docs/launcher.md](docs/launcher.md) (French: [docs/fr/lanceur.md](docs/fr/lanceur.md)).

### From the command line

```powershell
cargo build --release -p anyxboxx
.\target\release\anyxboxx.exe run --game-dir <game folder>
```

`anyxboxx run` starts the game with the same measured settings as the launcher. Full guide: [docs/running-gta6.md](docs/running-gta6.md) (French: [docs/fr/lancer-gta6.md](docs/fr/lancer-gta6.md)).

## Translation progress

![Progress map: one cell per function, green if translated](docs/images/carte-avancee.svg)

Regenerate: `scarlett.py --carte <package folder> docs/images/carte-avancee.svg`.

![Xbox Series X translation progress](docs/images/traduction.svg)

- **CPU instructions: 100%.** The Series X uses an AMD Zen 2 in x86-64, the same instruction set as a PC: the game's code runs as is.
- **GPU instructions: 91%.** 1,364 of the 1,490 RDNA 2 encodings run in the reference interpreter, and the RDNA 2 → DXIL recompiler already draws a first vertex and pixel shader with D3D12.
- **System functions: 66%** of the 1,337 functions imported by the DLLs of the test package are resolved.
- **GDK runtime: 210 of the 396 functions** are written in AnyXboxX, in Rust and C++: the task queue, the offline user, the Xbox Series X identity, saves, the package, the offline Store and invitations. The other interfaces are served by the Windows GDK runtime when it is installed.

The chart is regenerated by `scarlett.py --traduction <package folder> docs/images/traduction.svg`.

## Project progress

| Step | Status |
|---|---|
| Rust launcher `anyxboxx-launcher` and `anyxboxx run`: check the machine, build VIctory, warm the shader cache, start the game | ✅ done |
| The Xbox Series 1.0.2.1 build runs through VIctory: boot, cutscenes, open world (about 24 fps median in the world in Full HD, 30 fps cap reached, about 20 in 4K, Series X mode, RTX 4070 SUPER) | 🟡 in progress |
| Unified memory emulation: copies and read-backs between the console's memory and the PC's (main cost of the CPU) | 🟡 in progress |
| Ray-traced lighting (the BVH structure builder is off) | ⏳ to do |
| NVIDIA Smooth Motion switch in the launcher, presets for 8 and 12 GB cards | ✅ done |
| Full HD by default, worker pools sized by the CPU's core count, mouse camera, overlay with per-core CPU and worker pools | ✅ done |
| Runtime DLLs in Rust: `xmem`, `kernelx`, `d3d12_xs`, `anyxboxx_tls`, `XCRDAPI`; `xgameruntime` ported and checked against the C++ DLL, which is still the one shipped | 🟡 in progress |
| GDK runtime: 210 of 396 functions written in AnyXboxX; the others delegated to the Windows GDK runtime, named by the trace | 🟡 in progress |
| `XTaskQueue`, `XAsync`, `XThread`, `XUser`, `XSystem`, `XGameSave`, `XPackage`, `XStore` written by AnyXboxX | ✅ done |
| D3D12.X: `D3D12XboxCreateDevice`, console `ID3D12*` interfaces, served by TomBox's Rust `d3d12_xs.dll` (MIT) | 🟡 in progress |
| RDNA 2 disassembler: 1,490 mnemonics, checked against 38,999 public LLVM encodings | 🟡 in progress |
| RDNA 2 disassembler operands identical to LLVM's text (scalar, VOP1/VOPC, DS, EXP) | ✅ done |
| RDNA 2 → DXIL shader recompiler: vertex and pixel shaders drawn with D3D12, textures with `image_sample`, identical to the reference interpreter on the GPU and on WARP | 🟡 in progress |
| Wave64 compute programs on GPUs with 32-lane waves | 🟡 in progress |
| Shader cache: translated shaders compiled to DXIL once, then reused from disk | 🟡 in progress |
| Console DXIL → PC DXIL retarget (Xbox2Windows, MIT) | ✅ done |
| Console textures in Rust: BC7 and GFX10 `SW_4K_S` tiling, ported from Xbox2Windows (MIT) | 🟡 in progress |
| Console XMA2 audio decoded to WAV by TomBox's Rust port of FFmpeg's decoder (LGPL) | 🟡 in progress |
| x64 SEH exceptions (`.pdata`), dynamic loading of a DLL at run time (`LoadLibrary` + `DllMain`) | ⏳ to do |
| D3D12.X graphics to Direct3D 12 / Vulkan | ⏳ to do |
| 1900+ automated tests | ✅ done |
| Pull request checks: lint and the English and French READMEs kept in sync (rows, images, test count) | ✅ done |

## Research and history

- [docs/progress.md](docs/progress.md): measurements of the game, and the earlier results on the console DLLs of the test package.
- [docs/test-game.md](docs/test-game.md): the free test games (SimpleTriangle, SDL3's testgdk) of the first prototype, and the Python tools that still generate the progress charts.
- [docs/day-one.md](docs/day-one.md): the first-launch report for a console executable (`jour_j.py`, in French: [docs/fr/jour-j.md](docs/fr/jour-j.md)).
- [Xbox Series X hardware architecture](docs/research/xbox-series-x-architecture.md), [existing emulators and compatibility layers](docs/research/state-of-the-art.md), [TomBox](docs/research/tombox.md), [Xbox2Windows](docs/research/xbox2windows.md).
- [Rust port status](docs/rust-full-status.md).

## Tests

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt -r requirements-dev.txt
.\.venv\Scripts\python.exe -m pytest
```

The tests that need the C++ runtime are skipped when it is not built. GPU tests need `dxc` and the GPU runner; without a GPU they use WARP.

## Contributing

The `main` branch is protected: every contribution goes through a reviewed pull request, and the tests must pass. Bugs and functions to emulate are reported in the issues. The full rules are in [CONTRIBUTING.md](CONTRIBUTING.md).

Discussion, help and test reports: [Discord](https://discord.gg/TcX42vRyPU)

## License

AnyXboxX is distributed under the [GNU GPL 3.0 or later](LICENSE). Third-party parts keep their own license: the `Task` module of libHttpClient (MIT, `runtime/third_party/libhttpclient-task`) and the RDNA 2 tables generated from LLVM's public encoding tests (Apache 2.0 with LLVM exception, `PrototypeScarlett/emu/rdna2_ops.py` and `rdna2_shapes.py`).

Third-party components and credits: [THIRD_PARTY.md](THIRD_PARTY.md).
