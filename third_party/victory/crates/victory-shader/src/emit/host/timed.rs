//! A reusable queue and raw-buffer bindings for the synthetic RT fixture.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, WaitForSingleObject};

use super::*;

const STAMPS: u32 = 3;

pub(in crate::emit) struct FrameTimes {
    pub frame: Duration,
    pub cpu_wait: Duration,
    pub gpu_update_ms: Option<f64>,
    pub gpu_dispatch_ms: f64,
    pub gpu_total_ms: f64,
}

pub(in crate::emit) struct BufferUpdate<'a> {
    pub source: &'a ID3D12Resource,
    pub offset: u64,
    pub bytes: u64,
}

/// Submits one frame at a time. The queue, PSO, heaps, and buffers are reused.
pub(in crate::emit) struct TimedDispatch {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    fence: ID3D12Fence,
    completion: OwnedHandle,
    submitted: u64,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    views: [Option<ID3D12DescriptorHeap>; 1],
    input: ID3D12Resource,
    output: ID3D12Resource,
    output_readback: ID3D12Resource,
    output_words: u32,
    stamps: ID3D12QueryHeap,
    stamp_readback: ID3D12Resource,
    frequency: f64,
}

impl TimedDispatch {
    pub fn new(
        host: &Host,
        shader: &HostShader,
        input: &[u32],
        output_words: u32,
    ) -> Result<Self, String> {
        let device = host.device.clone();
        let (root, pso) = host.compute_pipeline(shader)?;
        let input_buffer = buffer(
            &device,
            size_of_val(input) as u64,
            D3D12_HEAP_TYPE_DEFAULT,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_FLAG_NONE,
        )?;
        let output_bytes = u64::from(output_words) * 4;
        let output = buffer(
            &device,
            output_bytes,
            D3D12_HEAP_TYPE_DEFAULT,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        )?;
        let output_readback = readback(&device, output_bytes)?;
        let stamp_readback = readback(&device, u64::from(STAMPS) * 8)?;
        // SAFETY: descriptors use live buffers, and the owned event is kept with the fence.
        unsafe {
            let queue: ID3D12CommandQueue = device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    ..Default::default()
                })
                .map_err(err)?;
            let allocator: ID3D12CommandAllocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(err)?;
            let list: ID3D12GraphicsCommandList = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
                .map_err(err)?;
            let fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(err)?;
            let completion = OwnedHandle::from_raw_handle(
                CreateEventW(None, false, false, None).map_err(err)?.0,
            );
            let mut stamps: Option<ID3D12QueryHeap> = None;
            device
                .CreateQueryHeap(
                    &D3D12_QUERY_HEAP_DESC {
                        Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
                        Count: STAMPS,
                        NodeMask: 0,
                    },
                    &mut stamps,
                )
                .map_err(err)?;
            let frequency = queue.GetTimestampFrequency().map_err(err)? as f64;
            if frequency == 0.0 {
                return Err("the queue reported zero timestamp frequency".to_owned());
            }
            let views = raw_views(
                &device,
                &input_buffer,
                input.len() as u32,
                &output,
                output_words,
            )?;
            let mut dispatch = Self {
                device,
                queue,
                allocator,
                list,
                fence,
                completion,
                submitted: 0,
                root,
                pso,
                views: [Some(views)],
                input: input_buffer,
                output,
                output_readback,
                output_words,
                stamps: stamps.ok_or("no timestamp query heap")?,
                stamp_readback,
                frequency,
            };
            let initial = dispatch.stage(input)?;
            dispatch.list.CopyBufferRegion(
                &dispatch.input,
                0,
                &initial,
                0,
                size_of_val(input) as u64,
            );
            dispatch.submit()?;
            Ok(dispatch)
        }
    }

    pub fn stage(&self, words: &[u32]) -> Result<ID3D12Resource, String> {
        let upload = buffer(
            &self.device,
            size_of_val(words) as u64,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            D3D12_RESOURCE_FLAG_NONE,
        )?;
        // SAFETY: the new upload buffer has room for every word and has no GPU users yet.
        unsafe {
            let mut ptr = std::ptr::null_mut();
            upload
                .Map(0, Some(&D3D12_RANGE::default()), Some(&mut ptr))
                .map_err(err)?;
            std::ptr::copy_nonoverlapping(words.as_ptr(), ptr.cast::<u32>(), words.len());
            upload.Unmap(0, None);
        }
        Ok(upload)
    }

    /// `rt` is user constant s0. Capture is used in untimed validation frames.
    pub fn frame(
        &mut self,
        groups_x: u32,
        rt: bool,
        update: Option<BufferUpdate<'_>>,
        capture: bool,
    ) -> Result<FrameTimes, String> {
        let start = Instant::now();
        // SAFETY: submit waits before this list, allocator or query storage is reused.
        // Buffers decay to COMMON after execution and promote on access in the next list.
        unsafe {
            self.allocator.Reset().map_err(err)?;
            self.list.Reset(&self.allocator, &self.pso).map_err(err)?;
            self.list.SetDescriptorHeaps(&self.views);
            self.list.SetComputeRootSignature(&self.root);
            let constants = [u32::from(rt), 0, 0, 0, 0];
            self.list.SetComputeRoot32BitConstants(
                0,
                constants.len() as u32,
                constants.as_ptr().cast(),
                0,
            );
            self.list
                .EndQuery(&self.stamps, D3D12_QUERY_TYPE_TIMESTAMP, 0);
            let updated = update.is_some();
            if let Some(update) = update {
                self.list.CopyBufferRegion(
                    &self.input,
                    update.offset,
                    update.source,
                    0,
                    update.bytes,
                );
                self.list.ResourceBarrier(&[transition(
                    &self.input,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                )]);
            }
            self.list
                .EndQuery(&self.stamps, D3D12_QUERY_TYPE_TIMESTAMP, 1);
            self.list.Dispatch(groups_x, 1, 1);
            self.list
                .EndQuery(&self.stamps, D3D12_QUERY_TYPE_TIMESTAMP, 2);
            self.list.ResolveQueryData(
                &self.stamps,
                D3D12_QUERY_TYPE_TIMESTAMP,
                0,
                STAMPS,
                &self.stamp_readback,
                0,
            );
            if capture {
                self.list.ResourceBarrier(&[transition(
                    &self.output,
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                )]);
                self.list.CopyBufferRegion(
                    &self.output_readback,
                    0,
                    &self.output,
                    0,
                    u64::from(self.output_words) * 4,
                );
            }
            let cpu_wait = self.submit()?;
            let frame = start.elapsed();
            let [before, dispatch, end] =
                read_words::<u64, _>(&self.stamp_readback, STAMPS as usize, |s| {
                    [s[0], s[1], s[2]]
                })?;
            if !(before <= dispatch && dispatch <= end) {
                return Err(format!(
                    "nonmonotonic GPU timestamps: {before}, {dispatch}, {end}"
                ));
            }
            let ms = |ticks: u64| ticks as f64 * 1000.0 / self.frequency;
            Ok(FrameTimes {
                frame,
                cpu_wait,
                gpu_update_ms: updated.then(|| ms(dispatch - before)),
                gpu_dispatch_ms: ms(end - dispatch),
                gpu_total_ms: ms(end - before),
            })
        }
    }

    pub fn captured(&self) -> Result<Vec<u32>, String> {
        read_words(
            &self.output_readback,
            self.output_words as usize,
            <[u32]>::to_vec,
        )
    }

    fn submit(&mut self) -> Result<Duration, String> {
        // SAFETY: the list and its resources are kept through completion. The event is auto-reset.
        unsafe {
            self.list.Close().map_err(err)?;
            self.queue
                .ExecuteCommandLists(&[Some(self.list.cast().map_err(err)?)]);
            self.submitted += 1;
            self.queue
                .Signal(&self.fence, self.submitted)
                .map_err(err)?;
            let start = Instant::now();
            let event = HANDLE(self.completion.as_raw_handle());
            self.fence
                .SetEventOnCompletion(self.submitted, event)
                .map_err(err)?;
            let result = WaitForSingleObject(event, 15_000);
            let elapsed = start.elapsed();
            self.device.GetDeviceRemovedReason().map_err(err)?;
            if result != WAIT_OBJECT_0 {
                return Err(format!("GPU fence wait failed: {:#x}", result.0));
            }
            Ok(elapsed)
        }
    }
}

