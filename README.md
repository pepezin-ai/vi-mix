# vi-mix

A tested mix for running an **Xbox Series title natively on a Windows PC**, from files the user supplies: [LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX) on its Vice-Tay VIctory base, plus the launch settings that measured best on a **12 GB NVIDIA card and an 8-core / 16-thread CPU**.

It is the successor of [pepezin-ai/victory-mix](https://github.com/pepezin-ai/victory-mix) (v5, fxceless base), which stays up as it was. Each time a newer build beats this one on our machine, this repository moves to it.

> **v8: about 11 fps on average, 12.8 median** (frames the game really draws) in the opening scene on the machine below, while drawing about 28% more objects a frame than v7 did in the same run, and **without the whole-picture colour banding** v7 showed on some launches. Smooth Motion shows about twice that. **Not fully playable yet:** ray tracing is off, so interiors and shade are darker than intended.

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
| **3 open AnyXboxX pull requests** merged before upstream does (#456, #457, #458, list below) | The other 5 of the 8 that made v7 are in AnyXboxX `main` now |
| `VICTORY_PIPELINE_HISTORY=0` on every start | Whole-picture colour banding (sky, skin, water in flat steps) came back on some launches with the pipeline scheduling history on ([TaynanGT/Vice-Tay#44](https://github.com/TaynanGT/Vice-Tay/issues/44)). Four launches in a row without it were clean here |
| `VICTORY_WIDE_COPY=1` on every start | AnyXboxX #500 (off by default): big title copies are cut into pieces on the kept threads |
| `play-vi-mix.cmd` / `play-vi-mix.ps1` | One double-click start with the creator's own settings (`anyxboxx run`, as the graphical launcher) plus the two switches above: asks for your game folder once, keeps saves and caches in `%LOCALAPPDATA%\vi-mix`, 1920x1080 output |

Pull requests included, with thanks to their authors (open upstream when merged here):

| PR | Author | What it does |
|---|---|---|
| [#456](https://github.com/LooperSalty/AnyXboxX/pull/456) | @LooperSalty | shader translator fixes ported onto the Vice-Tay base |
| [#457](https://github.com/LooperSalty/AnyXboxX/pull/457) | @LooperSalty | GPU pipeline fixes ported onto the Vice-Tay base |
| [#458](https://github.com/LooperSalty/AnyXboxX/pull/458) | @Lewandox | zero fallback for missing root views (logged once), drop draws missing stride tables |

Already in AnyXboxX `main`, measured in v7: [#444](https://github.com/LooperSalty/AnyXboxX/pull/444) and [#465](https://github.com/LooperSalty/AnyXboxX/pull/465) (@KainanP), [#446](https://github.com/LooperSalty/AnyXboxX/pull/446) and [#451](https://github.com/LooperSalty/AnyXboxX/pull/451) (@Lewandox), [#466](https://github.com/LooperSalty/AnyXboxX/pull/466) (@KainanP).

Everything else is AnyXboxX `main` as of 2026-11-27 (`a445308`). Its own README is kept in [`docs/README-AnyXboxX.md`](docs/README-AnyXboxX.md).

## How to use

1. Install the Rust MSVC toolchain and the Visual Studio C++ build tools (see `docs/README-AnyXboxX.md`).
2. Clone with the submodule: `git clone --recurse-submodules https://github.com/pepezin-ai/vi-mix`
3. Double-click `play-vi-mix.cmd`. The first start builds VIctory and asks for the folder of your own unpacked copy.
4. The first start of a new build translates shaders for 10 to 20 minutes before the world appears. Later starts reach the world in under a minute.

Options: `play-vi-mix.cmd -NativeOutput` (keep 4K), `-Warm` (translate every shader before the game), `-Stock` (leave out the two vi-mix switches).

The `-SeriesS` option of v7 is gone: AnyXboxX removed the script it used, and `anyxboxx run` has no console switch. Series S was slower here anyway.

Tip for NVIDIA RTX 40/50: turn on **Smooth Motion** for `victory.exe` in the NVIDIA App. The numbers below are the frames the game really draws; Smooth Motion doubles what you see on screen. The game uses 20 to 22 GB of RAM: close browsers and other large programs before playing on 32 GB.

## Measured on

Windows 11, AMD Ryzen 7 5700X (8C/16T), 32 GB DDR4, NVIDIA RTX 5070 12 GB. Same save, warm shader cache, 90 s measured from the moment shaders reached 0 and the world was drawn, no input.

v8 against v7, one after the other in the same session, same opening scene of that save (letterboxed):

| Build | fps (mean / median) | lowest | draws a frame | banding |
|---|---|---|---|---|
| **vi-mix v8** (AnyXboxX a445308 + #456 #457 #458, both switches) | **11.1 / 12.8** | 4.2 | about 7,600 | none |
| vi-mix v7 (AnyXboxX b62053f + the 8 PRs) | 10.3 / 12.0 | 2.5 | about 5,900 | yes, on this launch |

One run of each, with a browser open and RAM almost full (about 31 of 32 GB) in both: read the gap in frame rate as small, and the gap in objects drawn and in the picture as the clearer part.

Other runs of the same day, for reference (same save, other moments):

| Build | fps (mean / median) |
|---|---|
| AnyXboxX a445308 alone, history off, no wide copy, standing on the deck | 8.1 / 9.7 |
| v8 without `VICTORY_WIDE_COPY` (main 14f488b), walking | 9.0 / 8.7 |
| v7, first measurement on 2026-11-26 (same opening scene) | 12.1 / 12.9 |
| AnyXboxX b62053f alone, creator's settings (same opening scene) | 7.2 / 7.4 |

## Credits

All the hard work is theirs:

- **[LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX)** and its contributors: the base of this mix, its launcher, settings and measurements.
- **[TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)**: the VIctory base AnyXboxX now runs on, and the banding report the history switch comes from.
- **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)**, by Marvisak and contributors: the loader, runtime services, Direct3D 12 back end and shader recompiler everything is built on (GPL-3.0-or-later).
- The projects AnyXboxX credits in [`THIRD_PARTY.md`](THIRD_PARTY.md), including TomBox (`third_party/tombox`).
- Our part (**pepezin-ai**): testing builds side by side on one machine, the launcher changes above and the measurements.

Every upstream commit keeps its original author.
