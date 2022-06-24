use ecs_macros::impl_query;

use crate::{
    archetype::Archetype,
    bitset::{BitsetBuilder, BorrowBitset},
};

pub trait Query {
    fn match_archetype(archetype: &Archetype) -> bool;
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self;
    fn bitset(builder: &mut BitsetBuilder) -> BorrowBitset;
}

impl<T: 'static> Query for &T {
    fn match_archetype(archetype: &Archetype) -> bool {
        archetype.has::<T>()
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        unsafe { &*(ptr.add(archetype.offset::<T>()) as *const T) }
    }
    fn bitset(builder: &mut BitsetBuilder) -> BorrowBitset {
        builder.start_borrow().borrow::<T>().build_borrow()
    }
}

impl<T: 'static> Query for &mut T {
    fn match_archetype(archetype: &Archetype) -> bool {
        archetype.has::<T>()
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        unsafe { &mut *(ptr.add(archetype.offset::<T>()) as *mut T) }
    }
    fn bitset(builder: &mut BitsetBuilder) -> BorrowBitset {
        builder.start_borrow().borrow::<T>().build_borrow()
    }
}

impl<T: Query> Query for Option<T> {
    fn match_archetype(_archetype: &Archetype) -> bool {
        true
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        if T::match_archetype(archetype) {
            Some(T::build(ptr, archetype))
        } else {
            None
        }
    }
    fn bitset(builder: &mut BitsetBuilder) -> BorrowBitset {
        let set = T::bitset(builder).optional();
        builder.start_borrow().with(set).build_borrow()
    }
}

impl_query!(16);