fn buffer(
    device: &ID3D12Device,
    bytes: u64,
    heap: D3D12_HEAP_TYPE,
    state: D3D12_RESOURCE_STATES,
    flags: D3D12_RESOURCE_FLAGS,
) -> Result<ID3D12Resource, String> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: flags,
        ..Default::default()
    };
    let mut resource = None;
    // SAFETY: a buffer description with a valid state for this heap type.
    unsafe {
        device
            .CreateCommittedResource(
                &D3D12_HEAP_PROPERTIES {
                    Type: heap,
                    ..Default::default()
                },
                D3D12_HEAP_FLAG_NONE,
                &desc,
                state,
                None,
                &mut resource,
            )
            .map_err(err)?;
    }
    resource.ok_or_else(|| "no buffer".to_owned())
}

fn readback(device: &ID3D12Device, bytes: u64) -> Result<ID3D12Resource, String> {
    buffer(
        device,
        bytes,
        D3D12_HEAP_TYPE_READBACK,
        D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_FLAG_NONE,
    )
}

fn raw_views(
    device: &ID3D12Device,
    input: &ID3D12Resource,
    input_words: u32,
    output: &ID3D12Resource,
    output_words: u32,
) -> Result<ID3D12DescriptorHeap, String> {
    // SAFETY: both resources are live and the dword ranges fit their allocations.
    unsafe {
        let heap: ID3D12DescriptorHeap = device
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 2,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })
            .map_err(err)?;
        let mut slot = heap.GetCPUDescriptorHandleForHeapStart();
        device.CreateShaderResourceView(
            input,
            Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Buffer: D3D12_BUFFER_SRV {
                        NumElements: input_words,
                        Flags: D3D12_BUFFER_SRV_FLAG_RAW,
                        ..Default::default()
                    },
                },
            }),
            slot,
        );
        slot.ptr += device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
            as usize;
        device.CreateUnorderedAccessView(
            output,
            None,
            Some(&D3D12_UNORDERED_ACCESS_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
                Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                    Buffer: D3D12_BUFFER_UAV {
                        NumElements: output_words,
                        Flags: D3D12_BUFFER_UAV_FLAG_RAW,
                        ..Default::default()
                    },
                },
            }),
            slot,
        );
        Ok(heap)
    }
}

