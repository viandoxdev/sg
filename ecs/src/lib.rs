#![feature(alloc_layout_extra)]
#![feature(ptr_metadata)]
// TODO: remove
#![allow(dead_code)]

use std::{
    any::TypeId,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU64, AtomicU8},
};

use archetype::{ArchetypeStorage, IntoArchetype, QueryIter};
use bitset::{ArchetypeBitset, BitsetBuilder, BorrowBitset, BorrowKind};
use parking_lot::Mutex;
use query::Query;
use std::sync::atomic::Ordering;

mod archetype;
mod bitset;
mod query;

struct Borrows {
    ref_count: Vec<AtomicU8>,
    bitset: Mutex<BorrowBitset>,
}

impl Borrows {
    fn new(id: WorldId) -> Self {
        Self {
            ref_count: Vec::new(),
            bitset: Mutex::new(BorrowBitset::new(id)),
        }
    }
    fn extend(&mut self, len: usize) {
        self.ref_count
            .extend(std::iter::repeat_with(|| AtomicU8::new(0)).take(len))
    }
    fn borrow<T>(&self, borrow: BorrowBitset, value: T) -> BorrowGuard<T> {
        if self.bitset.lock().collide(borrow) {
            panic!("Borrow collision");
        }
        for (i, b) in &borrow {
            match b {
                BorrowKind::Mutable => {
                    self.ref_count[i].store(255, Ordering::SeqCst);
                }
                BorrowKind::Imutable => {
                    self.ref_count[i].fetch_add(1, Ordering::SeqCst);
                }
                BorrowKind::None => {}
            }
        }
        self.bitset.lock().merge(borrow);
        BorrowGuard {
            borrows: self,
            bitset: borrow,
            val: value,
        }
    }
    fn release(&self, borrow: BorrowBitset) {
        for (i, b) in &borrow {
            match b {
                BorrowKind::Mutable => {
                    self.ref_count[i].store(0, Ordering::SeqCst);
                    self.bitset.lock().release(i);
                }
                BorrowKind::Imutable => {
                    let old = self.ref_count[i].fetch_sub(1, Ordering::SeqCst);
                    if old == 1 {
                        // now is 0
                        self.bitset.lock().release(i);
                    }
                }
                BorrowKind::None => {}
            }
        }
    }
}

pub struct BorrowGuard<'a, T> {
    val: T,
    bitset: BorrowBitset,
    borrows: &'a Borrows,
}

impl<'a, T> Deref for BorrowGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

impl<'a, T> DerefMut for BorrowGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.val
    }
}

impl<'a, T: Iterator> Iterator for BorrowGuard<'a, T> {
    type Item = <T as Iterator>::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.deref_mut().next()
    }
}

impl<'a, T> Drop for BorrowGuard<'a, T> {
    fn drop(&mut self) {
        self.borrows.release(self.bitset)
    }
}

pub struct World {
    id: WorldId,
    bitset_builder: Mutex<BitsetBuilder>,
    archetypes: Vec<ArchetypeStorage>,
    archetype_bitsets: Vec<ArchetypeBitset>,
    borrows: Borrows,
}

// Can't use chain, so this will be it.
/// An iterator chaining multiple QueryIter, iterators are run in reverse (FIFO)
pub struct QueryIterBundle<Q: Query> {
    iters: Vec<QueryIter<Q>>,
}

impl<Q: Query> QueryIterBundle<Q> {
    fn new() -> Self {
        Self { iters: Vec::new() }
    }
    fn with_capacity(capacity: usize) -> Self {
        Self {
            iters: Vec::with_capacity(capacity),
        }
    }
    fn push(&mut self, iter: QueryIter<Q>) {
        self.iters.push(iter);
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

impl World {
    fn new() -> Self {
        let id = get_id();
        Self {
            id,
            borrows: Borrows::new(id),
            bitset_builder: Mutex::new(BitsetBuilder::new(id)),
            archetypes: Vec::with_capacity(8),
            archetype_bitsets: Vec::with_capacity(8),
        }
    }
    fn register_component_if_needed(&mut self, id: TypeId) {
        let b = self.bitset_builder.get_mut();
        if !b.mapping().contains_key(&id) {
            b.mapping_mut().insert(id, self.borrows.ref_count.len());
            self.borrows.extend(1);
        }
    }
    fn add_archetype<T: IntoArchetype>(&mut self) -> &mut ArchetypeStorage {
        let index = self.archetypes.len();
        self.bitset_builder.get_mut().start_archetype();
        for id in T::types() {
            self.register_component_if_needed(id);
            self.bitset_builder.get_mut().add(id);
        }
        let set = self.bitset_builder.get_mut().build_archetype();
        let ats = ArchetypeStorage::new::<T>();
        self.archetypes.push(ats);
        self.archetype_bitsets.push(set);
        &mut self.archetypes[index]
    }
    pub fn spawn<T: IntoArchetype>(&mut self, enitity: T) {
        match self
            .archetypes
            .iter_mut()
            .find(|a| T::match_archetype(a.archetype()))
        {
            Some(storage) => storage.push(enitity),
            None => self.add_archetype::<T>().push(enitity),
        }
    }
    pub fn query<Q: Query>(&self) -> BorrowGuard<'_, QueryIterBundle<Q>> {
        let set = Q::bitset(&mut self.bitset_builder.lock());
        let requirements = set.required();
        let storages = self
            .archetype_bitsets
            .iter()
            .zip(self.archetypes.iter())
            .filter_map(|(set, storage)| match (*set & requirements).any() {
                true => Some(storage),
                false => None,
            });
        // TODO: use with_capacity
        let mut iter = QueryIterBundle::new();
        for storage in storages {
            iter.push(unsafe { storage.iter_query::<Q>() });
        }
        self.borrows.borrow(set, iter)
    }
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct WorldId(u64);

// return the unique world id
fn get_id() -> WorldId {
    static WORLD_IDS: AtomicU64 = AtomicU64::new(1);
    WorldId(WORLD_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn push() {
        let mut w = World::new();
        w.spawn(("a".to_owned(),));
        let mut iter = w.query::<&String>();
        assert_eq!("a", iter.next().unwrap());
    }
}
