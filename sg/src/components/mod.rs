use anyhow::Result;
use ecs::Component;
use glam::{Mat4, Quat, Vec3};

use crate::systems::graphics::{
    mesh_manager::MeshHandle,
    texture_manager::{SingleValue, TextureHandle, TextureSet},
    GraphicSystem, Light,
};

#[derive(Debug, Clone, Copy)]
pub struct PositionComponent {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub struct GraphicsComponent {
    pub(crate) mesh: MeshHandle,
    pub(crate) textures: TextureSet,
}
#[derive(Clone, Copy)]
pub struct LightComponent {
    pub light: Light,
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

impl GraphicsComponent {
    pub fn new_with_values(
        mesh: MeshHandle,
        albedo: TextureHandle,
        normal_map: Option<TextureHandle>,
        metallic: f32,
        roughness: f32,
        ao: Option<TextureHandle>,
        gfx: &mut GraphicSystem,
    ) -> Result<Self> {
        let metallic = gfx.texture_manager.get_or_add_single_value_texture(
            &gfx.device,
            &gfx.queue,
            SingleValue::Factor(metallic),
        );
        let roughness = gfx.texture_manager.get_or_add_single_value_texture(
            &gfx.device,
            &gfx.queue,
            SingleValue::Factor(roughness),
        );
        Self::new(mesh, albedo, normal_map, metallic, roughness, ao, gfx)
    }
    pub fn new(
        mesh: MeshHandle,
        albedo: TextureHandle,
        normal_map: Option<TextureHandle>,
        metallic: TextureHandle,
        roughness: TextureHandle,
        ao: Option<TextureHandle>,
        gfx: &mut GraphicSystem,
    ) -> Result<Self> {
        let set = gfx.texture_manager.add_set();
        let normal_map = normal_map.unwrap_or_else(|| {
            gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Normal(Vec3::new(0.0, 0.0, 1.0)),
            )
        });
        let ao = ao.unwrap_or_else(|| {
            gfx.texture_manager.get_or_add_single_value_texture(
                &gfx.device,
                &gfx.queue,
                SingleValue::Factor(1.0),
            )
        });
        gfx.texture_manager.add_texture_to_set(albedo, set)?;
        gfx.texture_manager.add_texture_to_set(normal_map, set)?;
        gfx.texture_manager.add_texture_to_set(metallic, set)?;
        gfx.texture_manager.add_texture_to_set(roughness, set)?;
        gfx.texture_manager.add_texture_to_set(ao, set)?;
        Ok(Self {
            textures: set,
            mesh,
        })
    }
}

impl Component for PositionComponent {}
impl Component for TransformsComponent {}
impl Component for GraphicsComponent {}
impl Component for LightComponent {}
