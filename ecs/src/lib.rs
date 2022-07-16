#![allow(dead_code)]
#![feature(alloc_layout_extra)]
#![feature(once_cell)]
// TODO: Remove those features

mod archetype;
mod bitset;
mod borrows;
mod entity;
mod executor;
mod query;
mod system;
mod thread_pool;
mod world;

pub use archetype::Component;
pub use entity::Entity;
pub use executor::Executor;
pub use executor::Schedule;
pub use executor::Scheduler;
pub use system::Entities;
pub use system::IntoSystem;
pub use world::World;

// TODO: Add component trait that requires 'static + Send + Sync
