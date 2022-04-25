use ecs::Component;

#[derive(Debug, Clone, Copy)]
pub struct PositionComponent {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Component for PositionComponent {}
