//! SYSCALL/SYSRET setup and the Ring 0 entry trampoline.
//! Intel SDM Vol. 2 (SYSCALL/SYSRET), Vol. 3A chapter 5.

pub const EFER: u32 = 0xC000_0080;
pub const STAR: u32 = 0xC000_0081;
pub const LSTAR: u32 = 0xC000_0082;
pub const FMASK: u32 = 0xC000_0084;

/// RFLAGS bits cleared on syscall entry: IF (9) and DF (10).
pub const FMASK_VALUE: u64 = (1 << 9) | (1 << 10);

pub const fn star_value(kernel_cs: u16, kernel_ds: u16) -> u64 {
    ((kernel_ds as u64) << 48) | ((kernel_cs as u64) << 32)
}

#[cfg(target_os = "none")]
mod hw {
    use super::*;
    use crate::cpu::{read_msr, write_msr};
    use crate::gdt::{KERNEL_CS, KERNEL_DS};
    use core::sync::atomic::{AtomicUsize, Ordering};

    static DISPATCH: AtomicUsize = AtomicUsize::new(0);

    pub fn set_dispatch(f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64) {
        DISPATCH.store(f as usize, Ordering::SeqCst);
    }

    /// # Safety
    /// Call once on the boot CPU after the per-CPU block and GDT are installed.
    pub unsafe fn init(entry: u64) {
        unsafe {
            write_msr(EFER, read_msr(EFER) | 1); // SCE
            write_msr(STAR, star_value(KERNEL_CS, KERNEL_DS));
            write_msr(LSTAR, entry);
            write_msr(FMASK, FMASK_VALUE);
        }
    }

    /// Called by the asm stub with the raw syscall arguments.
    #[unsafe(no_mangle)]
    extern "C" fn tukos_syscall_dispatch(
        nr: u64,
        a0: u64,
        a1: u64,
        a2: u64,
        a3: u64,
        user_rsp: u64,
    ) -> u64 {
        let raw = DISPATCH.load(Ordering::SeqCst);
        if raw == 0 {
            return u64::MAX;
        }
        let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64 =
            unsafe { core::mem::transmute(raw) };
        f(nr, a0, a1, a2, a3, user_rsp)
    }
}

#[cfg(target_os = "none")]
pub use hw::{init, set_dispatch};

#[cfg(target_os = "none")]
unsafe extern "C" {
    pub fn tukos_syscall_entry();
}

// SYSCALL entry (Windows x64 syscall ABI): rcx=user RIP, r11=user RFLAGS,
// eax=nr, r10,rdx,r8,r9 = a0..a3 (ntdll copies rcx->r10). C ABI target:
// dispatch(rdi=nr, rsi=a0, rdx=a1, rcx=a2, r8=a3, r9=user_rsp). Each register is
// overwritten only after its old value was read: rsi<-r10, rdi<-rax, rcx<-r8
// (a2, before r8 changes), r8<-r9 (a3, before r9 changes), r9<-saved user rsp.
// rdx (a1) is already in place.
#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.global tukos_syscall_entry
tukos_syscall_entry:
    swapgs
    mov gs:[{off_user_rsp}], rsp
    mov rsp, gs:[{off_kernel_rsp}]
    and rsp, -16
    push rcx
    push r11
    mov rsi, r10
    mov rdi, rax
    mov rcx, r8
    mov r8,  r9
    mov r9,  gs:[{off_user_rsp}]
    call tukos_syscall_dispatch
    pop r11
    pop rcx
    mov rsp, gs:[{off_user_rsp}]
    swapgs
    sysretq
"#,
    off_user_rsp = const crate::percpu::OFF_USER_RSP_SCRATCH,
    off_kernel_rsp = const crate::percpu::OFF_KERNEL_RSP,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_encodes_kernel_selectors() {
        // KERNEL_CS=0x08, KERNEL_DS=0x10. SYSCALL loads CS=0x08/SS=0x10;
        // SYSRET (64-bit) loads CS=0x10+16|3=0x23, SS=0x10+8|3=0x1b.
        let star = star_value(0x08, 0x10);
        assert_eq!((star >> 32) & 0xFFFF, 0x08);
        assert_eq!((star >> 48) & 0xFFFF, 0x10);
    }

    #[test]
    fn fmask_clears_if_and_df() {
        let m = core::hint::black_box(FMASK_VALUE);
        assert!(m & (1 << 9) != 0, "limpa IF");
        assert!(m & (1 << 10) != 0, "limpa DF");
    }
}
