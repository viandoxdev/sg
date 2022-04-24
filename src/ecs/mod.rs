use std::{any::{TypeId, Any}, collections::HashMap};

use uuid::Uuid;

use crate::utils::IntoString;

pub mod components;
pub mod systems;

pub struct ECS {
    components: HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>,
    systems: HashMap<TypeId, Box<dyn System>>,
    system_handles: HashMap<String, Vec<SystemInternal>>,
}

impl ECS  {
    /// Initilize a new ECS
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            systems: HashMap::new(),
            system_handles: HashMap::new()
        }
    }

    // The beauty of ecs
    /// Create new empty entity
    pub fn new_entity(&mut self) -> Uuid {
        Uuid::new_v4()
    }

    /// Add owned entity into ECS
    pub fn add_entity(&mut self, entity: OwnedEntity) -> Uuid {
        let uuid = self.new_entity();
        for (tid, boxed) in entity.into_iter_raw() {
            if let Some(comp) = self.components.get_mut(&tid) {
                comp.insert(uuid, boxed);
            }
        }
        uuid
    }

    /// Remove entity from ECS, returning an owned entity
    pub fn remove_entity(&mut self, entity: Uuid) -> OwnedEntity {
        let mut owned_entity = OwnedEntity::new();
        for (tid, comp) in &mut self.components {
            if let Some(boxed) = comp.remove(&entity) {
                owned_entity.add_raw(*tid, boxed);
            }
        }
        owned_entity
    }

    /// Add component to entity in ECS
    pub fn add_component<C: Component + 'static>(&mut self, entity: Uuid, component: C) {
        self.components.get_mut(&TypeId::of::<C>()).expect("Adding unregistered component")
            .insert(entity, Box::new(component));
    }

    /// Register a new component type
    pub fn register_component<C: Component + 'static>(&mut self) {
        log::debug!("Registering component {}", std::any::type_name::<C>());
        self.components.insert(TypeId::of::<C>(), HashMap::new());
    }

    /// Register a new system into the ECS, systems will be run sequentially in order of
    /// registration
    pub fn register_system<T: System + 'static, S: IntoString>(&mut self, system: T, category: S) {
        let category = category.into_string();
        log::debug!("Registering system {} (-> {category})", T::name());
        self.systems.insert(TypeId::of::<T>(), Box::new(system));
        if let Some(vec) = self.system_handles.get_mut(&category) {
            vec.push(T::handle());
        } else {
            self.system_handles.insert(category, vec![T::handle()]);
        }
    }

    pub fn run_systems<S: IntoString>(&mut self, category: S) {
        let cat = self.system_handles.get(&category.into_string()).expect("Trying to run unknown category");
        for handle in cat {
            (handle.run)(&mut self.components, &mut self.systems);
        }
    }
} 

pub struct OwnedEntity {
    components: HashMap<TypeId, Box<dyn Component + 'static>>
}

impl OwnedEntity {
    pub fn new() -> Self {
        Self {
            components: HashMap::new()
        }
    }
    pub fn add_raw(&mut self, tid: TypeId, comp: Box<dyn Component + 'static>) {
        self.components.insert(tid, comp);
    }
    pub fn add<C: Component + 'static>(&mut self, component: C) {
        self.add_raw(TypeId::of::<C>(), Box::new(component));
    }
    pub fn into_iter_raw(self) -> std::collections::hash_map::IntoIter<TypeId, Box<dyn Component + 'static>> {
        self.components.into_iter()
    }
    pub fn remove_raw(&mut self, tid: TypeId) -> Option<Box<dyn Component + 'static>> {
        self.components.remove(&tid)
    }
    pub fn remove<C: Component + 'static>(&mut self) -> Option<C> {
        let boxed = self.remove_raw(TypeId::of::<C>())?;
        <Box<dyn Any>>::downcast::<C>(boxed).ok().map(|b| *b)
    }
    pub fn get_mut<C: Component + 'static>(&mut self) -> Option<&mut C> {
        <dyn Any>::downcast_mut::<C>(self.components.get_mut(&TypeId::of::<C>())?)
    }
    pub fn get<C: Component + 'static>(&self) -> Option<&C> {
        <dyn Any>::downcast_ref::<C>(self.components.get(&TypeId::of::<C>())?)
    }
}

