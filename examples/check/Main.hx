import gpu.d3d12.*;
import haxe.io.Bytes;
import Shaders;

/**
	What CI runs: a compute shader over a buffer, and a render pass into a
	texture, each copied to a readback buffer and checked, with the resource
	transitions Direct3D 12 needs. It needs a Direct3D 12 adapter, WARP
	included, and prints the same lines on every runtime.
**/
class Main {
	static function check(ok:Bool, message:String) {
		if (!ok)
			throw message;
	}

	static function closeAll(objects:Array<{function close():Void;}>) {
		for (o in objects)
			o.close();
	}

	static function main() {
		var factory = Factory.create();
		// A hardware adapter when there is one, else WARP.
		var names = factory.adapterNames();
		var chosen = names.length - 1;
		for (i in 0...names.length)
			if (!factory.isSoftware(i)) {
				chosen = i;
				break;
			}
		var device = try factory.createDevice(chosen) catch (e:haxe.Exception) {
			Sys.println('no Direct3D 12: ${e.message}');
			return;
		}
		// The compute pass completes in a callback from the Haxe event loop, after main
		// returns, and the render pass runs from there.
		compute(device, () -> {
			render(device);
			device.close();
			factory.close();
		});
		Sys.println("compute: submitted");
	}

	static function compute(device:Device, next:() -> Void) {
		var bytecode = device.compile(Tripled.HLSL, "compute_main", "cs_5_1");
		var signature = device.createRootSignature(gpu.d3d12.Shaders.rootParameters(Tripled.HLSL_SLOTS, Tripled.LAYOUT));
		var pipeline = device.createComputePipeline(bytecode, signature);
		var count = 64;
		var input = Bytes.alloc(count * 4);
		for (i in 0...count)
			input.setInt32(i * 4, i);
		var upload = device.createBufferWith(input);
		var data = device.createBuffer(count * 4, Heap.Default, State.Common, true);
		var readback = device.createBuffer(count * 4, Heap.Readback, 0, false);

		var commands = device.createCommandList();
		commands.transitionBuffer(data, State.Common, State.CopyDest);
		commands.copyBuffer(data, upload);
		commands.transitionBuffer(data, State.CopyDest, State.UnorderedAccess);
		commands.setComputeRootSignature(signature);
		commands.setPipelineState(pipeline);
		commands.setComputeRootBuffer(gpu.d3d12.Shaders.rootIndex(Tripled.HLSL_SLOTS, Tripled.BUFFER_data_GROUP, Tripled.BUFFER_data),
			ParameterKind.UnorderedAccess, data);
		commands.dispatch(Std.int(count / Tripled.ENTRY_POINTS[0].workgroup[0]), 1, 1);
		commands.transitionBuffer(data, State.UnorderedAccess, State.CopySource);
		commands.copyBuffer(readback, data);
		device.execute(commands).then(_ -> {
			var output = readback.read(0, count * 4);
			for (i in 0...count)
				check(output.getInt32(i * 4) == i * 3 + 1, 'value $i is ${output.getInt32(i * 4)}, not ${i * 3 + 1}');
			Sys.println('compute: $count values tripled plus one, the last ${output.getInt32((count - 1) * 4)}');
			closeAll([commands, readback, data, upload, pipeline, signature]);
			next();
		}, e -> {
			Sys.println('compute failed: ${e.message}');
			hxn.Loop.exit(1);
		});
	}

	static function render(device:Device) {
		var vertex = device.compile(Tinted.HLSL, "vertex_main", "vs_5_1");
		var pixel = device.compile(Tinted.HLSL, "fragment_main", "ps_5_1");
		var signature = device.createRootSignature(gpu.d3d12.Shaders.rootParameters(Tinted.HLSL_SLOTS, Tinted.LAYOUT));
		var pipeline = device.createGraphicsPipeline(vertex, pixel, signature, Format.Rgba8Unorm, Topology.TriangleStrip,
			[{location: Tinted.INPUT_position, slot: 0, format: Format.Rg32Float, offset: 0}]);
		var size = 4;
		var target = device.createTexture({width: size, height: size, format: Format.Rgba8Unorm, usage: TextureUsage.RenderTarget});
		var pitch = target.copyPitch();
		var readback = device.createBuffer(pitch * size, Heap.Readback, 0, false);

		var quad = Bytes.alloc(4 * 8);
		var corners = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
		for (i in 0...corners.length)
			quad.setFloat(i * 4, corners[i]);
		var positions = device.createBufferWith(quad);
		var params = Bytes.alloc(Tinted.PARAMS_SIZE);
		var tint = [0.25, 0.5, 0.75, 1.0];
		for (i in 0...4)
			params.setFloat(Tinted.PARAMS_tint + i * 4, tint[i]);
		var uniforms = device.createBufferWith(params);

		var commands = device.createCommandList();
		commands.transitionTexture(target, State.Common, State.RenderTarget);
		commands.setRenderTarget(target, 0, 0, 0, 0);
		commands.setGraphicsRootSignature(signature);
		commands.setPipelineState(pipeline);
		commands.setGraphicsRootBuffer(gpu.d3d12.Shaders.rootIndex(Tinted.HLSL_SLOTS, Tinted.PARAMS_GROUP, Tinted.PARAMS_BINDING),
			ParameterKind.ConstantBuffer, uniforms);
		commands.setTopology(Topology.TriangleStrip);
		commands.setVertexBuffer(0, positions, 8);
		commands.draw(4, 1, 0);
		// The rendered texture, moved for the copy into the readback buffer.
		commands.transitionTexture(target, State.RenderTarget, State.CopySource);
		commands.copyTextureToBuffer(target, readback);
		device.executeAndWait(commands);

		var pixels = readback.read(0, pitch * size);
		var expected = [for (c in tint) Math.round(c * 255)];
		for (y in 0...size)
			for (x in 0...size)
				for (c in 0...4) {
					var at = y * pitch + x * 4 + c;
					check(pixels.get(at) == expected[c], 'pixel $x,$y channel $c is ${pixels.get(at)}, not ${expected[c]}');
				}
		Sys.println('render: ${size}x$size pixels of ${[for (c in 0...4) pixels.get(c)].join(", ")}');
		closeAll([commands, uniforms, positions, readback, target, pipeline, signature]);
	}
}
