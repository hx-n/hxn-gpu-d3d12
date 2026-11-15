//! Direct3D 12 for Haxe: a thin layer over Direct3D 12, keeping its conventions. Shaders are
//! hxnsl's HLSL, compiled to shader model 5.1 bytecode, and bound by the registers hxnsl reports
//! in `HLSL_SLOTS`. Synchronization is explicit: a program records the resource state
//! transitions its commands need, as in Direct3D 12.

#![cfg(windows)]

use std::ffi::{CString, c_void};
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};

use hxn_future::Future;
use windows::Win32::Foundation::{HANDLE, RECT};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{Interface, PCSTR};

hxn::package!("gpu.d3d12");

/// A free-threaded Direct3D 12 or DXGI object, which Microsoft documents as usable from any
/// thread. Command lists are the exception, and their methods take `&mut self`.
struct Shared<T>(T);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

fn failed(what: &str, error: windows::core::Error) -> String {
    format!("{what}: {error}")
}

fn usize_of(value: i64, what: &str) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| format!("{what} cannot be {value}"))
}

fn u32_of(value: i32, what: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{what} cannot be {value}"))
}

/// A blob's bytes.
fn blob_bytes(blob: &ID3DBlob) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
            .to_vec()
    }
}

/// A texture or vertex attribute format, DXGI's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum Format {
    Rgba32Float = 2,
    Rgb32Float = 6,
    Rgba16Float = 10,
    Rg32Float = 16,
    Rgba8Unorm = 28,
    Rgba8UnormSrgb = 29,
    D32Float = 40,
    R32Float = 41,
    R32Uint = 42,
    R32Sint = 43,
    R8Unorm = 61,
    Bgra8Unorm = 87,
    Bgra8UnormSrgb = 91,
}

impl Format {
    fn raw(self) -> DXGI_FORMAT {
        DXGI_FORMAT(self as i32)
    }

    fn bytes_per_pixel(self) -> u32 {
        match self {
            Format::Rgba32Float => 16,
            Format::Rgb32Float => 12,
            Format::Rgba16Float | Format::Rg32Float => 8,
            Format::R8Unorm => 1,
            _ => 4,
        }
    }
}

/// Where a buffer's memory is: GPU-only, CPU-written for the GPU to read, or GPU-written for the
/// CPU to read. Direct3D 12's values.
#[hxn::data]
#[derive(Clone, Copy, PartialEq)]
pub enum Heap {
    Default = 1,
    Upload = 2,
    Readback = 3,
}

/// Resource state bits, Direct3D 12's values, or-ed together.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum State {
    Common = 0,
    VertexAndConstantBuffer = 1,
    IndexBuffer = 2,
    RenderTarget = 4,
    UnorderedAccess = 8,
    DepthWrite = 16,
    DepthRead = 32,
    NonPixelShaderResource = 64,
    PixelShaderResource = 128,
    CopyDest = 1024,
    CopySource = 2048,
    GenericRead = 2755,
}

fn state(bits: i32) -> D3D12_RESOURCE_STATES {
    D3D12_RESOURCE_STATES(bits)
}

/// `TextureDescriptor.usage` bits, Direct3D 12's resource flags.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum TextureUsage {
    RenderTarget = 1,
    DepthStencil = 2,
    UnorderedAccess = 4,
}

#[hxn::data]
pub struct TextureDescriptor {
    pub width: i32,
    pub height: i32,
    pub format: Format,
    /// `TextureUsage` bits, or-ed together.
    pub usage: i32,
}

/// A root parameter's kind: a root descriptor for a buffer. Direct3D 12's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum ParameterKind {
    ConstantBuffer = 2,
    ShaderResource = 3,
    UnorderedAccess = 4,
}

#[hxn::data]
#[derive(Clone, Copy)]
pub enum Visibility {
    All = 0,
    Vertex = 1,
    Pixel = 5,
}

