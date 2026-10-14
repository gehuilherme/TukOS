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
