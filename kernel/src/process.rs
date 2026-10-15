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

/// Test PE whose entry derefs unmapped user VA 0x1 (`mov rax, [0x1]`): a
/// deterministic Ring-3 #PF (Plan 14 regression).
#[cfg(feature = "ktest")]
pub fn spawn_faulting_pe() -> usize {
    const TEXT: [u8; 10] = [
        0x48, 0x8B, 0x04, 0x25, 0x01, 0x00, 0x00, 0x00, // mov rax, [0x1]
        0x0F, 0x0B, // ud2 (never reached)
    ];
    let pe = build_pe(0x1000, &TEXT, TEXT.len() as u32, SCN_CODE_EXEC_READ);
    spawn_pe(&pe, PE_IMAGE_BASE)
}

/// Test PE whose entry is `ud2`: a Ring-3 #UD (vector 6).
#[cfg(feature = "ktest")]
pub fn spawn_ud_pe() -> usize {
    const TEXT: [u8; 2] = [0x0F, 0x0B]; // ud2
    let pe = build_pe(0x1000, &TEXT, TEXT.len() as u32, SCN_CODE_EXEC_READ);
    spawn_pe(&pe, PE_IMAGE_BASE)
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
    build_exe_importing_ntdll_inner(false)
}

/// Like `build_exe_importing_ntdll`, but the first import's IAT slot (FirstThunk)
/// points INTO the read+execute `.text` section: a malicious/corrupt exe that tries
/// to make the loader write into code. The user-mode loader must refuse it.
pub fn build_exe_with_bad_iat() -> alloc::vec::Vec<u8> {
    build_exe_importing_ntdll_inner(true)
}

fn build_exe_importing_ntdll_inner(bad_iat: bool) -> alloc::vec::Vec<u8> {
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
    // FirstThunk: the real IAT, or (bad_iat) the start of the code section.
    let first_thunk = if bad_iat { TEXT_RVA } else { DATA_RVA + 0x60 };
    put(&mut d, 16, &first_thunk.to_le_bytes());
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
    put(&mut v, opt + 60, &(PE_RAW_OFF as u32).to_le_bytes()); // SizeOfHeaders
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
/// Extra stack pages (below the top page) of the multi-module process spaces.
const STACK_EXTRA_PAGES: u64 = 15;

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
    build_exe_ntdll_space_with(build_exe_importing_ntdll())
}

/// Same as `build_exe_ntdll_space` for an arbitrary test `.exe`.
pub fn build_exe_ntdll_space_with(exe: alloc::vec::Vec<u8>) -> ExeSpace {
    build_space(exe, false, &[])
}

const KERNEL32_LOAD_BASE: u64 = 0x0800_0000;
pub const KERNEL32_BASE: u64 = KERNEL32_LOAD_BASE;

/// Like `build_exe_ntdll_space_with` but maps three modules (exe 0x40_0000,
/// kernel32 0x0800_0000, ntdll 0x1000_0000) and lists them in load order
/// exe, kernel32, ntdll. Requires both DLLs to be embedded.
pub fn build_exe_k32_space_with(exe: alloc::vec::Vec<u8>) -> ExeSpace {
    build_space(exe, true, &[])
}

/// `build_exe_k32_space_with` with an explicit command line (UTF-16, no NUL) stored in
/// `PEB->ProcessParameters->CommandLine`. Empty = default image name.
pub fn build_exe_k32_space_with_cmdline(exe: alloc::vec::Vec<u8>, cmdline: &[u16]) -> ExeSpace {
    build_space(exe, true, cmdline)
}