/// One root descriptor: `register` and `space` are hxnsl's `HLSL_SLOTS` ones.
#[hxn::data]
pub struct RootParameter {
    pub kind: ParameterKind,
    pub register: i32,
    pub space: i32,
    pub visibility: Visibility,
}

/// One vertex attribute: `location` is the shader's (hxnsl's `INPUT_<name>`), its HLSL
/// semantic `LOC<location>`.
#[hxn::data]
pub struct VertexAttribute {
    pub location: i32,
    pub slot: i32,
    pub format: Format,
    pub offset: i32,
}

#[hxn::data]
#[derive(Clone, Copy)]
pub enum Topology {
    PointList = 1,
    LineList = 2,
    LineStrip = 3,
    TriangleList = 4,
    TriangleStrip = 5,
}

impl Topology {
    fn kind(self) -> D3D12_PRIMITIVE_TOPOLOGY_TYPE {
        match self {
            Topology::PointList => D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT,
            Topology::LineList | Topology::LineStrip => D3D12_PRIMITIVE_TOPOLOGY_TYPE_LINE,
            _ => D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        }
    }
}

/// The DXGI factory: the adapters, WARP included.
#[hxn::interface]
pub struct Factory {
    factory: Shared<IDXGIFactory4>,
    adapters: Vec<Shared<IDXGIAdapter1>>,
}

#[hxn::methods]
impl Factory {
    pub fn create() -> Result<Factory, String> {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
            .map_err(|e| failed("creating the DXGI factory", e))?;
        let mut adapters = Vec::new();
        let mut index = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(index) } {
            adapters.push(Shared(adapter));
            index += 1;
        }
        Ok(Factory {
            factory: Shared(factory),
            adapters,
        })
    }

    /// The name of each adapter, in the order `createDevice` takes them.
    pub fn adapter_names(&self) -> Vec<String> {
        self.adapters
            .iter()
            .map(|a| {
                unsafe { a.0.GetDesc1() }
                    .map(|d| {
                        let end = d.Description.iter().position(|&c| c == 0).unwrap_or(128);
                        String::from_utf16_lossy(&d.Description[..end])
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Whether adapter `index` is a software one, such as WARP.
    pub fn is_software(&self, index: i32) -> bool {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.adapters.get(i))
            .and_then(|a| unsafe { a.0.GetDesc1() }.ok())
            .is_some_and(|d| d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0)
    }

    /// A device on adapter `index`, at feature level 11.0, with one direct queue.
    pub fn create_device(&self, index: i32) -> Result<Device, String> {
        let adapter = usize::try_from(index)
            .ok()
            .and_then(|i| self.adapters.get(i))
            .ok_or_else(|| format!("there is no adapter {index}"))?;
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter.0, D3D_FEATURE_LEVEL_11_0, &mut device) }
            .map_err(|e| failed("creating the device", e))?;
        let device = device.ok_or("Direct3D 12 made no device")?;
        let queue: ID3D12CommandQueue = unsafe {
            device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                ..Default::default()
            })
        }
        .map_err(|e| failed("creating the queue", e))?;
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
            .map_err(|e| failed("creating the fence", e))?;
        let _ = &self.factory;
        Ok(Device {
            shared: Arc::new(DeviceShared {
                device: Shared(device),
                queue: Shared(queue),
                fence: Shared(fence),
                signalled: Mutex::new(0),
            }),
        })
    }
}

struct DeviceShared {
    device: Shared<ID3D12Device>,
    queue: Shared<ID3D12CommandQueue>,
    fence: Shared<ID3D12Fence>,
    // The last fence value signalled; submits take the next one under this lock.
    signalled: Mutex<u64>,
}

/// A device and its direct queue; every other object comes from one.
#[hxn::interface]
pub struct Device {
    shared: Arc<DeviceShared>,
}

