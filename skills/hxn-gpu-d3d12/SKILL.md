---
name: hxn-gpu-d3d12
description: Drive the GPU from Haxe through Direct3D 12 with the hxn-gpu-d3d12 haxelib (package `gpu.d3d12`) on Ash, HashLink, hxcpp and CPPIA, on Windows. Use when a Haxe program runs compute shaders or renders with Direct3D 12, records resource state transitions, or uses HXSL shaders from hxnsl as HLSL.
---

# hxn-gpu-d3d12

## Setup

```
haxelib install hxn-gpu-d3d12
haxelib install hxnsl
haxe -lib hxn-gpu-d3d12 -lib hxnsl -main Main -hl main.hl     # Ash or HashLink
haxe -lib hxn-gpu-d3d12 -lib hxnsl -main Main -cpp out        # hxcpp
```

Windows x86-64; a program needs no Rust.

## Use

Shaders are hxnsl classes implementing `gpu.d3d12.Shader`; they get `HLSL`,
`HLSL_SLOTS` and `LAYOUT`. Entry points are `vertex_main`, `fragment_main`
and `compute_main`.

```haxe
import gpu.d3d12.*;

var factory = Factory.create();
var device = factory.createDevice(0);            // isSoftware(i) tells WARP apart
var cs = device.compile(Tripled.HLSL, "compute_main", "cs_5_1");
var signature = device.createRootSignature(gpu.d3d12.Shaders.rootParameters(Tripled.HLSL_SLOTS, Tripled.LAYOUT));
var pipeline = device.createComputePipeline(cs, signature);
var upload = device.createBufferWith(bytes);                         // upload heap
var data = device.createBuffer(n, Heap.Default, State.Common, true); // allows UAV
var readback = device.createBuffer(n, Heap.Readback, 0, false);

var commands = device.createCommandList();       // open for recording
commands.transitionBuffer(data, State.Common, State.CopyDest);
commands.copyBuffer(data, upload);
commands.transitionBuffer(data, State.CopyDest, State.UnorderedAccess);
commands.setComputeRootSignature(signature);
commands.setPipelineState(pipeline);
commands.setComputeRootBuffer(gpu.d3d12.Shaders.rootIndex(Tripled.HLSL_SLOTS, group, binding), ParameterKind.UnorderedAccess, data);
commands.dispatch(groups, 1, 1);
commands.transitionBuffer(data, State.UnorderedAccess, State.CopySource);
commands.copyBuffer(readback, data);
device.execute(commands).then(_ -> use(readback.read(0, n)));   // or executeAndWait
```

Render: `createTexture({width, height, format: Format.Rgba8Unorm, usage: TextureUsage.RenderTarget})`,
`transitionTexture(t, State.Common, State.RenderTarget)`, `setRenderTarget(t, r, g, b, a)`
(clears, sets viewport and scissor), `setGraphicsRootSignature`, `setPipelineState`,
`setGraphicsRootBuffer(index, ParameterKind.ConstantBuffer, uniforms)`, `setTopology`,
`setVertexBuffer(0, vertices, stride)`, `draw(count, instances, first)`; then
`transitionTexture(t, State.RenderTarget, State.CopySource)` and
`copyTextureToBuffer(t, readback)`, whose rows are `t.copyPitch()` bytes apart.

## Rules

- Synchronization is explicit: transition a resource into the state each
  use needs before that use. `uavBarrier` between unordered-access writes and
  later unordered access of the same buffer.
- Only buffers bind in this version, as root descriptors: uniform blocks as
  `ConstantBuffer`, storage buffers as `UnorderedAccess`, read-only storage
  as `ShaderResource`. Textures and samplers are not supported yet.
- HLSL compiles when the program calls `compile`, to shader model 5.1.
- Upload buffers are written, readback buffers read; default-heap buffers
  only through copies.
- A command list is recording until `execute`; after that, make a new one.
- Every object holds a Direct3D 12 object until `close()`.
- `execute`'s future delivers through `hxn.Loop`; a custom frame loop sets
  `hxn.Loop.manual = true` and calls `hxn.Loop.sync()` each frame.

## Reference

The generated classes are committed under `binding/gpu/d3d12/` in
https://github.com/hx-n/hxn-gpu-d3d12. `examples/check/Main.hx` is a
complete compute and render program with its transitions.

## Working on this repository

Windows only. Test with the hxn CLI and hxnsl: `hxn check hl --vm ash` and
`hxn check cpp cppia`. On another platform, `cargo check --target
x86_64-pc-windows-msvc` and `hxn build hl --target x86_64-pc-windows-msvc`
compile it.
