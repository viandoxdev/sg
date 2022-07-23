use std::num::NonZeroU64;

use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::systems::graphics::Light;

trait Align {
    fn align(self, rhs: Self) -> Self;
}

impl Align for u64 {
    fn align(self, rhs: Self) -> Self {
        (self + rhs - 1) / rhs * rhs
    }
}

impl Align for usize {
    fn align(self, rhs: Self) -> Self {
        (self + rhs - 1) / rhs * rhs
    }
}

pub struct GBuffer {
    pub albedo_tex: wgpu::TextureView,
    pub position_tex: wgpu::TextureView,
    pub normal_tex: wgpu::TextureView,
    pub mra_tex: wgpu::TextureView,
    pub depth_tex: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub lights_buffer: wgpu::Buffer,
    pub bindgroup: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub max_lights: u32,
}

impl GBuffer {
    fn make_textures(device: &wgpu::Device, size: wgpu::Extent3d) -> [wgpu::TextureView; 5] {
        let tex = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    size,
                    label: Some(label),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    sample_count: 1,
                    mip_level_count: 1,
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        [
            tex("albedo", wgpu::TextureFormat::Rgba8UnormSrgb),
            tex("position", wgpu::TextureFormat::Rgba16Float),
            tex("normal", wgpu::TextureFormat::Rgba16Float),
            tex("metallic roughness ao", wgpu::TextureFormat::Rgba8Unorm),
            tex("depth", wgpu::TextureFormat::Depth32Float),
        ]
    }
    // This is just a function to avoid repeats
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn make_bindgroup(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        albedo_tex: &wgpu::TextureView,
        position_tex: &wgpu::TextureView,
        normal_tex: &wgpu::TextureView,
        mra_tex: &wgpu::TextureView,
        depth_tex: &wgpu::TextureView,
        lights_buffer: &wgpu::Buffer,
        max_lights: u32,
    ) -> wgpu::BindGroup {
        let max_lights = max_lights as u64;
        let alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let dlights_offset = 0;
        let plights_offset = (16 + max_lights * 32).align(alignment);
        let slights_offset = (32 + max_lights * 64).align(alignment);
        create_bind_group!(device, layout, "GBuffer Bindgroup": {
            0 | Sampler(sampler),
            1 | TextureView(albedo_tex),
            2 | TextureView(position_tex),
            3 | TextureView(normal_tex),
            4 | TextureView(mra_tex),
            5 | TextureView(depth_tex),
            6 | Buffer(
                size: (Some(NonZeroU64::new(16 + max_lights * 32).unwrap())),
                buffer: lights_buffer,
                offset: dlights_offset,
            ),
            7 | Buffer(
                size: (Some(NonZeroU64::new(16 + max_lights * 32).unwrap())),
                buffer: lights_buffer,
                offset: plights_offset,
            ),
            8 | Buffer(
                size: (Some(NonZeroU64::new(16 + max_lights * 48).unwrap())),
                buffer: lights_buffer,
                offset: slights_offset,
            ),
        })
    }
    fn update_bindgroup(&mut self, device: &wgpu::Device) {
        self.bindgroup = Self::make_bindgroup(
            device,
            &self.bind_group_layout,
            &self.sampler,
            &self.albedo_tex,
            &self.position_tex,
            &self.normal_tex,
            &self.mra_tex,
            &self.depth_tex,
            &self.lights_buffer,
            self.max_lights,
        );
    }
    fn make_lights_buffer<'a>(
        device: &wgpu::Device,
        lights: impl IntoIterator<Item = &'a Light>,
        max: u32,
    ) -> (wgpu::Buffer, u32) {
        let mut dlights = Vec::with_capacity(max as usize);
        let mut plights = Vec::with_capacity(max as usize);
        let mut slights = Vec::with_capacity(max as usize);
        for l in lights {
            match l {
                Light::Directional(l) => dlights.push(*l),
                Light::Point(l) => plights.push(*l),
                Light::Spot(l) => slights.push(*l),
            }
        }
        let max = max as usize;
        let alignment = device.limits().min_uniform_buffer_offset_alignment as usize;
        let dlights_bytes = (16 + max * 32).align(alignment); // 12 padding + 4 u32 bytes for length
        let plights_bytes = (16 + max * 32).align(alignment);
        let slights_bytes = 16 + max * 48; // no alignment because last
        let mut bytes: Vec<u8> = Vec::with_capacity(dlights_bytes + plights_bytes + slights_bytes);
        {
            // directional
            let len = dlights.len().min(max);
            bytes.extend_from_slice(bytemuck::bytes_of(&(len as u32))); // length field
            bytes.extend(std::iter::repeat(0).take(12)); // padding to 16 align the length
            bytes.extend_from_slice(bytemuck::cast_slice(&dlights[0..len as usize])); // push lights
            bytes.extend(std::iter::repeat(0).take(dlights_bytes - len * 32 - 16));
            // fill the rest with zeros
        }
        {
            // point
            let len = plights.len().min(max);
            bytes.extend_from_slice(bytemuck::bytes_of(&(len as u32)));
            bytes.extend(std::iter::repeat(0).take(12)); // padding to 16 align the length
            bytes.extend_from_slice(bytemuck::cast_slice(&plights[0..len as usize]));
            bytes.extend(std::iter::repeat(0).take(plights_bytes - len * 32 - 16));
        }
        {
            // spot
            let len = slights.len().min(max);
            bytes.extend_from_slice(bytemuck::bytes_of(&(len as u32)));
            bytes.extend(std::iter::repeat(0).take(12)); // padding to 16 align the length
            bytes.extend_from_slice(bytemuck::cast_slice(&plights[0..len as usize]));
            bytes.extend(std::iter::repeat(0).take(slights_bytes - len * 48 - 16));
        }
        let buf = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("lights buffer"),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            contents: &bytes,
        });
        let overflow = dlights
            .len()
            .saturating_sub(max)
            .max(plights.len().saturating_sub(max))
            .max(slights.len().saturating_sub(max));
        (buf, overflow as u32)
    }
    pub fn new(
        device: &wgpu::Device,
        size: wgpu::Extent3d,
        lights: &[Light],
        max_lights: u32,
    ) -> Self {
        let bind_group_layout = create_bind_group_layout!(device, "GBuffer Bind Group Layout": {
            0 => FRAGMENT | Sampler(Filtering),
            1 => FRAGMENT | Texture(view_dim: D2, sample: FloatFilterable),
            2 => FRAGMENT | Texture(view_dim: D2, sample: FloatFilterable),
            3 => FRAGMENT | Texture(view_dim: D2, sample: FloatFilterable),
            4 => FRAGMENT | Texture(view_dim: D2, sample: FloatFilterable),
            5 => FRAGMENT | Texture(view_dim: D2, sample: Depth),
            6 => FRAGMENT | Buffer(type: Uniform),
            7 => FRAGMENT | Buffer(type: Uniform),
            8 => FRAGMENT | Buffer(type: Uniform),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gbuffer sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let [albedo_tex, position_tex, normal_tex, mra_tex, depth_tex] =
            Self::make_textures(device, size);
        let (lights_buffer, overflow) = Self::make_lights_buffer(device, lights, max_lights);

        if overflow > 0 {
            log::warn!("Lights exceed the limit of {max_lights}");
        }

        let bindgroup = Self::make_bindgroup(
            device,
            &bind_group_layout,
            &sampler,
            &albedo_tex,
            &position_tex,
            &normal_tex,
            &mra_tex,
            &depth_tex,
            &lights_buffer,
            max_lights,
        );

        Self {
            sampler,
            albedo_tex,
            position_tex,
            normal_tex,
            mra_tex,
            depth_tex,
            bind_group_layout,
            bindgroup,
            lights_buffer,
            max_lights,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: wgpu::Extent3d) {
        let [albedo_tex, position_tex, normal_tex, mra_tex, depth_tex] =
            Self::make_textures(device, size);
        self.albedo_tex = albedo_tex;
        self.position_tex = position_tex;
        self.normal_tex = normal_tex;
        self.mra_tex = mra_tex;
        self.depth_tex = depth_tex;
        self.update_bindgroup(device);
    }

    pub fn update_lights<'a>(
        &mut self,
        device: &wgpu::Device,
        lights: impl IntoIterator<Item = &'a Light>,
    ) -> Result<(), u32> {
        let (lights_buffer, overflow) = Self::make_lights_buffer(device, lights, self.max_lights);
        self.lights_buffer = lights_buffer;
        self.update_bindgroup(device);
        if overflow > 0 {
            Err(overflow)
        } else {
            Ok(())
        }
    }
}
