# vi-mix

A tested mix for running an **Xbox Series title natively on a Windows PC**, from files the user supplies: [LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX) on its Vice-Tay VIctory base, plus the launch settings that measured best on a **12 GB NVIDIA card and an 8-core / 16-thread CPU**.

It is the successor of [pepezin-ai/victory-mix](https://github.com/pepezin-ai/victory-mix) (v5, fxceless base), which stays up as it was. Each time a newer build beats this one on our machine, this repository moves to it.

> **v7: about 12 fps** (frames the game really draws, never below 8 in our run) in the open world on the machine below, with Smooth Motion showing about twice that. **Not fully playable yet:** ray tracing is off, so interiors and shade are darker than intended.

## Legal notice

- **This repository contains no game files, code or assets**, no keys, and no code, headers or binaries from the console SDK. You need your own legally obtained copy of the title.
- It works only on **unpacked game files you already own**. It does not download, decrypt or unpack game packages (XVC / MSIXVC) and contains nothing for circumventing DRM or any other protection.
- The game folder is only read. Saves, caches and logs go to `%LOCALAPPDATA%\vi-mix`.
- The Windows components it relies on (D3D12, DXGI, XAudio2, DXC) come from your own system or the public Windows SDK and are not distributed here.
- It is **not affiliated with or endorsed by** the title's developer or publisher, or by Microsoft. All trademarks belong to their owners and are not used in this repository.
- The goal is interoperability and preservation research on hardware you own. Do not use it to obtain, share or run copies you do not own, and do not open issues asking for game files or where to get them: such issues and links are removed.
- If you hold rights in something here and believe it should not be, please open an issue, and it will be reviewed and removed promptly.

The code is GPL-3.0-or-later, the license of AnyXboxX and VIctory it is built on. Third-party parts keep their own licenses (see `THIRD_PARTY.md`).

## What is different from AnyXboxX

| Change | Why |
|---|---|
| **8 open AnyXboxX pull requests**, merged before upstream does (list below) | Picture fixes, memory and speed on the Vice-Tay base: 7.2 → **12.1 fps** in the same scene here |
| `play-vi-mix.cmd` / `play-vi-mix.ps1` | One double-click start with the creator's own settings (`anyxboxx run`, as the 0.2.x launcher): asks for your game folder once, keeps saves and caches in `%LOCALAPPDATA%\vi-mix`, 1920x1080 output |
| **16-thread worker pools** also in the PowerShell launcher (`THIN 12 / UNDER 8 / LAND 6` on 16+ logical CPUs) | `anyxboxx run` has them since upstream #462; `tools/lancer_victory.ps1`, used by `-SeriesS`, did not |
| `-SeriesS` switch | Passes `--console series-s` to VIctory, for comparison (it was slower here) |

Pull requests included, with thanks to their authors (all open upstream when merged here):

| PR | Author | What it does |
|---|---|---|
| [#444](https://github.com/LooperSalty/AnyXboxX/pull/444) | @KainanP | texture reads ask the texture, not process-wide tables under a lock |
| [#446](https://github.com/LooperSalty/AnyXboxX/pull/446) | @Lewandox | retire kept host resources when dynamic descriptor slots are released |
| [#451](https://github.com/LooperSalty/AnyXboxX/pull/451) | @Lewandox | exact upload block spans, zeroing and allocation outside the arena lock |
| [#456](https://github.com/LooperSalty/AnyXboxX/pull/456) | @LooperSalty | shader translator fixes ported onto the Vice-Tay base |
| [#457](https://github.com/LooperSalty/AnyXboxX/pull/457) | @LooperSalty | GPU pipeline fixes ported onto the Vice-Tay base |
| [#458](https://github.com/LooperSalty/AnyXboxX/pull/458) | @Lewandox | zero fallback for missing root views, drop draws missing stride tables |
| [#465](https://github.com/LooperSalty/AnyXboxX/pull/465) | @KainanP | idle textures leave the texture queue's share |
| [#466](https://github.com/LooperSalty/AnyXboxX/pull/466) | @KainanP | every loose window copy counts against the window budget |

Everything else is AnyXboxX `main` as of 2026-11-27 (`b62053f`, launcher 0.2.1). Its own README is kept in [`docs/README-AnyXboxX.md`](docs/README-AnyXboxX.md).

## How to use

1. Install the Rust MSVC toolchain and the Visual Studio C++ build tools (see `docs/README-AnyXboxX.md`).
2. Clone with the submodule: `git clone --recurse-submodules https://github.com/pepezin-ai/vi-mix`
3. Double-click `play-vi-mix.cmd`. The first start builds VIctory and asks for the folder of your own unpacked copy.
4. The first start of a new build translates shaders for 10 to 20 minutes before the world appears. Later starts reach the world in under a minute.

Options: `play-vi-mix.cmd -SeriesS`, `-NativeOutput` (keep 4K), `-Warm` (translate every shader before the game).

Tip for NVIDIA RTX 40/50: turn on **Smooth Motion** for `victory.exe` in the NVIDIA App. The numbers below are the frames the game really draws; Smooth Motion doubles what you see on screen.

## Measured on

Windows 11, AMD Ryzen 7 5700X (8C/16T), 32 GB DDR4, NVIDIA RTX 5070 12 GB. Same save in the open world, warm shader cache, 90 s measured after shaders reached 0, frame lines with more than 3,000 draws:

Same opening scene of that save (letterboxed, about 7,000 draws a frame):

| Build | fps (mean / median) |
|---|---|
| **vi-mix v7** (AnyXboxX b62053f, creator's settings, Full HD, + the 8 PRs) | **12.1 / 12.9** (min 8.0) |
| AnyXboxX b62053f alone, creator's settings, Full HD | 7.2 / 7.4 |

Earlier runs on the same save, different moments, for reference:

| Build | fps (mean / median) |
|---|---|
| vi-mix v6 (AnyXboxX 2225a8e, Full HD, 16-thread pools, Series X) | 10.9 / 11.7 |
| same, without the 16-thread pools | 9.1 / 9.6 |
| same, Series S | 7.1 / 7.5 |
| same, more card reserves (windows 2 GB, buffers 2.5 GB) | 6.0 / 6.2 |
| rorodev6/victory-nvidia-megafork (Series S, half resolution) | 7.7 |

## Credits

All the hard work is theirs:

- **[LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX)** and its contributors: the base of this mix, its launcher, settings and measurements.
- **[TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)**: the VIctory base AnyXboxX now runs on, and its 16-core pool sizes.
- **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)**, by Marvisak and contributors: the loader, runtime services, Direct3D 12 back end and shader recompiler everything is built on (GPL-3.0-or-later).
- The projects AnyXboxX credits in [`THIRD_PARTY.md`](THIRD_PARTY.md), including TomBox (`third_party/tombox`).
- Our part (**pepezin-ai**): testing builds side by side on one machine, the launcher changes above and the measurements.

Every upstream commit keeps its original author.
