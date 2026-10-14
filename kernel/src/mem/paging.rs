//! 4-level page mapping over the currently active address space (the one Limine
//! set up). Intermediate tables are reached through the HHDM and allocated from
//! the global frame allocator when missing.

use crate::mem::frames::{alloc_frame, phys_to_virt};
use mm::paging::{
    PageFlags, entry_flags, entry_frame, entry_present, make_entry, page_table_indices,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    AllocFailed,
    AlreadyMapped,
    NotMapped,
    HugePage,
}

fn table_mut(phys: u64) -> *mut u64 {
    phys_to_virt(phys) as *mut u64
}

/// Ensures every higher-half PML4 entry (256..512) of the active space exists,
/// so later kernel mappings only touch lower-level tables (shared by pointer
/// across all address spaces) rather than creating a new top-level entry that
/// would not propagate to already-forked user spaces.
pub fn reserve_kernel_higher_half() {
    let pml4 = table_mut(crate::arch_cr3_base());
    for i in 256..512 {
        let slot = unsafe { pml4.add(i) };
        let e = unsafe { core::ptr::read_volatile(slot) };
        if !entry_present(e) {
            let frame = alloc_frame().expect("sem frame p/ PDPT do kernel");
            unsafe {
                core::ptr::write_bytes(table_mut(frame) as *mut u8, 0, 4096);
                // Present + writable, NOT user: kernel-only higher half.
                core::ptr::write_volatile(
                    slot,
                    make_entry(frame, PageFlags::PRESENT.union(PageFlags::WRITABLE)),
                );
            }
        }
    }
}

/// Returns the physical address of the next-level table for `index`, creating it
/// if absent (when `create`). Fails on a huge-page entry.
unsafe fn next_table(table: *mut u64, index: usize, create: bool) -> Result<Option<u64>, MapError> {
    let slot = unsafe { table.add(index) };
    let entry = unsafe { core::ptr::read_volatile(slot) };
    if entry_present(entry) {
        if entry_flags(entry).contains(PageFlags::HUGE) {
            return Err(MapError::HugePage);
        }
        return Ok(Some(entry_frame(entry)));
    }
    if !create {
        return Ok(None);
    }
    let frame = alloc_frame().ok_or(MapError::AllocFailed)?;
    // Zero the new table through the HHDM.
    unsafe { core::ptr::write_bytes(table_mut(frame) as *mut u8, 0, 4096) };
    let flags = PageFlags::PRESENT
        .union(PageFlags::WRITABLE)
        .union(PageFlags::USER);
    unsafe { core::ptr::write_volatile(slot, make_entry(frame, flags)) };
    Ok(Some(frame))
}

/// # Safety
/// Modifies the page tables rooted at `root`; `virt` must be a page-aligned address
/// not in active use by the kernel, and `phys` a valid frame.
pub unsafe fn map_page_in(
    root: u64,
    virt: u64,
    phys: u64,
    flags: PageFlags,
) -> Result<(), MapError> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(root);
    for &i in &idx[..3] {
        let next = unsafe { next_table(table, i, true)?.ok_or(MapError::AllocFailed)? };
        table = table_mut(next);
    }
    let slot = unsafe { table.add(idx[3]) };
    if entry_present(unsafe { core::ptr::read_volatile(slot) }) {
        return Err(MapError::AlreadyMapped);
    }
    unsafe {
        core::ptr::write_volatile(slot, make_entry(phys, flags.union(PageFlags::PRESENT)));
        // Only the active space can have TLB entries.
        if root == crate::arch_cr3_base() {
            arch_x86_64::cpu::invlpg(virt);
        }
    }
    Ok(())
}

/// # Safety
/// Modifies the page tables rooted at `root`; `virt` must be a mapped page-aligned address.
pub unsafe fn unmap_page_in(root: u64, virt: u64) -> Result<(), MapError> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(root);
    for &i in &idx[..3] {
        match unsafe { next_table(table, i, false)? } {
            Some(next) => table = table_mut(next),
            None => return Err(MapError::NotMapped),
        }
    }
    let slot = unsafe { table.add(idx[3]) };
    if !entry_present(unsafe { core::ptr::read_volatile(slot) }) {
        return Err(MapError::NotMapped);
    }
    unsafe {
        core::ptr::write_volatile(slot, 0);
        if root == crate::arch_cr3_base() {
            arch_x86_64::cpu::invlpg(virt);
        }
    }
    Ok(())
}

