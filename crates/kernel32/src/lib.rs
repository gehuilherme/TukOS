//! TukOS kernel32.dll: the minimal Win32 layer (GetStdHandle / WriteFile /
//! ExitProcess) implemented over our ntdll.
//!
//! No CRT, no Rust allocator (the Heap* API forwards to ntdll). The only import is `ntdll.dll`, declared with `raw-dylib`
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
    fn NtReadFile(
        file: isize,
        event: isize,
        apc_routine: *const c_void,
        apc_context: *const c_void,
        iosb: *mut IoStatusBlock,
        buffer: *mut u8,
        length: u32,
        byte_offset: *const i64,
        key: *const u32,
    ) -> i32;
    // TukOS file extensions (kernel/src/syscall.rs NT_OPEN_FILE/SET_FILE_POINTER/QUERY_FILE_SIZE).
    fn NtTukosOpenFile(
        out_handle: *mut isize,
        path: *const u16,
        path_bytes: u32,
        access: u32,
    ) -> i32;
    fn NtTukosSetFilePointer(handle: isize, dist: i64, method: u32, out_new: *mut u64) -> i32;
    fn NtTukosQueryFileSize(handle: isize, out_size: *mut u64) -> i32;
    // TukOS process extensions (kernel/src/syscall.rs NT_CREATE_PROCESS/NT_WAIT/NT_QUERY_EXIT_CODE).
    fn NtTukosCreateProcess(path: *const u16, path_bytes: u32, out_process: *mut isize) -> i32;
    fn NtTukosWait(process: isize) -> i32;
    fn NtTukosQueryExitCode(process: isize, out_code: *mut u32) -> i32;
    fn NtTerminateProcess(process: isize, status: i32) -> i32;
    fn NtClose(handle: isize) -> i32;
    fn RtlGetProcessHeap() -> *mut c_void;
    fn RtlAllocateHeap(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void;
    fn RtlFreeHeap(heap: *mut c_void, flags: u32, ptr: *mut c_void) -> i32;
    fn RtlSizeHeap(heap: *mut c_void, flags: u32, ptr: *const c_void) -> usize;
    fn RtlReAllocateHeap(
        heap: *mut c_void,
        flags: u32,
        ptr: *mut c_void,
        size: usize,
    ) -> *mut c_void;
}

/// `CloseHandle` over `NtClose` (also keeps the `NtClose` import alive, which the
/// `TukosCloseForward -> ntdll.NtClose` forwarder export needs at link time).
#[unsafe(no_mangle)]
pub extern "system" fn CloseHandle(handle: isize) -> i32 {
    i32::from(unsafe { NtClose(handle) } >= 0)
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

/// Shared body of `ReadFile`/`ReadConsoleA` over `NtReadFile`.
unsafe fn read_via_nt(handle: isize, buf: *mut u8, len: u32, read: *mut u32) -> i32 {
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let st = unsafe {
        NtReadFile(
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
        if !read.is_null() {
            unsafe { core::ptr::write(read, 0) };
        }
        return 0;
    }
    if !read.is_null() {
        unsafe { core::ptr::write(read, iosb.information as u32) };
    }
    1
}

/// `ReadFile` over `NtReadFile`. On the console-input handle it blocks for a line
/// (with echo). Overlapped I/O is not supported (ignored).
///
/// # Safety
/// `buf` must be valid for `len` bytes; `read` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ReadFile(
    handle: isize,
    buf: *mut u8,
    len: u32,
    read: *mut u32,
    _overlapped: *mut c_void,
) -> i32 {
    unsafe { read_via_nt(handle, buf, len, read) }
}

/// `ReadConsoleA`: ANSI console read (chars == bytes); `_input_control` is ignored.
///
/// # Safety
/// `buf` must be valid for `to_read` bytes; `read` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ReadConsoleA(
    handle: isize,
    buf: *mut u8,
    to_read: u32,
    read: *mut u32,
    _input_control: *mut c_void,
) -> i32 {
    unsafe { read_via_nt(handle, buf, to_read, read) }
}

/// `ReadConsoleW`: reads a line like `ReadConsoleA`, widening each byte (Latin-1/ASCII)
/// to UTF-16. At most 512 chars per call; `_input_control` is ignored.
///
/// # Safety
/// `buf` must be valid for `to_read` u16 units; `read` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ReadConsoleW(
    handle: isize,
    buf: *mut u16,
    to_read: u32,
    read: *mut u32,
    _input_control: *mut c_void,
) -> i32 {
    let mut tmp = [0u8; 512];
    let want = core::cmp::min(to_read as usize, tmp.len()) as u32;
    let mut got = 0u32;
    if unsafe { read_via_nt(handle, tmp.as_mut_ptr(), want, &mut got) } == 0 {
        if !read.is_null() {
            unsafe { core::ptr::write(read, 0) };
        }
        return 0;
    }
    for (i, &b) in tmp[..got as usize].iter().enumerate() {
        unsafe { core::ptr::write(buf.add(i), b as u16) };
    }
    if !read.is_null() {
        unsafe { core::ptr::write(read, got) };
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

/// `GetProcessHeap`: the process heap handle (ntdll creates the heap on first use and
/// always returns the same handle). Null if the heap cannot be created.
#[unsafe(no_mangle)]
pub extern "system" fn GetProcessHeap() -> *mut c_void {
    unsafe { RtlGetProcessHeap() }
}

/// `HeapAlloc` over `RtlAllocateHeap` (`HEAP_ZERO_MEMORY` = 0x8 honoured by ntdll).
///
/// # Safety
/// `heap` must be a handle from `GetProcessHeap`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn HeapAlloc(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void {
    unsafe { RtlAllocateHeap(heap, flags, size) }
}

/// `HeapFree` over `RtlFreeHeap`; nonzero on success.
///
/// # Safety
/// `ptr` must be null or a live block returned by `HeapAlloc` on `heap`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn HeapFree(heap: *mut c_void, flags: u32, ptr: *mut c_void) -> i32 {
    unsafe { RtlFreeHeap(heap, flags, ptr) }
}

