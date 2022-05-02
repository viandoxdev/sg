use anyhow::{Result, anyhow};
use image::{RgbaImage, DynamicImage};
use slotmap::{SlotMap, SecondaryMap};

slotmap::new_key_type! {
    pub struct TextureHandle;
    pub struct TextureSet;
}

pub struct TextureManager {
    /// All images the TextureManager owns
    images: SlotMap<TextureHandle, RgbaImage>,
    /// All sets existing in the TextureManager (mapped to their textures)
    sets: SlotMap<TextureSet, Vec<TextureHandle>>,
    /// The set mapped to each texture
    images_set: SecondaryMap<TextureHandle, TextureSet>,
    /// currently cached set bind_groups, all groups have to be valid (i.e. contain all textures
    /// assigned to the set)
    bind_groups: SecondaryMap<TextureSet, wgpu::BindGroup>,
    /// Currently loaded (on the gpu) textures.
    textures: SecondaryMap<TextureHandle, wgpu::TextureView>,
    /// Vector of sets which textures are currently loaded, sorted by last usage
    loaded_sets: Vec<TextureSet>,
    loaded_bytes: usize,
    /// Cache for bindgroup layout
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    /// Cache for sampler
    sampler: Option<wgpu::Sampler>
}

impl TextureManager {
    pub const TEXTURE_SET_MAX: u32 = 256;
    pub const MAX_LOADED_BYTES: usize = 4294967296; // 4 GB

    pub fn new() -> Self {
        Self {
            images: SlotMap::with_key(),
            sets: SlotMap::with_key(),
            images_set: SecondaryMap::new(),
            bind_groups: SecondaryMap::new(),
            loaded_sets: Vec::new(),
            textures: SecondaryMap::new(),
            loaded_bytes: 0,
            bind_group_layout: None,
            sampler: None,
        }
    }

    /// Create a new set
    pub fn add_set(&mut self) -> TextureSet {
        self.sets.insert(vec![])
    }

    /// Add a texture to the TextureManager and assign it to a set
    pub fn add_texture(&mut self, img: DynamicImage, set: TextureSet) -> Result<TextureHandle> {
        let handle = self.images.insert(img.into_rgba8());
        self.sets.get_mut(set).ok_or_else(|| anyhow!("Trying to add texture to unknown set"))?.push(handle);
        self.images_set.insert(handle, set);
        // delete bind group as it is no longer valid
        self.bind_groups.remove(set);
        Ok(handle)
    }

    pub fn ensure_layout(&mut self, device: &wgpu::Device) {
        if self.bind_group_layout.is_none() {
            self.bind_group_layout = Some(device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            }));
        }
    }

    pub fn ensure_sampler(&mut self, device: &wgpu::Device) {
        if self.sampler.is_none() {
            self.sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                label: Some("TextureSet sampler"),
                ..Default::default()
            }));
        }
    }

    /// Get the size, in bytes of an image
    fn byte_count(&self, tex: TextureHandle) -> usize {
        self.images.get(tex).map(|img| img.len()).unwrap_or(0)
    }

    /// Ensures that the textures of set are loaded
    fn load_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, set: TextureSet) {
        if let Some(index) = self.loaded_sets.iter().position(|s| *s == set) {
            // push ourselves back at the front
            let s = self.loaded_sets.remove(index);
            self.loaded_sets.insert(0, s);
            return;
        }

        let handles = self.sets.get(set).expect("Trying to load textures of unknown set");

        let mut size = 0; // byte count of the current set
        for handle in handles {
            size += self.byte_count(*handle);
        }

        assert!(size <= Self::MAX_LOADED_BYTES, "Trying to load set exceeding MAX_LOADED_BYTES in size");

        if self.loaded_bytes + size > Self::MAX_LOADED_BYTES {
            let unload = self.loaded_sets.remove(self.loaded_sets.len() - 1);
            let handles = self.sets.get(unload).unwrap();
            for handle in handles {
                // remove the TextureViews, this calls their destructor and (should) end up freeing
                // the textures memory as there are no longer any reference pointing to them.
                self.textures.remove(*handle);
                self.loaded_bytes -= self.byte_count(*handle);
            }
        }

        for handle in handles {
            let tex = self.create_texture(device, queue, *handle);
            self.textures.insert(*handle, tex.create_view(&wgpu::TextureViewDescriptor::default()));
        }
    }

    fn ensure_bindgroup(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, set: TextureSet) {
        if !self.bind_groups.contains_key(set) {
            //self.load_textures(device, queue, set);
            //self.ensure_layout(device);
            //self.ensure_sampler(device);

            //let layout = self.bind_group_layout.as_ref().unwrap();
            //let sampler = self.sampler.as_ref().unwrap();
            //let handles = self.sets.get(set).expect("Building bind group for unknown set");
            //let views: Vec<_> = handles.iter().map(|handle| self.textures.get(*handle).unwrap()).collect();

            //self.bind_groups.insert(set, device.create_bind_group(&wgpu::BindGroupDescriptor {
            //    layout,
            //    entries: &[
            //        wgpu::BindGroupEntry {
            //            binding: 1,
            //            resource: wgpu::BindingResource::TextureViewArray(&views)
            //        },
            //        wgpu::BindGroupEntry {
            //            binding: 1,
            //            resource: wgpu::BindingResource::Sampler(sampler)
            //        }
            //    ],
            //    label: Some("TextureSet bind group")
            //}));
        }
    }

    pub fn get_bindgroup(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tex: TextureHandle) -> (&wgpu::BindGroup, usize) {
        let set = *self.images_set.get(tex).expect("Trying to bind bindgroup of unknown texture");
        //self.ensure_bindgroup(device, queue, set);
        let index = self.sets.get(set).unwrap().iter().position(|handle| *handle == tex).unwrap();
        let bind_group = self.bind_groups.get(set).unwrap();
        (bind_group, index)
    }

    pub fn create_texture(&self, device: &wgpu::Device, queue: &wgpu::Queue, tex: TextureHandle) -> wgpu::Texture {
        let img = self.images.get(tex).expect("creating texture but no image data");
        let dim = img.dimensions();

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
            label: Some(&format!("tex{tex:?}"))
        });

        queue.write_texture(gtex.as_image_copy(), &img, wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: std::num::NonZeroU32::new(4 * dim.0),
            rows_per_image: std::num::NonZeroU32::new(dim.1),
        }, size);

        gtex
    }
}