fn read_words<T: Copy, R>(
    resource: &ID3D12Resource,
    words: usize,
    read: impl FnOnce(&[T]) -> R,
) -> Result<R, String> {
    // SAFETY: the callers wait for the GPU and only read initialized u32 or u64 words that fit.
    unsafe {
        let mut ptr = std::ptr::null_mut();
        resource
            .Map(
                0,
                Some(&D3D12_RANGE {
                    Begin: 0,
                    End: words * size_of::<T>(),
                }),
                Some(&mut ptr),
            )
            .map_err(err)?;
        let result = read(std::slice::from_raw_parts(ptr.cast::<T>(), words));
        resource.Unmap(0, Some(&D3D12_RANGE::default()));
        Ok(result)
    }
}

pub(in crate::emit) struct MemoryPeak {
    adapter: Option<IDXGIAdapter3>,
    pub local: Option<u64>,
    pub nonlocal: Option<u64>,
}

impl MemoryPeak {
    pub fn new(host: &Host) -> Self {
        // SAFETY: the device and DXGI factory are live for the adapter lookup.
        let adapter = unsafe {
            CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0))
                .and_then(|f| f.EnumAdapterByLuid(host.device.GetAdapterLuid()))
                .ok()
        };
        Self {
            adapter,
            local: None,
            nonlocal: None,
        }
    }

    pub fn sample(&mut self) {
        let Some(adapter) = &self.adapter else { return };
        for (segment, peak) in [
            (DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut self.local),
            (DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut self.nonlocal),
        ] {
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            // SAFETY: query node 0 of this adapter and write into the correctly sized structure.
            if unsafe { adapter.QueryVideoMemoryInfo(0, segment, &mut info) }.is_ok() {
                *peak = (*peak).max(Some(info.CurrentUsage));
            }
        }
    }
}

pub(in crate::emit) fn process_peak_bytes() -> Option<usize> {
    let mut memory = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: the process handle is pseudo-owned by Windows and the output is correctly sized.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut memory, memory.cb) }.ok()?;
    Some(memory.PeakWorkingSetSize)
}
