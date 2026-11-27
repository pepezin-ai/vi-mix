//! The steps of a launch as reusable pieces: the CLI (`anyxboxx run`) chains them like main's
//! script did, the graphical launcher (`crates/anyxboxx-launcher`) drives them one by one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Console, RunArgs, RunError, settings};

pub const GAME_EXE: &str = "game_scarlett_master.exe";
/// The game's two DLLs warmed with its shaders by `--precompile`.
pub const PRECOMPILED_DLLS: [&str; 2] = ["XboxBvhBuilder.dll", "amd_fidelityfx_upscaler_xbox.dll"];
pub const SHADERS: &str = "shaders.rpf";

/// Environment variables applied to the VIctory child process in order.
/// An empty value removes a VICTORY_* variable from that child.
pub type Env = Vec<(String, OsString)>;

/// Where VIctory's sources and its built `victory.exe` are. `manifest` is `None` for a
/// `victory.exe` built beforehand (`--victory-exe`): there is nothing to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Victory {
    pub manifest: Option<PathBuf>,
    pub exe: PathBuf,
    /// Built with the diag profile and the diagnostics feature (`--diag`, `--dred`).
    pub diag: bool,
}

impl Victory {
    /// VIctory in `repo`, release build by default; its target folder is
    /// `third_party\victory\target` unless `CARGO_TARGET_DIR` is set (VIctory is its own workspace).
    pub fn locate(repo: &Path, diag: bool) -> Result<Self, RunError> {
        let victory = repo.join("third_party").join("victory");
        let manifest = victory.join("Cargo.toml");
        if !manifest.is_file() {
            return Err(RunError::NoVictory);
        }
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map_or_else(|| victory.join("target"), PathBuf::from);
        let profile = if diag { "diag" } else { "release" };
        Ok(Victory {
            manifest: Some(manifest),
            exe: target.join(profile).join("victory.exe"),
            diag,
        })
    }

    /// A `victory.exe` built beforehand, used as is: under Proton there is no cargo, so VIctory
    /// is cross-built on the Linux side (`--target x86_64-pc-windows-msvc`) and only run here.
    pub fn prebuilt(exe: &Path) -> Result<Self, RunError> {
        if !exe.is_file() {
            return Err(RunError::NoVictoryExe(exe.display().to_string()));
        }
        Ok(Victory {
            manifest: None,
            exe: exe.to_path_buf(),
            diag: false,
        })
    }

    /// `cargo build` of the release profile (or the diag profile with diagnostics): does nothing
    /// when victory.exe is current. `None` for a prebuilt `victory.exe`.
    pub fn build_command(&self) -> Option<Command> {
        let manifest = self.manifest.as_ref()?;
        let mut command = Command::new("cargo");
        if self.diag {
            command.args(["build", "--profile", "diag", "-p", "victory"]);
            command.args(["--features", "diagnostics"]);
        } else {
            command.args(["build", "--release", "-p", "victory"]);
        }
        command.arg("--manifest-path").arg(manifest);
        Some(command)
    }

    /// `victory warm` with the game's environment (the cache key depends on it). On the Vice-Tay
    /// base `warm` takes no archives: it makes the host pipelines of every pipeline earlier runs
    /// noted, so the driver's cache has them before the game asks. `archives` only enter the warm
    /// fingerprint (`warm_state`), never the command line, which Vice-Tay refuses.
    pub fn warm_command(&self, archives: &[PathBuf], env: &Env) -> Command {
        let _ = archives;
        let mut command = Command::new(&self.exe);
        command.arg("warm");
        apply_env(&mut command, env);
        command
    }

    /// The warm before a first launch (`--precompile`): checks that shaders.rpf, the game and its
    /// two DLLs are there, then runs `victory warm` with the game's environment.
    pub fn precompile_command(&self, game_dir: &Path, env: &Env) -> Result<Command, RunError> {
        validate_precompile_inputs(game_dir)?;
        // Vice-Tay's `warm` reads no files: the inputs are checked so a broken game folder still
        // fails here, as before, and the command is the plain warm.
        let mut command = Command::new(&self.exe);
        command.arg("warm");
        apply_env(&mut command, env);
        Ok(command)
    }

