use anyhow::{anyhow, Result};
use glam::Vec2;
use glam::Vec3;
use slotmap::SlotMap;
use wgpu::util::DeviceExt;

use super::Vertex;

#[derive(Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<[u16; 3]>,
}

/// A mesh living on the gpu
pub struct BufferedMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub num_indices: u32,
}

slotmap::new_key_type! {
    pub struct MeshHandle;
}

impl Mesh {
    fn buffered(&self, device: &wgpu::Device) -> BufferedMesh {
        let num_indices = self.indices.len() as u32 * 3;
        BufferedMesh {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&self.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(&self.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            num_indices,
        }
    }
    pub fn recompute_normals(&mut self) {
        let mut normals = vec![Vec3::ZERO; self.vertices.len()];
        for tri in &self.indices {
            let p1 = self.vertices[tri[0] as usize].position;
            let p2 = self.vertices[tri[1] as usize].position;
            let p3 = self.vertices[tri[2] as usize].position;
            let normal = (p3 - p1).cross(p2 - p1).normalize();
            normals[tri[0] as usize] += normal;
            normals[tri[1] as usize] += normal;
            normals[tri[2] as usize] += normal;
        }
        // average out the normals
        for (i, acc) in normals.into_iter().enumerate() {
            self.vertices[i].normal = acc.normalize_or_zero();
        }
    }
    /// Merge the vertices of a mesh, giving it a smooth look (from normal interpolation)
    pub fn merge_vertices(self) -> Self {
        todo!()
    }
    /// Duplicate the vertices of a meshn giving it a flat look
    pub fn duplicate_vertices(self) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut i = 0u16;
        for tri in self.indices {
            indices.push([i, i + 1, i + 2]);
            vertices.extend_from_slice(&[
                self.vertices[tri[0] as usize],
                self.vertices[tri[1] as usize],
                self.vertices[tri[2] as usize],
            ]);
            i += 3;
        }
        let mut res = Self { vertices, indices };
        res.recompute_normals();
        res.recompute_tangents();
        res
    }
    pub fn recompute_tangents(&mut self) {
        let mut tangents = vec![Vec3::ZERO; self.vertices.len()];
        for tri in &self.indices {
            let v1 = self.vertices[tri[0] as usize];
            let v2 = self.vertices[tri[1] as usize];
            let v3 = self.vertices[tri[2] as usize];
            let (p1, p2, p3) = (v1.position, v2.position, v3.position);
            let (uv1, uv2, uv3) = (v1.tex_coords, v2.tex_coords, v3.tex_coords);
            let e1 = p2 - p1;
            let e2 = p3 - p1;
            let duv1 = uv2 - uv1;
            let duv2 = uv3 - uv1;
            let fi = duv1.x * duv2.y - duv2.x * duv1.y;
            let f = if fi == 0.0 { 1.0 } else { 1.0 / fi };
            let t = Vec3::new(
                f * (duv2.y * e1.x - duv1.y * e2.x),
                f * (duv2.y * e1.y - duv1.y * e2.y),
                f * (duv2.y * e1.z - duv1.y * e2.z),
            );
            tangents[tri[0] as usize] += t;
            tangents[tri[1] as usize] += t;
            tangents[tri[2] as usize] += t;
        }
        // average out the tangents
        for (i, acc) in tangents.into_iter().enumerate() {
            let acc = acc.normalize_or_zero();
            if acc.is_normalized() {
                self.vertices[i].tangent = acc;
            } else {
                self.vertices[i].tangent = Vec3::new(1.0, 0.0, 0.0) // should not be full of 0
            }
        }
    }
}
pub trait Primitives {
    fn new_icosphere(detail: u32) -> Self;
    fn new_cubic_sphere(detail: u32) -> Self;
    fn new_cube() -> Self;
}