#[hxn::methods]
impl Device {
    /// A buffer of `size` bytes in `heap`, in the state each heap requires (generic read for
    /// upload, copy destination for readback, `initial` for default), allowing unordered
    /// access when `unordered` is true.
    pub fn create_buffer(
        &self,
        size: i64,
        heap: Heap,
        initial: i32,
        unordered: bool,
    ) -> Result<Buffer, String> {
        let size = usize_of(size, "a buffer's size")?.max(1) as u64;
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: if unordered {
                D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
            } else {
                D3D12_RESOURCE_FLAG_NONE
            },
            ..Default::default()
        };
        let start = match heap {
            Heap::Upload => D3D12_RESOURCE_STATE_GENERIC_READ,
            Heap::Readback => D3D12_RESOURCE_STATE_COPY_DEST,
            Heap::Default => state(initial),
        };
        let resource = self.commit(heap, &desc, start, None)?;
        Ok(Buffer {
            resource: Shared(resource),
            size: size as usize,
            heap,
        })
    }

    /// An upload buffer holding a copy of `bytes`.
    pub fn create_buffer_with(&self, bytes: &[u8]) -> Result<Buffer, String> {
        let buffer = self.create_buffer(bytes.len() as i64, Heap::Upload, 0, false)?;
        buffer.write(0, bytes)?;
        Ok(buffer)
    }

    /// A 2D texture in the default heap, in the common state, with a render target view when
    /// its usage includes `TextureUsage.RenderTarget`.
    pub fn create_texture(&self, descriptor: TextureDescriptor) -> Result<Texture, String> {
        let width = u32_of(descriptor.width, "a texture's width")?;
        let height = u32_of(descriptor.height, "a texture's height")?;
        let flags = D3D12_RESOURCE_FLAGS(descriptor.usage);
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: descriptor.format.raw(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: flags,
            ..Default::default()
        };
        let resource = self.commit(Heap::Default, &desc, D3D12_RESOURCE_STATE_COMMON, None)?;
        let d = &self.shared.device.0;
        let target = if flags.contains(D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET) {
            let heap: ID3D12DescriptorHeap = unsafe {
                d.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                    NumDescriptors: 1,
                    ..Default::default()
                })
            }
            .map_err(|e| failed("creating the render target heap", e))?;
            let handle = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
            unsafe { d.CreateRenderTargetView(&resource, None, handle) };
            Some(Shared(heap))
        } else {
            None
        };
        Ok(Texture {
            resource: Shared(resource),
            target,
            width,
            height,
            format: descriptor.format,
        })
    }

    /// HLSL `source` compiled to shader model 5.1 bytecode for `entry` and `profile` (`cs_5_1`,
    /// `vs_5_1`, `ps_5_1`), with the compiler that ships with Windows.
    pub fn compile(&self, source: &str, entry: &str, profile: &str) -> Result<Vec<u8>, String> {
        let entry = CString::new(entry).map_err(|_| "the entry point contains a NUL")?;
        let profile = CString::new(profile).map_err(|_| "the profile contains a NUL")?;
        let mut code: Option<ID3DBlob> = None;
        let mut errors: Option<ID3DBlob> = None;
        let compiled = unsafe {
            D3DCompile(
                source.as_ptr() as *const c_void,
                source.len(),
                PCSTR::null(),
                None,
                None,
                PCSTR(entry.as_ptr() as *const u8),
                PCSTR(profile.as_ptr() as *const u8),
                D3DCOMPILE_OPTIMIZATION_LEVEL3,
                0,
                &mut code,
                Some(&mut errors),
            )
        };
        if let Err(e) = compiled {
            let log = errors
                .map(|b| String::from_utf8_lossy(&blob_bytes(&b)).into_owned())
                .unwrap_or_default();
            return Err(format!("compiling the HLSL: {e}: {log}"));
        }
        Ok(blob_bytes(&code.ok_or("the compiler made no bytecode")?))
    }

    /// A root signature of root descriptors, in parameter order.
    pub fn create_root_signature(
        &self,
        parameters: Vec<RootParameter>,
    ) -> Result<RootSignature, String> {
        let raw = parameters
            .iter()
            .map(|p| {
                Ok(D3D12_ROOT_PARAMETER {
                    ParameterType: D3D12_ROOT_PARAMETER_TYPE(p.kind as i32),
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        Descriptor: D3D12_ROOT_DESCRIPTOR {
                            ShaderRegister: u32_of(p.register, "a register")?,
                            RegisterSpace: u32_of(p.space, "a register space")?,
                        },
                    },
                    ShaderVisibility: D3D12_SHADER_VISIBILITY(p.visibility as i32),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: raw.len() as u32,
            pParameters: raw.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        let mut blob: Option<ID3DBlob> = None;
        let mut errors: Option<ID3DBlob> = None;
        if let Err(e) = unsafe {
            D3D12SerializeRootSignature(
                &desc,
                D3D_ROOT_SIGNATURE_VERSION_1,
                &mut blob,
                Some(&mut errors),
            )
        } {
            let log = errors
                .map(|b| String::from_utf8_lossy(&blob_bytes(&b)).into_owned())
                .unwrap_or_default();
            return Err(format!("serializing the root signature: {e}: {log}"));
        }
        let bytes = blob_bytes(&blob.ok_or("no root signature")?);
        let signature: ID3D12RootSignature =
            unsafe { self.shared.device.0.CreateRootSignature(0, &bytes) }
                .map_err(|e| failed("creating the root signature", e))?;
        Ok(RootSignature {
            signature: Shared(signature),
        })
    }

    /// A compute pipeline state from compute shader bytecode.
    pub fn create_compute_pipeline(
        &self,
        bytecode: &[u8],
        signature: &RootSignature,
    ) -> Result<PipelineState, String> {
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&signature.signature.0) },
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: bytecode.as_ptr() as *const c_void,
                BytecodeLength: bytecode.len(),
            },
            ..Default::default()
        };
        let state: ID3D12PipelineState =
            unsafe { self.shared.device.0.CreateComputePipelineState(&desc) }
                .map_err(|e| failed("creating the compute pipeline state", e))?;
        Ok(PipelineState {
            state: Shared(state),
        })
    }

    /// A graphics pipeline state rendering into one target of `format`, with no blending, no
    /// culling and no depth.
    #[allow(clippy::too_many_arguments)]
    pub fn create_graphics_pipeline(
        &self,
        vertex: &[u8],
        pixel: &[u8],
        signature: &RootSignature,
        format: Format,
        topology: Topology,
        attributes: Vec<VertexAttribute>,
    ) -> Result<PipelineState, String> {
        // naga names every vertex input's semantic LOC, indexed by its location.
        static SEMANTIC: &[u8] = b"LOC\0";
        let elements = attributes
            .iter()
            .map(|a| {
                Ok(D3D12_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(SEMANTIC.as_ptr()),
                    SemanticIndex: u32_of(a.location, "a location")?,
                    Format: a.format.raw(),
                    InputSlot: u32_of(a.slot, "an input slot")?,
                    AlignedByteOffset: u32_of(a.offset, "an offset")?,
                    InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut blend = D3D12_BLEND_DESC::default();
        blend.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        let mut formats = [DXGI_FORMAT_UNKNOWN; 8];
        formats[0] = format.raw();
        let desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&signature.signature.0) },
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: vertex.as_ptr() as *const c_void,
                BytecodeLength: vertex.len(),
            },
            PS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: pixel.as_ptr() as *const c_void,
                BytecodeLength: pixel.len(),
            },
            BlendState: blend,
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            },
            InputLayout: D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: elements.as_ptr(),
                NumElements: elements.len() as u32,
            },
            PrimitiveTopologyType: topology.kind(),
            NumRenderTargets: 1,
            RTVFormats: formats,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        let state: ID3D12PipelineState =
            unsafe { self.shared.device.0.CreateGraphicsPipelineState(&desc) }
                .map_err(|e| failed("creating the graphics pipeline state", e))?;
        Ok(PipelineState {
            state: Shared(state),
        })
    }

    /// A direct command list and its allocator, open for recording.
    pub fn create_command_list(&self) -> Result<CommandList, String> {
        let d = &self.shared.device.0;
        let allocator: ID3D12CommandAllocator =
            unsafe { d.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }
                .map_err(|e| failed("creating the command allocator", e))?;
        let list: ID3D12GraphicsCommandList =
            unsafe { d.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }
                .map_err(|e| failed("creating the command list", e))?;
        Ok(CommandList {
            recorded: Arc::new(Recorded {
                list: Shared(list),
                _allocator: Shared(allocator),
            }),
            closed: false,
        })
    }

    /// Closes and executes `commands`, and waits for the GPU to finish them.
    #[hxn(blocking)]
    pub fn execute_and_wait(&self, commands: &mut CommandList) -> Result<(), String> {
        let (value, _) = self.execute_fenced(commands)?;
        wait(&self.shared, value)
    }

    /// Closes and executes `commands`. The future settles when the GPU has finished them.
    pub fn execute(&self, commands: &mut CommandList) -> Result<Future<()>, String> {
        let (value, recorded) = self.execute_fenced(commands)?;
        let shared = self.shared.clone();
        // The command list and allocator stay alive until the fence reaches `value`.
        Ok(hxn_future::spawn_blocking(move || {
            let waited = wait(&shared, value);
            drop(recorded);
            waited
        }))
    }
}

