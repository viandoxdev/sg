use std::{
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU8, Ordering},
};

use parking_lot::Mutex;

use crate::bitset::{BorrowBitset, BorrowKind};

pub struct Borrows {
    ref_count: Vec<AtomicU8>,
    bitset: Mutex<BorrowBitset>,
}

impl Borrows {
    pub fn new() -> Self {
        Self {
            ref_count: Vec::new(),
            bitset: Mutex::new(BorrowBitset::new()),
        }
    }
    pub fn extend(&mut self, len: usize) {
        self.ref_count
            .extend(std::iter::repeat_with(|| AtomicU8::new(0)).take(len))
    }
    pub fn borrow<T>(&self, borrow: BorrowBitset, value: T) -> BorrowGuard<T> {
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
            borrows: Some(self),
            bitset: borrow,
            val: value,
        }
    }
    pub fn release(&self, borrow: BorrowBitset) {
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
    borrows: Option<&'a Borrows>,
}

impl<'a, T> BorrowGuard<'a, T> {
    pub fn dummy(val: T) -> Self {
        Self {
            bitset: BorrowBitset::default(),
            val,
            borrows: None,
        }
    }
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
        if let Some(borrows) = self.borrows {
            borrows.release(self.bitset)
        }
    }
}
