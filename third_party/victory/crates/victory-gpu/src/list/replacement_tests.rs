use super::*;

#[test]
fn replacements_preserve_copies_waits_and_submission_fences() {
    let Some(host) = crate::host::host() else {
        eprintln!("no D3D12 host for the replacement list test");
        return;
    };
    let null = std::ptr::null();
    let (mut allocator, mut a, mut b) = (null, null, null);
    assert_eq!(
        create_allocator(null, 0, &ALLOCATOR_IIDS[0], &mut allocator),
        S_OK
    );
    for list in [&mut a, &mut b] {
        assert_eq!(
            create_list(null, 0, 0, allocator, null, &LIST_IIDS[0], list),
            S_OK
        );
    }
    let (upload, data) = host.upload_buffer(4).unwrap();
    let back = host.read_back_buffer(4).unwrap();
    let mut seen = [0u8; 4];
    let copy = |list, dst, src, len| {
        // SAFETY: open list; both four-byte buffers outlive every submission.
        unsafe {
            host_list(list)
                .unwrap()
                .list
                .CopyBufferRegion(&back, dst, &upload, src, len)
        };
    };
    // SAFETY: the upload has four mapped bytes and no pending readers.
    unsafe { data.cast::<[u8; 4]>().write([11, 22, 33, 44]) };
    copy(a, 0, 0, 2);
    copy(b, 0, 2, 2);
    copy(b, 2, 0, 2);
    host_list(a)
        .unwrap()
        .defer_read_back(back.clone(), seen.as_mut_ptr(), [4, 1, 4]);
    assert_eq!(close(a), S_OK);
    assert_eq!(close(b), S_OK);
    let (taken, order) = detach(&[a, b, a]);
    let leave_open = std::env::var_os("VICTORY_LEAVE_OPEN").is_none_or(|v| v != "0");
    assert_eq!(host_list(a).unwrap().left_open(), leave_open);
    assert_eq!(order, [0, 1, 0]);
    let runs: Vec<_> = order.iter().map(|&i| taken[i].host()).collect();
    let first = host.execute(&runs);
    assert_eq!(seen, [11, 22, 11, 22]);
    recycle(taken, &order);

    // B goes straight back to the card twice without a reset. A takes its
    // title reset instead, then records new work behind a value wait.
    let empty = host_list(b).unwrap();
    let second = host.execute(&[empty, empty]);
    assert!(second > first);
    assert_eq!(reset(a, allocator, null), S_OK);
    let ready = 1u32;
    cut(
        a,
        ValueWait {
            address: (&raw const ready) as u64,
            comparison: 3,
            reference: 1,
            wide: false,
        },
    );
    copy(a, 1, 2, 2);
    host_list(a)
        .unwrap()
        .defer_read_back(back.clone(), seen.as_mut_ptr(), [4, 1, 4]);
    assert_eq!(close(a), S_OK);
    let mut parts = detach_parts(&[a, b]);
    assert_eq!(parts.len(), 1);
    let (waits, taken, order) = parts.pop().unwrap();
    assert_eq!(waits.len(), 1);
    assert!(waits[0].met());
    assert_eq!(order, [0, 1]);
    let runs: Vec<_> = order.iter().map(|&i| taken[i].host()).collect();
    assert!(host.execute(&runs) > second);
    assert_eq!(seen, [11, 33, 44, 22]);

    // Both detach paths must install an open replacement with the switch on.
    for list in [a, b] {
        assert_eq!(host_list(list).unwrap().left_open(), leave_open);
        assert_eq!(list_release(list), 0);
    }
    assert_eq!(allocator_release(allocator), 0);
}