// ---------------------------------------------------------------------------
// MSVC /MT CRT surface (Plan 13, Task 2). Everything below is implemented from the
// public Win32 reference (MSDN / winnt.h / winternl.h); the per-import triage lives in
// docs/observations/2026-10-14-crt-imports-survey.md.
//
// SINGLE-THREAD ASSUMPTION: a TukOS process has exactly one user thread, so the
// per-process statics here (TLS table, ...) need no locking and the critical-section
// primitives are no-ops. When user threads exist, TLS must move into the TEB (the
// TlsSlots @0x1480 do not fit the one-page TEB we map) and Enter/Leave must become
// real locks.
//
// Legend: [real] works as documented for one thread; [stub] benign value, rarely or
// never hit by a clean hello; [T3] placeholder that Task 3 verifies/finishes;
// [COLD-BUG] being reached means something is wrong (process dies with a sentinel).
// ---------------------------------------------------------------------------

const TEB_LAST_ERROR: usize = 0x68;
const PEB_IMAGE_BASE: usize = 0x10;

const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_FILENAME_EXCED_RANGE: u32 = 206;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const ERROR_MOD_NOT_FOUND: u32 = 126;
const ERROR_PROC_NOT_FOUND: u32 = 127;
const TLS_OUT_OF_INDEXES: u32 = 0xFFFF_FFFF;
const FILE_TYPE_DISK: u32 = 1;
const FILE_TYPE_CHAR: u32 = 2;

/// Sentinel exit statuses of the cold paths (seen as a TerminateProcess status).
const COLD_RAISE: i32 = 0xC000_7201u32 as i32;
const COLD_UNWIND: i32 = 0xC000_7202u32 as i32;

fn teb_peb() -> usize {
    let peb: usize;
    unsafe {
        core::arch::asm!("mov {}, gs:[{off}]", out(reg) peb, off = const TEB_PEB,
            options(nostack, readonly, preserves_flags));
    }
    peb
}

// --- last error (per thread: TEB.LastErrorValue @0x68) ---------------------------

/// `GetLastError` [real]: TEB->LastErrorValue.
#[unsafe(no_mangle)]
pub extern "system" fn GetLastError() -> u32 {
    let v: u32;
    unsafe {
        core::arch::asm!("mov {0:e}, gs:[{off}]", out(reg) v, off = const TEB_LAST_ERROR,
            options(nostack, readonly, preserves_flags));
    }
    v
}

/// `SetLastError` [real].
#[unsafe(no_mangle)]
pub extern "system" fn SetLastError(code: u32) {
    unsafe {
        core::arch::asm!("mov gs:[{off}], {0:e}", in(reg) code, off = const TEB_LAST_ERROR,
            options(nostack, preserves_flags));
    }
}

// --- TLS / FLS (single thread: one process-wide table of 64 slots) ----------------

const TLS_SLOTS: usize = 64;
static mut TLS_USED: [bool; TLS_SLOTS] = [false; TLS_SLOTS];
static mut TLS_VALUES: [usize; TLS_SLOTS] = [0; TLS_SLOTS];

/// True if `index` names an allocated slot.
fn tls_valid(index: u32) -> bool {
    let i = index as usize;
    i < TLS_SLOTS && unsafe { (*core::ptr::addr_of!(TLS_USED))[i] }
}

/// `TlsAlloc` [real]: lowest free slot, zeroed; `TLS_OUT_OF_INDEXES` when full.
#[unsafe(no_mangle)]
pub extern "system" fn TlsAlloc() -> u32 {
    unsafe {
        let used = &mut *core::ptr::addr_of_mut!(TLS_USED);
        let vals = &mut *core::ptr::addr_of_mut!(TLS_VALUES);
        let mut i = 0;
        while i < TLS_SLOTS {
            if !used[i] {
                used[i] = true;
                vals[i] = 0;
                return i as u32;
            }
            i += 1;
        }
    }
    SetLastError(8); // ERROR_NOT_ENOUGH_MEMORY
    TLS_OUT_OF_INDEXES
}

/// `TlsGetValue` [real]: sets last error to 0 on success (as documented).
#[unsafe(no_mangle)]
pub extern "system" fn TlsGetValue(index: u32) -> usize {
    if !tls_valid(index) {
        SetLastError(87); // ERROR_INVALID_PARAMETER
        return 0;
    }
    SetLastError(0);
    unsafe { (*core::ptr::addr_of!(TLS_VALUES))[index as usize] }
}

/// `TlsSetValue` [real].
#[unsafe(no_mangle)]
pub extern "system" fn TlsSetValue(index: u32, value: usize) -> i32 {
    if !tls_valid(index) {
        SetLastError(87);
        return 0;
    }
    unsafe { (*core::ptr::addr_of_mut!(TLS_VALUES))[index as usize] = value };
    1
}

/// `TlsFree` [real].
#[unsafe(no_mangle)]
pub extern "system" fn TlsFree(index: u32) -> i32 {
    if !tls_valid(index) {
        SetLastError(87);
        return 0;
    }
    unsafe {
        (*core::ptr::addr_of_mut!(TLS_USED))[index as usize] = false;
        (*core::ptr::addr_of_mut!(TLS_VALUES))[index as usize] = 0;
    }
    1
}

/// `FlsAlloc` [real, on the TLS table]: the destructor callback is ignored (no
/// thread/process-detach notifications exist yet).
#[unsafe(no_mangle)]
pub extern "system" fn FlsAlloc(_callback: *const c_void) -> u32 {
    TlsAlloc()
}
/// `FlsGetValue` [real].
#[unsafe(no_mangle)]
pub extern "system" fn FlsGetValue(index: u32) -> usize {
    TlsGetValue(index)
}
/// `FlsSetValue` [real].
#[unsafe(no_mangle)]
pub extern "system" fn FlsSetValue(index: u32, value: usize) -> i32 {
    TlsSetValue(index, value)
}
/// `FlsFree` [real].
#[unsafe(no_mangle)]
pub extern "system" fn FlsFree(index: u32) -> i32 {
    TlsFree(index)
}

// --- critical sections (single thread: nothing can contend) -----------------------

/// CRITICAL_SECTION is 40 bytes on x64.
const CS_SIZE: usize = 40;

