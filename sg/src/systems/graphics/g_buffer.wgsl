struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_coords: vec2<f32>,
    @location(3) tangent: vec3<f32>,
}
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) TBN: mat3x3<f32>,
}
struct CameraInfo {
    view_proj: mat4x4<f32>,
    @size(16)
    pos: vec3<f32>,
}
struct PushConstants {
    model_mat: mat4x4<f32>,
    normal_mat: mat4x4<f32>,
}
var<push_constant> pc: PushConstants;

@group(1) @binding(0)
var<uniform> cam: CameraInfo;

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    let normal = normalize((pc.normal_mat * vec4<f32>(model.normal, 0.0)).xyz);
    var tangent = normalize((pc.normal_mat * vec4<f32>(model.tangent, 0.0)).xyz);
    tangent = normalize(tangent - dot(tangent, normal) * normal);
    let bitangent = -cross(normal, tangent);
    let TBN = mat3x3<f32>(tangent, bitangent, normal);

    var v_out: VertexOutput;
    v_out.world_position = (pc.model_mat * vec4<f32>(model.position, 1.0)).xyz;
    v_out.clip_position = cam.view_proj * vec4<f32>(v_out.world_position, 1.0);
    v_out.tex_coords = model.tex_coords;
    v_out.TBN = TBN;
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
    @location(3) mra: vec4<f32>,
}

@fragment
fn fs_main(v_in: VertexOutput) -> FragmentOutput {
    var f_out: FragmentOutput;
    let normal = normalize(v_in.TBN * (
        textureSample(textures[1], smpl, v_in.tex_coords).xyz * 2.0 - vec3<f32>(1.0)
    ));
    f_out.albedo = textureSample(textures[0], smpl, v_in.tex_coords);
    f_out.position = vec4<f32>(v_in.world_position, 1.0);
    f_out.normal = vec4<f32>(normal, 1.0);
    // metallic
    f_out.mra.x = textureSample(textures[2], smpl, v_in.tex_coords).x;
    // roughness
    f_out.mra.y = textureSample(textures[3], smpl, v_in.tex_coords).x;
    // ambiant occlusion
    f_out.mra.z = textureSample(textures[4], smpl, v_in.tex_coords).x;
    f_out.mra.w = 1.0;
    return f_out;
}
