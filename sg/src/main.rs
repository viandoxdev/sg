#![feature(trait_upcasting)]
#![feature(once_cell)]
#![feature(trace_macros)]
#![allow(incomplete_features)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::f32::consts::PI;
use std::io::BufReader;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::sync::{Arc, Barrier, mpsc};

use ecs::{Executor, World};
use glam::{EulerRot, Quat, Vec2, Vec3, Vec4};
use image::{GenericImageView, Rgba};
use parking_lot::RwLock;
use slotmap::SlotMap;
use systems::graphics::convolution::ConvolutionComputer;
use systems::graphics::cubemap::CubeMapComputer;
use systems::graphics::mesh_manager::{Mesh, Primitives};
use systems::graphics::texture_manager::SingleValue;
use systems::graphics::{GraphicContext, Light, PointLight, Material};
use systems::graphics::renderer::{WorldRenderer, UIRenderer};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event, KeyboardInput, ScanCode, VirtualKeyCode, WindowEvent, MouseButton};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use egui_winit::State as EState;
use systems::graphics::gltf;

use components::{LightComponent, GraphicsComponent, TransformsComponent};

mod chess;
pub mod components;
pub mod systems;

slotmap::new_key_type! {
    struct Input;
}

const CENTER_POS: PhysicalPosition<f64> = PhysicalPosition::new(100.0, 100.0);

#[derive(Default)]
struct InputState {
    states: RwLock<SlotMap<Input, RwLock<ElementState>>>,
    keycodes: RwLock<HashMap<VirtualKeyCode, Input>>,
    scancodes: RwLock<HashMap<ScanCode, Input>>,
    mouse_delta: RwLock<Vec2>,
}

#[derive(Clone, Copy)]
pub struct Grabbed(bool);

impl Deref for Grabbed {
    type Target = bool;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl InputState {
    fn new() -> Self {
        Self::default()
    }

    fn get_input_by_keycode(&self, keycode: VirtualKeyCode) -> Option<Input> {
        self.keycodes.read().get(&keycode).copied()
    }

    fn get_input_by_scancode(&self, scancode: ScanCode) -> Option<Input> {
        self.scancodes.read().get(&scancode).copied()
    }

    fn try_get_input(&self, input: &KeyboardInput) -> Option<Input> {
        self.get_input_by_scancode(input.scancode)
            .or(self.get_input_by_keycode(input.virtual_keycode?))
    }

    fn get_state(&self, input: Input) -> Option<ElementState> {
        self.states.read().get(input).map(|e| *e.read())
    }

    fn get_state_by_keycode(&self, keycode: VirtualKeyCode) -> Option<ElementState> {
        self.get_state(self.get_input_by_keycode(keycode)?)
    }

    fn get_state_by_scancode(&self, scancode: ScanCode) -> Option<ElementState> {
        self.get_state(self.get_input_by_scancode(scancode)?)
    }

    fn is_pressed_keycode(&self, keycode: VirtualKeyCode) -> bool {
        matches!(
            self.get_state_by_keycode(keycode),
            Some(ElementState::Pressed)
        )
    }

    fn notify(&self, input: KeyboardInput) {
        let key = self.try_get_input(&input).unwrap_or_else(|| {
            let key = self.states.write().insert(RwLock::new(input.state));
            self.scancodes.write().insert(input.scancode, key);
            if let Some(keycode) = input.virtual_keycode {
                self.keycodes.write().insert(keycode, key);
            }
            key
        });

        *self.states.read().get(key).unwrap().write() = input.state;
    }

    fn get_mouse_delta(&self) -> Vec2 {
        *self.mouse_delta.read()
    }

    fn notify_mouse(&self, pos: PhysicalPosition<f64>) {
        *self.mouse_delta.write() = Vec2::new(
            pos.x as f32 - CENTER_POS.x as f32,
            pos.y as f32 - CENTER_POS.y as f32,
        );
    }
}

async fn run(mut world: World, mut executor: Executor) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut gfx = GraphicContext::new(&window).await;
    let mut wr = WorldRenderer::new(&mut gfx);
    let uir = UIRenderer::new(&gfx, window.scale_factor() as f32);
    let mut estate = EState::new(&event_loop);
    let ui = egui::Context::default();
    let inputs = Arc::new(InputState::new());

    estate.set_max_texture_side(gfx.device.limits().max_texture_dimension_2d as usize);
    estate.set_pixels_per_point(window.scale_factor() as f32);
    wr.camera.set_position(Vec3::new(0.0, 0.0, 2.0));
    wr.camera.set_rotation(Quat::from_rotation_y(PI));
    //world.spawn_many(gltf::open("models/ka.glb", &mut gfx).expect("Error"));

