use std::collections::HashMap;

use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct Entity;
}

#[derive(Eq, Hash, PartialEq, Clone, Copy, Debug)]
pub struct Location {
    pub archetype: usize,
    pub entity: usize,
}

pub struct LocationMap {
    entities: SlotMap<Entity, Location>,
    locations: HashMap<Location, Entity>,
    lengths: Vec<usize>,
}

impl LocationMap {
    pub fn new() -> Self {
        Self {
            entities: SlotMap::with_key(),
            locations: HashMap::new(),
            lengths: Vec::new(),
        }
    }
    fn fetch_add_archetype_len(&mut self, archetype: usize, add: usize) -> usize {
        match self.lengths.get_mut(archetype) {
            Some(len) => {
                let res = *len;
                *len += add;
                res
            }
            None => {
                if archetype >= self.lengths.len() {
                    self.lengths
                        .extend(std::iter::repeat(0).take(archetype - self.lengths.len() + 1));
                }
                self.lengths[archetype] = add;
                0
            }
        }
    }
    /// Shift the elements of an archetype, typically after a remove
    fn shift(&mut self, count: usize, from: usize, archetype: usize) {
        let len = &mut self.lengths[archetype];
        for i in from..*len {
            let loc = Location {
                archetype,
                entity: i,
            };
            let new_loc = Location {
                archetype,
                entity: i - count,
            };
            let e = self.locations.remove(&loc).unwrap();
            self.entities[e].entity -= count;
            self.locations.insert(new_loc, e);
        }
        *len -= count;
    }
    pub fn move_archetype(&mut self, entity: Entity, archetype: usize) {
        let index = self.fetch_add_archetype_len(archetype, 1);
        let location = Location {
            archetype,
            entity: index,
        };

        let old_loc = self.entities[entity];
        self.entities[entity] = location;
        self.locations.remove(&old_loc).unwrap();
        self.shift(1, old_loc.entity + 1, old_loc.archetype);
    }
    pub fn add_single(&mut self, archetype: usize) -> Entity {
        let index = self.fetch_add_archetype_len(archetype, 1);
        let location = Location {
            archetype,
            entity: index,
        };
        let entity = self.entities.insert(location);
        self.locations.insert(location, entity);
        entity
    }
    pub fn add(&mut self, archetype: usize, count: usize) -> Vec<Entity> {
        let mut res = Vec::with_capacity(count);
        let start = self.fetch_add_archetype_len(archetype, count);
        for i in 0..count {
            let location = Location {
                archetype,
                entity: i + start,
            };
            let entity = self.entities.insert(location);
            self.locations.insert(location, entity);
            res.push(entity);
        }
        res
    }
    pub fn remove_single(&mut self, entity: Entity) -> Option<Location> {
        let loc = self.entities.remove(entity)?;
        self.locations.remove(&loc)?;
        self.shift(1, loc.entity + 1, loc.archetype);
        Some(loc)
    }
    pub fn remove(&mut self, entities: impl IntoIterator<Item = Entity>) -> Option<Vec<Location>> {
        let mut res = Vec::new();
        for e in entities {
            let loc = self.entities.remove(e)?;
            self.locations.remove(&loc)?;
            res.push(loc);
        }
        let archetype;
        let index;
        let count;
        if res.is_empty() {
            return Some(res);
        } else {
            archetype = res[0].archetype;
            index = res[0].entity;
            count = res.len();
        }
        self.shift(count, index + 1, archetype);
        Some(res)
    }
    pub fn get(&self, entity: Entity) -> Option<&Location> {
        self.entities.get(entity)
    }
}
