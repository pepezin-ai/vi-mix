//! The environment `anyxboxx run` gives VIctory: the same variables, values and conditions as
//! main's `tools/lancer_victory.ps1`, on the Vice-Tay base. The golden copy of that script is
//! `tests/fixtures/lancer_victory_vicetay.ps1`; `tests/test_anyxboxx_run.py` compares both. (The Neptune-base
//! script survives as `tests/fixtures/lancer_victory_neptune.ps1`, unused.)

use std::ffi::OsString;
use std::path::Path;

use super::{Profile, RunArgs};

/// Vice-Tay's `scripts/play.ps1 -Machine 4070` (RTX 4070 / 8-core profile), not `-Safe`, windowed, no panel (our
/// overlay replaces it), in the order of the script's table. Measured 2026-11-26 on an RTX 4070 SUPER + i7-13700KF,
/// Series X, warm cache: about 20 fps median in the world against 14.2 for the Neptune set it replaces.
/// Applied unless `--no-tuned-settings`, in this order.
///
/// `VICTORY_D3D12_DRED=light` is part of the measured set: on this base it keeps the lists without markers (and only
/// does anything in the diagnostics build). `--dred` still overrides it with the full mode (`VICTORY_D3D12_DRED=1`):
/// `base` pushes that after the tuned set, the quality set and `--override`.
pub const TUNED: &[(&str, &str)] = &[
    ("VICTORY_TEXTURE_NO_WAIT", "1"),
    ("VICTORY_RELAY", "2"),
    ("VICTORY_WIDE_BACK", "1"),
    ("VICTORY_CARD_REHASH_RUNS", "1"),
    ("VICTORY_JOIN_COPIES", "1"),
    ("VICTORY_INDIRECT_WINDOW", "1"),
    ("VICTORY_RELAY_COPY", "1"),
    ("VICTORY_CARD_STEPS", "2"),
    ("VICTORY_TITLE_PRIORITIES", "0"),
    ("VICTORY_SHOW_AFTER", "1"),
    ("VICTORY_AMBIENT", "0.8"),
    ("VICTORY_D3D12_DRED", "light"),
    // The console wave64 programs a 32-lane card takes only in the wave-wide form.
    (
        "VICTORY_WAVE_WIDE_ONLY",
        "6f225b641c5b0708,5dc3b25986622949,369cbe906ea72764,2cc4c197ee29e291",
    ),
    ("VICTORY_THIN_THREADS", "8"),
    ("VICTORY_UNDER_THREADS", "6"),
    ("VICTORY_LAND_THREADS", "4"),
    ("VICTORY_MAP_THREADS", "1"),
    ("VICTORY_PIPELINE_WORKERS", "3"),
    ("VICTORY_CARD_RESERVE_BUDGETS", "1"),
    ("VICTORY_CARD_BUFFERS_MIN_KB", "64"),
    ("VICTORY_CARD_BUFFERS_MB", "1536"),
    ("VICTORY_CARD_WINDOWS_MB", "1024"),
    ("VICTORY_THIN", "1"),
    ("VICTORY_JOIN_PARTS", "8"),
    ("VICTORY_UPLOAD_AHEAD", "1"),
    ("VICTORY_INDIRECT_BATCH", "2"),
];

/// Logical CPUs from which the three worker pools below replace the 8-core `TUNED` sizes.
pub const BIG_CPU_THRESHOLD: usize = 16;

/// The larger pools, for CPUs with at least [`BIG_CPU_THRESHOLD`] logical CPUs: Vice-Tay's own 16-core values
/// (for its RTX 4090 machine). `TUNED` stays the 8-core set (Ryzen 7800X3D).
///
/// Measured 2026-11-27, RTX 4070 SUPER + i7-13700KF (24 logical CPUs), Series X, 1080p, warm cache, same build,
/// one run each: 8/6/4 gave a world median of 19.6 fps (10th percentile 5.7); 12/8/6 gave 23.8 fps (10th
/// percentile 11.9). The landing pool sat full at x3.5-3.7 of 4 threads before, x4.4-5.2 of 6 after.
const BIG_POOLS: &[(&str, &str)] = &[
    ("VICTORY_THIN_THREADS", "12"),
    ("VICTORY_UNDER_THREADS", "8"),
    ("VICTORY_LAND_THREADS", "6"),
];

/// From this many logical CPUs, the landing pool gets 8 threads ([`HUGE_POOLS`]).
pub const HUGE_CPU_THRESHOLD: usize = 24;

/// [`BIG_POOLS`] with 8 landing threads, for CPUs with at least [`HUGE_CPU_THRESHOLD`] logical CPUs.
///
/// Measured 2026-11-27, RTX 4070 SUPER + i7-13700KF (24 logical CPUs), Series X, 1080p, warm cache, same build,
/// one long run each, by draws a frame: 5,000-7,000 draws 17.1 -> 22.0 fps, over 7,000 draws 14.6 -> 17.8 fps,
/// 10th percentile 10.4 -> 16.1 fps, lighter scenes unchanged (23-25 fps). With 6 threads the landing pool sat at
/// x5.2-5.4 in heavy scenes; with 8 it reaches x6.1-6.6.
const HUGE_POOLS: &[(&str, &str)] = &[
    ("VICTORY_THIN_THREADS", "12"),
    ("VICTORY_UNDER_THREADS", "8"),
    ("VICTORY_LAND_THREADS", "8"),
];

