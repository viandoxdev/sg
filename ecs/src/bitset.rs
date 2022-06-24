use std::ops::{self, Deref};
use std::{any::TypeId, collections::HashMap};

use crate::WorldId;

/// What are bitsets composed of
type BitsetComp = u64;

#[derive(Clone, Copy, Default, Debug, Hash)]
pub struct Bitset {
    bits: [BitsetComp; Self::LENGTH],
}

pub struct BitsetIter {
    bits: [BitsetComp; Bitset::LENGTH],
    current: usize,
    length: usize,
}

impl Iterator for BitsetIter {
    type Item = bool;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.length {
            None
        } else {
            let res = (self.bits[self.current / Bitset::COMP_BITS]
                >> (self.current % Bitset::COMP_BITS))
                & 1;
            self.current += 1;
            Some(res != 0)
        }
    }
}

impl Bitset {
    const COMP_BITS: usize = std::mem::size_of::<BitsetComp>() * 8;
    /// How many BitsetComp compose a bitset, this decides the maximum number of types of component in a
    /// world (Self::BITS)
    const LENGTH: usize = 1;
    const BITS: usize = Self::COMP_BITS * Self::LENGTH;

    pub fn new_with_bit(index: usize) -> Self {
        if index >= Self::BITS {
            panic!("Trying to set a bit the bitset can't store");
        }

        let mut bits = [0; Self::LENGTH];
        // COMP_BITS is constant and a power of 2, so this should all get optimized to
        // bitshifts and masks
        bits[index / Self::COMP_BITS] = 1 << (index % Self::COMP_BITS);
        Self { bits }
    }
    pub fn len(&self) -> usize {
        // Go in reverse, looking for the last non zero component
        for i in (0..Self::LENGTH).rev() {
            if self.bits[i] != 0 {
                let msb = Self::COMP_BITS - 1 - self.bits[i].trailing_zeros() as usize;
                return i * Self::COMP_BITS + msb;
            }
        }
        0
    }
    pub fn iter(&self) -> BitsetIter {
        self.into_iter()
    }
    pub fn new_with_all() -> Self {
        Self {
            bits: [!0; Self::LENGTH],
        }
    }
    /// Checks if any bit is set
    pub fn any(&self) -> bool {
        for bits in self.bits {
            if bits > 0 {
                return true;
            }
        }
        false
    }
}

impl IntoIterator for Bitset {
    type Item = bool;
    type IntoIter = BitsetIter;
    fn into_iter(self) -> Self::IntoIter {
        BitsetIter {
            bits: self.bits,
            current: 0,
            length: self.len(),
        }
    }
}

impl ops::BitOr for Bitset {
    type Output = Self;
    fn bitor(mut self, rhs: Self) -> Self::Output {
        for i in 0..Self::LENGTH {
            self.bits[i] |= rhs.bits[i];
        }
        self
    }
}

impl ops::BitAnd for Bitset {
    type Output = Self;
    fn bitand(mut self, rhs: Self) -> Self::Output {
        for i in 0..Self::LENGTH {
            self.bits[i] &= rhs.bits[i];
        }
        self
    }
}

impl ops::BitXor for Bitset {
    type Output = Self;
    fn bitxor(mut self, rhs: Self) -> Self::Output {
        for i in 0..Self::LENGTH {
            self.bits[i] ^= rhs.bits[i];
        }
        self
    }
}

impl ops::Not for Bitset {
    type Output = Self;
    fn not(mut self) -> Self::Output {
        for i in 0..Self::LENGTH {
            self.bits[i] = !self.bits[i]
        }
        self
    }
}

impl ops::BitOrAssign for Bitset {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl ops::BitAndAssign for Bitset {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = *self & rhs;
    }
}

impl ops::BitXorAssign for Bitset {
    fn bitxor_assign(&mut self, rhs: Self) {
        *self = *self ^ rhs;
    }
}

pub struct BitsetBuilder {
    /// Id of the world this builder refers to, as the mapping from component type to index of bit
    /// depends on the world
    id: WorldId,
    borrow: Bitset,
    mutable: Bitset,
    required: Bitset,
    mapping: HashMap<TypeId, usize>,
}

