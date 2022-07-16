use std::{num::NonZeroU32, path::Path};

use anyhow::{Context, Result};
use glam::{Quat, Vec2, Vec3};
use gltf::image::Data as ImageData;
use gltf::image::Format;
use gltf::Node;

use crate::components::{GraphicsComponent, TransformsComponent};
use crate::systems::graphics::mesh_manager::MeshHandle;

use super::Material;
use super::{
    mesh_manager::{Mesh, Vertex},
    texture_manager::{SingleValue, TextureHandle},
    GraphicContext,
};

#[derive(Default)]
struct ChannelIndex {
    red: Option<usize>,
    green: Option<usize>,
    blue: Option<usize>,
    alpha: Option<usize>,
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

    log::trace!("image loading - cloning");
    let mut data = image.pixels.clone();

    let mut format = image.format.to_wgpu(srgb);
    let mut bytes_per_pixel = image.format.bytes_per_pixel();
    match image.format {
        // these formats require adding in an alpha channel
        Format::R8G8B8 => {
            log::trace!("image loading - format conversion (RGB8 -> RGBA8)");
            let mut new = Vec::with_capacity(data.len() / 3 * 4);
            for rgb in data.chunks(3) {
                new.extend_from_slice(rgb);
                new.push(255);
            }
            data = new;
            log::trace!("image loading - format converted");
        }
        Format::B8G8R8 => {
            log::trace!("image loading - format conversion (BGR8 -> BGRA8)");
            let mut new = Vec::with_capacity(data.len() / 3 * 4);
            for rgb in data.chunks(3) {
                new.extend_from_slice(rgb);
                new.push(255);
            }
            data = new;
            log::trace!("image loading - format converted");
        }
        Format::R16G16B16 => {
            log::trace!("image loading - format conversion (BGR16 -> BGRA16)");
            let mut new = Vec::with_capacity(data.len() / 6 * 8);
            for rgb in data.chunks(6) {
                new.extend_from_slice(rgb);
                new.extend_from_slice(&[255; 2]);
            }
            data = new;
            log::trace!("image loading - format converted");
        }
        // Theses single channel format are converted to grayscale rgba
        Format::R8 => {
            log::trace!("image loading - format conversion (R8 -> RGBA8)");
            let mut new = Vec::with_capacity(data.len() * 4);
            for r in data {
                new.extend_from_slice(&[r, r, r, 255]);
            }
            data = new;
            format = wgpu::TextureFormat::Rgba8Unorm;
            bytes_per_pixel = 4;
            log::trace!("image loading - format converted");
        }
        Format::R16 => {
            log::trace!("image loading - format conversion (RG16 -> RGBA16)");
            let mut new = Vec::with_capacity(data.len() * 4);
            for r in data.chunks(2) {
                new.extend_from_slice(r);
                new.extend_from_slice(r);
                new.extend_from_slice(r);
                new.extend_from_slice(&[255, 255]);
            }
            data = new;
            format = wgpu::TextureFormat::Rgba16Unorm;
            bytes_per_pixel = 8;
            log::trace!("image loading - format converted");
        }
        // Theses two channel formats are converted to alpha luminesance rgba
        Format::R8G8 => {
            log::trace!("image loading - format conversion (RG8 -> RGBA8)");
            let mut new = Vec::with_capacity(data.len() * 2);
            for chunk in data.chunks(2) {
                let l = chunk[0];
                let a = chunk[1];
                new.extend_from_slice(&[l, l, l, a]);
            }
            data = new;
            format = wgpu::TextureFormat::Rgba8Unorm;
            bytes_per_pixel = 4;
            log::trace!("image loading - format converted");
        }
        Format::R16G16 => {
            log::trace!("image loading - format conversion (R16 -> RGBA16)");
            let mut new = Vec::with_capacity(data.len() * 2);
            for chunk in data.chunks(4) {
                if let [l1, l2, a1, a2] = *chunk {
                    new.extend_from_slice(&[l1, l2, l1, l2, l1, l2, a1, a2]);
                } else {
                    panic!("No bytes ?");
                }
            }
            data = new;
            format = wgpu::TextureFormat::Rgba16Unorm;
            bytes_per_pixel = 8;
            log::trace!("image loading - format converted");
        }
        _ => {}
    }
    let bytes_per_row = Some(NonZeroU32::new(bytes_per_pixel as u32 * image.width).unwrap());
    log::trace!("image loading - gpu texture creation");
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

