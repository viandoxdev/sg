#![feature(alloc_layout_extra)]
#![feature(ptr_metadata)]
// TODO: remove
#![allow(dead_code)]

mod archetype;

#[derive(Default)]
struct ComponentList {
    inner: u128,
}

struct World {}