/// The worker-pool sizes to apply over `TUNED` for a CPU with `logical_cpus` logical CPUs, or `None` to keep the
/// 8-core sizes.
pub fn pool_sizes(logical_cpus: usize) -> Option<&'static [(&'static str, &'static str)]> {
    if logical_cpus >= HUGE_CPU_THRESHOLD {
        Some(HUGE_POOLS)
    } else {
        (logical_cpus >= BIG_CPU_THRESHOLD).then_some(BIG_POOLS)
    }
}

/// Overrides the detected logical CPU count (a positive integer): lets tests and benchmarks pin the pool sizes.
pub const LOGICAL_CPUS_VAR: &str = "ANYXBOXX_LOGICAL_CPUS";

/// Logical CPUs of this machine (`std::thread::available_parallelism`, 1 when unknown), or the positive integer
/// in `ANYXBOXX_LOGICAL_CPUS`.
pub fn logical_cpus() -> usize {
    std::env::var(LOGICAL_CPUS_VAR)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .or_else(|| std::thread::available_parallelism().ok().map(|n| n.get()))
        .unwrap_or(1)
}

/// Linux (Proton) + AMD measured set (PR #199, RX 9070 XT / RADV / vkd3d-proton).
/// Kept in sync with `crates/anyxboxx-launcher/profiles.toml` (`linux-amd`).
pub const LINUX_AMD: &[(&str, &str)] = &[
    ("VICTORY_SHADER_LOOP_LIMIT", "256"),
    ("VICTORY_LATE_WAIT", "0"),
    ("VICTORY_WIDE_BACK", "0"),
    ("VICTORY_PIPELINE_WAIT_MS", "50"),
    ("VICTORY_TEXTURE_BUDGET_MB", "8192"),
    ("VICTORY_TEXTURE_NO_WAIT", "1"),
    ("VICTORY_CARD_BUFFERS_MIN_MB", "1"),
    ("VICTORY_CARD_BUFFERS_MB", "1024"),
    ("VKD3D_CONFIG", "descriptor_heap"),
];

/// Picture first (`--quality`): applied after the tuned set, before `--override`.
pub const QUALITY: &[(&str, &str)] = &[
    ("VICTORY_CARD_WINDOWS_MB", "0"),
    ("VICTORY_CARD_BUFFERS_MB", "1024"),
    ("VICTORY_TEXTURE_STAGE_MB", "192"),
];

/// Set on every launch, before the tuned set (the script sets them first).
pub const ALWAYS: &[(&str, &str)] = &[("VICTORY_DISCORD", "0")];

pub const GAME_DIR: &str = "VICTORY_GAME_DIR";
/// The tuned set carries `light`; `--dred` sets the full mode `1` after it (and after overrides), so it wins.
pub const DRED: &str = "VICTORY_D3D12_DRED";
pub const AUTO_A: &str = "VICTORY_AUTO_A";
pub const FRAMETIME_DUMP: &str = "VICTORY_FRAMETIME_DUMP";
pub const CARD_REHASH: &str = "VICTORY_CARD_REHASH_RUNS";
pub const SKIP_PIPELINES: &str = "VICTORY_SKIP_PIPELINES";
pub const XG_DLL: &str = "VICTORY_XG_DLL";
pub const XG_MODE: &str = "VICTORY_XG_MODE";

fn pairs(
    table: &'static [(&'static str, &'static str)],
) -> impl Iterator<Item = (String, OsString)> {
    table
        .iter()
        .map(|(k, v)| ((*k).to_string(), OsString::from(*v)))
}

/// Variables set before the XG library is checked, in the script's order.
pub fn base(args: &RunArgs) -> Vec<(String, OsString)> {
    base_with_cpus(args, logical_cpus())
}

/// [`base`] for a machine with `logical_cpus` logical CPUs.
pub fn base_with_cpus(args: &RunArgs, logical_cpus: usize) -> Vec<(String, OsString)> {
    let mut env = vec![(GAME_DIR.to_string(), args.game_dir.clone().into_os_string())];
    env.extend(pairs(ALWAYS));
    if !args.no_tuned_settings {
        match args.profile {
            Profile::WindowsNvidia => {
                env.extend(pairs(TUNED));
                env.extend(pairs(pool_sizes(logical_cpus).unwrap_or(&[])));
            }
            Profile::LinuxAmd => env.extend(pairs(LINUX_AMD)),
            Profile::Defaults => {}
        }
    }
    if args.quality {
        env.extend(pairs(QUALITY));
    }
    for (name, value) in &args.overrides {
        env.push((name.clone(), OsString::from(value)));
    }
    if args.auto_a {
        env.push((AUTO_A.to_string(), OsString::from("150")));
    }
    if args.frametime_dump {
        env.push((FRAMETIME_DUMP.to_string(), OsString::from("1")));
    }
    if args.dred {
        env.push((DRED.to_string(), OsString::from("1")));
    }
    if args.batch_card_rehash {
        env.push((CARD_REHASH.to_string(), OsString::from("1")));
    }
    if let Some(skip) = args.skip_pipelines.as_deref().filter(|s| !s.is_empty()) {
        env.push((SKIP_PIPELINES.to_string(), OsString::from(skip)));
    }
    env
}

/// Your own copy of the GDK's offline PC `xg_xs.dll` (never in the repository): Microsoft's
/// library lays out the textures. `absolute` is the resolved path of an existing file.
pub fn xg(absolute: &Path, mode: &str) -> [(String, OsString); 2] {
    [
        (XG_DLL.to_string(), absolute.as_os_str().to_owned()),
        (XG_MODE.to_string(), OsString::from(mode)),
    ]
}
