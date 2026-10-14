//! TukOS ntdll.dll: thin syscall stubs with the Windows x64 convention, plus
//! `LdrInitializeThunk`, the user-mode import loader.
//! Each `Nt*` export does `mov r10, rcx; mov eax, <tukos ssn>; syscall; ret`.
//! The syscall numbers MUST match the TukOS kernel dispatch.
//!
//! The DLL has no imports, no heap and no CRT: everything it needs is in `core`,
//! the alloc-free accessors of the `pe` crate, and the `mem*` primitives below.
#![no_std]
#![allow(non_snake_case)]

use core::panic::PanicInfo;
use pe::{PeFile, SCN_MEM_EXECUTE, SCN_MEM_WRITE};

/// A panic can only come from a latent bug; never unwind, just end the process.
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    die(ST_PANIC)
}

// ---------------------------------------------------------------------------
// No-CRT memory primitives. The compiler may emit calls to these even in
// freestanding code; they use `rep` string ops / volatile accesses so the
// optimizer cannot turn them back into calls to themselves.
// ---------------------------------------------------------------------------

/// # Safety
/// Standard `memcpy` contract: `dst`/`src` valid for `n` bytes, non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        core::arch::asm!(
            "rep movsb",
            inout("rcx") n => _,
            inout("rdi") dst => _,
            inout("rsi") src => _,
            options(nostack, preserves_flags)
        );
    }
    dst
}

/// # Safety
/// Standard `memset` contract: `dst` valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        core::arch::asm!(
            "rep stosb",
            inout("rcx") n => _,
            inout("rdi") dst => _,
            in("al") c as u8,
            options(nostack, preserves_flags)
        );
    }
    dst
}

/// # Safety
/// Standard `memmove` contract: both pointers valid for `n` bytes (may overlap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        if (dst as usize) <= (src as usize) || (dst as usize) >= (src as usize).wrapping_add(n) {
            let mut i = 0;
            while i < n {
                core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
                i += 1;
            }
        } else {
            let mut i = n;
            while i > 0 {
                i -= 1;
                core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
            }
        }
    }
    dst
}

/// # Safety
/// Standard `memcmp` contract: both pointers valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let (x, y) = unsafe {
            (
                core::ptr::read_volatile(a.add(i)),
                core::ptr::read_volatile(b.add(i)),
            )
        };
        if x != y {
            return i32::from(x) - i32::from(y);
        }
        i += 1;
    }
    0
}

/// # Safety
/// Same contract as `memcmp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

// Linker symbols the MSVC toolchain expects from `core`/`compiler_builtins` even
// though this DLL never uses floats or C++ exceptions (/NODEFAULTLIB: no CRT).
#[unsafe(no_mangle)]
static _fltused: i32 = 0;