/// Blocks until the device's fence reaches `value`: a null event makes the call wait.
fn wait(shared: &DeviceShared, value: u64) -> Result<(), String> {
    unsafe {
        shared
            .fence
            .0
            .SetEventOnCompletion(value, HANDLE::default())
    }
    .map_err(|e| failed("waiting for the GPU", e))
}

impl Device {
    fn commit(
        &self,
        heap: Heap,
        desc: &D3D12_RESOURCE_DESC,
        start: D3D12_RESOURCE_STATES,
        clear: Option<&D3D12_CLEAR_VALUE>,
    ) -> Result<ID3D12Resource, String> {
        let properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE(heap as i32),
            ..Default::default()
        };
        let mut resource: Option<ID3D12Resource> = None;
        unsafe {
            self.shared.device.0.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                desc,
                start,
                clear.map(|c| c as *const _),
                &mut resource,
            )
        }
        .map_err(|e| failed("creating the resource", e))?;
        resource.ok_or_else(|| "Direct3D 12 made no resource".into())
    }

    fn execute_fenced(&self, commands: &mut CommandList) -> Result<(u64, Arc<Recorded>), String> {
        commands.close_recording()?;
        let list: ID3D12CommandList = commands
            .recorded
            .list
            .0
            .cast()
            .map_err(|e| failed("the command list", e))?;
        let mut signalled = self
            .shared
            .signalled
            .lock()
            .map_err(|_| "the fence's lock is poisoned")?;
        unsafe { self.shared.queue.0.ExecuteCommandLists(&[Some(list)]) };
        let value = *signalled + 1;
        unsafe { self.shared.queue.0.Signal(&self.shared.fence.0, value) }
            .map_err(|e| failed("signalling the fence", e))?;
        *signalled = value;
        Ok((value, commands.recorded.clone()))
    }
}

