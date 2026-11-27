# vi-mix

A tested mix for running an **Xbox Series title natively on a Windows PC**, from files the user supplies: [LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX) on its Vice-Tay VIctory base, plus the launch settings that measured best on a **12 GB NVIDIA card and an 8-core / 16-thread CPU**.

It is the successor of [pepezin-ai/victory-mix](https://github.com/pepezin-ai/victory-mix) (v5, fxceless base), which stays up as it was. Each time a newer build beats this one on our machine, this repository moves to it.

> **Not fully playable yet.** The open world runs with characters, HUD, water and vegetation at about **11 fps** (frames the game really draws) on the machine below. Ray tracing is off, so interiors and shade are darker than intended.

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
| `play-vi-mix.cmd` / `play-vi-mix.ps1` | One double-click start: asks for your game folder once, keeps saves and caches in `%LOCALAPPDATA%\vi-mix`, forces a 1920x1080 output |
| Output forced to **1920x1080** (`VICTORY_OUTPUT`) | The title otherwise renders for a 4K Series X: 7.4 → 9.1 fps here. Side effect: the HUD looks larger |
| **16-thread worker pools** in the PowerShell launcher (`THIN 12 / UNDER 8 / LAND 6` when the PC has 16+ logical CPUs) | `anyxboxx run` got them in upstream #462, `tools/lancer_victory.ps1` did not: 9.1 → **10.9 fps** here |
| `-SeriesS` switch in `tools/lancer_victory.ps1` | Passes `--console series-s` to VIctory, for comparison (it was slower here: 7.1 fps) |

Everything else is AnyXboxX `main` as of 2026-11-27 (`4340aac`). Its own README is kept in [`docs/README-AnyXboxX.md`](docs/README-AnyXboxX.md).

## How to use

1. Install the Rust MSVC toolchain and the Visual Studio C++ build tools (see `docs/README-AnyXboxX.md`).
2. Clone with the submodule: `git clone --recurse-submodules https://github.com/pepezin-ai/vi-mix`
3. Double-click `play-vi-mix.cmd`. The first start builds VIctory and asks for the folder of your own unpacked copy.
4. The first start of a new build translates shaders for 10 to 20 minutes before the world appears. Later starts reach the world in under a minute.

Options: `play-vi-mix.cmd -SeriesS`, `-NativeOutput` (keep 4K), `-Warm` (translate every shader before the game).

Tip for NVIDIA RTX 40/50: turn on **Smooth Motion** for `victory.exe` in the NVIDIA App. The numbers below are the frames the game really draws; Smooth Motion doubles what you see on screen.

## Measured on

Windows 11, AMD Ryzen 7 5700X (8C/16T), 32 GB DDR4, NVIDIA RTX 5070 12 GB. Same save in the open world, warm shader cache, 90 s measured after shaders reached 0, frame lines with more than 3,000 draws:

| Build | fps (mean / median) |
|---|---|
| **vi-mix** (AnyXboxX 2225a8e, Full HD, 16-thread pools, Series X) | **10.9 / 11.7** |
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