fn build_space(exe: alloc::vec::Vec<u8>, with_k32: bool, cmdline: &[u16]) -> ExeSpace {
    let nt = crate::ntdll_image::NTDLL_BYTES;
    let k32 = crate::kernel32_image::KERNEL32_BYTES;
    let space = AddressSpace::new_user();
    let exe_entry = crate::loader::load_pe(&space, &exe, PE_IMAGE_BASE).expect("load exe");
    let exe_top = crate::loader::image_top(&exe, PE_IMAGE_BASE).expect("image_top");
    let mut mods = alloc::vec::Vec::new();
    mods.push(crate::teb::LdrModule {
        base: PE_IMAGE_BASE,
        entry: exe_entry,
        size: (exe_top - PE_IMAGE_BASE) as u32,
        name: "program.exe",
    });
    let mut prev_top = exe_top;
    if with_k32 {
        let entry = crate::loader::load_pe(&space, k32, KERNEL32_LOAD_BASE).expect("load kernel32");
        let top = crate::loader::image_top(k32, KERNEL32_LOAD_BASE).expect("image_top");
        // Images must not overlap: exe < kernel32 < ntdll.
        assert!(prev_top <= KERNEL32_LOAD_BASE, "exe invade o kernel32");
        mods.push(crate::teb::LdrModule {
            base: KERNEL32_LOAD_BASE,
            entry,
            size: (top - KERNEL32_LOAD_BASE) as u32,
            name: "kernel32.dll",
        });
        prev_top = top;
    }
    let ntdll_entry = crate::loader::load_pe(&space, nt, NTDLL_LOAD_BASE).expect("load ntdll");
    let nt_top = crate::loader::image_top(nt, NTDLL_LOAD_BASE).expect("image_top");
    assert!(prev_top <= NTDLL_LOAD_BASE, "modulo invade o ntdll");
    mods.push(crate::teb::LdrModule {
        base: NTDLL_LOAD_BASE,
        entry: ntdll_entry,
        size: (nt_top - NTDLL_LOAD_BASE) as u32,
        name: "ntdll.dll",
    });
    // The initial stack is STACK_EXTRA_PAGES + 1 pages (the MSVC CRT startup needs more
    // than one): the extra pages sit right below `stack_va` (the top page), and the
    // unmapped page under them is the guard.
    let stack_va = nt_top + 4096 + STACK_EXTRA_PAGES * 4096;
    for i in 0..=STACK_EXTRA_PAGES {
        let va = stack_va - (STACK_EXTRA_PAGES - i) * 4096;
        let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha");
        unsafe {
            space
                .map(
                    va,
                    stack_phys,
                    PageFlags::USER
                        .union(PageFlags::WRITABLE)
                        .union(PageFlags::NO_EXECUTE),
                )
                .expect("map pilha");
        }
    }
    let teb_va = stack_va + 2 * 4096;
    let peb_va = stack_va + 3 * 4096;
    let ldr_va = stack_va + 4 * 4096;
    let params_va = stack_va + 5 * 4096;
    crate::teb::setup(
        &space,
        teb_va,
        peb_va,
        PE_IMAGE_BASE,
        stack_va + 4096,
        stack_va - STACK_EXTRA_PAGES * 4096,
    )
    .expect("teb setup");
    crate::teb::setup_ldr(&space, ldr_va, peb_va, &mods).expect("ldr setup");
    crate::teb::setup_process_parameters(
        &space,
        params_va,
        peb_va,
        crate::object::CONSOLE_HANDLE,
        cmdline,
    )
    .expect("process parameters setup");
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
    spawn_exe_image_with_ntdll(build_exe_importing_ntdll());
}

/// True when the embedded ntdll exports `LdrInitializeThunk`.
pub fn ntdll_has_thunk() -> bool {
    pe::PeFile::parse(crate::ntdll_image::NTDLL_BYTES)
        .ok()
        .and_then(|p| p.export_rva_by_name("LdrInitializeThunk"))
        .is_some()
}

