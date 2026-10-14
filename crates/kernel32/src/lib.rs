//! TukOS kernel32.dll: the minimal Win32 layer (GetStdHandle / WriteFile /
//! ExitProcess) implemented over our ntdll.
//!
//! No CRT, no heap. The only import is `ntdll.dll`, declared with `raw-dylib`
//! so the compiler synthesizes the import directory (no import library and no
//! build-order dependency on the ntdll crate).
#![no_std]
#![allow(non_snake_case)]

use core::ffi::c_void;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    ExitProcess(0xC000_7108)
}

// No-CRT memory primitives (same as ntdll).

/// # Safety
/// Standard `memcpy` contract.
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
/// Standard `memset` contract.
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
/// Standard `memmove` contract.
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
/// Standard `memcmp` contract.
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

#[unsafe(no_mangle)]
static _fltused: i32 = 0;

#[unsafe(no_mangle)]
extern "C" fn __CxxFrameHandler3() -> i32 {
    0
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtWriteFile(
        file: isize,
        event: isize,
        apc_routine: *const c_void,
        apc_context: *const c_void,
        iosb: *mut IoStatusBlock,
        buffer: *const u8,
        length: u32,
        byte_offset: *const i64,
        key: *const u32,
    ) -> i32;
    fn NtTerminateProcess(process: isize, status: i32) -> i32;
}

/// IO_STATUS_BLOCK (x64): Status @0, Information @8.
#[repr(C)]
struct IoStatusBlock {
    status: i64,
    information: usize,
}

const STD_INPUT_HANDLE: i32 = -10;
const STD_OUTPUT_HANDLE: i32 = -11;
const STD_ERROR_HANDLE: i32 = -12;
const INVALID_HANDLE_VALUE: isize = -1;

const TEB_PEB: usize = 0x60;
const PEB_PROCESS_PARAMETERS: usize = 0x20;
// RTL_USER_PROCESS_PARAMETERS (winternl.h): StandardInput/Output/Error handles.
const PP_STANDARD_INPUT: usize = 0x20;
const PP_STANDARD_OUTPUT: usize = 0x28;
const PP_STANDARD_ERROR: usize = 0x30;

/// `GetStdHandle`: reads `PEB->ProcessParameters` through GS; no imports.
#[unsafe(no_mangle)]
pub extern "system" fn GetStdHandle(n: i32) -> isize {
    let off = match n {
        STD_INPUT_HANDLE => PP_STANDARD_INPUT,
        STD_OUTPUT_HANDLE => PP_STANDARD_OUTPUT,
        STD_ERROR_HANDLE => PP_STANDARD_ERROR,
        _ => return INVALID_HANDLE_VALUE,
    };
    unsafe {
        // gs:[0x60] is TEB->ProcessEnvironmentBlock.
        let peb: usize;
        core::arch::asm!("mov {}, gs:[{off}]", out(reg) peb, off = const TEB_PEB,
            options(nostack, readonly, preserves_flags));
        if peb == 0 {
            return INVALID_HANDLE_VALUE;
        }
        let pp = core::ptr::read((peb + PEB_PROCESS_PARAMETERS) as *const usize);
        if pp == 0 {
            return INVALID_HANDLE_VALUE;
        }
        core::ptr::read((pp + off) as *const isize)
    }
}

/// `WriteFile` over `NtWriteFile`. Overlapped I/O is not supported (ignored).
///
/// # Safety
/// `buf` must be valid for `len` bytes; `written` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn WriteFile(
    handle: isize,
    buf: *const u8,
    len: u32,
    written: *mut u32,
    _overlapped: *mut c_void,
) -> i32 {
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let st = unsafe {
        NtWriteFile(
            handle,
            0,
            core::ptr::null(),
            core::ptr::null(),
            &mut iosb,
            buf,
            len,
            core::ptr::null(),
            core::ptr::null(),
        )
    };
    if st < 0 {
        return 0;
    }
    if !written.is_null() {
        unsafe { core::ptr::write(written, iosb.information as u32) };
    }
    1
}

/// `ExitProcess` over `NtTerminateProcess(NtCurrentProcess(), code)`.
#[unsafe(no_mangle)]
pub extern "system" fn ExitProcess(code: u32) -> ! {
    unsafe {
        NtTerminateProcess(-1, code as i32);
    }
    loop {
        unsafe { core::arch::asm!("ud2", options(nomem, nostack)) };
    }
}
