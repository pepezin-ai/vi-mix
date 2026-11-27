//! The environment VIctory runs with: the base of `anyxboxx::run::environment` (game folder, XG
//! library), then the profile, then the user's overrides, which win over everything. Shader and
//! game launches use the same function, because the shader cache key depends on the environment.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyxboxx::run::{Env, RunArgs, XgMode, environment, logical_cpus, pool_sizes};

use crate::profile::Profile;

/// One override typed by the user: `KEY=VALUE`.
pub type Pair = (String, String);

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("line {0}: expected KEY=VALUE, got '{1}'")]
    Syntax(usize, String),
    #[error(
        "line {0}: '{1}' is not a VIctory or Proton setting (names start with VICTORY_ or VKD3D_)"
    )]
    Key(usize, String),
    #[error("line {0}: '{1}' has no value (use 0 to turn a switch off)")]
    EmptyValue(usize, String),
    #[error(transparent)]
    Base(#[from] anyxboxx::run::RunError),
}

/// Parses the advanced panel's text: one `KEY=VALUE` per line, `#` comments and blank lines ignored.
pub fn parse_overrides(text: &str) -> Result<Vec<Pair>, EnvError> {
    let mut pairs = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| EnvError::Syntax(number, line.to_string()))?;
        let key = key.trim();
        let value = value.trim();
        if !(key.starts_with("VICTORY_") || key.starts_with("VKD3D_"))
            || !key
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(EnvError::Key(number, key.to_string()));
        }
        if value.is_empty() {
            return Err(EnvError::EmptyValue(number, key.to_string()));
        }
        pairs.push((key.to_string(), value.to_string()));
    }
    Ok(pairs)
}

/// Where the launch's XG library comes from, when the user gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xg {
    pub dll: PathBuf,
    pub mode: XgMode,
}

/// The full environment of a launch. Later entries replace earlier ones with the same key; the
/// key keeps the position of its first appearance.
pub fn build_env(
    game_dir: &Path,
    profile: &Profile,
    overrides: &[Pair],
    xg: Option<&Xg>,
) -> Result<Env, EnvError> {
    build_env_with_cpus(game_dir, profile, overrides, xg, logical_cpus())
}

/// The id of the profile whose set is `anyxboxx::run::TUNED` (the one the bigger pools apply to).
const TUNED_PROFILE_ID: &str = "windows-nvidia";

/// [`build_env`] for a machine with `logical_cpus` logical CPUs: on the tuned profile, 16 or more of them
/// replace the 8-core worker-pool sizes (after the profile, before the user's overrides).
pub fn build_env_with_cpus(
    game_dir: &Path,
    profile: &Profile,
    overrides: &[Pair],
    xg: Option<&Xg>,
    logical_cpus: usize,
) -> Result<Env, EnvError> {
    let base = environment(&RunArgs {
        game_dir: game_dir.to_path_buf(),
        skip_pipelines: None,
        // The profile replaces the tuned set, so the base must not add it a second time.
        no_tuned_settings: true,
        profile: anyxboxx::run::Profile::Defaults,
        quality: false,
        overrides: Vec::new(),
        auto_a: false,
        frametime_dump: false,
        diag: false,
        dred: false,
        warm: false,
        precompile: false,
        xg_dll: xg.map(|x| x.dll.clone()),
        xg_mode: xg.map_or(XgMode::Real, |x| x.mode),
        batch_card_rehash: false,
        victory_exe: None,
        repo: None,
    })?;
    let profile_pairs = profile.env.iter().map(|(k, v)| (k.clone(), v.clone()));
    let pools = (profile.id == TUNED_PROFILE_ID)
        .then(|| pool_sizes(logical_cpus))
        .flatten()
        .unwrap_or(&[])
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()));
    let overrides = overrides.iter().cloned();
    Ok(merge(base, profile_pairs.chain(pools).chain(overrides)))
}

/// Applies `(key, value)` pairs over `base`: a repeated key replaces the value in place.
pub fn merge(base: Env, pairs: impl IntoIterator<Item = Pair>) -> Env {
    let mut env: Env = base;
    for (key, value) in pairs {
        let value = OsString::from(value);
        match env.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => env.push((key, value)),
        }
    }
    env
}

