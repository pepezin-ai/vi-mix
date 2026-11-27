use super::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_POINTLIST, Fxc::D3DCompile};

const PROBE: &str = r#"
    RWByteAddressBuffer readback : register(u0);
    [RootSignature("RootFlags(ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT), UAV(u0, visibility=SHADER_VISIBILITY_VERTEX)")]
    float4 main(uint2 tag : TEXCOORD0, float2 corner : TEXCOORD1) : SV_Position {
        readback.Store3(0, uint3(asuint(corner), tag.x));
        return float4(0, 0, 0, 1);
    }
"#;

fn element(slot: u32, index: u32, format: DXGI_FORMAT, offset: u32) -> D3D12_INPUT_ELEMENT_DESC {
    D3D12_INPUT_ELEMENT_DESC {
        SemanticName: windows::core::s!("TEXCOORD"),
        SemanticIndex: index,
        Format: format,
        InputSlot: slot,
        AlignedByteOffset: offset,
        InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        InstanceDataStepRate: 0,
    }
}

#[test]
fn appended_offsets_follow_their_own_input_slots() {
    const APPEND: u32 = D3D12_APPEND_ALIGNED_ELEMENT;
    let mut graphics = GraphicsState {
        inputs: vec![
            element(0, 0, DXGI_FORMAT_R8_UINT, 0),
            element(1, 1, DXGI_FORMAT_R8G8_UINT, 6),
            element(0, 2, DXGI_FORMAT_R16_FLOAT, APPEND),
            element(0, 3, DXGI_FORMAT_R8_UINT, APPEND),
            element(1, 4, DXGI_FORMAT_R32G32_FLOAT, APPEND),
            element(0, 5, DXGI_FORMAT_R32G32_FLOAT, APPEND),
        ],
        ..Default::default()
    };
    let offsets: Vec<_> = graphics.elements().iter().map(|e| e.offset).collect();
    assert_eq!(offsets, [0, 6, 2, 4, 8, 8]);

    // Exercise the offset arithmetic without passing an invalid layout to D3D12.
    graphics.inputs = vec![
        element(0, 0, DXGI_FORMAT_R32G32B32A32_FLOAT, u32::MAX - 15),
        element(0, 1, DXGI_FORMAT_R32_FLOAT, APPEND),
    ];
    let offsets: Vec<_> = graphics.elements().iter().map(|e| e.offset).collect();
    assert_eq!(offsets, [u32::MAX - 15, u32::MAX]);
}

