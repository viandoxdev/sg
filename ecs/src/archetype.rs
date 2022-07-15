use std::{
    alloc::{self, Layout},
    any::TypeId,
    collections::HashMap,
    mem::MaybeUninit,
    ops::{Bound, RangeBounds},
    ptr::NonNull,
};

use ecs_macros::impl_archetype;

use crate::{
    bitset::{ArchetypeBitset, ArchetypeBitsetBuilder, ArchetypeBitsetMapping, BitsetBuilder},
    query::{Query, QueryIter}, entity::LocationMap,
};

type DropInPlace = fn(*mut ());

// Most of the code here is *heavily* inspired by the implementing Vec chapter of the Rustonomicon
// https://doc.rust-lang.org/nomicon/vec/vec.html

/// The storage for an archetype
pub struct ArchetypeStorage {
    data: NonNull<u8>,
    capacity: usize,
    length: usize,
    archetype: Archetype,
}

#[derive(PartialEq, Eq, Clone)]
pub struct ComponentType {
    /// The offset from the begining of the entity
    offset: usize,
    /// A fn pointer to the drop implementation of the type (if needed)
    drop: Option<DropInPlace>,
    /// The size of an instance of the component
    size: usize,
    /// The min alignment of the component
    alignment: usize,
}

#[derive(Clone)]
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
    pub fn is_zst(&self) -> bool {
        self.layout.size() == 0
    }
    /// Test if two archetypes match, does't care about order, but ensure both archetypes contain
    /// the same number of types
    pub fn match_archetype(&self, other: &Archetype) -> bool {
        self.info.len() == other.info.len() && self.lose_match(other)
    }
    /// Test if two archetypes exactly match (same memory layout)
    pub fn exact_match(&self, other: &Archetype) -> bool {
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
    /// Test if self 'fits' into other, doesn't care about order or extra types which might be
    /// there
    pub fn lose_match(&self, other: &Archetype) -> bool {
        for id in self.info.keys() {
            if !other.info.contains_key(id) {
                return false;
            }
        }
        true
    }
    /// Get the offset of the value of a type in the memory layout of this archetype
    pub fn offset<T: 'static>(&self) -> usize {
        self.info[&TypeId::of::<T>()].offset
    }
    /// Check if the archetype contains a type
    pub fn has<T: 'static>(&self) -> bool {
        self.info.contains_key(&TypeId::of::<T>())
    }
    /// Copy the components from a location with this archetype to another location following
    /// another archetype.
    /// # safety
    /// Components not included in the other archetype are *not* dropped.
    pub unsafe fn try_write(&self, src: *const u8, dst: *mut u8, archetype: &Archetype) {
        for (id, src_c) in &self.info {
            let dst_c = match archetype.info.get(id) {
                Some(v) => v,
                None => continue,
            };
            let src = src.add(src_c.offset);
            let dst = dst.add(dst_c.offset);
            std::ptr::copy(src, dst, src_c.size);
        }
    }
    /// Merge the archetypes to create a new one. Note that this can create an archetype with a non
    /// repr(rust) memory layout, that can't be reinterpreted as a rust tuple.
    pub fn merge(&mut self, other: Archetype) {
        let (layout, offset) = self
            .layout
            .extend(other.layout)
            .expect("Archetype overflow");
        for (id, mut info) in other.info {
            info.offset += offset;
            self.info.insert(id, info);
        }
        self.layout = layout;
    }
    /// Remove the components of other from self. Note that this will recompute the memory layout
    /// of the archetype and will not be interpretable as a valid rust tuple anymore (if it was).
    pub fn subtract(&mut self, other: Archetype) {
        for id in other.info.keys() {
            self.info.remove(id);
        }
        // recompute memory layout
        self.layout = Layout::from_size_align(0, 1).unwrap();
        for info in self.info.values_mut() {
            let field = Layout::from_size_align(info.size, info.alignment).unwrap();
            let (new_layout, offset) = self.layout.extend(field).unwrap();
            info.offset = offset;
            self.layout = new_layout;
        }
        self.layout = self.layout.pad_to_align();
    }
    /// returns the size of an element of the archetype
    pub fn size(&self) -> usize {
        self.layout.size()
    }
}