impl Primitives for Mesh {
    fn new_icosphere(detail: u32) -> Self {
        const O: f32 = 0.0000000;
        const H: f32 = 0.5257311;
        const L: f32 = 0.8506508;
        macro_rules! v {
            ($a:expr, $b:expr, $c:expr) => {
                Vertex {
                    position: Vec3::new($a, $b, $c),
                    normal: Vec3::new($a, $b, $c),
                    tex_coords: Vec2::ZERO,
                    tangent: Vec3::ZERO,
                }
            };
            ($v:ident) => {
                Vertex {
                    position: $v,
                    normal: $v,
                    tex_coords: Vec2::ZERO,
                    tangent: Vec3::ONE,
                }
            };
        }
        let mut vertices = vec![
            v![-H, L, O],
            v![O, H, L],
            v![H, L, O],
            v![O, H, -L],
            v![L, O, H],
            v![L, O, -H],
            v![-H, -L, O],
            v![H, -L, O],
            v![O, -H, L],
            v![O, -H, -L],
            v![-L, O, H],
            v![-L, O, -H],
        ];
        let mut indices: Vec<[u16; 3]> = vec![
            [0, 1, 2],
            [2, 3, 0],
            [2, 1, 4],
            [2, 5, 3],
            [5, 2, 4],
            [6, 7, 8],
            [6, 9, 7],
            [6, 8, 10],
            [6, 11, 9],
            [10, 11, 6],
            [0, 10, 1],
            [0, 3, 11],
            [0, 11, 10],
            [7, 4, 8],
            [7, 9, 5],
            [7, 5, 4],
            [9, 11, 3],
            [9, 3, 5],
            [1, 10, 8],
            [1, 8, 4],
        ];
        for _ in 0..detail {
            for _ in 0..indices.len() {
                let tri = indices.remove(0);
                let (i1, i2, i3) = (tri[0], tri[1], tri[2]);
                let p1 = vertices[i1 as usize].position;
                let p2 = vertices[i2 as usize].position;
                let p3 = vertices[i3 as usize].position;
                let (i4, i5, i6) = (
                    vertices.len() as u16,
                    vertices.len() as u16 + 1,
                    vertices.len() as u16 + 2,
                );
                let p4 = p1.lerp(p2, 0.5).normalize(); // halfway point p1 -> p2
                let p5 = p2.lerp(p3, 0.5).normalize(); // normalize to keep the vertices on the unit sphere
                let p6 = p3.lerp(p1, 0.5).normalize();
                //       p1
                //    1. /\
                //   p6 /__\ p4
                //  2. /\3./\ 4.
                // p3 /__\/__\ p2
                //       p5
                indices.extend_from_slice(&[
                    [i1, i4, i6], // 1.
                    [i6, i5, i3], // 2.
                    [i6, i4, i5], // 3.
                    [i4, i2, i5], // 4.
                ]);
                vertices.extend_from_slice(&[v!(p4), v!(p5), v!(p6)]);
            }
        }
        Self { vertices, indices }
    }
    fn new_cubic_sphere(detail: u32) -> Self {
        macro_rules! v {
            ($v:expr) => {
                Vertex {
                    position: $v,
                    normal: $v,
                    tex_coords: Vec2::ZERO,
                    tangent: Vec3::ZERO,
                }
            };
            ($x:expr, $y:expr, $z:expr) => {
                v!(Vec3::new($x, $y, $z))
            };
        }
        const L: f32 = 0.5773502; // 1/sqrt(3)
        let mut vertices = vec![
            v![L, L, -L],
            v![-L, L, -L],
            v![-L, -L, -L],
            v![L, -L, -L],
            v![-L, L, L],
            v![L, L, L],
            v![L, -L, L],
            v![-L, -L, L],
        ];
        let mut quads = vec![
            [0, 1, 2, 3], // front
            [4, 5, 6, 7], // back
            [5, 0, 3, 6], // right
            [1, 4, 7, 2], // left
            [5, 4, 1, 0], // top
            [3, 2, 7, 6], // bottom
        ];
        for _ in 0..detail {
            for _ in 0..quads.len() {
                let quad = quads.remove(0);
                // indices of the points in the vertices array
                let (i1, i2, i3, i4) = (quad[0], quad[1], quad[2], quad[3]);
                let i5 = vertices.len() as u16;
                let (i6, i7, i8, i9) = (i5 + 1, i5 + 2, i5 + 3, i5 + 4);
                let p1 = vertices[i1 as usize].position;
                let p2 = vertices[i2 as usize].position;
                let p3 = vertices[i3 as usize].position;
                let p4 = vertices[i4 as usize].position;
                let p5 = p1.lerp(p3, 0.5); // middle of a diagonal
                let p6 = p2.lerp(p1, 0.5); // all the points lie on the unit
                let p7 = p2.lerp(p3, 0.5); // sphere
                let p8 = p3.lerp(p4, 0.5);
                let p9 = p4.lerp(p1, 0.5);
                //          p6
                //  p2┌──────┬──────┐p1
                //    │  2.  │   1. │
                //    │      │p5    │
                //  p7├──────┼──────┤p9
                //    │  3.  │   4. │
                //    │      │      │
                //  p3└──────┴──────┘p4
                //           p8
                quads.extend_from_slice(&[
                    [i1, i6, i5, i9], // 1.
                    [i6, i2, i7, i5], // 2.
                    [i5, i7, i3, i8], // 3.
                    [i9, i5, i8, i4], // 4.
                ]);
                vertices.extend_from_slice(&[v!(p5), v!(p6), v!(p7), v!(p8), v!(p9)]);
            }
        }
        for vert in &mut vertices {
            vert.position = vert.position.normalize();
            vert.normal = vert.normal.normalize();
            let d = -vert.position;
            vert.tex_coords.x = 0.5 - d.x.atan2(d.z) / std::f32::consts::TAU;
            vert.tex_coords.y = 0.5 + d.y.asin() / std::f32::consts::PI;
        }
        // quads, but for triangles
        let mut indices = Vec::new();
        for quad in quads {
            indices.push([quad[0], quad[1], quad[2]]);
            indices.push([quad[0], quad[2], quad[3]]);
        }
        let mut res = Self { indices, vertices };
        res.recompute_tangents();
        res
    }
    fn new_cube() -> Self {
        macro_rules! v {
            ($p:tt $n:tt $t:tt) => {
                Vertex {
                    position: Vec3::from($p),
                    normal: Vec3::from($n),
                    tex_coords: Vec2::from($t),
                    tangent: Vec3::ZERO,
                }
            };
        }
        let mut res = Self {
            vertices: vec![
                // Front face
                v!([ 0.5,  0.5, -0.5] [ 0.0,  0.0, -1.0] [0.0, 0.0]),
                v!([-0.5,  0.5, -0.5] [ 0.0,  0.0, -1.0] [1.0, 0.0]),
                v!([-0.5, -0.5, -0.5] [ 0.0,  0.0, -1.0] [1.0, 1.0]),
                v!([ 0.5, -0.5, -0.5] [ 0.0,  0.0, -1.0] [0.0, 1.0]),
                // Back face
                v!([-0.5,  0.5,  0.5] [ 0.0,  0.0,  1.0] [0.0, 0.0]),
                v!([ 0.5,  0.5,  0.5] [ 0.0,  0.0,  1.0] [1.0, 0.0]),
                v!([ 0.5, -0.5,  0.5] [ 0.0,  0.0,  1.0] [1.0, 1.0]),
                v!([-0.5, -0.5,  0.5] [ 0.0,  0.0,  1.0] [0.0, 1.0]),
                // Right face
                v!([ 0.5,  0.5,  0.5] [ 1.0,  0.0,  0.0] [0.0, 0.0]),
                v!([ 0.5,  0.5, -0.5] [ 1.0,  0.0,  0.0] [1.0, 0.0]),
                v!([ 0.5, -0.5, -0.5] [ 1.0,  0.0,  0.0] [1.0, 1.0]),
                v!([ 0.5, -0.5,  0.5] [ 1.0,  0.0,  0.0] [0.0, 1.0]),
                // Left face
                v!([-0.5,  0.5, -0.5] [-1.0,  0.0,  0.0] [0.0, 0.0]),
                v!([-0.5,  0.5,  0.5] [-1.0,  0.0,  0.0] [1.0, 0.0]),
                v!([-0.5, -0.5,  0.5] [-1.0,  0.0,  0.0] [1.0, 1.0]),
                v!([-0.5, -0.5, -0.5] [-1.0,  0.0,  0.0] [0.0, 1.0]),
                // Top face
                v!([-0.5,  0.5,  0.5] [ 0.0,  1.0,  0.0] [0.0, 0.0]),
                v!([-0.5,  0.5, -0.5] [ 0.0,  1.0,  0.0] [1.0, 0.0]),
                v!([ 0.5,  0.5, -0.5] [ 0.0,  1.0,  0.0] [1.0, 1.0]),
                v!([ 0.5,  0.5,  0.5] [ 0.0,  1.0,  0.0] [0.0, 1.0]),
                // Bottom face
                v!([-0.5, -0.5, -0.5] [ 0.0, -1.0,  0.0] [0.0, 0.0]),
                v!([-0.5, -0.5,  0.5] [ 0.0, -1.0,  0.0] [1.0, 0.0]),
                v!([ 0.5, -0.5,  0.5] [ 0.0, -1.0,  0.0] [1.0, 1.0]),
                v!([ 0.5, -0.5, -0.5] [ 0.0, -1.0,  0.0] [0.0, 1.0]),
            ],
            indices: vec![
                [0, 1, 2],
                [0, 2, 3],
                [4, 5, 6],
                [4, 6, 7],
                [8, 9, 10],
                [8, 10, 11],
                [12, 13, 14],
                [12, 14, 15],
                [16, 17, 18],
                [16, 18, 19],
                [20, 21, 22],
                [20, 22, 23],
            ],
        };
        res.recompute_tangents();
        res
    }
}

