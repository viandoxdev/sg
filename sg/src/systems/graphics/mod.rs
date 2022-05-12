use std::{collections::binary_heap, borrow::Borrow, cell::{RefCell, Ref}, f32::consts::{PI, FRAC_PI_2}, lazy::OnceCell};

use ecs::{System, system_pass};
use glam::{Mat4, Quat, Vec3};
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
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                }
            ]
        }
    }
}

#[derive(Clone, Copy)]
pub enum Projection {
    Perspective,
    Orthograhic,
    None,
}

pub struct Camera {
    position: Vec3,
    rotation: Quat,
    fov: f32,
    near: f32,
    far: f32,
    projection: Projection,
    aspect: f32,
    matrix: Mat4,
    dirty: bool,
    buffer: OnceCell<wgpu::Buffer>,
    bind_group: OnceCell<wgpu::BindGroup>,
    bind_group_layout: OnceCell<wgpu::BindGroupLayout>,
}

impl Camera {
    pub fn new() -> Self {
        Self::default()
    }
    fn set_dirty(&mut self) {
        self.dirty = true;
    }
    fn unset_dirty(&mut self) {
        self.dirty = false;
    }
    fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn set_position(&mut self, position: Vec3) {
        self.position = position;
        self.set_dirty();
    }
    pub fn set_rotation(&mut self, rotation: Quat) {
        self.rotation = rotation;
        self.set_dirty();
    }
    pub fn set_fov(&mut self, fov: f32) {
        self.fov = fov;
        self.set_dirty();
    }
    pub fn set_near(&mut self, near: f32) {
        self.near = near;
        self.set_dirty();
    }
    pub fn set_far(&mut self, far: f32) {
        self.far = far;
        self.set_dirty();
    }
    pub fn set_projection(&mut self, projection: Projection) {
        self.projection = projection;
        self.set_dirty();
    }
    pub fn set_aspect(&mut self, aspect: f32) {
        self.aspect = aspect;
        self.set_dirty();
    }
    pub fn get_position(&self) -> Vec3 {
        self.position
    }
    pub fn get_rotation(&self) -> Quat {
        self.rotation
    }
    pub fn get_fov(&self) -> f32 {
        self.fov
    }
    pub fn get_near(&self) -> f32 {
        self.near
    }
    pub fn get_far(&self) -> f32 {
        self.far
    }
    pub fn get_projection(&self) -> Projection {
        self.projection
    }
    pub fn get_aspect(&self) -> f32 {
        self.aspect
    }
    fn recompute_matrix(&mut self) {
        let view = Mat4::from_rotation_translation(self.rotation.inverse(), -self.position);
        let projection = match self.projection {
            Projection::Perspective => Mat4::perspective_lh(self.fov, self.aspect, self.near, self.far),
            Projection::Orthograhic => {
                let top = self.near * self.fov.tan();
                let bottom = -top;
                let left = top * self.aspect;
                let right = -left;
                Mat4::orthographic_rh(left, right, bottom, top, self.near, self.far)
            }
            Projection::None => Mat4::IDENTITY,
        };
        self.matrix = projection * view;
    }
    fn get_buffer(&self, device: &wgpu::Device) -> &wgpu::Buffer {
        self.buffer.get_or_init(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                mapped_at_creation: false,
                size: 64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                label: Some("Camera matrix buffer")
            })
        })
    }
    fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.is_dirty() {
            log::info!("Dirty {:?}", self.position);
            self.recompute_matrix();
            queue.write_buffer(self.get_buffer(device), 0, bytemuck::cast_slice(self.matrix.borrow().as_ref()));
            self.unset_dirty();
        }
    }
    fn get_bind_group(&self, device: &wgpu::Device) -> &wgpu::BindGroup {
        self.bind_group.get_or_init(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: self.get_bind_group_layout(device),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: self.get_buffer(device),
                            offset: 0,
                            size: None,
                        })
                    }
                ],
                label: Some("Camera bind group")
            })
        })
    }
    // TODO: just use &mut self
    fn get_bind_group_layout(&self, device: &wgpu::Device) -> &wgpu::BindGroupLayout {
        self.bind_group_layout.get_or_init(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None
                        },
                        count: None,
                    }
                ],
                label: Some("Camera bind group layout")
            })
        })
    }
}

impl Default for Camera {
    fn default() -> Self {
        let mut cam = Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective,
            aspect: 1.777,
            fov: FRAC_PI_2,
            near: 0.1,
            far: 100.0,
            matrix: Mat4::IDENTITY,
            dirty: true,
            buffer: OnceCell::new(),
            bind_group: OnceCell::new(),
            bind_group_layout: OnceCell::new(),
        };
        cam.recompute_matrix();
        cam
    }
}

pub struct GraphicSystem {
    pub size: winit::dpi::PhysicalSize<u32>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    feedback: Result<(), wgpu::SurfaceError>,
    pub camera: Camera,
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

                self.camera.update(&self.device, &self.queue);

                for (id, (gfx, tsm)) in entities {
                    let tsm = tsm.cloned().unwrap_or_default();
                    let mesh = self.mesh_manager.get(gfx.mesh).expect(&format!("Unknown mesh on {id}"));


                    let (tex_bindgroup, index) = self.texture_manager.get_bindgroup(&self.device, gfx.texture);
                    let cam_bindgroup = self.camera.get_bind_group(&self.device);

                    render_pass.set_vertex_buffer(0, mesh.verticies.slice(..));
                    render_pass.set_index_buffer(mesh.indicies.slice(..), wgpu::IndexFormat::Uint16);
                    render_pass.set_bind_group(0, tex_bindgroup, &[]);
                    render_pass.set_bind_group(1, cam_bindgroup, &[]);
                    render_pass.set_push_constants(wgpu::ShaderStages::VERTEX, 0, bytemuck::cast_slice(tsm.mat().as_ref()));
                    render_pass.set_push_constants(wgpu::ShaderStages::FRAGMENT, 64, &index.to_be_bytes());
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
        limits.max_push_constant_size = 68;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::PUSH_CONSTANTS | wgpu::Features::TEXTURE_BINDING_ARRAY | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
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
            range: 64..68
        };
        // create now to get the bind group layout.
        let texture_manager = TextureManager::new();
        let mut camera = Camera::new();
        camera.set_aspect(size.width as f32 / size.height as f32);
        let tm_bind_group_layout = texture_manager.layout(&device);
        let cam_bind_group_layout = camera.get_bind_group_layout(&device);
        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render pipeline layout"),
            bind_group_layouts: &[tm_bind_group_layout, cam_bind_group_layout],
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
            feedback: Ok(()),
            mesh_manager: MeshManager::new(),
            texture_manager,
            camera,
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
            self.camera.set_aspect(new_size.width as f32 / new_size.height as f32);
        }
    }
}
