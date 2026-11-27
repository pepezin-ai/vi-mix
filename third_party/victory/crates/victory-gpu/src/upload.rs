//! Memory `allocate_upload` (`list.rs`) hands out: the title's CPU writes
//! it and its shaders read it at the same address (the structure builder's
//! instance descriptions lie there).
//!
//! The console's GPU addresses have 40 bits, and a translated shader that
//! reads by address has no more (`victory_shader`'s `Lower::memory`): memory
//! from the process heap lies above 2^40 and was read at another address, as
//! zeros. So this memory is taken in blocks of 64 KiB out of arenas in
//! [4 GiB, 2^37), as upstream VIctory 0f637570 and 6d021fe3 place it (there
//! an allocation of its own each). An arena is a title buffer to the host
//! copies ([`crate::shadow`]) and lives as long as the process: the directory
//! of memory by address ([`crate::memory`]) keeps the copy it found at an
//! address for good, so the same addresses must stay the same copy.
//! `VICTORY_UPLOAD_LOW=0`: from the process heap, as before.

use std::sync::{Mutex, OnceLock};

/// What blocks are cut on: a page of the directory of memory by address.
const BLOCK: usize = 64 << 10;
/// An arena's size, if one request does not need more.
const ARENA: usize = 4 << 20;

/// Ownership of one 64 KiB block inside an [`Arena`].
/// `count == 0` means the block is free; otherwise every block belonging to the
/// allocation stores the allocation's `(first, count)` block span so
/// [`block`] never bleeds into adjacent allocations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    first: u32,
    count: u32,
}

impl Slot {
    const FREE: Slot = Slot { first: 0, count: 0 };

    fn is_free(self) -> bool {
        self.count == 0
    }
}

struct Arena {
    at: usize,
    /// Per block its owning allocation span, or [`Slot::FREE`].
    slots: Vec<Slot>,
    /// Number of free blocks in `slots` (maintained under [`ARENAS`]).
    free: usize,
}

static ARENAS: Mutex<Vec<Arena>> = Mutex::new(Vec::new());
/// Serializes slow-path arena growth so simultaneous cold-start misses share
/// one newly committed 4 MiB arena instead of each reserving its own, while
/// keeping `VirtualAlloc2`, `shadow::register`, and `write_bytes` outside
/// `ARENAS`.
static ARENA_GROW: Mutex<()> = Mutex::new(());

/// Upload memory of one request, zeroed, given back when dropped.
pub(crate) struct Upload(Kind);

enum Kind {
    /// Address, bytes.
    Low(usize, usize),
    Heap(Vec<u8>),
}

fn low() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_UPLOAD_LOW").is_none_or(|v| v != "0"))
}

/// The first place of `n` free blocks side by side.
fn gap(slots: &[Slot], n: usize) -> Option<usize> {
    let mut free = 0;
    for (i, slot) in slots.iter().enumerate() {
        free = if slot.is_free() { free + 1 } else { 0 };
        if free == n {
            return Some(i + 1 - n);
        }
    }
    None
}

/// Fresh zeroed memory in [4 GiB, 2^37) (where the directory of memory by
/// address reaches, `MEMORY_PAGES`), write-watched as the title's own
/// pools are (copies then take only written pages).
pub(crate) fn reserve(len: usize) -> Option<usize> {
    use windows_sys::Win32::System::Memory::{
        MEM_ADDRESS_REQUIREMENTS, MEM_COMMIT, MEM_EXTENDED_PARAMETER, MEM_RESERVE, MemExtendedParameterAddressRequirements,
        PAGE_READWRITE, VirtualAlloc2,
    };
    const MEM_WRITE_WATCH: u32 = 0x20_0000;
    let mut requirements = MEM_ADDRESS_REQUIREMENTS {
        LowestStartingAddress: (1usize << 32) as *mut _,
        HighestEndingAddress: ((1usize << 37) - 1) as *mut _,
        Alignment: 0,
    };
    let mut parameter = MEM_EXTENDED_PARAMETER::default();
    parameter.Anonymous1._bitfield = MemExtendedParameterAddressRequirements as u64;
    parameter.Anonymous2.Pointer = (&raw mut requirements).cast();
    // SAFETY: a fresh allocation; the parameter points at `requirements`,
    // which outlives the call.
    let at = unsafe {
        VirtualAlloc2(
            std::ptr::null_mut(),
            std::ptr::null(),
            len,
            MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH,
            PAGE_READWRITE,
            &mut parameter,
            1,
        )
    } as usize;
    (at != 0).then_some(at)
}

