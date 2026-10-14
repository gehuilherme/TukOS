//! A user process: an address space plus (for now) a single user thread.
//! The thread owns its address space; both are released when it exits.

extern crate alloc;

use crate::mem::space::AddressSpace;
use mm::paging::PageFlags;

const USER_CODE_VA: u64 = 0x0000_0000_0040_0000;
const USER_STACK_VA: u64 = 0x0000_0000_0050_0000;
const USER_STACK_TOP: u64 = USER_STACK_VA + 4096; // top of the single mapped page
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

.global tukos_demo_user_a
.global tukos_demo_user_a_end
tukos_demo_user_a:
1:
    mov rax, 1
    mov rdi, 0xA
    syscall
    jmp 1b
tukos_demo_user_a_end:

.global tukos_demo_user_b
.global tukos_demo_user_b_end
tukos_demo_user_b:
1:
    mov rax, 1
    mov rdi, 0xB
    syscall
    jmp 1b
tukos_demo_user_b_end:

.global tukos_exiting_user
.global tukos_exiting_user_end
tukos_exiting_user:
    mov rax, 0          // SYS_EXIT
    mov rdi, 0
    syscall
    ud2
tukos_exiting_user_end:
"#
);

unsafe extern "C" {
    static tukos_demo_user: u8;
    static tukos_demo_user_end: u8;
    static tukos_demo_user_a: u8;
    static tukos_demo_user_a_end: u8;
    static tukos_demo_user_b: u8;
    static tukos_demo_user_b_end: u8;
    static tukos_exiting_user: u8;
    static tukos_exiting_user_end: u8;
}

fn payload(start: *const u8, end: *const u8) -> &'static [u8] {
    let (start, end) = (start as usize, end as usize);
    unsafe { core::slice::from_raw_parts(start as *const u8, end - start) }
}

fn demo_payload() -> &'static [u8] {
    payload(&raw const tukos_demo_user, &raw const tukos_demo_user_end)
}

fn demo_payload_a() -> &'static [u8] {
    payload(
        &raw const tukos_demo_user_a,
        &raw const tukos_demo_user_a_end,
    )
}

fn demo_payload_b() -> &'static [u8] {
    payload(
        &raw const tukos_demo_user_b,
        &raw const tukos_demo_user_b_end,
    )
}

/// Creates a user process that immediately calls SYS_EXIT.
pub fn spawn_exiting_user() -> usize {
    spawn_payload(payload(
        &raw const tukos_exiting_user,
        &raw const tukos_exiting_user_end,
    ))
}

/// Creates a user process running the 0xBEEF demo payload.
pub fn spawn_demo_user() -> usize {
    spawn_payload(demo_payload())
}

/// Creates two user processes (marks 0xA and 0xB) in separate address spaces
/// that use the same user VAs.
pub fn spawn_demo_user_pair() {
    spawn_payload(demo_payload_a());
    spawn_payload(demo_payload_b());
}

/// Creates a user process running `bytes` and schedules its thread.
fn spawn_payload(bytes: &[u8]) -> usize {
    let space = AddressSpace::new_user();
    let code_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ codigo");
    let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha");

    unsafe {
        // Copy the payload through the HHDM window of the frame, then map it
        // read-only + executable (W^X) in the user space.
        core::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            crate::mem::frames::phys_to_virt(code_phys) as *mut u8,
            bytes.len(),
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
    crate::sched::spawn_user(space, USER_CODE_VA, USER_STACK_TOP, kstack)
}
