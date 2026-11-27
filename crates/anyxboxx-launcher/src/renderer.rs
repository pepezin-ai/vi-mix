//! The graphics API of the launcher's own window.
//!
//! eframe draws the window through wgpu, and egui-wgpu lets wgpu try every primary backend (Vulkan and
//! Direct3D 12 on Windows) unless `WGPU_BACKEND` names one. On one Windows 11 machine (Insider build
//! 26340, issue #483) the launcher then never opened its window: the process stayed alive at about
//! 1 MB until it was killed. With `WGPU_BACKEND=dx12` it opened normally. So on Windows the launcher
//! asks for Direct3D 12 alone, the API the game needs anyway (`docs/launcher.md` requires a Direct3D 12
//! GPU), and Vulkan is never loaded for its window.
//!
//! `WGPU_BACKEND` still wins when it is set, so `vulkan` stays one setting away. Under Proton and Wine
//! the default is left alone: there Direct3D 12 is itself a translation onto Vulkan (vkd3d), and the
//! hang was not seen.

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu::Backends;

use crate::host::Host;

/// The backends the window may use: those `WGPU_BACKEND` names when it is set; else Direct3D 12
/// alone on Windows; else `default`, egui-wgpu's own choice.
pub fn window_backends(from_env: Option<Backends>, host: Host, default: Backends) -> Backends {
    match (from_env, host) {
        (Some(named), _) => named,
        (None, Host::Windows) => Backends::DX12,
        (None, Host::WineOrProton) => default,
    }
}

/// eframe's wgpu options, with the backends of [`window_backends`] for `host`.
pub fn wgpu_options(host: Host) -> WgpuConfiguration {
    let mut options = WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        let default = setup.instance_descriptor.backends;
        setup.instance_descriptor.backends = window_backends(Backends::from_env(), host, default);
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    const EGUI_DEFAULT: Backends = Backends::PRIMARY.union(Backends::GL);

    #[test]
    fn windows_asks_for_direct3d_12_alone() {
        assert_eq!(
            window_backends(None, Host::Windows, EGUI_DEFAULT),
            Backends::DX12
        );
    }

    #[test]
    fn wine_and_proton_keep_the_default() {
        assert_eq!(
            window_backends(None, Host::WineOrProton, EGUI_DEFAULT),
            EGUI_DEFAULT
        );
    }

    #[test]
    fn wgpu_backend_wins_on_every_host() {
        for host in [Host::Windows, Host::WineOrProton] {
            assert_eq!(
                window_backends(Some(Backends::VULKAN), host, EGUI_DEFAULT),
                Backends::VULKAN
            );
        }
    }

    #[test]
    fn the_options_create_their_own_instance() {
        // The backends only take effect if eframe creates the wgpu instance itself.
        assert!(matches!(
            wgpu_options(Host::WineOrProton).wgpu_setup,
            WgpuSetup::CreateNew(_)
        ));
    }
}
