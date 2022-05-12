use std::{cell::UnsafeCell, lazy::OnceCell};

use anyhow::{Result, anyhow};
use image::DynamicImage;
use slotmap::{SlotMap, SecondaryMap};

slotmap::new_key_type! {
    pub struct TextureHandle;
    pub struct TextureSet;
}

// Late night coding here, so RefCell it'll be (for now, or forever)

pub struct TextureManager {
    textures: SlotMap<TextureHandle, wgpu::TextureView>,
    /// All sets existing in the TextureManager (mapped to their textures)
    sets: SlotMap<TextureSet, Vec<TextureHandle>>,
    /// The set mapped to each texture
    textures_set: SecondaryMap<TextureHandle, TextureSet>,
    /// currently cached set bind_groups, all groups have to be valid (i.e. contain all textures
    /// assigned to the set)
    cache_bind_groups: UnsafeCell<SecondaryMap<TextureSet, wgpu::BindGroup>>,
    /// Cache for bindgroup layout
    bind_group_layout: OnceCell<wgpu::BindGroupLayout>,
    /// Cache for sampler
    sampler: OnceCell<wgpu::Sampler>
}

impl TextureManager {
    pub const TEXTURE_SET_MAX: u32 = 16;

    pub fn new() -> Self {
        Self {
            sets: SlotMap::with_key(),
            textures_set: SecondaryMap::new(),
            textures: SlotMap::with_key(),
            cache_bind_groups: UnsafeCell::new(SecondaryMap::new()),
            bind_group_layout: OnceCell::new(),
            sampler: OnceCell::new(),
        }
    }

    /// Create a new set
    pub fn add_set(&mut self) -> TextureSet {
        self.sets.insert(vec![])
    }

    /// Add a texture to the TextureManager and assign it to a set
    pub fn add_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, img: DynamicImage, set: TextureSet) -> Result<TextureHandle> {
        let tex = self.create_texture(device, queue, img);
        let handle = self.textures.insert(tex);
        self.sets.get_mut(set).ok_or_else(|| anyhow!("Trying to add texture to unknown set"))?.push(handle);
        self.textures_set.insert(handle, set);
        // delete bind group as it is no longer valid
        self.cache_bind_groups.get_mut().remove(set);
        Ok(handle)
    }

    pub fn layout(&self, device: &wgpu::Device) -> &wgpu::BindGroupLayout {
        self.bind_group_layout.get_or_init(|| device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: std::num::NonZeroU32::new(Self::TEXTURE_SET_MAX)
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None
                    },
                ],
                label: Some("TextureSet bind group layout"),
            }
        ))
    }

    pub fn sampler(&self, device: &wgpu::Device) -> &wgpu::Sampler {
        self.sampler.get_or_init(|| device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                label: Some("TextureSet sampler"),
                ..Default::default()
            }
        ))
    }

    // /!\ This contains unsafe code, cache_bind_groups is an UnsafeCell, so look here for any
    // wacky errors.
    // As for the rational behind this: This is the simplest way to do this, wgpu requires I give
    // it &wgpu::BindGroup s, but if I try to write safe code with RefCells, all I can get is a
    // Ref<wgpu::BindGroup>. Data races shouldn't happen unless textures of a bound set are changed
    // in the middle of a draw call (or its recording).
    // TODO: Make this safe ?
    fn bindgroup(&self, device: &wgpu::Device, set: TextureSet) -> &wgpu::BindGroup {
        // There's no way this'll ever fail... Right ?
        let bindgroups = unsafe { &mut *self.cache_bind_groups.get() };
        if !bindgroups.contains_key(set) {
            let layout = self.layout(device);
            let sampler = self.sampler(device);
            let handles = self.sets.get(set).expect("Attempting to build bind group for unknown set");
            let views: Vec<_> = handles.iter().map(|handle| self.textures.get(*handle).unwrap()).collect();

            bindgroups.insert(set, device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureViewArray(&views)
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler)
                    }
                ],
                label: Some("TextureSet bind group")
            }));
        }
        bindgroups.get(set).unwrap()
    }

    pub fn get_bindgroup(&self, device: &wgpu::Device, tex: TextureHandle) -> (&wgpu::BindGroup, u32) {
        let set = *self.textures_set.get(tex).expect("Trying to bind bindgroup of unknown texture");
        let index = self.sets.get(set).unwrap().iter().position(|handle| *handle == tex).unwrap();
        let bind_group = self.bindgroup(device, set);
        (bind_group, index as u32)
    }

    pub fn create_texture(&self, device: &wgpu::Device, queue: &wgpu::Queue, img: DynamicImage) -> wgpu::TextureView {
        let img = img.into_rgba8();
        let dim = img.dimensions();
        log::info!("Creating texture: {dim:?}");

        let size = wgpu::Extent3d {
                width: dim.0,
                height: dim.1,
                depth_or_array_layers: 1
            };

        let gtex = device.create_texture(&wgpu::TextureDescriptor {
            size,
            // TODO: MipMaps
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some(&format!("TextureManager texture"))
        });

        queue.write_texture(wgpu::ImageCopyTexture {
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
            size
        );

        gtex.create_view(&wgpu::TextureViewDescriptor::default())
    }
}

