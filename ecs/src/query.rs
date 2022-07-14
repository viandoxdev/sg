use std::{any::TypeId, marker::PhantomData, ptr::NonNull};

use ecs_macros::impl_query;

use crate::{
    archetype::Archetype,
    bitset::{BitsetBuilder, BorrowBitset, BorrowBitsetBuilder, BorrowBitsetMapping},
};

/// A single query used in a tuple
trait QuerySingle {
    fn match_archetype(archetype: &Archetype) -> bool;
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self;
    #[doc(hidden)]
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder;
    fn r#type() -> TypeId;
}

pub trait Query {
    fn match_archetype(archetype: &Archetype) -> bool;
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self;
    #[doc(hidden)]
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder;
    fn bitset(mapping: &BorrowBitsetMapping) -> Option<BorrowBitset> {
        let builder = BorrowBitsetBuilder::start(mapping);
        Self::add_to_bitset(builder).build()
    }
    fn types() -> Vec<TypeId>;
}

impl<T: 'static> QuerySingle for &T {
    fn match_archetype(archetype: &Archetype) -> bool {
        archetype.has::<T>()
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        unsafe { &*(ptr.add(archetype.offset::<T>()) as *const T) }
    }
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder {
        builder.borrow::<T>()
    }
    fn r#type() -> TypeId {
        TypeId::of::<T>()
    }
}

impl<T: 'static> QuerySingle for &mut T {
    fn match_archetype(archetype: &Archetype) -> bool {
        archetype.has::<T>()
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        unsafe { &mut *(ptr.add(archetype.offset::<T>()) as *mut T) }
    }
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder {
        builder.borrow_mut::<T>()
    }
    fn r#type() -> TypeId {
        TypeId::of::<T>()
    }
}

impl<T: 'static> QuerySingle for Option<&T> {
    fn match_archetype(_archetype: &Archetype) -> bool {
        true
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        if archetype.has::<T>() {
            Some(<&T as QuerySingle>::build(ptr, archetype))
        } else {
            None
        }
    }
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder {
        builder.borrow_optional::<T>()
    }
    fn r#type() -> TypeId {
        TypeId::of::<T>()
    }
}

impl<T: 'static> QuerySingle for Option<&mut T> {
    fn match_archetype(_archetype: &Archetype) -> bool {
        true
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        if archetype.has::<T>() {
            Some(<&mut T as QuerySingle>::build(ptr, archetype))
        } else {
            None
        }
    }
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder {
        builder.borrow_optional_mut::<T>()
    }
    fn r#type() -> TypeId {
        TypeId::of::<T>()
    }
}

impl<T: QuerySingle> Query for T {
    fn match_archetype(archetype: &Archetype) -> bool {
        T::match_archetype(archetype)
    }
    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
        T::build(ptr, archetype)
    }
    fn add_to_bitset(builder: BorrowBitsetBuilder) -> BorrowBitsetBuilder {
        T::add_to_bitset(builder)
    }
    fn types() -> Vec<TypeId> {
        vec![Self::r#type()]
    }
}

#[cfg(not(feature = "extended_limits"))]
impl_query!(16);
#[cfg(feature = "extended_limits")]
impl_query!(24);

/// An iterator that runs a query on a store
///
/// # safety
///
/// This isn't memory safe, this Iterator doesn't borrow the storage at all, and will lead to data
/// races and other fun stuff, it is necessary to manually enforce aliasing rules when using this.
pub struct QueryIter<Q: Query> {
    data: NonNull<u8>,
    length: usize,
    archetype: *const Archetype,
    current: usize,
    _phantom: PhantomData<Q>,
}

impl<Q: Query> QueryIter<Q> {
    pub fn new(data: NonNull<u8>, length: usize, archetype: *const Archetype) -> Self {
        Self {
            data,
            length,
            archetype,
            current: 0,
            _phantom: PhantomData,
        }
    }
}

impl<Q: Query> Iterator for QueryIter<Q> {
    type Item = Q;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.length {
            None
        } else {
            let ptr = unsafe {
                self.data
                    .as_ptr()
                    .add((*self.archetype).size() * self.current)
            };
            self.current += 1;
            Some(Q::build(ptr, unsafe { &*(self.archetype) }))
        }
    }
}

// Can't use chain, so this will be it.
/// An iterator chaining multiple QueryIter, iterators are run in reverse (LIFO)
pub struct QueryIterBundle<Q: Query> {
    iters: Vec<QueryIter<Q>>,
}

impl<Q: Query> QueryIterBundle<Q> {
    pub fn new() -> Self {
        Self { iters: Vec::new() }
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            iters: Vec::with_capacity(capacity),
        }
    }
    pub fn push(&mut self, iter: QueryIter<Q>) {
        self.iters.push(iter);
    }
}

impl<Q: Query> Default for QueryIterBundle<Q> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Q: Query> Iterator for QueryIterBundle<Q> {
    type Item = Q;
    fn next(&mut self) -> Option<Self::Item> {
        match self.iters.last_mut() {
            Some(last) => match last.next() {
                Some(next) => Some(next),
                None => {
                    self.iters.pop();
                    self.next()
                }
            },
            None => None,
        }
    }
}
