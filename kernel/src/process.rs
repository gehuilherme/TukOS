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
    mov r10, 0xBEEF
    syscall
1:
    mov rax, 1          // keep marking; the timer preempts this loop
    mov r10, 0xBEEF
    syscall
    jmp 1b
tukos_demo_user_end:

.global tukos_demo_user_a
.global tukos_demo_user_a_end
tukos_demo_user_a:
1:
    mov rax, 1
    mov r10, 0xA
    syscall
    jmp 1b
tukos_demo_user_a_end:

.global tukos_demo_user_b
.global tukos_demo_user_b_end
tukos_demo_user_b:
1:
    mov rax, 1
    mov r10, 0xB
    syscall
    jmp 1b
tukos_demo_user_b_end:

.global tukos_exiting_user
.global tukos_exiting_user_end
tukos_exiting_user:
    mov rax, 0          // SYS_EXIT
    mov r10, 0
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
    // TEB/PEB pages sit above the stack page, one guard page apart.
    let teb_va = USER_STACK_VA + 2 * 4096;
    let peb_va = USER_STACK_VA + 3 * 4096;
    crate::teb::setup(
        &space,
        teb_va,
        peb_va,
        USER_CODE_VA,
        USER_STACK_TOP,
        USER_STACK_VA,
    )
    .expect("teb setup");
    crate::sched::spawn_user(space, USER_CODE_VA, USER_STACK_TOP, kstack, teb_va)
}

// ---------------------------------------------------------------------------
// TukOS-built test PE (clean room: assembled by hand here, no imports).
// ---------------------------------------------------------------------------

/// Machine code of the test PE entry (position-independent).
const TEST_PE_TEXT: [u8; 32] = [
    0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1      (SYS_WRITE_MARK)
    0x49, 0xC7, 0xC2, 0x5E, 0x00, 0x00, 0x00, // mov r10, 0x5E
    0x0F, 0x05, // syscall
    0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0      (SYS_EXIT)
    0x49, 0xC7, 0xC2, 0x00, 0x00, 0x00, 0x00, // mov r10, 0
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

/// A PE with one section at RVA 0x1000 holding an import table for
/// `ntdll.dll!<func>` (descriptor@0, INT@0x40, IAT@0x60). The section is writable
/// data when `iat_writable`, else read+execute code (so its IAT slot is not writable).
pub fn build_import_pe(func: &str, iat_writable: bool) -> alloc::vec::Vec<u8> {
    let mut b = alloc::vec![0u8; 0xC0];
    put(&mut b, 0, &0x1040u32.to_le_bytes()); // OriginalFirstThunk
    put(&mut b, 12, &0x1080u32.to_le_bytes()); // Name
    put(&mut b, 16, &0x1060u32.to_le_bytes()); // FirstThunk
    for base in [0x40, 0x60] {
        put(&mut b, base, &0x1090u64.to_le_bytes());
    }
    put(&mut b, 0x80, b"ntdll.dll\0");
    put(&mut b, 0x92, func.as_bytes()); // hint(2)@0x90 = 0, then the name + NUL
    let ch = if iat_writable {
        SCN_DATA_RW
    } else {
        SCN_CODE_EXEC_READ
    };
    let mut pe = build_pe(0x1000, &b, b.len() as u32, ch);
    // Data directory 1 (import): RVA 0x1000, size 0x28.
    let d = 0x44 + 20 + 112 + 8;
    put(&mut pe, d, &0x1000u32.to_le_bytes());
    put(&mut pe, d + 4, &0x28u32.to_le_bytes());
    pe
}

/// The embedded test PE bytes (built once per call and leaked; test use only).
pub fn test_pe_bytes() -> &'static [u8] {
    build_test_pe().leak()
}

/// Loads and starts the embedded test PE at its preferred base.
pub fn spawn_test_pe() -> usize {
    spawn_pe(&build_test_pe(), 0x40_0000)
}

/// Machine code of the TEB probe: reads gs:[0x30] (NtTib.Self), reports it via
/// SYS_WRITE_MARK, then exits.
const TEB_PROBE_TEXT: [u8; 39] = [
    0x65, 0x48, 0x8B, 0x04, 0x25, 0x30, 0x00, 0x00, 0x00, // mov rax, gs:[0x30]
    0x49, 0x89, 0xC2, // mov r10, rax
    0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1 (SYS_WRITE_MARK)
    0x0F, 0x05, // syscall
    0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0 (SYS_EXIT)
    0x49, 0xC7, 0xC2, 0x00, 0x00, 0x00, 0x00, // mov r10, 0
    0x0F, 0x05, // syscall
    0x0F, 0x0B, // ud2
];