#[unsafe(no_mangle)]
extern "C" fn __CxxFrameHandler3() -> i32 {
    0
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

// ---------------------------------------------------------------------------
// LdrInitializeThunk: the user-mode import loader.
// ---------------------------------------------------------------------------

// Distinct NtTerminateProcess statuses for every way the loader can fail.
const ST_NO_PEB: u32 = 0xC000_7001;
const ST_NO_EXE: u32 = 0xC000_7002;
const ST_NO_NTDLL: u32 = 0xC000_7003;
const ST_BAD_PE: u32 = 0xC000_7004;
const ST_UNRESOLVED: u32 = 0xC000_7005;
/// Must match `LDR_ST_BAD_IAT` in kernel/src/ktest.rs.
const ST_BAD_IAT: u32 = 0xC000_7006;
const ST_BAD_ENTRY: u32 = 0xC000_7007;
const ST_PANIC: u32 = 0xC000_7008;

/// NtTerminateProcess(status) via syscall; never returns.
fn die(status: u32) -> ! {
    loop {
        unsafe {
            core::arch::asm!(
                "syscall",
                in("eax") 3u32,
                in("r10") u64::from(status),
                out("rcx") _,
                out("r11") _,
                options(nostack)
            );
        }
    }
}

// TEB / PEB / PEB_LDR_DATA / LDR_DATA_TABLE_ENTRY offsets (public winternl.h layout).
const TEB_SELF: usize = 0x30;
const TEB_PEB: usize = 0x60;
const PEB_LDR: usize = 0x18;
const LDR_IN_LOAD_ORDER: usize = 0x10;
const ENT_DLL_BASE: usize = 0x30;
const ENT_SIZE_OF_IMAGE: usize = 0x40;
const ENT_BASE_NAME_LEN: usize = 0x58;
const ENT_BASE_NAME_BUF: usize = 0x60;
const MAX_MODULES: usize = 32;

/// True if the UTF-16 `BaseDllName` of the entry equals `want` (ASCII, case-insensitive).
///
/// # Safety
/// `entry` must point to a readable LDR_DATA_TABLE_ENTRY.
unsafe fn name_is(entry: *const u8, want: &[u8]) -> bool {
    unsafe {
        let len = core::ptr::read(entry.add(ENT_BASE_NAME_LEN) as *const u16) as usize;
        let buf = core::ptr::read(entry.add(ENT_BASE_NAME_BUF) as *const *const u16);
        if len != want.len() * 2 || buf.is_null() {
            return false;
        }
        let mut i = 0;
        while i < want.len() {
            let c = core::ptr::read(buf.add(i));
            if c > 0x7F || (c as u8).to_ascii_lowercase() != want[i].to_ascii_lowercase() {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// The 8-byte IAT slot at `rva` must lie entirely inside one section of the loaded
/// exe that is writable, so a malformed exe can only make us write its own data.
fn iat_slot_ok(exe: &PeFile, size_of_image: u64, rva: u32) -> bool {
    let start = u64::from(rva);
    let Some(end) = start.checked_add(8) else {
        return false;
    };
    if end > size_of_image {
        return false;
    }
    // A slot is acceptable inside a writable, non-executable section, or inside the
    // exe's declared IAT directory in a non-executable section (MSVC's read-only
    // `.rdata` IAT; the kernel maps those pages writable, as the Windows loader does).
    let (iat_rva, iat_size) = exe.iat_directory();
    let iat_start = u64::from(iat_rva);
    let iat_end = iat_start + u64::from(iat_size);
    let in_iat_dir = iat_size != 0 && start >= iat_start && end <= iat_end;
    exe.sections().any(|s| {
        let s_start = u64::from(s.rva);
        let s_end = s_start + u64::from(s.virtual_size);
        let inside = start >= s_start && end <= s_end;
        let exec = s.characteristics & SCN_MEM_EXECUTE != 0;
        inside && !exec && (s.characteristics & SCN_MEM_WRITE != 0 || in_iat_dir)
    })
}

/// Rust body of the thunk (the naked entry below aligns the stack first).
extern "C" fn ldr_main() -> ! {
    unsafe {
        // TEB via GS (NtTib.Self), then PEB, then PEB->Ldr.
        let teb: usize;
        core::arch::asm!("mov {}, gs:[{off}]", out(reg) teb, off = const TEB_SELF,
            options(nostack, readonly, preserves_flags));
        if teb == 0 {
            die(ST_NO_PEB);
        }
        let peb = core::ptr::read((teb + TEB_PEB) as *const usize);
        if peb == 0 {
            die(ST_NO_PEB);
        }
        let ldr = core::ptr::read((peb + PEB_LDR) as *const usize);
        if ldr == 0 {
            die(ST_NO_PEB);
        }

        // Walk InLoadOrderModuleList: first entry is the exe, then find ntdll.dll.
        let head = (ldr + LDR_IN_LOAD_ORDER) as *const usize;
        let mut cur = core::ptr::read(head) as *const u8;
        if cur as usize == head as usize || cur.is_null() {
            die(ST_NO_EXE);
        }
        let exe_entry = cur;
        let mut ntdll_entry: *const u8 = core::ptr::null();
        let mut n = 0;
        while cur as usize != head as usize && !cur.is_null() && n < MAX_MODULES {
            if name_is(cur, b"ntdll.dll") {
                ntdll_entry = cur;
                break;
            }
            cur = core::ptr::read(cur as *const usize) as *const u8;
            n += 1;
        }
        if ntdll_entry.is_null() {
            die(ST_NO_NTDLL);
        }

        let exe_base = core::ptr::read(exe_entry.add(ENT_DLL_BASE) as *const usize);
        let exe_size = core::ptr::read(exe_entry.add(ENT_SIZE_OF_IMAGE) as *const u32) as usize;
        let nt_base = core::ptr::read(ntdll_entry.add(ENT_DLL_BASE) as *const usize);
        let nt_size = core::ptr::read(ntdll_entry.add(ENT_SIZE_OF_IMAGE) as *const u32) as usize;
        if exe_base == 0 || exe_size == 0 || nt_base == 0 || nt_size == 0 {
            die(ST_BAD_PE);
        }

        // Parse both as LOADED images (sections at their RVAs).
        let exe_img = core::slice::from_raw_parts(exe_base as *const u8, exe_size);
        let nt_img = core::slice::from_raw_parts(nt_base as *const u8, nt_size);
        let Ok(exe) = PeFile::parse_loaded(exe_img) else {
            die(ST_BAD_PE)
        };
        let Ok(nt) = PeFile::parse_loaded(nt_img) else {
            die(ST_BAD_PE)
        };
        let Ok(imports) = exe.imports() else {
            die(ST_BAD_PE)
        };

        for dll in imports {
            if !dll.name.eq_ignore_ascii_case("ntdll.dll") {
                continue;
            }
            for entry in dll.thunks {
                let Some(rva) = nt.resolve_export(&entry) else {
                    die(ST_UNRESOLVED)
                };
                if rva as usize >= nt_size {
                    die(ST_UNRESOLVED);
                }
                if !iat_slot_ok(&exe, exe_size as u64, entry.iat_rva) {
                    die(ST_BAD_IAT);
                }
                let slot = (exe_base + entry.iat_rva as usize) as *mut u64;
                core::ptr::write_volatile(slot, (nt_base + rva as usize) as u64);
            }
        }

        let ep = exe.entry_point_rva() as usize;
        if ep == 0 || ep >= exe_size {
            die(ST_BAD_ENTRY);
        }
        let entry: extern "system" fn() -> ! = core::mem::transmute(exe_base + ep);
        entry()
    }
}

/// Initial user-mode entry of every process: aligns the stack to the x64 ABI
/// (16 bytes at the call + 32 bytes of shadow space) and runs the loader.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn LdrInitializeThunk() -> ! {
    core::arch::naked_asm!(
        "and rsp, -16",
        "sub rsp, 32",
        "call {main}",
        "ud2",
        main = sym ldr_main,
    );
}