pub struct MeshManager {
    meshes: SlotMap<MeshHandle, BufferedMesh>,
}

impl MeshManager {
    pub fn new() -> Self {
        Self {
            meshes: SlotMap::with_key(),
        }
    }

    pub fn add(&mut self, device: &wgpu::Device, mesh: &Mesh) -> MeshHandle {
        self.meshes.insert(mesh.buffered(device))
    }

    pub fn add_buffered(&mut self, mesh: BufferedMesh) -> MeshHandle {
        self.meshes.insert(mesh)
    }

    pub fn remove(&mut self, handle: MeshHandle) -> Option<BufferedMesh> {
        self.meshes.remove(handle)
    }

    pub fn update(&mut self, handle: MeshHandle, device: &wgpu::Device, mesh: &Mesh) -> Result<()> {
        self.update_buffered(handle, mesh.buffered(device))
    }

    pub fn update_buffered(&mut self, handle: MeshHandle, mesh: BufferedMesh) -> Result<()> {
        *self
            .meshes
            .get_mut(handle)
            .ok_or_else(|| anyhow!("Handle doesn't point to any mesh"))? = mesh;
        Ok(())
    }

    pub fn get(&self, handle: MeshHandle) -> Option<&BufferedMesh> {
        self.meshes.get(handle)
    }
}

impl Default for MeshManager {
    fn default() -> Self {
        Self::new()
    }
}
