//! Loads a parsed PE image into a user AddressSpace with per-section W^X.

use crate::mem::frames::{alloc_frame, phys_to_virt};
use crate::mem::space::AddressSpace;
use mm::paging::PageFlags;
use pe::{PeError, PeFile, REL_BASED_ABSOLUTE, REL_BASED_DIR64, characteristics_to_flags};

#[derive(Debug)]
#[allow(dead_code)] // payloads are for Debug/diagnostics
pub enum LoadError {
    Parse(PeError),
    Map,
    Alloc,
    UnsupportedReloc,
    Overflow,
}

const PAGE: u64 = 4096;

/// Read-modify-writes a little-endian u64 at user VA `va` in `space`, translating
/// each byte's page separately so it is safe across a page boundary. Returns
/// false if any byte is unmapped.
fn rmw_u64(space: &AddressSpace, va: u64, f: impl FnOnce(u64) -> u64) -> bool {
    let mut bytes = [0u8; 8];
    for i in 0..8u64 {
        let Some(a) = va.checked_add(i) else {
            return false;
        };
        let Some(phys) = space.translate(a & !(PAGE - 1)) else {
            return false;
        };
        let p = (phys_to_virt(phys) + (a & (PAGE - 1))) as *const u8;
        bytes[i as usize] = unsafe { core::ptr::read_volatile(p) };
    }
    let v = f(u64::from_le_bytes(bytes)).to_le_bytes();
    for i in 0..8u64 {
        let Some(a) = va.checked_add(i) else {
            return false;
        };
        let Some(phys) = space.translate(a & !(PAGE - 1)) else {
            return false;
        };
        let p = (phys_to_virt(phys) + (a & (PAGE - 1))) as *mut u8;
        unsafe { core::ptr::write_volatile(p, v[i as usize]) };
    }
    true
}

/// First page-aligned address above the image when loaded at `load_base`.
pub fn image_top(image: &[u8], load_base: u64) -> Result<u64, LoadError> {
    let pe = PeFile::parse(image).map_err(LoadError::Parse)?;
    let mut end = 0u64;
    for sec in pe.sections() {
        let sec_end = (sec.rva as u64)
            .checked_add(sec.virtual_size as u64)
            .ok_or(LoadError::Overflow)?;
        if sec_end > end {
            end = sec_end;
        }
    }
    let aligned = end.checked_add(PAGE - 1).ok_or(LoadError::Overflow)? & !(PAGE - 1);
    load_base.checked_add(aligned).ok_or(LoadError::Overflow)
}

/// Maps every section of `image` into `space` at `load_base + rva`, zero-filling
/// the slack beyond raw data, applies DIR64 base relocations for the delta, and
/// returns the entry point virtual address.
pub fn load_pe(space: &AddressSpace, image: &[u8], load_base: u64) -> Result<u64, LoadError> {
    let pe = PeFile::parse(image).map_err(LoadError::Parse)?;
    let delta = load_base.wrapping_sub(pe.image_base());

    for sec in pe.sections() {
        let (writable, exec) = characteristics_to_flags(sec.characteristics);
        let mut flags = PageFlags::USER;
        if writable {
            flags = flags.union(PageFlags::WRITABLE);
        }
        if !exec {
            flags = flags.union(PageFlags::NO_EXECUTE);
        }
        let pages = (sec.virtual_size as u64).div_ceil(PAGE).max(1);
        let sec_base = load_base
            .checked_add(sec.rva as u64)
            .ok_or(LoadError::Overflow)?;
        for p in 0..pages {
            let va = p
                .checked_mul(PAGE)
                .and_then(|o| sec_base.checked_add(o))
                .ok_or(LoadError::Overflow)?;
            let frame = alloc_frame().ok_or(LoadError::Alloc)?;
            let dst = phys_to_virt(frame) as *mut u8;
            // Zero the whole page first (covers .bss slack), then copy raw bytes
            // through the HHDM alias; the user mapping carries the final W^X flags.
            unsafe {
                core::ptr::write_bytes(dst, 0, PAGE as usize);
                let off = (p * PAGE) as usize;
                let vsize = sec.virtual_size as usize;
                if off < sec.raw.len() && off < vsize {
                    // Never copy file-alignment padding beyond virtual_size.
                    let n = core::cmp::min(
                        PAGE as usize,
                        core::cmp::min(sec.raw.len() - off, vsize - off),
                    );
                    core::ptr::copy_nonoverlapping(sec.raw.as_ptr().add(off), dst, n);
                }
                space.map(va, frame, flags).map_err(|_| LoadError::Map)?;
            }
        }
    }

    if delta != 0 {
        for r in pe.relocations().map_err(LoadError::Parse)? {
            match r.kind {
                REL_BASED_ABSOLUTE => {}
                REL_BASED_DIR64 => {
                    let va = load_base
                        .checked_add(r.rva as u64)
                        .ok_or(LoadError::Overflow)?;
                    if !rmw_u64(space, va, |v| v.wrapping_add(delta)) {
                        return Err(LoadError::Map);
                    }
                }
                _ => return Err(LoadError::UnsupportedReloc),
            }
        }
    }

    load_base
        .checked_add(pe.entry_point_rva() as u64)
        .ok_or(LoadError::Overflow)
}
