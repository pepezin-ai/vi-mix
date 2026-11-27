# vi-mix

A tested mix for running an **Xbox Series title natively on a Windows PC**, from files the user supplies: [LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX) on its Vice-Tay VIctory base, plus the launch settings that measured best on a **12 GB NVIDIA card and an 8-core / 16-thread CPU**.

It is the successor of [pepezin-ai/victory-mix](https://github.com/pepezin-ai/victory-mix) (v5, fxceless base), which stays up as it was. Each time a newer build beats this one on our machine, this repository moves to it.

> **v9: about 14 to 15 fps on average** (frames the game really draws) in the opening scene (cell and balcony) on the machine below, against about 11 for v8. Frame generation through NVIDIA Streamline is in the code but **off by default**. **Not fully playable yet:** ray tracing is off, so interiors and shade are darker than intended.
>
> Each figure is the mean of 2 to 6 runs of 120 s, run-to-run noise about +-1 fps, one machine. Treat differences under 1 fps as noise.

## Legal notice

- **This repository contains no game files, code or assets**, no keys, and no code, headers or binaries from the console SDK. You need your own legally obtained copy of the title.
- **You need your own copy of the title and, for frame generation only, the NVIDIA Streamline SDK DLLs, which you must get from NVIDIA yourself.** Neither is included here (no `sl.*` or `nvngx_*` DLLs, no FidelityFX DLLs).
- It works only on **unpacked game files you already own**. It does not download, decrypt or unpack game packages (XVC / MSIXVC) and contains nothing for circumventing DRM or any other protection.
- The game folder is only read. Saves, caches and logs go to `%LOCALAPPDATA%\vi-mix`.
- The Windows components it relies on (D3D12, DXGI, XAudio2, DXC) come from your own system or the public Windows SDK and are not distributed here.
- It is **not affiliated with or endorsed by** the title's developer or publisher, or by Microsoft. All trademarks belong to their owners and are not used in this repository.
- The goal is interoperability and preservation research on hardware you own. Do not use it to obtain, share or run copies you do not own, and do not open issues asking for game files or where to get them: such issues and links are removed.
- If you hold rights in something here and believe it should not be, please open an issue, and it will be reviewed and removed promptly.

The code is GPL-3.0-or-later, the license of AnyXboxX and VIctory it is built on. Third-party parts keep their own licenses (see `THIRD_PARTY.md`).

## What v9 is

v9 is AnyXboxX `main` of 2026-11-27 (`e7118957`) plus a stack of pull requests still open upstream, our port of DLSS frame generation (with the title's FidelityFX inputs), and switches tested one at a time. It replaces v8, which stays in the history of this branch.

| Change | Why |
|---|---|
| AnyXboxX `main` moved from `a445308` to `e7118957` | Most of the v8 stack is in `main` now |
| Open pull requests stacked on top (list in Credits) | Tested together on one machine |
| DLSS frame generation through NVIDIA Streamline (`VICTORY_FRAME_GEN`, off by default) | Port of trustno1-coder/VIctory-NVIDIA. The title's own FidelityFX dispatch is only watched for depth, motion vectors and jitter, never replaced |
| Real fps and frame-generation fps in the window caption and the overlay | So the numbers are the frames the game draws, not what Smooth Motion or frame generation shows |
| `VICTORY_SECTION_MB`, `VICTORY_GPU_BACK_MAX_KB`, `VICTORY_WINDOWS_BAR_MB` measured | See the tables below |
| `play-vi-mix.cmd` / `.ps1`: new `-Tuned` and `-Override KEY=VALUE` | `-Tuned` adds the two memory switches that helped; `-Override` passes any `VICTORY_*` setting |
| Same two v8 switches on every start (`VICTORY_PIPELINE_HISTORY=0`, `VICTORY_WIDE_COPY=1`) | Banding with the history on ([TaynanGT/Vice-Tay#44](https://github.com/TaynanGT/Vice-Tay/issues/44)); wide copy of big title copies |

Saves, caches and logs stay in `%LOCALAPPDATA%\vi-mix`. AnyXboxX's own README is kept in [`docs/README-AnyXboxX.md`](docs/README-AnyXboxX.md).

## Switches

All are `VICTORY_*` environment settings; pass them with `play-vi-mix.cmd -Override KEY=VALUE` (or `anyxboxx run --override KEY=VALUE`).

| Setting | What it does | Default |
|---|---|---|
| `VICTORY_SECTION_MB=2` | Backs the title's physical memory with sections of at most 2 MiB instead of one huge section per request, so Windows' large-page worker stops holding up the title's map and unmap calls | off |
| `VICTORY_WINDOWS_BAR_MB=2048` | Keeps up to 2 GiB of windows the CPU only writes in card memory, filled through the bus (resizable BAR), with no copy to send | 0 (off) |
| `VICTORY_GPU_BACK_MAX_KB=n` | Ranges written by the GPU that are larger than n KiB are not copied back into title memory and nothing waits for them; 0 copies none | unset (every range, as before) |
| `VICTORY_FRAME_GEN=2\|3\|4` | DLSS frame generation, frames shown per rendered frame. Needs `VICTORY_STREAMLINE_DIR` (your own NVIDIA Streamline DLLs) and `VICTORY_UPSCALER=7` (FidelityFX) so it gets motion vectors | off |
| `VICTORY_HELPER_PRIORITY=1` | Raises the priority of the submitting thread's helpers | off |

> **Warning on `VICTORY_GPU_BACK_MAX_KB`:** ranges written by the GPU above n KiB do **not** come back to RAM. If the title reads such a range on the CPU, it may see stale data, which can change what the game shows or does. It is off by default in the code and not part of `-Tuned`. Test it on your own save before trusting it.

Tried with **no measurable effect**: `VICTORY_HEAP_PASS_DIRTY`, `VICTORY_COPY_COALESCE`, `VICTORY_UPLOAD_AHEAD`, `VICTORY_CARD_WINDOWS_MB`. `VICTORY_INPLACE` made it **worse**.

## How to use

1. Install the Rust MSVC toolchain and the Visual Studio C++ build tools (see `docs/README-AnyXboxX.md`).
2. Clone with the submodule: `git clone --recurse-submodules https://github.com/pepezin-ai/vi-mix`
3. Double-click `play-vi-mix.cmd`. The first start builds VIctory and asks for the folder of your own unpacked copy.
4. The first start of a new build translates shaders for 10 to 20 minutes before the world appears. Later starts reach the world in under a minute.

Options: `play-vi-mix.cmd -NativeOutput` (keep 4K), `-Warm` (translate every shader before the game), `-Stock` (leave out the vi-mix switches), `-Tuned` (add `VICTORY_WINDOWS_BAR_MB=2048` and `VICTORY_SECTION_MB=2`), `-Override KEY=VALUE`.

Frame generation needs the NVIDIA Streamline SDK (the signed `sl.*` DLLs), obtained by you from NVIDIA and not included here: set `VICTORY_STREAMLINE_DIR` to its folder.

The `-SeriesS` option of v7 is gone: AnyXboxX removed the script it used, and `anyxboxx run` has no console switch. Series S was slower here anyway.

Tip for NVIDIA RTX 40/50: turn on **Smooth Motion** for `victory.exe` in the NVIDIA App. The numbers below are the frames the game really draws; Smooth Motion doubles what you see on screen. The game uses 20 to 22 GB of RAM: close browsers and other large programs before playing on 32 GB.

## Measured on

Windows 11, AMD Ryzen 7 5700X (8C/16T), 32 GB DDR4-3200, NVIDIA RTX 5070 12 GB. Full HD output, FSR4 720p to 1080p, opening scene (cell and balcony), 120 s, 2 to 6 runs per row, noise about +-1 fps. Mean fps of frames the game really draws.

| Build / settings | fps |
|---|---|
| v8 (no tweaks) | ~11 |
| v8 + frame generation / FidelityFX port + tuned env (D) | 8.5 |
| P2: PRs + `VICTORY_WINDOWS_BAR_MB=2048` + `VICTORY_SECTION_MB=2` | 12.3 (6 runs, 10.8 to 13.8) |
| P2 + `VICTORY_GPU_BACK_MAX_KB=64` | 14.8 (4 runs: 14.4, 15.1, 15.2, 14.4) |
| P2 + `VICTORY_GPU_BACK_MAX_KB=16` | 15.2 (2 runs: 16.1, 14.3) |
| New `main` (`e7118957`) + the stack (v9), `VICTORY_GPU_BACK_MAX_KB=64` | 14.3 (4 runs: 14.8, 13.8, 13.9, 14.6) |
| v9 with `VICTORY_GPU_BACK_MAX_KB=16` | 14.7 (3 runs: 14.4, 14.6, 15.0) |

The `VICTORY_GPU_BACK_MAX_KB` rows carry the warning above.

Earlier, v8 against v7 (opening scene of another save): v8 11.1 mean / 12.8 median against 10.3 / 12.0 for v7, with about 28% more objects drawn a frame and no colour banding.

## Credits

All the hard work is theirs. Sources stacked in v9 (every commit keeps its original author):

| Source | Author | What it does |
|---|---|---|
| [LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX) `main` | @LooperSalty and contributors | The base of this mix, its launcher, settings and measurements |
| [AnyXboxX #456](https://github.com/LooperSalty/AnyXboxX/pull/456) | @LooperSalty (ports of mauroalvare's #435 and #448) | Shader translator fixes ported onto the Vice-Tay base |
| [AnyXboxX #457](https://github.com/LooperSalty/AnyXboxX/pull/457) | @LooperSalty (ports of mauroalvare's #433 and #447) | GPU pipeline fixes ported onto the Vice-Tay base |
| [AnyXboxX #458](https://github.com/LooperSalty/AnyXboxX/pull/458) | @Lewandox | Zero fallback for missing root views (logged once), drop draws missing stride tables |
| Follow-up of [AnyXboxX #465](https://github.com/LooperSalty/AnyXboxX/pull/465) | @KainanP | Drops the launcher's texture size cap that the Vice-Tay base does not read |
| [AnyXboxX #510](https://github.com/LooperSalty/AnyXboxX/pull/510) | from [TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay) `5c803d236` (Ash) | `VICTORY_HELPER_PRIORITY` |
| [AnyXboxX #511](https://github.com/LooperSalty/AnyXboxX/pull/511) | from JMAN730/victory `b975c652` (fever308) | Large pre-write refresh spans shared across wide workers |
| [AnyXboxX #520](https://github.com/LooperSalty/AnyXboxX/pull/520) | from Skyfrye/AnyXboxX (Skyfrye) | Idle scratch kept through memory pressure, swept scratch released off the submission thread, replacement command lists left open until reset |
| TaynanGT/Vice-Tay PRs #247 and #322 | Vice-Tay contributors | `VICTORY_REAPER`; card steps planned once |
| `VICTORY_SECTION_MB` (AnyXboxX commit by @KainanP) | CuteBooty (fxceless/victory-fork#1274); idea and first patch by RyanCase94 (#1089) | Physical pages from sections of at most 2 MiB |
| `VICTORY_GPU_BACK_MAX_KB` | **scarlet-fps** (commit `24e9826`) | Skip copying big GPU-written ranges back to RAM |
| DLSS frame generation through Streamline | **[trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA)** | `streamline.rs` and `fg.rs` ported (without its DLSS super resolution) |

Already in AnyXboxX `main`, measured earlier: [#444](https://github.com/LooperSalty/AnyXboxX/pull/444) and [#465](https://github.com/LooperSalty/AnyXboxX/pull/465) (@KainanP), [#446](https://github.com/LooperSalty/AnyXboxX/pull/446) and [#451](https://github.com/LooperSalty/AnyXboxX/pull/451) (@Lewandox), [#466](https://github.com/LooperSalty/AnyXboxX/pull/466) (@KainanP).

Also:

- **[TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)**: the VIctory base AnyXboxX runs on, and the banding report behind `VICTORY_PIPELINE_HISTORY=0`.
- **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)**, by Marvisak and contributors: the loader, runtime services, Direct3D 12 back end and shader recompiler everything is built on (GPL-3.0-or-later).
- The projects AnyXboxX credits in [`THIRD_PARTY.md`](THIRD_PARTY.md), including TomBox (`third_party/tombox`).
- Our part (**pepezin-ai**): testing builds side by side on one machine, the launcher changes, the frame-generation and switch ports, and the measurements.
