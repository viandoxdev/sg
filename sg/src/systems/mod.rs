use ecs::{system_pass, System};
use winit::window::Window;

use crate::components::PositionComponent;

pub struct GravitySystem {
    g: f64
}

impl System for GravitySystem {
    #[system_pass]
    fn pass(&mut self, pos: PositionComponent) {
        pos.z -= self.g;
    }
}

pub struct CenterSystem {
    pub res: PositionComponent
}

impl System for CenterSystem {
    #[system_pass]
    fn pass_many(&mut self, entities: Vec<(PositionComponent)>) {
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
        self.res = pos;
        log::debug!("Ran CenterSystem => {pos:?}");
    }
}

pub struct LoggingSystem {}

impl System for LoggingSystem {
    #[system_pass]
    fn pass(&mut self, pos: PositionComponent) {
        log::debug!("{pos:?}");
    }
}

pub struct GraphicSystem {
    pub surface: wgpu::Surface,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
}

impl System for GraphicSystem {
    fn name() -> &'static str {
        "GraphicSystem"
    }
    #[system_pass]
    fn pass(&mut self, _pos: PositionComponent) -> () {}
}

impl GraphicSystem {
    pub async fn new(window: &Window) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::Backends::all());
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
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();
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