/// A buffer. An upload or readback buffer can be written or read by the CPU.
#[hxn::interface]
pub struct Buffer {
    resource: Shared<ID3D12Resource>,
    size: usize,
    heap: Heap,
}

#[hxn::methods]
impl Buffer {
    pub fn size(&self) -> i64 {
        self.size as i64
    }

    /// Copies `bytes` into an upload buffer at `offset`.
    pub fn write(&self, offset: i64, bytes: &[u8]) -> Result<(), String> {
        if self.heap != Heap::Upload {
            return Err("only an upload buffer can be written".into());
        }
        let start = self.range(offset, bytes.len())?;
        let base = self.map()?;
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), base.add(start), bytes.len());
            self.resource.0.Unmap(0, None);
        }
        Ok(())
    }

    /// `length` bytes of a readback buffer from `offset`.
    pub fn read(&self, offset: i64, length: i64) -> Result<Vec<u8>, String> {
        if self.heap != Heap::Readback {
            return Err("only a readback buffer can be read".into());
        }
        let length = usize_of(length, "a length")?;
        let start = self.range(offset, length)?;
        let base = self.map()?;
        let bytes = unsafe { std::slice::from_raw_parts(base.add(start), length) }.to_vec();
        unsafe {
            self.resource
                .0
                .Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }))
        };
        Ok(bytes)
    }
}

