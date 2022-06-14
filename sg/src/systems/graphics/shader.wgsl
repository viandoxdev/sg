struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var v_out: VertexOutput;
    v_out.uv = vec2<f32>(f32((vertex_index << 1u) & 2u), f32(vertex_index & 2u));
    v_out.clip_position = vec4<f32>(v_out.uv * 2.0 - 1.0, 0.0, 1.0);
    return v_out;
}

struct DiretionalLight {
    @size(16)
    direction: vec3<f32>,
    color: vec4<f32>
}

struct PointLight {
    @size(16)
    position: vec3<f32>,
    color: vec4<f32>
}

struct SpotLight {
    @size(16)
    position: vec3<f32>,
    direction_cutoff: vec4<f32>,
    color: vec4<f32>
}

@group(0) @binding(0)
var g_sampler: sampler;
@group(0) @binding(1)
var g_albedo: texture_2d<f32>;
@group(0) @binding(2)
var g_position: texture_2d<f32>;
@group(0) @binding(3)
var g_normals: texture_2d<f32>;
@group(0) @binding(4)
var g_depth: texture_depth_2d;
@group(0) @binding(5)
var<uniform> d_lights: array<DiretionalLight, {{LIGHTS_LENGTH}}>;
@group(0) @binding(6)
var<uniform> p_lights: array<PointLight, {{LIGHTS_LENGTH}}>;
@group(0) @binding(7)
var<uniform> s_lights: array<SpotLight, {{LIGHTS_LENGTH}}>;

@fragment
fn fs_main(v_in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(vec3(1.0, 1.0, 1.0) * textureSample(g_depth, g_sampler, v_in.uv), 1.0);
}
