use std::{f32::consts::FRAC_PI_2, lazy::OnceCell};

use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy)]
pub enum Projection {
    Perspective,
    Orthograhic,
    None,
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
    dirty: bool,
    buffer: OnceCell<wgpu::Buffer>,
    bind_group: OnceCell<wgpu::BindGroup>,
    bind_group_layout: OnceCell<wgpu::BindGroupLayout>,
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
        let view = Mat4::from_rotation_translation(self.rotation.inverse(), -self.position);
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
    }
    fn get_buffer(&self, device: &wgpu::Device) -> &wgpu::Buffer {
        self.buffer.get_or_init(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                mapped_at_creation: false,
                size: 64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                label: Some("Camera matrix buffer"),
            })
        })
    }
    pub(in crate::systems::graphics) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        if self.is_dirty() {
            log::info!("Dirty {:?}", self.position);
            self.recompute_matrix();
            queue.write_buffer(
                self.get_buffer(device),
                0,
                bytemuck::cast_slice(self.matrix.as_ref()),
            );
            self.unset_dirty();
        }
    }
    pub(in crate::systems::graphics) fn get_bind_group(
        &self,
        device: &wgpu::Device,
    ) -> &wgpu::BindGroup {
        self.bind_group.get_or_init(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: self.get_bind_group_layout(device),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: self.get_buffer(device),
                        offset: 0,
                        size: None,
                    }),
                }],
                label: Some("Camera bind group"),
            })
        })
    }
    // TODO: just use &mut self
    pub(in crate::systems::graphics) fn get_bind_group_layout(
        &self,
        device: &wgpu::Device,
    ) -> &wgpu::BindGroupLayout {
        self.bind_group_layout.get_or_init(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("Camera bind group layout"),
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
            dirty: true,
            buffer: OnceCell::new(),
            bind_group: OnceCell::new(),
            bind_group_layout: OnceCell::new(),
        };
        cam.recompute_matrix();
        cam
    }
}
