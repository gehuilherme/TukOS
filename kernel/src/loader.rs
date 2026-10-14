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
    /// Import by ordinal (deferred to Plan 9).
    UnsupportedImport,
    /// Imported function not exported by the target module (no stub in this plan).
    UnresolvedImport,
    /// IAT slot is outside the exe image or not in writable mapped user pages.
    BadIatSlot,
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

    // Map the PE headers read-only + NX at the image base, as Windows does, so a
    // user-mode loader (ntdll!LdrInitializeThunk) can parse the loaded image.
    let hdr = (pe.size_of_headers() as usize).min(image.len());
    if hdr != 0 {
        let pages = (hdr as u64).div_ceil(PAGE);
        for p in 0..pages {
            let frame = alloc_frame().ok_or(LoadError::Alloc)?;
            let dst = phys_to_virt(frame) as *mut u8;
            let off = (p * PAGE) as usize;
            let n = core::cmp::min(PAGE as usize, hdr - off);
            unsafe {
                core::ptr::write_bytes(dst, 0, PAGE as usize);
                core::ptr::copy_nonoverlapping(image.as_ptr().add(off), dst, n);
                let va = load_base.checked_add(p * PAGE).ok_or(LoadError::Overflow)?;
                space
                    .map(va, frame, PageFlags::USER.union(PageFlags::NO_EXECUTE))
                    .map_err(|_| LoadError::Map)?;
            }
        }
    }

    // MSVC puts the IAT in a read-only section; like the Windows loader, make the
    // pages holding the IAT directory writable (never executable) so imports can
    // be bound. Pages of executable sections are never made writable.
    let (iat_rva, iat_size) = pe.iat_directory();
    let iat_start = u64::from(iat_rva);
    let iat_end = iat_start + u64::from(iat_size);

    for sec in pe.sections() {
        let (writable, exec) = characteristics_to_flags(sec.characteristics);
        let base_flags = {
            let mut f = PageFlags::USER;
            if !exec {
                f = f.union(PageFlags::NO_EXECUTE);
            }
            f
        };
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
            let page_rva = u64::from(sec.rva) + p * PAGE;
            let holds_iat = iat_size != 0 && page_rva < iat_end && page_rva + PAGE > iat_start;
            let flags = if writable || (holds_iat && !exec) {
                base_flags.union(PageFlags::WRITABLE)
            } else {
                base_flags
            };
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

/// Translates `va` to a pointer (via the HHDM alias of its frame) if the user
/// page is mapped writable, else None.
fn writable_ptr(space: &AddressSpace, va: u64) -> Option<*mut u8> {
    let phys = crate::mem::paging::translate_user_writable_in(space.root(), va & !(PAGE - 1))?;
    Some((phys_to_virt(phys) + (va & (PAGE - 1))) as *mut u8)
}

/// Resolves the imports of `exe` (loaded at `exe_base`) that target `ntdll.dll`
/// against the exports of `ntdll` (loaded at `ntdll_base`), writing each address
/// into the exe's IAT. Only by-name imports are supported (ordinals ->
/// `UnsupportedImport`); a name missing from the ntdll exports yields
/// `UnresolvedImport` (no "unimplemented" stub in this plan). Each 8-byte IAT slot
/// is validated to lie inside the exe image and in writable user pages (W^X per
/// section means a code-section slot is rejected with `BadIatSlot`) before being
/// written through the frame's HHDM alias, never through the user VA.
pub fn resolve_imports(
    space: &AddressSpace,
    exe: &[u8],
    exe_base: u64,
    ntdll: &[u8],
    ntdll_base: u64,
) -> Result<(), LoadError> {
    let exe_pe = PeFile::parse(exe).map_err(LoadError::Parse)?;
    let ntdll_pe = PeFile::parse(ntdll).map_err(LoadError::Parse)?;
    let exe_top = image_top(exe, exe_base)?;
    for dll in exe_pe.imports().map_err(LoadError::Parse)? {
        if !dll.name.eq_ignore_ascii_case("ntdll.dll") {
            continue;
        }
        for entry in dll.thunks {
            let name = match entry.kind {
                pe::ImportKind::Name { name, .. } => name,
                pe::ImportKind::Ordinal(_) => return Err(LoadError::UnsupportedImport),
            };
            let rva = ntdll_pe
                .export_rva_by_name(name)
                .ok_or(LoadError::UnresolvedImport)?;
            let addr = ntdll_base
                .checked_add(rva as u64)
                .ok_or(LoadError::Overflow)?;
            let slot = exe_base
                .checked_add(entry.iat_rva as u64)
                .ok_or(LoadError::BadIatSlot)?;
            let end = slot.checked_add(8).ok_or(LoadError::BadIatSlot)?;
            if end > exe_top {
                return Err(LoadError::BadIatSlot);
            }
            // Validate all 8 bytes before writing any (the slot may straddle pages).
            let mut ptrs = [core::ptr::null_mut::<u8>(); 8];
            for (i, p) in ptrs.iter_mut().enumerate() {
                *p = writable_ptr(space, slot + i as u64).ok_or(LoadError::BadIatSlot)?;
            }
            for (p, b) in ptrs.iter().zip(addr.to_le_bytes()) {
                unsafe { core::ptr::write_volatile(*p, b) };
            }
        }
    }
    Ok(())
}
