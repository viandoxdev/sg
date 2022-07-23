use anyhow::Result;
use ecs::{Entities, Entity};
use glam::{Vec3, Vec4};
use winit::window::Window;
use std::sync::Arc;

use crate::{components::{GraphicsComponent, TransformsComponent}, Grabbed};

use self::{
    mesh_manager::MeshManager,
    texture_manager::{SingleValue, TextureHandle, TextureManager, TextureSet}, renderer::{WorldRenderer, UIRenderer},
};

#[macro_use] // avoid importing each and every macro
pub mod desc; // Large descriptors
pub mod camera; // Camera
pub mod g_buffer; // GBuffer
pub mod gltf; // Gltf loading (-> ECS)
pub mod mesh_manager; // Mesh Manager
pub mod pipeline; // Abstraction over pipelines and shaders (with ad hoc specialization constant)
pub mod texture_manager; // Texture manager
pub mod renderer; // UI and World rendered
pub mod cubemap; // Equirectangular to cubemap conversion
pub mod convolution; // Convolution of environment maps

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
    feedback: Result<(), wgpu::SurfaceError>,
    pub mesh_manager: MeshManager,
    pub texture_manager: TextureManager,
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
                        max_texture_dimension_2d: 20000,
                        max_buffer_size: 1024u64.pow(3) * 4,
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

        surface.configure(&device, &config);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            feedback: Ok(()),
            mesh_manager: MeshManager::new(),
            texture_manager,
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

    pub fn render(
        &mut self,
        wr: &mut WorldRenderer,
        uir: &mut UIRenderer,
        estate: &mut egui_winit::State,
        ui: &egui::Context,
        window: &Arc<Window>,
        grabbed: &Grabbed,
        renderables: Entities<(Entity, &GraphicsComponent, Option<&TransformsComponent>)>,
    ) {
        self.feedback = Ok(());
        
        let output = self.surface.get_current_texture();
        match output {
            Ok(output) => {
                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder = self
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("gfx render encoder"),
                    });
                
                wr.render(self, &mut encoder, &view, renderables);
                uir.render(self, &mut encoder, &view, estate, ui, grabbed, window);

                self.queue.submit(std::iter::once(encoder.finish()));
                output.present();
            }
            Err(error) => {
                log::info!("Error on surface");
                self.feedback = Err(error);
            }
        }
    }
}
