struct VertexInput {
	[[location(0)]] position: vec3<f32>;
	[[location(1)]] tex_coords: vec2<f32>;
};
struct VertexOutput {
	[[builtin(position)]] clip_position: vec4<f32>;
	[[location(0)]] tex_coords: vec2<f32>;
};
struct PushConstants {
	model_mat: mat4x4<f32>;
	tex_index: u32
};
var<push_constant> pc: PushConstants;

[[stage(vertex)]]
fn vs_main(model: VertexInput) -> VertexOutput {
	var out: VertexOutput;
	out.clip_position = pc.model_mat * vec4<f32>(model.position, 1.0);
	out.tex_coords = model.tex_coords;
	return out;
}

[[group(0), binding(0)]]
var t_diffuse: binding_array<texture_2d<f32>>;
[[group(0), binding(1)]]
var s_diffuse: sampler;

[[stage(fragment)]]
fn fs_main(in: VertexOutput) -> [[location(0)]] vec4<f32> {
	return textureSample(t_diffuse[pc.tex_index], s_diffuse, in.tex_coords);
}
