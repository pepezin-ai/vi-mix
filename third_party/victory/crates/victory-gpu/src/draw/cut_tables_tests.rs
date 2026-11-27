//! A host list cut onto a new one must not reuse the earlier part's tables
//! (`State::replay`, ported from PR #433 by mauroalvare).

use std::ptr::null;

use windows::core::Interface;

use super::*;
use crate::descriptor::{create_cbv, create_heap, heap};
use crate::host::host;
use crate::object::{S_OK, guid, slot};

struct TitleHeap(This);

impl TitleHeap {
    fn new(slots: u32) -> Self {
        let desc = [
            D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 as u32,
            slots,
            D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE.0 as u32,
            0,
            0,
            0,
            0,
            0,
        ];
        let i = ID3D12DescriptorHeap::IID;
        let iid = guid(i.data1, i.data2, i.data3, i.data4);
        let mut out = null();
        assert_eq!(create_heap(null(), desc.as_ptr().cast(), &iid, &mut out), S_OK);
        Self(out)
    }
}

impl Drop for TitleHeap {
    fn drop(&mut self) {
        slot::<extern "system" fn(This) -> u32>(self.0, 0x10)(self.0);
    }
}

fn cbv_plan() -> TablePlan {
    let used = [UsedResource {
        kind: ResourceType::Cbv,
        root: RootType::Table,
        slot: 0,
        table_offset_dwords: 0,
        lower_bound: 0,
        stride: 0,
        dwords: 4,
        user_data: 0,
    }];
    TablePlan::new(&used, &[])
}

/// Part 2 after a cut makes its own constant buffer table, in scratch of its
/// own host list; a second table request in the same part reuses it.
#[test]
fn cut_list_does_not_reuse_the_earlier_parts_cbv_table() {
    let Some(host) = host() else {
        return; // no host GPU here
    };
    let title_heap = TitleHeap::new(64);
    let table = heap(title_heap.0).handle(0).expect("slot 0 handle");
    let memory = Box::new([0x1111_2222u32; 64]);
    let address = memory.as_ptr() as u64;
    create_cbv(null(), &[address as u32, (address >> 32) as u32, 256, 0], table);

    let plan = cbv_plan();
    let mut state = State::default();
    state.set_heaps(&[title_heap.0]);
    state.set_table(true, 0, table);

    let first = host.new_list().expect("first host list");
    let made1 = state.tables(&first, &plan, true);
    assert_eq!(made1.len(), 1);
    // The same part: the table is reused.
    assert_eq!(state.tables(&first, &plan, true)[0], made1[0]);
    first.close();

    // The cut: a new host list continues the title's list.
    let second = host.new_list().expect("second host list");
    state.replay(&second);
    let made2 = state.tables(&second, &plan, true);
    assert_eq!(made2.len(), 1);
    assert_ne!(
        made2[0].1, made1[0].1,
        "the part after the cut reused a table whose constants live in the earlier part's scratch"
    );
    assert_eq!(state.tables(&second, &plan, true)[0], made2[0]);
    assert_eq!(state.dynamic.len(), 2, "the earlier part's slots stay until the list has run");
    second.close();
    drop(memory);
}
