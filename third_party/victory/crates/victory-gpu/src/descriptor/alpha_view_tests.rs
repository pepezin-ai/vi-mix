use super::*;
use crate::object::{S_OK, This, guid, slot};
use windows::Win32::Graphics::Direct3D::{Fxc::D3DCompile, ID3DBlob};
use windows::core::Interface;

const PROBE: &str = r#"
    Texture2D<float4> alpha : register(t0);
    Texture2D<float4> crossView : register(t1);
    RWByteAddressBuffer result : register(u0);
    [RootSignature("DescriptorTable(SRV(t0, numDescriptors=2)), UAV(u0)")]
    [numthreads(1, 1, 1)]
    void main() {
        int3 texel = int3(0, 0, 0);
        result.Store2(0, asuint(float2(alpha.Load(texel).a, crossView.Load(texel).a)));
    }
"#;

struct AlphaTexture(This);

impl AlphaTexture {
    fn new() -> Self {
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
            Format: DXGI_FORMAT_A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            ..Default::default()
        };
        let id = ID3D12Resource::IID;
        let iid = guid(id.data1, id.data2, id.data3, id.data4);
        let null = std::ptr::null();
        let mut texture = null;
        // The title's heap and resource descriptions have the PC layout.
        assert_eq!(
            crate::resource::create_committed(
                null,
                (&raw const heap).cast(),
                0,
                (&raw const desc).cast(),
                D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32,
                null.cast(),
                &iid,
                &mut texture,
            ),
            S_OK
        );
        Self(texture)
    }

    fn srv(&self, format: DXGI_FORMAT) -> View {
        let mut words = [0; 10];
        words[0] = format.0 as u32;
        words[1] = D3D12_SRV_DIMENSION_TEXTURE2D.0 as u32;
        words[2] = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        words[5] = 1; // mip count
        View::Shader(self.0 as usize, Some(words))
    }
}

impl Drop for AlphaTexture {
    fn drop(&mut self) {
        slot::<extern "system" fn(This) -> u32>(self.0, 0x10)(self.0);
    }
}

#[test]
fn red_view_on_alpha_texture_reads_zero() {
    let Some(host) = host() else {
        eprintln!("no host D3D12 adapter; skipping A8 view readback");
        return;
    };
    let adapter = crate::host::adapter_summary()
        .expect("identified test adapter")
        .0;
    let title = AlphaTexture::new();
    let texture = crate::resource::host_resource(title.0).expect("host A8 texture");
    // SAFETY: a live resource.
    assert_eq!(unsafe { texture.GetDesc() }.Format, DXGI_FORMAT_A8_UNORM);

    // The baseline's accepted view is coerced to A8 and safe to submit.
    // Stop before the native call if the materializer would emit a cast.
    let mut resolved = DXGI_FORMAT_R8_UNORM;
    let accepted = fits(texture, Some(&mut resolved), false);
    if accepted {
        assert_eq!(resolved, DXGI_FORMAT_A8_UNORM);
    }

    const ALPHA: f32 = 64.0 / 255.0;
    let list = host.new_list().unwrap();
    let result = host.uav_buffer(8).unwrap();
    let readback = host.read_back_buffer(8).unwrap();
    let device = &host.device;
    // SAFETY: a live texture, correctly sized heaps, an open list and a
    // single shader thread reading the two initialized slots.
    let resources = unsafe {
        let rtv_heap: ID3D12DescriptorHeap = device
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })
            .unwrap();
        let rtv = rtv_heap.GetCPUDescriptorHandleForHeapStart();
        device.CreateRenderTargetView(texture, None, rtv);
        list.list
            .ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, ALPHA], None);
        list.list.ResourceBarrier(&[crate::host::transition(
            texture,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        )]);

        let srv_heap: ID3D12DescriptorHeap = device
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 2,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })
            .unwrap();
        let cpu = srv_heap.GetCPUDescriptorHandleForHeapStart();
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        let compatible = make(&title.srv(DXGI_FORMAT_A8_UNORM), cpu).expect("valid alpha view");
        let cross = title.srv(DXGI_FORMAT_R8_UNORM);
        let cross_cpu = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: cpu.ptr + step as usize,
        };
        let incompatible = make(&cross, cross_cpu);
        assert_eq!(incompatible.is_some(), accepted);
        if incompatible.is_none() {
            null_for(&cross, cross_cpu);
        }

        let mut shader: Option<ID3DBlob> = None;
        D3DCompile(
            PROBE.as_ptr().cast(),
            PROBE.len(),
            windows::core::s!("synthetic alpha view"),
            None,
            None,
            windows::core::s!("main"),
            windows::core::s!("cs_5_1"),
            0,
            0,
            &mut shader,
            None,
        )
        .unwrap();
        let shader = shader.unwrap();
        let code = std::slice::from_raw_parts(
            shader.GetBufferPointer().cast::<u8>(),
            shader.GetBufferSize(),
        );
        let root: ID3D12RootSignature = device.CreateRootSignature(0, code).unwrap();
        let pso: ID3D12PipelineState = device
            .CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: code.as_ptr().cast(),
                    BytecodeLength: code.len(),
                },
                ..Default::default()
            })
            .unwrap();
        list.list.SetDescriptorHeaps(&[Some(srv_heap.clone())]);
        list.list.SetComputeRootSignature(&root);
        list.list.SetPipelineState(&pso);
        list.list
            .SetComputeRootDescriptorTable(0, srv_heap.GetGPUDescriptorHandleForHeapStart());
        list.list
            .SetComputeRootUnorderedAccessView(1, result.GetGPUVirtualAddress());
        list.list.Dispatch(1, 1, 1);
        list.list.ResourceBarrier(&[crate::host::transition(
            &result,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        )]);
        list.list.CopyBufferRegion(&readback, 0, &result, 0, 8);
        (rtv_heap, srv_heap, compatible, incompatible, root, pso)
    };
    list.close();
    host.execute(&[&list]);
    let mut data = std::ptr::null_mut();
    // SAFETY: the copy completed and readback contains two f32 values.
    let [control, cross] = unsafe {
        device.GetDeviceRemovedReason().unwrap();
        readback.Map(0, None, Some(&mut data)).unwrap();
        let sampled = data.cast::<[f32; 2]>().read();
        readback.Unmap(0, Some(&D3D12_RANGE::default()));
        sampled
    };
    drop(resources);
    println!(
        "adapter={adapter}, R8 on A8 accepted={accepted}, resolved format={}, alpha={control}, cross={cross}",
        resolved.0,
    );
    assert!((control - ALPHA).abs() < 0.001, "valid alpha readback");
    assert_eq!(cross, 0.0, "an incompatible red view must read zero");
    assert!(!accepted);
}
