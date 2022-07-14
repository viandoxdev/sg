use anyhow::Result;
use glam::{Mat4, Quat, Vec3};
use uuid::Uuid;

use crate::systems::graphics::{
    mesh_manager::MeshHandle,
    texture_manager::{SingleValue, TextureHandle, TextureSet},
    GraphicContext, Light, Material,
};

#[derive(Debug, Clone, Copy)]
pub struct PositionComponent {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub struct GraphicsComponent {
    pub(crate) mesh: MeshHandle,
    pub(crate) material: Material,
}

#[derive(Clone, Copy)]
pub struct LightComponent {
    pub light: Light,
    pub id: Uuid,
}

impl LightComponent {
    pub fn new(light: Light) -> Self {
        Self {
            light,
            id: Uuid::new_v4(),
        }
    }
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
            matrix: Mat4::IDENTITY,
        }
    }
    fn update(&mut self) {
        self.matrix =
            Mat4::from_scale_rotation_translation(self.scale, self.rotate, self.translate);
    }
    pub fn set_translation(&mut self, trans: Vec3) -> &mut Self {
        self.translate = trans;
        self.update();
        self
    }
    pub fn set_scale(&mut self, scale: Vec3) -> &mut Self {
        self.scale = scale;
        self.update();
        self
    }
    pub fn set_rotation(&mut self, rotation: Quat) -> &mut Self {
        self.rotate = rotation;
        self.update();
        self
    }
    pub fn mat(&self) -> Mat4 {
        self.matrix
    }
}