/// Machine code of `nt_hello`: the real 9-parameter NtWriteFile
/// (CONSOLE_HANDLE, 0, 0, 0, &iosb, msg, 6, 0, 0), then reports
/// `iosb.Information` via SYS_WRITE_MARK and ends with NtTerminateProcess(0).
/// Stack frame (rsp after `sub rsp, 0x68`): args 5..9 at +0x28..+0x48, the
/// 16-byte IO_STATUS_BLOCK at +0x50 (Information at +0x58, pre-set to 0x77 so a
/// missing write is detectable). `msg` sits right after the code (offset 107);
/// the lea displacement is 107 - 21 (end of the lea) = 86 = 0x56.
const NT_HELLO_TEXT: [u8; 113] = [
    0x48, 0x83, 0xEC, 0x68, // sub rsp, 0x68
    0x48, 0x8D, 0x44, 0x24, 0x50, // lea rax, [rsp+0x50]      (&iosb)
    0x48, 0x89, 0x44, 0x24, 0x28, // mov [rsp+0x28], rax      (IoStatusBlock)
    0x48, 0x8D, 0x05, 0x56, 0x00, 0x00, 0x00, // lea rax, [rip+0x56] -> msg
    0x48, 0x89, 0x44, 0x24, 0x30, // mov [rsp+0x30], rax      (Buffer)
    0x48, 0xC7, 0x44, 0x24, 0x38, 0x06, 0x00, 0x00, 0x00, // [rsp+0x38] = 6 (Length)
    0x48, 0xC7, 0x44, 0x24, 0x40, 0x00, 0x00, 0x00, 0x00, // [rsp+0x40] = 0 (ByteOffset)
    0x48, 0xC7, 0x44, 0x24, 0x48, 0x00, 0x00, 0x00, 0x00, // [rsp+0x48] = 0 (Key)
    0x48, 0xC7, 0x44, 0x24, 0x58, 0x77, 0x00, 0x00, 0x00, // iosb.Information = 0x77
    0x41, 0xBA, 0x04, 0x00, 0x00, 0x00, // mov r10d, 4 (CONSOLE_HANDLE)
    0x31, 0xD2, // xor edx, edx
    0x45, 0x31, 0xC0, // xor r8d, r8d
    0x45, 0x31, 0xC9, // xor r9d, r9d
    0xB8, 0x02, 0x00, 0x00, 0x00, // mov eax, 2 (NT_WRITE_FILE)
    0x0F, 0x05, // syscall
    0x4C, 0x8B, 0x54, 0x24, 0x58, // mov r10, [rsp+0x58]  (iosb.Information)
    0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_WRITE_MARK)
    0x0F, 0x05, // syscall
    0xB8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3 (NT_TERMINATE_PROCESS)
    0x45, 0x31, 0xD2, // xor r10d, r10d
    0x0F, 0x05, // syscall
    0x0F, 0x0B, // ud2
    b'T', b'u', b'k', b'O', b'S', b'\n', // msg
];

/// Starts a PE that prints "TukOS" via the full NtWriteFile signature and ends
/// via NtTerminateProcess.
pub fn spawn_nt_hello_pe() -> usize {
    let pe = build_pe(
        0x1000,
        &NT_HELLO_TEXT,
        NT_HELLO_TEXT.len() as u32,
        SCN_CODE_EXEC_READ,
    );
    spawn_pe_inner(&pe, PE_IMAGE_BASE).0
}

/// Alias used by the IO_STATUS_BLOCK test: same payload as `nt_hello`.
pub fn spawn_ntwrite_real_pe() -> usize {
    spawn_nt_hello_pe()
}

