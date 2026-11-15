package gpu.d3d12;

/** Shaders from hxnsl, for Direct3D 12. **/
class Shaders {
	#if macro
	/**
		Registers hxnsl's HLSL target for `gpu.d3d12.Shader`. The package's
		extraParams.hxml runs it; without hxnsl it does nothing.
	**/
	public static function register():Void {
		#if hxnsl
		hxnsl.Targets.register(new hxnsl.targets.Hlsl(), "gpu.d3d12.Shader");
		hxnsl.Targets.register(new gpu.d3d12.Dxil(), "gpu.d3d12.Shader");
		#end
	}
	#elseif hxnsl

	/**
		An entry point's bytecode: its DXIL when the build made it, so
		nothing is compiled now, else its HLSL compiled now to shader model
		5.1. `dxil` is the shader's `dxil(entry)`, `stage` vertex, fragment or
		compute.
	**/
	public static function bytecode(device:Device, dxil:Null<haxe.io.Bytes>, hlsl:String, entry:String, stage:String):haxe.io.Bytes {
		if (dxil != null)
			return dxil;
		var profile = switch (stage) {
			case "vertex": "vs_5_1";
			case "fragment": "ps_5_1";
			default: "cs_5_1";
		}
		return device.compile(hlsl, entry, profile);
	}

	#end
}
