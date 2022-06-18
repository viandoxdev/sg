use std::path::Path;

use anyhow::Result;
use glam::{Quat, Vec3};

use crate::components::TransformsComponent;

pub fn open<P: AsRef<Path>>(path: P) -> Result<()> {
    let (doc, _buffers, _images) = gltf::import(path)?;
    for scene in doc.scenes() {
        println!("Scene");
        for node in scene.nodes() {
            let name = node.name().unwrap_or("<node>");
            let (translation, rotation, scale) = node.transform().decomposed();
            let mut tsm = TransformsComponent::new();
            tsm.set_translation(Vec3::from(translation));
            tsm.set_rotation(Quat::from_array(rotation));
            tsm.set_scale(Vec3::from(scale));
            println!("  Node {name}");
            println!("  transforms {translation:?} {rotation:?} {scale:?}");
            if let Some(mesh) = node.mesh() {
                let name = mesh.name().unwrap_or("<mesh>");
                println!("    Mesh {name}");
                for primitive in mesh.primitives() {
                    let mode = primitive.mode();
                    println!("      Primitive {mode:?}");
                    for attribute in primitive.attributes() {
                        let sem = attribute.0;
                        let acc = attribute.1;
                        let name = acc.name().unwrap_or("<accessor>");
                        let size = acc.size();
                        let ty = acc.dimensions();
                        let comp = acc.data_type();
                        let len = acc.count();
                        println!("        Attribute {sem:?} ");
                        println!("          Accessor {name} ({size}) {len} {ty:?} of {comp:?}");
                    }
                }
            }
        }
    }
    Ok(())
}
