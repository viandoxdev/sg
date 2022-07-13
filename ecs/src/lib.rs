#![feature(alloc_layout_extra)]
#![feature(once_cell)]

mod archetype;
mod bitset;
mod borrows;
mod entity;
mod query;
mod scheduler;
mod system;
mod thread_pool;
mod world;

pub use world::World;
