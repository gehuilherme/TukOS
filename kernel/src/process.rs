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

// ---------------------------------------------------------------------------
// TukOS-built test PE (clean room: assembled by hand here, no imports).
// ---------------------------------------------------------------------------

/// Machine code of the test PE entry (position-independent).
const TEST_PE_TEXT: [u8; 32] = [
    0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1      (SYS_WRITE_MARK)
    0x48, 0xC7, 0xC7, 0x5E, 0x00, 0x00, 0x00, // mov rdi, 0x5E
    0x0F, 0x05, // syscall
    0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0      (SYS_EXIT)
    0x48, 0xC7, 0xC7, 0x00, 0x00, 0x00, 0x00, // mov rdi, 0
    0x0F, 0x05, // syscall
];

const PE_IMAGE_BASE: u64 = 0x40_0000;
const PE_RAW_OFF: usize = 0x200;
const SCN_CODE_EXEC_READ: u32 = 0x6000_0020; // CODE | EXECUTE | READ
const SCN_DATA_RW: u32 = 0xC000_0040; // INITIALIZED_DATA | READ | WRITE

fn put(v: &mut [u8], off: usize, b: &[u8]) {
    v[off..off + b.len()].copy_from_slice(b);
}

/// Builds a minimal PE32+ (AMD64) with one section at RVA 0x1000.
fn build_pe(entry_rva: u32, raw: &[u8], vsize: u32, ch: u32) -> alloc::vec::Vec<u8> {
    const OPT_SIZE: usize = 112 + 16 * 8;
    let mut v = alloc::vec![0u8; PE_RAW_OFF + raw.len()];
    put(&mut v, 0, &0x5A4Du16.to_le_bytes()); // "MZ"
    put(&mut v, 0x3C, &0x40u32.to_le_bytes()); // e_lfanew
    put(&mut v, 0x40, &0x0000_4550u32.to_le_bytes()); // "PE\0\0"
    let coff = 0x44;
    put(&mut v, coff, &0x8664u16.to_le_bytes()); // AMD64
    put(&mut v, coff + 2, &1u16.to_le_bytes()); // 1 section
    put(&mut v, coff + 16, &(OPT_SIZE as u16).to_le_bytes());
    let opt = coff + 20;
    put(&mut v, opt, &0x20Bu16.to_le_bytes()); // PE32+
    put(&mut v, opt + 16, &entry_rva.to_le_bytes());
    put(&mut v, opt + 24, &PE_IMAGE_BASE.to_le_bytes());
    put(&mut v, opt + 56, &0x2000u32.to_le_bytes()); // SizeOfImage
    put(&mut v, opt + 108, &16u32.to_le_bytes()); // NumberOfRvaAndSizes
    let s = opt + OPT_SIZE;
    put(&mut v, s, b".text\0\0\0");
    put(&mut v, s + 8, &vsize.to_le_bytes());
    put(&mut v, s + 12, &0x1000u32.to_le_bytes());
    put(&mut v, s + 16, &(raw.len() as u32).to_le_bytes());
    put(&mut v, s + 20, &(PE_RAW_OFF as u32).to_le_bytes());
    put(&mut v, s + 36, &ch.to_le_bytes());
    put(&mut v, PE_RAW_OFF, raw);
    v
}

/// The test PE: SYS_WRITE_MARK(0x5E) then SYS_EXIT(0).
pub fn build_test_pe() -> alloc::vec::Vec<u8> {
    let mut text = alloc::vec::Vec::from(TEST_PE_TEXT);
    text.extend_from_slice(&[0x0F, 0x0B]); // ud2
    let n = text.len() as u32;
    build_pe(0x1000, &text, n, SCN_CODE_EXEC_READ)
}

/// A PE whose single data section has VirtualSize 0x20 but one raw byte (0xAA).
pub fn build_bss_test_pe() -> alloc::vec::Vec<u8> {
    build_pe(0x1000, &[0xAA], 0x20, SCN_DATA_RW)
}

pub const RELOC_FIXTURE_IMAGE_BASE: u64 = 0x40_0000;
pub const RELOC_FIXTURE_ORIGINAL: u64 = 0x1122_3344_5566_7788;

