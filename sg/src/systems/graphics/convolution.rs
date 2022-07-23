use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::include_shader;

use super::{pipeline::ComputePipeline, GraphicContext, cubemap::get_cubemap_face_rotations_buffer};

pub struct ConvolutionComputer {
    pipeline: ComputePipeline,
    workgroups_size: u32,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl ConvolutionComputer {
    const SAMPLE_DELTA: f64 = 0.01;
    pub fn new(ctx: &GraphicContext) -> Self {
        let mut shader = include_shader!("convolution.wgsl", "Convolution Shader");
        let wgs = f64::from(
                ctx.device.limits().max_compute_workgroup_size_x
                    .max(ctx.device.limits().max_compute_workgroup_size_y)
            ).sqrt()
            .floor() as u32;
        shader.set("WG_SIZE", i64::from(wgs));
        shader.set("SAMPLE_DELTA", Self::SAMPLE_DELTA);
        let bind_group_layout = create_bind_group_layout!(ctx.device, "Convolution Bind Group Layout": {
            0 => COMPUTE | Buffer(type: Uniform),
            1 => COMPUTE | Texture(sample: FloatFilterable, view_dim: Cube),
            2 => COMPUTE | StorageTexture(access: WriteOnly, format: Rgba16Float, view_dim: D2Array),
            3 => COMPUTE | Sampler(Filtering)
        });
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pipeline = ComputePipeline::new(
            &ctx.device,
            ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Convolution Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[]
            }),
            shader,
            |device, layout, module| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Convolution Pipeline"),
                    layout: Some(layout),
                    module,
                    entry_point: "main"
                })
            }
        );

        Self {
            pipeline,
            workgroups_size: wgs,
            bind_group_layout,
            sampler,
        }
    }

    pub fn run(&self, env_map: &wgpu::TextureView, size: u32, usage: wgpu::TextureUsages, ctx: &GraphicContext) -> wgpu::Texture {
        let tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6,
            },
            label: None,
            usage: wgpu::TextureUsages::STORAGE_BINDING | usage,
            format: wgpu::TextureFormat::Rgba16Float,
            dimension: wgpu::TextureDimension::D2,
            sample_count: 1,
            mip_level_count: 1,
        });

        let view = tex.create_view(&Default::default());

        let bind_group = create_bind_group!(ctx.device, &self.bind_group_layout, "Convolution Bind Group": {
            0 | Buffer(buffer: (get_cubemap_face_rotations_buffer(&ctx.device))),
            1 | TextureView(env_map),
            2 | TextureView(&view),
            3 | Sampler(&self.sampler),
        });

        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Convolution Compute Pass")
        });
        let workgroups = (size + self.workgroups_size - 1) / self.workgroups_size;

        compute_pass.set_pipeline(&self.pipeline.pipeline);
        compute_pass.set_bind_group(0, &bind_group, &[]);
        compute_pass.dispatch_workgroups(workgroups, workgroups, 6);
        drop(compute_pass);

        
        let si = ctx.queue.submit(std::iter::once(encoder.finish()));
        ctx.device.poll(wgpu::Maintain::WaitForSubmissionIndex(si));
        tex
    }
}
