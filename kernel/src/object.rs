//! Minimal object manager: kernel objects and the per-process handle table.
//! Handle values are NT-like: multiples of 4 starting at 4; 0 is invalid.

extern crate alloc;

use alloc::vec::Vec;

/// Handle every user process receives for the console.
pub const CONSOLE_HANDLE: u64 = 4;

#[derive(Clone, PartialEq, Debug)]
pub enum Object {
    Console,
}

#[derive(Default)]
pub struct HandleTable {
    slots: Vec<Option<Object>>,
}

impl HandleTable {
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Stores `obj` in the first free slot and returns its handle value.
    pub fn insert(&mut self, obj: Object) -> u64 {
        let idx = match self.slots.iter().position(Option::is_none) {
            Some(i) => {
                self.slots[i] = Some(obj);
                i
            }
            None => {
                self.slots.push(Some(obj));
                self.slots.len() - 1
            }
        };
        (idx as u64 + 1) * 4
    }

    fn index(&self, handle: u64) -> Option<usize> {
        if handle == 0 || !handle.is_multiple_of(4) {
            return None;
        }
        let idx = usize::try_from(handle / 4 - 1).ok()?;
        (idx < self.slots.len()).then_some(idx)
    }

    pub fn get(&self, handle: u64) -> Option<&Object> {
        self.slots[self.index(handle)?].as_ref()
    }

    /// Closes `handle`; false if it was not a valid open handle.
    pub fn close(&mut self, handle: u64) -> bool {
        match self.index(handle) {
            Some(i) => self.slots[i].take().is_some(),
            None => false,
        }
    }
}
