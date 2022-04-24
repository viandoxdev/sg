#![feature(trait_upcasting)]
#![feature(min_specialization)]
#![allow(incomplete_features)]
#![allow(dead_code)]

use ecs::systems::GraphicSystem;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{EventLoop, ControlFlow};
use winit::window::WindowBuilder;

use crate::ecs::ECS;
use crate::ecs::components::PositionComponent;
use crate::ecs::systems::{GravitySystem, CenterSystem, LoggingSystem};

mod utils;
mod ecs;

async fn run(ecs: &mut ECS) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().with_title("Test").build(&event_loop).unwrap();

    let gfx = GraphicSystem::new(&window).await;
    ecs.register_system(gfx, "graphics");

    event_loop.run(move  |event, _, control_flow| match event {
        Event::WindowEvent { window_id, ref event } if window_id == window.id() => match event {
            WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
            _ => {}
        }
        _ =>  {}
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

    pollster::block_on(run(&mut ecs));
}
