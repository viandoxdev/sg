use std::any::TypeId;

use parking_lot::Mutex;

use crate::{
    archetype::{ArchetypeStorage, IntoArchetype},
    bitset::{ArchetypeBitset, BitsetBuilder, BorrowBitset},
    borrows::{BorrowGuard, Borrows},
    entity::{Entity, LocationMap},
    query::{Query, QueryIterBundle},
};

pub struct World {
    bitset_builder: Mutex<BitsetBuilder>,
    archetypes: Vec<(ArchetypeStorage, ArchetypeBitset)>,
    borrows: Borrows,
    location_map: LocationMap,
}

impl World {
    fn new() -> Self {
        Self {
            borrows: Borrows::new(),
            bitset_builder: Mutex::new(BitsetBuilder::new()),
            archetypes: Vec::with_capacity(8),
            location_map: LocationMap::new(),
        }
    }
    fn register_component_if_needed(&mut self, id: TypeId) {
        let b = self.bitset_builder.get_mut();
        if !b.mapping().contains_key(&id) {
            let new_index = b.mapping().len();
            b.mapping_mut().insert(id, new_index);
            self.borrows.extend(1);
        }
    }
    fn add_archetype<T: IntoArchetype>(&mut self) -> &mut ArchetypeStorage {
        let index = self.archetypes.len();
        for t in T::types() {
            self.register_component_if_needed(t);
        }
        let set = T::bitset(&mut self.bitset_builder.lock()).unwrap();
        let ats = ArchetypeStorage::new::<T>();
        self.archetypes.push((ats, set));
        &mut self.archetypes[index].0
    }
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
    pub fn remove(&mut self, entity: Entity) -> Option<()> {
        let loc = self.location_map.remove_single(entity)?;
        self.archetypes[loc.archetype].0.remove(loc.entity);
        Some(())
    }
    pub fn remove_many(&mut self, entities: impl IntoIterator<Item = Entity>) -> Option<()> {
        let locs = self.location_map.remove(entities)?;
        for loc in locs {
            self.archetypes[loc.archetype].0.remove(loc.entity);
        }
        Some(())
    }
    pub fn take<T: IntoArchetype>(&mut self, entity: Entity) -> Option<T> {
        let loc = self.location_map.remove_single(entity)?;
        Some(self.archetypes[loc.archetype].0.take(loc.entity))
    }
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
    pub fn add_component<T: IntoArchetype>(&mut self, entity: Entity, value: T) -> Option<()> {
        let loc = *self.location_map.get(entity)?;
        let archetype_bitset = self.archetypes[loc.archetype].1;
        let mut archetype = self.archetypes[loc.archetype].0.archetype().clone();
        for t in T::types() {
            self.register_component_if_needed(t);
        }
        let t_bitset = T::bitset(&mut self.bitset_builder.lock()).unwrap();
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
        // Safety: i and loc.archetype are never the same so this is just borrowing two different
        // values
        let (src_storage, dst_storage): (&mut _, &mut _) = unsafe {
            let r = (&mut self.archetypes) as *mut Vec<(ArchetypeStorage, ArchetypeBitset)>;
            (&mut (&mut *r)[loc.archetype].0, &mut (&mut *r)[dst_index].0)
        };

        unsafe {
            let index = src_storage.move_entity(loc.entity, dst_storage);
            dst_storage.write(index, value);
        }

        self.location_map.move_archetype(entity, dst_index);

        Some(())
    }
    pub fn take_component<T: IntoArchetype>(&mut self, entity: Entity) -> Option<T> {
        let loc = *self.location_map.get(entity)?;
        let archetype_bitset = self.archetypes[loc.archetype].1;
        let mut archetype = self.archetypes[loc.archetype].0.archetype().clone();

        let t_bitset = T::bitset(&mut self.bitset_builder.lock()).unwrap();
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
        // Safety: i and loc.archetype are never the same so this is just borrowing two different
        // values
        let (src_storage, dst_storage): (&mut _, &mut _) = unsafe {
            let r = (&mut self.archetypes) as *mut Vec<(ArchetypeStorage, ArchetypeBitset)>;
            (&mut (&mut *r)[loc.archetype].0, &mut (&mut *r)[dst_index].0)
        };
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
        let storages =
            self.archetypes
                .iter()
                .filter_map(|(storage, set)| match (*set & requirements).any() {
                    true => Some(storage),
                    false => None,
                });
        // TODO: use with_capacity
        let mut iter = QueryIterBundle::new();
        for storage in storages {
            iter.push(unsafe { storage.iter_query::<Q>() });
        }
        iter
    }
    pub fn query<Q: Query>(&self) -> BorrowGuard<'_, QueryIterBundle<Q>> {
        let set = match Q::bitset(&mut self.bitset_builder.lock()) {
            Some(set) => set,
            None => return BorrowGuard::dummy(QueryIterBundle::new()),
        };
        let iter = self.query_iter::<Q>(set);
        self.borrows.borrow(set, iter)
    }
    pub fn query_single<Q: Query>(&self) -> Option<BorrowGuard<'_, Q>> {
        let set = Q::bitset(&mut self.bitset_builder.lock())?;
        let mut iter = self.query_iter::<Q>(set);
        iter.next().map(|q| self.borrows.borrow(set, q))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    #[test]
    fn push() {
        let mut w = World::new();
        w.spawn(("a".to_owned(),));
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
}