/// Starts `exe` (loaded with the embedded ntdll) on a new user thread.
pub fn spawn_exe_image_with_ntdll(exe: alloc::vec::Vec<u8>) {
    let nt = crate::ntdll_image::NTDLL_BYTES;
    if nt.is_empty() {
        return;
    }
    let es = build_exe_ntdll_space_with(exe);
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

/// Starts `exe` loaded with the embedded kernel32 AND ntdll (three modules) on a new
/// user thread that begins at ntdll's `LdrInitializeThunk`. Does nothing when either
/// DLL is not embedded.
pub fn spawn_exe_image_with_k32(exe: alloc::vec::Vec<u8>) {
    let _ = spawn_exe_image_with_k32_exit(exe, None);
}

/// Why `spawn_exe_image_with_k32_exit` did not start a process.
#[derive(Debug, PartialEq)]
pub enum SpawnError {
    /// kernel32/ntdll are not embedded in this kernel build.
    NoDlls,
    /// The scheduler's thread table is full.
    NoThreads,
}

/// `spawn_exe_image_with_k32` that also attaches `proc_exit` to the new thread (the
/// child records its exit status there, see `sched::exit_current_with`). The child
/// gets its own address space and handle table; only the console handles 4/8 exist.
pub fn spawn_exe_image_with_k32_exit(
    exe: alloc::vec::Vec<u8>,
    proc_exit: Option<alloc::sync::Arc<crate::object::ProcExit>>,
) -> Result<(), SpawnError> {
    spawn_exe_image_with_k32_cmd(exe, proc_exit, &[])
}

/// `spawn_exe_image_with_k32_exit` plus the child's command line (UTF-16, no NUL;
/// empty = default image name).
pub fn spawn_exe_image_with_k32_cmd(
    exe: alloc::vec::Vec<u8>,
    proc_exit: Option<alloc::sync::Arc<crate::object::ProcExit>>,
    cmdline: &[u16],
) -> Result<(), SpawnError> {
    let nt = crate::ntdll_image::NTDLL_BYTES;
    if nt.is_empty() || crate::kernel32_image::KERNEL32_BYTES.is_empty() {
        return Err(SpawnError::NoDlls);
    }
    if !crate::sched::has_thread_capacity() {
        return Err(SpawnError::NoThreads);
    }
    let es = build_exe_k32_space_with_cmdline(exe, cmdline);
    let rva = pe::PeFile::parse(nt)
        .ok()
        .and_then(|p| p.export_rva_by_name("LdrInitializeThunk"))
        .expect("ntdll sem LdrInitializeThunk");
    let kstack = alloc::vec![0u8; KSTACK_SIZE].into_boxed_slice();
    crate::sched::spawn_user_with_exit(
        es.space,
        NTDLL_LOAD_BASE + u64::from(rva),
        es.stack_va + 4096,
        kstack,
        es.teb_va,
        proc_exit,
    )
    .map(|_| ())
    .ok_or(SpawnError::NoThreads)
}

/// Test `.exe` that dereferences unmapped VA 1 right at its entry (Ring-3 #PF).
pub fn build_exe_faults() -> alloc::vec::Vec<u8> {
    build_import_exe("kernel32.dll", &["ExitProcess"], None, None, |_, _| {
        alloc::vec![
            0x48, 0x8B, 0x04, 0x25, 0x01, 0x00, 0x00, 0x00, // mov rax, [0x1]
            0x0F, 0x0B, // ud2 (never reached)
        ]
    })
}

const TEXT_RVA: u32 = 0x1000;
const DATA_RVA: u32 = 0x2000;
const DATA_RAW_OFF: usize = 0x1000;
/// Offset of the "TukOS\n" message in the `.data` of `build_import_exe`.
const IMPORT_EXE_MSG_OFF: u32 = 0x1C0;
/// `.data` offsets of `build_import_exe`: INT, IAT (8 slots + null), DLL name, hint/names.
const IMP_INT: usize = 0x40;
const IMP_IAT: usize = 0x90;
const IMP_NAME: usize = 0xE0;
const IMP_HINTS: usize = 0xF0;

/// Appends `opcode` + a rip-relative disp32 aimed at `target_rva` (the exe's `.text`
/// sits at `TEXT_RVA`).
fn rip_rel_to(code: &mut alloc::vec::Vec<u8>, opcode: &[u8], target_rva: u32) {
    code.extend_from_slice(opcode);
    let next = TEXT_RVA + code.len() as u32 + 4;
    code.extend_from_slice(&(target_rva.wrapping_sub(next) as i32).to_le_bytes());
}

/// Builds a two-section test `.exe` (`.text` R+X at 0x1000, `.data` R+W at 0x2000)
/// importing `funcs` (at most 8, by name) from `dll`. `.data` holds: descriptor 0x00,
/// INT 0x40, IAT 0x90 (slot i at 0x90+8i), DLL name 0xE0, hint/name entries from
/// 0xF0, message 0x1C0. `first_thunk` overrides the descriptor's FirstThunk RVA and
/// `dir12` sets data directory #12 (both used to craft hostile exes). `code` gets the
/// IAT slot RVAs and the message RVA and returns the `.text` bytes (padded to 32).
fn build_import_exe(
    dll: &str,
    funcs: &[&str],
    first_thunk: Option<u32>,
    dir12: Option<(u32, u32)>,
    code: impl FnOnce(&[u32], u32) -> alloc::vec::Vec<u8>,
) -> alloc::vec::Vec<u8> {
    const OPT_SIZE: usize = 112 + 16 * 8;
    assert!(funcs.len() <= 8 && dll.len() < 15);
    let slots: alloc::vec::Vec<u32> = (0..funcs.len() as u32)
        .map(|i| DATA_RVA + IMP_IAT as u32 + 8 * i)
        .collect();
    let mut c = code(&slots, DATA_RVA + IMPORT_EXE_MSG_OFF);
    while c.len() < 32 {
        c.push(0xCC);
    }

    let mut d = alloc::vec![0u8; IMPORT_EXE_MSG_OFF as usize + 0x20];
    put(&mut d, 0, &(DATA_RVA + IMP_INT as u32).to_le_bytes()); // OriginalFirstThunk
    put(&mut d, 12, &(DATA_RVA + IMP_NAME as u32).to_le_bytes()); // Name
    let ft = first_thunk.unwrap_or(DATA_RVA + IMP_IAT as u32);
    put(&mut d, 16, &ft.to_le_bytes()); // FirstThunk
    put(&mut d, IMP_NAME, dll.as_bytes()); // NUL already there (zeroed)
    let mut hn = IMP_HINTS;
    for (i, f) in funcs.iter().enumerate() {
        assert!(hn + 2 + f.len() < IMPORT_EXE_MSG_OFF as usize);
        let rva = u64::from(DATA_RVA) + hn as u64;
        put(&mut d, IMP_INT + 8 * i, &rva.to_le_bytes()); // INT
        put(&mut d, IMP_IAT + 8 * i, &rva.to_le_bytes()); // IAT (pre-bind = INT)
        put(&mut d, hn + 2, f.as_bytes()); // hint(2)=0, name, NUL
        hn += (2 + f.len() + 1 + 1) & !1;
    }
    put(&mut d, IMPORT_EXE_MSG_OFF as usize, b"TukOS\n");

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
    put(&mut v, opt + 60, &(PE_RAW_OFF as u32).to_le_bytes());
    put(&mut v, opt + 108, &16u32.to_le_bytes());
    put(&mut v, opt + 112 + 8, &DATA_RVA.to_le_bytes()); // import dir
    put(&mut v, opt + 112 + 12, &0x28u32.to_le_bytes());
    if let Some((rva, size)) = dir12 {
        put(&mut v, opt + 112 + 12 * 8, &rva.to_le_bytes());
        put(&mut v, opt + 112 + 12 * 8 + 4, &size.to_le_bytes());
    }
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

/// Test `.exe` importing `kernel32.dll!{GetStdHandle, WriteFile, ExitProcess}`:
/// `WriteFile(GetStdHandle(-11), "TukOS\n", 6, &n, 0)` then `ExitProcess(0)`.
pub fn build_exe_importing_kernel32() -> alloc::vec::Vec<u8> {
    build_import_exe(
        "kernel32.dll",
        &["GetStdHandle", "WriteFile", "ExitProcess"],
        None,
        None,
        |slot, msg| {
            let mut c = alloc::vec::Vec::new();
            c.extend_from_slice(&[0x48, 0x83, 0xEC, 0x38]); // sub rsp, 0x38
            c.extend_from_slice(&[0xB9, 0xF5, 0xFF, 0xFF, 0xFF]); // mov ecx, -11
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [GetStdHandle]
            c.extend_from_slice(&[0x48, 0x89, 0xC1]); // mov rcx, rax
            rip_rel_to(&mut c, &[0x48, 0x8D, 0x15], msg); // lea rdx, [msg]
            c.extend_from_slice(&[0x41, 0xB8, 6, 0, 0, 0]); // mov r8d, 6
            c.extend_from_slice(&[0x4C, 0x8D, 0x4C, 0x24, 0x30]); // lea r9, [rsp+0x30] (&n)
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 0, 0, 0, 0]); // [rsp+0x20]=0
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[1]); // call [WriteFile]
            c.extend_from_slice(&[0x31, 0xC9]); // xor ecx, ecx
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[2]); // call [ExitProcess]
            c.extend_from_slice(&[0x0F, 0x0B]); // ud2
            c
        },
    )
}

