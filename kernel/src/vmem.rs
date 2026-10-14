//! User virtual-memory allocation backing NtAllocateVirtualMemory /
//! NtFreeVirtualMemory. All range validation happens here, BEFORE any page is
//! mapped, so a rejected request never leaves a partial mapping behind.

use crate::mem::frames::{alloc_frame, free_frame, phys_to_virt};
use crate::mem::paging::{map_page_in, translate_in, translate_user_in, unmap_page_in};
use mm::paging::PageFlags;

pub const PAGE: u64 = 4096;
/// Exclusive upper bound of the user (lower canonical) half.
pub const USER_MAX: u64 = 0x0000_8000_0000_0000;
/// Lowest address a request may touch (keeps the null page and its neighbours unmapped).
pub const USER_MIN: u64 = 0x1_0000;
/// Where the kernel starts looking for a free range when the caller passes base 0
/// (above the EXE at 0x40_0000 and ntdll/kernel32 around 0x1000_0000).
const SEARCH_START: u64 = 0x2000_0000;

pub const STATUS_INVALID_PARAMETER: u64 = 0xC000_000D;
pub const STATUS_NO_MEMORY: u64 = 0xC000_0017;
pub const STATUS_CONFLICTING_ADDRESSES: u64 = 0xC000_0018;

/// PAGE_READWRITE. It is the only protection accepted; the pages are always
/// mapped USER|WRITABLE|NO_EXECUTE (no user-executable memory in this phase).
pub const PAGE_READWRITE: u32 = 0x04;

/// Rounds the requested range to whole pages (base down, size up) with checked
/// arithmetic. Returns (base, size) or None on overflow / zero size.
fn round_range(base: u64, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let end = base.checked_add(size)?; // size is measured from the unrounded base
    let rbase = base & !(PAGE - 1);
    let rend = end.checked_add(PAGE - 1)? & !(PAGE - 1);
    Some((rbase, rend - rbase))
}

/// True if [base, base+size) is page-aligned, non-overflowing and entirely inside
/// [USER_MIN, USER_MAX).
fn in_user_half(base: u64, size: u64) -> bool {
    match base.checked_add(size) {
        Some(end) => base >= USER_MIN && end <= USER_MAX && size != 0,
        None => false,
    }
}

fn is_free(root: u64, base: u64, size: u64) -> bool {
    let mut va = base;
    let end = base + size; // validated by in_user_half
    while va < end {
        if translate_in(root, va).is_some() {
            return false;
        }
        va += PAGE;
    }
    true
}

/// First-fit search for an unmapped gap of `size` bytes, scanning upward from
/// SEARCH_START and skipping past any mapped page found.
fn find_free(root: u64, size: u64) -> Option<u64> {
    let mut cand = SEARCH_START;
    while cand.checked_add(size)? <= USER_MAX {
        let mut va = cand;
        let end = cand + size;
        let mut clash = None;
        while va < end {
            if translate_in(root, va).is_some() {
                clash = Some(va);
                break;
            }
            va += PAGE;
        }
        match clash {
            None => return Some(cand),
            Some(m) => cand = m + PAGE,
        }
    }
    None
}

/// Maps `size` bytes of zeroed USER|RW|NX pages in the space `root`. `req_base`
/// 0 lets the kernel choose. Returns the (base, size) actually reserved.
pub fn alloc_in(root: u64, req_base: u64, req_size: u64, protect: u32) -> Result<(u64, u64), u64> {
    if protect != PAGE_READWRITE {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let (base, size) = round_range(req_base, req_size).ok_or(STATUS_INVALID_PARAMETER)?;
    let base = if req_base == 0 {
        find_free(root, size).ok_or(STATUS_NO_MEMORY)?
    } else {
        base
    };
    if !in_user_half(base, size) {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if !is_free(root, base, size) {
        return Err(STATUS_CONFLICTING_ADDRESSES);
    }
    let flags = PageFlags::USER
        .union(PageFlags::WRITABLE)
        .union(PageFlags::NO_EXECUTE);
    let mut va = base;
    let end = base + size;
    while va < end {
        let ok = match alloc_frame() {
            Some(frame) => {
                unsafe { core::ptr::write_bytes(phys_to_virt(frame) as *mut u8, 0, PAGE as usize) };
                if unsafe { map_page_in(root, va, frame, flags) }.is_ok() {
                    true
                } else {
                    free_frame(frame);
                    false
                }
            }
            None => false,
        };
        if !ok {
            // Roll back everything mapped by this call: no partial region, no leak.
            let mut undo = base;
            while undo < va {
                if let Some(phys) = translate_in(root, undo) {
                    let _ = unsafe { unmap_page_in(root, undo) };
                    free_frame(phys & !(PAGE - 1));
                }
                undo += PAGE;
            }
            return Err(STATUS_NO_MEMORY);
        }
        va += PAGE;
    }
    Ok((base, size))
}

/// Unmaps and frees every page of the range. The whole range must be in the
/// user half and currently mapped as USER pages, otherwise nothing is touched.
pub fn free_in(root: u64, req_base: u64, req_size: u64) -> Result<(u64, u64), u64> {
    let (base, size) = round_range(req_base, req_size).ok_or(STATUS_INVALID_PARAMETER)?;
    if !in_user_half(base, size) {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut va = base;
    let end = base + size;
    while va < end {
        if translate_user_in(root, va).is_none() {
            return Err(STATUS_INVALID_PARAMETER);
        }
        va += PAGE;
    }
    let mut va = base;
    while va < end {
        if let Some(phys) = translate_in(root, va) {
            let _ = unsafe { unmap_page_in(root, va) };
            free_frame(phys & !(PAGE - 1));
        }
        va += PAGE;
    }
    Ok((base, size))
}