pub fn translate_in(root: u64, virt: u64) -> Option<u64> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(root);
    for &i in &idx[..3] {
        let entry = unsafe { core::ptr::read_volatile(table.add(i)) };
        if !entry_present(entry) {
            return None;
        }
        table = table_mut(entry_frame(entry));
    }
    let entry = unsafe { core::ptr::read_volatile(table.add(idx[3])) };
    if entry_present(entry) {
        Some(entry_frame(entry) + mm::paging::page_offset(virt))
    } else {
        None
    }
}

/// Like [`translate_in`] but for user-copy validation: every level (and the
/// leaf) must carry the USER bit, and a HUGE entry at an intermediate level is
/// rejected (large user pages are unsupported).
pub fn translate_user_in(root: u64, virt: u64) -> Option<u64> {
    translate_user_flags_in(root, virt, false)
}

/// Like [`translate_user_in`] but the leaf must also be WRITABLE (user-copy
/// write validation: a read-only page yields `None`).
pub fn translate_user_writable_in(root: u64, virt: u64) -> Option<u64> {
    translate_user_flags_in(root, virt, true)
}

fn translate_user_flags_in(root: u64, virt: u64, need_write: bool) -> Option<u64> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(root);
    for &i in &idx[..3] {
        let entry = unsafe { core::ptr::read_volatile(table.add(i)) };
        if !entry_present(entry) {
            return None;
        }
        let f = entry_flags(entry);
        if f.contains(PageFlags::HUGE) || !f.contains(PageFlags::USER) {
            return None;
        }
        table = table_mut(entry_frame(entry));
    }
    let entry = unsafe { core::ptr::read_volatile(table.add(idx[3])) };
    if !entry_present(entry) {
        return None;
    }
    let f = entry_flags(entry);
    if f.contains(PageFlags::USER) && (!need_write || f.contains(PageFlags::WRITABLE)) {
        Some(entry_frame(entry) + mm::paging::page_offset(virt))
    } else {
        None
    }
}

///ktest helper: marks the PD entry covering a mapped USER page as HUGE and
/// checks `translate_user_in` refuses it, then restores the entry.
#[cfg(feature = "ktest")]
pub fn test_huge_rejected() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    const VA: u64 = 0x30_0000;
    let space = AddressSpace::new_user();
    let frame = alloc_frame().ok_or("sem frame")?;
    unsafe {
        space
            .map(VA, frame, PageFlags::USER.union(PageFlags::WRITABLE))
            .map_err(|_| "map")?;
    }
    if translate_user_in(space.root(), VA).is_none() {
        return Err("pagina USER normal deveria ser aceita");
    }
    // Walk PML4 -> PDPT -> PD and locate the PD entry (level 2) for VA.
    let idx = page_table_indices(VA);
    let mut table = table_mut(space.root());
    for &i in &idx[..2] {
        let e = unsafe { core::ptr::read_volatile(table.add(i)) };
        table = table_mut(entry_frame(e));
    }
    let slot = unsafe { table.add(idx[2]) };
    let orig = unsafe { core::ptr::read_volatile(slot) };
    unsafe { core::ptr::write_volatile(slot, orig | (1 << 7)) };
    let rejected = translate_user_in(space.root(), VA).is_none();
    unsafe { core::ptr::write_volatile(slot, orig) };
    if !rejected {
        return Err("entrada HUGE foi aceita");
    }
    if translate_user_in(space.root(), VA).is_none() {
        return Err("restauracao falhou");
    }
    Ok(())
}

/// # Safety
/// Modifies the active page tables; see [`map_page_in`].
pub unsafe fn map_page(virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError> {
    unsafe { map_page_in(crate::arch_cr3_base(), virt, phys, flags) }
}

/// # Safety
/// Modifies the active page tables; see [`unmap_page_in`].
pub unsafe fn unmap_page(virt: u64) -> Result<(), MapError> {
    unsafe { unmap_page_in(crate::arch_cr3_base(), virt) }
}

pub fn translate(virt: u64) -> Option<u64> {
    translate_in(crate::arch_cr3_base(), virt)
}
