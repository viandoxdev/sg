#![feature(trait_upcasting)]
#![feature(once_cell)]
#![allow(incomplete_features)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;

use ecs::{Executor, World};
use glam::{EulerRot, Quat, Vec2, Vec3, Vec4};
use parking_lot::RwLock;
use slotmap::SlotMap;
use systems::graphics::{gltf, graphic_system, lights_system, GraphicContext, Light, PointLight};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event, KeyboardInput, ScanCode, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

use components::LightComponent;

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
    gfx.camera.set_position(Vec3::new(0.0, 0.0, 2.0));
    gfx.camera.set_rotation(Quat::from_rotation_y(PI));
    world.spawn_many(gltf::open("armor.glb", &mut gfx).expect("Error"));
    let inputs = Arc::new(InputState::new());

    executor.add_resource(gfx);

    //let entity;

    //{
    //    let gfx = ecs.get_system_mut::<GraphicSystem>().unwrap();

    //    let mesh = gfx.mesh_manager.add(&gfx.device, &Mesh::new_cube());
    //    let material = {
    //        let albedo = image::open("albedo.png").unwrap().flipv();
    //        let albedo = gfx
    //            .texture_manager
    //            .add_image_texture(&gfx.device, &gfx.queue, albedo);
    //        let norm = image::open("norm.png").unwrap().flipv();
    //        let norm = gfx
    //            .texture_manager
    //            .add_image_texture(&gfx.device, &gfx.queue, norm);
    //        let met = gfx.texture_manager.get_or_add_single_value_texture(
    //            &gfx.device,
    //            &gfx.queue,
    //            SingleValue::Factor(0.0),
    //        );
    //        let roughness = image::open("roughness.png").unwrap().flipv();
    //        let roughness = gfx
    //            .texture_manager
    //            .add_image_texture(&gfx.device, &gfx.queue, roughness);
    //        let ao = image::open("ao.png").unwrap().flipv();
    //        let ao = gfx
    //            .texture_manager
    //            .add_image_texture(&gfx.device, &gfx.queue, ao);
    //        Material::new(albedo, Some(norm), met, roughness, Some(ao), gfx).unwrap()
    //    };

    //    let gfc = GraphicsComponent {
    //        mesh,
    //        material,
    //    };

    //    let mut tsm = TransformsComponent::new();
    //    tsm.set_translation(Vec3::new(0.0, 0.0, 0.0));

    //    entity = ecs.new_entity();
    //    ecs.add_component(entity, gfc);
    //    ecs.add_component(entity, tsm);
    //}

    let pos = [
        Vec3::new(1.0, 1.0, 6.0),
        Vec3::new(-1.0, 1.0, 6.0),
        Vec3::new(-1.0, -1.0, 6.0),
        Vec3::new(1.0, -1.0, 6.0),
        Vec3::new(1.0, 4.0, 6.0),
        Vec3::new(-1.0, 4.0, 6.0),
        Vec3::new(-1.0, 2.0, 6.0),
        Vec3::new(1.0, 2.0, 6.0),
    ];
    let lc = Vec4::splat(27.0);
    for pos in pos {
        world.spawn((LightComponent::new(Light::Point(PointLight::new(pos, lc))),));
    }

    executor.add_resource(0f64);

    let transforms = {
        let inputs = inputs.clone();
        move |count: &mut f64, gfx: &mut GraphicContext| {
            *count += 1.0;
            let mut changed = false;
            let mut cam_pos = gfx.camera.get_position();
            let mut cam_rot = gfx.camera.get_rotation();
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
                gfx.camera.set_position(cam_pos);
                gfx.camera.set_rotation(cam_rot);
            }
        }
    };

    let schedule = executor
        .schedule()
        .then(lights_system)
        .then(transforms)
        .then(graphic_system)
        .build();
    let mut grabbed = false;

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
        } if window_id == window.id() => match event {
            WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
            WindowEvent::Resized(physical_size) => executor
                .get_resource_mut::<GraphicContext>()
                .unwrap()
                .resize(*physical_size),
            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => executor
                .get_resource_mut::<GraphicContext>()
                .unwrap()
                .resize(**new_inner_size),
            WindowEvent::CursorMoved { position, .. } => {
                if grabbed {
                    inputs.notify_mouse(*position);
                    window.set_cursor_position(CENTER_POS).unwrap();
                }
            }
            WindowEvent::Focused(_) => {
                grabbed = !grabbed;
                window.set_cursor_visible(!grabbed);
                window.set_cursor_grab(grabbed).unwrap();
            }
            WindowEvent::KeyboardInput { input, .. } => {
                if grabbed {
                    inputs.notify(*input);
                }
            }
            _ => {}
        },
        _ => {}
    });
}
// TODO: implement more hooks on systems (pre, post, init), and refractor the macros if I can
// manage.

fn main() {
    pretty_env_logger::init();

    //let mut client = Client::new("127.0.0.1:50000").unwrap();
    //let _peer = Client::new("127.0.0.1:50001").unwrap();
    //client.request_game("127.0.0.1:50001").unwrap();
    //thread::sleep(Duration::from_secs(2));

    let world = World::new();
    let executor = Executor::new();
    pollster::block_on(run(world, executor));
}
