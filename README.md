# hxn-gpu-d3d12

[![Ash](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/ash.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/ash.yml)
[![HashLink](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hashlink.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hashlink.yml)
[![hxcpp](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hxcpp.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hxcpp.yml)
[![CPPIA](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/cppia.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/cppia.yml)

Direct3D 12 bindings for Haxe on Ash, HashLink, hxcpp and CPPIA, on Windows.
The API follows Direct3D 12: a DXGI `Factory` and its adapters, `Device`,
resources, root signatures, pipeline states, command lists and swap chains.
Synchronization is explicit: you record resource state transitions, or
enhanced barriers where the device supports them.

Shaders come from [hxnsl](https://github.com/hx-n/hxnsl), which generates HLSL
at compile time and lists each binding's register in `HLSL_SLOTS`. With DXC
installed, each entry point is also compiled to DXIL at compile time.
`RootLayout` builds the root signature from the registers.

```haxe
import hxn.gpu.d3d12.*;

var factory = Factory.create();
var device = factory.createDevice(0);              // factory.adapterNames(), WARP included
var bytecode = hxn.gpu.d3d12.Shaders.bytecode(device, Tripled.dxil("compute_main"), Tripled.HLSL, "compute_main", "compute");
var layout = new RootLayout(Tripled.HLSL_SLOTS, Tripled.LAYOUT);
var signature = device.createRootSignature(layout.parameters);
var pipeline = device.createComputePipeline(bytecode, signature);
var data = device.createBuffer(size, Heap.Default, State.Common, true);

var commands = device.createCommandList();
commands.setComputeRootSignature(signature);
commands.setPipelineState(pipeline);
commands.setComputeRootBuffer(layout.buffer(Tripled.BUFFER_data_GROUP, Tripled.BUFFER_data), ParameterKind.UnorderedAccess, data);
commands.dispatch(4, 1, 1);
device.execute(commands).then(_ -> trace("done"));
```

[`examples/check/Main.hx`](https://github.com/hx-n/hxn-gpu-d3d12/blob/main/examples/check/Main.hx)
is the whole program: a compute pass over a buffer, a render pass into a
texture, a pass sampling a texture, and the compute pass again with enhanced
barriers. [`examples/present/Main.hx`](https://github.com/hx-n/hxn-gpu-d3d12/blob/main/examples/present/Main.hx)
presents to an [hxn-window](https://github.com/hx-n/hxn-window) window.

## Installing

```
haxelib install hxn-gpu-d3d12
haxelib install hxnsl
haxe -lib hxn-gpu-d3d12 -lib hxnsl -main Main -hl main.hl
haxe -lib hxn-gpu-d3d12 -lib hxnsl -main Main -cpp out
```

The haxelib carries the native library prebuilt for Windows on x86-64, for
HashLink and hxcpp. A program needs no Rust.

For DXIL, install [DXC](https://github.com/microsoft/DirectXShaderCompiler/releases)
1.8 or later and put `dxc` on `PATH`, or set `HXN_GPU_D3D12_DXC` to its path.
Without it, compilation warns once and the program compiles the HLSL when it
loads.

## Classes

All in the package `hxn.gpu.d3d12`:

| Class | |
|---|---|
| `Factory` | The adapters (`adapterNames`, `isSoftware`), `createDevice(index)` and `createSwapchain`. |
| `Device` | Buffers, textures, descriptor heaps, HLSL compiles, root signatures, pipeline states and command lists; `supportsEnhancedBarriers`, `execute` and `executeAndWait`. |
| `Buffer` | In the default, upload or readback heap. An upload buffer is written, a readback buffer read. |
| `Texture` | A 2D texture in the default heap, described by a `TextureDescriptor`, with a render target view when it is one. |
| `DescriptorHeap` | A shader-visible heap of texture views (`writeTexture`) or samplers (`writeSampler`). |
| `Swapchain` | A flip-model swap chain on a window: `currentIndex`, `buffer(index)` and `present(interval)`. |
| `RootSignature`, `PipelineState` | A root signature of root descriptors and descriptor tables, and a compute or graphics pipeline state. |
| `CommandList` | Root signatures, root buffers and tables, descriptor heaps, dispatches, render targets, draws, copies, transitions and barriers. |
| `RootLayout` | The root parameters for an hxnsl shader, and the index of each binding's parameter. |

Every object holds its Direct3D 12 object until `close()`. Direct3D 12's
errors are thrown as Haxe exceptions.

## Synchronization

`transitionBuffer` and `transitionTexture` move a resource between `State`s,
which a resource must be in for each use: `UnorderedAccess` for a compute
shader's writes, `RenderTarget` to render into, `PixelShaderResource` to
sample, `CopySource` and `CopyDest` for copies. `uavBarrier` orders
unordered-access writes to one resource.

Where `Device.supportsEnhancedBarriers()` is true, `bufferBarrier` and
`textureBarrier` take the place of transitions. Each names the stages
(`BarrierSync`) and accesses (`Access`) before and after it, and a texture
barrier also names the `Layout` before and after. A resource's first barrier
starts from `BarrierSync.None` and `Access.NoAccess`.

`Device.execute` returns an `hxn.Future` that settles when the GPU has
finished, from a thread waiting on the device's fence; the command list is
kept until then. `executeAndWait` is the blocking form.

## Shaders from hxnsl

A shader for this driver implements `hxn.gpu.d3d12.Shader`, which extends
`hxnsl.Shader` and gets hxnsl's `HLSL`, `HLSL_SLOTS` and `LAYOUT`.

- `Shader.dxil(entry)` is the entry point's DXIL, shader model 6.0, compiled
  by DXC at compile time, or null without DXC.
  `hxn.gpu.d3d12.Shaders.bytecode(device, Shader.dxil(entry), Shader.HLSL, entry, stage)`
  returns it, or else compiles the HLSL to shader model 5.1 with the
  compiler that ships with Windows. The entry points are `vertex_main`,
  `fragment_main` and `compute_main`.
- `new RootLayout(Shader.HLSL_SLOTS, Shader.LAYOUT)` lays out the root
  signature: a root descriptor for each buffer, a descriptor table for each
  group's textures, and, when the shader samples, the sampler heap tables
  and each group's sampler index array. `buffer(group, binding)`,
  `textureTable(group)`, `textureOffset(group, binding)`, `samplerHeap`,
  `comparisonSamplerHeap` and `samplerIndices(group)` give the root
  parameter indices.
- The HLSL reads samplers from a heap of up to 2048 through an index array
  per group. `RootLayout.samplerIndexArray([binding => slot])` makes that
  array, mapping each sampler binding to its slot in the sampler heap; it
  binds as a root shader resource at `samplerIndices(group)`. Both sampler
  heap tables point at the sampler heap.
- Vertex inputs have the semantic `LOC` with the location as its index.

## Presenting

`factory.createSwapchain(device, hwnd, width, height, Format.Bgra8Unorm, 2)`
makes a flip-model swap chain on a window. With hxn-window the window's
handle is `window.raw(0)`. Each frame renders into
`chain.buffer(chain.currentIndex())`, transitions it back to `State.Common`,
executes, and calls `chain.present(1)` to wait for vertical sync or
`present(0)` not to.

## Building from source

Requires the [hxn](https://github.com/hx-n/hxn) CLI, Rust, Haxe 4.3, hxnsl
and Windows, and DXC for the DXIL. `hxn check` builds the binding and runs
`examples/check` on each runtime named, and fails unless every run passes and
prints the same output. CI runs it with WARP, Direct3D 12's software
rasterizer, and runs `examples/present` on a window:

```
hxn check hl --vm ash
hxn check cpp cppia
```

## Agent skill

[`skills/hxn-gpu-d3d12/SKILL.md`](https://github.com/hx-n/hxn-gpu-d3d12/blob/main/skills/hxn-gpu-d3d12/SKILL.md)
describes the library for coding agents. The haxelib includes it.

## License

Apache-2.0.

## Third-party notices

Dependency licenses and redistribution notices are in
[third-party/NOTICES.md](https://github.com/hx-n/hxn-gpu-d3d12/blob/main/third-party/NOTICES.md). Retain the applicable
notices when redistributing binaries.

## Logging

Diagnostics use core `hxn.Log` with target `hxn.gpu.d3d12`. The application
can set `Log.level` and `Log.sink` to capture Haxe and native records together.
Custom frame loops drain them with `hxn.Loop.sync()`; idle background work
needs an application-driven `hxn.Log.flush()`.
