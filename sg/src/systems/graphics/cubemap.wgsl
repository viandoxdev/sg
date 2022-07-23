@group(0) @binding(0)
var t_sampler: sampler;
@group(0) @binding(1)
var input: texture_2d<f32>;
@group(0) @binding(2)
var output: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(3)
var<uniform> rotations: array<mat4x4<f32>, 6>;

// 1/2pi
let I2PI = 0.15915494309;
let PI = 3.14159265359;

@compute @workgroup_size({{WG_SIZE}}, {{WG_SIZE}}, 1)
fn main(@builtin(global_invocation_id) param: vec3<u32>, @builtin(num_workgroups) wgs: vec3<u32>) {
    let width = wgs.x * u32({{WG_SIZE}});
    let height = wgs.y * u32({{WG_SIZE}});
    let size = vec2<f32>(f32(width), f32(height));
    let un_p = vec2<f32>(param.xy) / size;

    let rot = rotations[param.z];

    let top_left = (vec4<f32>(-1.0,  1.0, 1.0, 0.0) * rot).xyz;
    let right =    (vec4<f32>( 2.0,  0.0, 0.0, 0.0) * rot).xyz;
    let bottom =   (vec4<f32>( 0.0, -2.0, 0.0, 0.0) * rot).xyz;

    // Normalized vector pointing towards the current texel on the cube
    let p = normalize(top_left + un_p.x * right + un_p.y * bottom);
    let a = atan2(p.x, p.z);
    let l = vec2<f32>(a * I2PI + 0.5, p.y * 0.5 + 0.5);
    // SampleLevel instead of Sample because mips are not supported in compute shaders
    let color = textureSampleLevel(input, t_sampler, l, 0.0);

    textureStore(output, vec2<i32>(param.xy), i32(param.z), color);
}
