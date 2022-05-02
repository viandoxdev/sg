use ecs::Component;
use glam::{Mat4, Vec3, Quat};

use crate::systems::graphics::{mesh_manager::MeshHandle, texture_manager::TextureHandle};

#[derive(Debug, Clone, Copy)]
pub struct PositionComponent {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub struct GraphicsComponent {
    pub mesh: MeshHandle,
    pub texture: TextureHandle,
}
#[derive(Clone)]
pub struct TransformsComponent {
    translate: Vec3,
    scale: Vec3,
    rotate: Quat,
    matrix: Mat4,
}
impl Default for TransformsComponent {
    fn default() -> Self {
        Self::new()
    }
}
impl TransformsComponent {
    pub fn new() -> Self {
        Self {
            translate: Vec3::ZERO,
            scale: Vec3::ONE,
            rotate: Quat::default(),
            matrix: Mat4::IDENTITY
        }
    }
    fn update(&mut self) {
        self.matrix = Mat4::from_scale_rotation_translation(self.scale, self.rotate, self.translate);
    }
    pub fn set_translation(&mut self, trans: Vec3) {
        self.translate = trans;
        self.update();
    }
    pub fn set_scale(&mut self, scale: Vec3) {
        self.scale = scale;
        self.update();
    }
    pub fn set_rotation(&mut self, rotation: Quat) {
        self.rotate = rotation;
        self.update();
    }
    pub fn mat(&self) -> Mat4 {
        self.matrix
    }
}

impl Component for PositionComponent {}
impl Component for TransformsComponent {}
impl Component for GraphicsComponent {}
