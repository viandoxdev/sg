use std::num::NonZeroU64;
use std::{collections::HashSet, num::NonZeroU32};
use std::sync::Arc;

use bimap::BiMap;
use ecs::{Entity, Entities};
use egui::TextureId;
use egui_wgpu::renderer::{RenderPass, ScreenDescriptor};
use slotmap::SecondaryMap;
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::Grabbed;
use crate::{include_shader, components::{LightComponent, GraphicsComponent, TransformsComponent}};

use super::mesh_manager::Mesh;
use super::pipeline::RenderPipeline;
use super::{pipeline::Pipeline, g_buffer::GBuffer, camera::Camera, GraphicContext, mesh_manager::Vertex, texture_manager::{TextureManager, TextureHandle}};

pub struct WorldRenderer {
    shading_pipeline: RenderPipeline,
    geometry_pipeline: RenderPipeline,
    g_buffer: GBuffer,
    pub camera: Camera,
    lights_cache: HashSet<Entity>,
    size: winit::dpi::PhysicalSize<u32>,
}

impl WorldRenderer {
    pub fn new(ctx: &mut GraphicContext) -> Self {
        let GraphicContext {
            device,
            config,
            texture_manager,
            size,
            ..
        } = ctx;

        let mut camera = Camera::new();
        camera.set_aspect(size.width as f32 / size.height as f32);

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

        Self {
            camera,
            g_buffer,
            lights_cache: HashSet::new(),
            shading_pipeline,
            geometry_pipeline,
            size: *size,
        }
    } 

    pub fn update_lights(&mut self, ctx: &GraphicContext, lights: Entities<(Entity, &LightComponent)>) {
        let lights = lights.collect::<Vec<_>>();
        let mut lights_changed = lights.len() != self.lights_cache.len();
        for (id, _) in &lights {
            if !self.lights_cache.contains(id) {
                lights_changed = true;
                break;
            }
        }

        if lights_changed {
            // update the cache
            self.lights_cache.clear();
            self.lights_cache.extend(lights.iter().map(|(id, _)| id));
            // TODO make this take an impl IntoIterator
            if let Err(overflow) = self
                .g_buffer
                .update_lights(&ctx.device, lights.iter().map(|(_, light)| &light.light))
            {
                let current_max = self
                    .shading_pipeline
                    .shader
                    .get_integer("LIGHTS_MAX")
                    .unwrap() as u32;
                let new_max = (current_max * 2).max(current_max + overflow);
                self.shading_pipeline
                    .shader
                    .set_integer("LIGHTS_MAX", new_max as i64);
                log::debug!("Max lights reached increasing limit, rebuilding shader and pipeline");
                self.shading_pipeline.rebuild(&ctx.device); // very expensive
            };
        }
    }

    pub fn render<'a>(
        &mut self,
        ctx: &mut GraphicContext,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        renderables: impl IntoIterator<Item = (Entity, &'a GraphicsComponent, Option<&'a TransformsComponent>)>
    ) {
        if ctx.size != self.size {
            self.resize(ctx, ctx.size);
        }

        {
            let mut render_pass =
                encoder.begin_render_pass(&geometry_renderpass_desc!(self.g_buffer));

            render_pass.set_pipeline(&self.geometry_pipeline.pipeline);
            self.camera.update(&ctx.device, &ctx.queue);

            for (_, gfx, tsm) in renderables {
                let tsm = tsm.cloned().unwrap_or_default();
                let mesh = ctx
                    .mesh_manager
                    .get(gfx.mesh)
                    .unwrap_or_else(|| panic!("Unknown mesh"));

                let tex_bindgroup = ctx
                    .texture_manager
                    .get_bindgroup(&ctx.device, gfx.material.textures);
                let cam_bindgroup = self.camera.get_bind_group(&ctx.device, &ctx.queue);

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
        }
        {
            let mut render_pass = encoder.begin_render_pass(&shading_renderpass_desc!(view));
            let cam_bindgroup = self.camera.get_bind_group(&ctx.device, &ctx.queue);

            render_pass.set_pipeline(&self.shading_pipeline.pipeline);
            render_pass.set_bind_group(0, &self.g_buffer.bindgroup, &[]);
            render_pass.set_bind_group(1, cam_bindgroup, &[]);
            render_pass.draw(0..3, 0..1);
        }
    }

    pub fn resize(&mut self, ctx: &GraphicContext, new_size: winit::dpi::PhysicalSize<u32>) {
        self.g_buffer.resize(
            &ctx.device,
            wgpu::Extent3d {
                width: new_size.width,
                height: new_size.height,
                depth_or_array_layers: 1,
            },
        );
        self.camera
            .set_aspect(new_size.width as f32 / new_size.height as f32);
        self.size = new_size;
    }
}

pub struct UIRenderer {
    size: winit::dpi::PhysicalSize<u32>,
    render_pass: RenderPass,
    screen_desc: ScreenDescriptor,
}

/// SAFETY: This isn't lmao
/// I'm doing this because my ECS requires resources to be send (rightfully so, as schedules are
/// threaded). But egui-wgpu's RenderPass isn't (because of a deeply nested Box<dyn Any>). Since
/// there is nothing I can do about that, and the dyn Any is only used for DrawCallbacks (which I
/// won't be using any), and since that would only be a problem if the draw callback used non send
/// types (pretty rare), I opted to just ignore it.
unsafe impl Send for UIRenderer {}

impl UIRenderer {
    pub fn new(ctx: &GraphicContext, ppp: f32) -> Self {
        Self {
            size: ctx.size,
            render_pass: RenderPass::new(&ctx.device, ctx.config.format, 1),
            screen_desc: ScreenDescriptor {
                size_in_pixels: [ctx.size.width, ctx.size.height],
                pixels_per_point: ppp
            },
        }
    }

    pub fn draw(&self, ctx: &egui::Context) {
        egui::Window::new("Test").show(ctx, |ui| {
            ui.heading("Test 2");
            if ui.button("Click").clicked() {
                log::info!("Clicked");
            }
        });
    }

    pub fn render(
        &mut self,
        ctx: &GraphicContext,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        estate: &mut egui_winit::State,
        ui: &egui::Context,
        grabbed: &Grabbed,
        window: &Arc<Window>,
    ) {
        if ctx.size != self.size {
            self.size = ctx.size;
            self.screen_desc.size_in_pixels = [ctx.size.width, ctx.size.height];
        }

        if window.scale_factor() as f32 != self.screen_desc.pixels_per_point {
            self.screen_desc.pixels_per_point = window.scale_factor() as f32;
        }

        let input = estate.take_egui_input(&window);

        let output = ui.run(input, |ui| {
            self.draw(ui)
        });
        
        if !**grabbed {
            estate.handle_platform_output(&window, ui, output.platform_output);
        }

        for (id, delta) in output.textures_delta.set {
            self.render_pass.update_texture(&ctx.device, &ctx.queue,id, &delta);
        }
            
        let primitives = ui.tessellate(output.shapes);
        self.render_pass.update_buffers(&ctx.device, &ctx.queue, &primitives, &self.screen_desc);
        self.render_pass.execute(encoder, view, &primitives, &self.screen_desc, None);

        for id in output.textures_delta.free {
            self.render_pass.free_texture(&id);
        }
    }
}