impl BitsetBuilder {
    pub fn new(id: WorldId) -> Self {
        Self {
            id,
            mapping: HashMap::with_capacity(8),
            borrow: Bitset::default(),
            mutable: Bitset::default(),
            required: Bitset::default(),
        }
    }
    pub fn mapping(&self) -> &HashMap<TypeId, usize> {
        &self.mapping
    }
    pub fn mapping_mut(&mut self) -> &mut HashMap<TypeId, usize> {
        &mut self.mapping
    }
    pub fn start_borrow(&mut self) -> &mut Self {
        self.borrow = Bitset::default();
        self.mutable = Bitset::default();
        self.required = Bitset::default();
        self
    }
    pub fn start_archetype(&mut self) -> &mut Self {
        self.required = Bitset::default();
        self
    }
    pub fn borrow<T: 'static>(&mut self) -> &mut Self {
        let set = Bitset::new_with_bit(self.mapping[&TypeId::of::<T>()]);
        self.borrow |= set;
        self.required |= set;
        self
    }
    pub fn borrow_mut<T: 'static>(&mut self) -> &mut Self {
        let set = Bitset::new_with_bit(self.mapping[&TypeId::of::<T>()]);
        self.borrow |= set;
        self.mutable |= set;
        self.required |= set;
        self
    }
    pub fn borrow_optional<T: 'static>(&mut self) -> &mut Self {
        let set = Bitset::new_with_bit(self.mapping[&TypeId::of::<T>()]);
        self.borrow |= set;
        self
    }
    pub fn borrow_optional_mut<T: 'static>(&mut self) -> &mut Self {
        let set = Bitset::new_with_bit(self.mapping[&TypeId::of::<T>()]);
        self.borrow |= set;
        self.mutable |= set;
        self
    }
    pub fn add(&mut self, id: TypeId) -> &mut Self {
        self.required |= Bitset::new_with_bit(self.mapping[&id]);
        self
    }
    pub fn with(&mut self, set: BorrowBitset) -> &mut Self {
        self.borrow |= set.borrow;
        self.mutable |= set.mutable;
        self.required |= set.required;
        self
    }
    pub fn build_archetype(&self) -> ArchetypeBitset {
        ArchetypeBitset {
            world_id: self.id,
            types: self.required,
        }
    }
    pub fn build_borrow(&self) -> BorrowBitset {
        BorrowBitset {
            world_id: self.id,
            borrow: self.borrow,
            mutable: self.mutable,
            required: self.required,
        }
    }
}

#[derive(Hash, Default, Clone, Copy)]
pub struct ArchetypeBitset {
    world_id: WorldId,
    types: Bitset,
}

impl ops::BitOr for ArchetypeBitset {
    type Output = ArchetypeBitset;
    fn bitor(mut self, rhs: Self) -> Self::Output {
        self.types |= rhs.types;
        self
    }
}

impl ops::BitAnd for ArchetypeBitset {
    type Output = ArchetypeBitset;
    fn bitand(mut self, rhs: Self) -> Self::Output {
        self.types &= rhs.types;
        self
    }
}

impl ops::BitXor for ArchetypeBitset {
    type Output = ArchetypeBitset;
    fn bitxor(mut self, rhs: Self) -> Self::Output {
        self.types ^= rhs.types;
        self
    }
}

impl Deref for ArchetypeBitset {
    type Target = Bitset;
    fn deref(&self) -> &Self::Target {
        &self.types
    }
}

#[derive(Hash, Default, Clone, Copy)]
pub struct BorrowBitset {
    /// Id of the world this bitset refers to, as the mapping from component type to index of bit
    /// depends on the world
    world_id: WorldId,
    borrow: Bitset,
    mutable: Bitset,
    required: Bitset,
}

impl BorrowBitset {
    pub fn new(id: WorldId) -> Self {
        Self {
            world_id: id,
            ..Default::default()
        }
    }
    /// Set all the borrowed types as optional
    pub fn optional(mut self) -> Self {
        self.required = Bitset::default();
        self
    }
    /// Tests wether or the borrow of self would break aliasing rules with another borrow
    pub fn collide(self, borrow: Self) -> bool {
        ((self.mutable & borrow.borrow) | (borrow.mutable & self.mutable)).any()
    }
    pub fn required(self) -> ArchetypeBitset {
        ArchetypeBitset {
            world_id: self.world_id,
            types: self.required,
        }
    }
    pub fn iter(&self) -> BorrowBitsetIter {
        BorrowBitsetIter {
            borrow: self.borrow.iter(),
            mutable: self.mutable.iter(),
            current: 0,
        }
    }
    /// Apply the borrow on self, should only be called if the borrows don't collide
    pub fn merge(&mut self, other: BorrowBitset) {
        self.mutable |= other.mutable;
        self.borrow |= other.borrow;
        self.required |= other.required;
    }
    /// remove the borrow at index
    pub fn release(&mut self, index: usize) {
        self.mutable &= !Bitset::new_with_bit(index);
        self.borrow &= !Bitset::new_with_bit(index);
        self.required &= !Bitset::new_with_bit(index);
    }
}

impl IntoIterator for &BorrowBitset {
    type Item = (usize, BorrowKind);
    type IntoIter = BorrowBitsetIter;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub enum BorrowKind {
    Mutable,
    Imutable,
    None,
}

pub struct BorrowBitsetIter {
    borrow: BitsetIter,
    mutable: BitsetIter,
    current: usize,
}

impl Iterator for BorrowBitsetIter {
    type Item = (usize, BorrowKind);
    fn next(&mut self) -> Option<Self::Item> {
        match self.borrow.next() {
            Some(b) => {
                let m = self.mutable.next().unwrap_or(false);
                let i = self.current;
                self.current += 1;
                match b {
                    true if m => Some((i, BorrowKind::Mutable)),
                    true => Some((i, BorrowKind::Imutable)),
                    false => Some((i, BorrowKind::None)),
                }
            }
            None => None,
        }
    }
}
