//! The launch options of the Play page and the environment each one sets. Every option that
//! reaches VIctory goes through the one table in `env_pairs`, and `ENV_NAMES` lists the variable
//! names it may use: a test checks each of them against VIctory's own sources in this tree, so an
//! option never names a variable nothing reads.
//!
//! Not offered, because the Neptune tree of VIctory has nothing behind them: the Series S console
//! profile. Discord presence stays off (VICTORY_DISCORD=0, set by `anyxboxx run`).

use serde::{Deserialize, Serialize};

use crate::env::Pair;

pub const DISPLAY_MODE: &str = "VICTORY_DISPLAY_MODE";
/// The size the title renders its output at (Vice-Tay base): `VICTORY_DISPLAY_MODE` only changes
/// what DXGI reports, the title keeps the size its own settings hold (4K as a Series X wrote them).
pub const OUTPUT: &str = "VICTORY_OUTPUT";
/// The sizes the title has a display mode for: `VICTORY_OUTPUT` takes only these.
pub const TITLE_MODES: [(u32, u32); 5] = [(1280, 720), (1664, 936), (1920, 1080), (2560, 1440), (3840, 2160)];
pub const WINDOW_MODE: &str = "VICTORY_WINDOW_MODE";
pub const KEYBOARD: &str = "VICTORY_KEYBOARD";
pub const LOG: &str = "VICTORY_LOG";
/// `VICTORY_OVERLAY=0` turns the stats overlay off; F10 shows and hides it otherwise.
pub const OVERLAY: &str = "VICTORY_OVERLAY";
/// Taps A on the start screens for 150 seconds (the same value as `--auto-a`).
pub const AUTO_A: &str = "VICTORY_AUTO_A";
/// One row a second into the cache folder's frametime.csv.
pub const FRAMETIME_DUMP: &str = "VICTORY_FRAMETIME_DUMP";
/// Every variable an option may set (the preset's own are in `Preset::overrides`); the tests
/// check them against VIctory's sources.
#[cfg(test)]
pub const ENV_NAMES: [&str; 8] = [
    DISPLAY_MODE,
    OUTPUT,
    WINDOW_MODE,
    KEYBOARD,
    LOG,
    OVERLAY,
    AUTO_A,
    FRAMETIME_DUMP,
];

/// VIctory accepts a display size inside these bounds and falls back to 4K outside them.
const WIDTH_RANGE: (u32, u32) = (640, 7680);
const HEIGHT_RANGE: (u32, u32) = (360, 4320);

/// The "8 GB card" preset: VIctory's own reserves on the card cut down so that the card stays
/// under the budget Windows gives the game. With the Windows + NVIDIA profile (1536 MB of draw-data
/// windows, 1536 MB of buffer copies, sized for 12 to 16 GB cards) an RTX 5060 8 GB stood at
/// 8,010 MB in use against a 6,823 MB budget: Windows moved the rest into the PC's memory (6,096 MB
/// of shared memory in use, 1,970 MB of RAM left free) and the texture queue was refused for good.
/// The windows and buffer values are the ones measured on that card at a 5.8 fps median in the
/// world (2026-11-26, Series X, 1080p display mode, Neptune base; Vice-Tay reads no list pool variable).
/// `VICTORY_TEXTURE_MAX_SIZE=1024` keeps the levels of the title's textures over 1024 texels off the
/// card (`victory-gpu` `tiles::max_size`; three quarters of a texture for each level cut): the host
/// copy of such a texture begins at its first level within the cap (`resource.rs` `cap_base`), so
/// the texture queue has room for every texture instead of being refused. The title still sees
/// its textures at their own size; textures the card copies into or out of keep every level.
pub const LOW_VRAM: &[(&str, &str)] = &[
    ("VICTORY_CARD_WINDOWS_MB", "512"),
    ("VICTORY_CARD_BUFFERS_MB", "512"),
    ("VICTORY_TEXTURE_ROOM_PERCENT", "85"),
    ("VICTORY_TEXTURE_MAX_SIZE", "1024"),
];

