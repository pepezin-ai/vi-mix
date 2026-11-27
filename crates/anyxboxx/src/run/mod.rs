//! `anyxboxx run`: builds VIctory (cargo does nothing when it is current) and launches it on the
//! user's own game folder. Rust replacement of main's `tools/lancer_victory.ps1`: same checks,
//! same build, same environment (`tests/fixtures/lancer_victory_neptune.ps1` is the golden copy),
//! same exit code. On Vice-Tay, warm and precompile share a pipeline-cache warm.

mod settings;
mod steps;

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, ValueEnum};

pub use settings::{ALWAYS, LINUX_AMD, QUALITY, TUNED, logical_cpus, pool_sizes};
pub use steps::{
    Env, GAME_EXE, PRECOMPILED_DLLS, SHADERS, Victory, environment, game_exe, status, warm_archives,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Profile {
    /// Windows + NVIDIA measured tuned set (`TUNED`).
    #[default]
    WindowsNvidia,
    /// Linux (Proton) + AMD measured set (`LINUX_AMD`, including `VKD3D_CONFIG=descriptor_heap`).
    LinuxAmd,
    /// VIctory's own defaults (no profile variables).
    Defaults,
}

/// The console model the title sees, VIctory's `victory run --console` (device type and memory plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Console {
    /// Xbox Series X, VIctory's default: nothing is passed, so the command stays as it was.
    #[default]
    SeriesX,
    /// Xbox Series S: the title asks for half the memory plan and renders at 1280x720.
    SeriesS,
}

