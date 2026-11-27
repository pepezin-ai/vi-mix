use super::*;
use crate::object::{S_OK, This, guid, slot};
use windows::Win32::Graphics::Direct3D::{Fxc::D3DCompile, ID3DBlob};
use windows::core::Interface;

struct TitleTexture(This);

impl TitleTexture {
    fn new(format: DXGI_FORMAT) -> Self {
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 1,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let id = ID3D12Resource::IID;
        let iid = guid(id.data1, id.data2, id.data3, id.data4);
        let null = std::ptr::null();
        let mut texture = null;
        // The title resource and heap descriptions use the PC layout.
        assert_eq!(
            crate::resource::create_committed(
                null,
                (&raw const heap).cast(),
                0,
                (&raw const desc).cast(),
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32,
                null.cast(),
                &iid,
                &mut texture,
            ),
            S_OK
        );
        Self(texture)
    }

    fn host(&self) -> &ID3D12Resource {
        crate::resource::host_resource(self.0).unwrap()
    }

    fn view(&self, format: DXGI_FORMAT) -> View {
        let mut words = [0; 10];
        words[0] = format.0 as u32;
        words[1] = D3D12_UAV_DIMENSION_TEXTURE2D.0 as u32;
        View::Unordered(self.0 as usize, 0, Some(words))
    }
}

impl Drop for TitleTexture {
    fn drop(&mut self) {
        slot::<extern "system" fn(This) -> u32>(self.0, 0x10)(self.0);
    }
}

const PROBE: &str = r#"
    RWTexture2D<uint> image : register(u0);
    RWByteAddressBuffer results : register(u1);
    cbuffer Probe : register(b0) { uint value; uint offset; }
    [RootSignature("DescriptorTable(UAV(u0)), UAV(u1), RootConstants(num32BitConstants=2, b0)")]
    [numthreads(1, 1, 1)]
    void main() {
        results.Store(offset, image[uint2(0, 0)]);
        image[uint2(0, 0)] = value;
    }
"#;

fn format_support(device: &ID3D12Device, format: DXGI_FORMAT) -> D3D12_FEATURE_DATA_FORMAT_SUPPORT {
    let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
        Format: format,
        ..Default::default()
    };
    // SAFETY: one correctly sized output structure.
    unsafe {
        device
            .CheckFeatureSupport(
                D3D12_FEATURE_FORMAT_SUPPORT,
                (&raw mut support).cast(),
                size_of_val(&support) as u32,
            )
            .unwrap();
    }
    support
}