/// The "12 GB card" preset: Fast's speed with headroom. The profile's reserves (1536 MB of windows
/// and of buffer copies) were sized on 12 to 16 GB cards and measured on a 16 GB one; here they
/// are 1024 MB each, and the title's textures are capped at 2048 texels (`VICTORY_TEXTURE_MAX_SIZE`,
/// one level cut on the 4096 ones only), so the texture queue keeps room on a 12 GB card. Not
/// measured on a 12 GB card yet.
pub const MID_VRAM: &[(&str, &str)] = &[
    ("VICTORY_CARD_WINDOWS_MB", "1024"),
    ("VICTORY_CARD_BUFFERS_MB", "1024"),
    ("VICTORY_TEXTURE_ROOM_PERCENT", "90"),
    ("VICTORY_TEXTURE_MAX_SIZE", "2048"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    #[default]
    Fast,
    Quality,
    /// For cards with 8 GB of video memory or less.
    LowVram,
    /// For cards with 12 GB of video memory.
    MidVram,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::Fast,
        Preset::Quality,
        Preset::MidVram,
        Preset::LowVram,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Fast => "Fast",
            Preset::Quality => "Quality",
            Preset::LowVram => "8 GB card",
            Preset::MidVram => "12 GB card",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Preset::Fast => "The profile as measured: the best speed known so far.",
            Preset::Quality => {
                "Picture first: windows and buffers read fresh each frame. About half the frame rate (measured 7 to 9 fps against 14 to 17)."
            }
            Preset::LowVram => {
                "For 8 GB cards: textures capped at 1024 pixels and smaller reserves on the card, so every texture fits and the card stays within its budget instead of spilling into the PC's memory. Pair it with the 1920x1080 display size."
            }
            Preset::MidVram => {
                "For 12 GB cards: Fast with headroom. Textures capped at 2048 pixels (only the largest lose a level) and 1024 MB reserves on the card, so every texture fits. Not measured on a 12 GB card yet."
            }
        }
    }

    /// The variables this preset adds on top of the profile. Fast is the profile itself; Quality
    /// is main's `-Quality` set (`anyxboxx::run::QUALITY`).
    pub fn overrides(self) -> Vec<Pair> {
        match self {
            Preset::Fast => Vec::new(),
            Preset::Quality => anyxboxx::run::QUALITY
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            Preset::LowVram => LOW_VRAM
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            Preset::MidVram => MID_VRAM
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// VIctory's own default (the 4K display the console reports).
    #[default]
    Default,
    P720,
    P900,
    P1080,
    P1440,
    P2160,
    Custom,
}

impl Resolution {
    pub const ALL: [Resolution; 7] = [
        Resolution::Default,
        Resolution::P720,
        Resolution::P900,
        Resolution::P1080,
        Resolution::P1440,
        Resolution::P2160,
        Resolution::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Resolution::Default => "Default",
            Resolution::P720 => "1280x720",
            Resolution::P900 => "1600x900",
            Resolution::P1080 => "1920x1080",
            Resolution::P1440 => "2560x1440",
            Resolution::P2160 => "3840x2160",
            Resolution::Custom => "Custom",
        }
    }

    fn fixed_size(self) -> Option<(u32, u32)> {
        match self {
            Resolution::P720 => Some((1280, 720)),
            Resolution::P900 => Some((1600, 900)),
            Resolution::P1080 => Some((1920, 1080)),
            Resolution::P1440 => Some((2560, 1440)),
            Resolution::P2160 => Some((3840, 2160)),
            Resolution::Default | Resolution::Custom => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Windowed,
    Borderless,
}

impl WindowMode {
    pub const ALL: [WindowMode; 2] = [WindowMode::Windowed, WindowMode::Borderless];

    pub fn label(self) -> &'static str {
        match self {
            WindowMode::Windowed => "Windowed",
            WindowMode::Borderless => "Borderless full screen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keyboard {
    /// By physical key position: ZQSD works on AZERTY with nothing to set.
    #[default]
    Auto,
    Qwerty,
    Azerty,
    Qwertz,
}

impl Keyboard {
    pub const ALL: [Keyboard; 4] = [
        Keyboard::Auto,
        Keyboard::Qwerty,
        Keyboard::Azerty,
        Keyboard::Qwertz,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Keyboard::Auto => "Auto (physical keys)",
            Keyboard::Qwerty => "QWERTY",
            Keyboard::Azerty => "AZERTY",
            Keyboard::Qwertz => "QWERTZ",
        }
    }

    /// The letters of the left stick, up/left/down/right, as printed on that keyboard.
    pub fn move_keys(self) -> &'static str {
        match self {
            Keyboard::Azerty => "Z Q S D",
            Keyboard::Auto => "W A S D positions (Z Q S D on AZERTY)",
            Keyboard::Qwerty | Keyboard::Qwertz => "W A S D",
        }
    }

    /// The letters of the bumper and trigger keys (left/right) as printed on that keyboard.
    pub fn shoulder_keys(self) -> &'static str {
        match self {
            Keyboard::Azerty => "Bumpers A E, triggers W C",
            Keyboard::Qwertz => "Bumpers Q E, triggers Y C",
            Keyboard::Auto | Keyboard::Qwerty => "Bumpers Q E, triggers Z C",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    #[default]
    Default,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    pub const ALL: [LogLevel; 4] = [
        LogLevel::Default,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogLevel::Default => "Default",
            LogLevel::Warn => "Warnings only",
            LogLevel::Info => "Info",
            LogLevel::Debug => "Debug (slow)",
        }
    }
}

/// What the Play page's cards choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchOptions {
    pub preset: Preset,
    pub resolution: Resolution,
    pub custom_width: u32,
    pub custom_height: u32,
    pub window: WindowMode,
    pub keyboard: Keyboard,
    pub log: LogLevel,
    /// Translate the shaders into the cache before starting the game.
    pub warm_first: bool,
    /// The stats overlay (F10 shows and hides it). Off sets VICTORY_OVERLAY=0.
    pub overlay: bool,
    /// Taps A through the start screens for 150 seconds.
    pub auto_a: bool,
    /// Writes a frame-time CSV into the cache folder.
    pub frametime_dump: bool,
    /// NVIDIA Smooth Motion (driver frame generation) for victory.exe: written into the driver's
    /// profile before each launch (`smooth_motion`), not into VIctory's environment.
    pub smooth_motion: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            preset: Preset::Fast,
            // Full HD by default: the title renders at 1280x720 inside instead of 2560x1440
            // (4K, the size a Series X wrote in its settings). Measured on an RTX 4070 SUPER,
            // Series X: about 20 fps in the world at 4K, 24-30 at 1080p.
            resolution: Resolution::P1080,
            custom_width: 1920,
            custom_height: 1080,
            window: WindowMode::Windowed,
            keyboard: Keyboard::Auto,
            log: LogLevel::Default,
            warm_first: false,
            overlay: true,
            auto_a: false,
            frametime_dump: false,
            smooth_motion: false,
        }
    }
}