/// Machine code of the SYS_SUM6 probe (Windows x64 syscall ABI): r10=1, rdx=2,
/// r8=3, r9=4 and 5, 6 at [rsp+0x28], [rsp+0x30]; then NtTerminateProcess(0).
const SUM6_TEXT: [u8; 65] = [
    0x48, 0x83, 0xEC, 0x38, // sub rsp, 0x38
    0x48, 0xC7, 0x44, 0x24, 0x28, 0x05, 0x00, 0x00, 0x00, // mov qword [rsp+0x28], 5
    0x48, 0xC7, 0x44, 0x24, 0x30, 0x06, 0x00, 0x00, 0x00, // mov qword [rsp+0x30], 6
    0x49, 0xC7, 0xC2, 0x01, 0x00, 0x00, 0x00, // mov r10, 1
    0xBA, 0x02, 0x00, 0x00, 0x00, // mov edx, 2
    0x41, 0xB8, 0x03, 0x00, 0x00, 0x00, // mov r8d, 3
    0x41, 0xB9, 0x04, 0x00, 0x00, 0x00, // mov r9d, 4
    0xB8, 0x64, 0x00, 0x00, 0x00, // mov eax, 100 (SYS_SUM6)
    0x0F, 0x05, // syscall
    0xB8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3 (NT_TERMINATE_PROCESS)
    0x45, 0x31, 0xD2, // xor r10d, r10d
    0x0F, 0x05, // syscall
    0x0F, 0x0B, // ud2
];

/// Starts a PE that calls SYS_SUM6 with 4 register + 2 stack arguments.
pub fn spawn_sum6_pe() -> usize {
    let pe = build_pe(
        0x1000,
        &SUM6_TEXT,
        SUM6_TEXT.len() as u32,
        SCN_CODE_EXEC_READ,
    );
    spawn_pe_inner(&pe, PE_IMAGE_BASE).0
}

/// Starts a PE whose code reports its own TEB address; returns that TEB VA.
pub fn spawn_teb_probe_pe() -> u64 {
    let pe = build_pe(
        0x1000,
        &TEB_PROBE_TEXT,
        TEB_PROBE_TEXT.len() as u32,
        SCN_CODE_EXEC_READ,
    );
    spawn_pe_inner(&pe, PE_IMAGE_BASE).1
}

/// Loads `image` at `load_base` into a fresh user space, maps a user stack and
/// schedules a user thread at the entry point.
pub fn spawn_pe(image: &[u8], load_base: u64) -> usize {
    spawn_pe_inner(image, load_base).0
}

/// Returns (thread id, TEB VA).
fn spawn_pe_inner(image: &[u8], load_base: u64) -> (usize, u64) {
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
    // TEB/PEB pages sit above the stack page, one guard page apart.
    let teb_va = stack_va + 2 * 4096;
    let peb_va = stack_va + 3 * 4096;
    crate::teb::setup(&space, teb_va, peb_va, load_base, stack_va + 4096, stack_va)
        .expect("teb setup");
    let kstack = alloc::vec![0u8; KSTACK_SIZE].into_boxed_slice();
    let id = crate::sched::spawn_user(space, entry, stack_va + 4096, kstack, teb_va);
    (id, teb_va)
}

