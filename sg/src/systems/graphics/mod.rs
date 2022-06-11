use std::lazy::OnceCell;

use anyhow::Result;
use ecs::{system_pass, System};
use glam::{Mat3, Mat3A, Vec3, Vec4, Vec2};
use wgpu::{include_wgsl, util::{DeviceExt, BufferInitDescriptor}};
use winit::window::Window;

use crate::components::{GraphicsComponent, TransformsComponent};

use self::{
    camera::Camera,
    mesh_manager::MeshManager,
    texture_manager::{TextureHandle, TextureManager}, g_buffer::GBuffer,
};

pub mod camera;
pub mod mesh_manager;
pub mod texture_manager;
pub mod g_buffer;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coords: Vec2,
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
                    offset: std::mem::size_of::<Vec3>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 2 * std::mem::size_of::<Vec3>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DiretionalLight {
    direction: Vec3,
    color: Vec4
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLight {
    position: Vec3,
    color: Vec4
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpotLight {
    position: Vec3,
    direction: Vec3,
    cut_off: f32,
    color: Vec4
}

pub enum Light {
    Directional(DiretionalLight),
    Point(PointLight),
    Spot(SpotLight),
}

pub struct GraphicSystem {
    pub size: winit::dpi::PhysicalSize<u32>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    geometry_pipeline: wgpu::RenderPipeline,
    feedback: Result<(), wgpu::SurfaceError>,
    g_buffer: GBuffer,
    pub camera: Camera,
    pub mesh_manager: MeshManager,
    pub texture_manager: TextureManager,
}

impl System for GraphicSystem {
    fn name() -> &'static str {
        "GraphicSystem"
    }
    #[system_pass]
    fn pass_many(
        &mut self,
        entities: HashMap<Uuid, (GraphicsComponent, Option<TransformsComponent>)>,
    ) {
        self.feedback = Ok(());

        let output = self.surface.get_current_texture();
        match output {
            Ok(output) => {
                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("gfx render encoder"),
                        });

                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("gfx render pass"),
                    color_attachments: &[
                        wgpu::RenderPassColorAttachment {
                            view: &self.g_buffer.albedo_tex,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 1.0,
                                }),
                                store: true,
                            },
                        },
                        wgpu::RenderPassColorAttachment {
                            view: &self.g_buffer.position_tex,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 0.0,
                                }),
                                store: true,
                            },
                        },
                        wgpu::RenderPassColorAttachment {
                            view: &self.g_buffer.normal_tex,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 0.0,
                                }),
                                store: true,
                            },
                        },
                    ],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.g_buffer.depth_tex,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: true,
                        }),
                        stencil_ops: None,
                    }),
                });
                render_pass.set_pipeline(&self.render_pipeline);

                self.camera.update(&self.device, &self.queue);

                for (id, (gfx, tsm)) in entities {
                    let tsm = tsm.cloned().unwrap_or_default();
                    let mesh = self
                        .mesh_manager
                        .get(gfx.mesh)
                        .unwrap_or_else(|| panic!("Unknown mesh on {id}"));

                    let tex_bindgroup = self
                        .texture_manager
                        .get_bindgroup(&self.device, gfx.textures);
                    let cam_bindgroup = self.camera.get_bind_group(&self.device);

                    render_pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    render_pass
                        .set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
                    render_pass.set_bind_group(0, tex_bindgroup, &[]);
                    render_pass.set_bind_group(1, cam_bindgroup, &[]);
                    render_pass.set_push_constants(
                        wgpu::ShaderStages::VERTEX,
                        0,
                        bytemuck::cast_slice(&[
                            tsm.mat(),
                            tsm.mat().inverse().transpose(),
                        ])
                    );
                    render_pass.draw_indexed(0..mesh.num_indices, 0, 0..1);
                }

                drop(render_pass);
                // begin new render pass
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Render pass"),
                    color_attachments: &[
                        wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 1.0,
                                }),
                                store: true,
                            },
                        }
                    ],
                    depth_stencil_attachment: None,
                });

                render_pass.set_bind_group(0, &self.g_buffer.bindgroup, &[]);
                render_pass.draw(0..3, 0..1);

                drop(render_pass);

                self.queue.submit(std::iter::once(encoder.finish()));
                output.present();
            }
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
        println!("{:?}", adapter.get_info());
        let limits = wgpu::Limits {
            max_push_constant_size: 128,
            ..Default::default()
        };
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::PUSH_CONSTANTS |
                        wgpu::Features::TEXTURE_BINDING_ARRAY |
                        wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY |
                        wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
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

        let g_buffer_shader = device.create_shader_module(&include_wgsl!("g_buffer.wgsl"));
        let render_shader = device.create_shader_module(&include_wgsl!("shader.wgsl"));

        let model_push_constant_range = wgpu::PushConstantRange {
            stages: wgpu::ShaderStages::VERTEX,
            range: 0..128,
        };

        let mut texture_manager = TextureManager::new();

        let mut camera = Camera::new();
        camera.set_aspect(size.width as f32 / size.height as f32);
        let tm_bind_group_layout = texture_manager.layout(&device);
        let cam_bind_group_layout = camera.get_bind_group_layout(&device);
        let geometry_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("geometry pipeline layout"),
                bind_group_layouts: &[tm_bind_group_layout, cam_bind_group_layout],
                push_constant_ranges: &[model_push_constant_range],
            });
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("render pipeline layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[]
            });
        let geometry_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Geomtry Pipeline"),
            layout: Some(&geometry_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &g_buffer_shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &g_buffer_shader,
                entry_point: "fs_main",
                targets: &[
                    wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    },
                    wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba32Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    },
                    wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba32Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }
                ],
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: TextureManager::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: "vs_main",
                buffers: &[]
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: "fs_main",
                targets: &[
                    wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    },
                ],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
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

        let g_buffer = GBuffer::new(&device, wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        }, &[]);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            geometry_pipeline,
            render_pipeline,
            feedback: Ok(()),
            mesh_manager: MeshManager::new(),
            texture_manager,
            camera,
            g_buffer,
        }
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
            self.g_buffer.resize(&self.device, wgpu::Extent3d {
                width: new_size.width,
                height: new_size.height,
                depth_or_array_layers: 1,
            });
            self.camera
                .set_aspect(new_size.width as f32 / new_size.height as f32);
        }
    }
}
