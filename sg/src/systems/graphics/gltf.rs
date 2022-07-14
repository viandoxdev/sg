use std::{num::NonZeroU32, path::Path};

use anyhow::{Context, Result};
use glam::{Quat, Vec2, Vec3};
use gltf::image::Data as ImageData;
use gltf::image::Format;

use crate::components::{GraphicsComponent, TransformsComponent};

use super::Material;
use super::{
    mesh_manager::{Mesh, Vertex},
    texture_manager::{SingleValue, TextureHandle},
    GraphicContext,
};

struct ChannelIndex {
    red: Option<usize>,
    green: Option<usize>,
    blue: Option<usize>,
    alpha: Option<usize>,
}

impl Default for ChannelIndex {
    fn default() -> Self {
        ChannelIndex {
            red: None,
            green: None,
            blue: None,
            alpha: None,
        }
    }
}

trait FormatExt {
    fn to_wgpu(self, srgb: bool) -> wgpu::TextureFormat;
    fn bytes_per_channel(self) -> usize;
    fn bytes_per_pixel(self) -> usize;
    fn bytes_per_pixel_unaligned(self) -> usize;
    fn count_channels(self) -> usize;
    fn channel_index(self) -> ChannelIndex;
}

impl FormatExt for Format {
    fn to_wgpu(self, srgb: bool) -> wgpu::TextureFormat {
        match self {
            Format::R8G8B8 if srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Format::B8G8R8 if srgb => wgpu::TextureFormat::Bgra8UnormSrgb,
            Format::R8G8B8A8 if srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Format::B8G8R8A8 if srgb => wgpu::TextureFormat::Bgra8UnormSrgb,

            Format::R8 => wgpu::TextureFormat::R8Unorm,
            Format::R16 => wgpu::TextureFormat::R16Unorm,
            Format::R8G8 => wgpu::TextureFormat::Rg8Unorm,
            Format::R16G16 => wgpu::TextureFormat::Rg16Unorm,
            Format::R8G8B8 => wgpu::TextureFormat::Rgba8Unorm,
            Format::B8G8R8 => wgpu::TextureFormat::Bgra8Unorm,
            Format::R8G8B8A8 => wgpu::TextureFormat::Rgba8Unorm,
            Format::B8G8R8A8 => wgpu::TextureFormat::Bgra8Unorm,
            Format::R16G16B16 => wgpu::TextureFormat::Rgba16Unorm,
            Format::R16G16B16A16 => wgpu::TextureFormat::Rgba16Unorm,
        }
    }

    fn bytes_per_pixel_unaligned(self) -> usize {
        match self {
            Format::R8 => 1,
            Format::R16 | Format::R8G8 => 2,
            Format::R8G8B8 | Format::B8G8R8 => 3,
            Format::R16G16 | Format::R8G8B8A8 | Format::B8G8R8A8 => 4,
            Format::R16G16B16 => 6,
            Format::R16G16B16A16 => 8,
        }
    }

    fn bytes_per_pixel(self) -> usize {
        match self.bytes_per_pixel_unaligned() {
            3 => 4,
            6 => 8,
            v => v,
        }
    }

    fn count_channels(self) -> usize {
        match self {
            Format::R8 | Format::R16 => 1,
            Format::R8G8 | Format::R16G16 => 2,
            Format::R8G8B8
            | Format::B8G8R8
            | Format::R8G8B8A8
            | Format::B8G8R8A8
            | Format::R16G16B16
            | Format::R16G16B16A16 => 4,
        }
    }

    fn bytes_per_channel(self) -> usize {
        self.bytes_per_pixel() / self.count_channels()
    }

