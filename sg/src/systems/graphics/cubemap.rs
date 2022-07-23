use std::lazy::{SyncLazy, SyncOnceCell};

use half::f16;
use image::GenericImageView;
use wgpu::util::DeviceExt;

use crate::include_shader;

use super::{GraphicContext, pipeline::ComputePipeline, texture_manager::TextureManager};

const CUBEMAP_FACE_ROTATION_MATRICES: [[f32; 16]; 6] = [
    // +X, rot: Y(-PI/2)
    [
        0.0, 0.0, 1.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        -1., 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 
    ], 
    // -X, rot: Y(PI/2)
    [
        0.0, 0.0, -1., 0.0,
        0.0, 1.0, 0.0, 0.0,
        1.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 
    ], 
    // +Y, rot: X(-PI/2)
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, -1., 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ], 
    // -Y, rot: X(PI/2)
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 0.0, -1., 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ], 
    // +Z, rot: none
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ], 
    // -Z, rot: Y(PI)
    [
        -1., 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, -1., 0.0,
        0.0, 0.0, 0.0, 1.0,
    ], 
];

static CUBEMAP_FACE_ROTATIONS_BUFFER: SyncOnceCell<wgpu::Buffer> = SyncOnceCell::new();

pub fn get_cubemap_face_rotations_buffer(device: &wgpu::Device) -> &wgpu::Buffer {
    CUBEMAP_FACE_ROTATIONS_BUFFER.get_or_init(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            usage: wgpu::BufferUsages::UNIFORM,
            label: None,
            contents: bytemuck::cast_slice(&CUBEMAP_FACE_ROTATION_MATRICES)
        })
    })
}

pub struct CubeMapComputer {
    pipeline: ComputePipeline,
    sampler: wgpu::Sampler,
    bindgroup_layout: wgpu::BindGroupLayout,
    workgroups_size: u32,
}

impl CubeMapComputer {
    pub fn new(ctx: &GraphicContext) -> Self {
        let mut shader = include_shader!("cubemap.wgsl", "CubeMap Shader");
        let wgs = f64::from(
                ctx.device.limits().max_compute_workgroup_size_x
                    .max(ctx.device.limits().max_compute_workgroup_size_y)
            ).sqrt()
            .floor() as u32;
        shader.set("WG_SIZE", i64::from(wgs));
        let bindgroup_layout = create_bind_group_layout!(ctx.device, "CubeMap Bindgroup Layout": {
            0 => COMPUTE | Sampler(Filtering),
            1 => COMPUTE | Texture(sample: FloatFilterable, view_dim: D2),
            2 => COMPUTE | StorageTexture(view_dim: D2Array, format: Rgba16Float, access: WriteOnly),
            3 => COMPUTE | Buffer(type: Uniform),
        });
        let pipeline = ComputePipeline::new(
            &ctx.device,
            ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("CubeMap Compute Pipeline Layout"),
                bind_group_layouts: &[
                    &bindgroup_layout
                ],
                push_constant_ranges: &[],
            }),
            shader,
            |device, layout, module| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("CubeMap Compute Pipeline"),
                    layout: Some(layout),
                    module,
                    entry_point: "main"
                })
            },
        );
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("CubeMap Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            pipeline,
            sampler,
            bindgroup_layout,
            workgroups_size: wgs
        }
    }

    pub fn render(&mut self, image: impl GenericImageView<Pixel = image::Rgba<f32>>, ctx: &GraphicContext, tex_size: u32, usage: wgpu::TextureUsages) -> wgpu::Texture {
        let bytes = image.pixels().map(|(_, _, image::Rgba(v))| [
           f16::from_f32(v[0]), f16::from_f32(v[1]), f16::from_f32(v[2]), f16::from_f32(v[3])
        ]).flatten().collect::<Vec<f16>>();

        let input_texture = TextureManager::create_texture_from_bytes(
            &ctx.device,
            &ctx.queue,
            bytemuck::cast_slice(&bytes),
            wgpu::TextureFormat::Rgba16Float,
            image.width(),
            image.height(),
            wgpu::TextureUsages::TEXTURE_BINDING,
            std::mem::size_of::<[f16;4]>() as u32,
        );

        let size = wgpu::Extent3d {
            width: tex_size,
            height: tex_size,
            depth_or_array_layers: 6,
        };

        let output_texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("CubeMap Texture"),
            size,
            usage: wgpu::TextureUsages::STORAGE_BINDING | usage,
            format: wgpu::TextureFormat::Rgba16Float,
            dimension: wgpu::TextureDimension::D2,
            sample_count: 1,
            mip_level_count: 1,
        });

        let input_view = input_texture.create_view(&Default::default());
        let output_view = output_texture.create_view(&Default::default());
        let bindgroup = create_bind_group!(ctx.device, &self.bindgroup_layout, "CubeMap Bindgroup": {
            0 | Sampler(&self.sampler),
            1 | TextureView(&input_view),
            2 | TextureView(&output_view),
            3 | Buffer( buffer: (get_cubemap_face_rotations_buffer(&ctx.device)) ),
        });

        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("CubeMap Encoder"),
        });
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("CubeMap Compute Pass"),
        });
        let workgroups = (tex_size + self.workgroups_size - 1) / self.workgroups_size;
        compute_pass.set_pipeline(&self.pipeline.pipeline);
        compute_pass.set_bind_group(0, &bindgroup, &[]);
        compute_pass.dispatch_workgroups(workgroups, workgroups, 6);
        drop(compute_pass);
        let si = ctx.queue.submit(std::iter::once(encoder.finish()));
        ctx.device.poll(wgpu::Maintain::WaitForSubmissionIndex(si));

        output_texture
    }
}
