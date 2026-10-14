//! Port-mapped I/O.

use core::arch::asm;

/// # Safety
/// Reads a hardware port; the caller must know the device semantics.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    unsafe {
        asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags))
    };
    v
}

/// # Safety
/// Writes a hardware port; the caller must know the device semantics.
#[inline]
pub unsafe fn outb(port: u16, v: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags))
    };
}

/// # Safety
/// Writes a hardware port; the caller must know the device semantics.
#[inline]
pub unsafe fn outl(port: u16, v: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") v, options(nomem, nostack, preserves_flags))
    };
}
