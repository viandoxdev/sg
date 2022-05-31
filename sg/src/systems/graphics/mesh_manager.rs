use anyhow::{anyhow, Result};
use slotmap::SlotMap;
use wgpu::util::DeviceExt;

use super::Vertex;

pub struct Mesh {
    pub verticies: wgpu::Buffer,
    pub indicies: wgpu::Buffer,
    pub num_indicies: u32,
}

slotmap::new_key_type! {
    pub struct MeshHandle;
}

impl Mesh {
    fn build_buffer(
        device: &wgpu::Device,
        verticies: &[Vertex],
        indicies: &[[u16; 3]],
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(verticies),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(indicies),
                usage: wgpu::BufferUsages::INDEX,
            }),
        )
    }
    pub fn new(device: &wgpu::Device, verticies: &[Vertex], indicies: &[[u16; 3]]) -> Self {
        let num_indicies = indicies.len() as u32 * 3;
        let (verticies, indicies) = Self::build_buffer(device, verticies, indicies);
        Self {
            verticies,
            indicies,
            num_indicies,
        }
    }
    pub fn update(&mut self, device: &wgpu::Device, verticies: &[Vertex], indicies: &[[u16; 3]]) {
        (self.verticies, self.indicies) = Self::build_buffer(device, verticies, indicies);
        self.num_indicies = indicies.len() as u32 * 3;
    }
}

pub struct MeshManager {
    meshes: SlotMap<MeshHandle, Mesh>,
}

impl MeshManager {
    pub fn new() -> Self {
        Self {
            meshes: SlotMap::with_key(),
        }
    }

    pub fn add(
        &mut self,
        device: &wgpu::Device,
        verticies: &[Vertex],
        indicies: &[[u16; 3]],
    ) -> MeshHandle {
        self.meshes.insert(Mesh::new(device, verticies, indicies))
    }

    pub fn remove(&mut self, handle: MeshHandle) -> Option<Mesh> {
        self.meshes.remove(handle)
    }

    pub fn update(
        &mut self,
        handle: MeshHandle,
        device: &wgpu::Device,
        verticies: &[Vertex],
        indicies: &[[u16; 3]],
    ) -> Result<()> {
        self.meshes
            .get_mut(handle)
            .ok_or_else(|| anyhow!("Handle doesn't point to any mesh"))?
            .update(device, verticies, indicies);
        Ok(())
    }

    pub fn get(&self, handle: MeshHandle) -> Option<&Mesh> {
        self.meshes.get(handle)
    }
}
