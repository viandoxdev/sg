use std::{collections::{HashSet, HashMap}, path::Path, lazy::{Lazy, SyncLazy}};

use anyhow::Result;
use codespan_reporting::{files::{SimpleFile, SimpleFiles}, diagnostic::{Diagnostic, Label}, term::termcolor::StandardStream};
use ecs::{System, filter_components};
use glam::{Vec3, Vec4, Vec2};
use regex::Regex;
use uuid::Uuid;
use winit::window::Window;

use crate::components::{GraphicsComponent, TransformsComponent, LightComponent};

use self::{
    camera::Camera,
    mesh_manager::MeshManager,
    texture_manager::{TextureManager, TextureHandle, SingleValue}, g_buffer::GBuffer,
};

pub mod camera;
pub mod mesh_manager;
pub mod texture_manager;
pub mod g_buffer;
#[macro_use]
pub mod desc;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coords: Vec2,
    pub tangent: Vec3,
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
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<(Vec3, Vec3, Vec2)>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DiretionalLight {
    direction: Vec3,
    padding: f32,
    color: Vec4
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLight {
    position: Vec3,
    padding: f32,
    color: Vec4
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpotLight {
    position: Vec3,
    padding: f32,
    direction: Vec3,
    cut_off: f32,
    color: Vec4
}

impl DiretionalLight {
    pub fn new(direction: Vec3, color: Vec4) -> Self { Self {
        direction,
        padding: 0.0,
        color,
    }}
}

impl PointLight {
    pub fn new(position: Vec3, color: Vec4) -> Self { Self {
        position,
        padding: 0.0,
        color,
    }}
}

impl SpotLight {
    pub fn new(position: Vec3, direction: Vec3, cut_off: f32, color: Vec4) -> Self { Self {
        position,
        padding: 0.0,
        direction,
        cut_off,
        color,
    }}
}

#[derive(Clone, Copy)]
pub enum Light {
    Directional(DiretionalLight),
    Point(PointLight),
    Spot(SpotLight),
}

pub enum ShaderConstant {
    Integer(i64),
    Float(f64),
    Bool(bool),
    Any(String),
}

impl ToString for ShaderConstant {
    fn to_string(&self) -> String {
        match self {
            Self::Integer(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Any(a) => a.to_string(),
        }
    }
}

pub struct Shader {
    name: &'static str,
    source: String,
    constants: HashMap<&'static str, ShaderConstant>
}

macro_rules! include_shader {
    ($path:literal, $name:literal) => {
        Shader::new(include_str!($path).to_owned(), $name)
    };
}

impl<'a> Shader {
    pub fn from_file(path: impl AsRef<Path>, name: &'static str) -> Self {
        Self::new(std::fs::read_to_string(path).expect("Error on file read"), name)
    }
    pub fn new(source: String, name: &'static str) -> Self {
        Self {
            name,
            source,
            constants: HashMap::new(),
        }
    }
    pub fn set(&mut self, key: &'static str, value: ShaderConstant) {
        self.constants.insert(key, value);
    }
    pub fn set_integer(&mut self, key: &'static str, value: i64) {
        self.set(key, ShaderConstant::Integer(value));
    }
    pub fn set_float(&mut self, key: &'static str, value: f64) {
        self.set(key, ShaderConstant::Float(value));
    }
    pub fn set_bool(&mut self, key: &'static str, value: bool) {
        self.set(key, ShaderConstant::Bool(value));
    }
    pub fn get(&self, key: &'static str) -> Option<&ShaderConstant> {
        self.constants.get(key)
    }
    pub fn get_integer(&self, key: &'static str) -> Option<i64> {
        match self.constants.get(key)? {
            ShaderConstant::Integer(i) => Some(*i),
            _ => None
        }
    }
    pub fn get_float(&self, key: &'static str) -> Option<f64> {
        match self.constants.get(key)? {
            ShaderConstant::Float(f) => Some(*f),
            _ => None
        }
    }
    pub fn get_bool(&self, key: &'static str) -> Option<bool> {
        match self.constants.get(key)? {
            ShaderConstant::Bool(b) => Some(*b),
            _ => None
        }
    }
    pub fn module(&self, device: &wgpu::Device) -> wgpu::ShaderModule {
        let mut source = self.source.to_owned();
        let mut pat = "{{_}}".to_owned();
        for (p, val) in &self.constants {
            pat.replace_range(2..(pat.len() - 2), p);
            source = source.replace(&pat, &val.to_string());
        }
        // check for unset constants in debug builds
        #[cfg(debug_assertions)]
        {
            let mut err_count = 0;
            let mut files = SimpleFiles::new();
            let file = files.add(self.name, &source);
            let writer = StandardStream::stderr(codespan_reporting::term::termcolor::ColorChoice::Always);
            let config = codespan_reporting::term::Config::default();
            static RE: SyncLazy<Regex> = SyncLazy::new(|| Regex::new(r"\{\{(.+?)\}\}").unwrap());
            for cap in RE.captures_iter(&source) {
                err_count += 1;
                let m = cap.get(1).unwrap();
                let diagnostic = Diagnostic::error()
                    .with_message("constant hasn't been given any value")
                    .with_labels(vec![
                        Label::primary(file, m.range()).with_message(format!("No value for `{}` given", m.as_str()))
                    ]);
                codespan_reporting::term::emit(&mut writer.lock(), &config, &files, &diagnostic).ok();
            }
            if err_count > 0 {
                panic!("Error{} in shader preprocessing.", if err_count == 1 { "" } else { "s" })
            }
        }
        device.create_shader_module(&wgpu::ShaderModuleDescriptor {
            source: wgpu::ShaderSource::Wgsl(source.into()),
            label: Some(self.name),
        })
    }
}

pub struct Pipeline {
    layout: wgpu::PipelineLayout,
    build: Box<dyn Fn(&wgpu::Device, &wgpu::PipelineLayout, &wgpu::ShaderModule) -> wgpu::RenderPipeline>,
    pub pipeline: wgpu::RenderPipeline,
    pub shader: Shader,
}

impl Pipeline {
    fn new<F>(device: &wgpu::Device, layout: wgpu::PipelineLayout, shader: Shader, build: F) -> Self
        where F: Fn(&wgpu::Device, &wgpu::PipelineLayout, &wgpu::ShaderModule) -> wgpu::RenderPipeline + 'static
    {
        let pipeline = build(device, &layout, &shader.module(device));
        let build = Box::new(build);
        Self {
            layout,
            build,
            pipeline,
            shader
        }
    }

    fn rebuild(&mut self, device: &wgpu::Device) {
        self.pipeline = (self.build)(device, &self.layout, &self.shader.module(device));
    }
}

pub struct GraphicSystem {
    pub size: winit::dpi::PhysicalSize<u32>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
    shading_pipeline: Pipeline,
    geometry_pipeline: Pipeline,
    feedback: Result<(), wgpu::SurfaceError>,
    g_buffer: GBuffer,
    lights_cache: HashSet<Uuid>,
    pub camera: Camera,
    pub mesh_manager: MeshManager,
    pub texture_manager: TextureManager,
}

impl System for GraphicSystem {
    fn name() -> &'static str {
        "GraphicSystem"
    }
    fn pass<'a>(&mut self, mut entities: ecs::EntitiesBorrow<'a>) {
        let renderables = filter_components!(entities
            => GraphicsComponent;
            ? TransformsComponent;
        );
        let lights = filter_components!(entities
            => LightComponent;
        );

        let mut lights_changed = lights.len() != self.lights_cache.len();
        for id in lights.keys() {
            if !self.lights_cache.contains(id) {
                lights_changed = true;
                break;
            }
        }

        if lights_changed {
            // update the cache
            self.lights_cache.clear();
            self.lights_cache.extend(lights.keys());
            // update the g_buffer
            let lights = lights.into_iter().map(|(_, c)| c.light).collect::<Vec<_>>();
            if let Err(overflow) = self.g_buffer.update_lights(&self.device, &lights) {
                let current_max = self.shading_pipeline.shader.get_integer("LIGHTS_MAX").unwrap() as u32;
                let new_max = (current_max * 2).max(current_max + overflow);
                self.shading_pipeline.shader.set_integer("LIGHTS_MAX", new_max as i64);
                log::debug!("Max lights reached increasing limit, rebuilding shader and pipeline");
                self.shading_pipeline.rebuild(&self.device); // very expensive
            };
        }

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

                let mut render_pass = encoder.begin_render_pass(&geometry_renderpass_desc!(&self.g_buffer));
                render_pass.set_pipeline(&self.geometry_pipeline.pipeline);

                self.camera.update(&self.device, &self.queue);

                for (id, (gfx, tsm)) in renderables {
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
                let mut render_pass = encoder.begin_render_pass(&shading_renderpass_desc!(&view));
                let cam_bindgroup = self.camera.get_bind_group(&self.device);

                render_pass.set_pipeline(&self.shading_pipeline.pipeline);
                render_pass.set_bind_group(0, &self.g_buffer.bindgroup, &[]);
                render_pass.set_bind_group(1, cam_bindgroup, &[]);
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
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::PUSH_CONSTANTS |
                        wgpu::Features::TEXTURE_BINDING_ARRAY |
                        wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY |
                        wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
                    limits: wgpu::Limits {
                        max_push_constant_size: 128,
                        ..Default::default()
                    },
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

        let texture_manager = TextureManager::new();
        let mut camera = Camera::new();
        let g_buffer = GBuffer::new(&device, wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        }, &[], 64);

        let geometry_pipeline = {
            let shader = include_shader!("g_buffer.wgsl", "geometry shader");
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("geometry pipeline layout"),
                bind_group_layouts: &[
                    texture_manager.layout(&device),
                    camera.get_bind_group_layout(&device)
                ],
                push_constant_ranges: &[
                    wgpu::PushConstantRange {
                        stages: wgpu::ShaderStages::VERTEX,
                        range: 0..128,
                    }
                ],
            });
            Pipeline::new(&device, layout, shader, |device, layout, shader| {
                device.create_render_pipeline(&geometry_pipeline_desc!(layout, shader))
            })
        };

        let shading_pipeline = {
            let mut shader = include_shader!("shader.wgsl", "shading shader");
            // default value
            shader.set_integer("LIGHTS_MAX", 64);
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shading pipeline layout"),
                bind_group_layouts: &[
                    &g_buffer.bind_group_layout,
                    camera.get_bind_group_layout(&device),
                ],
                push_constant_ranges: &[]
            });
            let format = config.format;
            Pipeline::new(&device, layout, shader, move |device, layout, shader| {
                device.create_render_pipeline(&shading_pipeline_desc!(layout, shader, format))
            })
        };

        surface.configure(&device, &config);
        camera.set_aspect(size.width as f32 / size.height as f32);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            geometry_pipeline,
            shading_pipeline,
            feedback: Ok(()),
            mesh_manager: MeshManager::new(),
            lights_cache: HashSet::new(),
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