    {
        let mut r = CubeMapComputer::new(&gfx);
        let mut reader = image::io::Reader::with_format(
                BufReader::new(std::fs::File::open("hdr.exr").unwrap()),
                image::ImageFormat::OpenExr,
            );
        reader.no_limits();
        let image = reader
            .decode()
            .unwrap()
            .flipv()
            .to_rgba32f();
        let f = 4096;
        let s = 128;
        let t = r.render(image, &gfx, f, wgpu::TextureUsages::TEXTURE_BINDING)
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
        let c = ConvolutionComputer::new(&gfx);
        let e = c.run(&t, s, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC, &gfx);
        let v = e
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
        wr.camera.set_skybox(t);
        wr.camera.set_irradiance_map(v);

        //let device = &gfx.device;
        //let queue = &gfx.queue;
        //let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        //    label: None,
        //    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        //    size: s as u64 * 4 * 4 * s as u64 * 6,
        //    mapped_at_creation: false,
        //});
        //let mut encoder = device.create_command_encoder(&Default::default());
        //encoder.copy_texture_to_buffer(
        //    e.as_image_copy(),
        //    wgpu::ImageCopyBuffer {
        //        buffer: &buffer,
        //        layout: wgpu::ImageDataLayout {
        //            offset: 0,
        //            bytes_per_row: NonZeroU32::new(s * 4 * 4),
        //            rows_per_image: NonZeroU32::new(s),
        //        }
        //    },
        //    wgpu::Extent3d {
        //        width: s,
        //        height: s,
        //        depth_or_array_layers: 6,
        //    }
        //);
        //let si = queue.submit(std::iter::once(encoder.finish()));

        //let (se, re) = mpsc::channel();

        //    buffer.slice(..).map_async(wgpu::MapMode::Read, move |b| {
        //        se.send(b).unwrap();
        //    });

        //device.poll(wgpu::Maintain::WaitForSubmissionIndex(si));

        //re.recv().unwrap().unwrap();

        //let bytes = buffer.slice(..)
        //    .get_mapped_range()
        //    .iter()
        //    .copied().collect::<Vec<_>>();
        //let floats: Vec<f32> = bytemuck::cast_slice::<_, f32>(&bytes).to_vec();
        //let buffer = image::ImageBuffer::<Rgba<f32>, Vec<f32>>::from_raw(s, s*6, floats).unwrap();
        //buffer.save_with_format("out.exr", image::ImageFormat::OpenExr).unwrap();

        //Box::leak(Box::new(t));
    }

