use std::collections::binary_heap;

use ecs::{System, system_pass};
use glam::Mat4;
use uuid::Uuid;
use wgpu::{include_wgsl, RenderPass};
use winit::window::Window;
use anyhow::Result;

use crate::components::{GraphicsComponent, TransformsComponent};

use self::{texture_manager::TextureManager, mesh_manager::MeshManager};

pub mod texture_manager;
pub mod mesh_manager;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2]
}
impl Vertex {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                }
            ]
        }
    }
}

pub struct GraphicSystem {
    pub size: winit::dpi::PhysicalSize<u32>,
    pub device: wgpu::Device,
    surface: wgpu::Surface,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    projection_matrix: Mat4,
    feedback: Result<(), wgpu::SurfaceError>,

    pub mesh_manager: MeshManager,
    pub texture_manager: TextureManager,
}

impl System for GraphicSystem {
    fn name() -> &'static str {
        "GraphicSystem"
    }
    #[system_pass]
    fn pass_many(&mut self, entities: HashMap<Uuid, (GraphicsComponent, Option<TransformsComponent>)>) {
        self.feedback = Ok(());

        let output = self.surface.get_current_texture();
        match output {
            Ok(output) => {

                let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("gfx render encoder")
                });

                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("gfx render pass"),
                    color_attachments: &[wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.4,
                                g: 0.8,
                                b: 0.3,
                                a: 1.0,
                            }),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                });
                render_pass.set_pipeline(&self.render_pipeline);

                for (id, (gfx, tsm)) in entities {
                    let tsm = tsm.cloned().unwrap_or_default();
                    let mesh = self.mesh_manager.get(gfx.mesh).expect(&format!("Unknown mesh on {id}"));

                    render_pass.set_vertex_buffer(0, mesh.verticies.slice(..));
                    render_pass.set_index_buffer(mesh.indicies.slice(..), wgpu::IndexFormat::Uint16);
                    let (bindgroup, index) = self.texture_manager.get_bindgroup(&self.device, &self.queue, gfx.texture);
                    let bindgroup = *bindgroup.clone();
                    render_pass.set_bind_group(0, &bindgroup, &[]);
                    render_pass.set_push_constants(wgpu::ShaderStages::VERTEX, 0, bytemuck::cast_slice(tsm.mat().as_ref()));
                    render_pass.set_push_constants(wgpu::ShaderStages::FRAGMENT, 64, bytemuck::cast_slice(&[index]));
                    render_pass.draw_indexed(0..mesh.num_indicies, 0, 0..1);
                }

                drop(render_pass);

                self.queue.submit(std::iter::once(encoder.finish()));
                output.present();
            },
            Err(error) => {
                self.feedback = Err(error);
            }
        }
    }
}

impl GraphicSystem {
    pub async fn new(window: &Window) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::Backends::VULKAN);
        let surface = unsafe { instance.create_surface(window) };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        let mut limits = wgpu::Limits::default();
        limits.max_push_constant_size = 64;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::PUSH_CONSTANTS | wgpu::Features::TEXTURE_BINDING_ARRAY | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY,
                    limits,
                    label: None,
                },
                None,
            )
            .await
            .unwrap();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_preferred_format(&adapter).unwrap(),
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(&include_wgsl!("shader.wgsl"));
        let model_push_constant_range = wgpu::PushConstantRange {
            stages: wgpu::ShaderStages::VERTEX,
            range: 0..64
        };
        let texture_push_constant_range = wgpu::PushConstantRange {
            stages: wgpu::ShaderStages::FRAGMENT,
            range: 0..4
        };
        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render pipeline layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[model_push_constant_range, texture_push_constant_range],
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    Vertex::desc()
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                }],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            projection_matrix: Mat4::IDENTITY,
            feedback: Ok(()),
            mesh_manager: MeshManager::new(),
            texture_manager: TextureManager::new()
        }
    }
    fn render<'a>(&'a mut self, render_pass: &mut RenderPass<'a>, id: Uuid, gfx: &mut GraphicsComponent, tsm: TransformsComponent) {
    }
    pub fn set_projection(&mut self, mat: Mat4) {
        self.projection_matrix = mat;
    }
    pub fn feedback(&self) -> Result<(), wgpu::SurfaceError> {
        self.feedback.as_ref().map_err(|err| err.clone())?;
        Ok(())
    }
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}