/// `InitializeCriticalSection` [real for 1 thread]: zeroes the structure.
///
/// # Safety
/// `cs` must point to a writable CRITICAL_SECTION.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeCriticalSection(cs: *mut u8) {
    if !cs.is_null() {
        unsafe { memset(cs, 0, CS_SIZE) };
    }
}
/// `InitializeCriticalSectionAndSpinCount` [real for 1 thread]: always TRUE.
///
/// # Safety
/// `cs` must point to a writable CRITICAL_SECTION.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeCriticalSectionAndSpinCount(
    cs: *mut u8,
    _spin: u32,
) -> i32 {
    unsafe { InitializeCriticalSection(cs) };
    1
}
/// `InitializeCriticalSectionEx` [real for 1 thread]: always TRUE.
///
/// # Safety
/// `cs` must point to a writable CRITICAL_SECTION.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeCriticalSectionEx(
    cs: *mut u8,
    _spin: u32,
    _flags: u32,
) -> i32 {
    unsafe { InitializeCriticalSection(cs) };
    1
}
/// `EnterCriticalSection` [real for 1 thread]: no-op.
#[unsafe(no_mangle)]
pub extern "system" fn EnterCriticalSection(_cs: *mut u8) {}
/// `LeaveCriticalSection` [real for 1 thread]: no-op.
#[unsafe(no_mangle)]
pub extern "system" fn LeaveCriticalSection(_cs: *mut u8) {}
/// `DeleteCriticalSection` [real for 1 thread]: no-op.
#[unsafe(no_mangle)]
pub extern "system" fn DeleteCriticalSection(_cs: *mut u8) {}

// --- cookie / time / ids ----------------------------------------------------------

fn tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// `GetSystemTimeAsFileTime` [real-enough]: a fixed base plus elapsed TSC (treated as
/// 100 ns units / 30). Monotonic; the CRT only uses it as cookie entropy.
///
/// # Safety
/// `ft` must be writable (8 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetSystemTimeAsFileTime(ft: *mut u64) {
    const BASE: u64 = 134_000_000_000_000_000;
    if !ft.is_null() {
        unsafe { core::ptr::write_unaligned(ft, BASE + tsc() / 30) };
    }
}

/// `QueryPerformanceCounter` [real-enough]: raw TSC (monotonic). Always succeeds.
///
/// # Safety
/// `out` must be writable (8 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn QueryPerformanceCounter(out: *mut u64) -> i32 {
    if out.is_null() {
        return 0;
    }
    unsafe { core::ptr::write_unaligned(out, tsc()) };
    1
}

/// `QueryPerformanceFrequency` [stub]: nominal 1 GHz (the TSC rate is not calibrated
/// in user mode yet), nonzero so callers never divide by zero.
///
/// # Safety
/// `out` must be writable (8 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn QueryPerformanceFrequency(out: *mut u64) -> i32 {
    if out.is_null() {
        return 0;
    }
    unsafe { core::ptr::write_unaligned(out, 1_000_000_000) };
    1
}

/// `GetTickCount64` [stub]: TSC / 2e6 (nominal 2 GHz), monotonic.
#[unsafe(no_mangle)]
pub extern "system" fn GetTickCount64() -> u64 {
    tsc() / 2_000_000
}
/// `GetTickCount` [stub].
#[unsafe(no_mangle)]
pub extern "system" fn GetTickCount() -> u32 {
    GetTickCount64() as u32
}
/// `GetCurrentProcessId` [stub]: TukOS has no PID API for user mode yet; 1.
#[unsafe(no_mangle)]
pub extern "system" fn GetCurrentProcessId() -> u32 {
    1
}
/// `GetCurrentThreadId` [stub]: one thread per process; 1.
#[unsafe(no_mangle)]
pub extern "system" fn GetCurrentThreadId() -> u32 {
    1
}
/// `GetCurrentProcess` [real]: the pseudo-handle (HANDLE)-1.
#[unsafe(no_mangle)]
pub extern "system" fn GetCurrentProcess() -> isize {
    -1
}

// --- modules / command line / environment -----------------------------------------

const EXE_PATH: &[u8; 13] = b"C:\\hello.exe\0";
static CMDLINE_A: [u8; 10] = *b"hello.exe\0";
static CMDLINE_W: [u16; 10] = widen(b"hello.exe\0");
/// Empty environment block: a double NUL terminator.
static EMPTY_ENV_W: [u16; 2] = [0, 0];

const fn widen<const N: usize>(s: &[u8; N]) -> [u16; N] {
    let mut a = [0u16; N];
    let mut i = 0;
    while i < N {
        a[i] = s[i] as u16;
        i += 1;
    }
    a
}

fn exe_base() -> usize {
    let peb = teb_peb();
    if peb == 0 {
        return 0;
    }
    unsafe { core::ptr::read((peb + PEB_IMAGE_BASE) as *const usize) }
}

