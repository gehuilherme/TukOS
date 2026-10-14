//! Thin wrappers over privileged CPU instructions.

use core::arch::asm;

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

#[inline]
pub fn hlt() {
    unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
}

#[inline]
pub fn cli() {
    unsafe { asm!("cli", options(nomem, nostack)) };
}

#[inline]
pub fn int3() {
    unsafe { asm!("int3", options(nomem, nostack)) };
}

#[inline]
pub fn read_cr2() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn read_cs() -> u16 {
    let v: u16;
    unsafe { asm!("mov {0:x}, cs", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn read_ss() -> u16 {
    let v: u16;
    unsafe { asm!("mov {0:x}, ss", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// Current task register selector.
#[inline]
pub fn str() -> u16 {
    let v: u16;
    unsafe { asm!("str {0:x}", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn sidt() -> DescriptorTablePointer {
    let mut p = DescriptorTablePointer { limit: 0, base: 0 };
    unsafe { asm!("sidt [{}]", in(reg) &mut p, options(nostack, preserves_flags)) };
    p
}

/// # Safety
/// `p` must describe a valid GDT that outlives its use by the CPU.
#[inline]
pub unsafe fn lgdt(p: &DescriptorTablePointer) {
    unsafe { asm!("lgdt [{}]", in(reg) p, options(readonly, nostack, preserves_flags)) };
}

/// # Safety
/// `p` must describe a valid IDT that outlives its use by the CPU.
#[inline]
pub unsafe fn lidt(p: &DescriptorTablePointer) {
    unsafe { asm!("lidt [{}]", in(reg) p, options(readonly, nostack, preserves_flags)) };
}