#[macro_export]
macro_rules! owned_entity {
    ($($comp:expr),*$(,)?) => {{
        use $crate::ecs::OwnedEntity;

        let mut entity = OwnedEntity::new();
        $(entity.add($comp);)*
        entity
    }}
}

pub trait Component: Any {}
pub trait System: Any {
    fn name() -> &'static str where Self: Sized;
    fn handle() -> SystemInternal where Self: Sized;
}
pub struct SystemInternal {
    run: fn (components: &mut HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>, systems: &mut HashMap<TypeId, Box<dyn System>>) -> ()
}

#[macro_export]
macro_rules! make_system {
    ($name:ident {
        $($f:ident: $t:ty),*$(,)?
    } run($self:ident, $($comp:ident: $type:ty),+) $run:block) => {

        pub struct $name {
            $(pub $f: $t),*
        }

        impl $crate::ecs::System for $name {
            fn handle() -> $crate::ecs::SystemInternal {
                use $crate::ecs::{System, SystemInternal, Component};
                use std::{any::{TypeId, Any}, collections::{HashMap, HashSet}};
                use uuid::Uuid;

                fn run(components: &mut HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>, systems: &mut HashMap<TypeId, Box<dyn System>>) {
                    let b = systems.get_mut(&TypeId::of::<$name>()).expect("System isn't part of ECS");
                    let $self = ((&mut **b) as &mut dyn Any).downcast_mut::<$name>().expect("Couldn't downcast system data struct");
                    let reqs: HashSet::<TypeId> = HashSet::from_iter([$(TypeId::of::<$type>()),+].into_iter());
                    let mut comps = components.iter().filter(|(k,_)| reqs.contains(k)).map(|(_, v)| v);
                    let uuids = comps.next().expect("No required component list found").keys()
                        .filter(|k| comps.all(|c| c.contains_key(k)))
                        .map(|u|  u.clone()).collect::<Vec<Uuid>>();
                    for id in uuids {
                        $(let $comp: &mut $type = ((&mut **components.get_mut(&TypeId::of::<$type>()).unwrap().get_mut(&id).unwrap()) as &mut dyn Any).downcast_mut::<$type>().unwrap();)+
                        $run
                    }
                }
                SystemInternal {
                    run
                }
            }

            fn name() -> &'static str {
                stringify!($name)
            }
        }
    };
    ($name:ident {
        $($f:ident: $t:ty),*$(,)?
    } run_many($self:ident, $entities:ident: Vec<($($type:ty),+)>) $run:block) => {
        pub struct $name {
            $(pub $f: $t),*
        }

        impl $crate::ecs::System for $name {
            fn handle() -> $crate::ecs::SystemInternal {
                use $crate::ecs::{System, SystemInternal, Component};
                use std::{any::{TypeId, Any}, collections::{HashMap, HashSet}};
                use uuid::Uuid;

                fn run(components: &mut HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>, systems: &mut HashMap<TypeId, Box<dyn System>>) {
                    let b = systems.get_mut(&TypeId::of::<$name>()).expect("System isn't part of ECS");
                    let $self = ((&mut **b) as &mut dyn Any).downcast_mut::<$name>().expect("Couldn't downcast system data struct");
                    let reqs: HashSet::<TypeId> = HashSet::from_iter([$(TypeId::of::<$type>()),+].into_iter());
                    let mut comps = components.iter().filter(|(k,_)| reqs.contains(k)).map(|(_, v)| v);
                    let uuids = comps.next().unwrap().keys()
                        .filter(|k| comps.all(|c| c.contains_key(k)))
                        .map(|u|  u.clone()).collect::<Vec<Uuid>>();
                    let mut $entities = Vec::new();
                    let mut map = components.iter_mut().map(|(k,v)| 
                            (*k, v.iter_mut().map(|(k, v)| (*k, v)).collect::<HashMap<Uuid, &mut Box<dyn Component + 'static>>>())
                        ).collect::<HashMap<TypeId, HashMap<Uuid, &mut Box<dyn Component +'static>>>>();
                    for id in uuids {
                        $entities.push(
                            (
                                $(((&mut **map.get_mut(&TypeId::of::<$type>()).unwrap().remove(&id).unwrap()) as &mut dyn Any).downcast_mut::<$type>().unwrap()),+
                            )
                        );
                    }
                    $run
                }
                SystemInternal {
                    run
                }
            }

            fn name() -> &'static str {
                stringify!($name)
            }
        }
    };
}