/// Test `.exe` importing kernel32 only: `ReadFile(GetStdHandle(-10), buf, 32, &n, 0)`
/// then `WriteFile(GetStdHandle(-11), buf, n, &n, 0)` (so the kernel's LAST_WRITE
/// shows exactly what the read returned) and `ExitProcess(0)`.
pub fn build_exe_reads_stdin() -> alloc::vec::Vec<u8> {
    build_import_exe(
        "kernel32.dll",
        &["GetStdHandle", "ReadFile", "WriteFile", "ExitProcess"],
        None,
        None,
        |slot, _| {
            let mut c = alloc::vec::Vec::new();
            c.extend_from_slice(&[0x48, 0x83, 0xEC, 0x68]); // sub rsp, 0x68
            c.extend_from_slice(&[0xB9, 0xF6, 0xFF, 0xFF, 0xFF]); // mov ecx, -10
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [GetStdHandle]
            c.extend_from_slice(&[0x48, 0x89, 0xC1]); // mov rcx, rax
            c.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x40]); // lea rdx, [rsp+0x40] (buf)
            c.extend_from_slice(&[0x41, 0xB8, 32, 0, 0, 0]); // mov r8d, 32
            c.extend_from_slice(&[0x4C, 0x8D, 0x4C, 0x24, 0x30]); // lea r9, [rsp+0x30] (&n)
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 0, 0, 0, 0]); // [rsp+0x20]=0
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[1]); // call [ReadFile]
            c.extend_from_slice(&[0xB9, 0xF5, 0xFF, 0xFF, 0xFF]); // mov ecx, -11
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [GetStdHandle]
            c.extend_from_slice(&[0x48, 0x89, 0xC1]); // mov rcx, rax
            c.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x40]); // lea rdx, [rsp+0x40]
            c.extend_from_slice(&[0x44, 0x8B, 0x44, 0x24, 0x30]); // mov r8d, [rsp+0x30]
            c.extend_from_slice(&[0x4C, 0x8D, 0x4C, 0x24, 0x30]); // lea r9, [rsp+0x30]
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 0, 0, 0, 0]); // [rsp+0x20]=0
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[2]); // call [WriteFile]
            c.extend_from_slice(&[0x31, 0xC9]); // xor ecx, ecx
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[3]); // call [ExitProcess]
            c.extend_from_slice(&[0x0F, 0x0B]); // ud2
            c
        },
    )
}

