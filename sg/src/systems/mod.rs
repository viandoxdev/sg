use ecs::{downcast_component, system_pass, System, SystemRequirements};

use crate::components::PositionComponent;

pub mod graphics;

pub struct GravitySystem {
    pub g: f64,
}

impl System for GravitySystem {
    #[system_pass]
    fn pass(&mut self, pos: PositionComponent) {
        pos.z -= self.g;
    }
}

pub struct CenterSystem {
    pub res: PositionComponent,
}

impl System for CenterSystem {
    #[system_pass]
    fn pass_many(&mut self, entities: HashMap<Uuid, (PositionComponent)>) {
        let mut pos = PositionComponent {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let len = entities.len();
        for (_, epos) in entities {
            pos.x += epos.x;
            pos.y += epos.y;
            pos.z += epos.z;
        }
        pos.x /= len as f64;
        pos.y /= len as f64;
        pos.z /= len as f64;
        self.res = pos;
        log::debug!("Ran CenterSystem => {pos:?}");
    }
}

pub struct LoggingSystem {}

impl System for LoggingSystem {
    fn pass(
        &mut self,
        components: &mut std::collections::HashMap<
            std::any::TypeId,
            std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>,
        >,
    ) {
        let reqs = SystemRequirements::new().add::<PositionComponent>();
        let entities = reqs.filter(components);
        for (_, mut comps) in entities {
            let pos = downcast_component::<PositionComponent>(&mut comps).unwrap();

            log::debug!("pos: {pos:?}");
        }
    }
}
