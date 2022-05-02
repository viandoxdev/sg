#![feature(trait_upcasting)]
#![allow(incomplete_features)]
#![allow(dead_code)]

use glam::Vec3;
use systems::graphics::{Vertex, GraphicSystem};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

use components::{PositionComponent, GraphicsComponent, TransformsComponent};
use ecs::{ECS, owned_entity};
use systems::{GravitySystem, CenterSystem, LoggingSystem};
//use systems::{GravitySystem, CenterSystem, LoggingSystem};

mod components;
mod systems;

async fn run(mut ecs: ECS) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .build(&event_loop)
        .unwrap();

    let gfx = GraphicSystem::new(&window).await;
    ecs.register_system(gfx, "graphics");
    ecs.register_component::<TransformsComponent>();
    ecs.register_component::<GraphicsComponent>();

    let entity;

    {
        let gfx = ecs.get_system_mut::<GraphicSystem>().unwrap();
        let square = gfx.mesh_manager.add(&gfx.device,
            &[
                Vertex { position: [-0.5, -0.5, 0.0], tex_coords: [1.0, 0.0] },
                Vertex { position: [ 0.5, -0.5, 0.0], tex_coords: [0.0, 0.0] },
                Vertex { position: [ 0.5,  0.5, 0.0], tex_coords: [0.0, 1.0] },
                Vertex { position: [-0.5,  0.5, 0.0], tex_coords: [1.0, 1.0] },
            ],
            &[
                [0, 1, 2],
                [0, 2, 3],
            ],
        );
        let img = image::load_from_memory(include_bytes!("../tex.jpg")).unwrap();
        let set = gfx.texture_manager.add_set();
        let tex = gfx.texture_manager.add_texture(img, set).unwrap();
        let mut tsm = TransformsComponent::new();
        tsm.set_scale(Vec3::new(2.0, 1.0, 1.0));
        entity = ecs.new_entity();
        ecs.add_component(entity, GraphicsComponent {
            mesh: square,
            texture: tex
        });
        ecs.add_component(entity, tsm);
    }

    let mut count = 0f64;

    event_loop.run(move |event, _, control_flow| match event {
        Event::RedrawRequested(id) if id == window.id() => {
            count += 1.0;
            ecs.run_systems("graphics");

            ecs.get_component_mut::<TransformsComponent>(entity).unwrap().set_scale(Vec3::new(
                (count / 100.0).sin() as f32 + 1.0,
                (count / 100.0).cos() as f32 + 1.0, 
                1.0
            ));
            let gfx = ecs.get_system_mut::<GraphicSystem>().unwrap();
            match gfx.feedback() {
                Ok(_) => {}
                Err(wgpu::SurfaceError::Lost) => gfx.resize(gfx.size),
                Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                Err(e) => eprintln!("{e:?}"),
            }
        },
        Event::MainEventsCleared => {
            window.request_redraw();
        },
        Event::WindowEvent {
            window_id,
            ref event,
        } if window_id == window.id() => match event {
            WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
            WindowEvent::Resized(physical_size) => ecs.get_system_mut::<GraphicSystem>().unwrap().resize(*physical_size),
            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => 
                ecs.get_system_mut::<GraphicSystem>().unwrap().resize(**new_inner_size),
            _ => {}
        },
        _ => {}
    });
}
// TODO: implement more hooks on systems (pre, post, init), and refractor the macros if I can
// manage.

fn main() {
    pretty_env_logger::init();

    let mut ecs = ECS::new();
    ecs.register_component::<PositionComponent>();
    ecs.register_system(GravitySystem { g: 4.0 }, "gravity");
    ecs.register_system(CenterSystem { res: PositionComponent { x: 0.0, y: 0.0, z: 0.0 }}, "gravity");
    ecs.register_system(LoggingSystem {}, "log");

    ecs.add_entity(owned_entity!{
        PositionComponent {
            x: 0.0, y: 0.0, z: 1.0
        }
    });
    
    ecs.run_systems("log");
    ecs.run_systems("gravity");
    ecs.run_systems("log");

    pollster::block_on(run(ecs));
}
