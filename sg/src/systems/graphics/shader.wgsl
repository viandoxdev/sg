struct VertexInput {
	@location(0) position: vec3<f32>,
	@location(1) tex_coords: vec2<f32>,
}
struct VertexOutput {
	@builtin(position) clip_position: vec4<f32>,
	@location(0) tex_coords: vec2<f32>,
}
struct PushConstants {
	model_mat: mat4x4<f32>,
	tex_index: u32,
}
var<push_constant> pc: PushConstants;

@group(1) @binding(0)
var<uniform> view_projection: mat4x4<f32>;

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
	var v_out: VertexOutput;
	v_out.clip_position = view_projection * pc.model_mat * vec4<f32>(model.position, 1.0);
	v_out.tex_coords = model.tex_coords;
	return v_out;
}

@group(0) @binding(0)
var t_diffuse: binding_array<texture_2d<f32>>;
@group(0) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(v_in: VertexOutput) -> @location(0) vec4<f32> {
	return textureSample(t_diffuse[pc.tex_index], s_diffuse, v_in.tex_coords);
}
