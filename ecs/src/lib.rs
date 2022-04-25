#![feature(trait_upcasting)]
#![allow(incomplete_features)]

pub use ecs_macros::system_pass;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use uuid::Uuid;

pub struct ECS {
    components: HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>,
    systems: HashMap<TypeId, Box<dyn System>>,
    systems_categories: HashMap<String, Vec<TypeId>>,
}

impl ECS {
    /// Initilize a new ECS
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            systems: HashMap::new(),
            systems_categories: HashMap::new(),
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
        self.components
            .get_mut(&TypeId::of::<C>())
            .expect("Adding unregistered component")
            .insert(entity, Box::new(component));
    }

    /// Register a new component type
    pub fn register_component<C: Component + 'static>(&mut self) {
        log::debug!("Registering component {}", std::any::type_name::<C>());
        self.components.insert(TypeId::of::<C>(), HashMap::new());
    }

    /// Register a new system into the ECS, systems will be run sequentially in order of
    /// registration
    pub fn register_system<T: System + 'static, S: ToString>(&mut self, system: T, category: S) {
        let category = category.to_string();
        log::debug!("Registering system {} (-> {category})", T::name());
        self.systems.insert(TypeId::of::<T>(), Box::new(system));
        if let Some(vec) = self.systems_categories.get_mut(&category) {
            vec.push(TypeId::of::<T>());
        } else {
            self.systems_categories
                .insert(category, vec![TypeId::of::<T>()]);
        }
    }
    pub fn run_systems<S: ToString>(&mut self, category: S) {
        let category = category.to_string();
        let cat = self
            .systems_categories
            .get(&category)
            .expect("Trying to run unknown category ({category})");
        for system_id in cat {
            let system = self
                .systems
                .get_mut(&system_id)
                .expect("Unknown system in category {category}");
        }
    }
}

pub struct OwnedEntity {
    components: HashMap<TypeId, Box<dyn Component + 'static>>,
}

impl OwnedEntity {
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
        }
    }
    pub fn add_raw(&mut self, tid: TypeId, comp: Box<dyn Component + 'static>) {
        self.components.insert(tid, comp);
    }
    pub fn add<C: Component + 'static>(&mut self, component: C) {
        self.add_raw(TypeId::of::<C>(), Box::new(component));
    }
    pub fn into_iter_raw(
        self,
    ) -> std::collections::hash_map::IntoIter<TypeId, Box<dyn Component + 'static>> {
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
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "<UNAMED_SYSTEM>"
    }
    fn __pass(
        &mut self,
        _components: &mut HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>,
    ) -> () {
    }
    fn pass(
        _components: &mut HashMap<TypeId, HashMap<Uuid, Box<dyn Component>>>,
        _systems: &mut HashMap<TypeId, Box<dyn System>>,
    ) -> ()
    where
        Self: Sized,
    {
    }
    fn pre(&mut self) -> ()
    where
        Self: Sized,
    {
    }
    fn post(&mut self) -> ()
    where
        Self: Sized,
    {
    }
}
