@group(0) @binding(0)
var<uniform> rotations: array<mat4x4<f32>, 6>;
@group(0) @binding(1)
var input: texture_cube<f32>;
@group(0) @binding(2)
var output: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(3)
var t_sampler: sampler;

let sample_delta = {{SAMPLE_DELTA}};

let PI = 3.14159265359;
let TAU = 6.28318530718;
let PI2 = 1.57079632679;

fn rad(normal: vec3<f32>) -> vec3<f32> {
    var rad = vec3<f32>(0.0, 0.0, 0.0);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    var right = cross(up, normal);
    if length(right) == 0.0 {
        right.x = 1.0;
    }
    right = normalize(right);
    up = normalize(cross(normal, right));

    var nr_samples = 0.0;
    for(var phi: f32 = 0.0; phi < TAU; phi += sample_delta) {
        for(var theta: f32 = 0.0; theta < PI2; theta += sample_delta) {
            let tangent = vec3<f32>(sin(theta) * cos(phi), sin(theta) * sin(phi), cos(theta));
            let sample_d = tangent.x * right + tangent.y * up + tangent.z * normal;
            rad += textureSampleLevel(input, t_sampler, sample_d, 0.0).xyz * cos(theta) *
            sin(theta);
            nr_samples += 1.0;
        }
    }
    return (PI * rad) / nr_samples;
}

@compute @workgroup_size({{WG_SIZE}}, {{WG_SIZE}}, 1)
fn main(@builtin(global_invocation_id) param: vec3<u32>, @builtin(num_workgroups) wgs: vec3<u32>) {
    let width = wgs.x * u32({{WG_SIZE}});
    let height = wgs.y * u32({{WG_SIZE}});
    let unorm_loc = vec2<f32>(param.xy) / vec2<f32>(f32(width), f32(height));

    let rot = rotations[param.z];
    let top_left = (vec4<f32>(-1.0,  1.0, 1.0, 0.0) * rot).xyz;
    let right =    (vec4<f32>( 2.0,  0.0, 0.0, 0.0) * rot).xyz;
    let bottom =   (vec4<f32>( 0.0, -2.0, 0.0, 0.0) * rot).xyz;

    let loc = normalize(top_left + unorm_loc.x * right + unorm_loc.y * bottom);

    let rad = vec4<f32>(rad(loc), 1.0);

    textureStore(output, vec2<i32>(param.xy), i32(param.z), rad);
}
