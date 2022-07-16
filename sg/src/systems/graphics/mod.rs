use std::collections::HashSet;

use anyhow::Result;
use ecs::{Entities, Entity};
use glam::{Vec3, Vec4};
use winit::window::Window;

use crate::{
    components::{GraphicsComponent, LightComponent, TransformsComponent},
    include_shader,
};

use self::{
    camera::Camera,
    g_buffer::GBuffer,
    mesh_manager::{MeshManager, Vertex},
    pipeline::Pipeline,
    texture_manager::{SingleValue, TextureHandle, TextureManager, TextureSet},
};

#[macro_use] // avoid importing each and every macro
pub mod desc;
pub mod camera;
pub mod g_buffer;
pub mod gltf;
pub mod mesh_manager;
pub mod pipeline;
pub mod texture_manager;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DiretionalLight {
    direction: Vec3,
    padding: f32,
    color: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLight {
    position: Vec3,
    padding: f32,
    color: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpotLight {
    position: Vec3,
    padding: f32,
    direction: Vec3,
    cut_off: f32,
    color: Vec4,
}

impl DiretionalLight {
    pub fn new(direction: Vec3, color: Vec4) -> Self {
        Self {
            direction,
            padding: 0.0,
            color,
        }
    }
}

impl PointLight {
    pub fn new(position: Vec3, color: Vec4) -> Self {
        Self {
            position,
            padding: 0.0,
            color,
        }
    }
}

impl SpotLight {
    pub fn new(position: Vec3, direction: Vec3, cut_off: f32, color: Vec4) -> Self {
        Self {
            position,
            padding: 0.0,
            direction,
            cut_off,
            color,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Light {
    Directional(DiretionalLight),
    Point(PointLight),
    Spot(SpotLight),
}

#[derive(Clone, Copy)]
pub struct Material {
    textures: TextureSet,
}

impl Material {
    pub fn new_with_values(
        albedo: TextureHandle,
        normal_map: Option<TextureHandle>,
        metallic: f32,
        roughness: f32,
        ao: Option<TextureHandle>,
        gfx: &mut GraphicContext,
    ) -> Result<Self> {
        let metallic = gfx.texture_manager.get_or_add_single_value_texture(
            &gfx.device,
            &gfx.queue,
            SingleValue::Factor(metallic),
        );
        let roughness = gfx.texture_manager.get_or_add_single_value_texture(
            &gfx.device,
            &gfx.queue,
            SingleValue::Factor(roughness),
        );
        Self::new(albedo, normal_map, metallic, roughness, ao, gfx)
    }
    pub fn new(
        albedo: TextureHandle,
        normal_map: Option<TextureHandle>,
        metallic: TextureHandle,
        roughness: TextureHandle,
        ao: Option<TextureHandle>,
        gfx: &mut GraphicContext,
    ) -> Result<Self> {
        let set = gfx.texture_manager.add_set();
        let normal_map = normal_map.unwrap_or_else(|| {
            gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Normal(Vec3::new(0.0, 0.0, 1.0)),
            )
        });
        let ao = ao.unwrap_or_else(|| {
            gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Factor(1.0),
            )
        });
        gfx.texture_manager.add_texture_to_set(albedo, set)?;
        gfx.texture_manager.add_texture_to_set(normal_map, set)?;
        gfx.texture_manager.add_texture_to_set(metallic, set)?;
        gfx.texture_manager.add_texture_to_set(roughness, set)?;
        gfx.texture_manager.add_texture_to_set(ao, set)?;
        Ok(Self { textures: set })
    }
}

pub struct GraphicContext {
    pub size: winit::dpi::PhysicalSize<u32>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
    shading_pipeline: Pipeline,
    geometry_pipeline: Pipeline,
    feedback: Result<(), wgpu::SurfaceError>,
    g_buffer: GBuffer,
    lights_cache: HashSet<Entity>,
    pub camera: Camera,
    pub mesh_manager: MeshManager,
    pub texture_manager: TextureManager,
}

pub fn lights_system(ctx: &mut GraphicContext, lights: Entities<(Entity, &LightComponent)>) {
    let lights = lights.collect::<Vec<_>>();
    let mut lights_changed = lights.len() != ctx.lights_cache.len();
    for (id, _) in &lights {
        if !ctx.lights_cache.contains(id) {
            lights_changed = true;
            break;
        }
    }

    if lights_changed {
        // update the cache
        ctx.lights_cache.clear();
        ctx.lights_cache.extend(lights.iter().map(|(id, _)| id));
        // TODO make this take an impl IntoIterator
        if let Err(overflow) = ctx
            .g_buffer
            .update_lights(&ctx.device, lights.iter().map(|(_, light)| &light.light))
        {
            let current_max = ctx
                .shading_pipeline
                .shader
                .get_integer("LIGHTS_MAX")
                .unwrap() as u32;
            let new_max = (current_max * 2).max(current_max + overflow);
            ctx.shading_pipeline
                .shader
                .set_integer("LIGHTS_MAX", new_max as i64);
            log::debug!("Max lights reached increasing limit, rebuilding shader and pipeline");
            ctx.shading_pipeline.rebuild(&ctx.device); // very expensive
        };
    }
}

pub fn graphic_system(
    ctx: &mut GraphicContext,
    renderables: Entities<(Entity, &GraphicsComponent, Option<&TransformsComponent>)>,
) {
    ctx.feedback = Ok(());

    let output = ctx.surface.get_current_texture();
    match output {
        Ok(output) => {
            let view = output
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("gfx render encoder"),
                });

            let mut render_pass =
                encoder.begin_render_pass(&geometry_renderpass_desc!(ctx.g_buffer));
            render_pass.set_pipeline(&ctx.geometry_pipeline.pipeline);

            ctx.camera.update(&ctx.device, &ctx.queue);

            for (_, gfx, tsm) in renderables {
                let tsm = tsm.cloned().unwrap_or_default();
                let mesh = ctx
                    .mesh_manager
                    .get(gfx.mesh)
                    .unwrap_or_else(|| panic!("Unknown mesh"));

                let tex_bindgroup = ctx
                    .texture_manager
                    .get_bindgroup(&ctx.device, gfx.material.textures);
                let cam_bindgroup = ctx.camera.get_bind_group(&ctx.device);

                render_pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                render_pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.set_bind_group(0, tex_bindgroup, &[]);
                render_pass.set_bind_group(1, cam_bindgroup, &[]);
                render_pass.set_push_constants(
                    wgpu::ShaderStages::VERTEX,
                    0,
                    bytemuck::cast_slice(&[tsm.mat(), tsm.mat().inverse().transpose()]),
                );
                render_pass.draw_indexed(0..mesh.num_indices, 0, 0..1);
            }

            drop(render_pass);
            let mut render_pass = encoder.begin_render_pass(&shading_renderpass_desc!(&view));
            let cam_bindgroup = ctx.camera.get_bind_group(&ctx.device);

            render_pass.set_pipeline(&ctx.shading_pipeline.pipeline);
            render_pass.set_bind_group(0, &ctx.g_buffer.bindgroup, &[]);
            render_pass.set_bind_group(1, cam_bindgroup, &[]);
            render_pass.draw(0..3, 0..1);

            drop(render_pass);

            ctx.queue.submit(std::iter::once(encoder.finish()));
            output.present();
        }
        Err(error) => {
            ctx.feedback = Err(error);
        }
    }
}

impl GraphicContext {
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
            format: surface.get_supported_formats(&adapter)[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };

        let texture_manager = TextureManager::new();
        let mut camera = Camera::new();
        let g_buffer = GBuffer::new(
            &device,
            wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            &[],
            64,
        );

        let geometry_pipeline = {
            let shader = include_shader!("g_buffer.wgsl", "geometry shader");
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("geometry pipeline layout"),
                bind_group_layouts: &[
                    texture_manager.layout(&device),
                    camera.get_bind_group_layout(&device),
                ],
                push_constant_ranges: &[wgpu::PushConstantRange {
                    stages: wgpu::ShaderStages::VERTEX,
                    range: 0..128,
                }],
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
                push_constant_ranges: &[],
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
            self.g_buffer.resize(
                &self.device,
                wgpu::Extent3d {
                    width: new_size.width,
                    height: new_size.height,
                    depth_or_array_layers: 1,
                },
            );
            self.camera
                .set_aspect(new_size.width as f32 / new_size.height as f32);
        }
    }
}
