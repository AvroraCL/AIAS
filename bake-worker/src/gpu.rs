use serde::Serialize;
use std::{
    mem::ManuallyDrop,
    time::{Duration, Instant},
};
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::{
    core::Interface,
    Win32::Graphics::{Direct3D::*, Direct3D12::*, Dxgi::*},
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Surface {
    pub position: [f32; 3],
    pub object: u32,
    pub normal: [f32; 3],
    pub pixel: u32,
}
pub struct Gpu {
    adapter: IDXGIAdapter3,
    pub peak_device_bytes: u64,
    device: ID3D12Device5,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList4,
    fence: ID3D12Fence,
    serial: u64,
    root: ID3D12RootSignature,
    pipeline: ID3D12PipelineState,
    tlas: ID3D12Resource,
    objects: ID3D12Resource,
    _geometry: Vec<ID3D12Resource>,
    pub block_size: usize,
}
type GResult<T> = Result<T, String>;
fn err(e: windows::core::Error) -> String {
    format!("GPU/D3D12: {e}")
}
unsafe fn buffer(
    device: &ID3D12Device5,
    size: u64,
    heap: D3D12_HEAP_TYPE,
    state: D3D12_RESOURCE_STATES,
    uav: bool,
) -> GResult<ID3D12Resource> {
    let properties = D3D12_HEAP_PROPERTIES {
        Type: heap,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size.max(256),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: if uav {
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
        } else {
            D3D12_RESOURCE_FLAG_NONE
        },
        ..Default::default()
    };
    let mut value = None;
    device
        .CreateCommittedResource(
            &properties,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            state,
            None,
            &mut value,
        )
        .map_err(err)?;
    value.ok_or("GPU 资源分配没有返回对象".into())
}
unsafe fn upload<T: Copy>(device: &ID3D12Device5, data: &[T]) -> GResult<ID3D12Resource> {
    let bytes = std::mem::size_of_val(data);
    let resource = buffer(
        device,
        bytes as u64,
        D3D12_HEAP_TYPE_UPLOAD,
        D3D12_RESOURCE_STATE_GENERIC_READ,
        false,
    )?;
    let mut mapped = std::ptr::null_mut();
    resource.Map(0, None, Some(&mut mapped)).map_err(err)?;
    std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, mapped as *mut u8, bytes);
    resource.Unmap(0, None);
    Ok(resource)
}
unsafe fn barrier(
    list: &ID3D12GraphicsCommandList4,
    resource: &ID3D12Resource,
    before: Option<D3D12_RESOURCE_STATES>,
    after: D3D12_RESOURCE_STATES,
) {
    let mut value = D3D12_RESOURCE_BARRIER::default();
    if let Some(before) = before {
        value.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        value.Anonymous.Transition = ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: ManuallyDrop::new(Some(resource.clone())),
            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            StateBefore: before,
            StateAfter: after,
        });
    } else {
        value.Type = D3D12_RESOURCE_BARRIER_TYPE_UAV;
        value.Anonymous.UAV = ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
            pResource: ManuallyDrop::new(Some(resource.clone())),
        });
    }
    list.ResourceBarrier(std::slice::from_ref(&value));
    if before.is_some() {
        ManuallyDrop::drop(&mut (*value.Anonymous.Transition).pResource);
    } else {
        ManuallyDrop::drop(&mut (*value.Anonymous.UAV).pResource);
    }
}
impl Gpu {
    pub fn new(index: u32, vertices: &[[f32; 3]], object_ids: &[u32]) -> GResult<Self> {
        Self::with_budget(index, vertices, object_ids, None)
    }
    #[cfg(test)]
    pub fn remove_device(&self) {
        unsafe {
            self.device.RemoveDevice();
        }
    }
    pub(crate) fn with_budget(
        index: u32,
        vertices: &[[f32; 3]],
        object_ids: &[u32],
        budget_limit: Option<u64>,
    ) -> GResult<Self> {
        unsafe {
            if vertices.is_empty() || vertices.len() != object_ids.len() * 3 {
                return Err("无效三角网格".into());
            }
            let mut info = capabilities()?
                .into_iter()
                .find(|x| x.index == index)
                .ok_or("所选 GPU 不存在")?;
            if let Some(limit) = budget_limit {
                info.available_bytes = info.available_bytes.min(limit);
            }
            if !info.supported {
                return Err(info.reason);
            }
            if vertices.len() > u32::MAX as usize
                || (std::mem::size_of_val(vertices) as u64).saturating_mul(4) + 32 * 1024 * 1024
                    > info.available_bytes * 8 / 10
            {
                return Err("显存预算不足，无法上传所选几何；未降低烘焙质量".into());
            }
            let factory: IDXGIFactory4 =
                CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).map_err(err)?;
            let adapter = factory.EnumAdapters1(index).map_err(err)?;
            let mut device = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &mut device).map_err(err)?;
            let device: ID3D12Device5 = device.ok_or("无法创建设备")?;
            let queue: ID3D12CommandQueue = device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    ..Default::default()
                })
                .map_err(err)?;
            let allocator: ID3D12CommandAllocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(err)?;
            let list: ID3D12GraphicsCommandList4 = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
                .map_err(err)?;
            let fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(err)?;
            let vertex = upload(&device, vertices)?;
            let objects = upload(&device, object_ids)?;
            let geometry = D3D12_RAYTRACING_GEOMETRY_DESC {
                Type: D3D12_RAYTRACING_GEOMETRY_TYPE_TRIANGLES,
                Flags: D3D12_RAYTRACING_GEOMETRY_FLAG_NONE,
                Anonymous: D3D12_RAYTRACING_GEOMETRY_DESC_0 {
                    Triangles: D3D12_RAYTRACING_GEOMETRY_TRIANGLES_DESC {
                        VertexFormat: DXGI_FORMAT_R32G32B32_FLOAT,
                        VertexCount: vertices.len() as u32,
                        VertexBuffer: D3D12_GPU_VIRTUAL_ADDRESS_AND_STRIDE {
                            StartAddress: vertex.GetGPUVirtualAddress(),
                            StrideInBytes: 12,
                        },
                        ..Default::default()
                    },
                },
            };
            let inputs = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
                Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_BOTTOM_LEVEL,
                Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
                NumDescs: 1,
                DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
                Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 {
                    pGeometryDescs: &geometry,
                },
            };
            let mut pre = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
            device.GetRaytracingAccelerationStructurePrebuildInfo(&inputs, &mut pre);
            let needed = pre.ResultDataMaxSizeInBytes
                + pre.ScratchDataSizeInBytes
                + std::mem::size_of_val(vertices) as u64
                + std::mem::size_of_val(object_ids) as u64
                + 32 * 1024 * 1024;
            if pre.ResultDataMaxSizeInBytes == 0 || needed > info.available_bytes * 8 / 10 {
                return Err(format!(
                    "显存不足：加速结构及缓冲区预计需要 {} MiB，可用预算 {} MiB",
                    needed / 1048576,
                    info.available_bytes / 1048576
                ));
            }
            let blas = buffer(
                &device,
                pre.ResultDataMaxSizeInBytes,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE,
                true,
            )?;
            let scratch = buffer(
                &device,
                pre.ScratchDataSizeInBytes,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                true,
            )?;
            list.BuildRaytracingAccelerationStructure(
                &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC {
                    Inputs: inputs,
                    DestAccelerationStructureData: blas.GetGPUVirtualAddress(),
                    ScratchAccelerationStructureData: scratch.GetGPUVirtualAddress(),
                    ..Default::default()
                },
                None,
            );
            barrier(
                &list,
                &blas,
                None,
                D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE,
            );
            let instance = D3D12_RAYTRACING_INSTANCE_DESC {
                Transform: [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.],
                _bitfield1: 255 << 24,
                _bitfield2: 0,
                AccelerationStructure: blas.GetGPUVirtualAddress(),
            };
            let instances = upload(&device, &[instance])?;
            let inputs = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
                Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL,
                Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
                NumDescs: 1,
                DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
                Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 {
                    InstanceDescs: instances.GetGPUVirtualAddress(),
                },
            };
            device.GetRaytracingAccelerationStructurePrebuildInfo(&inputs, &mut pre);
            let tlas = buffer(
                &device,
                pre.ResultDataMaxSizeInBytes,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE,
                true,
            )?;
            let scratch_top = buffer(
                &device,
                pre.ScratchDataSizeInBytes,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                true,
            )?;
            list.BuildRaytracingAccelerationStructure(
                &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC {
                    Inputs: inputs,
                    DestAccelerationStructureData: tlas.GetGPUVirtualAddress(),
                    ScratchAccelerationStructureData: scratch_top.GetGPUVirtualAddress(),
                    ..Default::default()
                },
                None,
            );
            barrier(
                &list,
                &tlas,
                None,
                D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE,
            );
            let mut params = Vec::new();
            for register in 0..3 {
                params.push(D3D12_ROOT_PARAMETER {
                    ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        Descriptor: D3D12_ROOT_DESCRIPTOR {
                            ShaderRegister: register,
                            RegisterSpace: 0,
                        },
                    },
                    ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
                });
            }
            params.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_UAV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            });
            params.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: 8,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            });
            let desc = D3D12_ROOT_SIGNATURE_DESC {
                NumParameters: params.len() as u32,
                pParameters: params.as_ptr(),
                ..Default::default()
            };
            let mut blob = None;
            D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None)
                .map_err(err)?;
            let blob = blob.ok_or("根签名编译失败")?;
            let root: ID3D12RootSignature = device
                .CreateRootSignature(
                    0,
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer() as *const u8,
                        blob.GetBufferSize(),
                    ),
                )
                .map_err(err)?;
            let shader = include_bytes!("../shaders/ao.dxil");
            let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: ManuallyDrop::new(Some(root.clone())),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: shader.as_ptr() as _,
                    BytecodeLength: shader.len(),
                },
                ..Default::default()
            };
            let pipeline = device.CreateComputePipelineState(&desc).map_err(err);
            ManuallyDrop::drop(&mut desc.pRootSignature);
            let pipeline = pipeline?;
            let block_size = (((info.available_bytes - needed) / 128).min(16384).max(256)) as usize;
            let mut gpu = Self {
                adapter: adapter.cast().map_err(err)?,
                peak_device_bytes: 0,
                device,
                queue,
                allocator,
                list,
                fence,
                serial: 0,
                root,
                pipeline,
                tlas,
                objects,
                _geometry: vec![vertex, blas, instances, scratch, scratch_top],
                block_size,
            };
            gpu.submit()?;
            gpu.sample_memory();
            Ok(gpu)
        }
    }
    fn sample_memory(&mut self) {
        unsafe {
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            if self
                .adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                .is_ok()
            {
                self.peak_device_bytes = self.peak_device_bytes.max(info.CurrentUsage);
            }
        }
    }
    unsafe fn submit(&mut self) -> GResult<()> {
        self.list.Close().map_err(err)?;
        self.queue
            .ExecuteCommandLists(&[Some(self.list.cast().map_err(err)?)]);
        self.serial += 1;
        self.queue.Signal(&self.fence, self.serial).map_err(err)?;
        let start = Instant::now();
        loop {
            let complete = self.fence.GetCompletedValue();
            if complete == u64::MAX {
                return Err(format!(
                    "GPU 设备丢失：{:?}",
                    self.device.GetDeviceRemovedReason()
                ));
            }
            if complete >= self.serial {
                break;
            }
            if start.elapsed() > Duration::from_secs(30) {
                return Err("GPU 超时（30 秒），已停止任务".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.allocator.Reset().map_err(err)?;
        self.list.Reset(&self.allocator, None).map_err(err)?;
        Ok(())
    }
    pub fn trace(
        &mut self,
        surfaces: &[Surface],
        samples: u32,
        distance: f32,
        bias: f32,
        self_only: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> GResult<Vec<u32>> {
        unsafe {
            if surfaces.is_empty() {
                return Ok(vec![]);
            }
            let input = upload(&self.device, surfaces)?;
            let output = buffer(
                &self.device,
                (surfaces.len() * 4) as u64,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                true,
            )?;
            let readback = buffer(
                &self.device,
                (surfaces.len() * 4) as u64,
                D3D12_HEAP_TYPE_READBACK,
                D3D12_RESOURCE_STATE_COPY_DEST,
                false,
            )?;
            self.sample_memory();
            for start in (0..samples).step_by(8) {
                if cancelled() {
                    return Err("任务已取消".into());
                }
                self.list.SetPipelineState(&self.pipeline);
                self.list.SetComputeRootSignature(&self.root);
                self.list
                    .SetComputeRootShaderResourceView(0, self.tlas.GetGPUVirtualAddress());
                self.list
                    .SetComputeRootShaderResourceView(1, input.GetGPUVirtualAddress());
                self.list
                    .SetComputeRootShaderResourceView(2, self.objects.GetGPUVirtualAddress());
                self.list
                    .SetComputeRootUnorderedAccessView(3, output.GetGPUVirtualAddress());
                let params = [
                    surfaces.len() as u32,
                    start,
                    8.min(samples - start),
                    samples,
                    distance.to_bits(),
                    bias.to_bits(),
                    self_only as u32,
                    0,
                ];
                self.list
                    .SetComputeRoot32BitConstants(4, 8, params.as_ptr() as _, 0);
                self.list.Dispatch((surfaces.len() as u32 + 63) / 64, 1, 1);
                barrier(
                    &self.list,
                    &output,
                    None,
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                );
                self.submit()?;
            }
            barrier(
                &self.list,
                &output,
                Some(D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            self.list
                .CopyBufferRegion(&readback, 0, &output, 0, (surfaces.len() * 4) as u64);
            self.submit()?;
            let mut mapped = std::ptr::null_mut();
            readback.Map(0, None, Some(&mut mapped)).map_err(err)?;
            let values = std::slice::from_raw_parts(mapped as *const u32, surfaces.len()).to_vec();
            readback.Unmap(0, None);
            Ok(values)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub index: u32,
    pub name: String,
    pub vendor: u32,
    pub dedicated_bytes: u64,
    pub budget_bytes: u64,
    pub available_bytes: u64,
    pub supported: bool,
    pub reason: String,
}

pub fn capabilities() -> Result<Vec<DeviceInfo>, String> {
    unsafe {
        let factory: IDXGIFactory4 =
            CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for index in 0..64 {
            let Ok(adapter) = factory.EnumAdapters1(index) else {
                break;
            };
            let desc = adapter.GetDesc1().map_err(|e| e.to_string())?;
            if !seen.insert((desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart)) {
                continue;
            }
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let name = String::from_utf16_lossy(
                &desc.Description[..desc.Description.iter().position(|x| *x == 0).unwrap_or(128)],
            );
            let mut budget = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            if let Ok(a) = adapter.cast::<IDXGIAdapter3>() {
                let _ = a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut budget);
            }
            let mut device: Option<ID3D12Device5> = None;
            let mut reason = String::new();
            if let Err(e) = D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &mut device) {
                reason = format!("D3D12: {e}");
            }
            if let Some(device) = device {
                let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS5::default();
                let mut shader = D3D12_FEATURE_DATA_SHADER_MODEL {
                    HighestShaderModel: D3D_SHADER_MODEL_6_5,
                };
                if device
                    .CheckFeatureSupport(
                        D3D12_FEATURE_D3D12_OPTIONS5,
                        (&mut options as *mut _) as _,
                        std::mem::size_of_val(&options) as u32,
                    )
                    .is_err()
                    || options.RaytracingTier.0 < D3D12_RAYTRACING_TIER_1_1.0
                {
                    reason = "需要 DXR Tier 1.1".into();
                }
                if device
                    .CheckFeatureSupport(
                        D3D12_FEATURE_SHADER_MODEL,
                        (&mut shader as *mut _) as _,
                        std::mem::size_of_val(&shader) as u32,
                    )
                    .is_err()
                    || shader.HighestShaderModel.0 < D3D_SHADER_MODEL_6_5.0
                {
                    reason = "需要 Shader Model 6.5".into();
                }
            }
            result.push(DeviceInfo {
                index,
                name,
                vendor: desc.VendorId,
                dedicated_bytes: desc.DedicatedVideoMemory as u64,
                budget_bytes: budget.Budget,
                available_bytes: budget.Budget.saturating_sub(budget.CurrentUsage),
                supported: reason.is_empty(),
                reason,
            });
        }
        Ok(result)
    }
}
