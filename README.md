# hxn-gpu-d3d12

[![Ash](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/ash.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/ash.yml)
[![HashLink](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hashlink.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hashlink.yml)
[![hxcpp](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hxcpp.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/hxcpp.yml)
[![CPPIA](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/cppia.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-d3d12/actions/workflows/cppia.yml)

Direct3D 12 for Haxe on Ash, HashLink, hxcpp and CPPIA, on Windows. It keeps
Direct3D 12's own conventions: a DXGI factory finds the adapters, a device
makes resources, root signatures and pipeline states, and command lists
record work a program executes on the device's queue. Synchronization is
explicit: the program records the resource state transitions its commands
need.

Shaders come from [hxnsl](https://github.com/hx-n/hxnsl), which writes HLSL
during the Haxe build and reports each binding's register in `HLSL_SLOTS`.
The driver builds root signatures from those registers.

```haxe
import gpu.d3d12.*;

var factory = Factory.create();
var device = factory.createDevice(0);              // factory.adapterNames(), WARP included
var bytecode = device.compile(Tripled.HLSL, "compute_main", "cs_5_1");
var signature = device.createRootSignature(gpu.d3d12.Shaders.rootParameters(Tripled.HLSL_SLOTS, Tripled.LAYOUT));
var pipeline = device.createComputePipeline(bytecode, signature);
var data = device.createBuffer(size, Heap.Default, State.Common, true);

var commands = device.createCommandList();
commands.setComputeRootSignature(signature);
commands.setPipelineState(pipeline);
commands.setComputeRootBuffer(0, ParameterKind.UnorderedAccess, data);
commands.dispatch(4, 1, 1);
device.execute(commands).then(_ -> trace("done"));
```

[`examples/check/Main.hx`](https://github.com/hx-n/hxn-gpu-d3d12/blob/main/examples/check/Main.hx)
is the whole program: buffers uploaded and read back, and a render pass into
a texture copied back.

## Installing

```
haxelib install hxn-gpu-d3d12
haxelib install hxnsl
haxe -lib hxn-gpu-d3d12 -lib hxnsl -main Main -hl main.hl
haxe -lib hxn-gpu-d3d12 -lib hxnsl -main Main -cpp out
```

The haxelib carries the native library prebuilt for Windows on x86-64, for
HashLink and hxcpp. A program needs no Rust.

## Classes

All in the package `gpu.d3d12`:

| Class | |
|---|---|
| `Factory` | The adapters (`adapterNames`, `isSoftware`) and `createDevice(index)`. |
| `Device` | Buffers, textures, HLSL compiles, root signatures, pipeline states and command lists; `execute` and `executeAndWait`. |
| `Buffer` | In the default, upload or readback heap. An upload buffer is written, a readback buffer read. |
| `Texture` | A 2D texture in the default heap, described by a `TextureDescriptor`, with a render target view when it is one. |
| `RootSignature`, `PipelineState` | A root signature of root descriptors, and a compute or graphics pipeline state. |
| `CommandList` | Root signatures and buffers, dispatches, render targets, draws, copies, `transitionBuffer`, `transitionTexture` and `uavBarrier`. |

Every object holds its Direct3D 12 object until `close()`. Direct3D 12's
errors are thrown as Haxe exceptions.

## Synchronization

`transitionBuffer` and `transitionTexture` move a resource between `State`s,
which a resource must be in for each use: `UnorderedAccess` for a compute
shader's writes, `RenderTarget` to render into, `CopySource` and `CopyDest`
for copies. `uavBarrier` orders unordered-access writes to one resource.

`Device.execute` returns an `hxn.Future` that settles when the GPU has
finished, from a thread waiting on the device's fence; the command list is
kept until then. `executeAndWait` is the blocking form.

## Shaders from hxnsl

A shader for this driver implements `gpu.d3d12.Shader`, which extends
`hxnsl.Shader` and gets hxnsl's `HLSL`, `HLSL_SLOTS` and `LAYOUT`.

- `Device.compile(Shader.HLSL, entry, profile)` compiles it with the HLSL
  compiler that ships with Windows, to shader model 5.1 bytecode. The entry
  points are `vertex_main`, `fragment_main` and `compute_main`.
- `gpu.d3d12.Shaders.rootParameters(Shader.HLSL_SLOTS, Shader.LAYOUT)` makes
  a root descriptor for each buffer binding, and `rootIndex` finds a
  binding's parameter.
- Vertex inputs have the semantic `LOC` with the location as its index.

## Not yet

- Textures and samplers in shaders, which need descriptor tables; only
  buffers bind, as root descriptors.
- DXIL compiled during the Haxe build: the HLSL is compiled when the
  program loads it.
- Enhanced barriers: transitions use resource states.
- Presenting to a window.

## Building from source

Requires the [hxn](https://github.com/hx-n/hxn) CLI, Rust, Haxe 4.3, hxnsl
and Windows. `hxn check` builds the binding and runs `examples/check` on each
runtime named, and fails unless every run passes and prints the same output.
CI runs it with WARP, Direct3D 12's software rasterizer:

```
hxn check hl --vm ash
hxn check cpp cppia
```

## Agent skill

[`skills/hxn-gpu-d3d12/SKILL.md`](https://github.com/hx-n/hxn-gpu-d3d12/blob/main/skills/hxn-gpu-d3d12/SKILL.md)
describes the library for coding agents. The haxelib includes it.

## License

Apache-2.0.
