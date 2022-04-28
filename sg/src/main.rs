#![feature(trait_upcasting)]
#![allow(incomplete_features)]
#![allow(dead_code)]

use systems::graphics::{Vertex, GraphicSystem};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

use components::{PositionComponent, GraphicsComponent};
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
    ecs.register_component::<GraphicsComponent>();
    {
        let gfx = ecs.get_system_mut::<GraphicSystem>().unwrap();
        let square = gfx.add_mesh(&[
                Vertex { position: [-0.5, -0.5, -1.0], color: [1.0, 0.0, 0.0] },
                Vertex { position: [ 0.5, -0.5, -1.0], color: [1.0, 1.0, 0.0] },
                Vertex { position: [ 0.5,  0.5, -1.0], color: [0.0, 1.0, 0.0] },
                Vertex { position: [-0.5,  0.5, -1.0], color: [0.0, 1.0, 1.0] },
            ],
            &[
                [0, 1, 2],
                [0, 2, 3],
            ]
        );
        let entity = ecs.new_entity();
        ecs.add_component(entity, GraphicsComponent {
            mesh: square
        });
    }


    event_loop.run(move |event, _, control_flow| match event {
        Event::RedrawRequested(id) if id == window.id() => {
            ecs.run_systems("graphics");
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