/// Builds the TukOS test `.exe` that imports `ntdll.dll!NtWriteFile` and
/// `NtTerminateProcess` and calls both through the IAT.
///
/// Layout: `.text` RVA 0x1000 (R+X), `.data` RVA 0x2000 (R+W) holding, at
/// data offsets: import descriptors 0x00 (+null terminator), INT 0x40, IAT 0x60
/// (slot0 NtWriteFile @0x60, slot1 NtTerminateProcess @0x68), "ntdll.dll" 0x80,
/// hint/name entries 0x90 and 0xA0, msg 0xC0 and IO_STATUS_BLOCK 0xD0.
pub fn build_exe_importing_ntdll() -> alloc::vec::Vec<u8> {
    const OPT_SIZE: usize = 112 + 16 * 8;
    const TEXT_RVA: u32 = 0x1000;
    const DATA_RVA: u32 = 0x2000;
    const DATA_RAW_OFF: usize = 0x400;
    let slot_write = DATA_RVA + 0x60;
    let slot_term = DATA_RVA + 0x68;
    let msg_rva = DATA_RVA + 0xC0;
    let iosb_rva = DATA_RVA + 0xD0;

    // Appends an instruction ending in a rip-relative disp32 and patches it:
    // disp = target_rva - rva_of_next_instruction.
    fn rip_rel(code: &mut alloc::vec::Vec<u8>, opcode: &[u8], target: u32) {
        code.extend_from_slice(opcode);
        let next = TEXT_RVA + code.len() as u32 + 4;
        code.extend_from_slice(&(target.wrapping_sub(next) as i32).to_le_bytes());
    }

    let mut c = alloc::vec::Vec::new();
    c.extend_from_slice(&[0x48, 0x83, 0xEC, 0x50]); // sub rsp, 0x50 (32 shadow + 5 args, aligned)
    rip_rel(&mut c, &[0x48, 0x8D, 0x05], iosb_rva); // lea rax, [rip+iosb]
    c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x20]); // [rsp+0x20] = IoStatusBlock
    rip_rel(&mut c, &[0x48, 0x8D, 0x05], msg_rva); // lea rax, [rip+msg]
    c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x28]); // [rsp+0x28] = Buffer
    c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x30, 6, 0, 0, 0]); // Length = 6
    c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x38, 0, 0, 0, 0]); // ByteOffset = 0
    c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x40, 0, 0, 0, 0]); // Key = 0
    c.extend_from_slice(&[0xB9, 4, 0, 0, 0]); // mov ecx, CONSOLE_HANDLE
    c.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    c.extend_from_slice(&[0x45, 0x31, 0xC0]); // xor r8d, r8d
    c.extend_from_slice(&[0x45, 0x31, 0xC9]); // xor r9d, r9d
    rip_rel(&mut c, &[0xFF, 0x15], slot_write); // call [rip+IAT NtWriteFile]
    c.extend_from_slice(&[0x31, 0xC9]); // xor ecx, ecx
    rip_rel(&mut c, &[0xFF, 0x15], slot_term); // call [rip+IAT NtTerminateProcess]
    c.extend_from_slice(&[0x0F, 0x0B]); // ud2

    let mut d = alloc::vec![0u8; 0xE0];
    put(&mut d, 0, &(DATA_RVA + 0x40).to_le_bytes()); // OriginalFirstThunk
    put(&mut d, 12, &(DATA_RVA + 0x80).to_le_bytes()); // Name
    put(&mut d, 16, &(DATA_RVA + 0x60).to_le_bytes()); // FirstThunk
    for base in [0x40usize, 0x60] {
        put(&mut d, base, &u64::from(DATA_RVA + 0x90).to_le_bytes());
        put(&mut d, base + 8, &u64::from(DATA_RVA + 0xA0).to_le_bytes());
    }
    put(&mut d, 0x80, b"ntdll.dll\0");
    put(&mut d, 0x92, b"NtWriteFile\0");
    put(&mut d, 0xA2, b"NtTerminateProcess\0");
    put(&mut d, 0xC0, b"TukOS\n");

    let text_raw = PE_RAW_OFF;
    let mut v = alloc::vec![0u8; DATA_RAW_OFF + d.len()];
    put(&mut v, 0, &0x5A4Du16.to_le_bytes());
    put(&mut v, 0x3C, &0x40u32.to_le_bytes());
    put(&mut v, 0x40, &0x0000_4550u32.to_le_bytes());
    let coff = 0x44;
    put(&mut v, coff, &0x8664u16.to_le_bytes());
    put(&mut v, coff + 2, &2u16.to_le_bytes());
    put(&mut v, coff + 16, &(OPT_SIZE as u16).to_le_bytes());
    let opt = coff + 20;
    put(&mut v, opt, &0x20Bu16.to_le_bytes());
    put(&mut v, opt + 16, &TEXT_RVA.to_le_bytes());
    put(&mut v, opt + 24, &PE_IMAGE_BASE.to_le_bytes());
    put(&mut v, opt + 56, &0x3000u32.to_le_bytes());
    put(&mut v, opt + 108, &16u32.to_le_bytes());
    put(&mut v, opt + 112 + 8, &DATA_RVA.to_le_bytes()); // import dir
    put(&mut v, opt + 112 + 12, &0x28u32.to_le_bytes());
    let s = opt + OPT_SIZE;
    put(&mut v, s, b".text\0\0\0");
    put(&mut v, s + 8, &(c.len() as u32).to_le_bytes());
    put(&mut v, s + 12, &TEXT_RVA.to_le_bytes());
    put(&mut v, s + 16, &(c.len() as u32).to_le_bytes());
    put(&mut v, s + 20, &(text_raw as u32).to_le_bytes());
    put(&mut v, s + 36, &SCN_CODE_EXEC_READ.to_le_bytes());
    let s2 = s + 40;
    put(&mut v, s2, b".data\0\0\0");
    put(&mut v, s2 + 8, &(d.len() as u32).to_le_bytes());
    put(&mut v, s2 + 12, &DATA_RVA.to_le_bytes());
    put(&mut v, s2 + 16, &(d.len() as u32).to_le_bytes());
    put(&mut v, s2 + 20, &(DATA_RAW_OFF as u32).to_le_bytes());
    put(&mut v, s2 + 36, &SCN_DATA_RW.to_le_bytes());
    put(&mut v, text_raw, &c);
    put(&mut v, DATA_RAW_OFF, &d);
    v
}