/// A PE with a 2-page data section at RVA 0x1000 holding a u64 at RVA 0x1FFC
/// (straddling pages 0x1000/0x2000) and a .reloc section at RVA 0x3000 with one
/// DIR64 entry for that u64.
pub fn build_reloc_cross_page_pe() -> alloc::vec::Vec<u8> {
    const OPT_SIZE: usize = 112 + 16 * 8;
    const DATA_RAW: usize = 0x2000;
    const RELOC_RAW_OFF: usize = PE_RAW_OFF + DATA_RAW;
    let mut v = alloc::vec![0u8; RELOC_RAW_OFF + 0x0A];
    put(&mut v, 0, &0x5A4Du16.to_le_bytes());
    put(&mut v, 0x3C, &0x40u32.to_le_bytes());
    put(&mut v, 0x40, &0x0000_4550u32.to_le_bytes());
    let coff = 0x44;
    put(&mut v, coff, &0x8664u16.to_le_bytes());
    put(&mut v, coff + 2, &2u16.to_le_bytes()); // 2 sections
    put(&mut v, coff + 16, &(OPT_SIZE as u16).to_le_bytes());
    let opt = coff + 20;
    put(&mut v, opt, &0x20Bu16.to_le_bytes());
    put(&mut v, opt + 16, &0x1000u32.to_le_bytes());
    put(&mut v, opt + 24, &RELOC_FIXTURE_IMAGE_BASE.to_le_bytes());
    put(&mut v, opt + 56, &0x4000u32.to_le_bytes()); // SizeOfImage
    put(&mut v, opt + 108, &16u32.to_le_bytes());
    // Data directory 5 (base relocation): RVA 0x3000, size 0x0A.
    put(&mut v, opt + 112 + 5 * 8, &0x3000u32.to_le_bytes());
    put(&mut v, opt + 112 + 5 * 8 + 4, &0x0Au32.to_le_bytes());
    let s = opt + OPT_SIZE;
    // .data: RVA 0x1000, 0x2000 bytes.
    put(&mut v, s, b".data   ");
    put(&mut v, s + 8, &(DATA_RAW as u32).to_le_bytes());
    put(&mut v, s + 12, &0x1000u32.to_le_bytes());
    put(&mut v, s + 16, &(DATA_RAW as u32).to_le_bytes());
    put(&mut v, s + 20, &(PE_RAW_OFF as u32).to_le_bytes());
    put(&mut v, s + 36, &SCN_DATA_RW.to_le_bytes());
    // .reloc: RVA 0x3000, 0x0A bytes.
    let s2 = s + 40;
    put(&mut v, s2, b".reloc  ");
    put(&mut v, s2 + 8, &0x0Au32.to_le_bytes());
    put(&mut v, s2 + 12, &0x3000u32.to_le_bytes());
    put(&mut v, s2 + 16, &0x0Au32.to_le_bytes());
    put(&mut v, s2 + 20, &(RELOC_RAW_OFF as u32).to_le_bytes());
    put(&mut v, s2 + 36, &0x4200_0040u32.to_le_bytes()); // INIT_DATA|DISCARDABLE|READ
    // The u64 at RVA 0x1FFC -> file offset PE_RAW_OFF + 0xFFC.
    put(
        &mut v,
        PE_RAW_OFF + 0xFFC,
        &RELOC_FIXTURE_ORIGINAL.to_le_bytes(),
    );
    // Base relocation block: PageRVA 0x1000, BlockSize 0x0A, one DIR64 @ 0xFFC.
    put(&mut v, RELOC_RAW_OFF, &0x1000u32.to_le_bytes());
    put(&mut v, RELOC_RAW_OFF + 4, &0x0Au32.to_le_bytes());
    put(
        &mut v,
        RELOC_RAW_OFF + 8,
        &((10u16 << 12) | 0xFFC).to_le_bytes(),
    );
    v
}

/// The embedded test PE bytes (built once per call and leaked; test use only).
pub fn test_pe_bytes() -> &'static [u8] {
    build_test_pe().leak()
}

/// Loads and starts the embedded test PE at its preferred base.
pub fn spawn_test_pe() -> usize {
    spawn_pe(&build_test_pe(), 0x40_0000)
}

/// Loads `image` at `load_base` into a fresh user space, maps a user stack and
/// schedules a user thread at the entry point.
pub fn spawn_pe(image: &[u8], load_base: u64) -> usize {
    let space = AddressSpace::new_user();
    let entry = crate::loader::load_pe(&space, image, load_base).expect("load_pe");
    // Stack goes one guard page above the image so it can never collide with it.
    let stack_va = crate::loader::image_top(image, load_base).expect("image_top") + 4096;
    let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha");
    unsafe {
        space
            .map(
                stack_va,
                stack_phys,
                PageFlags::USER
                    .union(PageFlags::WRITABLE)
                    .union(PageFlags::NO_EXECUTE),
            )
            .expect("map pilha");
    }
    let kstack = alloc::vec![0u8; KSTACK_SIZE].into_boxed_slice();
    crate::sched::spawn_user(space, entry, stack_va + 4096, kstack)
}
