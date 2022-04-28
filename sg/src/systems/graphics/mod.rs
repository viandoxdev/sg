use ecs::{System, system_pass};
use slotmap::SlotMap;
use wgpu::{include_wgsl, util::DeviceExt};
use winit::window::Window;
use anyhow::{anyhow, Result};

use crate::components::GraphicsComponent;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3]
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
                    format: wgpu::VertexFormat::Float32x3,
                }
            ]
        }
    }
}

pub struct Mesh {
    pub verticies: wgpu::Buffer,
    pub indicies: wgpu::Buffer,
    pub num_indicies: u32,
}

impl Mesh {
    fn build_buffer(device: &wgpu::Device, verticies: &[Vertex], indicies: &[[u16; 3]]) -> (wgpu::Buffer, wgpu::Buffer) {
        (
            device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("Vertex Buffer"),
                    contents: bytemuck::cast_slice(verticies),
                    usage: wgpu::BufferUsages::VERTEX,
                }
            ),
            device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("Index Buffer"),
                    contents: bytemuck::cast_slice(indicies),
                    usage: wgpu::BufferUsages::INDEX,
                }
            )
        )
    }
    pub fn new(device: &wgpu::Device, verticies: &[Vertex], indicies: &[[u16; 3]]) -> Self {
        let num_indicies = indicies.len() as u32;
        let (verticies, indicies) = Self::build_buffer(device, verticies, indicies);
        Self {
            verticies,
            indicies,
            num_indicies
        }
    }
    pub fn update(&mut self, device: &wgpu::Device, verticies: &[Vertex], indicies: &[[u16; 3]]) {
        (self.verticies, self.indicies) = Self::build_buffer(device, verticies, indicies);
        self.num_indicies = indicies.len() as u32;
    }
}

slotmap::new_key_type! {
    pub struct MeshHandle;
}

pub struct GraphicSystem {
    pub size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    meshes: SlotMap<MeshHandle, Mesh>,
    feedback: Result<(), wgpu::SurfaceError>,
}

impl System for GraphicSystem {
    fn name() -> &'static str {
        "GraphicSystem"
    }
    #[system_pass]
    fn pass_many(&mut self, entities: HashMap<Uuid, GraphicsComponent>) {
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

                for (id, gfx) in entities {
                    let mesh = self.get_mesh(gfx.mesh).expect(&format!("Unknown mesh on {id}"));

                    render_pass.set_vertex_buffer(0, mesh.verticies.slice(..));
                    render_pass.set_index_buffer(mesh.indicies.slice(..), wgpu::IndexFormat::Uint16);
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

        let instance = wgpu::Instance::new(wgpu::Backends::GL);
        let surface = unsafe { instance.create_surface(window) };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
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
        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render pipeline layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
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
            meshes: SlotMap::with_key(),
            feedback: Ok(()),
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
        }
    }
    pub fn add_mesh(&mut self, verticies: &[Vertex], indicies: &[[u16; 3]]) -> MeshHandle {
        self.meshes.insert(Mesh::new(&self.device, verticies, indicies))
    }
    pub fn remove_mesh(&mut self, handle: MeshHandle) -> Option<Mesh> {
        self.meshes.remove(handle)
    }
    pub fn update_mesh(&mut self, handle: MeshHandle, verticies: &[Vertex], indicies: &[[u16; 3]]) -> Result<()> {
        self.meshes.get_mut(handle).ok_or(anyhow!("Handle doesn't point to any mesh"))?.update(&self.device, verticies, indicies);
        Ok(())
    }
    pub fn get_mesh(&self, handle: MeshHandle) -> Option<&Mesh> {
        self.meshes.get(handle)
    }
}
