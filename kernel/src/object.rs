//! Minimal object manager: kernel objects and the per-process handle table.
//! Handle values are NT-like: multiples of 4 starting at 4; 0 is invalid.

extern crate alloc;

use alloc::vec::Vec;

/// Handle every user process receives for the console.
pub const CONSOLE_HANDLE: u64 = 4;

/// Handle every user process receives for console (keyboard) input; the second
/// slot of the table, so its value is 8.
pub const CONSOLE_INPUT_HANDLE: u64 = 8;

#[derive(Clone, PartialEq, Debug)]
pub enum Object {
    Console,
    ConsoleInput,
    /// Read-only initrd file with its own (per-handle) read offset.
    File {
        content: &'static [u8],
        offset: usize,
    },
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

    fn file_mut(&mut self, handle: u64) -> Option<(&'static [u8], &mut usize)> {
        let i = self.index(handle)?;
        match self.slots[i].as_mut()? {
            Object::File { content, offset } => Some((*content, offset)),
            _ => None,
        }
    }

    /// The bytes a read of up to `want` bytes would return from a file handle
    /// (empty at EOF); `None` if `handle` is not a file. Does not move the offset.
    pub fn file_peek(&mut self, handle: u64, want: usize) -> Option<&'static [u8]> {
        let (content, offset) = self.file_mut(handle)?;
        let start = (*offset).min(content.len());
        let n = want.min(content.len() - start);
        content.get(start..start.checked_add(n)?)
    }

    /// Advances the file offset by `n` (clamped to the file length).
    pub fn file_commit(&mut self, handle: u64, n: usize) {
        if let Some((content, offset)) = self.file_mut(handle) {
            *offset = offset.saturating_add(n).min(content.len());
        }
    }

    /// Total size of a file handle.
    pub fn file_size(&mut self, handle: u64) -> Option<u64> {
        self.file_mut(handle).map(|(c, _)| c.len() as u64)
    }

    /// SetFilePointerEx semantics. `method`: 0 = BEGIN, 1 = CURRENT, 2 = END.
    /// A negative resulting position is STATUS_INVALID_PARAMETER; a position
    /// past EOF is clamped to the file length (reads there give EOF anyway).
    /// Returns the new offset; STATUS_INVALID_HANDLE if not a file handle.
    pub fn file_seek(&mut self, handle: u64, distance: i64, method: u32) -> Result<u64, u64> {
        let Some((content, offset)) = self.file_mut(handle) else {
            return Err(0xC000_0008);
        };
        let base: i128 = match method {
            0 => 0,
            1 => *offset as i128,
            2 => content.len() as i128,
            _ => return Err(0xC000_000D),
        };
        let target = base + i128::from(distance); // cannot overflow i128
        if target < 0 {
            return Err(0xC000_000D);
        }
        let new = target.min(content.len() as i128) as usize;
        *offset = new;
        Ok(new as u64)
    }
}