    fn channel_index(self) -> ChannelIndex {
        macro_rules! ci {
            ($r:expr) => {
                ChannelIndex {
                    red: Some($r),
                    ..Default::default()
                }
            };
            ($r:expr, $g:expr) => {
                ChannelIndex {
                    red: Some($r),
                    green: Some($g),
                    ..Default::default()
                }
            };
            ($r:expr, $g:expr, $b:expr) => {
                ChannelIndex {
                    red: Some($r),
                    green: Some($g),
                    blue: Some($b),
                    ..Default::default()
                }
            };
            ($r:expr, $g:expr, $b:expr, $a:expr) => {
                ChannelIndex {
                    red: Some($r),
                    green: Some($g),
                    blue: Some($b),
                    alpha: Some($a),
                }
            };
        }
        match self {
            Format::R8 => ci!(0),
            Format::R16 => ci!(0),
            Format::R8G8 => ci!(0, 1),
            Format::R16G16 => ci!(0, 1),
            Format::R8G8B8 => ci!(0, 1, 2),
            Format::B8G8R8 => ci!(2, 1, 0),
            Format::R8G8B8A8 => ci!(0, 1, 2, 3),
            Format::B8G8R8A8 => ci!(2, 1, 0, 3),
            Format::R16G16B16 => ci!(0, 1, 2),
            Format::R16G16B16A16 => ci!(0, 1, 2, 3),
        }
    }
}

fn load_image(gfx: &mut GraphicContext, image: &mut ImageData, srgb: bool) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: image.width,
        height: image.height,
        depth_or_array_layers: 1,
    };

    let data = &mut image.pixels;

    let format = image.format.to_wgpu(srgb);
    let bytes_per_pixel = image.format.bytes_per_pixel();
    match image.format {
        // these formats require adding in an alpha channel
        Format::R8G8B8 => {
            for i in 0..(data.len() / 3) {
                data.insert(i * 4 + 3, 255);
            }
        }
        Format::B8G8R8 => {
            for i in 0..(data.len() / 3) {
                data.insert(i * 4 + 3, 255);
            }
        }
        Format::R16G16B16 => {
            for i in 0..(data.len() / 6) {
                data.insert(i * 8 + 6, 255);
                data.insert(i * 8 + 6, 255);
            }
        }
        _ => {}
    }
    let bytes_per_row = Some(NonZeroU32::new(bytes_per_pixel as u32 * image.width).unwrap());

    let tex = gfx.device.create_texture(&wgpu::TextureDescriptor {
        format,
        size,
        label: Some("GLTF Texture"),
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        dimension: wgpu::TextureDimension::D2,
        sample_count: 1,
        mip_level_count: 1,
    });

    gfx.queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row,
            rows_per_image: std::num::NonZeroU32::new(image.height),
        },
        size,
    );

    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