const NTDLL_LOAD_BASE: u64 = 0x1000_0000;

/// A process space holding the test `.exe` + ntdll, with stack/TEB/PEB and
/// `PEB->Ldr` built. The thread is not started yet.
pub struct ExeSpace {
    pub space: AddressSpace,
    pub exe: alloc::vec::Vec<u8>,
    pub exe_entry: u64,
    pub stack_va: u64,
    pub teb_va: u64,
    pub peb_va: u64,
}

pub const EXE_BASE: u64 = PE_IMAGE_BASE;
pub const NTDLL_BASE: u64 = NTDLL_LOAD_BASE;

/// Loads exe (0x40_0000) and ntdll (0x1000_0000), maps stack/TEB/PEB and the
/// loader-data page, and fills `PEB->Ldr` (exe first, then ntdll). Layout above
/// the ntdll image: guard, stack, guard, TEB, PEB, ldr page.
/// Requires the ntdll to be embedded.
pub fn build_exe_ntdll_space() -> ExeSpace {
    let nt = crate::ntdll_image::NTDLL_BYTES;
    let exe = build_exe_importing_ntdll();
    let space = AddressSpace::new_user();
    let exe_entry = crate::loader::load_pe(&space, &exe, PE_IMAGE_BASE).expect("load exe");
    let ntdll_entry = crate::loader::load_pe(&space, nt, NTDLL_LOAD_BASE).expect("load ntdll");
    let exe_top = crate::loader::image_top(&exe, PE_IMAGE_BASE).expect("image_top");
    let nt_top = crate::loader::image_top(nt, NTDLL_LOAD_BASE).expect("image_top");
    let exe_size = (exe_top - PE_IMAGE_BASE) as u32;
    let nt_size = (nt_top - NTDLL_LOAD_BASE) as u32;
    let stack_va = nt_top + 4096;
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
    let teb_va = stack_va + 2 * 4096;
    let peb_va = stack_va + 3 * 4096;
    let ldr_va = stack_va + 4 * 4096;
    crate::teb::setup(
        &space,
        teb_va,
        peb_va,
        PE_IMAGE_BASE,
        stack_va + 4096,
        stack_va,
    )
    .expect("teb setup");
    crate::teb::setup_ldr(
        &space,
        ldr_va,
        peb_va,
        &[
            crate::teb::LdrModule {
                base: PE_IMAGE_BASE,
                entry: exe_entry,
                size: exe_size,
                name: "program.exe",
            },
            crate::teb::LdrModule {
                base: NTDLL_LOAD_BASE,
                entry: ntdll_entry,
                size: nt_size,
                name: "ntdll.dll",
            },
        ],
    )
    .expect("ldr setup");
    ExeSpace {
        space,
        exe,
        exe_entry,
        stack_va,
        teb_va,
        peb_va,
    }
}

/// Loads the test `.exe` plus the embedded ntdll and starts a user thread.
/// The thread starts at ntdll's `LdrInitializeThunk`, which walks `PEB->Ldr`.
/// Does nothing when the ntdll is not embedded.
pub fn spawn_exe_with_ntdll() {
    let nt = crate::ntdll_image::NTDLL_BYTES;
    if nt.is_empty() {
        return;
    }
    let es = build_exe_ntdll_space();
    let thunk = pe::PeFile::parse(nt)
        .ok()
        .and_then(|p| p.export_rva_by_name("LdrInitializeThunk"));
    let start = match thunk {
        Some(rva) => NTDLL_LOAD_BASE + u64::from(rva),
        None => {
            // FALLBACK (until Task 3 adds LdrInitializeThunk to the ntdll): the
            // kernel resolves the exe's imports itself and starts at the exe entry.
            crate::loader::resolve_imports(&es.space, &es.exe, PE_IMAGE_BASE, nt, NTDLL_LOAD_BASE)
                .expect("resolve imports");
            es.exe_entry
        }
    };
    let kstack = alloc::vec![0u8; KSTACK_SIZE].into_boxed_slice();
    crate::sched::spawn_user(es.space, start, es.stack_va + 4096, kstack, es.teb_va);
}