/// `GetModuleHandleW` [real for NULL]: NULL -> PEB->ImageBaseAddress; any name ->
/// NULL + ERROR_MOD_NOT_FOUND (the CRT then takes its static fallbacks).
///
/// # Safety
/// `name` is null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetModuleHandleW(name: *const u16) -> usize {
    if name.is_null() {
        return exe_base();
    }
    SetLastError(ERROR_MOD_NOT_FOUND);
    0
}
/// `GetModuleHandleA` [real for NULL], see `GetModuleHandleW`.
///
/// # Safety
/// `name` is null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetModuleHandleA(name: *const u8) -> usize {
    if name.is_null() {
        return exe_base();
    }
    SetLastError(ERROR_MOD_NOT_FOUND);
    0
}
/// `GetModuleHandleExW` [real for NULL/FROM_ADDRESS]: yields the exe base for a NULL
/// name or `GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS` (4), otherwise fails like a
/// by-name lookup.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetModuleHandleExW(
    flags: u32,
    name: *const u16,
    out: *mut usize,
) -> i32 {
    if name.is_null() || flags & 4 != 0 {
        if !out.is_null() {
            unsafe { core::ptr::write(out, exe_base()) };
        }
        return 1;
    }
    if !out.is_null() {
        unsafe { core::ptr::write(out, 0) };
    }
    SetLastError(ERROR_MOD_NOT_FOUND);
    0
}
/// `GetModuleFileNameW` [stub]: always `C:\hello.exe` (truncated to `size`; returns
/// the copied length without the NUL).
///
/// # Safety
/// `buf` must be writable for `size` u16.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetModuleFileNameW(_module: usize, buf: *mut u16, size: u32) -> u32 {
    let full = EXE_PATH.len() - 1;
    let n = full.min(size as usize);
    unsafe {
        let mut i = 0;
        while i < n {
            core::ptr::write(buf.add(i), u16::from(EXE_PATH[i]));
            i += 1;
        }
        if n < size as usize {
            core::ptr::write(buf.add(n), 0);
        }
    }
    if n < full {
        SetLastError(ERROR_INSUFFICIENT_BUFFER);
    }
    n as u32
}
/// `GetModuleFileNameA` [stub], ASCII twin of the above.
///
/// # Safety
/// `buf` must be writable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetModuleFileNameA(_module: usize, buf: *mut u8, size: u32) -> u32 {
    let full = EXE_PATH.len() - 1;
    let n = full.min(size as usize);
    unsafe {
        let mut i = 0;
        while i < n {
            core::ptr::write(buf.add(i), EXE_PATH[i]);
            i += 1;
        }
        if n < size as usize {
            core::ptr::write(buf.add(n), 0);
        }
    }
    if n < full {
        SetLastError(ERROR_INSUFFICIENT_BUFFER);
    }
    n as u32
}
/// `GetCommandLineA` [stub]: static `hello.exe` (no command line is passed yet).
#[unsafe(no_mangle)]
pub extern "system" fn GetCommandLineA() -> *const u8 {
    CMDLINE_A.as_ptr()
}
/// `GetCommandLineW` [stub].
#[unsafe(no_mangle)]
pub extern "system" fn GetCommandLineW() -> *const u16 {
    CMDLINE_W.as_ptr()
}
/// `GetEnvironmentStringsW` [real, empty]: an empty block (`\0\0`).
#[unsafe(no_mangle)]
pub extern "system" fn GetEnvironmentStringsW() -> *const u16 {
    EMPTY_ENV_W.as_ptr()
}
/// `FreeEnvironmentStringsW` [real]: the block is static; nothing to free.
#[unsafe(no_mangle)]
pub extern "system" fn FreeEnvironmentStringsW(_block: *const u16) -> i32 {
    1
}
/// `SetEnvironmentVariableW` [stub, cold]: unsupported -> 0.
#[unsafe(no_mangle)]
pub extern "system" fn SetEnvironmentVariableW(_name: *const u16, _value: *const u16) -> i32 {
    SetLastError(ERROR_CALL_NOT_IMPLEMENTED);
    0
}
/// `GetStartupInfoW` [real]: zeroed STARTUPINFOW (104 bytes on x64), `cb` set.
///
/// # Safety
/// `si` must be writable for 104 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetStartupInfoW(si: *mut u8) {
    if !si.is_null() {
        unsafe {
            memset(si, 0, 104);
            core::ptr::write(si as *mut u32, 104);
        }
    }
}

/// `GetProcAddress` [stub, DECISION]: always NULL + ERROR_PROC_NOT_FOUND, so the CRT
/// falls back to its built-in code instead of dynamic APIs (Fls*, LCMapStringEx, ...).
/// If a CRT path ever uses the result without a NULL check, provide that symbol.
#[unsafe(no_mangle)]
pub extern "system" fn GetProcAddress(_module: usize, _name: *const u8) -> usize {
    SetLastError(ERROR_PROC_NOT_FOUND);
    0
}
/// `LoadLibraryExW` [stub, DECISION]: NULL + ERROR_MOD_NOT_FOUND.
#[unsafe(no_mangle)]
pub extern "system" fn LoadLibraryExW(_name: *const u16, _file: usize, _flags: u32) -> usize {
    SetLastError(ERROR_MOD_NOT_FOUND);
    0
}
/// `LoadLibraryW` [stub]: NULL + ERROR_MOD_NOT_FOUND.
#[unsafe(no_mangle)]
pub extern "system" fn LoadLibraryW(_name: *const u16) -> usize {
    SetLastError(ERROR_MOD_NOT_FOUND);
    0
}
/// `FreeLibrary` [stub]: success.
#[unsafe(no_mangle)]
pub extern "system" fn FreeLibrary(_module: usize) -> i32 {
    1
}

// --- process / debugger / exceptions (cold) ---------------------------------------

