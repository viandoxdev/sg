use std::{f32::consts::FRAC_PI_2, lazy::OnceCell};

use glam::{Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

#[derive(Clone, Copy)]
pub enum Projection {
    Perspective,
    Orthograhic,
    None,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraInfo {
    view_projection: Mat4,
    view: Mat4,
    camera_pos: Vec3,
    aspect: f32,
}

pub struct Camera {
    position: Vec3,
    rotation: Quat,
    fov: f32,
    near: f32,
    far: f32,
    projection: Projection,
    aspect: f32,
    matrix: Mat4,
    view_mat: Mat4,
    dirty: bool,
    buffer: OnceCell<wgpu::Buffer>,
    bind_group: OnceCell<wgpu::BindGroup>,
    bind_group_layout: OnceCell<wgpu::BindGroupLayout>,
    skybox: OnceCell<wgpu::TextureView>,
    irradiance_map: OnceCell<wgpu::TextureView>
}

impl Camera {
    pub fn new() -> Self {
        Self::default()
    }
    fn set_dirty(&mut self) {
        self.dirty = true;
    }
    fn unset_dirty(&mut self) {
        self.dirty = false;
    }
    fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn set_position(&mut self, position: Vec3) {
        self.position = position;
        self.set_dirty();
    }
    pub fn set_rotation(&mut self, rotation: Quat) {
        self.rotation = rotation;
        self.set_dirty();
    }
    pub fn set_fov(&mut self, fov: f32) {
        self.fov = fov;
        self.set_dirty();
    }
    pub fn set_near(&mut self, near: f32) {
        self.near = near;
        self.set_dirty();
    }
    pub fn set_far(&mut self, far: f32) {
        self.far = far;
        self.set_dirty();
    }
    pub fn set_projection(&mut self, projection: Projection) {
        self.projection = projection;
        self.set_dirty();
    }
    pub fn set_aspect(&mut self, aspect: f32) {
        self.aspect = aspect;
        self.set_dirty();
    }
    pub fn set_skybox(&mut self, skybox: wgpu::TextureView) {
        self.skybox.take();
        self.skybox.set(skybox).ok();
        self.bind_group.take();
    }
    pub fn set_irradiance_map(&mut self, irr_map: wgpu::TextureView) {
        self.irradiance_map.take();
        self.irradiance_map.set(irr_map).ok();
        self.bind_group.take();
    }
    pub fn get_position(&self) -> Vec3 {
        self.position
    }
    pub fn get_rotation(&self) -> Quat {
        self.rotation
    }
    pub fn get_fov(&self) -> f32 {
        self.fov
    }
    pub fn get_near(&self) -> f32 {
        self.near
    }
    pub fn get_far(&self) -> f32 {
        self.far
    }
    pub fn get_projection(&self) -> Projection {
        self.projection
    }
    pub fn get_aspect(&self) -> f32 {
        self.aspect
    }
    fn recompute_matrix(&mut self) {
        let mut view = Mat4::from_quat(self.rotation.inverse());
        view *= Mat4::from_translation(-self.position);
        let projection = match self.projection {
            Projection::Perspective => {
                Mat4::perspective_lh(self.fov, self.aspect, self.near, self.far)
            }
            Projection::Orthograhic => {
                let top = self.near * self.fov.tan();
                let bottom = -top;
                let left = top * self.aspect;
                let right = -left;
                Mat4::orthographic_rh(left, right, bottom, top, self.near, self.far)
            }
            Projection::None => Mat4::IDENTITY,
        };
        self.matrix = projection * view;
        self.view_mat = view;
    }
    fn get_info(&self) -> CameraInfo {
        CameraInfo {
            view_projection: self.matrix,
            view: self.view_mat,
            camera_pos: self.position,
            aspect: self.aspect,
        }
    }
    fn get_buffer(&self, device: &wgpu::Device) -> &wgpu::Buffer {
        self.buffer.get_or_init(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                mapped_at_creation: false,
                size: std::mem::size_of::<CameraInfo>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                label: Some("Camera matrix buffer"),
            })
        })
    }
    fn get_skybox(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> &wgpu::TextureView {
        self.skybox.get_or_init(|| {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 6,
                    },
                    label: Some("Default Skybox"),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    dimension: wgpu::TextureDimension::D2,
                    sample_count: 1,
                    mip_level_count: 1,
                },
                bytemuck::cast_slice::<[u8; 4], u8>(&[[255; 4]; 6])
            ).create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            })
        })
    }
    pub fn get_irradiance(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> &wgpu::TextureView {
        self.irradiance_map.get_or_init(|| {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 6,
                    },
                    label: Some("Default Skybox"),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    dimension: wgpu::TextureDimension::D2,
                    sample_count: 1,
                    mip_level_count: 1,
                },
                bytemuck::cast_slice::<[u8; 4], u8>(&[[30; 4]; 6])
            ).create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            })
        })
    }
    pub(in crate::systems::graphics) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        if self.is_dirty() {
            self.recompute_matrix();
            queue.write_buffer(
                self.get_buffer(device),
                0,
                bytemuck::bytes_of(&self.get_info()),
            );
            self.unset_dirty();
        }
    }
    pub(in crate::systems::graphics) fn get_bind_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> &wgpu::BindGroup {
        self.bind_group.get_or_init(|| {
            create_bind_group!(device, &self.get_bind_group_layout(device), "Camera Bind Group": {
                0 | Buffer(buffer: (self.get_buffer(device))),
                1 | TextureView(self.get_skybox(device, queue)),
                2 | TextureView(self.get_irradiance(device, queue))
            })
        })
    }
    // TODO: just use &mut self
    pub(in crate::systems::graphics) fn get_bind_group_layout(
        &self,
        device: &wgpu::Device,
    ) -> &wgpu::BindGroupLayout {
        self.bind_group_layout.get_or_init(|| {
            create_bind_group_layout!(device, "Camera Bind Group Layout": {
                0 => VERTEX, FRAGMENT | Buffer(type: Uniform),
                1 => FRAGMENT | Texture(view_dim: Cube, sample: FloatFilterable),
                2 => FRAGMENT | Texture(view_dim: Cube, sample: FloatFilterable),
            })
        })
    }
}

impl Default for Camera {
    fn default() -> Self {
        let mut cam = Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective,
            aspect: 1.777,
            fov: FRAC_PI_2,
            near: 0.1,
            far: 100.0,
            matrix: Mat4::IDENTITY,
            view_mat: Mat4::IDENTITY,
            dirty: true,
            buffer: OnceCell::new(),
            bind_group: OnceCell::new(),
            bind_group_layout: OnceCell::new(),
            skybox: OnceCell::new(),
            irradiance_map: OnceCell::new(),
        };
        cam.recompute_matrix();
        cam
    }
}
