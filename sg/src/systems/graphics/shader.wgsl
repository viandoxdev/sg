struct VertexOutput {
	@builtin(position) clip_position: vec4<f32>,
	@location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
	var v_out: VertexOutput;
	v_out.uv = vec2<f32>((vertex_index << 1) & 2, vertex_index & 2);
	v_out.clip_position = vec4<f32>(v_out.uv * 2.0 - 1.0, 0.0, 1.0);
	return v_out;
}

@fragment
fn fs_main(v_in: VertexOutput) -> @location(0) vec4<f32> {
	return vec4(1.0, 0.0, 0.0, 1.0);
}