/// Test `.exe` (kernel32 only) that reads `C:\cat.txt` from the initrd: `CreateFileW`
/// (GENERIC_READ, OPEN_EXISTING), checks `GetFileType == FILE_TYPE_DISK` and
/// `GetFileSizeEx == 26`, then loops `ReadFile(32)` -> `WriteFile(stdout)` until a read
/// returns 0 bytes, `CloseHandle`, `ExitProcess(0)`. Any failed check exits 0xBAD.
/// The UTF-16 path lives in `.text` after the code.
pub fn build_exe_reads_file() -> alloc::vec::Vec<u8> {
    build_import_exe(
        "kernel32.dll",
        &[
            "GetStdHandle",
            "CreateFileW",
            "GetFileType",
            "GetFileSizeEx",
            "ReadFile",
            "WriteFile",
            "CloseHandle",
            "ExitProcess",
        ],
        None,
        None,
        |slot, _| {
            use alloc::vec::Vec;
            let mut c: Vec<u8> = Vec::new();
            let mut fails: Vec<usize> = Vec::new();
            c.extend_from_slice(&[0x48, 0x81, 0xEC, 0x88, 0, 0, 0]); // sub rsp, 0x88
            // h = CreateFileW(path, GENERIC_READ, 1, 0, OPEN_EXISTING, 0x80, 0)
            c.extend_from_slice(&[0x48, 0x8D, 0x0D]); // lea rcx,[rip+path] (patched)
            let lea_at = c.len();
            c.extend_from_slice(&[0, 0, 0, 0]);
            c.extend_from_slice(&[0xBA, 0, 0, 0, 0x80]); // mov edx, 0x80000000
            c.extend_from_slice(&[0x41, 0xB8, 1, 0, 0, 0]); // mov r8d, 1
            c.extend_from_slice(&[0x45, 0x31, 0xC9]); // xor r9d, r9d
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 3, 0, 0, 0]); // [rsp+20]=3
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x28, 0x80, 0, 0, 0]); // [rsp+28]=0x80
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x30, 0, 0, 0, 0]); // [rsp+30]=0
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[1]); // call [CreateFileW]
            c.extend_from_slice(&[0x48, 0x83, 0xF8, 0xFF]); // cmp rax, -1
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x40]); // mov [rsp+40], rax
            // GetFileType(h) must be FILE_TYPE_DISK (1)
            c.extend_from_slice(&[0x48, 0x89, 0xC1]); // mov rcx, rax
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[2]); // call [GetFileType]
            c.extend_from_slice(&[0x83, 0xF8, 0x01]); // cmp eax, 1
            c.extend_from_slice(&[0x0F, 0x85, 0, 0, 0, 0]); // jne fail
            fails.push(c.len() - 4);
            // GetFileSizeEx(h, &size @ [rsp+48]) must give 26
            c.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, 0x40]); // mov rcx, [rsp+40]
            c.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x48]); // lea rdx, [rsp+48]
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[3]); // call [GetFileSizeEx]
            c.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x48, 0x83, 0x7C, 0x24, 0x48, 26]); // cmp qword [rsp+48], 26
            c.extend_from_slice(&[0x0F, 0x85, 0, 0, 0, 0]); // jne fail
            fails.push(c.len() - 4);
            // loop: ReadFile(h, buf @ [rsp+50], 32, &n @ [rsp+38], 0)
            let loop_pos = c.len();
            c.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, 0x40]); // mov rcx, [rsp+40]
            c.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x50]); // lea rdx, [rsp+50]
            c.extend_from_slice(&[0x41, 0xB8, 32, 0, 0, 0]); // mov r8d, 32
            c.extend_from_slice(&[0x4C, 0x8D, 0x4C, 0x24, 0x38]); // lea r9, [rsp+38]
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[4]); // call [ReadFile]
            c.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x83, 0x7C, 0x24, 0x38, 0x00]); // cmp dword [rsp+38], 0
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je done
            let done_fix = c.len() - 4;
            // WriteFile(GetStdHandle(-11), buf, n, &w @ [rsp+3C], 0)
            c.extend_from_slice(&[0xB9, 0xF5, 0xFF, 0xFF, 0xFF]); // mov ecx, -11
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [GetStdHandle]
            c.extend_from_slice(&[0x48, 0x89, 0xC1]); // mov rcx, rax
            c.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x50]); // lea rdx, [rsp+50]
            c.extend_from_slice(&[0x44, 0x8B, 0x44, 0x24, 0x38]); // mov r8d, [rsp+38]
            c.extend_from_slice(&[0x4C, 0x8D, 0x4C, 0x24, 0x3C]); // lea r9, [rsp+3C]
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[5]); // call [WriteFile]
            c.push(0xE9); // jmp loop
            let rel = loop_pos as i32 - (c.len() as i32 + 4);
            c.extend_from_slice(&rel.to_le_bytes());
            // done: CloseHandle(h); ExitProcess(0)
            let done_pos = c.len();
            let rel = done_pos as i32 - (done_fix as i32 + 4);
            c[done_fix..done_fix + 4].copy_from_slice(&rel.to_le_bytes());
            c.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, 0x40]); // mov rcx, [rsp+40]
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[6]); // call [CloseHandle]
            c.extend_from_slice(&[0x31, 0xC9]); // xor ecx, ecx
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[7]); // call [ExitProcess]
            c.extend_from_slice(&[0x0F, 0x0B]); // ud2
            // fail: ExitProcess(0xBAD)
            let fail_pos = c.len();
            c.extend_from_slice(&[0xB9, 0xAD, 0x0B, 0, 0]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[7]);
            c.extend_from_slice(&[0x0F, 0x0B]);
            for f in fails {
                let rel = fail_pos as i32 - (f as i32 + 4);
                c[f..f + 4].copy_from_slice(&rel.to_le_bytes());
            }
            // path (UTF-16LE, NUL-terminated) after the code; patch the lea.
            while !c.len().is_multiple_of(2) {
                c.push(0xCC);
            }
            let path_pos = c.len();
            for ch in r"C:\cat.txt".encode_utf16().chain(core::iter::once(0)) {
                c.extend_from_slice(&ch.to_le_bytes());
            }
            let rel = path_pos as i32 - (lea_at as i32 + 4);
            c[lea_at..lea_at + 4].copy_from_slice(&rel.to_le_bytes());
            c
        },
    )
}