/// `TerminateProcess` [real]: over `NtTerminateProcess`.
#[unsafe(no_mangle)]
pub extern "system" fn TerminateProcess(process: isize, code: u32) -> i32 {
    i32::from(unsafe { NtTerminateProcess(process, code as i32) } >= 0)
}
/// `IsDebuggerPresent` [stub]: FALSE.
#[unsafe(no_mangle)]
pub extern "system" fn IsDebuggerPresent() -> i32 {
    0
}
/// `SetUnhandledExceptionFilter` [stub]: previous filter = NULL.
#[unsafe(no_mangle)]
pub extern "system" fn SetUnhandledExceptionFilter(_filter: usize) -> usize {
    0
}
/// `UnhandledExceptionFilter` [stub, cold]: EXCEPTION_CONTINUE_SEARCH (0).
#[unsafe(no_mangle)]
pub extern "system" fn UnhandledExceptionFilter(_info: *const c_void) -> i32 {
    0
}
/// `InitializeSListHead` [real]: zeroes the 16-byte SLIST_HEADER.
///
/// # Safety
/// `head` must be writable for 16 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeSListHead(head: *mut u8) {
    if !head.is_null() {
        unsafe { memset(head, 0, 16) };
    }
}
/// `IsProcessorFeaturePresent` [stub]: MMX(3), SSE(6), SSE2(10) = TRUE; everything
/// else (notably PF_FASTFAIL_AVAILABLE 23, AVX2 40) = FALSE (conservative).
#[unsafe(no_mangle)]
pub extern "system" fn IsProcessorFeaturePresent(feature: u32) -> i32 {
    i32::from(matches!(feature, 3 | 6 | 10))
}
/// `RaiseException` [COLD-BUG]: no SEH dispatch exists; dies with 0xC0007201.
#[unsafe(no_mangle)]
pub extern "system" fn RaiseException(_code: u32, _flags: u32, _n: u32, _args: *const usize) {
    unsafe { NtTerminateProcess(-1, COLD_RAISE) };
}
/// `RtlCaptureContext` [stub, cold]: zeroes the CONTEXT (1232 bytes on x64).
///
/// # Safety
/// `ctx` must be writable for 1232 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn RtlCaptureContext(ctx: *mut u8) {
    if !ctx.is_null() {
        unsafe { memset(ctx, 0, 1232) };
    }
}
/// `RtlLookupFunctionEntry` [stub, cold]: NULL (no function-table lookups).
#[unsafe(no_mangle)]
pub extern "system" fn RtlLookupFunctionEntry(
    _pc: u64,
    _base: *mut u64,
    _hist: *mut c_void,
) -> *mut c_void {
    core::ptr::null_mut()
}
/// `RtlVirtualUnwind` [stub, cold]: NULL handler.
#[unsafe(no_mangle)]
pub extern "system" fn RtlVirtualUnwind(
    _kind: u32,
    _base: u64,
    _pc: u64,
    _entry: *mut c_void,
    _ctx: *mut c_void,
    _handler_data: *mut *mut c_void,
    _establisher: *mut u64,
    _ctx_ptrs: *mut c_void,
) -> *mut c_void {
    core::ptr::null_mut()
}
/// `RtlUnwindEx` [COLD-BUG]: no unwinder; dies with 0xC0007202.
#[unsafe(no_mangle)]
pub extern "system" fn RtlUnwindEx(
    _target_frame: *mut c_void,
    _target_ip: *mut c_void,
    _record: *mut c_void,
    _ret: *mut c_void,
    _ctx: *mut c_void,
    _hist: *mut c_void,
) {
    unsafe { NtTerminateProcess(-1, COLD_UNWIND) };
}

// --- heap completion ---------------------------------------------------------------

/// `HeapSize` over `RtlSizeHeap`: usable size, or `(SIZE_T)-1` on failure.
///
/// # Safety
/// `ptr` must be a live block of `heap`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn HeapSize(heap: *mut c_void, flags: u32, ptr: *const c_void) -> usize {
    unsafe { RtlSizeHeap(heap, flags, ptr) }
}
/// `HeapReAlloc` over `RtlReAllocateHeap`; null on failure (old block untouched).
///
/// # Safety
/// `ptr` must be a live block of `heap`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn HeapReAlloc(
    heap: *mut c_void,
    flags: u32,
    ptr: *mut c_void,
    size: usize,
) -> *mut c_void {
    unsafe { RtlReAllocateHeap(heap, flags, ptr, size) }
}

// --- files / console / locale (Task-3 placeholders: all names resolve) ---------------

/// `GetFileType` [real]: the kernel itself tells file handles from console ones --
/// `NtTukosQueryFileSize` succeeds only on a file handle, so success => FILE_TYPE_DISK.
/// Otherwise the three standard handles (console/serial) are FILE_TYPE_CHAR and
/// anything else FILE_TYPE_UNKNOWN (0, with ERROR_INVALID_HANDLE as documented).
#[unsafe(no_mangle)]
pub extern "system" fn GetFileType(h: isize) -> u32 {
    let mut size = 0u64;
    if unsafe { NtTukosQueryFileSize(h, &mut size) } >= 0 {
        return FILE_TYPE_DISK;
    }
    if h != 0
        && h != INVALID_HANDLE_VALUE
        && (h == GetStdHandle(STD_INPUT_HANDLE)
            || h == GetStdHandle(STD_OUTPUT_HANDLE)
            || h == GetStdHandle(STD_ERROR_HANDLE))
    {
        return FILE_TYPE_CHAR;
    }
    SetLastError(ERROR_INVALID_HANDLE);
    0
}
/// `GetConsoleMode` [T3]: FALSE, so the CRT writes through `WriteFile`.
#[unsafe(no_mangle)]
pub extern "system" fn GetConsoleMode(_h: isize, _mode: *mut u32) -> i32 {
    SetLastError(ERROR_INVALID_HANDLE);
    0
}
/// `GetConsoleOutputCP` [T3]: 437.
#[unsafe(no_mangle)]
pub extern "system" fn GetConsoleOutputCP() -> u32 {
    437
}
/// `WriteConsoleW` [T3 stub]: unsupported (GetConsoleMode is FALSE so it is unused).
#[unsafe(no_mangle)]
pub extern "system" fn WriteConsoleW(
    _h: isize,
    _buf: *const u16,
    _n: u32,
    _written: *mut u32,
    _r: *mut c_void,
) -> i32 {
    SetLastError(ERROR_CALL_NOT_IMPLEMENTED);
    0
}
/// `FlushFileBuffers` [T3]: nothing is buffered below us; success.
#[unsafe(no_mangle)]
pub extern "system" fn FlushFileBuffers(_h: isize) -> i32 {
    1
}
/// `SetStdHandle` [stub, cold]: success, value ignored.
#[unsafe(no_mangle)]
pub extern "system" fn SetStdHandle(_which: i32, _h: isize) -> i32 {
    1
}
const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034u32 as i32;
const STATUS_NAME_TOO_LONG: i32 = 0xC000_0106u32 as i32;
const STATUS_ACCESS_DENIED: i32 = 0xC000_0022u32 as i32;
const STATUS_INVALID_PARAMETER: i32 = 0xC000_000Du32 as i32;
/// Longest accepted path in UTF-16 units (MAX_PATH); the kernel enforces 520 bytes.
const MAX_PATH_CHARS: usize = 260;

