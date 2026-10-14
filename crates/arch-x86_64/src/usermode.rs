//! Entering Ring 3 and returning via the exit syscall. Intel SDM Vol. 3A ch. 6
//! (IRET to a less-privileged level) and Vol. 2 (SYSRET).

#[cfg(target_os = "none")]
use crate::gdt::{USER_CS, USER_DS};
#[cfg(target_os = "none")]
use crate::percpu::{OFF_KERNEL_RETURN_RSP, OFF_KERNEL_RSP};

#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.global enter_user
enter_user:
    // rdi = user entry RIP, rsi = user stack top.
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    // Save kernel rsp so the exit path can longjmp back here, and use the
    // current kernel stack as the syscall-entry stack.
    mov gs:[{off_ret}], rsp
    mov gs:[{off_ksp}], rsp
    // Build the iretq frame for Ring 3.
    push {user_ds}        // SS
    push rsi              // RSP (user stack top)
    push 0x2              // RFLAGS: reserved bit 1 set, IF clear (no preemption)
    push {user_cs}        // CS
    push rdi              // RIP (user entry)
    swapgs                // kernel GS -> user; per-CPU now in KERNEL_GS_BASE
    iretq

.global tukos_user_return
tukos_user_return:
    // rdi = exit code. Restore the kernel stack saved by enter_user and return.
    mov rsp, gs:[{off_ret}]
    mov rax, rdi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
"#,
    off_ret = const OFF_KERNEL_RETURN_RSP,
    off_ksp = const OFF_KERNEL_RSP,
    user_ds = const USER_DS as u64,
    user_cs = const USER_CS as u64,
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    pub fn enter_user(entry: u64, user_stack_top: u64) -> u64;
    pub fn tukos_user_return(exit_code: u64) -> !;
}

/// Jumps back to the kernel context saved by `enter_user`, carrying `code`.
///
/// # Safety
/// Must run on the syscall-entry kernel stack, during a user excursion started
/// by `enter_user`; `gs` must currently be the kernel per-CPU block.
#[cfg(target_os = "none")]
pub unsafe fn exit_to_kernel(code: u64) -> ! {
    unsafe { tukos_user_return(code) }
}
