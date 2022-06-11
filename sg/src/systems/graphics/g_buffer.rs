use crate::systems::graphics::Light;
use wgpu::util::BufferInitDescriptor;
use wgpu::util::DeviceExt;



pub struct GBuffer {
    pub albedo_tex: wgpu::TextureView,
    pub position_tex: wgpu::TextureView,
    pub normal_tex: wgpu::TextureView,
    pub depth_tex: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub directional_lights_buffer: wgpu::Buffer,
    pub point_lights_buffer: wgpu::Buffer,
    pub spot_lights_buffer: wgpu::Buffer,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bindgroup: wgpu::BindGroup,
}

impl GBuffer {
    fn make_textures(device: &wgpu::Device, size: wgpu::Extent3d) -> (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView, wgpu::TextureView) {
        let tex = |label, format|
            device.create_texture(&wgpu::TextureDescriptor {
                size,
                label: Some(label),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                dimension: wgpu::TextureDimension::D2,
                format,
                sample_count: 1,
                mip_level_count: 1,
            }).create_view(&wgpu::TextureViewDescriptor::default());
        (
            tex("albedo", wgpu::TextureFormat::Rgba8Unorm),
            tex("position", wgpu::TextureFormat::Rgba32Float),
            tex("normal", wgpu::TextureFormat::Rgba16Float),
            tex("depth", wgpu::TextureFormat::Depth32Float),
        )
    }
    fn update_bindgroup(&mut self, device: &wgpu::Device) {
        self.bindgroup = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("g buffer bindgroup"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler)
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.albedo_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.position_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.normal_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.depth_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        size: None,
                        buffer: &self.directional_lights_buffer,
                        offset: 0,
                    })
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        size: None,
                        buffer: &self.point_lights_buffer,
                        offset: 0,
                    })
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        size: None,
                        buffer: &self.spot_lights_buffer,
                        offset: 0,
                    })
                },
            ]
        });
    }
    fn make_lights_buffer(device: &wgpu::Device, lights: &[Light]) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
        let mut dlights = Vec::with_capacity(lights.len());
        let mut plights = Vec::with_capacity(lights.len());
        let mut slights = Vec::with_capacity(lights.len());
        for l in lights {
            match l {
                Light::Directional(l) => dlights.push(*l),
                Light::Point(l) => plights.push(*l),
                Light::Spot(l) => slights.push(*l),
            }
        }
        macro_rules! buf {
            ($label:literal, $data:expr) => {
                {
                    let mut bytes = Vec::with_capacity(4 + $data.len() * 30); // over allocate
                    bytes.extend_from_slice(bytemuck::bytes_of(&($data.len() as u32)));
                    bytes.extend_from_slice(bytemuck::cast_slice($data));
                    device.create_buffer_init(&BufferInitDescriptor {
                        label: Some($label),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        contents: &bytes
                    })
                }
            };
        }
        (
            buf!("directional lights buffer", &dlights),
            buf!("point lights buffer", &plights),
            buf!("spot lights buffer", &slights),
        )
    }
    pub fn new(device: &wgpu::Device, size: wgpu::Extent3d, lights: &[Light]) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Gbuffer bind group layout"),
            entries: &[
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 0,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering) 
                },
                // albedo
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 1,
                    ty: wgpu::BindingType::Texture {
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    }
                },
                // position
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 2,
                    ty: wgpu::BindingType::Texture {
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    }
                },
                // normals
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 3,
                    ty: wgpu::BindingType::Texture {
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    }
                },
                // depth
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 4,
                    ty: wgpu::BindingType::Texture {
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    }
                },
                // directional lights
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 5,
                    ty: wgpu::BindingType::Buffer { 
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
                },
                // point lights
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 6,
                    ty: wgpu::BindingType::Buffer { 
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
                },
                // spot lights
                wgpu::BindGroupLayoutEntry {
                    count: None,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    binding: 7,
                    ty: wgpu::BindingType::Buffer { 
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
                }
            ]
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gbuffer sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let (albedo_tex, position_tex, normal_tex, depth_tex) = Self::make_textures(device, size);
        let (
            directional_lights_buffer,
            point_lights_buffer,
            spot_lights_buffer,
        ) = Self::make_lights_buffer(device, lights);

        let bindgroup = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("g buffer bindgroup"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&sampler)
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&albedo_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&position_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&normal_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&depth_tex)
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        size: None,
                        buffer: &directional_lights_buffer,
                        offset: 0,
                    })
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        size: None,
                        buffer: &point_lights_buffer,
                        offset: 0,
                    })
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        size: None,
                        buffer: &spot_lights_buffer,
                        offset: 0,
                    })
                },
            ]
        });
        Self {
            sampler,
            albedo_tex,
            position_tex,
            normal_tex,
            depth_tex,
            bind_group_layout,
            bindgroup,
            directional_lights_buffer,
            point_lights_buffer,
            spot_lights_buffer,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: wgpu::Extent3d) {
        let (albedo_tex, position_tex, normal_tex, depth_tex) = Self::make_textures(device, size);
        self.albedo_tex = albedo_tex;
        self.position_tex = position_tex;
        self.normal_tex = normal_tex;
        self.depth_tex = depth_tex;
        self.update_bindgroup(device);
    }

    pub fn update_lights(&mut self, device: &wgpu::Device, lights: &[Light]) {
        let (
            directional_lights_buffer,
            point_lights_buffer,
            spot_lights_buffer,
        ) = Self::make_lights_buffer(device, lights);
        self.directional_lights_buffer = directional_lights_buffer;
        self.point_lights_buffer = point_lights_buffer;
        self.spot_lights_buffer = spot_lights_buffer;
    }
}