/// Maps an NTSTATUS from the file syscalls to a Win32 error code.
fn status_to_win32(st: i32) -> u32 {
    match st {
        STATUS_OBJECT_NAME_NOT_FOUND => ERROR_FILE_NOT_FOUND,
        STATUS_ACCESS_DENIED => ERROR_ACCESS_DENIED,
        STATUS_NAME_TOO_LONG => ERROR_FILENAME_EXCED_RANGE,
        STATUS_INVALID_PARAMETER => ERROR_INVALID_PARAMETER,
        _ => ERROR_INVALID_HANDLE,
    }
}

/// `CreateFileW` [real, read-only FS]: forwards to `NtTukosOpenFile` with the access
/// mask. The in-kernel initrd is read-only, so any write-class access is refused by
/// the kernel with ACCESS_DENIED -> ERROR_ACCESS_DENIED; creation dispositions
/// (CREATE_NEW/CREATE_ALWAYS/TRUNCATE_EXISTING) are write operations too and are
/// refused the same way. Share mode, security attributes, flags and template are
/// ignored. Missing file -> ERROR_FILE_NOT_FOUND; path > 260 chars ->
/// ERROR_FILENAME_EXCED_RANGE. Failure returns INVALID_HANDLE_VALUE.
///
/// # Safety
/// `name` is null or a NUL-terminated UTF-16 string.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn CreateFileW(
    name: *const u16,
    access: u32,
    _share: u32,
    _sa: *const c_void,
    disp: u32,
    _attrs: u32,
    _template: isize,
) -> isize {
    if name.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return INVALID_HANDLE_VALUE;
    }
    // CREATE_NEW(1)/CREATE_ALWAYS(2)/TRUNCATE_EXISTING(5) always write.
    if matches!(disp, 1 | 2 | 5) {
        SetLastError(ERROR_ACCESS_DENIED);
        return INVALID_HANDLE_VALUE;
    }
    // Bounded scan: at most MAX_PATH_CHARS + 1 units to find the terminator.
    let mut len = 0usize;
    while len <= MAX_PATH_CHARS && unsafe { core::ptr::read_volatile(name.add(len)) } != 0 {
        len += 1;
    }
    if len > MAX_PATH_CHARS {
        SetLastError(ERROR_FILENAME_EXCED_RANGE);
        return INVALID_HANDLE_VALUE;
    }
    let mut h: isize = 0;
    let st = unsafe { NtTukosOpenFile(&mut h, name, (len * 2) as u32, access) };
    if st < 0 {
        SetLastError(status_to_win32(st));
        return INVALID_HANDLE_VALUE;
    }
    h
}
/// Synthetic process/thread id source for `CreateProcessA` (the kernel has no pids yet).
static NEXT_SYNTH_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0x100);

const ERROR_NOT_ENOUGH_MEMORY_PROC: u32 = 8;
const STATUS_INSUFFICIENT_RESOURCES: i32 = 0xC000_009Au32 as i32;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_FAILED: u32 = 0xFFFF_FFFF;

/// `CreateProcessA` [real, staged]: runs an initrd `.exe` as a new process through
/// `NtTukosCreateProcess`. The program is `lpApplicationName` when non-null, else the
/// first token of `lpCommandLine` (up to the first space/tab, or the quoted span if it
/// starts with `"`). The ANSI bytes are widened to UTF-16LE (each byte -> one unit) into
/// a bounded 260-unit stack buffer; longer -> ERROR_FILENAME_EXCED_RANGE. No `.exe`
/// suffix or PATH search is added. `lpProcessInformation` (PROCESS_INFORMATION, 24
/// bytes: hProcess@0, hThread@8, dwProcessId@16, dwThreadId@20) is written field by
/// field: hProcess = the process handle, hThread = the same handle value (the kernel
/// has no thread objects; do not CloseHandle both), ids = synthetic nonzero values from
/// a per-process counter. Process/thread attributes, bInheritHandles, creation flags,
/// environment, current directory and STARTUPINFOA are ignored: the child gets a fresh
/// handle table with only the console. Missing file -> ERROR_FILE_NOT_FOUND.
///
/// # Safety
/// Strings are null or NUL-terminated; `lpProcessInformation` is null or 24 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn CreateProcessA(
    app_name: *const u8,
    cmd_line: *mut u8,
    _proc_attrs: *const c_void,
    _thread_attrs: *const c_void,
    _inherit: i32,
    _flags: u32,
    _env: *const c_void,
    _cwd: *const u8,
    _startup_info: *const c_void,
    proc_info: *mut u8,
) -> i32 {
    let (src, quoted_cmd) = if !app_name.is_null() {
        (app_name, false)
    } else if !cmd_line.is_null() {
        (cmd_line as *const u8, true)
    } else {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    };
    let mut i = 0usize;
    let mut quoted = false;
    if quoted_cmd && unsafe { core::ptr::read_volatile(src) } == b'"' {
        quoted = true;
        i = 1;
    }
    let mut buf = [0u16; MAX_PATH_CHARS];
    let mut n = 0usize;
    loop {
        let b = unsafe { core::ptr::read_volatile(src.add(i)) };
        let end =
            b == 0 || (quoted && b == b'"') || (quoted_cmd && !quoted && (b == b' ' || b == b'\t'));
        if end {
            break;
        }
        if n == MAX_PATH_CHARS {
            SetLastError(ERROR_FILENAME_EXCED_RANGE);
            return 0;
        }
        buf[n] = u16::from(b);
        n += 1;
        i += 1;
    }
    if n == 0 {
        SetLastError(ERROR_FILE_NOT_FOUND);
        return 0;
    }
    let mut h: isize = 0;
    let st = unsafe { NtTukosCreateProcess(buf.as_ptr(), (n * 2) as u32, &mut h) };
    if st < 0 {
        SetLastError(match st {
            STATUS_INSUFFICIENT_RESOURCES => ERROR_NOT_ENOUGH_MEMORY_PROC,
            other => status_to_win32(other),
        });
        return 0;
    }
    if !proc_info.is_null() {
        use core::sync::atomic::Ordering;
        let pid = NEXT_SYNTH_ID.fetch_add(4, Ordering::Relaxed);
        let tid = NEXT_SYNTH_ID.fetch_add(4, Ordering::Relaxed);
        unsafe {
            core::ptr::write_unaligned(proc_info as *mut isize, h); // hProcess @0
            core::ptr::write_unaligned(proc_info.add(8) as *mut isize, h); // hThread @8
            core::ptr::write_unaligned(proc_info.add(16) as *mut u32, pid); // dwProcessId @16
            core::ptr::write_unaligned(proc_info.add(20) as *mut u32, tid); // dwThreadId @20
        }
    }
    1
}

