use std::{any::TypeId, mem::MaybeUninit};

use crate::{
    archetype::{ArchetypeStorage, IntoArchetype},
    bitset::{ArchetypeBitset, BitsetMapping, BorrowBitset},
    borrows::{BorrowGuard, Borrows},
    entity::{Entity, LocationMap},
    query::{Query, QueryIterBundle},
};

pub struct World {
    mapping: BitsetMapping<TypeId>,
    archetypes: Vec<(ArchetypeStorage, ArchetypeBitset)>,
    borrows: Borrows,
    location_map: LocationMap,
}

// This needs to move, a utils mod maybe ?

trait VecExt<T> {
    unsafe fn get_mut_many_unchecked<const N: usize>(
        &mut self,
        indices: [usize; N],
    ) -> [Option<&mut T>; N];
    fn get_mut_many<const N: usize>(&mut self, indices: [usize; N]) -> [Option<&mut T>; N];
}
impl<T> VecExt<T> for Vec<T>
where
    T: Sized,
{
    unsafe fn get_mut_many_unchecked<const N: usize>(
        &mut self,
        indices: [usize; N],
    ) -> [Option<&mut T>; N] {
        let mut res: MaybeUninit<[Option<&mut T>; N]> = MaybeUninit::uninit();
        let s = self as *mut Self;
        for (i, index) in indices.iter().enumerate().take(N) {
            (res.as_mut_ptr() as *mut Option<&mut T>)
                .add(i)
                .write((*s).get_mut(*index));
        }
        res.assume_init()
    }
    fn get_mut_many<const N: usize>(&mut self, indices: [usize; N]) -> [Option<&mut T>; N] {
        // Its O(n²) but can definitely be unrolled, and/or vectorized by the compiler so it shouldn't matter
        #[allow(clippy::needless_range_loop)]
        for i in 0..N {
            for j in 0..N {
                if i != j && indices[i] == indices[j] {
                    panic!("Trying to borrow mutable the same index more than once at a time (index {i} is the same as index {j}: {})", indices[i])
                }
            }
        }
        unsafe { self.get_mut_many_unchecked(indices) }
    }
}