impl LaunchOptions {
    /// The display size to report, `None` for VIctory's default (or an out-of-range custom size).
    pub fn size(&self) -> Option<(u32, u32)> {
        match self.resolution {
            Resolution::Custom => self
                .custom_valid()
                .then_some((self.custom_width, self.custom_height)),
            other => other.fixed_size(),
        }
    }

    /// Whether the custom size is one VIctory accepts.
    pub fn custom_valid(&self) -> bool {
        (WIDTH_RANGE.0..=WIDTH_RANGE.1).contains(&self.custom_width)
            && (HEIGHT_RANGE.0..=HEIGHT_RANGE.1).contains(&self.custom_height)
    }

    /// The variables the options set: the preset's, then one table row per option that left its
    /// default. The user's own overrides go after these and win.
    pub fn env_pairs(&self) -> Vec<Pair> {
        let mut pairs = self.preset.overrides();
        if let Some((w, h)) = self.size() {
            pairs.push((DISPLAY_MODE.to_string(), format!("{w}x{h}")));
            if TITLE_MODES.contains(&(w, h)) {
                pairs.push((OUTPUT.to_string(), format!("{w}x{h}")));
            }
        }
        if self.window == WindowMode::Borderless {
            pairs.push((WINDOW_MODE.to_string(), "borderless".to_string()));
        }
        let keyboard = match self.keyboard {
            Keyboard::Auto => None,
            Keyboard::Qwerty => Some("qwerty"),
            Keyboard::Azerty => Some("azerty"),
            Keyboard::Qwertz => Some("qwertz"),
        };
        if let Some(value) = keyboard {
            pairs.push((KEYBOARD.to_string(), value.to_string()));
        }
        let log = match self.log {
            LogLevel::Default => None,
            LogLevel::Warn => Some("warn"),
            LogLevel::Info => Some("info"),
            LogLevel::Debug => Some("debug"),
        };
        if let Some(value) = log {
            pairs.push((LOG.to_string(), value.to_string()));
        }
        if !self.overlay {
            pairs.push((OVERLAY.to_string(), "0".to_string()));
        }
        if self.auto_a {
            pairs.push((AUTO_A.to_string(), "150".to_string()));
        }
        if self.frametime_dump {
            pairs.push((FRAMETIME_DUMP.to_string(), "1".to_string()));
        }
        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_defaults_set_only_the_full_hd_size() {
        let pairs = LaunchOptions::default().env_pairs();
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [DISPLAY_MODE, OUTPUT], "{pairs:?}");
    }

