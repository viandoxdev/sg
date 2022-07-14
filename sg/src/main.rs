#![feature(trait_upcasting)]
#![feature(once_cell)]
#![allow(incomplete_features)]
#![allow(dead_code)]

use ecs::{World, Executor};
use glam::{Quat, Vec3, Vec4};
use systems::graphics::mesh_manager::{Mesh, Primitives};
use systems::graphics::texture_manager::SingleValue;
use systems::graphics::{gltf, GraphicContext, Light, Material, PointLight, lights_system, graphic_system};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

use components::LightComponent;

mod chess;
pub mod components;
pub mod systems;

async fn run(mut world: World, mut executor: Executor) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let mut gfx = GraphicContext::new(&window).await;
    gfx.camera.set_position(Vec3::new(0.0, 7.0, -8.0));
    let entities = gltf::open("model.glb", &mut gfx).expect("Error");

    world.spawn_many(entities);
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
        Vec3::new(1.0, 1.0, -3.0),
        Vec3::new(-1.0, 1.0, -3.0),
        Vec3::new(-1.0, -1.0, -3.0),
        Vec3::new(1.0, -1.0, -3.0),
        Vec3::new(1.0, 4.0, -3.0),
        Vec3::new(-1.0, 4.0, -3.0),
        Vec3::new(-1.0, 2.0, -3.0),
        Vec3::new(1.0, 2.0, -3.0),
    ];
    let lc = Vec4::splat(27.0);
    for pos in pos {
        world.spawn((
            LightComponent::new(Light::Point(PointLight::new(pos, lc)))
        ,));
    }

    let mut count = 0f64;
    let schedule = executor.schedule()
        .then(lights_system)
        .then(graphic_system)
        .build();

    event_loop.run(move |event, _, control_flow| match event {
        Event::RedrawRequested(id) if id == window.id() => {
            count += 1.0;

            executor.execute(&schedule, &mut world);

            //ecs.get_component_mut::<TransformsComponent>(entity)
            //    .unwrap()
            //    .set_rotation(Quat::from_rotation_y((count as f32 / 300.0).cos()));
            //ecs.get_component_mut::<TransformsComponent>(entity)
            //    .unwrap()
            //    .set_translation(Vec3::new(0.0, (count / 100.0).cos() as f32 / 2.0, 2.0));
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
