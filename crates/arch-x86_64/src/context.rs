//! Kernel-thread context switch. Saves the callee-saved registers of the
//! System V AMD64 ABI on the current stack, swaps stacks, restores and returns.

pub const fn initial_stack_words() -> usize {
    7
}

#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.global switch_context
switch_context:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

.global tukos_user_entry
tukos_user_entry:
    // Reached via switch_context's `ret` on a user thread's first run. The
    // iretq frame (RIP,CS,RFLAGS,RSP,SS) sits just above on the kernel stack.
    swapgs
    iretq
"#
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Saves the current stack pointer into `*save_rsp` and resumes the thread whose
    /// saved stack pointer is `load_rsp`.
    pub fn switch_context(save_rsp: *mut u64, load_rsp: u64);
    /// First-run target of a user thread: `swapgs; iretq` into Ring 3.
    pub fn tukos_user_entry();
}

#[cfg(test)]
mod tests {
    #[test]
    fn initial_frame_has_six_callee_saved_plus_return() {
        assert_eq!(super::initial_stack_words(), 7);
    }
}
