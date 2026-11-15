import gpu.d3d12.*;
import window.Window;
import window.WindowAttributes;

/**
	Presents to a window: a swap chain on an hxn-window window's HWND,
	three frames each cleared to one color and presented, the last copied
	out first and checked.
**/
class Main {
	static function main() {
		var attributes = new WindowAttributes();
		attributes.title("hxn-gpu-d3d12 present");
		attributes.width(64);
		attributes.height(64);
		var window = Window.open(attributes);
		attributes.close();

		var factory = Factory.create();
		var chosen = factory.adapterNames().length - 1;
		for (i in 0...factory.adapterNames().length)
			if (!factory.isSoftware(i)) {
				chosen = i;
				break;
			}
		var device = factory.createDevice(chosen);
		var size = 64;
		var chain = factory.createSwapchain(device, window.raw(0), size, size, Format.Bgra8Unorm, 2);
		// Each frame's color, as a Bgra8 pixel reads back.
		var colors = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
		for (frame in 0...colors.length) {
			var target = chain.buffer(chain.currentIndex());
			var c = colors[frame];
			var commands = device.createCommandList();
			commands.transitionTexture(target, State.Common, State.RenderTarget);
			commands.setRenderTarget(target, c[0], c[1], c[2], 1);
			var last = frame == colors.length - 1;
			var readback = device.createBuffer(target.copyPitch() * size, Heap.Readback, 0, false);
			if (last) {
				commands.transitionTexture(target, State.RenderTarget, State.CopySource);
				commands.copyTextureToBuffer(target, readback);
				commands.transitionTexture(target, State.CopySource, State.Common);
			} else
				commands.transitionTexture(target, State.RenderTarget, State.Common);
			device.executeAndWait(commands);
			chain.present(0);
			if (last) {
				var pixel = readback.read(0, 4);
				var expected = [for (v in c) Math.round(v * 255)];
				// Bgra8: blue, green, red, alpha.
				if (pixel.get(2) != expected[0] || pixel.get(1) != expected[1] || pixel.get(0) != expected[2])
					throw 'the last frame is ${pixel.get(2)}, ${pixel.get(1)}, ${pixel.get(0)}, not $expected';
				Sys.println('present: ${colors.length} frames to a ${size}x$size window, the last ${expected.join(", ")}');
			}
			readback.close();
			commands.close();
			target.close();
		}
		chain.close();
		device.close();
		factory.close();
		window.close();
	}
}
