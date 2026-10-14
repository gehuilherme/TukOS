//! A user process: an address space plus (for now) a single user thread.
//! No process exit yet, so the address space is intentionally leaked.

extern crate alloc;

use crate::mem::space::AddressSpace;
use mm::paging::PageFlags;

const USER_CODE_VA: u64 = 0x0000_0000_0040_0000;
const USER_STACK_VA: u64 = 0x0000_0000_0050_0000;
const USER_STACK_TOP: u64 = 0x0000_0000_0051_0000;
const KSTACK_SIZE: usize = 16 * 1024;

core::arch::global_asm!(
    r#"
.section .rodata
.global tukos_demo_user
.global tukos_demo_user_end
tukos_demo_user:
    mov rax, 1          // SYS_WRITE_MARK
    mov rdi, 0xBEEF
    syscall
1:
    mov rax, 1          // keep marking; the timer preempts this loop
    mov rdi, 0xBEEF
    syscall
    jmp 1b
tukos_demo_user_end:
"#
);

unsafe extern "C" {
    static tukos_demo_user: u8;
    static tukos_demo_user_end: u8;
}

fn demo_payload() -> &'static [u8] {
    let start = (&raw const tukos_demo_user) as usize;
    let end = (&raw const tukos_demo_user_end) as usize;
    unsafe { core::slice::from_raw_parts(start as *const u8, end - start) }
}

/// Creates a user process running the demo payload and schedules its thread.
pub fn spawn_demo_user() -> usize {
    let space = AddressSpace::new_user();
    let code_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ codigo");
    let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha");

    unsafe {
        // Copy the payload through the HHDM window of the frame, then map it
        // read-only + executable (W^X) in the user space.
        core::ptr::copy_nonoverlapping(
            demo_payload().as_ptr(),
            crate::mem::frames::phys_to_virt(code_phys) as *mut u8,
            demo_payload().len(),
        );
        space
            .map(USER_CODE_VA, code_phys, PageFlags::USER)
            .expect("map codigo");
        space
            .map(
                USER_STACK_VA,
                stack_phys,
                PageFlags::USER
                    .union(PageFlags::WRITABLE)
                    .union(PageFlags::NO_EXECUTE),
            )
            .expect("map pilha");
    }

    let kstack = alloc::vec![0u8; KSTACK_SIZE].into_boxed_slice();
    let root = space.root();
    // Leak the address space: no process teardown in this plan.
    core::mem::forget(space);
    crate::sched::spawn_user(root, USER_CODE_VA, USER_STACK_TOP, kstack)
}
