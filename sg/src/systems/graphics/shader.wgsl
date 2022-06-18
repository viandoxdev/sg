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

struct DiretionalLights {
    @size(16)
    length: u32,
    lights: array<DiretionalLight, {{LIGHTS_MAX}}>,
}
struct PointLights {
    @size(16)
    length: u32,
    lights: array<PointLight, {{LIGHTS_MAX}}>,
}
struct SpotLights {
    @size(16)
    length: u32,
    lights: array<SpotLight, {{LIGHTS_MAX}}>,
}
struct CameraInfo {
    view_proj: mat4x4<f32>,
    @size(16)
    pos: vec3<f32>,
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
var g_mra: texture_2d<f32>;
@group(0) @binding(5)
var g_depth: texture_depth_2d;
@group(0) @binding(6)
var<uniform> d_lights: DiretionalLights;
@group(0) @binding(7)
var<uniform> p_lights: PointLights;
@group(0) @binding(8)
var<uniform> s_lights: SpotLights;
@group(1) @binding(0)
var<uniform> cam: CameraInfo;

let PI = 3.1415926535;

fn filmic(x: vec3<f32>) -> vec3<f32> {
  let X = max(vec3<f32>(0.0), x - 0.004);
  let result = (X * (6.2 * X + 0.5)) / (X * (6.2 * X + 1.7) + 0.06);
  return pow(result, vec3<f32>(2.2));
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32>{
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a*a;
    let ndot_h = max(dot(n, h), 0.0);
    let ndot_h2 = ndot_h * ndot_h;

    let num = a2;
    var denom = (ndot_h2 * (a2 - 1.0) + 1.0);
    denom = PI * denom * denom;

    return num / denom;
}

fn geometry_schlick_ggx(ndot_v: f32, roughness: f32) -> f32 {
    let r = (roughness + 1.0);
    let k = (r*r) / 8.0;

    let num = ndot_v;
    let denom = ndot_v * (1.0 - k) + k;

    return num / denom;
}

fn geometry_smith(n: vec3<f32> , v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let ndot_v = max(dot(n, v), 0.0);
    let ndot_l = max(dot(n, l), 0.0);
    let ggx2 = geometry_schlick_ggx(ndot_v, roughness);
    let ggx1 = geometry_schlick_ggx(ndot_l, roughness);

    return ggx1 * ggx2;
}

fn dir_light(light: DiretionalLight, normal: vec3<f32>, view_dir: vec3<f32>) -> vec4<f32>  {
    let light_dir = normalize(-light.direction);
    let diff = max(dot(normal, light_dir), 0.0);
    let diffuse = light.color  * diff;
    return diffuse;
}

fn point_light(light: PointLight, normal: vec3<f32>, albedo: vec3<f32>, metallic: f32, roughness: f32, frag_pos: vec3<f32>, view_dir: vec3<f32>) -> vec3<f32>  {
    let light_dir = normalize(light.position - frag_pos);
    let halfway = normalize(light_dir + view_dir);
    let distance = length(light.position - frag_pos);
    let attenuation = 1.0 / (distance * distance);
    let radiance = light.color.xyz * attenuation;
    // cook-torrance
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let F = fresnel_schlick(max(dot(halfway, view_dir), 0.0), f0);
    let NDF = distribution_ggx(normal, halfway, roughness);       
    let G = geometry_smith(normal, view_dir, light_dir, roughness);
    let numerator = NDF * G * F;
    let denominator = 4.0 * max(dot(normal, view_dir), 0.0) * max(dot(normal, light_dir), 0.0)  + 0.0001;
    let specular = numerator / denominator;
    let kS = F;
    let kD = (vec3<f32>(1.0) - kS) * (1.0 - metallic);
    let ndot_l = max(dot(normal, light_dir), 0.0);        
    return (kD * albedo / PI + specular) * radiance * ndot_l;
}

@fragment
fn fs_main(v_in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = textureSample(g_normals, g_sampler, v_in.uv).xyz;
    let albedo = textureSample(g_albedo, g_sampler, v_in.uv).xyz;
    let metallic = textureSample(g_mra, g_sampler, v_in.uv).x;
    let roughness = textureSample(g_mra, g_sampler, v_in.uv).y;
    let ao = textureSample(g_mra, g_sampler, v_in.uv).z;
    let pos = textureSample(g_position, g_sampler, v_in.uv).xyz;

    let view_dir = normalize(cam.pos - pos);
    var l = vec3<f32>(0.0);
    for(var i: u32 = 0u; i < p_lights.length; i++) {
        l += point_light(p_lights.lights[i], normal, albedo, metallic, roughness, pos, view_dir);
    }
    let ambiant = vec3<f32>(0.03) * albedo * ao;
    var color = l + ambiant;
    color = filmic(color);
    return vec4<f32>(color, 1.0);
}