/// Test `.exe` that calls `kernel32!TukosCloseForward(0x1234)`, which kernel32
/// exports as a forwarder to `ntdll.NtClose` (1 arg -> 1 arg). The bogus handle makes
/// NtClose return STATUS_INVALID_HANDLE in rax; the exe then calls
/// `ExitProcess(eax)`, so that status reaches the kernel only if the loader chased
/// the forward to ntdll's real stub.
pub fn build_exe_calling_forwarder() -> alloc::vec::Vec<u8> {
    build_import_exe(
        "kernel32.dll",
        &["TukosCloseForward", "ExitProcess"],
        None,
        None,
        |slot, _| {
            let mut c = alloc::vec::Vec::new();
            c.extend_from_slice(&[0x48, 0x83, 0xEC, 0x28]); // sub rsp, 0x28
            c.extend_from_slice(&[0xB9, 0x34, 0x12, 0, 0]); // mov ecx, 0x1234
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [TukosCloseForward]
            c.extend_from_slice(&[0x89, 0xC1]); // mov ecx, eax
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[1]); // call [ExitProcess]
            c.extend_from_slice(&[0x0F, 0x0B]);
            c
        },
    )
}

/// Test `.exe` for the user heap (imports kernel32 only): `GetProcessHeap` (twice:
/// same handle), four `HeapAlloc`s (24, 100, 5000, 70000 bytes: the last forces a
/// second segment), a distinct fill byte in each, a read-back of every block (no
/// overlap), `HeapFree` of the first two, a `HeapAlloc(50)` that must REUSE the first
/// block's address, a `HeapAlloc(HEAP_ZERO_MEMORY, 60)` that must come back zeroed
/// although it reuses bytes the freed block dirtied, a final read-back of everything,
/// frees, then `WriteFile("TukOS\n")` and `ExitProcess(0)`. Any failure ends with
/// `ExitProcess(0xBAD)` without writing.
pub fn build_exe_heap_test() -> alloc::vec::Vec<u8> {
    use alloc::vec::Vec;
    const H: u8 = 0x30;
    const P0: u8 = 0x38;
    const P1: u8 = 0x40;
    const P2: u8 = 0x48;
    const P3: u8 = 0x50;
    const E: u8 = 0x58;
    const F: u8 = 0x60;
    const JE: u8 = 0x84;
    const JNE: u8 = 0x85;
    // jcc rel32 to the fail label (patched at the end).
    fn jfail(c: &mut Vec<u8>, fails: &mut Vec<usize>, op: u8) {
        c.extend_from_slice(&[0x0F, op, 0, 0, 0, 0]);
        fails.push(c.len() - 4);
    }
    // [dst] = HeapAlloc([h], flags, size); null -> fail.
    fn alloc(c: &mut Vec<u8>, fails: &mut Vec<usize>, slot: u32, flags: u8, size: u32, dst: u8) {
        c.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, H]); // mov rcx,[h]
        c.extend_from_slice(&[0xBA, flags, 0, 0, 0]); // mov edx,flags
        c.extend_from_slice(&[0x41, 0xB8]); // mov r8d,size
        c.extend_from_slice(&size.to_le_bytes());
        rip_rel_to(c, &[0xFF, 0x15], slot); // call [HeapAlloc]
        c.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax,rax
        jfail(c, fails, JE);
        c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, dst]); // mov [dst],rax
    }
    // HeapFree([h], 0, [src]); 0 -> fail.
    fn free(c: &mut Vec<u8>, fails: &mut Vec<usize>, slot: u32, src: u8) {
        c.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, H]); // mov rcx,[h]
        c.extend_from_slice(&[0x31, 0xD2]); // xor edx,edx
        c.extend_from_slice(&[0x4C, 0x8B, 0x44, 0x24, src]); // mov r8,[src]
        rip_rel_to(c, &[0xFF, 0x15], slot);
        c.extend_from_slice(&[0x85, 0xC0]); // test eax,eax
        jfail(c, fails, JE);
    }
    // rep stosb of `size` x `pat` at [src].
    fn fill(c: &mut Vec<u8>, src: u8, pat: u8, size: u32) {
        c.extend_from_slice(&[0x48, 0x8B, 0x7C, 0x24, src, 0xB0, pat, 0xB9]);
        c.extend_from_slice(&size.to_le_bytes());
        c.extend_from_slice(&[0xF3, 0xAA]);
    }
    // repe scasb of `size` x `pat` at [src]; any mismatch -> fail.
    fn check(c: &mut Vec<u8>, fails: &mut Vec<usize>, src: u8, pat: u8, size: u32) {
        c.extend_from_slice(&[0x48, 0x8B, 0x7C, 0x24, src, 0xB0, pat, 0xB9]);
        c.extend_from_slice(&size.to_le_bytes());
        c.extend_from_slice(&[0xF3, 0xAE]);
        jfail(c, fails, JNE);
    }
    build_import_exe(
        "kernel32.dll",
        &[
            "GetProcessHeap",
            "HeapAlloc",
            "HeapFree",
            "GetStdHandle",
            "WriteFile",
            "ExitProcess",
        ],
        None,
        None,
        |slot, msg| {
            let mut c: Vec<u8> = Vec::new();
            let mut fails: Vec<usize> = Vec::new();
            c.extend_from_slice(&[0x48, 0x83, 0xEC, 0x68]); // sub rsp,0x68
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [GetProcessHeap]
            c.extend_from_slice(&[0x48, 0x85, 0xC0]);
            jfail(&mut c, &mut fails, JE);
            c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, H]); // mov [h],rax
            // The handle must be stable: a 2nd GetProcessHeap returns the same value.
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]);
            c.extend_from_slice(&[0x48, 0x3B, 0x44, 0x24, H]); // cmp rax,[h]
            jfail(&mut c, &mut fails, JNE);

            alloc(&mut c, &mut fails, slot[1], 0, 24, P0);
            alloc(&mut c, &mut fails, slot[1], 0, 100, P1);
            alloc(&mut c, &mut fails, slot[1], 0, 5000, P2);
            alloc(&mut c, &mut fails, slot[1], 0, 70000, P3);
            fill(&mut c, P0, 0xA1, 24);
            fill(&mut c, P1, 0xB2, 100);
            fill(&mut c, P2, 0xC3, 5000);
            fill(&mut c, P3, 0xD4, 70000);
            check(&mut c, &mut fails, P0, 0xA1, 24);
            check(&mut c, &mut fails, P1, 0xB2, 100);
            check(&mut c, &mut fails, P2, 0xC3, 5000);
            check(&mut c, &mut fails, P3, 0xD4, 70000);

            free(&mut c, &mut fails, slot[2], P0);
            free(&mut c, &mut fails, slot[2], P1);
            // E = HeapAlloc(50): the coalesced hole is the first fit -> same address as P0.
            alloc(&mut c, &mut fails, slot[1], 0, 50, E);
            c.extend_from_slice(&[0x48, 0x8B, 0x44, 0x24, E]); // mov rax,[e]
            c.extend_from_slice(&[0x48, 0x3B, 0x44, 0x24, P0]); // cmp rax,[p0]
            jfail(&mut c, &mut fails, JNE);
            fill(&mut c, E, 0xE5, 50);
            // F = HeapAlloc(HEAP_ZERO_MEMORY, 60) reuses the dirty tail of the hole: zero.
            alloc(&mut c, &mut fails, slot[1], 8, 60, F);
            check(&mut c, &mut fails, F, 0x00, 60);
            fill(&mut c, F, 0xF6, 60);
            check(&mut c, &mut fails, E, 0xE5, 50);
            check(&mut c, &mut fails, F, 0xF6, 60);
            check(&mut c, &mut fails, P2, 0xC3, 5000);
            check(&mut c, &mut fails, P3, 0xD4, 70000);
            free(&mut c, &mut fails, slot[2], E);
            free(&mut c, &mut fails, slot[2], F);
            free(&mut c, &mut fails, slot[2], P2);
            free(&mut c, &mut fails, slot[2], P3);

            // WriteFile(GetStdHandle(-11), msg, 6, &n, 0); ExitProcess(0)
            c.extend_from_slice(&[0xB9, 0xF5, 0xFF, 0xFF, 0xFF]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[3]);
            c.extend_from_slice(&[0x48, 0x89, 0xC1]);
            rip_rel_to(&mut c, &[0x48, 0x8D, 0x15], msg);
            c.extend_from_slice(&[0x41, 0xB8, 6, 0, 0, 0]);
            c.extend_from_slice(&[0x4C, 0x8D, 0x4C, 0x24, 0x38]); // lea r9,[rsp+0x38] (&n)
            c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[4]);
            c.extend_from_slice(&[0x31, 0xC9]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[5]);
            c.extend_from_slice(&[0x0F, 0x0B]);
            // fail: ExitProcess(0xBAD)
            let fail_pos = c.len();
            c.extend_from_slice(&[0xB9, 0xAD, 0x0B, 0, 0]);
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[5]);
            c.extend_from_slice(&[0x0F, 0x0B]);
            for f in fails {
                let rel = (fail_pos as i32) - (f as i32 + 4);
                c[f..f + 4].copy_from_slice(&rel.to_le_bytes());
            }
            c
        },
    )
}

