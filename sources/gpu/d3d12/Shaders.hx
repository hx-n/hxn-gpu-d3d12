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
		#end
	}
	#elseif hxnsl

	/**
		A root descriptor for each of a shader's buffers, in `slots` order:
		what `Device.createRootSignature` takes. `slots` and `layout` are the
		shader's `HLSL_SLOTS` and `LAYOUT`. Textures and samplers need
		descriptor tables, which this does not make yet.
	**/
	public static function rootParameters(slots:Array<hxnsl.Layout.HlslBinding>, layout:Array<Array<hxnsl.Layout.LayoutEntry>>):Array<RootParameter> {
		return [
			for (slot in slots)
				{
					kind: kind(entry(layout, slot.group, slot.binding).type),
					register: slot.index,
					space: slot.space,
					visibility: Visibility.All
				}
		];
	}

	/** The root parameter `rootParameters` gave the binding at `group` and `binding`. **/
	public static function rootIndex(slots:Array<hxnsl.Layout.HlslBinding>, group:Int, binding:Int):Int {
		for (i in 0...slots.length)
			if (slots[i].group == group && slots[i].binding == binding)
				return i;
		throw 'no binding $binding in group $group';
	}

	/** The root descriptor kind of an hxnsl buffer type. **/
	public static function kind(type:String):ParameterKind {
		return switch (type) {
			case "uniform": ParameterKind.ConstantBuffer;
			case "storage": ParameterKind.UnorderedAccess;
			case "read-only-storage": ParameterKind.ShaderResource;
			case other: throw 'a $other binding needs a descriptor table, which hxn-gpu-d3d12 does not make yet';
		}
	}

	static function entry(layout:Array<Array<hxnsl.Layout.LayoutEntry>>, group:Int, binding:Int):hxnsl.Layout.LayoutEntry {
		var found = group < layout.length ? Lambda.find(layout[group], e -> e.binding == binding) : null;
		if (found == null)
			throw 'no binding $binding in group $group';
		return found;
	}
	#end
}
