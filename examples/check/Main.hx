import hxn.gpu.d3d12.*;
import hxn.gpu.d3d12.Shaders as D3D12Shaders;
import haxe.io.Bytes;
import Shaders;

/**
	What CI runs: a compute shader over a buffer, a render pass into a
	texture, and a pass sampling a texture, each copied to a readback buffer
	and checked, with the resource transitions Direct3D 12 needs; then the
	compute pass again with enhanced barriers where the device takes them.
	It needs a Direct3D 12 adapter, WARP included, and prints the same lines
	on every runtime.
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
		// returns, and the other passes run from there.
		compute(device, false, () -> {
			render(device);
			textured(device);
			if (device.supportsEnhancedBarriers())
				compute(device, true, () -> {
					device.close();
					factory.close();
				});
			else {
				Sys.println("enhanced barriers: not supported");
				device.close();
				factory.close();
			}
		});
		Sys.println("compute: submitted");
	}

	static function compute(device:Device, enhanced:Bool, next:() -> Void) {
		var bytecode = D3D12Shaders.bytecode(device, Tripled.dxil("compute_main"), Tripled.HLSL, "compute_main", "compute");
		var layout = new RootLayout(Tripled.HLSL_SLOTS, Tripled.LAYOUT);
		var signature = device.createRootSignature(layout.parameters);
		var pipeline = device.createComputePipeline(bytecode, signature);
		var count = 64;
		var input = Bytes.alloc(count * 4);
		for (i in 0...count)
			input.setInt32(i * 4, i);
		var upload = device.createBufferWith(input);
		var data = device.createBuffer(count * 4, Heap.Default, State.Common, true);
		var readback = device.createBuffer(count * 4, Heap.Readback, 0, false);

		var commands = device.createCommandList();
		if (enhanced)
			commands.bufferBarrier(data, BarrierSync.None, Access.NoAccess, BarrierSync.Copy, Access.CopyDest);
		else
			commands.transitionBuffer(data, State.Common, State.CopyDest);
		commands.copyBuffer(data, upload);
		if (enhanced)
			commands.bufferBarrier(data, BarrierSync.Copy, Access.CopyDest, BarrierSync.ComputeShading, Access.UnorderedAccess);
		else
			commands.transitionBuffer(data, State.CopyDest, State.UnorderedAccess);
		commands.setComputeRootSignature(signature);
		commands.setPipelineState(pipeline);
		commands.setComputeRootBuffer(layout.buffer(Tripled.BUFFER_data_GROUP, Tripled.BUFFER_data), ParameterKind.UnorderedAccess, data);
		commands.dispatch(Std.int(count / Tripled.ENTRY_POINTS[0].workgroup[0]), 1, 1);
		if (enhanced)
			commands.bufferBarrier(data, BarrierSync.ComputeShading, Access.UnorderedAccess, BarrierSync.Copy, Access.CopySource);
		else
			commands.transitionBuffer(data, State.UnorderedAccess, State.CopySource);
		commands.copyBuffer(readback, data);
		device.execute(commands).then(_ -> {
			var output = readback.read(0, count * 4);
			for (i in 0...count)
				check(output.getInt32(i * 4) == i * 3 + 1, 'value $i is ${output.getInt32(i * 4)}, not ${i * 3 + 1}');
			if (enhanced)
				Sys.println('enhanced barriers: $count values tripled plus one');
			else
				Sys.println('compute: $count values tripled plus one, the last ${output.getInt32((count - 1) * 4)}');
			closeAll([commands, readback, data, upload, pipeline, signature]);
			next();
		}, e -> {
			Sys.println('compute failed: ${e.message}');
			hxn.Loop.exit(1);
		});
	}

	/** Reads a 4x4 Rgba8 target back, and checks every pixel is `expected`, within `tolerance`. **/
	static function readTarget(device:Device, commands:CommandList, target:Texture, expected:Array<Int>, label:String, tolerance = 0) {
		var size = target.width();
		var pitch = target.copyPitch();
		var readback = device.createBuffer(pitch * size, Heap.Readback, 0, false);
		commands.transitionTexture(target, State.RenderTarget, State.CopySource);
		commands.copyTextureToBuffer(target, readback);
		device.executeAndWait(commands);
		var pixels = readback.read(0, pitch * size);
		for (y in 0...size)
			for (x in 0...size)
				for (c in 0...4) {
					var at = y * pitch + x * 4 + c;
					check(Math.abs(pixels.get(at) - expected[c]) <= tolerance, '$label: pixel $x,$y channel $c is ${pixels.get(at)}, not ${expected[c]}');
				}
		Sys.println('$label: ${size}x$size pixels of ${[for (c in 0...4) pixels.get(c)].join(", ")}');
		readback.close();
	}

	// Blending is off: the quad's color replaces the target's.
	static final REPLACE:Blend = {
		enabled: false,
		colorOperation: 0,
		colorSource: 1,
		colorDestination: 0,
		alphaOperation: 0,
		alphaSource: 1,
		alphaDestination: 0
	};

	// Straight alpha: color = source * source alpha + destination * (1 - source alpha),
	// and alpha = source + destination * (1 - source alpha). The shared factor codes 4
	// (SrcAlpha), 5 (OneMinusSrcAlpha) and 1 (One), and the operation 0 (Add).
	static final ALPHA:Blend = {
		enabled: true,
		colorOperation: 0,
		colorSource: 4,
		colorDestination: 5,
		alphaOperation: 0,
		alphaSource: 1,
		alphaDestination: 5
	};

	static function render(device:Device) {
		var tint = [0.25, 0.5, 0.75, 1.0];
		quad(device, tint, [0.0, 0.0, 0.0, 0.0], REPLACE, [for (c in tint) Math.round(c * 255)], "render", 0);
		// A half-transparent red quad over a blue target: half red, half blue, opaque.
		quad(device, [1.0, 0.0, 0.0, 0.5], [0.0, 0.0, 1.0, 1.0], ALPHA, [128, 0, 128, 255], "blend", 1);
	}

	// Draws a quad of `tint` into a 4x4 target cleared to `clear`, with `blend`, and checks
	// every pixel is `expected` within `tolerance`.
	static function quad(device:Device, tint:Array<Float>, clear:Array<Float>, blend:Blend, expected:Array<Int>, label:String, tolerance:Int) {
		var vertex = D3D12Shaders.bytecode(device, Tinted.dxil("vertex_main"), Tinted.HLSL, "vertex_main", "vertex");
		var pixel = D3D12Shaders.bytecode(device, Tinted.dxil("fragment_main"), Tinted.HLSL, "fragment_main", "fragment");
		var layout = new RootLayout(Tinted.HLSL_SLOTS, Tinted.LAYOUT);
		var signature = device.createRootSignature(layout.parameters);
		var pipeline = device.createGraphicsPipeline(vertex, pixel, signature, Format.Rgba8Unorm, Topology.TriangleStrip,
			[{location: Tinted.INPUT_position, slot: 0, format: Format.Rg32Float, offset: 0, perInstance: false}], blend);
		var target = device.createTexture({width: 4, height: 4, format: Format.Rgba8Unorm, usage: TextureUsage.RenderTarget});

		var quad = Bytes.alloc(4 * 8);
		var corners = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
		for (i in 0...corners.length)
			quad.setFloat(i * 4, corners[i]);
		var positions = device.createBufferWith(quad);
		var params = Bytes.alloc(Tinted.PARAMS_SIZE);
		for (i in 0...4)
			params.setFloat(Tinted.PARAMS_tint + i * 4, tint[i]);
		var uniforms = device.createBufferWith(params);

		var commands = device.createCommandList();
		commands.transitionTexture(target, State.Common, State.RenderTarget);
		commands.setRenderTarget(target, false, clear[0], clear[1], clear[2], clear[3]);
		commands.setGraphicsRootSignature(signature);
		commands.setPipelineState(pipeline);
		commands.setGraphicsRootBuffer(layout.buffer(Tinted.PARAMS_GROUP, Tinted.PARAMS_BINDING), ParameterKind.ConstantBuffer, uniforms);
		commands.setTopology(Topology.TriangleStrip);
		commands.setVertexBuffer(0, positions, 8, 0);
		commands.draw(4, 1, 0);
		readTarget(device, commands, target, expected, label, tolerance);
		closeAll([commands, uniforms, positions, target, pipeline, signature]);
	}

	static function textured(device:Device) {
		var vertex = D3D12Shaders.bytecode(device, Textured.dxil("vertex_main"), Textured.HLSL, "vertex_main", "vertex");
		var pixel = D3D12Shaders.bytecode(device, Textured.dxil("fragment_main"), Textured.HLSL, "fragment_main", "fragment");
		var layout = new RootLayout(Textured.HLSL_SLOTS, Textured.LAYOUT);
		var signature = device.createRootSignature(layout.parameters);
		var pipeline = device.createGraphicsPipeline(vertex, pixel, signature, Format.Rgba8Unorm, Topology.TriangleStrip, [
			{location: Textured.INPUT_position, slot: 0, format: Format.Rg32Float, offset: 0, perInstance: false},
			{location: Textured.INPUT_uv, slot: 0, format: Format.Rg32Float, offset: 8, perInstance: false}
		], REPLACE);

		// A 2x2 image of one color, uploaded through a buffer with Direct3D 12's row pitch.
		var color = [40, 80, 120, 255];
		var image = device.createTexture({width: 2, height: 2, format: Format.Rgba8Unorm, usage: 0});
		var pixels = Bytes.alloc(image.copyPitch() * 2);
		for (y in 0...2)
			for (x in 0...2)
				for (c in 0...4)
					pixels.set(y * image.copyPitch() + x * 4 + c, color[c]);
		var staging = device.createBufferWith(pixels);

		var group = Textured.TEXTURE_image_GROUP;
		var resources = device.createDescriptorHeap(HeapKind.Resources, layout.textureCount(group));
		resources.writeTexture(layout.textureOffset(group, Textured.TEXTURE_image), image);
		var samplers = device.createDescriptorHeap(HeapKind.Samplers, 1);
		samplers.writeSampler(0, Filter.Nearest, AddressMode.Clamp);
		// The image's sampler, the next binding, is descriptor 0 of the sampler heap.
		var indices = device.createBufferWith(RootLayout.samplerIndexArray([Textured.TEXTURE_image + 1 => 0]));

		var quad = Bytes.alloc(4 * 16);
		var corners = [-1.0, -1.0, 0, 1, 1.0, -1.0, 1, 1, -1.0, 1.0, 0, 0, 1.0, 1.0, 1, 0];
		for (i in 0...corners.length)
			quad.setFloat(i * 4, corners[i]);
		var vertices = device.createBufferWith(quad);
		var target = device.createTexture({width: 4, height: 4, format: Format.Rgba8Unorm, usage: TextureUsage.RenderTarget});

		var commands = device.createCommandList();
		commands.transitionTexture(image, State.Common, State.CopyDest);
		commands.copyBufferToTexture(image, staging);
		commands.transitionTexture(image, State.CopyDest, State.PixelShaderResource);
		commands.transitionTexture(target, State.Common, State.RenderTarget);
		commands.setRenderTarget(target, false, 0, 0, 0, 0);
		commands.setGraphicsRootSignature(signature);
		commands.setPipelineState(pipeline);
		commands.setDescriptorHeaps(resources, samplers);
		commands.setGraphicsRootTable(layout.textureTable(group), resources, 0);
		commands.setGraphicsRootTable(layout.samplerHeap, samplers, 0);
		commands.setGraphicsRootTable(layout.comparisonSamplerHeap, samplers, 0);
		commands.setGraphicsRootBuffer(layout.samplerIndices(group), ParameterKind.ShaderResource, indices);
		commands.setTopology(Topology.TriangleStrip);
		commands.setVertexBuffer(0, vertices, 16, 0);
		commands.draw(4, 1, 0);
		readTarget(device, commands, target, color, "textured");
		closeAll([commands, target, vertices, indices, samplers, resources, staging, image, pipeline, signature]);
	}
}
