use std::{
    cell::UnsafeCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    lazy::OnceCell,
};

use anyhow::{Context, Result};
use glam::{Vec3, Vec4};
use image::DynamicImage;
use slotmap::{SecondaryMap, SlotMap};

slotmap::new_key_type! {
    pub struct TextureHandle;
    pub struct TextureSet;
}

pub enum SingleValuePurpose {}

#[derive(Clone, Copy)]
pub enum SingleValue {
    /// The value represents a color (implies TextureFormat::Rgba8UnormSrgb)
    Color(Vec4),
    /// The value represents a normal (implies TextureFormat::Rgba8Unorm)
    Normal(Vec3),
    /// The value is any single float (implies TextureFormat::R32Float)
    Float(f32),
    /// The value represents a single float from 0 to 1 (implies TextureFormat::R8Unorm)
    Factor(f32),
}

impl SingleValue {
    fn format(&self) -> wgpu::TextureFormat {
        match self {
            Self::Color(_) => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::Normal(_) => wgpu::TextureFormat::Rgba8Unorm,
            Self::Float(_) => wgpu::TextureFormat::R32Float,
            Self::Factor(_) => wgpu::TextureFormat::R8Unorm,
        }
    }
}

impl Hash for SingleValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            SingleValue::Color(c) => {
                state.write_u8(0);
                [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()].hash(state);
            }
            SingleValue::Normal(n) => {
                state.write_u8(1);
                [n.x.to_bits(), n.y.to_bits(), n.z.to_bits()].hash(state);
            }
            SingleValue::Float(f) => {
                state.write_u8(2);
                f.to_bits().hash(state);
            }
            SingleValue::Factor(f) => {
                state.write_u8(3);
                f.to_bits().hash(state);
            }
        }
    }
}

impl PartialEq for SingleValue {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Self::Color(a) => {
                if let Self::Color(b) = other {
                    a.x.to_bits() == b.x.to_bits()
                        && a.z.to_bits() == b.y.to_bits()
                        && a.y.to_bits() == b.z.to_bits()
                        && a.w.to_bits() == b.w.to_bits()
                } else {
                    false
                }
            }
            Self::Normal(a) => {
                if let Self::Normal(b) = other {
                    a.x.to_bits() == b.x.to_bits()
                        && a.z.to_bits() == b.y.to_bits()
                        && a.y.to_bits() == b.z.to_bits()
                } else {
                    false
                }
            }
            Self::Float(a) => {
                if let Self::Float(b) = other {
                    a.to_bits() == b.to_bits()
                } else {
                    false
                }
            }
            Self::Factor(a) => {
                if let Self::Factor(b) = other {
                    a.to_bits() == b.to_bits()
                } else {
                    false
                }
            }
        }
    }
}

impl Eq for SingleValue {}

pub struct TextureManager {
    textures: SlotMap<TextureHandle, wgpu::TextureView>,
    /// All sets existing in the TextureManager (mapped to their textures)
    sets: SlotMap<TextureSet, Vec<TextureHandle>>,
    /// The set mapped to each texture
    textures_set: SecondaryMap<TextureHandle, Vec<TextureSet>>,
    /// currently cached set bind_groups, all groups have to be valid (i.e. contain all textures
    /// assigned to the set)
    cache_bind_groups: UnsafeCell<SecondaryMap<TextureSet, wgpu::BindGroup>>,
    /// Cache for bindgroup layout
    bind_group_layout: OnceCell<wgpu::BindGroupLayout>,
    /// Cache for sampler
    sampler: OnceCell<wgpu::Sampler>,
    /// Cache for single value textures
    single_value_cache: HashMap<SingleValue, TextureHandle>,
    /// Same but opposit direction
    texture_value: SecondaryMap<TextureHandle, SingleValue>,
}

impl TextureManager {
    pub const TEXTURE_SET_MAX: u32 = 16;
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    pub fn new() -> Self {
        Self {
            sets: SlotMap::with_key(),
            textures_set: SecondaryMap::new(),
            textures: SlotMap::with_key(),
            cache_bind_groups: UnsafeCell::new(SecondaryMap::new()),
            bind_group_layout: OnceCell::new(),
            sampler: OnceCell::new(),
            single_value_cache: HashMap::new(),
            texture_value: SecondaryMap::new(),
        }
    }

