//! `anyxboxx-launcher`: the graphical launcher a player double-clicks. It checks the setup,
//! verifies the game folder, preloads VIctory's shader cache, picks a profile for the host and the
//! GPU, and starts the game through the same steps as `anyxboxx run`.
//!
//! GUI library: egui through eframe (pure Rust, no C++ binding, no installer). The window is a
//! handful of pages with a log and progress bars, which immediate-mode egui draws in a few hundred
//! lines, and it renders through the OS's own graphics API. The alternative, raw Win32 through the
//! `windows` crate, would have meant hand-written message loops and control layout for the same
//! pages, for no gain in what the launcher does. The logic stays in plain modules that the window
//! only calls, so the tests never need a window.

// No console window in release builds: the children's output goes to the log drawer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about;
mod anim;
mod app;
mod app_check;
mod app_launch;
mod bar;
mod cache;
mod cover;
mod detached;
mod env;
mod format;
mod home;
mod host;
mod job;
mod launch;
mod launch_progress;
mod loglevel;
mod manifest;
mod noconsole;
mod options;
mod picker;
mod processes;
mod profile;
mod progress;
mod renderer;
mod session;
mod setup;
mod shade;
mod smooth_motion;
mod splash;
mod store;
mod theme;
mod toast;
mod tokens;
mod ui;
mod ui_chrome;
mod ui_home;
mod ui_launch;
mod ui_manifest;
mod ui_options;
mod ui_pages;
mod ui_play;
mod ui_states;
mod verify;
mod warm_state;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnyXboxX launcher")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([860.0, 560.0]),
        wgpu_options: renderer::wgpu_options(host::detect_host()),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "AnyXboxX launcher",
        options,
        Box::new(|creation| {
            theme::apply(&creation.egui_ctx);
            let mut state = app::App::new()?;
            state.apply_args(app_launch::parse_args(std::env::args().skip(1)));
            Ok(Box::new(state))
        }),
    )
}