    #[test]
    fn every_choice_maps_to_its_variable() {
        let options = LaunchOptions {
            preset: Preset::Quality,
            resolution: Resolution::P1440,
            window: WindowMode::Borderless,
            keyboard: Keyboard::Azerty,
            log: LogLevel::Debug,
            ..LaunchOptions::default()
        };
        let pairs = options.env_pairs();
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get(DISPLAY_MODE), Some("2560x1440"));
        assert_eq!(get(OUTPUT), Some("2560x1440"));
        assert_eq!(get(WINDOW_MODE), Some("borderless"));
        assert_eq!(get(KEYBOARD), Some("azerty"));
        assert_eq!(get(LOG), Some("debug"));
        assert_eq!(get("VICTORY_CARD_WINDOWS_MB"), Some("0"));
        assert_eq!(get("VICTORY_TEXTURE_STAGE_MB"), Some("192"));
    }

    #[test]
    fn overlay_auto_a_and_frametime_are_switches_of_their_own() {
        let options = LaunchOptions {
            overlay: false,
            auto_a: true,
            frametime_dump: true,
            ..LaunchOptions::default()
        };
        let pairs = options.env_pairs();
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get(OVERLAY), Some("0"));
        assert_eq!(get(AUTO_A), Some("150"));
        assert_eq!(get(FRAMETIME_DUMP), Some("1"));
        // The defaults keep the overlay on and set nothing else.
        assert!(LaunchOptions::default().overlay);
    }

    #[test]
    fn quality_is_exactly_mains_quality_set() {
        let quality = Preset::Quality.overrides();
        let expected: Vec<Pair> = anyxboxx::run::QUALITY
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        assert_eq!(quality, expected);
        assert_eq!(quality.len(), 3);
    }

    #[test]
    fn the_8_gb_preset_cuts_the_card_reserves_below_the_profiles() {
        let pairs = Preset::LowVram.overrides();
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.parse::<u32>().expect("a number"))
        };
        let profile = |name: &str| {
            anyxboxx::run::TUNED
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.parse::<u32>().expect("a number"))
                .expect("in the tuned set")
        };
        for name in ["VICTORY_CARD_WINDOWS_MB", "VICTORY_CARD_BUFFERS_MB"] {
            let low = get(name).expect("set by the preset");
            assert!(
                low < profile(name),
                "{name}: {low} is not below the profile"
            );
        }
        // The size cap, and the switch it needs in the profile.
        assert_eq!(get("VICTORY_TEXTURE_MAX_SIZE"), Some(1024));
        // The texture room is not in the tuned set any more: the preset sits below VIctory's default (90).
        assert!(get("VICTORY_TEXTURE_ROOM_PERCENT").expect("set by the preset") < 90);
        assert!(Preset::ALL.contains(&Preset::LowVram));
    }

    #[test]
    fn the_12_gb_preset_sits_between_fast_and_the_8_gb_one() {
        let value = |preset: Preset, name: &str| {
            preset
                .overrides()
                .into_iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.parse::<u32>().expect("a number"))
                .expect("set by the preset")
        };
        let fast = |name: &str| {
            anyxboxx::run::TUNED
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.parse::<u32>().expect("a number"))
                .expect("in the tuned set")
        };
        for name in ["VICTORY_CARD_WINDOWS_MB", "VICTORY_CARD_BUFFERS_MB"] {
            let mid = value(Preset::MidVram, name);
            // Fast's windows are 1024 MB too, since the Vice-Tay set: the buffers carry the difference.
            assert!(mid <= fast(name), "{name}: {mid} is above Fast");
            assert!(
                mid > value(Preset::LowVram, name),
                "{name}: {mid} is not above 8 GB"
            );
        }
        assert_eq!(value(Preset::MidVram, "VICTORY_TEXTURE_MAX_SIZE"), 2048);
        assert!(Preset::ALL.contains(&Preset::MidVram));
    }

    #[test]
    fn a_custom_size_is_checked_against_victorys_bounds() {
        let mut options = LaunchOptions {
            resolution: Resolution::Custom,
            custom_width: 1366,
            custom_height: 768,
            ..LaunchOptions::default()
        };
        assert_eq!(options.size(), Some((1366, 768)));
        options.custom_width = 100;
        assert_eq!(options.size(), None);
        assert!(options.env_pairs().is_empty());
        options.custom_width = 1366;
        options.custom_height = 5000;
        assert!(!options.custom_valid());
    }

    #[test]
    fn the_legend_shows_the_keys_of_the_layout() {
        assert_eq!(Keyboard::Azerty.move_keys(), "Z Q S D");
        assert_eq!(Keyboard::Qwerty.move_keys(), "W A S D");
        assert!(Keyboard::Auto.move_keys().contains("AZERTY"));
        assert!(Keyboard::Qwertz.shoulder_keys().contains("Y C"));
    }

    #[test]
    fn every_variable_an_option_names_exists_in_victorys_sources() {
        let victory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("third_party")
            .join("victory")
            .join("crates");
        if !victory.is_dir() {
            return;
        }
        let mut sources = String::new();
        collect_rs(&victory, &mut sources);
        let preset_names: Vec<String> = Preset::Quality
            .overrides()
            .into_iter()
            .chain(Preset::LowVram.overrides())
            .chain(Preset::MidVram.overrides())
            .map(|(k, _)| k)
            .collect();
        for name in ENV_NAMES
            .iter()
            .copied()
            .chain(preset_names.iter().map(String::as_str))
        {
            assert!(
                sources.contains(&format!("\"{name}\"")),
                "{name} is read nowhere in third_party/victory"
            );
        }
    }

    fn collect_rs(dir: &Path, into: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs(&path, into);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                into.push_str(&text);
            }
        }
    }
    #[test]
    fn the_output_size_is_set_only_for_the_titles_own_modes() {
        let output = |resolution| {
            LaunchOptions { resolution, ..LaunchOptions::default() }
                .env_pairs()
                .into_iter()
                .find(|(k, _)| k == OUTPUT)
                .map(|(_, v)| v)
        };
        assert_eq!(output(Resolution::P1080).as_deref(), Some("1920x1080"));
        assert_eq!(output(Resolution::P720).as_deref(), Some("1280x720"));
        assert_eq!(output(Resolution::P900), None);
        assert_eq!(output(Resolution::Default), None);
    }

    #[test]
    fn a_new_launcher_plays_in_full_hd() {
        let pairs = LaunchOptions::default().env_pairs();
        assert!(pairs.iter().any(|(k, v)| k == OUTPUT && v == "1920x1080"), "{pairs:?}");
    }

}