impl World {
    /// Hello World
    pub fn new() -> Self {
        Self {
            mapping: BitsetMapping::new(),
            borrows: Borrows::new(),
            archetypes: Vec::with_capacity(8),
            location_map: LocationMap::new(),
        }
    }
    fn register_component_if_needed(&mut self, id: TypeId) {
        let mapping = &mut self.mapping;
        if !mapping.has(&id) {
            mapping.map(id);
            self.borrows.extend(1);
        }
    }
    fn add_archetype<T: IntoArchetype>(&mut self) -> &mut ArchetypeStorage {
        let index = self.archetypes.len();
        for t in T::types() {
            self.register_component_if_needed(t);
        }
        let set = T::bitset(&self.mapping).unwrap();
        let ats = ArchetypeStorage::new::<T>();
        self.archetypes.push((ats, set));
        &mut self.archetypes[index].0
    }
    /// Spawn an entity in the world
    pub fn spawn<T: IntoArchetype>(&mut self, entity: T) -> Entity {
        match self
            .archetypes
            .iter_mut()
            .enumerate()
            .find(|(_, (storage, _))| T::match_archetype(storage.archetype()))
        {
            Some((i, (storage, _))) => {
                storage.push(entity);
                self.location_map.add_single(i)
            }
            None => {
                self.add_archetype::<T>().push(entity);
                self.location_map.add_single(self.archetypes.len() - 1)
            }
        }
    }
    /// Spawn many entities in the world
    pub fn spawn_many<T: IntoArchetype>(
        &mut self,
        entities: impl IntoIterator<Item = T>,
    ) -> Vec<Entity> {
        match self
            .archetypes
            .iter_mut()
            .enumerate()
            .find(|(_, (storage, _))| T::match_archetype(storage.archetype()))
        {
            Some((i, (storage, _))) => {
                let mut len = storage.len();
                storage.extend(entities);
                len = storage.len() - len;
                self.location_map.add(i, len)
            }
            None => {
                let storage = self.add_archetype::<T>();
                storage.extend(entities);
                let len = storage.len();
                self.location_map.add(self.archetypes.len() - 1, len)
            }
        }
    }
    /// Delete an entity from the world (calls drop), unlike take, this doesn't need to know the
    /// type of the components of the entity.
    pub fn remove(&mut self, entity: Entity) -> Option<()> {
        let loc = self.location_map.remove_single(entity)?;
        self.archetypes[loc.archetype].0.remove(loc.entity);
        Some(())
    }
    /// Like remove, for multiple entities
    pub fn remove_many(&mut self, entities: impl IntoIterator<Item = Entity>) -> Option<()> {
        let locs = self.location_map.remove(entities)?;
        for loc in locs {
            self.archetypes[loc.archetype].0.remove(loc.entity);
        }
        Some(())
    }
    /// Take an entity away from the world, unlike remove, this returns the entity, but needs to
    /// know the type of its components
    pub fn take<T: IntoArchetype>(&mut self, entity: Entity) -> Option<T> {
        let loc = self.location_map.remove_single(entity)?;
        Some(self.archetypes[loc.archetype].0.take(loc.entity))
    }
    /// Like take, for multiple entities
    pub fn take_many<T: IntoArchetype>(
        &mut self,
        entities: impl IntoIterator<Item = Entity>,
    ) -> Option<Vec<T>> {
        let locs = self.location_map.remove(entities)?;
        let mut res = Vec::with_capacity(locs.len());
        for loc in locs {
            res.push(self.archetypes[loc.archetype].0.take(loc.entity));
        }
        Some(res)
    }
    /// Add a component to an entity, this is very slow (comparatively) and should be avoided
    pub fn add_component<T: IntoArchetype>(&mut self, entity: Entity, value: T) -> Option<()> {
        let loc = self.location_map.get_location(entity)?;
        let archetype_bitset = self.archetypes[loc.archetype].1;
        let mut archetype = self.archetypes[loc.archetype].0.archetype().clone();
        for t in T::types() {
            self.register_component_if_needed(t);
        }
        let t_bitset = T::bitset(&self.mapping).unwrap();
        if (t_bitset & archetype_bitset).any() {
            panic!("Can't add a component to an entity that already has one");
        }
        let set = t_bitset | archetype_bitset;

        let dst_index = match self
            .archetypes
            .iter()
            .enumerate()
            .find(|(_, (_, aset))| set == *aset)
        {
            Some((i, (_, _))) => i,
            None => {
                archetype.merge(T::into_archetype());
                let ats = ArchetypeStorage::new_from_archetype(archetype);
                let i = self.archetypes.len();
                self.archetypes.push((ats, set));
                i
            }
        };
        let [src_storage, dst_storage] = self.archetypes.get_mut_many([loc.archetype, dst_index]);
        let src_storage = &mut src_storage.unwrap().0;
        let dst_storage = &mut dst_storage.unwrap().0;

        unsafe {
            let index = src_storage.move_entity(loc.entity, dst_storage);
            dst_storage.write(index, value);
        }

        self.location_map.move_archetype(entity, dst_index);

        Some(())
    }
    /// Take a component from an entity, this is very slow (comparatively) and should be avoided
    pub fn take_component<T: IntoArchetype>(&mut self, entity: Entity) -> Option<T> {
        let loc = self.location_map.get_location(entity)?;
        let archetype_bitset = self.archetypes[loc.archetype].1;
        let mut archetype = self.archetypes[loc.archetype].0.archetype().clone();

        let t_bitset = T::bitset(&self.mapping).unwrap();
        if t_bitset & archetype_bitset != t_bitset {
            panic!("Can't take a component from an entity that doesn't have one");
        }
        let set = archetype_bitset & !t_bitset;

        let dst_index = match self
            .archetypes
            .iter()
            .enumerate()
            .find(|(_, (_, aset))| set == *aset)
        {
            Some((i, (_, _))) => i,
            None => {
                archetype.subtract(T::into_archetype());
                let ats = ArchetypeStorage::new_from_archetype(archetype);
                let i = self.archetypes.len();
                self.archetypes.push((ats, set));
                i
            }
        };
        let [src_storage, dst_storage] = self.archetypes.get_mut_many([loc.archetype, dst_index]);
        let src_storage = &mut src_storage.unwrap().0;
        let dst_storage = &mut dst_storage.unwrap().0;
        let res;
        unsafe {
            res = src_storage.read(loc.entity);
            src_storage.move_entity(loc.entity, dst_storage);
        }

        self.location_map.move_archetype(entity, dst_index);

        Some(res)
    }
    fn query_iter<Q: Query>(&self, set: BorrowBitset) -> QueryIterBundle<Q> {
        let requirements = set.required();
        let storages = self.archetypes.iter().enumerate().filter_map(|(index, (storage, set))| {
            match *set & requirements == requirements {
                true => Some((index, storage)),
                false => None,
            }
        });
        // TODO: use with_capacity
        let mut iter = QueryIterBundle::new();
        for (index, storage) in storages {
            iter.push(unsafe { storage.iter_query::<Q>(index, Some(&self.location_map)) });
        }
        iter
    }
    /// Run a query on the world, without any borrow checking
    ///
    /// # Safety
    ///
    /// This should only be used when the query has been proven to not alias with any other
    /// existing query.
    pub(crate) unsafe fn query_unchecked<Q: Query>(&self) -> QueryIterBundle<Q> {
        let set = match Q::bitset(&self.mapping) {
            Some(set) => set,
            None => return QueryIterBundle::new(),
        };
        self.query_iter::<Q>(set)
    }
    /// Query the world
    ///
    /// # Panics
    ///
    /// This panics if another existing query collide with this one
    pub fn query<Q: Query>(&self) -> BorrowGuard<'_, QueryIterBundle<Q>> {
        let set = match Q::bitset(&self.mapping) {
            Some(set) => set,
            None => return BorrowGuard::dummy(QueryIterBundle::new()),
        };
        let iter = self.query_iter::<Q>(set);
        self.borrows.borrow(set, iter)
    }
    /// Query a single entity from the world
    pub fn query_single<Q: Query>(&self) -> Option<BorrowGuard<'_, Q>> {
        let set = Q::bitset(&self.mapping)?;
        let mut iter = self.query_iter::<Q>(set);
        iter.next().map(|q| self.borrows.borrow(set, q))
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::{AtomicU64, Ordering}, collections::HashSet};

    use crate::{Entities, Executor};

    use super::*;
    #[test]
    fn push() {
        let mut w = World::new();
        let s = "a".to_owned();
        w.spawn((s,));
        let mut iter = w.query::<&String>();
        assert_eq!("a", iter.next().unwrap());
    }
    #[test]
    #[should_panic]
    fn borrow_collision() {
        let mut w = World::new();
        w.spawn(("a".to_owned(), 34));
        {
            let _b1 = w.query::<&String>();
            let _b2 = w.query::<&mut String>();
        }
    }
    #[test]
    fn borrow_release() {
        let mut w = World::new();
        w.spawn(("a".to_owned(), 34));
        {
            let _b1 = w.query::<&String>();
        }
        {
            let _b2 = w.query::<&mut String>();
        }
    }
    #[test]
    fn multiple_archetypes() {
        let mut w = World::new();
        w.spawn((12, false));
        w.spawn((12, "test"));
        w.spawn((12, ()));
        for i in w.query::<&i32>() {
            assert_eq!(*i, 12);
        }
    }
    #[test]
    fn drop_world() {
        static DROPPED: AtomicU64 = AtomicU64::new(0);
        struct S;
        impl Drop for S {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, Ordering::SeqCst);
            }
        }
        let mut w = World::new();
        w.spawn((12, false, S));
        w.spawn((12, "test", S));
        w.spawn((12, (), S));
        drop(w);
        assert_eq!(3, DROPPED.load(Ordering::SeqCst));
    }
    #[test]
    fn remove_component() {
        let mut w = World::new();
        let e = w.spawn((24, true));
        assert_eq!(true, **w.query_single::<&bool>().unwrap());
        w.take_component::<(bool,)>(e);
        assert!(w.query_single::<&bool>().is_none());
        assert_eq!(24, **w.query_single::<&i32>().unwrap());
    }
    #[test]
    fn add_component() {
        let mut w = World::new();
        let e = w.spawn((24,));
        assert!(w.query_single::<&bool>().is_none());
        w.add_component(e, (true,));
        assert_eq!(true, **w.query_single::<&bool>().unwrap());
        assert_eq!(24, **w.query_single::<&i32>().unwrap());
    }
    #[test]
    fn query_id() {
        let mut w = World::new();
        let mut e = Executor::new();
        let entities_id = std::iter::repeat_with(|| w.spawn((0,)))
            .take(10)
            .collect::<HashSet<Entity>>();

        let sys = move |entities: Entities<Entity>| {
            let entities = entities.collect::<Vec<_>>();
            assert_eq!(entities.len(), entities_id.len());
            for entity in entities {
                assert!(entities_id.contains(&entity));
            }
        };
        let s = e.schedule().then(sys)
            .build();
        e.execute(&s, &mut w);
    }
}
