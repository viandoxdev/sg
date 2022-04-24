use winit::window::Window;

use crate::make_system;

use super::components::PositionComponent;

make_system! {
    GravitySystem {
        g: f64
    }

    run(sys, pos: PositionComponent) {
        pos.z -= sys.g;
    }
}
make_system! {
    CenterSystem {
        res: PositionComponent
    }

    run_many(sys, entities: Vec<(PositionComponent)>) {
        let mut pos = PositionComponent {
            x: 0.0, y: 0.0, z: 0.0
        };
        let len = entities.len();
        for epos in entities {
            pos.x += epos.x;
            pos.y += epos.y;
            pos.z += epos.z;
        }
        pos.x /= len as f64;
        pos.y /= len as f64;
        pos.z /= len as f64;
        sys.res = pos;
        log::debug!("Ran CenterSystem => {pos:?}");
    }
}
make_system!{
    LoggingSystem {}

    run(_sys, pos: PositionComponent) {
        log::debug!("{pos:?}");
    }
}


make_system!{
    GraphicSystem {
        surface: wgpu::Surface,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        size: winit::dpi::PhysicalSize<u32>,
    }
    run(_gfx, _pos: PositionComponent) {

    }
}

impl GraphicSystem {
    pub async fn new(window: &Window) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::Backends::all());
        let surface = unsafe { instance.create_surface(window) };
        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }
        ).await.unwrap();
        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                features: wgpu::Features::empty(),
                limits: wgpu::Limits::default(),
                label: None,
            }, None
        ).await.unwrap();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_preferred_format(&adapter).unwrap(),
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };
        surface.configure(&device, &config);

        Self {
            surface,
            device,
            queue,
            config,
            size,
        }
    }
}
