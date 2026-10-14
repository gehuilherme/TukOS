//! A per-process address space: a fresh PML4 whose higher half (kernel) is
//! cloned from the active space and whose lower half (user) starts empty.

use crate::mem::frames::{alloc_frame, free_frame, phys_to_virt};
use crate::mem::paging::{MapError, map_page_in, translate_in, unmap_page_in};
use mm::paging::{PageFlags, entry_frame, entry_present};

/// Entries 0..256 of a PML4 cover the lower (user) half; 256..512 the higher
/// (kernel) half.
const HALF: usize = 256;

pub struct AddressSpace {
    root: u64,
}

fn table(phys: u64) -> *mut u64 {
    phys_to_virt(phys) as *mut u64
}

impl AddressSpace {
    pub fn new_user() -> AddressSpace {
        let root = alloc_frame().expect("sem frame para a PML4");
        let dst = table(root);
        let src = table(crate::arch_cr3_base());
        unsafe {
            // Lower half: empty. Higher half: clone the kernel's PML4 entries.
            for i in 0..HALF {
                dst.add(i).write(0);
            }
            // INVARIANT: the higher half is cloned by value at the PML4 level, so a
            // NEW top-level kernel PML4 entry created after any AddressSpace exists
            // would NOT propagate. All kernel higher-half regions must be mapped at
            // boot, before any user space is forked.
            for i in HALF..512 {
                dst.add(i).write(core::ptr::read_volatile(src.add(i)));
            }
        }
        AddressSpace { root }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    /// # Safety
    /// `virt` must be a page-aligned lower-half (user) address.
    pub unsafe fn map(&self, virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError> {
        unsafe { map_page_in(self.root, virt, phys, flags) }
    }

    /// # Safety
    /// `virt` must be a mapped lower-half address.
    #[allow(dead_code)] // used by later process tasks
    pub unsafe fn unmap(&self, virt: u64) -> Result<(), MapError> {
        unsafe { unmap_page_in(self.root, virt) }
    }

    #[allow(dead_code)] // used by later process tasks
    pub fn translate(&self, virt: u64) -> Option<u64> {
        translate_in(self.root, virt)
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        // Walk only the lower half (user), freeing leaf frames and the
        // intermediate tables, then the PML4 frame itself. The higher half is
        // shared with the kernel and must never be freed here.
        unsafe { free_lower_half(self.root) };
        free_frame(self.root);
    }
}

/// Recursively frees the lower-half (indices 0..256 at the top level) subtree.
unsafe fn free_lower_half(pml4: u64) {
    let p4 = table(pml4);
    for i in 0..HALF {
        let e4 = unsafe { core::ptr::read_volatile(p4.add(i)) };
        if !entry_present(e4) {
            continue;
        }
        unsafe { free_table(entry_frame(e4), 3) };
    }
}

/// Frees a page-table subtree rooted at `phys` of the given `level`
/// (3 = PDPT, 2 = PD, 1 = PT). At level 1 the entries are leaf frames.
unsafe fn free_table(phys: u64, level: u8) {
    let t = table(phys);
    for i in 0..512 {
        let e = unsafe { core::ptr::read_volatile(t.add(i)) };
        if !entry_present(e) {
            continue;
        }
        let child = entry_frame(e);
        if level == 1 {
            free_frame(child); // leaf user page
        } else {
            unsafe { free_table(child, level - 1) };
        }
    }
    free_frame(phys); // the table itself
}