/// `WaitForSingleObject` [real, staged]: only process handles are supported. Always
/// blocks until the process exits (the timeout is ignored, i.e. every value behaves as
/// INFINITE) and returns WAIT_OBJECT_0; a non-process handle -> WAIT_FAILED with
/// ERROR_INVALID_HANDLE.
#[unsafe(no_mangle)]
pub extern "system" fn WaitForSingleObject(handle: isize, _millis: u32) -> u32 {
    if unsafe { NtTukosWait(handle) } < 0 {
        SetLastError(ERROR_INVALID_HANDLE);
        return WAIT_FAILED;
    }
    WAIT_OBJECT_0
}

/// `GetExitCodeProcess` [real]: the exit code of the process, or STILL_ACTIVE (259) if
/// it has not exited. FALSE with ERROR_INVALID_HANDLE for a non-process handle.
///
/// # Safety
/// `code` is null or a writable u32.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetExitCodeProcess(handle: isize, code: *mut u32) -> i32 {
    let mut v = 0u32;
    if unsafe { NtTukosQueryExitCode(handle, &mut v) } < 0 {
        SetLastError(ERROR_INVALID_HANDLE);
        return 0;
    }
    if code.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    }
    unsafe { core::ptr::write_unaligned(code, v) };
    1
}

/// `FindFirstFileExW` [stub, cold]: INVALID_HANDLE_VALUE + ERROR_FILE_NOT_FOUND.
#[unsafe(no_mangle)]
pub extern "system" fn FindFirstFileExW(
    _name: *const u16,
    _level: u32,
    _data: *mut c_void,
    _op: u32,
    _filter: *const c_void,
    _flags: u32,
) -> isize {
    SetLastError(ERROR_FILE_NOT_FOUND);
    INVALID_HANDLE_VALUE
}
/// `FindNextFileW` [stub, cold]: FALSE (ERROR_NO_MORE_FILES).
#[unsafe(no_mangle)]
pub extern "system" fn FindNextFileW(_h: isize, _data: *mut c_void) -> i32 {
    SetLastError(18);
    0
}
/// `FindClose` [stub, cold]: TRUE.
#[unsafe(no_mangle)]
pub extern "system" fn FindClose(_h: isize) -> i32 {
    1
}
/// `GetFileSizeEx` [real]: size via `NtTukosQueryFileSize`; FALSE on a non-file handle.
///
/// # Safety
/// `size` is null or writable (8 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetFileSizeEx(h: isize, size: *mut i64) -> i32 {
    let mut v = 0u64;
    let st = unsafe { NtTukosQueryFileSize(h, &mut v) };
    if st < 0 || size.is_null() {
        SetLastError(if st < 0 {
            status_to_win32(st)
        } else {
            ERROR_INVALID_PARAMETER
        });
        return 0;
    }
    unsafe { core::ptr::write_unaligned(size, v as i64) };
    1
}
/// `SetFilePointerEx` [real]: FILE_BEGIN(0)/CURRENT(1)/END(2) via `NtTukosSetFilePointer`
/// (position clamped to the file size by the kernel; negative result -> FALSE with
/// ERROR_INVALID_PARAMETER). Writes the new position when `new` is non-null.
///
/// # Safety
/// `new` is null or writable (8 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn SetFilePointerEx(
    h: isize,
    dist: i64,
    new: *mut i64,
    method: u32,
) -> i32 {
    let mut pos = 0u64;
    let st = unsafe { NtTukosSetFilePointer(h, dist, method, &mut pos) };
    if st < 0 {
        SetLastError(status_to_win32(st));
        return 0;
    }
    if !new.is_null() {
        unsafe { core::ptr::write_unaligned(new, pos as i64) };
    }
    1
}
/// `SetFilePointer` [real, thin wrapper]: 32-bit legacy form over `SetFilePointerEx`
/// (honours `high` as the upper 32 bits of the distance and returns the new position's
/// high half there); returns INVALID_SET_FILE_POINTER (0xFFFFFFFF) on failure.
///
/// # Safety
/// `high` is null or read/writable (4 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn SetFilePointer(
    h: isize,
    low: i32,
    high: *mut i32,
    method: u32,
) -> u32 {
    let dist = if high.is_null() {
        i64::from(low)
    } else {
        (i64::from(unsafe { core::ptr::read_unaligned(high) }) << 32) | i64::from(low as u32)
    };
    let mut pos = 0i64;
    if unsafe { SetFilePointerEx(h, dist, &mut pos, method) } == 0 {
        return 0xFFFF_FFFF;
    }
    if !high.is_null() {
        unsafe { core::ptr::write_unaligned(high, (pos >> 32) as i32) };
    }
    SetLastError(0);
    pos as u32
}
/// `SetEndOfFile` [stub]: the initrd is read-only, so FALSE + ERROR_ACCESS_DENIED.
/// (`fopen("rb")` never calls it; this just resolves the import.)
#[unsafe(no_mangle)]
pub extern "system" fn SetEndOfFile(_h: isize) -> i32 {
    SetLastError(ERROR_ACCESS_DENIED);
    0
}
/// `GetACP` [T3]: 1252.
#[unsafe(no_mangle)]
pub extern "system" fn GetACP() -> u32 {
    1252
}
/// `GetOEMCP` [T3]: 437.
#[unsafe(no_mangle)]
pub extern "system" fn GetOEMCP() -> u32 {
    437
}
/// `IsValidCodePage` [T3]: TRUE for 437/1252/65001.
#[unsafe(no_mangle)]
pub extern "system" fn IsValidCodePage(cp: u32) -> i32 {
    i32::from(matches!(cp, 437 | 1252 | 65001))
}
/// `GetCPInfo` [T3]: single-byte code page (MaxCharSize 1, DefaultChar '?').
///
/// # Safety
/// `info` must be writable for a CPINFO (20 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetCPInfo(_cp: u32, info: *mut u8) -> i32 {
    if info.is_null() {
        return 0;
    }
    unsafe {
        memset(info, 0, 20);
        core::ptr::write(info as *mut u32, 1); // MaxCharSize
        core::ptr::write(info.add(4), b'?'); // DefaultChar[0]
    }
    1
}