    log::trace!("image loading - gpu texture created");
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

pub fn open<P: AsRef<Path>>(
    path: P,
    gfx: &mut GraphicContext,
) -> Result<Vec<(GraphicsComponent, TransformsComponent)>> {
    log::trace!("Importing gltf...");
    let (doc, buffers, mut doc_images) = gltf::import(path)?;
    log::trace!("done");
    let mut mesh_handles = vec![vec![]; doc.meshes().count()];
    let mut materials: Vec<Option<Material>> = vec![None; doc.materials().count() + 1];
    let mut images: Vec<Vec<TextureHandle>> = vec![vec![]; doc.images().count()];
    let mut entities: Vec<(GraphicsComponent, TransformsComponent)> = Vec::new();

    let default_material_index = materials.len() - 1;
    log::trace!("Processing gltf 1/3 - meshes");
    for mesh in doc.meshes() {
        for primitive in mesh.primitives() {
            log::trace!("  mesh: getting data ({:?})", mesh.name());
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            let positions = reader
                .read_positions()
                .context("Couldn't read vertex positions")?;
            let mut normals = reader.read_normals().context("Couldn't read normals")?;
            let mut tex_coords = reader
                .read_tex_coords(0)
                .or_else(|| reader.read_tex_coords(1))
                .map(|t| t.into_f32());
            let mut default_tex_coords = std::iter::repeat([0.0; 2]);
            let tex_coords: &mut dyn Iterator<Item = [f32; 2]> = tex_coords
                .as_mut()
                .map(|t| t as &mut dyn Iterator<Item = [f32; 2]>)
                .unwrap_or_else(|| {
                    log::warn!("No tex coords for mesh");
                    &mut default_tex_coords
                });
            let mut indices = reader
                .read_indices()
                .context("Couldn't read indices")?
                .into_u32();

            log::trace!("    - processing indices");

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

            log::trace!("    - processing vertices");

            for position in positions {
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
            log::trace!("    - processing tangents");
            m_mesh.recompute_tangents();
            mesh_handles[mesh.index()].push(gfx.mesh_manager.add(&gfx.device, &m_mesh));
        }
    }
    log::trace!("Processing gltf 2/3 - materials");
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

        log::trace!("  material: loading data");

        let pbrmr = material.pbr_metallic_roughness();
        log::trace!("    - albedo loading");
        let albedo = pbrmr
            .base_color_texture()
            .map(|tex| load(gfx, tex.texture(), true))
            .unwrap_or_else(|| {
                log::trace!("    - albedo caching");
                gfx.texture_manager.get_or_add_single_value_texture(
                    &gfx.device,
                    &gfx.queue,
                    SingleValue::Color(pbrmr.base_color_factor().into()),
                )
            });
        log::trace!("    - normals");
        let normal_map = material
            .normal_texture()
            .map(|tex| load(gfx, tex.texture(), false));
        log::trace!("    - ao");
        let ao = material
            .occlusion_texture()
            .map(|tex| load(gfx, tex.texture(), false));
        let metallic;
        let roughness;
        log::trace!("    - processing MR");
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
    log::trace!("Processing gltf 3/3 - scenes");

    fn process_node(
        node: Node,
        parent_tsm: &TransformsComponent,
        default_material_index: usize,
        materials: &Vec<Option<Material>>,
        mesh_handles: &Vec<Vec<MeshHandle>>,
        entities: &mut Vec<(GraphicsComponent, TransformsComponent)>,
    ) -> Result<()> {
        log::trace!("  scene: getting node transforms");
        let (translation, rotation, scale) = node.transform().decomposed();
        let mut tsm = TransformsComponent::new();
        tsm.set_translation(Vec3::from(translation));
        tsm.set_rotation(Quat::from_array(rotation));
        tsm.set_scale(Vec3::from(scale));
        tsm.apply(parent_tsm);
        if let Some(mesh) = node.mesh() {
            log::trace!("    - has mesh, making entities");
            for (index, primitive) in mesh.primitives().enumerate() {
                let material_index = primitive
                    .material()
                    .index()
                    .unwrap_or(default_material_index);
                let material = materials[material_index].context("No such material")?;
                let mesh = mesh_handles[mesh.index()][index];
                let gfc = GraphicsComponent { material, mesh };
                log::trace!("      - adding entity");
                entities.push((gfc, tsm.clone()));
            }
        } else {
            log::trace!("    - no mesh found");
        }
        log::trace!("    - iterating over children");
        for node in node.children() {
            process_node(
                node,
                &tsm,
                default_material_index,
                materials,
                mesh_handles,
                entities,
            )?;
        }
        Ok(())
    }

    for scene in doc.scenes() {
        for node in scene.nodes() {
            process_node(
                node,
                &TransformsComponent::default(),
                default_material_index,
                &materials,
                &mesh_handles,
                &mut entities,
            )?;
        }
    }
    log::trace!("Processing gltf - done");
    Ok(entities)
}