#[test]
fn appended_rectangle_corner_reaches_the_vertex_shader() {
    let mut graphics = GraphicsState {
        inputs: vec![
            element(0, 0, DXGI_FORMAT_R8G8_UINT, 0),
            element(0, 1, DXGI_FORMAT_R32G32_FLOAT, D3D12_APPEND_ALIGNED_ELEMENT),
        ],
        ..Default::default()
    };
    let elements: Vec<_> = graphics
        .elements()
        .iter()
        .map(|e| (e.offset, e.format))
        .collect();

    const STRIDE: usize = 12;
    const TAG: u8 = 17;
    let mut src = [0u8; 3 * STRIDE];
    let mut expanded = [0u8; 6 * STRIDE];
    // One tilted rectangle, with a 2-byte tag, 2 bytes of padding, then
    // two floats. The last vertex is produced by Neptune's CPU fallback.
    let corners = [[0.25f32, 0.75], [0.5, 0.25], [1.0, 0.5]];
    for (bytes, [x, y]) in src.chunks_exact_mut(STRIDE).zip(corners) {
        bytes[0] = TAG;
        bytes[4..8].copy_from_slice(&x.to_le_bytes());
        bytes[8..12].copy_from_slice(&y.to_le_bytes());
    }
    crate::primitive::expand_rects(&src, &mut expanded, STRIDE, &elements);
    let corner = &expanded[5 * STRIDE..];
    let expected = [0.75f32.to_bits(), 1.0f32.to_bits(), u32::from(TAG)];

    if let Some(host) = host() {
        let (vertices, mapped) = host.upload_buffer(STRIDE as u64).unwrap();
        let written = host.uav_buffer(12).unwrap();
        let readback = host.read_back_buffer(12).unwrap();
        let list = host.new_list().unwrap();

        graphics.depth.DepthEnable = false.into();
        graphics.depth.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ZERO;

        // SAFETY: live buffers, an open command list and one in-bounds point.
        let (root, pipeline) = unsafe {
            std::ptr::copy_nonoverlapping(corner.as_ptr(), mapped, corner.len());
            vertices.Unmap(0, None);

            let mut shader: Option<ID3DBlob> = None;
            D3DCompile(
                PROBE.as_ptr().cast(),
                PROBE.len(),
                windows::core::s!("synthetic rectangle corner"),
                None,
                None,
                windows::core::s!("main"),
                windows::core::s!("vs_5_1"),
                0,
                0,
                &mut shader,
                None,
            )
            .unwrap();
            let shader = shader.unwrap();
            let bytecode = std::slice::from_raw_parts(
                shader.GetBufferPointer().cast::<u8>(),
                shader.GetBufferSize(),
            );
            let root: ID3D12RootSignature = host.device.CreateRootSignature(0, bytecode).unwrap();
            let pipeline: ID3D12PipelineState = host
                .device
                .CreateGraphicsPipelineState(&D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                    pRootSignature: std::mem::transmute_copy(&root),
                    VS: D3D12_SHADER_BYTECODE {
                        pShaderBytecode: bytecode.as_ptr().cast(),
                        BytecodeLength: bytecode.len(),
                    },
                    BlendState: graphics.blend,
                    SampleMask: graphics.sample_mask,
                    RasterizerState: graphics.rasterizer,
                    DepthStencilState: graphics.depth,
                    InputLayout: D3D12_INPUT_LAYOUT_DESC {
                        pInputElementDescs: graphics.inputs.as_ptr(),
                        NumElements: graphics.inputs.len() as u32,
                    },
                    PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT,
                    SampleDesc: graphics.samples,
                    ..Default::default()
                })
                .unwrap();
            list.list.SetGraphicsRootSignature(&root);
            list.list.SetPipelineState(&pipeline);
            list.list
                .SetGraphicsRootUnorderedAccessView(0, written.GetGPUVirtualAddress());
            list.list
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
            list.list.IASetVertexBuffers(
                0,
                Some(&[D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: vertices.GetGPUVirtualAddress(),
                    SizeInBytes: STRIDE as u32,
                    StrideInBytes: STRIDE as u32,
                }]),
            );
            list.list.RSSetViewports(&[D3D12_VIEWPORT {
                Width: 1.0,
                Height: 1.0,
                MaxDepth: 1.0,
                ..Default::default()
            }]);
            list.list.RSSetScissorRects(&[RECT {
                right: 1,
                bottom: 1,
                ..Default::default()
            }]);
            list.list.DrawInstanced(1, 1, 0, 0);
            list.list.ResourceBarrier(&[crate::host::transition(
                &written,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.list.CopyBufferRegion(&readback, 0, &written, 0, 12);
            (root, pipeline)
        };
        list.close();
        host.execute(&[&list]);
        let mut data = std::ptr::null_mut();
        // SAFETY: the GPU copy has finished and readback holds three dwords.
        let got = unsafe {
            host.device.GetDeviceRemovedReason().unwrap();
            readback.Map(0, None, Some(&mut data)).unwrap();
            let got = data.cast::<[u32; 3]>().read();
            readback.Unmap(0, Some(&D3D12_RANGE::default()));
            got
        };
        drop((pipeline, root));
        println!(
            "expanded offset={}, shader readback={got:08x?}, expected={expected:08x?}",
            elements[1].0
        );
        assert_eq!(got, expected, "native vertex fetch of the expanded corner");
    } else {
        eprintln!("no host D3D12 adapter; skipping rectangle GPU readback");
    }

    assert_eq!(elements[1].0, 4);
    assert_eq!(corner[4..12], [0, 0, 0x40, 0x3f, 0, 0, 0x80, 0x3f]);
}
