use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;

use super::*;

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    run: RunArgs,
}

fn parse(args: &[&str]) -> RunArgs {
    Cli::parse_from(std::iter::once("anyxboxx").chain(args.iter().copied())).run
}

fn names(env: &[(String, OsString)]) -> Vec<&str> {
    env.iter().map(|(k, _)| k.as_str()).collect()
}

fn value(env: &[(String, OsString)], name: &str) -> Option<OsString> {
    env.iter()
        .rev()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

/// Always set, before the tuned set: the script sets these two first.
const HEAD: [&str; 2] = ["VICTORY_GAME_DIR", "VICTORY_DISCORD"];

#[test]
fn tuned_settings_are_set_by_default_and_can_be_turned_off() {
    let env = settings::base_with_cpus(&parse(&["--game-dir", "G"]), 8);
    assert_eq!(
        env[0],
        ("VICTORY_GAME_DIR".to_string(), OsString::from("G"))
    );
    assert_eq!(env.len(), HEAD.len() + TUNED.len());
    assert!(
        TUNED
            .iter()
            .all(|(k, v)| value(&env, k) == Some(OsString::from(v)))
    );
    assert_eq!(value(&env, "VICTORY_DISCORD"), Some(OsString::from("0")));
    assert_eq!(value(&env, "VICTORY_OCEAN_READBACK"), None);
    let env = settings::base(&parse(&["--game-dir", "G", "--no-tuned-settings"]));
    assert_eq!(names(&env), HEAD);
}

#[test]
fn the_tuned_set_is_vice_tay_s_4070_set() {
    // Golden copy: tests/fixtures/lancer_victory_vicetay.ps1 (checked in tests/test_anyxboxx_run.py).
    assert_eq!(TUNED.len(), 26);
    assert!(TUNED.contains(&("VICTORY_CARD_WINDOWS_MB", "1024")));
    assert!(TUNED.contains(&("VICTORY_CARD_BUFFERS_MB", "1536")));
    assert!(TUNED.contains(&("VICTORY_RELAY", "2")));
    assert!(TUNED.contains(&("VICTORY_D3D12_DRED", "light")));
}

#[test]
fn quality_is_applied_after_the_tuned_set_and_before_overrides() {
    let env = environment(&parse(&[
        "--game-dir",
        "G",
        "--quality",
        "--override",
        "VICTORY_RELAY=0",
    ]))
    .unwrap();
    assert_eq!(
        value(&env, "VICTORY_CARD_WINDOWS_MB"),
        Some(OsString::from("0"))
    );
    assert_eq!(
        value(&env, "VICTORY_TEXTURE_STAGE_MB"),
        Some(OsString::from("192"))
    );
    assert_eq!(value(&env, "VICTORY_RELAY"), Some(OsString::from("0")));
    let quality_at = env
        .iter()
        .position(|(k, _)| k == "VICTORY_CARD_WINDOWS_MB")
        .unwrap();
    let override_at = env.iter().rposition(|(k, _)| k == "VICTORY_RELAY").unwrap();
    assert!(quality_at < override_at);
}

#[test]
fn overrides_must_be_victory_variables_in_key_value_form() {
    assert!(Cli::try_parse_from(["anyxboxx", "--game-dir", "G", "--override", "PATH=x"]).is_err());
    assert!(
        Cli::try_parse_from(["anyxboxx", "--game-dir", "G", "--override", "VICTORY_RELAY"])
            .is_err()
    );
    let args = parse(&["--game-dir", "G", "--override", "VICTORY_LATE_WAIT=12000"]);
    assert_eq!(
        args.overrides,
        [("VICTORY_LATE_WAIT".to_string(), "12000".to_string())]
    );
}

#[test]
fn the_tuned_table_has_dred_light_and_no_xg() {
    // DRED=light is part of the measured set; the full mode and XG stay opt-in.
    assert!(
        TUNED
            .iter()
            .all(|(k, v)| (!k.contains("DRED") || *v == "light") && !k.contains("XG"))
    );
}

#[test]
fn dred_asked_for_wins_over_the_tuned_light_value() {
    let env = settings::base(&parse(&["--game-dir", "G", "--dred"]));
    assert_eq!(value(&env, "VICTORY_D3D12_DRED"), Some(OsString::from("1")));
    let tuned_at = env
        .iter()
        .position(|(k, _)| k == "VICTORY_D3D12_DRED")
        .unwrap();
    let dred_at = env
        .iter()
        .rposition(|(k, _)| k == "VICTORY_D3D12_DRED")
        .unwrap();
    assert!(tuned_at < dred_at);
    let env = settings::base(&parse(&["--game-dir", "G"]));
    assert_eq!(
        value(&env, "VICTORY_D3D12_DRED"),
        Some(OsString::from("light"))
    );
}

#[test]
fn opt_in_switches_add_their_variable() {
    let env = settings::base(&parse(&[
        "--game-dir",
        "G",
        "--no-tuned-settings",
        "--dred",
        "--batch-card-rehash",
        "--skip-pipelines",
        "3",
        "--auto-a",
        "--frametime-dump",
    ]));
    let mut expected = HEAD.to_vec();
    expected.extend([
        "VICTORY_AUTO_A",
        "VICTORY_FRAMETIME_DUMP",
        "VICTORY_D3D12_DRED",
        "VICTORY_CARD_REHASH_RUNS",
        "VICTORY_SKIP_PIPELINES",
    ]);
    assert_eq!(names(&env), expected);
    assert_eq!(value(&env, "VICTORY_AUTO_A"), Some(OsString::from("150")));
    assert_eq!(
        value(&env, "VICTORY_SKIP_PIPELINES"),
        Some(OsString::from("3"))
    );
    // An empty value is not passed, like `-SkipPipelines ''` in the script.
    let env = settings::base(&parse(&[
        "--game-dir",
        "G",
        "--no-tuned-settings",
        "--skip-pipelines",
        "",
    ]));
    assert_eq!(names(&env), HEAD);
}

#[test]
fn dred_needs_the_diag_profile() {
    assert!(parse(&["--game-dir", "G", "--dred"]).uses_diag());
    assert!(parse(&["--game-dir", "G", "--diag"]).uses_diag());
    assert!(!parse(&["--game-dir", "G"]).uses_diag());
}

#[test]
fn powershell_names_are_accepted() {
    let args = parse(&[
        "--GameDir",
        "G",
        "--XgDll",
        "x.dll",
        "--XgMode",
        "compare",
        "--Precompile",
        "--Warm",
        "--Quality",
        "--AutoA",
    ]);
    assert_eq!(args.game_dir, PathBuf::from("G"));
    assert_eq!(args.xg_mode, XgMode::Compare);
    assert!(args.precompile && args.warm && args.quality && args.auto_a);
}

#[test]
fn the_title_runs_as_a_series_x_unless_series_s_is_asked() {
    let victory = Victory::prebuilt(&std::env::current_exe().unwrap()).unwrap();
    let args_of = |command: Command| -> Vec<OsString> {
        command.get_args().map(|a| a.to_os_string()).collect()
    };
    let game = PathBuf::from("G");
    let expected_head = [
        OsString::from("run"),
        game.join(GAME_EXE).into_os_string(),
        OsString::from("--execute"),
    ];
    // The default asks nothing more of VIctory: the command is the one the launcher has always run.
    let default = parse(&["--game-dir", "G"]);
    assert_eq!(default.console, Console::SeriesX);
    let command = victory.game_command_as(&game, &Env::new(), default.console);
    assert_eq!(args_of(command), expected_head);
    assert_eq!(
        args_of(victory.game_command(&game, &Env::new())),
        expected_head
    );
    // Series S passes VIctory's own flag after --execute; the PowerShell-style name is accepted too.
    for flag in ["--console", "--Console"] {
        let series_s = parse(&["--game-dir", "G", flag, "series-s"]);
        assert_eq!(series_s.console, Console::SeriesS);
        let mut expected = expected_head.to_vec();
        expected.extend(["--console", "series-s"].map(OsString::from));
        assert_eq!(
            args_of(victory.game_command_as(&game, &Env::new(), series_s.console)),
            expected
        );
    }
    assert_eq!(
        parse(&["--game-dir", "G", "--console", "series-x"]).console,
        Console::SeriesX
    );
}

#[test]
fn a_missing_xg_library_is_an_error() {
    let args = parse(&["--game-dir", "G", "--xg-dll", "does-not-exist/xg_xs.dll"]);
    assert!(matches!(environment(&args), Err(RunError::NoXgDll(_))));
}

#[test]
fn the_xg_library_is_passed_with_an_absolute_path_and_its_mode() {
    let dll = std::env::current_exe().unwrap();
    let args = parse(&[
        "--game-dir",
        "G",
        "--no-tuned-settings",
        "--xg-dll",
        dll.to_str().unwrap(),
    ]);
    let env = environment(&args).unwrap();
    let mut expected = HEAD.to_vec();
    expected.extend(["VICTORY_XG_DLL", "VICTORY_XG_MODE"]);
    assert_eq!(names(&env), expected);
    assert!(PathBuf::from(value(&env, "VICTORY_XG_DLL").unwrap()).is_absolute());
    assert_eq!(value(&env, "VICTORY_XG_MODE"), Some(OsString::from("real")));
}

#[test]
fn a_repository_without_victory_is_refused_before_building() {
    let empty = std::env::temp_dir().join("anyxboxx-run-no-victory");
    let args = parse(&["--game-dir", "G"]);
    assert!(matches!(run(&args, Some(&empty)), Err(RunError::NoVictory)));
}

#[test]
fn a_prebuilt_victory_has_nothing_to_build() {
    let exe = std::env::current_exe().unwrap();
    let victory = Victory::prebuilt(&exe).unwrap();
    assert_eq!(victory.exe, exe);
    assert!(victory.build_command().is_none());
}

#[test]
fn a_located_victory_is_built_in_release_by_default_and_in_diag_on_request() {
    let repo = std::env::temp_dir().join(format!("anyxboxx-run-located-{}", std::process::id()));
    let victory = repo.join("third_party").join("victory");
    std::fs::create_dir_all(&victory).unwrap();
    std::fs::write(victory.join("Cargo.toml"), b"").unwrap();
    let release = Victory::locate(&repo, false).unwrap();
    let diag = Victory::locate(&repo, true).unwrap();
    std::fs::remove_dir_all(&repo).unwrap();
    assert!(
        release.exe.ends_with("release/victory.exe")
            || release.exe.ends_with(r"release\victory.exe")
    );
    assert!(diag.exe.ends_with(r"diag\victory.exe") || diag.exe.ends_with("diag/victory.exe"));
    let build = release.build_command().unwrap();
    assert_eq!(build.get_program(), "cargo");
    assert!(build.get_args().any(|a| a == "--release"));
    assert!(
        diag.build_command()
            .unwrap()
            .get_args()
            .any(|a| a == "diagnostics")
    );
}

#[test]
fn warm_archives_are_the_game_folder_rpf_files_only_sorted() {
    let dir = std::env::temp_dir().join(format!("anyxboxx-run-warm-{}", std::process::id()));
    let nested = dir.join("sub");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(dir.join("b.rpf"), b"").unwrap();
    std::fs::write(dir.join("A.RPF"), b"").unwrap();
    std::fs::write(dir.join("notes.txt"), b"").unwrap();
    std::fs::write(nested.join("deep.rpf"), b"").unwrap();
    let found = steps::warm_archives(&dir);
    let empty =
        std::env::temp_dir().join(format!("anyxboxx-run-warm-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let none = steps::warm_archives(&empty);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&empty).unwrap();
    let names: Vec<String> = found
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["A.RPF", "b.rpf"]);
    assert!(matches!(none, Err(RunError::NoArchive(_))));
}

#[test]
fn a_missing_prebuilt_victory_is_refused_before_anything_runs() {
    let args = parse(&[
        "--game-dir",
        "G",
        "--victory-exe",
        "does-not-exist/victory.exe",
    ]);
    assert!(matches!(run(&args, None), Err(RunError::NoVictoryExe(_))));
}

#[test]
fn a_prebuilt_victory_does_not_need_the_repository() {
    // The game folder is checked after the exe: reaching NoGame means no repository was asked for.
    let exe = std::env::current_exe().unwrap();
    let args = parse(&[
        "--game-dir",
        "does-not-exist",
        "--VictoryExe",
        exe.to_str().unwrap(),
    ]);
    assert!(matches!(run(&args, None), Err(RunError::NoGame(_))));
}

#[test]
fn without_a_prebuilt_victory_the_repository_is_required() {
    let args = parse(&["--game-dir", "G"]);
    assert!(matches!(run(&args, None), Err(RunError::NoRepo)));
}

#[test]
fn linux_amd_profile_sets_its_variables_instead_of_windows_tuned() {
    let env = settings::base(&parse(&["--game-dir", "G", "--profile", "linux-amd"]));
    assert_eq!(env.len(), HEAD.len() + LINUX_AMD.len());
    assert!(
        LINUX_AMD
            .iter()
            .all(|(k, v)| value(&env, k) == Some(OsString::from(v)))
    );
    assert_eq!(
        value(&env, "VKD3D_CONFIG"),
        Some(OsString::from("descriptor_heap"))
    );
    assert_eq!(value(&env, "VICTORY_RELAY"), None);
}

#[test]
fn defaults_profile_and_no_tuned_settings_leave_only_base_variables() {
    let defaults = settings::base(&parse(&["--game-dir", "G", "--Profile", "defaults"]));
    assert_eq!(names(&defaults), HEAD);
    let disabled = settings::base(&parse(&[
        "--game-dir",
        "G",
        "--profile",
        "linux-amd",
        "--no-tuned-settings",
    ]));
    assert_eq!(names(&disabled), HEAD);
}

#[test]
fn pool_sizes_are_the_16_core_set_from_16_logical_cpus() {
    let big = Some(
        &[
            ("VICTORY_THIN_THREADS", "12"),
            ("VICTORY_UNDER_THREADS", "8"),
            ("VICTORY_LAND_THREADS", "6"),
        ][..],
    );
    for cpus in [7, 8, 15] {
        assert_eq!(settings::pool_sizes(cpus), None, "{cpus}");
    }
    for cpus in [16, 20, 23] {
        assert_eq!(settings::pool_sizes(cpus), big, "{cpus}");
    }
    let huge = Some(
        &[
            ("VICTORY_THIN_THREADS", "12"),
            ("VICTORY_UNDER_THREADS", "8"),
            ("VICTORY_LAND_THREADS", "8"),
        ][..],
    );
    for cpus in [24, 32] {
        assert_eq!(settings::pool_sizes(cpus), huge, "{cpus}");
    }
}

#[test]
fn big_pools_come_after_the_tuned_set_and_before_quality_and_overrides() {
    let args = parse(&[
        "--game-dir",
        "G",
        "--quality",
        "--override",
        "VICTORY_LAND_THREADS=5",
    ]);
    let env = settings::base_with_cpus(&args, 24);
    let thin = value(&env, "VICTORY_THIN_THREADS");
    assert_eq!(thin, Some(OsString::from("12")));
    let under = value(&env, "VICTORY_UNDER_THREADS");
    assert_eq!(under, Some(OsString::from("8")));
    // 24+ CPUs land on 8 threads (#469); the user's override is applied later, at launch.
    assert_eq!(value(&env, "VICTORY_LAND_THREADS"), Some(OsString::from("8")));
    let positions = |name: &str| -> Vec<usize> {
        (0..env.len()).filter(|i| env[*i].0 == name).collect()
    };
    let land = positions("VICTORY_LAND_THREADS");
    let quality = positions("VICTORY_CARD_WINDOWS_MB");
    assert_eq!(land.len(), 2);
    assert!(land[1] < *quality.last().unwrap());
    let launch_env = environment(&args).unwrap();
    assert_eq!(value(&launch_env, "VICTORY_LAND_THREADS"), Some(OsString::from("5")));
    // Small machines, `--no-tuned-settings` and other profiles keep what they had.
    let small = settings::base_with_cpus(&parse(&["--game-dir", "G"]), 8);
    assert_eq!(
        value(&small, "VICTORY_LAND_THREADS"),
        Some(OsString::from("4"))
    );
    let off = settings::base_with_cpus(&parse(&["--game-dir", "G", "--no-tuned-settings"]), 24);
    assert_eq!(names(&off), HEAD);
    let amd = settings::base_with_cpus(
        &parse(&["--game-dir", "G", "--profile", "linux-amd"]),
        24,
    );
    assert_eq!(value(&amd, "VICTORY_LAND_THREADS"), None);
}
