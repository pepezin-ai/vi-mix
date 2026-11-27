//! A draw's read of a texture through a view ([`readable`]): one lookup of
//! the texture's host side for the upload, the twin and the state.

use super::*;
use crate::object::{S_OK, guid, slot};
use crate::resource::HOSTED_ASKED;

const SHADER_RESOURCE: u32 = 0x40 | 0x80;
const COPY_SOURCE: u32 = 0x800;

/// A 64x64 RGBA8 title texture at `address` (one level, one layer).
fn texture_at(address: u64) -> This {
    let words: [u32; 14] = [3, 0, 0, 0, 64, 0, 64, 1 | 1 << 16, 28, 1, 0, 0, 0, 0];
    let i = ID3D12Resource::IID;
    let iid = guid(i.data1, i.data2, i.data3, i.data4);
    let mut out = std::ptr::null();
    let made = crate::resource::create_at_address(
        std::ptr::null(),
        address,
        words.as_ptr().cast(),
        0,
        std::ptr::null(),
        &iid,
        &mut out,
    );
    assert_eq!(made, S_OK);
    out
}

/// Title memory of `bytes`, 64 KiB aligned; leaked, as title memory
/// outlives a test.
fn title_memory(bytes: usize) -> u64 {
    let layout = std::alloc::Layout::from_size_align(bytes, 1 << 16).unwrap();
    // SAFETY: a non-zero size.
    unsafe { std::alloc::alloc_zeroed(layout) as u64 }
}

/// A draw's read looks the texture's host side up once (each lookup takes
/// the texture's lock), and the texture still moves as a read by resource
/// does: none on its first use in the list (the prelude's), then copy
/// source -> shader resource; the title-filled texture's upload is recorded
/// in the list, a texture the host wrote is not uploaded.
#[test]
fn a_texture_read_looks_its_host_side_up_once() {
    let Some(host) = crate::host::host() else {
        return; // no host GPU here
    };
    let address = title_memory(2 << 16);
    let textures: Vec<This> = (0..2).map(|k| texture_at(address + (k << 16))).collect();
    crate::texture::host_written(textures[1]);
    let list = host.new_list().unwrap();
    // The lookups of one read (the first read of a texture in a list also
    // asks for its upload, which looks it up once more).
    let read = |k: usize| {
        let before = HOSTED_ASKED.get();
        readable(&list, ResourceType::Srv, &View::Shader(textures[k] as usize, None));
        HOSTED_ASKED.get() - before
    };
    read(0);
    assert!(list.transition(textures[0], SHADER_RESOURCE).is_none(), "already a shader resource");
    assert_eq!(list.transition(textures[0], COPY_SOURCE).map(|(_, before)| before), Some(SHADER_RESOURCE));
    assert_eq!(read(0), 1, "lookups of the texture's host side in a read");
    assert_eq!(
        list.transition(textures[0], COPY_SOURCE).map(|(_, before)| before),
        Some(SHADER_RESOURCE),
        "the read moved it back from the copy source state"
    );
    assert_eq!(read(0), 1);
    assert_eq!(read(0), 1);
    assert!(list.transition(textures[0], SHADER_RESOURCE).is_none(), "the last read left it a shader resource");
    assert!(!list.first_use(textures[0] as usize), "the title-filled texture's upload is recorded in the list");
    read(1);
    assert!(list.first_use(textures[1] as usize), "a texture the host wrote is not uploaded");
    assert!(list.transition(textures[1], SHADER_RESOURCE).is_none(), "the read still moved it");
    assert_eq!(read(1), 1);
    list.close();
    type Release = extern "system" fn(This) -> u32;
    for t in textures {
        let _ = slot::<Release>(t, 0x10)(t);
    }
}