impl Buffer {
    fn range(&self, offset: i64, length: usize) -> Result<usize, String> {
        let start = usize_of(offset, "an offset")?;
        if start.checked_add(length).is_none_or(|end| end > self.size) {
            return Err(format!(
                "{length} bytes at {start} run past the buffer's {}",
                self.size
            ));
        }
        Ok(start)
    }

    fn map(&self) -> Result<*mut u8, String> {
        let mut data: *mut c_void = std::ptr::null_mut();
        unsafe { self.resource.0.Map(0, None, Some(&mut data)) }
            .map_err(|e| failed("mapping the buffer", e))?;
        Ok(data as *mut u8)
    }
}

/// A 2D texture in the default heap.
#[hxn::interface]
pub struct Texture {
    resource: Shared<ID3D12Resource>,
    target: Option<Shared<ID3D12DescriptorHeap>>,
    width: u32,
    height: u32,
    format: Format,
}

#[hxn::methods]
impl Texture {
    pub fn width(&self) -> i32 {
        self.width as i32
    }

    pub fn height(&self) -> i32 {
        self.height as i32
    }

    /// The bytes between rows when `CommandList.copyTextureToBuffer` copies it: its row,
    /// rounded up to Direct3D 12's 256-byte alignment.
    pub fn copy_pitch(&self) -> i32 {
        self.pitch() as i32
    }
}

impl Texture {
    fn pitch(&self) -> u32 {
        let row = self.width * self.format.bytes_per_pixel();
        row.div_ceil(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT) * D3D12_TEXTURE_DATA_PITCH_ALIGNMENT
    }
}

#[hxn::interface]
pub struct RootSignature {
    signature: Shared<ID3D12RootSignature>,
}

#[hxn::methods]
impl RootSignature {}

#[hxn::interface]
pub struct PipelineState {
    state: Shared<ID3D12PipelineState>,
}

#[hxn::methods]
impl PipelineState {}

/// A command list and its allocator, kept until the GPU has finished them.
struct Recorded {
    list: Shared<ID3D12GraphicsCommandList>,
    _allocator: Shared<ID3D12CommandAllocator>,
}

/// Records commands until a `Device.execute`. Not thread-safe: its methods take `&mut self`.
#[hxn::interface]
pub struct CommandList {
    recorded: Arc<Recorded>,
    closed: bool,
}

#[hxn::methods]
impl CommandList {
    pub fn set_compute_root_signature(&mut self, signature: &RootSignature) -> Result<(), String> {
        let list = self.recording()?;
        unsafe { list.SetComputeRootSignature(&signature.signature.0) };
        Ok(())
    }

    pub fn set_graphics_root_signature(&mut self, signature: &RootSignature) -> Result<(), String> {
        let list = self.recording()?;
        unsafe { list.SetGraphicsRootSignature(&signature.signature.0) };
        Ok(())
    }

    pub fn set_pipeline_state(&mut self, state: &PipelineState) -> Result<(), String> {
        let list = self.recording()?;
        unsafe { list.SetPipelineState(&state.state.0) };
        Ok(())
    }

