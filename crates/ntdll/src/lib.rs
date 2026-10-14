//! TukOS ntdll.dll: thin syscall stubs with the Windows x64 convention.
//! Each `Nt*` export does `mov r10, rcx; mov eax, <tukos ssn>; syscall; ret`.
//! The syscall numbers MUST match the TukOS kernel dispatch.
#![no_std]
#![allow(non_snake_case)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

// TukOS syscall numbers (must match kernel/src/syscall.rs).
// NT_WRITE_FILE = 2, NT_TERMINATE_PROCESS = 3, NT_CLOSE = 4.

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtWriteFile() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 2", "syscall", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtTerminateProcess() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 3", "syscall", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtClose() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 4", "syscall", "ret");
}