unsafe fn cstr_len(p: *const u8) -> usize {
    let mut n = 0;
    while unsafe { core::ptr::read_volatile(p.add(n)) } != 0 {
        n += 1;
    }
    n
}
unsafe fn wstr_len(p: *const u16) -> usize {
    let mut n = 0;
    while unsafe { core::ptr::read_volatile(p.add(n)) } != 0 {
        n += 1;
    }
    n
}

/// `MultiByteToWideChar` [T3, ASCII/Latin-1 identity]. Returns the required/written
/// count.
///
/// # Safety
/// `src` readable for `n` (or NUL-terminated if -1); `dst` writable for `dn` u16.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MultiByteToWideChar(
    _cp: u32,
    _flags: u32,
    src: *const u8,
    n: i32,
    dst: *mut u16,
    dn: i32,
) -> i32 {
    unsafe {
        let len = if n < 0 { cstr_len(src) + 1 } else { n as usize };
        if dn == 0 {
            return len as i32;
        }
        if len > dn as usize {
            SetLastError(ERROR_INSUFFICIENT_BUFFER);
            return 0;
        }
        let mut i = 0;
        while i < len {
            core::ptr::write(dst.add(i), u16::from(core::ptr::read(src.add(i))));
            i += 1;
        }
        len as i32
    }
}
/// `WideCharToMultiByte` [T3, ASCII identity; non-ASCII -> '?'].
///
/// # Safety
/// `src` readable for `n` (or NUL-terminated if -1); `dst` writable for `dn` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn WideCharToMultiByte(
    _cp: u32,
    _flags: u32,
    src: *const u16,
    n: i32,
    dst: *mut u8,
    dn: i32,
    _def: *const u8,
    _used: *mut i32,
) -> i32 {
    unsafe {
        let len = if n < 0 { wstr_len(src) + 1 } else { n as usize };
        if dn == 0 {
            return len as i32;
        }
        if len > dn as usize {
            SetLastError(ERROR_INSUFFICIENT_BUFFER);
            return 0;
        }
        let mut i = 0;
        while i < len {
            let c = core::ptr::read(src.add(i));
            core::ptr::write(dst.add(i), if c < 0x80 { c as u8 } else { b'?' });
            i += 1;
        }
        len as i32
    }
}
/// `CompareStringW` [T3, ordinal]: 1 = LESS, 2 = EQUAL, 3 = GREATER.
///
/// # Safety
/// Strings readable for their counts (or NUL-terminated if -1).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn CompareStringW(
    _locale: u32,
    _flags: u32,
    a: *const u16,
    an: i32,
    b: *const u16,
    bn: i32,
) -> i32 {
    unsafe {
        let la = if an < 0 { wstr_len(a) } else { an as usize };
        let lb = if bn < 0 { wstr_len(b) } else { bn as usize };
        let mut i = 0;
        while i < la.min(lb) {
            let (x, y) = (core::ptr::read(a.add(i)), core::ptr::read(b.add(i)));
            if x != y {
                return if x < y { 1 } else { 3 };
            }
            i += 1;
        }
        match la.cmp(&lb) {
            core::cmp::Ordering::Less => 1,
            core::cmp::Ordering::Equal => 2,
            core::cmp::Ordering::Greater => 3,
        }
    }
}
/// `LCMapStringW` [T3, ASCII]: copies, applying LCMAP_LOWERCASE (0x100) /
/// LCMAP_UPPERCASE (0x200) to ASCII letters.
///
/// # Safety
/// `src` readable for `n` (or NUL-terminated if -1); `dst` writable for `dn` u16.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn LCMapStringW(
    _locale: u32,
    flags: u32,
    src: *const u16,
    n: i32,
    dst: *mut u16,
    dn: i32,
) -> i32 {
    unsafe {
        let len = if n < 0 { wstr_len(src) + 1 } else { n as usize };
        if dn == 0 {
            return len as i32;
        }
        if len > dn as usize {
            SetLastError(ERROR_INSUFFICIENT_BUFFER);
            return 0;
        }
        let mut i = 0;
        while i < len {
            let mut c = core::ptr::read(src.add(i));
            if flags & 0x100 != 0 && (0x41..=0x5A).contains(&c) {
                c += 0x20;
            } else if flags & 0x200 != 0 && (0x61..=0x7A).contains(&c) {
                c -= 0x20;
            }
            core::ptr::write(dst.add(i), c);
            i += 1;
        }
        len as i32
    }
}
/// `GetStringTypeW` [T3, ASCII CT_CTYPE1 only; others 0].
///
/// # Safety
/// `src` readable for `n` (or NUL-terminated if -1); `out` writable for that many u16.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetStringTypeW(
    kind: u32,
    src: *const u16,
    n: i32,
    out: *mut u16,
) -> i32 {
    unsafe {
        let len = if n < 0 { wstr_len(src) + 1 } else { n as usize };
        let mut i = 0;
        while i < len {
            let c = core::ptr::read(src.add(i));
            let mut t = 0u16;
            if kind == 1 && c < 0x80 {
                let b = c as u8;
                if b.is_ascii_uppercase() {
                    t |= 0x1 | 0x100;
                }
                if b.is_ascii_lowercase() {
                    t |= 0x2 | 0x100;
                }
                if b.is_ascii_digit() {
                    t |= 0x4;
                }
                if matches!(b, b' ' | 9..=13) {
                    t |= 0x8;
                }
                if b.is_ascii_punctuation() {
                    t |= 0x10;
                }
                if b.is_ascii_control() {
                    t |= 0x20;
                }
                if b == b' ' || b == 9 {
                    t |= 0x40;
                }
                if b.is_ascii_hexdigit() {
                    t |= 0x80;
                }
            }
            core::ptr::write(out.add(i), t);
            i += 1;
        }
    }
    1
}
