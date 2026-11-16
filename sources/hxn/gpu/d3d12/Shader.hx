package hxn.gpu.d3d12;

#if hxnsl
/**
	An hxnsl shader for Direct3D 12. It gets hxnsl's `HLSL`, which
	`Device.compile` turns into bytecode, and `HLSL_SLOTS`, the register of
	each binding; `Shaders.rootParameters` makes the root signature's.
**/
interface Shader extends hxnsl.Shader {}
#end