/// Reserves `n` contiguous free blocks from an existing arena under `ARENAS`.
fn try_take(arenas: &mut [Arena], n: usize, count: u32) -> Option<usize> {
    arenas.iter_mut().find_map(|arena| {
        if arena.free < n {
            return None;
        }
        let first = gap(&arena.slots, n)?;
        let span = Slot {
            first: u32::try_from(first).ok()?,
            count,
        };
        arena.slots[first..first + n].fill(span);
        arena.free -= n;
        Some(arena.at + first * BLOCK)
    })
}

impl Upload {
    pub(crate) fn new(len: usize) -> Option<Upload> {
        if !low() {
            return Some(Upload(Kind::Heap(vec![0; len])));
        }
        let n = len.max(1).div_ceil(BLOCK);
        let count = u32::try_from(n).ok()?;
        let reused = {
            let mut arenas = ARENAS.lock().unwrap_or_else(|e| e.into_inner());
            try_take(&mut arenas, n, count)
        };
        if let Some(at) = reused {
            // Reused blocks are exclusively owned now; zero outside `ARENAS` so
            // concurrent recording threads do not stall behind `write_bytes`.
            // SAFETY: the blocks are ours and nobody else holds them.
            unsafe { std::ptr::write_bytes(at as *mut u8, 0, len) };
            return Some(Upload(Kind::Low(at, len)));
        }
        // Serialize slow-path growth so simultaneous misses share one new arena,
        // while still keeping `VirtualAlloc2` and `shadow::register` outside `ARENAS`.
        let _grow = ARENA_GROW.lock().unwrap_or_else(|e| e.into_inner());
        let reused_after_wait = {
            let mut arenas = ARENAS.lock().unwrap_or_else(|e| e.into_inner());
            try_take(&mut arenas, n, count)
        };
        if let Some(at) = reused_after_wait {
            drop(_grow);
            // SAFETY: the blocks are ours and nobody else holds them.
            unsafe { std::ptr::write_bytes(at as *mut u8, 0, len) };
            return Some(Upload(Kind::Low(at, len)));
        }
        let blocks = n.max(ARENA / BLOCK);
        let bytes = blocks.checked_mul(BLOCK)?;
        let at = reserve(bytes)?;
        crate::shadow::register(at, at as u64, bytes as u64);
        tracing::info!(at = format_args!("{at:#x}"), bytes, "memory for lists' uploads, where shaders reach it by address");
        let mut slots = vec![Slot::FREE; blocks];
        slots[..n].fill(Slot { first: 0, count });
        let arena = Arena {
            at,
            slots,
            free: blocks - n,
        };
        ARENAS.lock().unwrap_or_else(|e| e.into_inner()).push(arena);
        drop(_grow);
        // Fresh `VirtualAlloc2(MEM_COMMIT)` pages are already zero-filled; skip
        // `write_bytes` so untouched pages are not prematurely dirtied in WriteWatch.
        Some(Upload(Kind::Low(at, len)))
    }

    pub(crate) fn start(&mut self) -> *mut u8 {
        match &mut self.0 {
            Kind::Low(at, _) => *at as *mut u8,
            Kind::Heap(block) => block.as_mut_ptr(),
        }
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        let Kind::Low(at, len) = self.0 else { return };
        let n = len.max(1).div_ceil(BLOCK);
        let mut arenas = ARENAS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(arena) = arenas.iter_mut().find(|a| (a.at..a.at + a.slots.len() * BLOCK).contains(&at)) {
            let first = (at - arena.at) / BLOCK;
            arena.slots[first..first + n].fill(Slot::FREE);
            arena.free += n;
        }
    }
}