/// Test `.exe` that calls `kernel32!ExitProcess(0x2A)`.
pub fn build_exe_exit_code_2a() -> alloc::vec::Vec<u8> {
    build_import_exe("kernel32.dll", &["ExitProcess"], None, None, |slot, _| {
        let mut c = alloc::vec::Vec::new();
        c.extend_from_slice(&[0x48, 0x83, 0xEC, 0x28]); // sub rsp, 0x28
        c.extend_from_slice(&[0xB9, 0x2A, 0, 0, 0]); // mov ecx, 0x2A
        rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [ExitProcess]
        c.extend_from_slice(&[0x0F, 0x0B]);
        c
    })
}

/// Test parent `.exe` (kernel32 only): `CreateProcessA("C:\test\exit42.exe")`,
/// `WaitForSingleObject(hProcess, INFINITE)`, `GetExitCodeProcess`, `CloseHandle`, then
/// `ExitProcess(childCode + 0x1000)`. Any failed check (or a zero pid) exits 0xBAD.
#[cfg(feature = "ktest")]
pub fn build_exe_spawns_child() -> alloc::vec::Vec<u8> {
    build_import_exe(
        "kernel32.dll",
        &[
            "CreateProcessA",
            "WaitForSingleObject",
            "GetExitCodeProcess",
            "CloseHandle",
            "ExitProcess",
        ],
        None,
        None,
        |slot, _| {
            use alloc::vec::Vec;
            let mut c: Vec<u8> = Vec::new();
            let mut fails: Vec<usize> = Vec::new();
            // Frame: args @0x20..0x50, STARTUPINFOA @0x60 (104), PROCESS_INFORMATION @0xC8,
            // exit code @0xE0.
            c.extend_from_slice(&[0x48, 0x81, 0xEC, 0x08, 0x01, 0, 0]); // sub rsp, 0x108
            c.extend_from_slice(&[0x48, 0x8D, 0x0D]); // lea rcx,[rip+path] (patched)
            let lea_at = c.len();
            c.extend_from_slice(&[0, 0, 0, 0]);
            c.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (lpCommandLine)
            c.extend_from_slice(&[0x45, 0x31, 0xC0]); // xor r8d, r8d
            c.extend_from_slice(&[0x45, 0x31, 0xC9]); // xor r9d, r9d
            for off in [0x20u8, 0x28, 0x30, 0x38] {
                c.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, off, 0, 0, 0, 0]);
            }
            c.extend_from_slice(&[0x48, 0x8D, 0x44, 0x24, 0x60]); // lea rax,[rsp+0x60]
            c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x40]); // [rsp+0x40] = &si
            c.extend_from_slice(&[0x48, 0x8D, 0x84, 0x24, 0xC8, 0, 0, 0]); // lea rax,[rsp+0xC8]
            c.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x48]); // [rsp+0x48] = &pi
            c.extend_from_slice(&[0xBB, 1, 0, 0, 0]); // mov ebx, step
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[0]); // call [CreateProcessA]
            c.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0xBB, 0, 0, 0, 0]); // mov ebx, 0 (pid check)
            c.extend_from_slice(&[0x83, 0xBC, 0x24, 0xD8, 0, 0, 0, 0]); // cmp dword [pi.pid], 0
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x48, 0x8B, 0x8C, 0x24, 0xC8, 0, 0, 0]); // mov rcx,[pi.hProcess]
            c.extend_from_slice(&[0xBA, 0xFF, 0xFF, 0xFF, 0xFF]); // mov edx, INFINITE
            c.extend_from_slice(&[0xBB, 2, 0, 0, 0]); // mov ebx, step
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[1]); // call [WaitForSingleObject]
            c.extend_from_slice(&[0x85, 0xC0]); // test eax, eax (WAIT_OBJECT_0)
            c.extend_from_slice(&[0x0F, 0x85, 0, 0, 0, 0]); // jne fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x48, 0x8B, 0x8C, 0x24, 0xC8, 0, 0, 0]); // mov rcx,[pi.hProcess]
            c.extend_from_slice(&[0x48, 0x8D, 0x94, 0x24, 0xE0, 0, 0, 0]); // lea rdx,[rsp+0xE0]
            c.extend_from_slice(&[0xBB, 3, 0, 0, 0]); // mov ebx, step
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[2]); // call [GetExitCodeProcess]
            c.extend_from_slice(&[0x85, 0xC0]);
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x48, 0x8B, 0x8C, 0x24, 0xC8, 0, 0, 0]); // mov rcx,[pi.hProcess]
            c.extend_from_slice(&[0xBB, 4, 0, 0, 0]); // mov ebx, step
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[3]); // call [CloseHandle]
            c.extend_from_slice(&[0x85, 0xC0]);
            c.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // je fail
            fails.push(c.len() - 4);
            c.extend_from_slice(&[0x8B, 0x8C, 0x24, 0xE0, 0, 0, 0]); // mov ecx,[code]
            c.extend_from_slice(&[0x81, 0xC1, 0x00, 0x10, 0, 0]); // add ecx, 0x1000
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[4]); // call [ExitProcess]
            c.extend_from_slice(&[0x0F, 0x0B]);
            let fail_pos = c.len();
            c.extend_from_slice(&[0x8D, 0x8B, 0xD0, 0xBA, 0, 0]); // lea ecx,[rbx+0xBAD0]
            rip_rel_to(&mut c, &[0xFF, 0x15], slot[4]);
            c.extend_from_slice(&[0x0F, 0x0B]);
            for f in fails {
                let rel = fail_pos as i32 - (f as i32 + 4);
                c[f..f + 4].copy_from_slice(&rel.to_le_bytes());
            }
            let path_pos = c.len();
            c.extend_from_slice(b"C:\\test\\exit42.exe\0");
            let rel = path_pos as i32 - (lea_at as i32 + 4);
            c[lea_at..lea_at + 4].copy_from_slice(&rel.to_le_bytes());
            c
        },
    )
}

/// Test `.exe` importing a DLL (`user32.dll`) that is not loaded in the process.
pub fn build_exe_importing_missing_dll() -> alloc::vec::Vec<u8> {
    build_import_exe("user32.dll", &["MessageBoxA"], None, None, |_, _| {
        alloc::vec![0x0F, 0x0B]
    })
}

/// Hostile `.exe`: its IAT (FirstThunk) lies inside the R+X `.text` AND its data
/// directory #12 claims `.text` is the IAT. The user-mode loader must still refuse it
/// (non-writable/executable section), proving the rejection does not rest on dir 12.
pub fn build_exe_with_bad_iat_dir12() -> alloc::vec::Vec<u8> {
    build_import_exe(
        "ntdll.dll",
        &["NtTerminateProcess"],
        Some(TEXT_RVA),
        Some((TEXT_RVA, 0x20)),
        |_, _| alloc::vec![0x0F, 0x0B],
    )
}