#[test]
fn a_texture_uav_requires_a_writable_host_format() {
    let Some(host) = host() else {
        eprintln!("skipped texture UAV test: no D3D12 device");
        return;
    };
    let adapter = crate::host::adapter_summary()
        .expect("identified test adapter")
        .0;
    let texture = TitleTexture::new(DXGI_FORMAT_R32_TYPELESS);
    let support = format_support(&host.device, DXGI_FORMAT_D32_FLOAT);
    assert!(
        !support
            .Support2
            .contains(D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE)
    );
    let mut depth = DXGI_FORMAT_D32_FLOAT;
    let accepted = fits(texture.host(), Some(&mut depth), true);
    println!(
        "adapter={adapter}, D32_FLOAT support1={:#x}, support2={:#x}, fits={accepted}",
        support.Support1.0, support.Support2.0,
    );
    // Check eligibility before issuing any CreateUnorderedAccessView call.
    assert!(
        !accepted,
        "the runtime accepted a D32_FLOAT UAV on an R32_TYPELESS texture"
    );

    let typed = TitleTexture::new(DXGI_FORMAT_R32_UINT);
    let mut translated = DXGI_FORMAT_D32_FLOAT;
    assert!(fits(typed.host(), Some(&mut translated), true));
    assert_eq!(translated, DXGI_FORMAT_R32_UINT);
    assert!(fits(typed.host(), None, true));
    let mut inherited = DXGI_FORMAT_UNKNOWN;
    assert!(fits(typed.host(), Some(&mut inherited), true));
    assert_eq!(inherited, DXGI_FORMAT_R32_UINT);
    assert!(!fits(texture.host(), None, true));

    let srgb = TitleTexture::new(DXGI_FORMAT_R8G8B8A8_TYPELESS);
    let mut translated = DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    assert!(fits(srgb.host(), Some(&mut translated), true));
    assert_eq!(translated, DXGI_FORMAT_R8G8B8A8_UNORM);

    let packed = TitleTexture::new(DXGI_FORMAT_B5G5R5A1_UNORM);
    // SAFETY: a live texture.
    let packed_format = unsafe { packed.host().GetDesc() }.Format;
    let packed_support = format_support(&host.device, packed_format);
    let mut translated = DXGI_FORMAT_B5G5R5A1_UNORM;
    let packed_fits = fits(packed.host(), Some(&mut translated), true);
    assert_eq!(
        packed_fits,
        packed_support
            .Support1
            .contains(D3D12_FORMAT_SUPPORT1_TYPED_UNORDERED_ACCESS_VIEW)
            && packed_support
                .Support2
                .contains(D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE)
    );
    if packed_fits {
        assert_eq!(translated, packed_format);
    }
    println!(
        "B5G5R5A1_UNORM resolved to DXGI format {}, fits={packed_fits}",
        packed_format.0
    );

    let result = host.uav_buffer(12).unwrap();
    let readback = host.read_back_buffer(12).unwrap();
    let list = host.new_list().unwrap();
    const MARKER: u32 = 0x1357_9bdf;
    // SAFETY: live resources, bounded shader accesses and a two-slot descriptor heap.
    let (heap, root, pipeline, kept) = unsafe {
        let device = &host.device;
        let heap: ID3D12DescriptorHeap = device
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 2,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })
            .unwrap();
        let cpu = heap.GetCPUDescriptorHandleForHeapStart();
        let gpu = heap.GetGPUDescriptorHandleForHeapStart();
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        let kept = make(&texture.view(DXGI_FORMAT_R32_UINT), cpu).expect("valid UAV slot");
        let bad = texture.view(DXGI_FORMAT_D32_FLOAT);
        let bad_cpu = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: cpu.ptr + step as usize,
        };
        assert!(make(&bad, bad_cpu).is_none());
        null_for(&bad, bad_cpu);

        let mut blob: Option<ID3DBlob> = None;
        D3DCompile(
            PROBE.as_ptr().cast(),
            PROBE.len(),
            windows::core::s!("synthetic texture UAV"),
            None,
            None,
            windows::core::s!("main"),
            windows::core::s!("cs_5_1"),
            0,
            0,
            &mut blob,
            None,
        )
        .unwrap();
        let blob = blob.unwrap();
        let code =
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
        let root: ID3D12RootSignature = device.CreateRootSignature(0, code).unwrap();
        let pipeline: ID3D12PipelineState = device
            .CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: code.as_ptr().cast(),
                    BytecodeLength: code.len(),
                },
                ..Default::default()
            })
            .unwrap();
        list.list.SetDescriptorHeaps(&[Some(heap.clone())]);
        list.list.SetComputeRootSignature(&root);
        list.list.SetPipelineState(&pipeline);
        list.list
            .SetComputeRootUnorderedAccessView(1, result.GetGPUVirtualAddress());
        // Write a marker through the valid slot, attempt a write through the
        // null slot, then read the valid slot back to check it kept the marker.
        let uav_barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            ..Default::default()
        };
        for (slot, value, offset) in [(0u32, MARKER, 0u32), (1, 0xffff_ffff, 4), (0, 0, 8)] {
            let handle = D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: gpu.ptr + u64::from(step) * u64::from(slot),
            };
            let constants = [value, offset];
            list.list.SetComputeRootDescriptorTable(0, handle);
            list.list
                .SetComputeRoot32BitConstants(2, 2, constants.as_ptr().cast(), 0);
            list.list.Dispatch(1, 1, 1);
            list.list
                .ResourceBarrier(std::slice::from_ref(&uav_barrier));
        }
        list.list.ResourceBarrier(&[crate::host::transition(
            &result,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        )]);
        list.list.CopyBufferRegion(&readback, 0, &result, 0, 12);
        (heap, root, pipeline, kept)
    };
    list.close();
    host.execute(&[&list]);
    let mut data = std::ptr::null_mut();
    // SAFETY: the copy has finished and the mapping holds three u32 values.
    let (null_read, marker) = unsafe {
        host.device.GetDeviceRemovedReason().unwrap();
        readback.Map(0, None, Some(&mut data)).unwrap();
        let words = data.cast::<u32>();
        let values = (words.add(1).read(), words.add(2).read());
        readback.Unmap(0, Some(&D3D12_RANGE::default()));
        values
    };
    drop((heap, root, pipeline, kept));
    println!("null UAV read={null_read:#010x}, valid UAV retained={marker:#010x}");
    assert_eq!((null_read, marker), (0, MARKER));
}