    /// `victory run <game> --execute` with the game's environment, as a Series X.
    pub fn game_command(&self, game_dir: &Path, env: &Env) -> Command {
        self.game_command_as(game_dir, env, Console::SeriesX)
    }

    /// `victory run <game> --execute`, then the arguments of `console` (none for Series X).
    pub fn game_command_as(&self, game_dir: &Path, env: &Env, console: Console) -> Command {
        let mut command = Command::new(&self.exe);
        command
            .arg("run")
            .arg(game_dir.join(GAME_EXE))
            .arg("--execute")
            .args(console.victory_args());
        apply_env(&mut command, env);
        command
    }
}

/// Check the extra inputs required for a first launch.
pub(super) fn validate_precompile_inputs(game_dir: &Path) -> Result<(), RunError> {
    for name in std::iter::once(SHADERS).chain(PRECOMPILED_DLLS) {
        let needed = game_dir.join(name);
        if !needed.is_file() {
            return Err(RunError::PrecompileInput(needed.display().to_string()));
        }
    }
    Ok(())
}

/// The `.rpf` archives directly in `game_dir`, sorted by name (never the folder's other trees).
pub fn warm_archives(game_dir: &Path) -> Result<Vec<PathBuf>, RunError> {
    let entries = std::fs::read_dir(game_dir)
        .map_err(|e| RunError::Spawn(game_dir.display().to_string(), e))?;
    let mut archives: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rpf"))
        })
        .collect();
    if archives.is_empty() {
        return Err(RunError::NoArchive(game_dir.display().to_string()));
    }
    archives.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(archives)
}

/// The game executable of `game_dir`, which must exist.
pub fn game_exe(game_dir: &Path) -> Result<PathBuf, RunError> {
    let exe = game_dir.join(GAME_EXE);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(RunError::NoGame(game_dir.display().to_string()))
    }
}

/// Validate XG and apply explicit overrides after profile values and dedicated switches.
pub fn environment(args: &RunArgs) -> Result<Env, RunError> {
    let mut env = settings::base(args);
    if let Some(dll) = args.xg_dll.as_deref().filter(|p| !p.as_os_str().is_empty()) {
        if !dll.is_file() {
            return Err(RunError::NoXgDll(dll.display().to_string()));
        }
        let absolute =
            std::path::absolute(dll).map_err(|e| RunError::Spawn(dll.display().to_string(), e))?;
        env.extend(settings::xg(&absolute, args.xg_mode.as_str()));
    }
    env.extend(
        args.overrides
            .iter()
            .map(|(name, value)| (name.clone(), OsString::from(value))),
    );
    Ok(env)
}

fn apply_env(command: &mut Command, env: &Env) {
    for (key, value) in env {
        if key.starts_with("VICTORY_") && value.is_empty() {
            command.env_remove(key);
        } else {
            command.env(key, value);
        }
    }
}

/// Ask Cargo for the executable path so custom targets and target directories work.
pub(super) fn build_executable(command: &mut Command) -> Result<PathBuf, RunError> {
    let output = command
        .arg("--message-format=json-render-diagnostics")
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| RunError::Spawn("cargo".into(), e))?;
    if !output.status.success() {
        return Err(RunError::BuildFailed(output.status.code().unwrap_or(1)));
    }
    let executables: Vec<_> = output
        .stdout
        .split(|&byte| byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .filter_map(|message| {
            let target = &message["target"];
            let is_binary = target["kind"]
                .as_array()
                .is_some_and(|kind| kind.iter().any(|k| k == "bin"));
            if message["reason"] != "compiler-artifact" || target["name"] != "victory" || !is_binary {
                return None;
            }
            message["executable"]
                .as_str()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .collect();
    let [executable] = executables.as_slice() else {
        return Err(RunError::BuildArtifactCount(executables.len()));
    };
    if !executable.is_file() {
        return Err(RunError::NoVictoryExe(executable.display().to_string()));
    }
    Ok(executable.clone())
}

/// Waits for the command; a process killed without exit code counts as 1, like PowerShell.
pub fn status(command: &mut Command, name: &str) -> Result<i32, RunError> {
    let status = command
        .status()
        .map_err(|e| RunError::Spawn(name.to_string(), e))?;
    Ok(status.code().unwrap_or(1))
}
