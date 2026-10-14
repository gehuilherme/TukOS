//! TEB/PEB construction in a process address space. Offsets follow the public
//! winternl.h x64 layout (NT_TIB / TEB / PEB).

use crate::mem::space::AddressSpace;
use mm::paging::PageFlags;

const TEB_STACK_BASE: usize = 0x08; // NT_TIB.StackBase
const TEB_STACK_LIMIT: usize = 0x10; // NT_TIB.StackLimit
const TEB_SELF: usize = 0x30; // NT_TIB.Self
const TEB_PEB: usize = 0x60; // TEB.ProcessEnvironmentBlock
const PEB_IMAGE_BASE: usize = 0x10; // PEB.ImageBaseAddress

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