    let gfc = {
        let mesh = gfx.mesh_manager.add(&gfx.device, &Mesh::new_icosphere(3));
        let material = {
            let albedo = gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Color(Vec4::new(1.0, 0.0, 0.0, 1.0))
            );
            Material::new_with_values(albedo, None, 0.0, 0.8, None, &mut gfx).unwrap()
        };
        GraphicsComponent {
            mesh,
            material,
        }
    };

    world.spawn((gfc,));

    let window = Arc::new(window);

    executor.add_resource(gfx);
    executor.add_resource(wr);
    executor.add_resource(uir);
    executor.add_resource(estate);
    executor.add_resource(ui);
    executor.add_resource(window.clone());

    let mut colors = std::iter::empty()
        .chain(std::iter::repeat(Vec4::new(7.0, 7.0, 7.0, 1.0)).take(8))
        .chain(std::iter::repeat(Vec4::new(2.5, 5.0, 10.0, 1.0)).take(8))
        .chain(std::iter::repeat(Vec4::new(15.0, 15.0, 15.0, 1.0)).take(4));
    let lights = [
        Vec3::new( 1.0,  1.0, 6.0),
        Vec3::new(-1.0,  1.0, 6.0),
        Vec3::new(-1.0, -1.0, 6.0),
        Vec3::new( 1.0, -1.0, 6.0),
        Vec3::new( 1.0,  4.0, 6.0),
        Vec3::new(-1.0,  4.0, 6.0),
        Vec3::new(-1.0,  2.0, 6.0),
        Vec3::new( 1.0,  2.0, 6.0),

        Vec3::new( 1.0,  1.0, -10.0),
        Vec3::new(-1.0,  1.0, -10.0),
        Vec3::new(-1.0, -1.0, -10.0),
        Vec3::new( 1.0, -1.0, -10.0),
        Vec3::new( 1.0,  4.0, -10.0),
        Vec3::new(-1.0,  4.0, -10.0),
        Vec3::new(-1.0,  2.0, -10.0),
        Vec3::new( 1.0,  2.0, -10.0),

        Vec3::new( 5.0, 0.0, 1.0),
        Vec3::new(-5.0, 3.0, 1.0),
        Vec3::new(-5.0, 0.0, 1.0),
        Vec3::new( 5.0, 3.0, 1.0),
    ];
    for pos in lights {
        let lc = colors.next().unwrap();
        let mut tsm = TransformsComponent::new();
        tsm.set_translation(pos);
        tsm.set_scale(Vec3::splat(0.5));
        world.spawn((
            LightComponent::new(Light::Point(PointLight::new(pos, lc))),
            tsm,
            gfc,
        ));
    }

    executor.add_resource(0f64);

    let transforms = {
        let inputs = inputs.clone();
        move |count: &mut f64, wr: &mut WorldRenderer| {
            *count += 1.0;
            let mut changed = false;
            let mut cam_pos = wr.camera.get_position();
            let mut cam_rot = wr.camera.get_rotation();
            let rot = {
                let (y, _, _) = cam_rot.to_euler(EulerRot::YXZ);
                Quat::from_euler(EulerRot::YXZ, y, 0.0, 0.0)
            };
            let fac = 0.01;
            let scale = 0.001;
            if inputs.is_pressed_keycode(VirtualKeyCode::Z) {
                changed = true;
                cam_pos += rot.mul_vec3(Vec3::new(0.0, 0.0, fac));
            }
            if inputs.is_pressed_keycode(VirtualKeyCode::Q) {
                changed = true;
                cam_pos += rot.mul_vec3(Vec3::new(-fac, 0.0, 0.0));
            }
            if inputs.is_pressed_keycode(VirtualKeyCode::S) {
                changed = true;
                cam_pos += rot.mul_vec3(Vec3::new(0.0, 0.0, -fac));
            }
            if inputs.is_pressed_keycode(VirtualKeyCode::D) {
                changed = true;
                cam_pos += rot.mul_vec3(Vec3::new(fac, 0.0, 0.0));
            }
            if inputs.is_pressed_keycode(VirtualKeyCode::Space) {
                changed = true;
                cam_pos += rot.mul_vec3(Vec3::new(0.0, fac, 0.0));
            }
            if inputs.is_pressed_keycode(VirtualKeyCode::Tab) {
                changed = true;
                cam_pos += rot.mul_vec3(Vec3::new(0.0, -fac, 0.0));
            }
            let delta = inputs.get_mouse_delta();
            if delta.length_squared() > 0.0 {
                changed = true;
                let (mut y, mut x, _) = cam_rot.to_euler(EulerRot::YXZ);
                x += delta.y * scale;
                y += delta.x * scale;
                x = x.clamp(-0.4999 * PI, 0.4999 * PI);
                cam_rot = Quat::from_euler(EulerRot::YXZ, y, x, 0.0);
            }
            if changed {
                wr.camera.set_position(cam_pos);
                wr.camera.set_rotation(cam_rot);
            }
        }
    };

    executor.add_resource(Grabbed(false));

    let schedule = executor
        .schedule()
        .then(WorldRenderer::update_lights)
        .then(GraphicContext::render)
        .then(transforms)
        .build();

    event_loop.run(move |event, _, control_flow| match event {
        Event::RedrawRequested(id) if id == window.id() => {
            executor.execute(&schedule, &mut world);
            let gfx = executor.get_resource_mut::<GraphicContext>().unwrap();

            match gfx.feedback() {
                Ok(_) => {}
                Err(wgpu::SurfaceError::Lost) => gfx.resize(gfx.size),
                Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                Err(e) => eprintln!("{e:?}"),
            }
        }
        Event::MainEventsCleared => {
            window.request_redraw();
        }
        Event::WindowEvent {
            window_id,
            ref event,
        } if window_id == window.id() => {
            match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(physical_size) => executor
                    .get_resource_mut::<GraphicContext>()
                    .unwrap()
                    .resize(*physical_size),
                WindowEvent::ScaleFactorChanged { new_inner_size, scale_factor } => {
                    executor
                        .get_resource_mut::<GraphicContext>()
                        .unwrap()
                        .resize(**new_inner_size);
                    executor
                        .get_resource_mut::<EState>()
                        .unwrap()
                        .set_pixels_per_point(*scale_factor as f32);
                }
                _ => {}
            }

            if !**executor.get_resource::<Grabbed>().unwrap() {
                let (estate, ui): (&mut EState, &egui::Context) = executor.query_resources().unwrap();
                if estate.on_event(ui, event) {
                    return;
                }

                if let WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } = event {
                    *executor.get_resource_mut::<Grabbed>().unwrap() = Grabbed(true);
                    window.set_cursor_visible(false);
                    window.set_cursor_grab(true).unwrap();
                }
            } else {
                if let WindowEvent::KeyboardInput { input: KeyboardInput { state: ElementState::Pressed, virtual_keycode: Some(VirtualKeyCode::Escape), .. }, .. } = event {
                    *executor.get_resource_mut::<Grabbed>().unwrap() = Grabbed(false);
                    window.set_cursor_visible(true);
                    window.set_cursor_grab(false).unwrap();
                }
                match event {
                    WindowEvent::CursorMoved { position, .. } => {
                        inputs.notify_mouse(*position);
                        window.set_cursor_position(CENTER_POS).unwrap();
                    }
                    WindowEvent::KeyboardInput { input, .. } => {
                        inputs.notify(*input);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    });
}

fn main() {
    env_logger::init();

    //let mut client = Client::new("127.0.0.1:50000").unwrap();
    //let _peer = Client::new("127.0.0.1:50001").unwrap();
    //client.request_game("127.0.0.1:50001").unwrap();
    //thread::sleep(Duration::from_secs(2));

    let world = World::new();
    let executor = Executor::new();
    pollster::block_on(run(world, executor));
}
