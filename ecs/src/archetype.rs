use std::{
    alloc::{self, Layout},
    any::{Any, TypeId},
    collections::HashMap,
    mem::MaybeUninit,
    ops::{Bound, RangeBounds},
    ptr::{DynMetadata, NonNull},
};

use ecs_macros::impl_archetype;

type DropInPlace = fn(*mut ());

// Most of the code here is *heavily* inspired by the implementing Vec chapter of the Rustinomicon
// https://doc.rust-lang.org/nomicon/vec/vec.html

/// The storage for an archetype
pub struct ArchetypeStorage {
    data: NonNull<u8>,
    capacity: usize,
    length: usize,
    archetype: Archetype,
}

#[derive(PartialEq, Eq)]
pub struct ComponentType {
    /// The offset from the begining of the entity
    offset: usize,
    /// A fn pointer to the drop implementation of the type (if needed)
    drop: Option<DropInPlace>,
    /// The size of an instance of the component
    size: usize,
}

pub struct Archetype {
    /// Info about each type
    info: HashMap<TypeId, ComponentType>,
    /// Memory layout of an entity of this archetype
    layout: Layout,
}

impl Archetype {
    /// Drop the entity at ptr
    fn drop(&self, ptr: *mut u8) {
        for comp in self.info.values() {
            if let Some(drop) = comp.drop {
                unsafe {
                    let ptr = ptr.add(comp.offset);
                    drop(ptr as *mut ());
                }
            }
        }
    }
    fn is_zst(&self) -> bool {
        self.layout.size() == 0
    }
    /// Test if two archetypes match (order independant)
    fn match_archetype(&self, other: &Archetype) -> bool {
        if self.info.len() == other.info.len() {
            for id in self.info.keys() {
                if !other.info.contains_key(id) {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
    /// Test if two archetypes exactly match (same memory layout)
    fn exact_match(&self, other: &Archetype) -> bool {
        if self.layout != other.layout {
            return false;
        }

        if self.info.len() == other.info.len() {
            for (id, info) in &self.info {
                if let Some(o_info) = other.info.get(id) {
                    if o_info != info {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
}

impl ArchetypeStorage {
    pub fn new<T: IntoArchetype>() -> Self {
        let archetype = T::into_archetype();
        // If size is 0, no allocation is needed, so we set capacity to the max:
        // The allocated bytes (none) is enough to hold an infinity of elements
        let capacity = if archetype.is_zst() { !0 } else { 0 };
        Self {
            archetype,
            data: NonNull::dangling(),
            capacity,
            length: 0,
        }
    }
    #[inline(always)]
    fn get_ptr_mut(&mut self, index: usize) -> *mut u8 {
        assert!(self.length > index);
        unsafe { self.data.as_ptr().add(self.archetype.layout.size() * index) }
    }
    #[inline(always)]
    fn get_ptr(&self, index: usize) -> *const u8 {
        assert!(self.length > index);
        unsafe { self.data.as_ptr().add(self.archetype.layout.size() * index) }
    }
    /// Push an entity, T must match the type
    pub fn push<T: IntoArchetype>(&mut self, value: T) {
        if self.capacity == self.length {
            self.grow(self.capacity + 1);
        }

        unsafe {
            let slot = self
                .data
                .as_ptr()
                .add(self.archetype.layout.size() * self.length);
            value.write(slot, &self.archetype);
        }

        self.length += 1;
    }
    /// Push multiple entities, optimized for allocations where possible
    pub fn extend<T: IntoArchetype>(&mut self, values: impl IntoIterator<Item = T>) {
        let iter = values.into_iter();
        let hint = iter.size_hint().1;
        if let Some(len) = hint {
            self.grow(self.capacity + len);
        }
        for value in iter {
            self.push(value);
        }
    }
    /// Fill the gap the vector from index start and for length element and set the new length
    #[inline(always)]
    fn fill_gap(&mut self, start: usize, length: usize) {
        // If the archetype is zero sized there is no allocation, so no gap
        if start + length < self.length && !self.archetype.is_zst() {
            let copy_to = self.get_ptr_mut(start);
            let copy_from = self.get_ptr_mut(start + length);
            let copy_for = self.archetype.layout.size() * (self.length - start - length);
            unsafe {
                std::ptr::copy(copy_from, copy_to, copy_for);
            }
        }
        self.length -= length;
    }
    /// Remove and drop and entity from the array
    pub fn remove(&mut self, index: usize) {
        let slot = self.get_ptr_mut(index);
        self.archetype.drop(slot);
        self.fill_gap(index, 1);
    }
    /// drop a range of entities
    pub fn clear(&mut self, bounds: impl RangeBounds<usize>) {
        // start index (inclusive)
        let start = match bounds.start_bound() {
            Bound::Unbounded => 0,
            Bound::Included(&i) => i.min(self.length - 1),
            Bound::Excluded(e) => (e + 1).min(self.length - 1),
        };
        // end index, exclusive
        let end = match bounds.end_bound() {
            Bound::Unbounded => self.length,
            Bound::Included(i) => i.saturating_sub(1).min(self.length),
            Bound::Excluded(&e) => e.min(self.length),
        };
        for i in start..end {
            let ptr = self.get_ptr_mut(i);
            self.archetype.drop(ptr);
        }
        if end > start {
            self.fill_gap(start, end - start);
        }
    }
    /// Take an entity and return it, the archetype needs to matche the storage's
    pub fn take<T: IntoArchetype>(&mut self, index: usize) -> T {
        let ptr = self.get_ptr_mut(index);
        let value = unsafe { T::read(ptr, &self.archetype) };
        self.fill_gap(index, 1);
        value
    }
    pub fn len(&self) -> usize {
        self.length
    }
    /// Get a slice of the entities, the archetypes must exactly match
    pub fn as_slice<T: IntoArchetype>(&self) -> &[T] {
        if !T::into_archetype().exact_match(&self.archetype) {
            panic!("Archetype don't exactly match");
        }
        unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const T, self.length) }
    }
    /// Grow the storage to hold at least new_cap elements
    /// This should (and will) never be called if entity_size is 0.
    fn grow(&mut self, new_cap: usize) {
        let new_cap = self
            .capacity
            .checked_mul(2)
            .expect("ArchetypeStorage overflow")
            .max(new_cap);

        // The offset is always just self.entity_layout.size(), so we ignore it
        let (layout, _) = self
            .archetype
            .layout
            .repeat(new_cap)
            .expect("ArchetypeStorage overflow");

        let ptr = if self.capacity == 0 {
            // We haven't allocated yet
            self.capacity = new_cap;
            unsafe { alloc::alloc(layout) }
        } else {
            // We need to reallocated
            let (old_layout, _) = self.archetype.layout.repeat(self.capacity).unwrap();
            unsafe { alloc::realloc(self.data.as_ptr(), old_layout, layout.size()) }
        };

        self.data = match NonNull::new(ptr) {
            Some(p) => p,
            None => alloc::handle_alloc_error(layout),
        };
        self.capacity = new_cap;
    }
}

impl Drop for ArchetypeStorage {
    fn drop(&mut self) {
        self.clear(..);
        // dealloc memory
        if self.capacity > 0 {
            unsafe {
                let layout = self.archetype.layout.repeat(self.capacity).unwrap().0;
                alloc::dealloc(self.data.as_ptr(), layout);
            }
        }
    }
}

// Where the cursed shit happens
/// Take a fat reference, extract its metadata and get the drop_in_place function pointer from it
unsafe fn get_drop(fat: &dyn Any) -> DropInPlace {
    // Get raw fat pointer
    let fat = fat as *const dyn Any;
    // get the metadata
    let (_, metadata): (_, DynMetadata<dyn Any>) = fat.to_raw_parts();
    // SAFETY: This is not safe, this only works because the DynMetadata struct has only
    // one (not ZST) field, so the struct should already follow alignment rules, and with
    // no reordering possible, a DynMetadata can be reinterpreted as that field: a pointer
    // to a VTable struct.
    // The VTable struct is repr(C), so reinterpreting this pointer as a pointer to its
    // first field (the fn pointer for drop_in_place) IS valid.
    let drop_ptr: *const fn(*mut ()) = std::mem::transmute(metadata);
    // unsafe, dereference the pointer to the fn pointer
    *drop_ptr
}

pub trait IntoArchetype {
    /// Get the archetype of this tuple
    fn into_archetype() -> Archetype;
    /// Check if the archetypes match, this is faster than calling into_archetype and matching over
    /// them.
    fn match_archetype(archetype: &Archetype) -> bool;
    /// Write self to dst, archetypes must match (order independant)
    unsafe fn write(self, dst: *mut u8, archetype: &Archetype);
    /// Read a value from src,, archetypes must match (order independant)
    unsafe fn read(src: *const u8, archetype: &Archetype) -> Self;
}

impl_archetype!(16);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drop() {
        type D = i32;
        let drop;
        unsafe {
            let fat = &*MaybeUninit::<D>::uninit().as_ptr();
            drop = get_drop(fat);
        }
        let mut v = 32;
        let ptr = &mut v as *mut D as *mut ();
        drop(ptr);
        std::mem::forget(v);
    }
    #[test]
    fn init() {
        let _at = ArchetypeStorage::new::<(i16, u64)>();
    }
    #[test]
    fn push_remove() {
        let mut at = ArchetypeStorage::new::<(String, u16, bool)>();
        at.push((true, "Test".to_owned(), 12u16)); // 0 -> 0
        at.push(("Another".to_owned(), false, 14u16)); // 1 -> X
        at.push((false, 57u16, "thing".to_owned())); // 2 -> 1
        println!("{:?}", at.as_slice::<(String, u16, bool)>());
        at.remove(1);
        let v = at.take::<(u16, bool, String)>(1);
        assert_eq!(v.0, 57);
        assert_eq!(v.1, false);
        assert_eq!(v.2, "thing");
        at.clear(..);
    }
    #[test]
    fn push_and_take() {
        let mut at = ArchetypeStorage::new::<(u16, u64)>();
        at.push((32u64, 12u16));
        let val: (u16, u64) = at.take(0);
        assert_eq!(val.0, 12);
        assert_eq!(val.1, 32);
    }
    #[test]
    fn clear() {
        let mut at = ArchetypeStorage::new::<(u16, u64)>();
        at.push((32u64, 12u16));
        at.push((35u16, 15u64));
        at.push((29u64, 16u16));
        println!("pre clear: {:?}", at.as_slice::<(u16, u64)>());
        at.clear(..);
        println!("post clear: {:?}", at.as_slice::<(u16, u64)>());
    }
}
