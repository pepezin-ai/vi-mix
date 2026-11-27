//! What a draw's read of a texture asks of the texture itself instead of
//! the shared tables (each behind a lock every recording thread took):
//! whether the host wrote it, the first level its staging buffer holds,
//! the placed texture a view names, and whether a target lies at its
//! memory. Each answer follows the table it stands for, and is given while
//! another thread holds that table's lock.

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::core::Interface;

use crate::object::{S_OK, This, guid, slot};

const RGBA8_64: [u32; 14] = [3, 0, 0, 0, 64, 0, 64, 1 | 1 << 16, 28, 1, 0, 0, 0, 0];
const RENDER_TARGET: u32 = 0x1;

/// A 64x64 RGBA8 title texture at `address` with resource flags `flags`.
fn texture_at(address: u64, flags: u32) -> This {
    let mut words = RGBA8_64;
    words[12] = flags;
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

fn release(t: This) {
    let _ = slot::<extern "system" fn(This) -> u32>(t, 0x10)(t);
}

/// Runs `prepare` on a thread of its own, then `read` there while this
/// thread holds what `hold` locks: whether `read` finished within five
/// seconds. (The lock is let go after, so a blocked `read` ends too.)
fn reads_while_held<G, S: 'static>(
    hold: impl FnOnce() -> G,
    prepare: impl FnOnce() -> S + Send + 'static,
    read: impl FnOnce(&S) + Send + 'static,
) -> bool {
    let (ready, is_ready) = mpsc::channel();
    let (go, goes) = mpsc::channel::<()>();
    let (done, is_done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let state = prepare();
        ready.send(()).unwrap();
        goes.recv().unwrap();
        read(&state);
        done.send(()).unwrap();
    });
    is_ready.recv().unwrap();
    let held = hold();
    go.send(()).unwrap();
    let finished = is_done.recv_timeout(Duration::from_secs(5)).is_ok();
    drop(held);
    worker.join().unwrap();
    finished
}

/// The first level a texture's staging buffer holds, as a read finds it
/// without the staging table's lock, follows the table: made by the first
/// upload, gone when the buffer is let go.
#[test]
fn a_textures_staged_level_follows_its_staging_buffer() {
    let Some(host) = crate::host::host() else {
        return; // no host GPU here
    };
    let t = texture_at(title_memory(1 << 16), 0);
    assert_eq!(crate::resource::staged_first(t), u32::MAX, "nothing staged yet");
    let list = host.new_list().unwrap();
    super::upload_from(&list, t, 0);
    assert_eq!(crate::resource::staged_first(t), super::staged_from(t));
    assert_eq!(crate::resource::staged_first(t), 0, "the upload staged the whole texture");
    super::unstage(t);
    assert_eq!(crate::resource::staged_first(t), u32::MAX, "let go");
    assert_eq!(super::staged_from(t), u32::MAX);
    list.close();
    release(t);
}

/// A texture the host wrote is known to be so without the set's lock, and
/// is not uploaded from title memory.
#[test]
fn a_texture_the_host_wrote_is_marked_and_not_uploaded() {
    let Some(host) = crate::host::host() else {
        return; // no host GPU here
    };
    let t = texture_at(title_memory(1 << 16), 0);
    assert!(!crate::resource::host_written_mark(t));
    super::host_written(t);
    assert!(crate::resource::host_written_mark(t));
    assert!(super::host_wrote(t as usize));
    let list = host.new_list().unwrap();
    super::upload_from(&list, t, 0);
    assert!(list.first_use(t as usize), "not uploaded");
    assert_eq!(crate::resource::staged_first(t), u32::MAX, "not staged");
    list.close();
    release(t);
}

/// A texture read again in a list, from another first level, asks whether
/// the host wrote it and which level its staging buffer holds while
/// another thread holds both tables' locks.
#[test]
fn a_texture_read_again_takes_neither_tables_lock() {
    if crate::host::host().is_none() {
        return; // no host GPU here
    }
    let prepare = || {
        let host = crate::host::host().unwrap();
        let t = texture_at(title_memory(1 << 16), 0);
        let list = host.new_list().unwrap();
        super::upload_from(&list, t, 0);
        assert_eq!(super::staged_from(t), 0);
        (t, list)
    };
    let read = |(t, list): &(This, crate::host::HostList)| {
        // Levels from 1 on: held by the staging buffer of the levels from 0.
        super::upload_from(list, *t, 1);
        list.close();
    };
    assert!(
        reads_while_held(super::hold_tables, prepare, read),
        "the read waited for the host-written set or the staging table"
    );
}

