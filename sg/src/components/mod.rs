use ecs::Component;

use crate::systems::graphics::MeshHandle;

#[derive(Debug, Clone, Copy)]
pub struct PositionComponent {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Component for PositionComponent {}

pub struct GraphicsComponent {
    pub mesh: MeshHandle
}
impl Component for GraphicsComponent {}