/// The allocation handed out that holds `address`: its exact start and byte
/// length (whole [`BLOCK`]s), never bleeding into adjacent allocations.
pub(crate) fn block(address: usize) -> Option<(usize, usize)> {
    let arenas = ARENAS.lock().unwrap_or_else(|e| e.into_inner());
    let arena = arenas.iter().find(|a| (a.at..a.at + a.slots.len() * BLOCK).contains(&address))?;
    let slot = arena.slots[(address - arena.at) / BLOCK];
    (!slot.is_free()).then_some((arena.at + slot.first as usize * BLOCK, slot.count as usize * BLOCK))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_lie_below_40_bits_and_are_used_again_zeroed() {
        let mut a = Upload::new(100).unwrap();
        let mut b = Upload::new(BLOCK + 1).unwrap();
        let (pa, pb) = (a.start(), b.start());
        assert!((pa as usize) >= 1 << 32 && (pb as usize) + 2 * BLOCK <= 1 << 40);
        assert!((pa as usize).is_multiple_of(BLOCK) && pa != pb);
        // Fresh `VirtualAlloc2` pages must already read as zeroed.
        // SAFETY: `a` and `b` hold 100 and `BLOCK + 1` bytes respectively.
        assert!(unsafe { std::slice::from_raw_parts(pa, 100) }.iter().all(|&x| x == 0));
        assert!(unsafe { std::slice::from_raw_parts(pb, BLOCK + 1) }.iter().all(|&x| x == 0));
        // SAFETY: 100 bytes were handed out.
        unsafe { pa.write_bytes(0xab, 100) };
        drop(a);
        // The lowest free place (another test may have taken it meanwhile;
        // zeroed either way).
        let mut c = Upload::new(100).unwrap();
        // SAFETY: as above.
        assert!(unsafe { std::slice::from_raw_parts(c.start(), 100) }.iter().all(|&x| x == 0));
        // More than an arena.
        let mut d = Upload::new(ARENA + 1).unwrap();
        // SAFETY: the bytes were handed out.
        unsafe { d.start().add(ARENA).write(1) };
        drop(b);
    }

    #[test]
    fn block_returns_exact_allocation_bounds_without_bleeding_into_neighbors() {
        let mut a = Upload::new(128).unwrap();
        let mut b = Upload::new(BLOCK + 128).unwrap();
        let mut c = Upload::new(128).unwrap();
        let (pa, pb, pc) = (a.start() as usize, b.start() as usize, c.start() as usize);

        assert_eq!(block(pa), Some((pa, BLOCK)));
        assert_eq!(block(pa + 64), Some((pa, BLOCK)));
        // Querying either the first or second block of `b` returns `b`'s exact span.
        assert_eq!(block(pb), Some((pb, 2 * BLOCK)));
        assert_eq!(block(pb + BLOCK + 32), Some((pb, 2 * BLOCK)));
        assert_eq!(block(pc), Some((pc, BLOCK)));

        drop(b);
        assert_eq!(block(pb), None);
        assert_eq!(block(pb + BLOCK), None);
        // Dropping `b` leaves neighbors intact.
        assert_eq!(block(pa), Some((pa, BLOCK)));
        assert_eq!(block(pc), Some((pc, BLOCK)));
    }

    #[test]
    fn a_gap_is_blocks_side_by_side() {
        let used = Slot { first: 0, count: 1 };
        let free = Slot::FREE;
        assert_eq!(gap(&[used, free, used, free, free, used], 1), Some(1));
        assert_eq!(gap(&[used, free, used, free, free, used], 2), Some(3));
        assert_eq!(gap(&[used, free, used, free, free, used], 3), None);
    }

    #[test]
    fn cold_parallel_uploads_share_a_single_arena() {
        const CHILD: &str = "VICTORY_TEST_COLD_PARALLEL_UPLOADS";
        const TEST_NAME: &str = "upload::tests::cold_parallel_uploads_share_a_single_arena";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([TEST_NAME, "--exact", "--nocapture", "--test-threads=1"])
                .env(CHILD, "1")
                .env("VICTORY_UPLOAD_LOW", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "cold parallel upload child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        const WORKERS: usize = 16;
        let start = std::sync::Barrier::new(WORKERS);
        let live = std::sync::Barrier::new(WORKERS);
        std::thread::scope(|scope| {
            for _ in 0..WORKERS {
                scope.spawn(|| {
                    start.wait();
                    let upload = Upload::new(128).expect("low upload");
                    live.wait();
                    drop(upload);
                });
            }
        });
        assert_eq!(
            ARENAS.lock().unwrap_or_else(|e| e.into_inner()).len(),
            1,
            "16 simultaneous cold-start uploads must share one 4 MiB arena"
        );
    }
}
