//! TEB/PEB construction in a process address space. Offsets follow the public
//! winternl.h x64 layout (NT_TIB / TEB / PEB).

use crate::mem::space::AddressSpace;
use mm::paging::PageFlags;

const TEB_STACK_BASE: usize = 0x08; // NT_TIB.StackBase
const TEB_STACK_LIMIT: usize = 0x10; // NT_TIB.StackLimit
const TEB_SELF: usize = 0x30; // NT_TIB.Self
const TEB_PEB: usize = 0x60; // TEB.ProcessEnvironmentBlock
const PEB_IMAGE_BASE: usize = 0x10; // PEB.ImageBaseAddress
const PEB_LDR: usize = 0x18; // PEB.Ldr
const PEB_PROCESS_PARAMETERS: usize = 0x20; // PEB.ProcessParameters
// RTL_USER_PROCESS_PARAMETERS (public winternl.h layout).
const PARAMS_MAXIMUM_LENGTH: usize = 0x00;
const PARAMS_LENGTH: usize = 0x04;
const PARAMS_STANDARD_OUTPUT: usize = 0x28;
const PARAMS_STANDARD_ERROR: usize = 0x30;
const PARAMS_SIZE: u32 = 0x80;
// PEB_LDR_DATA (public winternl.h layout).
const LDR_LENGTH: usize = 0x00;
const LDR_INITIALIZED: usize = 0x04;
const LDR_IN_LOAD_ORDER: usize = 0x10; // LIST_ENTRY head
const LDR_DATA_SIZE: u32 = 0x58;
// LDR_DATA_TABLE_ENTRY.
const ENT_DLL_BASE: usize = 0x30;
const ENT_ENTRY_POINT: usize = 0x38;
const ENT_SIZE_OF_IMAGE: usize = 0x40;
const ENT_FULL_DLL_NAME: usize = 0x48; // UNICODE_STRING
const ENT_BASE_DLL_NAME: usize = 0x58; // UNICODE_STRING
// Placement inside the ldr page.
const ENT_AREA: usize = 0x100;
const ENT_STRIDE: usize = 0x80;
const NAME_AREA: usize = 0x400;
const NAME_STRIDE: usize = 0x40;

/// Maps a zeroed USER|WRITABLE|NO_EXECUTE page at `va`; returns its HHDM pointer.
fn map_zeroed_page(space: &AddressSpace, va: u64) -> Result<*mut u8, ()> {
    let phys = crate::mem::frames::alloc_frame().ok_or(())?;
    let ptr = crate::mem::frames::phys_to_virt(phys) as *mut u8;
    unsafe {
        core::ptr::write_bytes(ptr, 0, 4096);
        space
            .map(
                va,
                phys,
                PageFlags::USER
                    .union(PageFlags::WRITABLE)
                    .union(PageFlags::NO_EXECUTE),
            )
            .map_err(|_| ())?;
    }
    Ok(ptr)
}

/// Builds the TEB at `teb_va` and the PEB at `peb_va` inside `space`, filling
/// the fields through the HHDM window of each frame (never the user VA).
pub fn setup(
    space: &AddressSpace,
    teb_va: u64,
    peb_va: u64,
    image_base: u64,
    stack_base: u64,
    stack_limit: u64,
) -> Result<(), ()> {
    let teb = map_zeroed_page(space, teb_va)?;
    let peb = map_zeroed_page(space, peb_va)?;
    unsafe {
        (teb.add(TEB_STACK_BASE) as *mut u64).write(stack_base);
        (teb.add(TEB_STACK_LIMIT) as *mut u64).write(stack_limit);
        (teb.add(TEB_SELF) as *mut u64).write(teb_va);
        (teb.add(TEB_PEB) as *mut u64).write(peb_va);
        (peb.add(PEB_IMAGE_BASE) as *mut u64).write(image_base);
    }
    Ok(())
}

/// One loaded module as described to the user-mode loader.
pub struct LdrModule<'a> {
    pub base: u64,
    pub entry: u64,
    pub size: u32,
    pub name: &'a str,
}