    /// Binds `buffer` to root parameter `index` of the compute root signature, as the kind of
    /// descriptor that parameter is.
    pub fn set_compute_root_buffer(
        &mut self,
        index: i32,
        kind: ParameterKind,
        buffer: &Buffer,
    ) -> Result<(), String> {
        let list = self.recording()?;
        let (index, address) = (u32_of(index, "a root parameter")?, unsafe {
            buffer.resource.0.GetGPUVirtualAddress()
        });
        unsafe {
            match kind {
                ParameterKind::ConstantBuffer => {
                    list.SetComputeRootConstantBufferView(index, address)
                }
                ParameterKind::ShaderResource => {
                    list.SetComputeRootShaderResourceView(index, address)
                }
                ParameterKind::UnorderedAccess => {
                    list.SetComputeRootUnorderedAccessView(index, address)
                }
            }
        };
        Ok(())
    }

    /// Binds `buffer` to root parameter `index` of the graphics root signature.
    pub fn set_graphics_root_buffer(
        &mut self,
        index: i32,
        kind: ParameterKind,
        buffer: &Buffer,
    ) -> Result<(), String> {
        let list = self.recording()?;
        let (index, address) = (u32_of(index, "a root parameter")?, unsafe {
            buffer.resource.0.GetGPUVirtualAddress()
        });
        unsafe {
            match kind {
                ParameterKind::ConstantBuffer => {
                    list.SetGraphicsRootConstantBufferView(index, address)
                }
                ParameterKind::ShaderResource => {
                    list.SetGraphicsRootShaderResourceView(index, address)
                }
                ParameterKind::UnorderedAccess => {
                    list.SetGraphicsRootUnorderedAccessView(index, address)
                }
            }
        };
        Ok(())
    }

    pub fn dispatch(&mut self, x: i32, y: i32, z: i32) -> Result<(), String> {
        let list = self.recording()?;
        unsafe {
            list.Dispatch(
                u32_of(x, "a group count")?,
                u32_of(y, "a group count")?,
                u32_of(z, "a group count")?,
            )
        };
        Ok(())
    }

    /// Moves `buffer` from state `before` to `after` (`State` bits).
    pub fn transition_buffer(
        &mut self,
        buffer: &Buffer,
        before: i32,
        after: i32,
    ) -> Result<(), String> {
        self.transition(&buffer.resource.0, before, after)
    }

    /// Moves `texture` from state `before` to `after` (`State` bits).
    pub fn transition_texture(
        &mut self,
        texture: &Texture,
        before: i32,
        after: i32,
    ) -> Result<(), String> {
        self.transition(&texture.resource.0, before, after)
    }

