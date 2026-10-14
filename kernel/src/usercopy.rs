//! Validated copies of user memory. The kernel never dereferences a user
//! pointer directly: it checks the range is in the lower (user) half and that
//! every page is mapped in the active address space, then copies via the HHDM.

extern crate alloc;
use alloc::vec::Vec;

const USER_MAX: u64 = 0x0000_8000_0000_0000;
const MAX_LEN: u64 = 1 << 20;
const PAGE: u64 = 4096;

pub fn copy_from_user(va: u64, len: u64) -> Option<Vec<u8>> {
    if len == 0 {
        return Some(Vec::new());
    }
    if len > MAX_LEN {
        return None;
    }
    let end = va.checked_add(len)?;
    if end > USER_MAX {
        return None; // not entirely in the user half
    }
    let root = crate::arch_cr3_base();
    let mut out = Vec::with_capacity(len as usize);
    let mut a = va;
    while a < end {
        let page = a & !(PAGE - 1);
        let phys = crate::mem::paging::translate_user_in(root, page)?;
        let off = a & (PAGE - 1);
        let n = core::cmp::min(PAGE - off, end - a);
        let src = (crate::mem::frames::phys_to_virt(phys) + off) as *const u8;
        for i in 0..n {
            out.push(unsafe { core::ptr::read_volatile(src.add(i as usize)) });
        }
        a += n;
    }
    Some(out)
}

/// Writes `data` to user memory at `va`. Same validation as `copy_from_user`
/// (lower half, every page mapped with USER and WRITABLE); false if any page is
/// invalid. Pages are checked before any byte is written.
pub fn copy_to_user(va: u64, data: &[u8]) -> bool {
    let len = data.len() as u64;
    if len == 0 {
        return true;
    }
    if len > MAX_LEN {
        return false;
    }
    let Some(end) = va.checked_add(len) else {
        return false;
    };
    if end > USER_MAX {
        return false;
    }
    let root = crate::arch_cr3_base();
    // First pass: validate every page so a failure leaves memory untouched.
    let mut a = va;
    while a < end {
        let page = a & !(PAGE - 1);
        if crate::mem::paging::translate_user_writable_in(root, page).is_none() {
            return false;
        }
        a = page + PAGE;
    }
    let mut a = va;
    let mut done = 0usize;
    while a < end {
        let page = a & !(PAGE - 1);
        let Some(phys) = crate::mem::paging::translate_user_writable_in(root, page) else {
            return false;
        };
        let off = a & (PAGE - 1);
        let n = core::cmp::min(PAGE - off, end - a) as usize;
        let dst = (crate::mem::frames::phys_to_virt(phys) + off) as *mut u8;
        for i in 0..n {
            unsafe { core::ptr::write_volatile(dst.add(i), data[done + i]) };
        }
        done += n;
        a += n as u64;
    }
    true
}
