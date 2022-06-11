struct VertexInput {
	@location(0) position: vec3<f32>,
	@location(1) normal: vec3<f32>,
	@location(2) tex_coords: vec2<f32>,
}
struct VertexOutput {
	@builtin(position) clip_position: vec4<f32>,
	@location(0) normal: vec3<f32>,
	@location(1) tex_coords: vec2<f32>,
	@location(2) world_position: vec3<f32>,
}
struct PushConstants {
	model_mat: mat4x4<f32>,
	normal_mat: mat4x4<f32>,
}
var<push_constant> pc: PushConstants;

@group(1) @binding(0)
var<uniform> view_projection: mat4x4<f32>;

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
	var v_out: VertexOutput;
	v_out.world_position = (pc.model_mat * vec4<f32>(model.position, 1.0)).xyz;
	v_out.clip_position = view_projection * vec4<f32>(v_out.world_position, 1.0);
	v_out.tex_coords = model.tex_coords;
	v_out.normal = normalize((pc.normal_mat * vec4<f32>(model.normal, 0.0)).xyz);
	return v_out;
}

@group(0) @binding(0)
var textures: binding_array<texture_2d<f32>>;
@group(0) @binding(1)
var smpl: sampler;

struct FragmentOutput {
	@location(0) albedo: vec4<f32>,
	@location(1) position: vec4<f32>,
	@location(2) normal: vec4<f32>,
}

@fragment
fn fs_main(v_in: VertexOutput) -> FragmentOutput {
	var f_out: FragmentOutput;
	f_out.albedo = textureSample(textures[0], smpl, v_in.tex_coords);
	f_out.position = vec4<f32>(v_in.world_position, 0.0);
	f_out.normal = vec4<f32>(v_in.normal, 0.0);
	return f_out;
}