pub fn open<P: AsRef<Path>>(path: P, gfx: &mut GraphicContext) -> Result<Vec<(GraphicsComponent, TransformsComponent)>> {
    let (doc, buffers, mut doc_images) = gltf::import(path)?;

    let mut mesh_handles = vec![vec![]; doc.meshes().count()];
    let mut materials: Vec<Option<Material>> = vec![None; doc.materials().count() + 1];
    let mut images: Vec<Vec<TextureHandle>> = vec![vec![]; doc.images().count()];
    let mut entities: Vec<(GraphicsComponent, TransformsComponent)> = Vec::new();

    let default_material_index = materials.len() - 1;

    for mesh in doc.meshes() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            let mut positions = reader
                .read_positions()
                .context("Couldn't read vertex positions")?;
            let mut normals = reader.read_normals().context("Couldn't read normals")?;
            let mut tex_coords = reader
                .read_tex_coords(0)
                .context("Couldn't read texture coordinates")?
                .into_f32();
            let mut indices = reader
                .read_indices()
                .context("Couldn't read indices")?
                .into_u32();

            let mut m_indices = Vec::new();
            let mut m_vertices = Vec::new();
            while let Some(i1) = indices.next() {
                let (i2, i3) = (
                    indices
                        .next()
                        .context("Indices count isn't a multiple of 3")?,
                    indices
                        .next()
                        .context("Indices count isn't a multiple of 3")?,
                );
                m_indices.push([i1, i3, i2]); // swap to invert winding
            }

            while let Some(position) = positions.next() {
                let position = Vec3::from(position);
                let normal = Vec3::from(normals.next().context("No normal given for vertex")?);
                let tex_coords = Vec2::from(
                    tex_coords
                        .next()
                        .context("No texture coordinate given for vertex")?,
                );
                let tangent = Vec3::ONE;

                m_vertices.push(Vertex {
                    position,
                    normal,
                    tex_coords,
                    tangent,
                });
            }

            let mut m_mesh = Mesh {
                indices: m_indices,
                vertices: m_vertices,
            };
            m_mesh.recompute_tangents();
            mesh_handles[mesh.index()].push(gfx.mesh_manager.add(&gfx.device, &m_mesh));
        }
    }

    for material in doc.materials() {
        let mut load = |gfx: &mut GraphicContext, tex: gltf::Texture, srgb| {
            // TODO: sampler
            let index = tex.source().index();

            if let Some(handle) = images[index].get(0) {
                return *handle;
            }

            let view = load_image(gfx, &mut doc_images[index], srgb);
            let handle = gfx.texture_manager.add_texture(view);
            images[index] = vec![handle];
            handle
        };

        let pbrmr = material.pbr_metallic_roughness();
        let albedo = pbrmr
            .base_color_texture()
            .map(|tex| load(gfx, tex.texture(), true))
            .unwrap_or_else(|| {
                gfx.texture_manager.get_or_add_single_value_texture(
                    &gfx.device,
                    &gfx.queue,
                    SingleValue::Color(pbrmr.base_color_factor().into()),
                )
            });
        let normal_map = material
            .normal_texture()
            .map(|tex| load(gfx, tex.texture(), false));
        let ao = material
            .occlusion_texture()
            .map(|tex| load(gfx, tex.texture(), false));
        let metallic;
        let roughness;
        if let Some(tex) = pbrmr.metallic_roughness_texture() {
            let tex = tex.texture();
            // TODO: sampler
            let index = tex.source().index();

            if let (Some(met), Some(rou)) = (images[index].get(0), images[index].get(1)) {
                metallic = *met;
                roughness = *rou;
            } else {
                let img_mr = &mut doc_images[index];
                let bpc = img_mr.format.bytes_per_channel();
                let bpp = img_mr.format.bytes_per_pixel_unaligned();
                let ci = img_mr.format.channel_index();

                let format = match bpc {
                    1 => Format::R8,
                    2 => Format::R16,
                    _ => unreachable!(),
                };
                let mut img_met = ImageData {
                    format,
                    width: img_mr.width,
                    height: img_mr.height,
                    pixels: Vec::with_capacity((img_mr.width * img_mr.height) as usize * bpc),
                };
                let mut img_rou = ImageData {
                    format,
                    width: img_mr.width,
                    height: img_mr.height,
                    pixels: Vec::with_capacity((img_mr.width * img_mr.height) as usize * bpc),
                };
                let data = &img_mr.pixels;
                for i in (0..data.len()).step_by(bpp) {
                    let met_index = i + bpc * ci.blue.context("No blue channel in MR texture")?;
                    let rou_index = i + bpc * ci.green.context("No green channel in MR texture")?;
                    let met_bytes = &data[met_index..(met_index + bpc)];
                    let rou_bytes = &data[rou_index..(rou_index + bpc)];
                    img_met.pixels.extend_from_slice(met_bytes);
                    img_rou.pixels.extend_from_slice(rou_bytes);
                }
                let met = load_image(gfx, &mut img_met, false);
                let rou = load_image(gfx, &mut img_rou, false);
                metallic = gfx.texture_manager.add_texture(met);
                roughness = gfx.texture_manager.add_texture(rou);
            }
        } else {
            metallic = gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Factor(pbrmr.metallic_factor()),
            );
            roughness = gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Factor(pbrmr.roughness_factor()),
            );
        }
        let index = material.index().unwrap_or(default_material_index);
        materials[index].replace(
            Material::new(albedo, normal_map, metallic, roughness, ao, gfx)
                .context("Error on material creation")?,
        );
    }

    for scene in doc.scenes() {
        println!("Scene");
        for node in scene.nodes() {
            let (translation, rotation, scale) = node.transform().decomposed();
            let mut tsm = TransformsComponent::new();
            tsm.set_translation(Vec3::from(translation));
            tsm.set_rotation(Quat::from_array(rotation));
            tsm.set_scale(Vec3::from(scale));
            if let Some(mesh) = node.mesh() {
                for (index, primitive) in mesh.primitives().enumerate() {
                    let material_index = primitive
                        .material()
                        .index()
                        .unwrap_or(default_material_index);
                    let material = materials[material_index].context("No such material")?;
                    let mesh = mesh_handles[mesh.index()][index];
                    let gfc = GraphicsComponent { material, mesh };

                    entities.push((gfc, tsm.clone()));
                }
            }
        }
    }
    Ok(entities)
}
