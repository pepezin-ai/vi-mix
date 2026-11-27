# Graphical launcher: install and use

French version: [fr/lanceur.md](fr/lanceur.md). In `main`. Code: `crates/anyxboxx-launcher`.

`anyxboxx-launcher.exe` is a window that checks your machine, verifies your game folder, builds VIctory, warms the shader cache and starts the game with the measured settings of `anyxboxx run` (the same set as `main`'s `tools\lancer_victory.ps1`). It does not contain or download any game file, and it never writes into the game folder.

## What you need

- Windows 10 or 11, x64, a CPU with AVX2, a Direct3D 12 GPU with shader model 6.6 and mesh shaders (NVIDIA RTX 20 series or newer, AMD RDNA 2 or newer), 32 GB of RAM recommended. Full list: [running-gta6.md](running-gta6.md#requirements).
- Your own copy of the Xbox Series 1.0.2.1 build: a folder that contains `game_scarlett_master.exe`. Not provided.
- [Git](https://git-scm.com/), [Rust through rustup](https://rustup.rs/), Visual Studio 2022 or 2026 Build Tools with the "Desktop development with C++" workload. The launcher builds VIctory from source with cargo the first time you press PLAY, so Rust and MSVC are needed even with the prebuilt launcher.
- Free disk space: the VIctory build needs a few GB, and the shader cache grows as you play. Keep more than 30 GB free on the system drive (with 10 GB left, loading never ends).

## Install

### 1. Get the code

```powershell
git clone https://github.com/LooperSalty/AnyXboxX.git
cd AnyXboxX
```

VIctory is included in `third_party/victory`: there is nothing else to download.

### 2. Get the launcher

Either option works:

- **Prebuilt**: download `anyxboxx-launcher.exe` from the [launcher releases](https://github.com/LooperSalty/AnyXboxX/releases?q=launcher&expanded=true), check its SHA-256 against `anyxboxx-launcher.exe.sha256` (`Get-FileHash .\anyxboxx-launcher.exe`), and put it **in the `AnyXboxX` folder** you cloned (or any folder inside it).
- **From source**: `cargo build --release -p anyxboxx-launcher`, then use `target\release\anyxboxx-launcher.exe`.

The launcher finds the repository by walking up from its own folder until it meets `third_party\victory\Cargo.toml`. If you keep the executable somewhere else, set `ANYXBOXX_REPO` to the clone folder (`setx ANYXBOXX_REPO C:\path\to\AnyXboxX`, then open a new window). Without either, PLAY stops with "repository not found".

### 3. First start

1. Double-click `anyxboxx-launcher.exe`.
2. **Setup** page: every line must be green or explained. Rust, the Direct3D 12 runtime, the VC++ redistributable and the GPU driver are checked; the Windows SDK `dxc.exe` is only needed for the repository's GPU tests. Links go to official pages only; the launcher installs nothing itself.
3. **Game folder** page: pick the folder that holds `game_scarlett_master.exe` (folder picker or automatic detection). The quick check reads only that folder's own files.
4. **Home**: press **PLAY**. The first time, a full-window screen shows the stages: building VIctory (several minutes, once), preloading shaders, then starting the game. Cancel stops the running step.
5. The game starts in its own window and the launcher closes, unless "Keep the launcher open" is on.

The first run after a new build or a new setting recompiles hundreds of pipelines and runs at well under 1 fps for a while: judge the speed on the second run, once the cache is filled.

## Options

- Resolution and window mode (windowed or borderless).
- Keyboard layout: automatic, QWERTY, AZERTY or QWERTZ (`VICTORY_KEYBOARD`).
- Fast (default, measured set), Quality (better lighting and textures, lower frame rate), 12 GB card or 8 GB card. The Fast profile's reserves on the card (1536 MB of draw-data windows and of buffer copies) were sized on 12 to 16 GB cards; on an RTX 5060 8 GB it stood at 8,010 MB in use against a 6,823 MB budget, Windows moved the rest into the PC's memory and the texture queue was refused. The two card presets cut those reserves (1024 MB, or 512 MB with a 256 MB list pool) and cap the title's textures with `VICTORY_TEXTURE_MAX_SIZE` (2048, or 1024 texels): the levels of a texture over the cap get no memory on the card and its views start at the first level within it, so every texture fits, slightly less sharp up close. The 12 GB preset is not measured on a 12 GB card yet.
- Frame rate counter, automatic A press through the start screens, frame time CSV in the cache folder.
- Advanced panel: overrides of single `VICTORY_*` variables; they always win over the profile.
- NVIDIA Smooth Motion (RTX 40 and 50 series): the driver's own frame generation, which inserts a generated picture between two of the game's. Twice the pictures shown; responsiveness stays that of the game's own frames. It is not a VIctory variable: at each PLAY the launcher writes it into the NVIDIA driver profile that holds `victory.exe` (made if none does), through NVAPI with the setting ids NVIDIA Profile Inspector uses (`0xB0D384C0` enable, `0xB0CC0875` allowed APIs: Direct3D 12). No other driver setting is touched, and no administrator rights are needed.

Settings are profiles per host and GPU vendor (`crates/anyxboxx-launcher/profiles.toml`). Only the Windows + NVIDIA set is measured; the others are VIctory's defaults.

## Files it writes

| Path | Content |
| --- | --- |
| `%LOCALAPPDATA%\AnyXboxX\launcher\settings.toml` | Game folder and options |
| `%LOCALAPPDATA%\AnyXboxX\logs\game-*.log` | The log of each game run (VIctory's output) |
| `%LOCALAPPDATA%\AnyXboxX\launcher\splash\` | Optional: your own PNG or JPEG pictures, shown as a slideshow. Nothing is shipped there. |
| `%LOCALAPPDATA%\victory\data\` | VIctory's saves, data and `crashes.log` |
| `third_party\victory\target\release\` | The VIctory build |
| NVIDIA driver settings | With Smooth Motion switched on or off: the Smooth Motion settings of the profile holding `victory.exe` (`AnyXboxX (victory.exe)` when the launcher makes it) |

## Troubleshooting

- **"repository not found"**: the executable is outside the clone and `ANYXBOXX_REPO` is not set (step 2).
- **The VIctory build fails**: run `cargo build --release -p victory --manifest-path third_party\victory\Cargo.toml` in a terminal to see cargo's message; usually the MSVC workload is missing.
- **A game is already running**: the launcher allows one game at a time, because two would share the save folder.
- **The window closed by itself**: the log says `window closed: suspending the title` when you closed it; a crash is written to `crashes.log` under `%LOCALAPPDATA%\victory\data\`.
- **Windows error 1455 in `crashes.log`**: the paging file is too small; close other applications or enlarge it.
- **The launcher never opens its window** (the process stays in Task Manager): its window is drawn with Direct3D 12 on Windows. To try another graphics API, start it from a Command Prompt after `set WGPU_BACKEND=vulkan` (or `dx12`, the default); see issue #483.

Under Proton or Wine the launcher cannot build VIctory (no cargo): use `anyxboxx run` as described in [running-gta6.md](running-gta6.md).