/// Maps a page at `ldr_va` holding a `PEB_LDR_DATA` and one
/// `LDR_DATA_TABLE_ENTRY` per module (circular `InLoadOrderModuleList`, in the
/// given order), then points `PEB.Ldr` (PEB at `peb_va`) at it. All pointers
/// stored are user VAs; bytes are written through the HHDM alias.
pub fn setup_ldr(
    space: &AddressSpace,
    ldr_va: u64,
    peb_va: u64,
    mods: &[LdrModule],
) -> Result<(), ()> {
    if mods.len() > (NAME_AREA - ENT_AREA) / ENT_STRIDE {
        return Err(());
    }
    let page = map_zeroed_page(space, ldr_va)?;
    let head = ldr_va + LDR_IN_LOAD_ORDER as u64;
    let ent_va = |i: usize| ldr_va + (ENT_AREA + i * ENT_STRIDE) as u64;
    // Link target for position `i` in the circular list (head after the last).
    let link = |i: usize| if i < mods.len() { ent_va(i) } else { head };
    unsafe {
        let w64 = |off: usize, v: u64| (page.add(off) as *mut u64).write(v);
        (page.add(LDR_LENGTH) as *mut u32).write(LDR_DATA_SIZE);
        (page.add(LDR_INITIALIZED) as *mut u32).write(1);
        w64(LDR_IN_LOAD_ORDER, link(0)); // head.Flink
        let last = if mods.is_empty() {
            head
        } else {
            ent_va(mods.len() - 1)
        };
        w64(LDR_IN_LOAD_ORDER + 8, last); // head.Blink
        for (i, m) in mods.iter().enumerate() {
            let eo = ENT_AREA + i * ENT_STRIDE;
            w64(eo, link(i + 1)); // Flink
            w64(eo + 8, if i == 0 { head } else { ent_va(i - 1) }); // Blink
            w64(eo + ENT_DLL_BASE, m.base);
            w64(eo + ENT_ENTRY_POINT, m.entry);
            (page.add(eo + ENT_SIZE_OF_IMAGE) as *mut u32).write(m.size);
            // UTF-16 name (ASCII widened), NUL-terminated, in the same page.
            let no = NAME_AREA + i * NAME_STRIDE;
            let mut n = 0usize;
            for b in m.name.bytes().take(NAME_STRIDE / 2 - 1) {
                (page.add(no + n * 2) as *mut u16).write(u16::from(b));
                n += 1;
            }
            let len = (n * 2) as u16;
            for us in [ENT_FULL_DLL_NAME, ENT_BASE_DLL_NAME] {
                (page.add(eo + us) as *mut u16).write(len); // Length
                (page.add(eo + us + 2) as *mut u16).write(len + 2); // MaximumLength
                w64(eo + us + 8, ldr_va + no as u64); // Buffer
            }
        }
    }
    // PEB.Ldr, via the HHDM alias of the PEB page.
    let peb_phys = space.translate(peb_va).ok_or(())?;
    unsafe {
        ((crate::mem::frames::phys_to_virt(peb_phys) as *mut u8).add(PEB_LDR) as *mut u64)
            .write(ldr_va);
    }
    Ok(())
}

/// Maps a page at `params_va` holding an `RTL_USER_PROCESS_PARAMETERS` whose
/// `StandardOutput`/`StandardError` are `console` (stdin stays 0), then points
/// `PEB.ProcessParameters` (PEB at `peb_va`) at it. This is where a program that
/// does not link kernel32 finds its stdout handle, as on real Windows.
pub fn setup_process_parameters(
    space: &AddressSpace,
    params_va: u64,
    peb_va: u64,
    console: u64,
) -> Result<(), ()> {
    let page = map_zeroed_page(space, params_va)?;
    unsafe {
        (page.add(PARAMS_MAXIMUM_LENGTH) as *mut u32).write(PARAMS_SIZE);
        (page.add(PARAMS_LENGTH) as *mut u32).write(PARAMS_SIZE);
        (page.add(PARAMS_STANDARD_OUTPUT) as *mut u64).write(console);
        (page.add(PARAMS_STANDARD_ERROR) as *mut u64).write(console);
    }
    let peb_phys = space.translate(peb_va).ok_or(())?;
    unsafe {
        ((crate::mem::frames::phys_to_virt(peb_phys) as *mut u8).add(PEB_PROCESS_PARAMETERS)
            as *mut u64)
            .write(params_va);
    }
    Ok(())
}