    /// Waits for unordered-access writes to `buffer` before later unordered access to it.
    pub fn uav_barrier(&mut self, buffer: &Buffer) -> Result<(), String> {
        let list = self.recording()?;
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                    pResource: unsafe { std::mem::transmute_copy(&buffer.resource.0) },
                }),
            },
        };
        unsafe { list.ResourceBarrier(&[barrier]) };
        Ok(())
    }

    /// Renders into `target`, which must be in the render target state, clearing it to
    /// `red`, `green`, `blue`, `alpha`, and sets the viewport and scissor to all of it.
    pub fn set_render_target(
        &mut self,
        target: &Texture,
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
    ) -> Result<(), String> {
        let list = self.recording()?;
        let heap = target
            .target
            .as_ref()
            .ok_or("the texture is not a render target")?;
        let handle = unsafe { heap.0.GetCPUDescriptorHandleForHeapStart() };
        let color = [red as f32, green as f32, blue as f32, alpha as f32];
        unsafe {
            list.OMSetRenderTargets(1, Some(&handle), false, None);
            list.ClearRenderTargetView(handle, &color, None);
            list.RSSetViewports(&[D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: target.width as f32,
                Height: target.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]);
            list.RSSetScissorRects(&[RECT {
                left: 0,
                top: 0,
                right: target.width as i32,
                bottom: target.height as i32,
            }]);
        }
        Ok(())
    }

    pub fn set_topology(&mut self, topology: Topology) -> Result<(), String> {
        let raw = match topology {
            Topology::PointList => D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
            Topology::LineList => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
            Topology::LineStrip => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
            Topology::TriangleList => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            Topology::TriangleStrip => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
        };
        let list = self.recording()?;
        unsafe { list.IASetPrimitiveTopology(raw) };
        Ok(())
    }

    /// Binds all of `buffer` to input slot `slot`, `stride` bytes per vertex.
    pub fn set_vertex_buffer(
        &mut self,
        slot: i32,
        buffer: &Buffer,
        stride: i32,
    ) -> Result<(), String> {
        let list = self.recording()?;
        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: unsafe { buffer.resource.0.GetGPUVirtualAddress() },
            SizeInBytes: buffer.size as u32,
            StrideInBytes: u32_of(stride, "a stride")?,
        };
        unsafe { list.IASetVertexBuffers(u32_of(slot, "an input slot")?, Some(&[view])) };
        Ok(())
    }

    /// Draws `count` vertices from `first`, `instances` times.
    pub fn draw(&mut self, count: i32, instances: i32, first: i32) -> Result<(), String> {
        let list = self.recording()?;
        unsafe {
            list.DrawInstanced(
                u32_of(count, "a vertex count")?,
                u32_of(instances, "an instance count")?,
                u32_of(first, "a first vertex")?,
                0,
            )
        };
        Ok(())
    }

    /// Copies all of `source` into `destination`, which must be at least as large.
    pub fn copy_buffer(&mut self, destination: &Buffer, source: &Buffer) -> Result<(), String> {
        let list = self.recording()?;
        if destination.size < source.size {
            return Err("the destination is smaller than the source".into());
        }
        unsafe {
            list.CopyBufferRegion(
                &destination.resource.0,
                0,
                &source.resource.0,
                0,
                source.size as u64,
            )
        };
        Ok(())
    }

    /// Copies all of `texture`, in the copy source state, into `buffer`, rows
    /// `texture.copyPitch()` bytes apart.
    pub fn copy_texture_to_buffer(
        &mut self,
        texture: &Texture,
        buffer: &Buffer,
    ) -> Result<(), String> {
        let list = self.recording()?;
        let needed = texture.pitch() as usize * texture.height as usize;
        if buffer.size < needed {
            return Err(format!(
                "the buffer holds {} bytes and the copy needs {needed}",
                buffer.size
            ));
        }
        let destination = D3D12_TEXTURE_COPY_LOCATION {
            pResource: unsafe { std::mem::transmute_copy(&buffer.resource.0) },
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: 0,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: texture.format.raw(),
                        Width: texture.width,
                        Height: texture.height,
                        Depth: 1,
                        RowPitch: texture.pitch(),
                    },
                },
            },
        };
        let source = D3D12_TEXTURE_COPY_LOCATION {
            pResource: unsafe { std::mem::transmute_copy(&texture.resource.0) },
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        unsafe { list.CopyTextureRegion(&destination, 0, 0, 0, &source, None) };
        Ok(())
    }
}

impl CommandList {
    fn recording(&self) -> Result<&ID3D12GraphicsCommandList, String> {
        if self.closed {
            return Err("the command list was executed; record into a new one".into());
        }
        Ok(&self.recorded.list.0)
    }

    fn transition(
        &mut self,
        resource: &ID3D12Resource,
        before: i32,
        after: i32,
    ) -> Result<(), String> {
        let list = self.recording()?;
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: unsafe { std::mem::transmute_copy(resource) },
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: state(before),
                    StateAfter: state(after),
                }),
            },
        };
        unsafe { list.ResourceBarrier(&[barrier]) };
        Ok(())
    }

    fn close_recording(&mut self) -> Result<(), String> {
        let list = self.recording()?;
        unsafe { list.Close() }.map_err(|e| failed("closing the command list", e))?;
        self.closed = true;
        Ok(())
    }
}