/// A placed texture's kept "no target at its memory" answer does not hide
/// a render target made there later, nor one gone after it; while kept, it
/// is given while another thread holds the targets' table.
#[test]
fn a_placed_texture_finds_a_target_made_at_its_memory_later() {
    if crate::host::host().is_none() {
        return; // no host GPU here
    }
    let at = title_memory(1 << 16);
    let placed = crate::resource::texture_at(at, &RGBA8_64).unwrap();
    assert_eq!(crate::resource::alias_of(placed), None);
    let p = placed as usize;
    assert!(
        reads_while_held(crate::resource::hold_targets, || (), move |_| {
            assert_eq!(crate::resource::alias_of(p as This), None, "the answer kept");
        }),
        "the read waited for the targets' table"
    );
    let target = texture_at(at, RENDER_TARGET);
    assert_eq!(crate::resource::alias_of(placed), Some(target), "the target made since");
    release(target);
    assert_eq!(crate::resource::alias_of(placed), None, "the target gone");
}

/// What a thread found of a placed texture is the table's (the filler
/// words are not part of the key, as before), and found again while
/// another thread holds the table's lock.
#[test]
fn placed_textures_found_again_are_the_tables() {
    if crate::host::host().is_none() {
        return; // no host GPU here
    }
    let at = title_memory(1 << 16);
    let first = crate::resource::texture_at(at, &RGBA8_64).unwrap();
    assert_eq!(crate::resource::texture_at(at, &RGBA8_64), Some(first));
    let mut filler = RGBA8_64;
    filler[1] = 7;
    assert_eq!(crate::resource::texture_at(at, &filler), Some(first), "the filler is not part of the key");
    let first = first as usize;
    // Each looked up once by that thread before the lock (what it found is
    // its own), then again while the table is held.
    let prepare = move || {
        assert_eq!(crate::resource::texture_at(at, &RGBA8_64), Some(first as This));
        assert_eq!(crate::resource::texture_at(at, &filler), Some(first as This));
    };
    let read = move |_: &()| {
        assert_eq!(crate::resource::texture_at(at, &RGBA8_64), Some(first as This));
        assert_eq!(crate::resource::texture_at(at, &filler), Some(first as This));
    };
    assert!(
        reads_while_held(crate::resource::hold_placed, prepare, read),
        "the read waited for the placed textures' table"
    );
}

/// The size cap lifted between two reads of a texture in one list (a copy
/// by the card, `keep_whole`): the capped host copy's staging buffer, and
/// the first level a read finds staged, go with that copy, so the list's
/// next read stages and uploads the whole copy instead of taking the
/// capped one's staged level as its own. A staging buffer a thread makes
/// for the capped copy after the swap (it took the copy before) is not
/// kept as the texture's either.
#[test]
fn a_texture_uncapped_after_a_read_stages_its_whole_copy() {
    let Some(host) = crate::host::host() else {
        return; // no host GPU here
    };
    if !crate::resource::held_only() {
        return;
    }
    // 4096x4096 BC7 with every level, as the title places it.
    let desc = [3u32, 0, 0, 0, 4096, 0, 4096, 13 << 16 | 1, 98, 1, 0, 0x105, 0, 0];
    let bytes = crate::resource::placed_bytes(&desc).unwrap() as usize;
    crate::tiles::TEST_MAX_SIZE.with(|cap| cap.set(Some(1024)));
    let t = crate::resource::texture_at(title_memory(bytes), &desc).unwrap();
    let list = host.new_list().unwrap();
    super::upload_from(&list, t, 0);
    let capped = crate::resource::host_copy(t).unwrap();
    assert_eq!(capped.base, 2, "capped at the 1024 level");
    assert_eq!(crate::resource::staged_first(t), 2, "the capped copy holds the levels from 2");
    assert_eq!(super::staged_from(t), 2);
    crate::resource::keep_whole(t);
    assert_eq!(crate::resource::staged_first(t), u32::MAX, "the swap let the capped copy's staging go");
    assert_eq!(super::staged_from(t), u32::MAX);
    // A thread that took the capped copy before the swap stages it after.
    assert!(super::staging(t, &capped, 0).is_some());
    assert_eq!(crate::resource::staged_first(t), u32::MAX, "not the texture's staging");
    assert_eq!(super::staged_from(t), u32::MAX);
    // The same list reads it again from level 2: not taken as staged.
    super::upload_from(&list, t, 2);
    let whole = crate::resource::host_copy(t).unwrap();
    assert_eq!(whole.base, 0, "the whole copy");
    assert_ne!(whole.serial, capped.serial);
    assert_eq!(crate::resource::staged_first(t), 2);
    let serial = super::STAGING.lock().unwrap().as_ref().and_then(|m| m.get(&(t as usize)).map(|s| s.serial));
    assert_eq!(serial, Some(whole.serial), "staged from the whole copy");
    // Another list reads it from level 0: every level of the whole copy.
    let next = host.new_list().unwrap();
    super::upload_from(&next, t, 0);
    assert_eq!(crate::resource::staged_first(t), 0);
    assert_eq!(super::staged_from(t), 0);
    crate::tiles::TEST_MAX_SIZE.with(|cap| cap.set(None));
    list.close();
    next.close();
    // (A placed texture stays in the table, as the title's memory does.)
}
