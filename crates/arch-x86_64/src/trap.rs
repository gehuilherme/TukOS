//! Register state saved by the exception entry stubs, and the dispatch hook.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Layout pushed by `tukos_isr_common`: GPRs (r15 at the lowest address), then the
/// vector and error code pushed by each stub, then the CPU-pushed interrupt frame.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

static HANDLER: AtomicUsize = AtomicUsize::new(0);

/// Installs the kernel function that receives every trap.
pub fn set_handler(handler: fn(&mut TrapFrame)) {
    HANDLER.store(handler as usize, Ordering::SeqCst);
}

/// Repairs a corrupted interrupt-return frame so `iretq` cannot `#GP` the kernel.
///
/// A well-formed frame returns either to the kernel (`CS == KERNEL_CS`) or to Ring 3
/// (`CS == USER_CS` and `SS == USER_DS`) — the only valid selectors in TukOS. A rare
/// state corruption has been observed to leave `0x18` (the user-data selector WITHOUT
/// RPL 3) in a frame's `CS` or `SS`; returning to Ring 3 with such a selector `#GP`s
/// the kernel at the `iretq` in `tukos_isr_common`. A user program must never be able
/// to crash the kernel, so any frame that is neither a clean kernel return nor the
/// canonical user return is treated as a corrupted user-return frame and repaired to
/// the only valid user selectors. The caller holds `&mut TrapFrame` over the exact
/// stack memory `iretq` reloads, so the repair takes effect on return. Clean frames
/// (the normal path) are left untouched. Returns `true` iff a repair was made.
pub fn sanitize_return_frame(frame: &mut TrapFrame) -> bool {
    let kernel_cs = crate::gdt::KERNEL_CS as u64;
    let user_cs = crate::gdt::USER_CS as u64;
    let user_ds = crate::gdt::USER_DS as u64;
    if frame.cs != kernel_cs && (frame.cs != user_cs || frame.ss != user_ds) {
        frame.cs = user_cs;
        frame.ss = user_ds;
        return true;
    }
    false
}

#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.macro TUKOS_ISR_NOERR n
.global tukos_isr_\n
tukos_isr_\n:
    push 0
    push \n
    jmp tukos_isr_common
.endm

.macro TUKOS_ISR_ERR n
.global tukos_isr_\n
tukos_isr_\n:
    push \n
    jmp tukos_isr_common
.endm

.irp n, 0,1,2,3,4,5,6,7,9,15,16,18,19,20,22,23,24,25,26,27,28,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
    TUKOS_ISR_NOERR \n
.endr
.irp n, 8,10,11,12,13,14,17,21,29,30
    TUKOS_ISR_ERR \n
.endr

/* Dedicated stub for the LAPIC spurious vector (255). */
.global tukos_isr_255
tukos_isr_255:
    push 0
    push 255
    jmp tukos_isr_common

tukos_isr_common:
    // Conditional swapgs on entry: swap only if we came from Ring 3 (CS RPL == 3).
    // At this point rsp -> vector; CPU-pushed CS is at [rsp+24].
    test byte ptr [rsp+24], 3
    jz 3f
    swapgs
3:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    cld
    call tukos_trap_entry
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    // Conditional swapgs on exit: swap back only if returning to Ring 3.
    test byte ptr [rsp+8], 3
    jz 4f
    swapgs
4:
    iretq

.section .rodata
.balign 8
.global tukos_isr_table
tukos_isr_table:
.irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
    .quad tukos_isr_\n
.endr
.section .text
"#
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Entry stub addresses for vectors 0..48, defined in the assembly above.
    pub static tukos_isr_table: [u64; 48];
    /// Entry stub for the spurious interrupt vector 255.
    pub fn tukos_isr_255();
}

/// Called by `tukos_isr_common` with a pointer to the saved frame.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
extern "C" fn tukos_trap_entry(frame: &mut TrapFrame) {
    let raw = HANDLER.load(Ordering::SeqCst);
    if raw == 0 {
        loop {
            crate::cpu::hlt();
        }
    }
    let handler: fn(&mut TrapFrame) = unsafe { core::mem::transmute(raw) };
    handler(frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    fn zeroed_frame(cs: u64, ss: u64) -> TrapFrame {
        let mut f: TrapFrame = unsafe { core::mem::zeroed() };
        f.cs = cs;
        f.ss = ss;
        f
    }

    #[test]
    fn sanitize_leaves_clean_frames_untouched() {
        use crate::gdt::{KERNEL_CS, USER_CS, USER_DS};
        let mut k = zeroed_frame(KERNEL_CS as u64, 0);
        assert!(!sanitize_return_frame(&mut k));
        let mut u = zeroed_frame(USER_CS as u64, USER_DS as u64);
        assert!(!sanitize_return_frame(&mut u));
        assert_eq!(u.cs, USER_CS as u64);
        assert_eq!(u.ss, USER_DS as u64);
    }

    #[test]
    fn sanitize_repairs_corrupted_selectors() {
        use crate::gdt::{USER_CS, USER_DS};
        // Bad SS (0x18 = user-data selector without RPL 3), good CS.
        let mut a = zeroed_frame(USER_CS as u64, 0x18);
        assert!(sanitize_return_frame(&mut a));
        assert_eq!((a.cs, a.ss), (USER_CS as u64, USER_DS as u64));
        // Bad CS (a data selector in CS), good SS.
        let mut b = zeroed_frame(0x18, USER_DS as u64);
        assert!(sanitize_return_frame(&mut b));
        assert_eq!((b.cs, b.ss), (USER_CS as u64, USER_DS as u64));
    }

    #[test]
    fn trap_frame_layout_matches_assembly_stub() {
        assert_eq!(core::mem::size_of::<TrapFrame>(), 22 * 8);
        assert_eq!(offset_of!(TrapFrame, r15), 0);
        assert_eq!(offset_of!(TrapFrame, rax), 14 * 8);
        assert_eq!(offset_of!(TrapFrame, vector), 15 * 8);
        assert_eq!(offset_of!(TrapFrame, error_code), 16 * 8);
        assert_eq!(offset_of!(TrapFrame, rip), 17 * 8);
        assert_eq!(offset_of!(TrapFrame, ss), 21 * 8);
    }
}