impl ArchetypeStorage {
    #[inline]
    pub fn new<T: IntoArchetype>() -> Self {
        Self::new_from_archetype(T::into_archetype())
    }
    pub fn new_from_archetype(archetype: Archetype) -> Self {
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
    unsafe fn get_ptr_mut_unchecked(&mut self, index: usize) -> *mut u8 {
        self.data.as_ptr().add(self.archetype.layout.size() * index)
    }
    #[inline(always)]
    unsafe fn get_ptr_unchecked(&self, index: usize) -> *const u8 {
        self.data.as_ptr().add(self.archetype.layout.size() * index)
    }
    #[inline(always)]
    fn get_ptr_mut(&mut self, index: usize) -> *mut u8 {
        assert!(self.length > index);
        unsafe { self.get_ptr_mut_unchecked(index) }
    }
    #[inline(always)]
    fn get_ptr(&self, index: usize) -> *const u8 {
        assert!(self.length > index);
        unsafe { self.get_ptr_unchecked(index) }
    }
    /// Push an entity, T must match the type
    pub fn push<T: IntoArchetype>(&mut self, value: T) {
        if self.capacity == self.length {
            self.grow(self.capacity + 1);
        }

        unsafe {
            let slot = self.get_ptr_mut_unchecked(self.length);
            value.write(slot, &self.archetype);
        }

        self.length += 1;
    }
    /// Push multiple entities, optimized for allocations where possible
    pub fn extend<T: IntoArchetype>(&mut self, values: impl IntoIterator<Item = T>) {
        let iter = values.into_iter();
        let hint = iter.size_hint().1;
        if let Some(len) = hint {
            if self.capacity < self.length + len {
                self.grow(self.capacity + len);
            }
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
    /// Move an entity from this storage to another
    /// # safety
    /// Components included in this archetype, but not in the destination are forgotten
    /// Components not included in this archetype but included in the destination are uninitialized
    /// This method should only be used when accounting for both case
    /// Returns the new index
    pub unsafe fn move_entity(&mut self, index: usize, other: &mut ArchetypeStorage) -> usize {
        let new_index = other.length;
        if other.capacity == other.length {
            other.grow(other.capacity + 1);
        }
        self.archetype.try_write(
            self.get_ptr(index),
            other.get_ptr_mut_unchecked(new_index),
            &other.archetype,
        );
        other.length += 1;
        self.fill_gap(index, 1);
        new_index
    }
    /// Write components to an index, this doesn't drop the previous value, and should only be
    /// called to write to uninitialized components
    pub unsafe fn write<T: IntoArchetype>(&mut self, index: usize, value: T) {
        value.write(self.get_ptr_mut(index), &self.archetype);
    }
    /// Read components of an entity, this copies the bytes and is unsafe for non Copy components,
    /// this should only be used to copy components that will be forgotten
    pub unsafe fn read<T: IntoArchetype>(&mut self, index: usize) -> T {
        T::read(self.get_ptr(index), &self.archetype)
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
    /// Create an QueryIter of this storage, this doesn't have any memory safety checks and will
    /// break if used after drop of this storage, or if used concurently.
    pub unsafe fn iter_query<Q: Query>(&self, index: usize, location_map: Option<&LocationMap>) -> QueryIter<Q> {
        QueryIter::new(
            self.data,
            self.length,
            &self.archetype as *const Archetype,
            index,
            location_map.map(|v| v as *const LocationMap),
        )
    }
    /// Get the archetype of this storage
    pub fn archetype(&self) -> &Archetype {
        &self.archetype
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
        if self.capacity > 0 && !self.archetype.is_zst() {
            unsafe {
                let layout = self.archetype.layout.repeat(self.capacity).unwrap().0;
                alloc::dealloc(self.data.as_ptr(), layout);
            }
        }
    }
}

/// Get the drop_in_place implementation for any type T
unsafe fn get_drop<T: 'static>() -> DropInPlace {
    std::mem::transmute(std::ptr::drop_in_place::<T> as unsafe fn(*mut T))
}

pub trait IntoArchetype {
    /// Get the archetype of this tuple
    fn into_archetype() -> Archetype;
    /// Check if the archetypes match, this is faster than calling into_archetype and matching over
    /// them.
    fn match_archetype(archetype: &Archetype) -> bool;
    /// Check if an archetype contains at least all the types of this archetype.
    fn archetype_contains(archetype: &Archetype) -> bool;
    fn bitset(mapping: &ArchetypeBitsetMapping) -> Option<ArchetypeBitset>;
    /// Write self to dst, archetypes must match (order independant)
    unsafe fn write(self, dst: *mut u8, archetype: &Archetype);
    /// Read a value from src,, archetypes must match (order independant)
    unsafe fn read(src: *const u8, archetype: &Archetype) -> Self;
    /// Get a vec of the TypeIds of the types composing the archetype
    fn types() -> Vec<TypeId>;
}

// Implement IntoArchetype for generic tuples of length 0 to 16
// see ecs_macros for implementation
#[cfg(not(feature = "extended_limits"))]
impl_archetype!(16);
#[cfg(feature = "extended_limits")]
impl_archetype!(24);

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU8;

    use super::*;

    #[test]
    fn cursed_drop() {
        type D = i32;
        let drop = unsafe { get_drop::<D>() };
        let mut v: D = 32;
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

    #[test]
    fn half_zst() {
        #[derive(Debug, PartialEq, Eq)]
        struct Tag {}

        static DROPPED: AtomicU8 = AtomicU8::new(0);

        impl Drop for Tag {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let mut at = ArchetypeStorage::new::<(Tag, u8)>();
        at.push((Tag {}, 16u8));
        at.push((65u8, Tag {}));
        at.extend(vec![(Tag {}, 0u8), (Tag {}, 5u8)]);
        assert_eq!(at.len(), 4);
        let val = at.take::<(u8, Tag)>(2);
        assert_eq!(val.0, 0);
        assert_eq!(val.1, Tag {});
        assert_eq!(at.len(), 3);
        at.clear(1..(at.len() - 1));
        assert_eq!(at.len(), 2);
        at.remove(0);
        assert_eq!(DROPPED.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn zst() {
        #[derive(Debug, PartialEq, Eq)]
        struct Tag {}

        static DROPPED: AtomicU8 = AtomicU8::new(0);

        impl Drop for Tag {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let mut at = ArchetypeStorage::new::<(Tag, ())>();
        at.push((Tag {}, ()));
        at.push(((), Tag {}));
        let v = vec![(Tag {}, ()), (Tag {}, ())];
        at.extend(v);
        assert_eq!(at.len(), 4);
        let val = at.take::<(Tag, ())>(2);
        assert_eq!(val.0, Tag {});
        assert_eq!(val.1, ());
        assert_eq!(at.len(), 3);
        at.clear(1..(at.len() - 1));
        assert_eq!(at.len(), 2);
        at.remove(0);
        assert_eq!(DROPPED.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
    #[test]
    fn query() {
        macro_rules! eq {
            ($a:expr, $b:expr) => {{
                let a = $a;
                let b = $b;
                assert_eq!(a.is_some(), b.is_some());
                if let Some(a) = a {
                    let b = b.unwrap();
                    assert_eq!(a.0, b.0);
                    assert_eq!(a.1, *b.1);
                    assert_eq!(a.2, b.2.copied());
                    assert_eq!(a.3, b.3);
                }
            }};
        }
        let mut at = ArchetypeStorage::new::<(String, u8, (), i32, bool)>();
        at.push((12u8, 34i32, "str".to_owned(), (), false));
        at.push((25i32, "abc".to_owned(), (), 17u8, true));
        at.push(("bob".to_owned(), (), 99u8, 68i32, false));
        let mut iter = unsafe { at.iter_query::<(&String, &i32, Option<&bool>, Option<&u128>)>(0, None) };

        eq!(Some(("str", 34i32, Some(false), None)), iter.next());
        eq!(Some(("abc", 25i32, Some(true), None)), iter.next());
        eq!(Some(("bob", 68i32, Some(false), None)), iter.next());
        assert_eq!(None, iter.next());

        let iter = unsafe { at.iter_query::<&mut i32>(0, None) };
        for i in iter {
            *i = 69;
        }
        let s = at.as_slice::<(String, u8, (), i32, bool)>();
        assert_eq!(s[0].3, 69);
        assert_eq!(s[1].3, 69);
        assert_eq!(s[2].3, 69);
    }
}
