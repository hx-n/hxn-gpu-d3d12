package gpu.d3d12;

#if (!macro && hxnsl)
import hxnsl.Layout.HlslBinding;
import hxnsl.Layout.LayoutEntry;

/**
	Where a shader's bindings go in a root signature, laid out from its
	`HLSL_SLOTS` and `LAYOUT`: each buffer a root descriptor, in slot order;
	each bind group's textures one descriptor table; and, when the shader
	samples, two tables over the sampler heap (naga's `s0, space0` and
	`s0, space1`, comparison samplers) and a root SRV per group for its
	sampler index array at `t<group>, space255`.
**/
class RootLayout {
	/** What `Device.createRootSignature` takes. **/
	public final parameters:Array<RootParameter> = [];

	/** The sampler heap's root parameter, or -1 when the shader samples nothing. **/
	public var samplerHeap(default, null):Int = -1;

	/** The comparison sampler heap's root parameter, or -1. It points at the same heap. **/
	public var comparisonSamplerHeap(default, null):Int = -1;

	final buffers = new Map<String, Int>();
	final tables = new Map<Int, Int>();
	final offsets = new Map<String, Int>();
	final counts = new Map<Int, Int>();
	final indexArrays = new Map<Int, Int>();

	/** The descriptors a sampler heap table spans: Direct3D 12's limit for a shader-visible one. **/
	public static inline final SAMPLER_HEAP_SIZE = 2048;

	public function new(slots:Array<HlslBinding>, layout:Array<Array<LayoutEntry>>) {
		var textures = new Map<Int, Array<RootRange>>();
		var sampled = new Map<Int, Bool>();
		for (slot in slots) {
			if (slot.kind == "sampler-index") {
				sampled.set(slot.group, true);
				continue;
			}
			var type = entry(layout, slot.group, slot.binding).type;
			switch (type) {
				case "uniform" | "storage" | "read-only-storage":
					buffers.set('${slot.group}:${slot.binding}', parameters.length);
					parameters.push({
						kind: switch (type) {
							case "uniform": ParameterKind.ConstantBuffer;
							case "storage": ParameterKind.UnorderedAccess;
							default: ParameterKind.ShaderResource;
						},
						register: slot.index,
						space: slot.space,
						visibility: Visibility.All,
						ranges: []
					});
				case "texture" | "storage-texture":
					var ranges = textures.exists(slot.group) ? textures.get(slot.group) : [];
					offsets.set('${slot.group}:${slot.binding}', ranges.length);
					ranges.push({
						kind: type == "texture" ? RangeKind.ShaderResource : RangeKind.UnorderedAccess,
						count: 1,
						register: slot.index,
						space: slot.space,
						offset: ranges.length
					});
					textures.set(slot.group, ranges);
				case other:
					throw 'an hxnsl binding of type $other has no place in a root signature';
			}
		}
		var groups = [for (g in textures.keys()) g];
		groups.sort(Reflect.compare);
		for (g in groups) {
			tables.set(g, parameters.length);
			counts.set(g, textures.get(g).length);
			parameters.push({kind: ParameterKind.DescriptorTable, register: 0, space: 0, visibility: Visibility.All, ranges: textures.get(g)});
		}
		var samplerGroups = [for (g in sampled.keys()) g];
		if (samplerGroups.length > 0) {
			samplerHeap = parameters.length;
			parameters.push(heapTable(0));
			comparisonSamplerHeap = parameters.length;
			parameters.push(heapTable(1));
			samplerGroups.sort(Reflect.compare);
			for (g in samplerGroups) {
				indexArrays.set(g, parameters.length);
				parameters.push({kind: ParameterKind.ShaderResource, register: g, space: 255, visibility: Visibility.All, ranges: []});
			}
		}
	}

	static function heapTable(space:Int):RootParameter {
		return {
			kind: ParameterKind.DescriptorTable,
			register: 0,
			space: 0,
			visibility: Visibility.All,
			ranges: [{kind: RangeKind.Sampler, count: SAMPLER_HEAP_SIZE, register: 0, space: space, offset: 0}]
		};
	}

	/** The root parameter of the buffer at `group` and `binding`. **/
	public function buffer(group:Int, binding:Int):Int {
		return found(buffers, '$group:$binding', 'no buffer at binding $binding of group $group');
	}

	/** The root parameter of group `group`'s texture table. **/
	public function textureTable(group:Int):Int {
		return found(tables, group, 'group $group has no textures');
	}

	/** Where the texture at `group` and `binding` sits in its group's table. **/
	public function textureOffset(group:Int, binding:Int):Int {
		return found(offsets, '$group:$binding', 'no texture at binding $binding of group $group');
	}

	/** How many descriptors group `group`'s texture table holds. **/
	public function textureCount(group:Int):Int {
		return found(counts, group, 'group $group has no textures');
	}

	/** The root parameter of group `group`'s sampler index array. **/
	public function samplerIndices(group:Int):Int {
		return found(indexArrays, group, 'group $group samples nothing');
	}

	/**
		A group's sampler index array: for each sampler binding in `heap`, the
		sampler heap descriptor of the sampler bound there. Put it in an
		upload buffer bound at `samplerIndices(group)`.
	**/
	public static function samplerIndexArray(heap:Map<Int, Int>):haxe.io.Bytes {
		var last = -1;
		for (binding in heap.keys())
			if (binding > last)
				last = binding;
		var bytes = haxe.io.Bytes.alloc((last + 1) * 4);
		for (binding => descriptor in heap)
			bytes.setInt32(binding * 4, descriptor);
		return bytes;
	}

	static function found<K>(map:Map<K, Int>, key:K, missing:String):Int {
		var value = map.get(key);
		if (value == null)
			throw missing;
		return value;
	}

	static function entry(layout:Array<Array<LayoutEntry>>, group:Int, binding:Int):LayoutEntry {
		var found = group < layout.length ? Lambda.find(layout[group], e -> e.binding == binding) : null;
		if (found == null)
			throw 'no binding $binding in group $group';
		return found;
	}
}
#end