/// The environment as `KEY=VALUE` lines, for the advanced panel.
pub fn describe(env: &Env) -> Vec<String> {
    env.iter()
        .map(|(k, v)| format!("{k}={}", v.to_string_lossy()))
        .collect()
}

/// The keys whose override changes a value the profile sets, for the panel (last override wins).
pub fn overridden_keys(profile: &Profile, overrides: &[Pair]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for (key, value) in overrides {
        let changes = profile.env.get(key).is_some_and(|p| p != value);
        if changes && !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::GpuVendor;
    use crate::profile::Profiles;
    use anyxboxx::run::TUNED;

    fn profile(id: &str) -> Profile {
        Profiles::embedded()
            .expect("profiles")
            .by_id(id)
            .expect("profile")
            .clone()
    }

    fn value(env: &Env, key: &str) -> Option<String> {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_string_lossy().into_owned())
    }

    fn game_folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("game_scarlett_master.exe"), b"x").expect("write");
        dir
    }

    #[test]
    fn overrides_parse_comments_and_blank_lines() {
        let text = "# a comment\n\nVICTORY_LATE_WAIT = 0\nVKD3D_CONFIG=descriptor_heap\n";
        assert_eq!(
            parse_overrides(text).expect("valid"),
            vec![
                ("VICTORY_LATE_WAIT".to_string(), "0".to_string()),
                ("VKD3D_CONFIG".to_string(), "descriptor_heap".to_string()),
            ]
        );
    }

    #[test]
    fn overrides_reject_bad_lines_with_their_line_number() {
        assert!(matches!(
            parse_overrides("VICTORY_X"),
            Err(EnvError::Syntax(1, _))
        ));
        assert!(matches!(
            parse_overrides("PATH=/x"),
            Err(EnvError::Key(1, _))
        ));
        assert!(matches!(
            parse_overrides("\nVICTORY_lower=1"),
            Err(EnvError::Key(2, _))
        ));
        assert!(matches!(
            parse_overrides("VICTORY_X="),
            Err(EnvError::EmptyValue(1, _))
        ));
    }

    #[test]
    fn merge_replaces_in_place_and_appends_new_keys() {
        let base = vec![
            ("A".to_string(), OsString::from("1")),
            ("B".to_string(), OsString::from("2")),
        ];
        let merged = merge(
            base,
            [
                ("B".to_string(), "9".to_string()),
                ("C".to_string(), "3".to_string()),
            ],
        );
        let keys: Vec<&str> = merged.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["A", "B", "C"]);
        assert_eq!(value(&merged, "B").as_deref(), Some("9"));
    }

    #[test]
    fn windows_nvidia_env_sets_the_tuned_settings_exactly() {
        let dir = game_folder();
        let env = build_env_with_cpus(dir.path(), &profile("windows-nvidia"), &[], None, 8)
            .expect("env");
        for (key, expected) in TUNED {
            assert_eq!(value(&env, key).as_deref(), Some(*expected), "{key}");
        }
        assert_eq!(
            value(&env, "VICTORY_GAME_DIR"),
            Some(dir.path().display().to_string())
        );
        assert_eq!(
            value(&env, "VICTORY_D3D12_DRED").as_deref(),
            Some("light"),
            "DRED=light is part of the tuned set"
        );
    }

    fn pools(env: &Env) -> Vec<Option<String>> {
        [
            "VICTORY_THIN_THREADS",
            "VICTORY_UNDER_THREADS",
            "VICTORY_LAND_THREADS",
        ]
        .iter()
        .map(|k| value(env, k))
        .collect()
    }

    fn some(values: [&str; 3]) -> Vec<Option<String>> {
        values.iter().map(|v| Some((*v).to_string())).collect()
    }

    #[test]
    fn windows_nvidia_env_takes_the_big_pools_from_16_logical_cpus() {
        let dir = game_folder();
        let nvidia = profile("windows-nvidia");
        let small = build_env_with_cpus(dir.path(), &nvidia, &[], None, 15).expect("env");
        assert_eq!(pools(&small), some(["8", "6", "4"]));
        let big = build_env_with_cpus(dir.path(), &nvidia, &[], None, 16).expect("env");
        assert_eq!(pools(&big), some(["12", "8", "6"]));
        let huge = build_env_with_cpus(dir.path(), &nvidia, &[], None, 24).expect("env");
        assert_eq!(pools(&huge), some(["12", "8", "8"]));
        assert_eq!(value(&big, "VICTORY_MAP_THREADS").as_deref(), Some("1"));
        // The user's options still win over the bigger pools.
        let overrides = parse_overrides("VICTORY_LAND_THREADS=5").expect("valid");
        let user = build_env_with_cpus(dir.path(), &nvidia, &overrides, None, 24).expect("env");
        assert_eq!(pools(&user), some(["12", "8", "5"]));
    }

    #[test]
    fn other_profiles_ignore_the_cpu_count() {
        let dir = game_folder();
        let linux = profile("linux-amd");
        let env = build_env_with_cpus(dir.path(), &linux, &[], None, 32).expect("env");
        assert_eq!(pools(&env), vec![None, None, None]);
    }

    #[test]
    fn linux_amd_env_is_the_measured_proton_set() {
        let dir = game_folder();
        let env = build_env(dir.path(), &profile("linux-amd"), &[], None).expect("env");
        assert_eq!(value(&env, "VICTORY_LATE_WAIT").as_deref(), Some("0"));
        assert_eq!(
            value(&env, "VKD3D_CONFIG").as_deref(),
            Some("descriptor_heap")
        );
        assert!(value(&env, "VICTORY_SKIP_PIPELINES").is_none());
    }

    #[test]
    fn overrides_win_over_the_profile() {
        let dir = game_folder();
        let overrides = parse_overrides("VICTORY_LATE_WAIT=0").expect("valid");
        let env = build_env(dir.path(), &profile("windows-nvidia"), &overrides, None).expect("env");
        assert_eq!(value(&env, "VICTORY_LATE_WAIT").as_deref(), Some("0"));
        assert_eq!(
            env.iter().filter(|(k, _)| k == "VICTORY_LATE_WAIT").count(),
            1
        );
    }

    #[test]
    fn overridden_keys_lists_only_changed_profile_values() {
        let windows = profile("windows-nvidia");
        let overrides = parse_overrides(
            "VICTORY_RELAY_COPY=0\nVICTORY_RELAY_COPY=1\nVICTORY_THIN=1\nVKD3D_CONFIG=x",
        )
        .expect("valid");
        assert_eq!(
            overridden_keys(&windows, &overrides),
            vec!["VICTORY_RELAY_COPY"]
        );
    }

    #[test]
    fn xg_library_adds_its_two_variables() {
        let dir = game_folder();
        let xg_dir = tempfile::tempdir().expect("tempdir");
        let dll = xg_dir.path().join("xg_xs.dll");
        std::fs::write(&dll, b"not a real library").expect("write");
        let xg = Xg {
            dll: dll.clone(),
            mode: XgMode::Compare,
        };
        let env = build_env(dir.path(), &profile("other"), &[], Some(&xg)).expect("env");
        assert_eq!(value(&env, "VICTORY_XG_MODE").as_deref(), Some("compare"));
        assert!(value(&env, "VICTORY_XG_DLL").is_some_and(|p| p.ends_with("xg_xs.dll")));
    }

    #[test]
    fn a_missing_xg_library_is_an_error() {
        let dir = game_folder();
        let xg = Xg {
            dll: PathBuf::from("Z:/nope/xg_xs.dll"),
            mode: XgMode::Real,
        };
        assert!(build_env(dir.path(), &profile("other"), &[], Some(&xg)).is_err());
    }

    #[test]
    fn an_unknown_vendor_gets_the_fallback_env() {
        let fallback = Profiles::embedded()
            .expect("profiles")
            .select(crate::host::Host::Windows, GpuVendor::Unknown)
            .clone();
        assert!(fallback.env.is_empty());
    }
}
