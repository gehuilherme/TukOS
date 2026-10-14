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
}

const PAGE: u64 = 4096;

/// Maps every section of `image` into `space` at `load_base + rva`, zero-filling
/// the slack beyond raw data, applies DIR64 base relocations for the delta, and
/// returns the entry point virtual address.
///
/// Limitation: a DIR64 relocation that crosses a page boundary is not handled.
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
        for p in 0..pages {
            let frame = alloc_frame().ok_or(LoadError::Alloc)?;
            let dst = phys_to_virt(frame) as *mut u8;
            // Zero the whole page first (covers .bss slack), then copy raw bytes
            // through the HHDM alias; the user mapping carries the final W^X flags.
            unsafe {
                core::ptr::write_bytes(dst, 0, PAGE as usize);
                let off = (p * PAGE) as usize;
                if off < sec.raw.len() {
                    let n = core::cmp::min(PAGE as usize, sec.raw.len() - off);
                    core::ptr::copy_nonoverlapping(sec.raw.as_ptr().add(off), dst, n);
                }
                space
                    .map(load_base + sec.rva as u64 + p * PAGE, frame, flags)
                    .map_err(|_| LoadError::Map)?;
            }
        }
    }

    if delta != 0 {
        for r in pe.relocations().map_err(LoadError::Parse)? {
            match r.kind {
                REL_BASED_ABSOLUTE => {}
                REL_BASED_DIR64 => {
                    let va = load_base + r.rva as u64;
                    let phys = space.translate(va & !(PAGE - 1)).ok_or(LoadError::Map)?;
                    let p = (phys_to_virt(phys) + (va & (PAGE - 1))) as *mut u64;
                    unsafe {
                        let v = core::ptr::read_unaligned(p);
                        core::ptr::write_unaligned(p, v.wrapping_add(delta));
                    }
                }
                _ => return Err(LoadError::UnsupportedReloc),
            }
        }
    }

    Ok(load_base + pe.entry_point_rva() as u64)
}
