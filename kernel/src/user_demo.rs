//! A tiny Ring 3 payload copied into a user page: it issues SYS_WRITE_MARK(0xCAFE)
//! then SYS_EXIT(0x2A). Position-independent (only `syscall` with immediates).

use mm::paging::PageFlags;

const USER_CODE_VA: u64 = 0x0000_0000_0040_0000;
const USER_STACK_VA: u64 = 0x0000_0000_0050_0000;
const USER_STACK_TOP: u64 = 0x0000_0000_0051_0000;

core::arch::global_asm!(
    r#"
.section .rodata
.global tukos_user_payload
.global tukos_user_payload_end
tukos_user_payload:
    mov rax, 1          // SYS_WRITE_MARK
    mov rdi, 0xCAFE     // mark argument
    syscall
    mov rax, 0          // SYS_EXIT
    mov rdi, 0x2A       // exit code
    syscall
    ud2                 // unreachable: SYS_EXIT does not return to user
tukos_user_payload_end:
"#
);

unsafe extern "C" {
    static tukos_user_payload: u8;
    static tukos_user_payload_end: u8;
}

fn payload_bytes() -> &'static [u8] {
    let start = (&raw const tukos_user_payload) as usize;
    let end = (&raw const tukos_user_payload_end) as usize;
    unsafe { core::slice::from_raw_parts(start as *const u8, end - start) }
}

/// Maps a user code page and a user stack, copies the payload in, enters Ring 3,
/// and returns the user's exit code.
pub fn run() -> u64 {
    let code_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ codigo de usuario");
    let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha de usuario");
    unsafe {
        // User code: present + user + executable (no NX), writable while we copy.
        crate::mem::paging::map_page(
            USER_CODE_VA,
            code_phys,
            PageFlags::USER.union(PageFlags::WRITABLE),
        )
        .expect("map codigo de usuario");
        crate::mem::paging::map_page(
            USER_STACK_VA,
            stack_phys,
            PageFlags::USER
                .union(PageFlags::WRITABLE)
                .union(PageFlags::NO_EXECUTE),
        )
        .expect("map pilha de usuario");

        let bytes = payload_bytes();
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), USER_CODE_VA as *mut u8, bytes.len());

        // rsp0 for any exception taken while in Ring 3.
        let kstack_top = {
            let mut x: u64;
            core::arch::asm!("mov {}, rsp", out(reg) x);
            x
        };
        arch_x86_64::gdt::set_rsp0(kstack_top);

        let code = arch_x86_64::usermode::enter_user(USER_CODE_VA, USER_STACK_TOP);

        // Unmap and free.
        let _ = crate::mem::paging::unmap_page(USER_CODE_VA);
        let _ = crate::mem::paging::unmap_page(USER_STACK_VA);
        crate::mem::frames::free_frame(code_phys);
        crate::mem::frames::free_frame(stack_phys);
        code
    }
}