    /// Create a new set
    pub fn add_set(&mut self) -> TextureSet {
        self.sets.insert(vec![])
    }

    pub fn add_image_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: DynamicImage,
    ) -> TextureHandle {
        let tex = Self::create_texture(device, queue, img);
        self.add_texture(tex)
    }

    pub fn add_depth_texture(
        &mut self,
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> TextureHandle {
        let tex = Self::create_depth_texture(device, config);
        self.add_texture(tex)
    }

    /// Return a handle to a texture with the SingleValue as contant, may create it if needed
    /// A SingleValue texture is a 1x1 pixel texture with a specific value.
    pub fn get_or_add_single_value_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        value: SingleValue,
    ) -> TextureHandle {
        match self.single_value_cache.get(&value) {
            Some(handle) => *handle,
            None => {
                let tex = Self::create_single_value_texture(device, queue, value);
                let handle = self.add_texture(tex);
                self.single_value_cache.insert(value, handle);
                self.texture_value.insert(handle, value);
                handle
            }
        }
    }

    /// Add a texture to the TextureManager and assign it to a set
    pub fn add_texture(&mut self, tex: wgpu::TextureView) -> TextureHandle {
        let handle = self.textures.insert(tex);
        self.textures_set.insert(handle, Vec::new());
        handle
    }

    pub fn add_texture_to_set(&mut self, tex: TextureHandle, set: TextureSet) -> Result<()> {
        self.textures.get(tex).context("No such texture")?;
        self.sets.get_mut(set).context("No such set")?.push(tex);
        self.textures_set.get_mut(tex).unwrap().push(set);
        Ok(())
    }

    pub fn get_view(&self, tex: TextureHandle) -> Option<&wgpu::TextureView> {
        self.textures.get(tex)
    }

    /// Swap the tetures of a and b, making the texture of a go to b and the texture of b go to a
    pub fn swap(&mut self, a: TextureHandle, b: TextureHandle) -> Result<()> {
        // delete cached bind groups
        let sets = self
            .textures_set
            .get(a)
            .context("No such texture")?
            .iter()
            .chain(self.textures_set.get(b).context("No such texture")?.iter());
        for set in sets {
            self.cache_bind_groups.get_mut().remove(*set);
        }
        {
            let [ma, mb] = self.textures.get_disjoint_mut([a, b]).unwrap();
            std::mem::swap(ma, mb);
        }
        let av = self.texture_value.get(a).copied();
        let bv = self.texture_value.get(b).copied();

        if let Some(value) = av {
            self.texture_value.insert(b, value);
            self.single_value_cache.insert(value, b);
        } else {
            self.texture_value.remove(b);
        }

        if let Some(value) = bv {
            self.texture_value.insert(a, value);
            self.single_value_cache.insert(value, a);
        } else {
            self.texture_value.remove(a);
        }
        Ok(())
    }

    pub fn replace_texture(
        &mut self,
        tex: TextureHandle,
        new_tex: wgpu::TextureView,
    ) -> Result<()> {
        *self
            .textures
            .get_mut(tex)
            .context("Can't replace unknown texture")? = new_tex;
        for set in self.textures_set.get(tex).unwrap() {
            // delete cache as it has a reference to the old view.
            self.cache_bind_groups.get_mut().remove(*set);
        }
        // If this texture was a single value texture, forget about it as we have no way of telling
        // if it is still the case
        if let Some(value) = self.texture_value.get(tex) {
            self.single_value_cache.remove(value);
            self.texture_value.remove(tex);
        }
        Ok(())
    }

    pub fn layout(&self, device: &wgpu::Device) -> &wgpu::BindGroupLayout {
        self.bind_group_layout.get_or_init(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: std::num::NonZeroU32::new(Self::TEXTURE_SET_MAX),
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("TextureSet bind group layout"),
            })
        })
    }

    pub fn sampler(&self, device: &wgpu::Device) -> &wgpu::Sampler {
        self.sampler.get_or_init(|| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                label: Some("TextureSet sampler"),
                ..Default::default()
            })
        })
    }

    // /!\ This contains unsafe code, cache_bind_groups is an UnsafeCell, so look here for any
    // wacky errors.
    // As for the rational behind this: This is the simplest way to do this, wgpu requires I give
    // it &wgpu::BindGroup s, but if I try to write safe code with RefCells, all I can get is a
    // Ref<wgpu::BindGroup>. Data races shouldn't happen unless textures of a bound set are changed
    // in the middle of a draw call (or its recording).
    // TODO: Make this safe ?
    pub fn get_bindgroup(&self, device: &wgpu::Device, set: TextureSet) -> &wgpu::BindGroup {
        // There's no way this'll ever fail... Right ?
        let bindgroups = unsafe { &mut *self.cache_bind_groups.get() };
        if !bindgroups.contains_key(set) {
            let layout = self.layout(device);
            let sampler = self.sampler(device);
            let handles = self
                .sets
                .get(set)
                .expect("Attempting to build bind group for unknown set");
            let views: Vec<_> = handles
                .iter()
                .map(|handle| self.textures.get(*handle).unwrap())
                .collect();

            bindgroups.insert(
                set,
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureViewArray(&views),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                    ],
                    label: Some("TextureSet bind group"),
                }),
            );
        }
        bindgroups.get(set).unwrap()
    }

    pub fn get_index_of_texture(&self, tex: TextureHandle, set: TextureSet) -> Option<usize> {
        self.sets.get(set)?.iter().position(|a| *a == tex)
    }

    pub fn remove_texture(&mut self, tex: TextureHandle) -> Result<wgpu::TextureView> {
        let res = self
            .textures
            .remove(tex)
            .context("Trying to remove unknown texture.")?; // remove wgpu texture
        for set in self.textures_set.remove(tex).unwrap() {
            let index = self
                .sets
                .get(set)
                .unwrap()
                .iter()
                .position(|s| *s == tex)
                .unwrap();
            self.sets.get_mut(set).unwrap().remove(index); // remove texture from set
            self.cache_bind_groups.get_mut().remove(set); // delete cached bind group as it is no longer valid and needs to be recreated
        }
        if let Some(value) = self.texture_value.get(tex) {
            self.single_value_cache.remove(value);
            self.texture_value.remove(tex);
        }
        Ok(res)
    }

    pub fn create_single_value_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        value: SingleValue,
    ) -> wgpu::TextureView {
        let size = wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        };

        let gtex = device.create_texture(&wgpu::TextureDescriptor {
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: value.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("TextureManager texture"),
        });

        let data = match value {
            SingleValue::Color(c) => {
                let c = c.clamp(Vec4::splat(0.0), Vec4::splat(1.0));
                vec![
                    (c.x * 255.0) as u8,
                    (c.y * 255.0) as u8,
                    (c.z * 255.0) as u8,
                    (c.w * 255.0) as u8,
                ]
            }
            SingleValue::Normal(n) => {
                let n = n.normalize() * Vec3::splat(0.5) + Vec3::splat(0.5);
                vec![
                    (n.x * 255.0) as u8,
                    (n.y * 255.0) as u8,
                    (n.z * 255.0) as u8,
                    0u8,
                ]
            }
            SingleValue::Float(f) => bytemuck::bytes_of(&f).to_vec(),
            SingleValue::Factor(f) => vec![(f.clamp(0.0, 1.0) * 255.0) as u8],
        };

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &gtex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(data.len() as u32),
                rows_per_image: std::num::NonZeroU32::new(1),
            },
            size,
        );

        gtex.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub fn create_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: DynamicImage,
    ) -> wgpu::TextureView {
        let img = img.into_rgba8();
        let dim = img.dimensions();
        log::info!("Creating texture: {dim:?}");

        let size = wgpu::Extent3d {
            width: dim.0,
            height: dim.1,
            depth_or_array_layers: 1,
        };

        let gtex = device.create_texture(&wgpu::TextureDescriptor {
            size,
            // TODO: MipMaps
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("TextureManager texture"),
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &gtex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(4 * dim.0),
                rows_per_image: std::num::NonZeroU32::new(dim.1),
            },
            size,
        );

        gtex.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub fn create_depth_texture(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> wgpu::TextureView {
        let size = wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("TextureManager Depth Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    }
}

impl Default for TextureManager {
    fn default() -> Self {
        Self::new()
    }
}
