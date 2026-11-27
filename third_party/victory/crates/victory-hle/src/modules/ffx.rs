//! (vt-dlss, 11/27) `amd_fidelityfx_loader_xbox.dll`'s `ffxDispatch`, watched for DLSS frame
//! generation (`victory_gpu::fg`), from trustno1-coder/VIctory-NVIDIA's `modules/ffx.rs`.
//!
//! Unlike there, nothing is replaced: every dispatch goes on to the title's own FidelityFX as
//! before. An upscale dispatch first hands its depth and motion vectors, jitter, motion vector
//! scale, near/far and render size to frame generation, which gives them to Streamline at the
//! next present. Bound only with `VICTORY_FRAME_GEN` above 1 (see [`wanted`]); the title calls
//! it on Series X only with `VICTORY_UPSCALER=7` (FidelityFX; its own upscaler is kind 6).
//!
//! The structures are the public FidelityFX API's (`ffx_api.h`, `ffx_upscale.h`): a header of a
//! type and a next pointer, and resources of a pointer, a 32-byte description and a state.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// The DLL whose `ffxDispatch` is watched.
pub const LOADER: &str = "amd_fidelityfx_loader_xbox.dll";

static REAL: AtomicUsize = AtomicUsize::new(0);

/// The title's own `ffxDispatch`, which every dispatch goes on to.
pub fn set_real_dispatch(address: usize) {
    REAL.store(address, Ordering::Relaxed);
}

/// Whether the title's `ffxDispatch` import is bound to [`dispatch_address`]: only with
/// `VICTORY_FRAME_GEN` above 1 and `VICTORY_FG_FFX` not `0`.
pub fn wanted() -> bool {
    let fg = std::env::var("VICTORY_FRAME_GEN").ok().and_then(|v| v.trim().parse::<u32>().ok()).is_some_and(|n| n > 1);
    fg && std::env::var_os("VICTORY_FG_FFX").is_none_or(|v| v != "0")
}

#[repr(C)]
struct Header {
    kind: u64,
    next: *const Header,
}

#[repr(C)]
struct Resource {
    resource: *const c_void,
    description: [u32; 8],
    state: u32,
}

#[repr(C)]
struct Upscale {
    header: Header,
    list: *const c_void,
    colour: Resource,
    depth: Resource,
    motion: Resource,
    exposure: Resource,
    reactive: Resource,
    composition: Resource,
    output: Resource,
    jitter: [f32; 2],
    motion_scale: [f32; 2],
    render: [u32; 2],
    upscale: [u32; 2],
    enable_sharpening: u8,
    sharpness: f32,
    frame_time_delta: f32,
    pre_exposure: f32,
    reset: u8,
    camera_near: f32,
    camera_far: f32,
    camera_fov: f32,
    view_to_meters: f32,
    flags: u32,
}

const _: () = assert!(size_of::<Resource>() == 48);
const _: () = assert!(std::mem::offset_of!(Upscale, output) == 0x138);

/// `FFX_API_RETURN_ERROR`: what a dispatch nothing could answer returns.
const ERROR: u32 = 1;

/// `FFX_API_DISPATCH_DESC_TYPE_UPSCALE`.
const UPSCALE: u64 = 0x0001_0001;

type Dispatch = extern "system" fn(*mut *mut c_void, *const Header) -> u32;

extern "system" fn dispatch(context: *mut *mut c_void, desc: *const Header) -> u32 {
    // SAFETY: the title passes a dispatch description, whose header says what follows it.
    if let Some(header) = unsafe { desc.as_ref() }
        && header.kind == UPSCALE
    {
        // SAFETY: an upscale description, by its type.
        let d = unsafe { &*desc.cast::<Upscale>() };
        static SEEN: AtomicU32 = AtomicU32::new(0);
        if SEEN.fetch_add(1, Ordering::Relaxed) < 2 {
            tracing::info!(
                colour_size = ?(d.colour.description[2], d.colour.description[3]),
                output_size = ?(d.output.description[2], d.output.description[3]),
                render = ?d.render,
                upscale = ?d.upscale,
                depth = ?d.depth.resource,
                motion = ?d.motion.resource,
                motion_scale = ?d.motion_scale,
                near = d.camera_near,
                far = d.camera_far,
                "frame generation: the title's upscale dispatch (passed on to its FidelityFX)"
            );
        }
        #[cfg(windows)]
        victory_gpu::fg::from_upscale(
            d.render,
            [d.colour.description[2], d.colour.description[3]],
            d.jitter,
            d.motion_scale,
            d.camera_near,
            d.camera_far,
            d.reset != 0,
            d.depth.resource as u64,
            d.motion.resource as u64,
        );
    }
    match REAL.load(Ordering::Relaxed) {
        0 => {
            static TOLD: std::sync::Once = std::sync::Once::new();
            TOLD.call_once(|| tracing::error!("a FidelityFX dispatch came with the title's own ffxDispatch not found"));
            ERROR
        }
        // SAFETY: the title's own `ffxDispatch`, with its own arguments.
        real => unsafe { std::mem::transmute::<usize, Dispatch>(real)(context, desc) },
    }
}

/// What the title's import of `ffxDispatch` is bound to when [`wanted`].
pub fn dispatch_address() -> usize {
    dispatch as *const () as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// The upscale description as the public API lays it out.
    #[test]
    fn the_upscale_description_is_laid_out_as_the_public_api() {
        assert_eq!(size_of::<Header>(), 16);
        assert_eq!(offset_of!(Upscale, list), 0x10);
        assert_eq!(offset_of!(Upscale, colour), 0x18);
        assert_eq!(offset_of!(Upscale, depth), 0x48);
        assert_eq!(offset_of!(Upscale, motion), 0x78);
        assert_eq!(offset_of!(Upscale, output), 0x138);
        assert_eq!(offset_of!(Upscale, jitter), 0x168);
        assert_eq!(offset_of!(Upscale, motion_scale), 0x170);
        assert_eq!(offset_of!(Upscale, render), 0x178);
        assert_eq!(offset_of!(Upscale, reset), 0x198);
        assert_eq!(offset_of!(Upscale, camera_near), 0x19c);
        assert_eq!(offset_of!(Upscale, flags), 0x1ac);
        assert_eq!(offset_of!(Resource, description), 8);
        assert_eq!(offset_of!(Resource, state), 40);
    }

    /// With no DLL of the title's to pass it to, a dispatch is an error and not a silent success.
    #[test]
    fn a_dispatch_nothing_answers_is_an_error() {
        let before = REAL.swap(0, Ordering::Relaxed);
        let other = Header { kind: 0x0002_0001, next: std::ptr::null() };
        assert_eq!(dispatch(std::ptr::null_mut(), &other), ERROR);
        assert_eq!(dispatch(std::ptr::null_mut(), std::ptr::null()), ERROR);
        REAL.store(before, Ordering::Relaxed);
    }
}
