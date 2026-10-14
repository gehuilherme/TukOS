//! Thin wrappers over privileged CPU instructions.

use core::arch::asm;

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

/// 512-byte, 16-byte-aligned FXSAVE area.
#[repr(C, align(16))]
pub struct FxArea(pub [u8; 512]);

impl FxArea {
    /// Power-on x87/SSE state: FCW = 0x37F, MXCSR = 0x1F80, registers zero.
    pub fn new() -> Self {
        let mut a = [0u8; 512];
        a[0..2].copy_from_slice(&0x037Fu16.to_le_bytes());
        a[24..28].copy_from_slice(&0x1F80u32.to_le_bytes());
        FxArea(a)
    }
}

impl Default for FxArea {
    fn default() -> Self {
        Self::new()
    }
}

/// Enables SSE for Ring 3 code: CR0.EM = 0, CR0.MP = 1, CR4.OSFXSR = 1,
/// CR4.OSXMMEXCPT = 1. The kernel itself is soft-float and never touches XMM;
/// the scheduler saves/restores the state per thread with `fxsave`/`fxrstor`.
///
/// # Safety
/// Ring 0 only; call once per CPU before any user code runs.
pub unsafe fn enable_sse() {
    unsafe {
        asm!(
            "mov rax, cr0",
            "and rax, -5",
            "or rax, 2",
            "mov cr0, rax",
            "mov rax, cr4",
            "or rax, 0x600",
            "mov cr4, rax",
            out("rax") _,
            options(nostack)
        );
    }
}

/// # Safety
/// SSE must be enabled (`enable_sse`); `area` is valid for writes.
#[inline]
pub unsafe fn fxsave(area: *mut FxArea) {
    unsafe { asm!("fxsave [{}]", in(reg) area, options(nostack, preserves_flags)) };
}

/// # Safety
/// SSE must be enabled; `area` holds a valid FXSAVE image.
#[inline]
pub unsafe fn fxrstor(area: *const FxArea) {
    unsafe { asm!("fxrstor [{}]", in(reg) area, options(nostack, preserves_flags)) };
}

#[inline]
pub fn hlt() {
    unsafe { asm!("hlt", options(nostack, preserves_flags)) };
}

#[inline]
pub fn cli() {
    unsafe { asm!("cli", options(nostack)) };
}

#[inline]
pub fn int3() {
    unsafe { asm!("int3", options(nostack)) };
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

#[inline]
pub fn read_cr3() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// # Safety
/// `phys` must point at a valid PML4 for the current address space.
#[inline]
pub unsafe fn write_cr3(phys: u64) {
    unsafe { asm!("mov cr3, {}", in(reg) phys, options(nostack, preserves_flags)) };
}

/// # Safety
/// Invalidates the TLB entry for `virt`; always safe to call, `unsafe` only for symmetry.
#[inline]
pub unsafe fn invlpg(virt: u64) {
    unsafe { asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags)) };
}

/// Enables maskable interrupts. Not `nomem`: it orders memory like `cli`.
#[inline]
pub fn sti() {
    unsafe { asm!("sti", options(nostack)) };
}

#[inline]
pub fn read_rflags() -> u64 {
    let v: u64;
    unsafe { asm!("pushfq", "pop {}", out(reg) v, options(nomem, preserves_flags)) };
    v
}

/// # Safety
/// `msr` must be a valid MSR for this CPU.
#[inline]
pub unsafe fn read_msr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags))
    };
    ((hi as u64) << 32) | lo as u64
}

/// # Safety
/// `msr` must be a valid, writable MSR and `value` legal for it.
#[inline]
pub unsafe fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nostack, preserves_flags))
    };
}

/// # Safety
/// Exchanges GS base with IA32_KERNEL_GS_BASE; only valid at defined kernel/user
/// transitions, exactly once per transition.
#[inline]
pub unsafe fn swapgs() {
    unsafe { asm!("swapgs", options(nostack, preserves_flags)) };
}