impl Console {
    /// The arguments that ask VIctory for this console: none for its default, Series X.
    pub fn victory_args(self) -> &'static [&'static str] {
        match self {
            Console::SeriesX => &[],
            Console::SeriesS => &["--console", "series-s"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum XgMode {
    /// Serve the title from xg_xs.dll.
    Real,
    /// Keep VIctory's layouts and log every difference with the library.
    Compare,
}

impl XgMode {
    pub fn as_str(self) -> &'static str {
        match self {
            XgMode::Real => "real",
            XgMode::Compare => "compare",
        }
    }
}

/// A `KEY=VALUE` pair of `--override`; the key must be a VICTORY_* name.
fn parse_override(text: &str) -> Result<(String, String), String> {
    let (key, value) = text
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got {text:?}"))?;
    if !key.starts_with("VICTORY_") {
        return Err(format!("--override takes VICTORY_* names only: {key:?}"));
    }
    Ok((key.to_string(), value.to_string()))
}

/// Options of `anyxboxx run`; the PowerShell names (`--GameDir`...) are accepted as aliases.
#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// Folder of the game package (kept outside this repository); must hold game_scarlett_master.exe.
    #[arg(long, alias = "GameDir")]
    pub game_dir: PathBuf,
    /// Passed unchanged to VICTORY_SKIP_PIPELINES.
    #[arg(long, alias = "SkipPipelines")]
    pub skip_pipelines: Option<String>,
    /// Do not set the tuned variables (VICTORY_GAME_DIR and VICTORY_DISCORD stay set).
    #[arg(long, alias = "NoTunedSettings")]
    pub no_tuned_settings: bool,
    /// Launch profile whose variables are applied unless `--no-tuned-settings` is set.
    #[arg(long, alias = "Profile", value_enum, default_value_t = Profile::WindowsNvidia)]
    pub profile: Profile,
    /// Picture first: windows and buffers read fresh each frame (about half the frame rate).
    #[arg(long, alias = "Quality")]
    pub quality: bool,
    /// Override any VICTORY_* launch setting with KEY=VALUE. Repeatable. Use KEY= to unset it.
    #[arg(long = "override", alias = "Override", value_parser = parse_override)]
    pub overrides: Vec<(String, String)>,
    /// Taps A through the start screens for 150 s (VICTORY_AUTO_A=150).
    #[arg(long, alias = "AutoA")]
    pub auto_a: bool,
    /// Writes frametime.csv into the cache folder, one row a second (VICTORY_FRAMETIME_DUMP=1).
    #[arg(long, alias = "FrametimeDump")]
    pub frametime_dump: bool,
    /// Build and run the diag profile with the diagnostics feature instead of the release build.
    #[arg(long, alias = "Diag")]
    pub diag: bool,
    /// Turn on device-removed data (VICTORY_D3D12_DRED); needs the diagnostics build, so implies --diag.
    #[arg(long, alias = "Dred")]
    pub dred: bool,
    /// Check for .rpf archives and warm the recorded host pipelines before the game.
    #[arg(long, alias = "Warm")]
    pub warm: bool,
    /// Check the shader archive and required DLLs, then warm the recorded host pipelines.
    #[arg(long, alias = "Precompile")]
    pub precompile: bool,
    /// Your own copy of the GDK's offline PC xg_xs.dll (never in this repository).
    #[arg(long, alias = "XgDll")]
    pub xg_dll: Option<PathBuf>,
    /// With --xg-dll: `real` serves the title from the library, `compare` logs differences.
    #[arg(long, alias = "XgMode", value_enum, default_value = "real")]
    pub xg_mode: XgMode,
    /// Set VICTORY_CARD_REHASH_RUNS=1 (hash adjacent GPU memory ranges in batches).
    #[arg(long, alias = "BatchCardRehash")]
    pub batch_card_rehash: bool,
    /// The console the title sees: `series-x` (default) or `series-s`, the profile VIctory sizes
    /// for 8 GB cards (half the memory plan, 1280x720 rendering).
    #[arg(long, alias = "Console", value_enum, default_value_t = Console::SeriesX)]
    pub console: Console,
    /// Use this victory.exe, built beforehand, and skip the cargo build (for Proton, where there
    /// is no cargo: cross-build VIctory on Linux with `--target x86_64-pc-windows-msvc`).
    /// The repository is then not needed.
    #[arg(long, alias = "VictoryExe")]
    pub victory_exe: Option<PathBuf>,
    /// Repository root (default: ANYXBOXX_REPO, then the folders above the binary and the
    /// current folder that hold third_party/victory/Cargo.toml).
    #[arg(long)]
    pub repo: Option<PathBuf>,
}

impl RunArgs {
    /// The diag profile serves `--diag`, and `--dred` (which needs its diagnostics).
    pub fn uses_diag(&self) -> bool {
        self.diag || self.dred
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("third_party/victory/Cargo.toml is missing: this copy of AnyXboxX is incomplete.")]
    NoVictory,
    #[error("victory.exe not found: {0:?}.")]
    NoVictoryExe(String),
    #[error("repository not found (pass --repo or set ANYXBOXX_REPO).")]
    NoRepo,
    #[error("game_scarlett_master.exe not found in {0:?}.")]
    NoGame(String),
    #[error("cargo build failed (exit code {0}).")]
    BuildFailed(i32),
    #[error("cargo build reported {0} VIctory executables, expected one.")]
    BuildArtifactCount(usize),
    #[error("xg_xs.dll not found: {0:?}.")]
    NoXgDll(String),
    #[error("--precompile needs {0:?}.")]
    PrecompileInput(String),
    #[error("--warm found no .rpf archive in {0:?}.")]
    NoArchive(String),
    #[error("victory warm failed (exit code {0}).")]
    WarmFailed(i32),
    #[error("cannot start {0}: {1}")]
    Spawn(String, std::io::Error),
}

/// Runs the launch like the script, step for step; returns VIctory's exit code. With
/// `--victory-exe` the build is skipped and the repository is not needed.
pub fn run(args: &RunArgs, repo: Option<&Path>) -> Result<i32, RunError> {
    let mut victory = match (&args.victory_exe, repo) {
        (Some(exe), _) => Victory::prebuilt(exe)?,
        (None, Some(repo)) => Victory::locate(repo, args.uses_diag())?,
        (None, None) => return Err(RunError::NoRepo),
    };
    let env = environment(args)?;
    game_exe(&args.game_dir)?;
    if args.warm {
        warm_archives(&args.game_dir)?;
    }
    if args.precompile {
        steps::validate_precompile_inputs(&args.game_dir)?;
    }
    if let Some(mut build) = victory.build_command() {
        let profile = if args.uses_diag() {
            "diag profile, diagnostics feature"
        } else {
            "release"
        };
        println!("Building VIctory ({profile})...");
        victory.exe = steps::build_executable(&mut build)?;
    }
    if args.warm {
        println!("Warming the host pipeline cache...");
        warm_or_fail(&mut victory.warm_command(&[], &env))?;
    } else if args.precompile {
        println!("Preparing the recorded host pipelines before launch...");
        warm_or_fail(&mut victory.warm_command(&[], &env))?;
    }
    status(
        &mut victory.game_command_as(&args.game_dir, &env, args.console),
        "victory",
    )
}

fn warm_or_fail(command: &mut Command) -> Result<(), RunError> {
    match status(command, "victory")? {
        0 => Ok(()),
        code => Err(RunError::WarmFailed(code)),
    }
}

/// Repository root: `--repo`, `ANYXBOXX_REPO`, then the first folder above the binary or the
/// current folder that holds third_party/victory/Cargo.toml.
pub fn find_repo(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(repo) = explicit {
        return Some(repo.to_path_buf());
    }
    if let Some(repo) = std::env::var_os("ANYXBOXX_REPO") {
        return Some(PathBuf::from(repo));
    }
    let starts = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf)),
        std::env::current_dir().ok(),
    ];
    starts.into_iter().flatten().find_map(|start| {
        start
            .ancestors()
            .find(|dir| {
                dir.join("third_party")
                    .join("victory")
                    .join("Cargo.toml")
                    .is_file()
            })
            .map(Path::to_path_buf)
    })
}

#[cfg(test)]
mod tests;
