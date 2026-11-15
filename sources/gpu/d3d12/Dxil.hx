package gpu.d3d12;

#if (macro && hxnsl)
import haxe.io.Bytes;
import haxe.macro.Context;
import haxe.macro.Expr;
import sys.io.Process;

/**
	An hxnsl target that compiles each of a shader's entry points to DXIL
	at compile time, for shader model 6.0, with DXC: the one
	`HXN_GPU_D3D12_DXC` names, else `dxc` on the path. DXC 1.8 signs the
	DXIL it writes on any platform, as Direct3D 12 requires. Without DXC the
	shader's `dxil()` is null, and the program compiles its HLSL when it
	loads it.
**/
class Dxil extends hxnsl.Target {
	static var compiler:Null<String>;
	static var version:Null<String>;
	static var looked = false;
	static var warned = false;

	public function new() {
		super("dxil");
	}

	/** DXC and its version line, or null without one. **/
	static function find():Null<String> {
		if (looked)
			return compiler;
		looked = true;
		var named = Sys.getEnv("HXN_GPU_D3D12_DXC");
		var candidate = named != null && named != "" ? named : "dxc";
		try {
			var p = new Process(candidate, ["--version"]);
			var out = p.stdout.readAll().toString();
			p.stderr.readAll();
			if (p.exitCode() == 0) {
				compiler = candidate;
				version = StringTools.trim(out.split("\n")[0]);
			}
			p.close();
		} catch (_:Dynamic) {}
		return compiler;
	}

	static function profile(stage:String):String {
		return switch (stage) {
			case "vertex": "vs_6_0";
			case "fragment": "ps_6_0";
			default: "cs_6_0";
		}
	}

	override public function build(s:hxnsl.Compiled):Array<Field> {
		var dxc = find();
		var cases:Array<Case> = [];
		if (dxc != null) {
			for (e in s.entryPoints) {
				var name:String = e.name;
				var target = profile(e.stage);
				var out = s.step('$version $target $name', ["shader.hlsl" => Bytes.ofString(s.hlsl)], dir -> {
					var p = new Process(dxc, ["-T", target, "-E", name, '$dir/shader.hlsl', "-Fo", '$dir/$name.dxil']);
					p.stdout.readAll();
					var errors = p.stderr.readAll().toString();
					var code = p.exitCode();
					p.close();
					if (code != 0)
						s.error('DXC could not compile $name: $errors');
				}, ['$name.dxil']);
				var resource = s.resource('$name.dxil', out.get('$name.dxil'));
				cases.push({values: [macro $v{name}], expr: macro haxe.Resource.getBytes($v{resource})});
			}
		} else if (!warned) {
			warned = true;
			Context.warning("hxn-gpu-d3d12: no DXC (set HXN_GPU_D3D12_DXC, or put dxc on the path); shaders are compiled from HLSL when the program loads them",
				s.pos);
		}
		var body:Expr = {expr: ESwitch(macro entry, cases, macro null), pos: s.pos};
		return [{
			name: "dxil",
			doc: "The entry point `entry` as signed DXIL for shader model 6.0, or null where DXC was not found at compile time.",
			access: [APublic, AStatic],
			kind: FFun({args: [{name: "entry", type: macro :String}], ret: macro :Null<haxe.io.Bytes>, expr: macro return $body}),
			pos: s.pos
		}];
	}
}
#end
