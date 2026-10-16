//! TukOS system call dispatch.

#[cfg(feature = "ktest")]
use arch_x86_64::interrupts::IrqMutex;
use core::sync::atomic::{AtomicU64, Ordering};

extern crate alloc;

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE_MARK: u64 = 1;

pub const NT_WRITE_FILE: u64 = 2;
pub const NT_TERMINATE_PROCESS: u64 = 3;
pub const NT_CLOSE: u64 = 4;
pub const NT_ALLOCATE_VIRTUAL_MEMORY: u64 = 5;
pub const NT_FREE_VIRTUAL_MEMORY: u64 = 6;
pub const NT_READ_FILE: u64 = 7;
/// TukOS-specific (not the real NtOpenFile signature): see `nt_open_file`.
pub const NT_OPEN_FILE: u64 = 8;
/// `(handle, distance_i64, method, out_new_offset_u64_ptr)`; see `nt_set_file_pointer`.
pub const NT_SET_FILE_POINTER: u64 = 9;
/// `(handle, out_size_u64_ptr)`; see `nt_query_file_size`.
pub const NT_QUERY_FILE_SIZE: u64 = 10;
/// TukOS-specific `(path_utf16, path_len_bytes, out_process_handle_ptr)`; see `nt_create_process`.
pub const NT_CREATE_PROCESS: u64 = 11;
/// `(process_handle)` -> 0 (WAIT_OBJECT_0) once the process exited.
pub const NT_WAIT: u64 = 12;
/// `(process_handle, out_u32_ptr)`; writes the exit code or STILL_ACTIVE (259).
pub const NT_QUERY_EXIT_CODE: u64 = 13;
/// `(window_id, out_fbinfo_ptr)`: writes `FbInfo { width, height, bpp }` (3 x u32) for
/// the window's CLIENT area; window_id 0 (or unknown) returns the screen size. (Plano 25
/// added window_id as arg 0; it draws into that window's backbuffer, not the screen.)
pub const NT_GDI_GET_FB_INFO: u64 = 14;
/// `(window_id, x, y, w, h, color)`; h and color (0x00RRGGBB) on the user stack. Draws
/// into window_id's backbuffer (client coords, clamped); 0 = the caller's implicit
/// full-screen compat window; then composites.
pub const NT_GDI_FILL_RECT: u64 = 15;
/// `(window_id, x, y, str_ptr, len, fg, bg)`; len/fg/bg on the user stack. Draws into
/// window_id's backbuffer (client coords, clamped); then composites.
pub const NT_GDI_TEXT_OUT: u64 = 16;
/// `(out_key_event_ptr)`: BLOCKS until a key event; writes `KeyEventAbi { vk, ch, flags }`
/// (3 x u32, flags bit0 = down).
pub const NT_USER_GET_KEY_EVENT: u64 = 17;
/// `(out_input_event_ptr)`: BLOCKS until a ROUTED input event destined for one of the
/// caller's windows; writes `InputEventAbi` = 6 x u32 `{ tag, a, b, c, d, hwnd }`.
/// tag 0 = key:   a = vk, b = ch, c = down (0/1), d = 0.
/// tag 1 = mouse: a = x, b = y, in the target window's CLIENT space (i32 as u32),
///                c = buttons (bit0 left, bit1 right, bit2 middle),
///                d = kind: 0 = move, 1 = left down, 2 = left up, 3 = right down,
///                4 = right up, 5 = middle down, 6 = middle up.
/// hwnd = the target window id (mouse -> window under the cursor; key -> focused window;
/// a button-down raises+focuses the hit window). Nr 17 keeps returning only KEY events.
pub const NT_USER_GET_INPUT_EVENT: u64 = 18;
/// `(x, y, w, h, style, out_hwnd_ptr)`; style and out_hwnd on the user stack. Creates a
/// window owned by the caller (size >= screen or style bit0 -> full-screen compat) and
/// writes its id (u64) to out_hwnd; then composites.
pub const NT_USER_CREATE_WINDOW: u64 = 19;
/// `(hwnd)`: destroys the window (frees its backbuffer); then composites.
pub const NT_USER_DESTROY_WINDOW: u64 = 20;
/// `(hwnd)`: marks the window dirty and recomposes the screen.
pub const NT_USER_FLUSH_WINDOW: u64 = 21;
/// Max bytes accepted by NtGdiTextOut.
const GDI_TEXT_MAX: u64 = 4096;
#[cfg(feature = "ktest")]
pub const SYS_SUM6: u64 = 100;
pub const STATUS_SUCCESS: u64 = 0;
pub const STATUS_ACCESS_VIOLATION: u64 = 0xC000_0005;
pub const STATUS_INVALID_HANDLE: u64 = 0xC000_0008;
pub const STATUS_INVALID_PARAMETER: u64 = 0xC000_000D;
pub const STATUS_OBJECT_NAME_NOT_FOUND: u64 = 0xC000_0034;
pub const STATUS_ACCESS_DENIED: u64 = 0xC000_0022;
pub const STATUS_NAME_TOO_LONG: u64 = 0xC000_0106;
pub const STATUS_UNSUCCESSFUL: u64 = 0xC000_0001;
pub const STATUS_INSUFFICIENT_RESOURCES: u64 = 0xC000_009A;
/// Wait result for a signaled object (WAIT_OBJECT_0).
pub const WAIT_OBJECT_0: u64 = 0;

#[cfg(feature = "ktest")]
pub static LAST_WRITE_LEN: AtomicU64 = AtomicU64::new(0);
/// Status passed to the most recent NtTerminateProcess (u64::MAX = none yet).
#[cfg(any(feature = "ktest", feature = "conformance"))]
pub static LAST_TERMINATE_STATUS: AtomicU64 = AtomicU64::new(u64::MAX);
#[cfg(feature = "ktest")]
static LAST_WRITE: IrqMutex<[u8; 64]> = IrqMutex::new([0; 64]);
#[cfg(feature = "ktest")]
pub fn last_write_is(expected: &[u8]) -> bool {
    let g = LAST_WRITE.lock();
    expected.len() <= 64 && g[..expected.len()] == *expected
}

pub static LAST_MARK: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    arch_x86_64::syscall::set_dispatch(dispatch);
    unsafe {
        arch_x86_64::syscall::init(
            arch_x86_64::syscall::tukos_syscall_entry as *const () as usize as u64,
        )
    };
}

/// Reads stack argument `index` (0-based beyond the 4 register args) of a
/// Windows x64 syscall: `[user_rsp + 0x28 + index*8]` (return slot + 32-byte shadow).
pub fn stack_arg(user_rsp: u64, index: u64) -> Option<u64> {
    let addr = user_rsp
        .checked_add(0x28)?
        .checked_add(index.checked_mul(8)?)?;
    let bytes = crate::usercopy::copy_from_user(addr, 8)?;
    Some(u64::from_le_bytes(bytes.as_slice().try_into().ok()?))
}

/// Everything user programs wrote to the console via NtWriteFile (ktest only; first 1 KiB).
#[cfg(feature = "ktest")]
static CONSOLE_LOG: IrqMutex<([u8; 1024], usize)> = IrqMutex::new(([0; 1024], 0));
#[cfg(feature = "ktest")]
pub fn console_log_reset() {
    CONSOLE_LOG.lock().1 = 0;
}
/// Byte offset of the first occurrence of `needle` at or after `from` in the console log.
#[cfg(feature = "ktest")]
pub fn console_log_find(needle: &[u8], from: usize) -> Option<usize> {
    let g = CONSOLE_LOG.lock();
    let hay = &g.0[..g.1];
    if from > hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// Console echo log (ktest only): everything `nt_read_file` echoed.
#[cfg(feature = "ktest")]
static ECHO: IrqMutex<([u8; 64], usize)> = IrqMutex::new(([0; 64], 0));
#[cfg(feature = "ktest")]
pub fn echo_reset() {
    ECHO.lock().1 = 0;
}
#[cfg(feature = "ktest")]
pub fn echo_is(expected: &[u8]) -> bool {
    let g = ECHO.lock();
    g.1 == expected.len() && g.0[..g.1] == *expected
}

/// Writes `bytes` to the console (framebuffer + serial via `kprint!`) as an echo.
fn console_echo(bytes: &[u8]) {
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::kprint!("{s}");
    }
    #[cfg(feature = "ktest")]
    {
        let mut g = ECHO.lock();
        for &b in bytes {
            if g.1 < 64 {
                let i = g.1;
                g.0[i] = b;
                g.1 += 1;
            }
        }
    }
}

/// Most bytes one NtReadFile call returns (a console line is short).
const MAX_CONSOLE_READ: usize = 4096;

/// NtReadFile on the console-input handle: a blocking, line-disciplined read.
/// Bytes come from the keyboard queue and are echoed; Backspace (0x08/0x7F) erases
/// the last typed char of this call's line (echoing "\b \b"); Enter ends the read
/// and is returned as '\n'; the read also ends when `len` bytes are collected.
/// Other handles (files) are not supported yet: STATUS_INVALID_HANDLE.
///
/// Blocking: when the queue is empty the thread drains the i8042 itself (the tick
/// cannot fire while the syscall runs with IF=0) and yields. No lock is held
/// across `yield_now`: `try_pop_input` takes and releases its own lock.
fn nt_read_file(handle: u64, iosb: u64, buf: u64, len: u64) -> u64 {
    let len = len as u32 as u64; // ULONG: the upper half of the slot is undefined
    let obj = crate::sched::with_current_handles(|t| t.get(handle).cloned()).flatten();
    if matches!(obj, Some(crate::object::Object::File { .. })) {
        return nt_read_initrd_file(handle, iosb, buf, len);
    }
    if obj != Some(crate::object::Object::ConsoleInput) {
        return STATUS_INVALID_HANDLE;
    }
    let max = core::cmp::min(len, MAX_CONSOLE_READ as u64) as usize;
    // Validate the destination (and zero it) BEFORE consuming any input.
    if !crate::usercopy::copy_to_user(buf, &[0u8; MAX_CONSOLE_READ][..max])
        || !crate::usercopy::copy_to_user(iosb, &[0u8; 16])
    {
        return STATUS_ACCESS_VIOLATION;
    }
    let mut line = alloc::vec::Vec::with_capacity(max);
    while line.len() < max {
        let Some(b) = crate::kbd::try_pop_input() else {
            crate::kbd::poll();
            crate::sched::yield_now();
            continue;
        };
        match b {
            0x08 | 0x7F => {
                if line.pop().is_some() {
                    console_echo(b"\x08 \x08");
                }
            }
            b'\n' | b'\r' => {
                line.push(b'\n');
                console_echo(b"\n");
                break;
            }
            _ => {
                line.push(b);
                console_echo(&[b]);
            }
        }
    }
    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&STATUS_SUCCESS.to_le_bytes());
    block[8..].copy_from_slice(&(line.len() as u64).to_le_bytes());
    if !crate::usercopy::copy_to_user(buf, &line) || !crate::usercopy::copy_to_user(iosb, &block) {
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// NtReadFile on an initrd file handle: copies `min(len, size - offset)` bytes
/// (0 at EOF, still success) and advances the per-handle offset only after the
/// user buffer and IO_STATUS_BLOCK were written successfully.
fn nt_read_initrd_file(handle: u64, iosb: u64, buf: u64, len: u64) -> u64 {
    let Some(Some(data)) =
        crate::sched::with_current_handles(|t| t.file_peek(handle, len as usize))
    else {
        return STATUS_INVALID_HANDLE;
    };
    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&STATUS_SUCCESS.to_le_bytes());
    block[8..].copy_from_slice(&(data.len() as u64).to_le_bytes());
    if !crate::usercopy::copy_to_user(buf, data) || !crate::usercopy::copy_to_user(iosb, &block) {
        return STATUS_ACCESS_VIOLATION;
    }
    let _ = crate::sched::with_current_handles(|t| t.file_commit(handle, data.len()));
    STATUS_SUCCESS
}

/// Longest accepted path, in bytes of UTF-16 (260 WCHARs = MAX_PATH).
const MAX_PATH_BYTES: u64 = 520;
/// Access bits that imply modification: FILE_WRITE_DATA, FILE_APPEND_DATA,
/// FILE_WRITE_ATTRIBUTES, DELETE, WRITE_DAC, WRITE_OWNER, GENERIC_WRITE/ALL.
const WRITE_ACCESS_MASK: u32 =
    0x0002 | 0x0004 | 0x0100 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x4000_0000 | 0x1000_0000;

/// Opens an initrd file by raw UTF-16LE path bytes into `table`. Pure core of
/// `nt_open_file` (no user memory): decodes with a bound, refuses write access,
/// normalizes + looks up, inserts `Object::File`. Errors allocate nothing.
pub fn open_file_in(
    table: &mut crate::object::HandleTable,
    path: &[u8],
    access: u32,
) -> Result<u64, u64> {
    if path.len() as u64 > MAX_PATH_BYTES {
        return Err(STATUS_NAME_TOO_LONG);
    }
    if !path.len().is_multiple_of(2) {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if access & WRITE_ACCESS_MASK != 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    let units = path
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]));
    let mut s = alloc::string::String::with_capacity(path.len() / 2);
    for r in char::decode_utf16(units) {
        match r {
            Ok(c) if c.is_ascii() => s.push(c),
            // Non-ASCII / unpaired surrogate: nothing in the initrd can match.
            _ => return Err(STATUS_OBJECT_NAME_NOT_FOUND),
        }
    }
    let content = crate::fs::lookup(&s).ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
    Ok(table.insert(crate::object::Object::File { content, offset: 0 }))
}

/// NT_OPEN_FILE (nr 8), TukOS-specific, all four args in registers:
///   a0 = `HANDLE* out_handle` (u64 written on success only; untouched on error)
///   a1 = `const WCHAR* path` (UTF-16LE, NOT NUL-terminated)
///   a2 = path length in BYTES (ULONG, <= 520)
///   a3 = desired access (ULONG; any write-class bit -> STATUS_ACCESS_DENIED)
/// Status: SUCCESS; OBJECT_NAME_NOT_FOUND (missing / non-ASCII / `..` / other
/// drive); NAME_TOO_LONG; INVALID_PARAMETER (odd length); ACCESS_DENIED;
/// ACCESS_VIOLATION (bad user pointer); INVALID_HANDLE if the caller has no
/// handle table (kernel thread).
fn nt_open_file(out: u64, path_ptr: u64, path_len: u64, access: u64) -> u64 {
    let path_len = path_len as u32 as u64;
    let access = access as u32;
    if path_len > MAX_PATH_BYTES {
        return STATUS_NAME_TOO_LONG;
    }
    if !crate::sched::current_is_user() {
        return STATUS_INVALID_HANDLE;
    }
    let Some(path) = crate::usercopy::copy_from_user(path_ptr, path_len) else {
        return STATUS_ACCESS_VIOLATION;
    };
    let handle = match crate::sched::with_current_handles(|t| open_file_in(t, &path, access)) {
        Some(Ok(h)) => h,
        Some(Err(st)) => return st,
        None => return STATUS_INVALID_HANDLE,
    };
    if !crate::usercopy::copy_to_user(out, &handle.to_le_bytes()) {
        let _ = crate::sched::with_current_handles(|t| t.close(handle)); // no leak
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// Decodes `path` (UTF-16LE bytes, bounded) and starts the initrd `.exe` it names as
/// a new process (own address space, console handles 4/8 inherited). Returns the
/// exit state shared with the child. Pure core of `nt_create_process`: touches no
/// user memory and no handle table, and holds no lock across the spawn.
pub fn create_process(path: &[u8]) -> Result<alloc::sync::Arc<crate::object::ProcExit>, u64> {
    create_process_cmd(path, &[])
}

/// Longest accepted command line, in bytes of UTF-16 (256 WCHARs). Longer is rejected
/// with STATUS_NAME_TOO_LONG (never silently truncated at the syscall boundary).
pub const MAX_CMDLINE_BYTES: u64 = 512;

/// `create_process` with the child's command line (UTF-16LE bytes, even length,
/// <= `MAX_CMDLINE_BYTES`). Empty = the child's command line is the image path.
pub fn create_process_cmd(
    path: &[u8],
    cmdline: &[u8],
) -> Result<alloc::sync::Arc<crate::object::ProcExit>, u64> {
    if cmdline.len() as u64 > MAX_CMDLINE_BYTES {
        return Err(STATUS_NAME_TOO_LONG);
    }
    if !cmdline.len().is_multiple_of(2) {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if path.len() as u64 > MAX_PATH_BYTES {
        return Err(STATUS_NAME_TOO_LONG);
    }
    if !path.len().is_multiple_of(2) {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let units = path
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]));
    let mut s = alloc::string::String::with_capacity(path.len() / 2);
    for r in char::decode_utf16(units) {
        match r {
            Ok(c) if c.is_ascii() => s.push(c),
            _ => return Err(STATUS_OBJECT_NAME_NOT_FOUND),
        }
    }
    let image = crate::fs::lookup(&s).ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
    let exit = crate::object::ProcExit::new();
    // Empty command line -> the image path as given (so argv[0] exists).
    let cmd: alloc::vec::Vec<u16> = if cmdline.is_empty() {
        path.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    } else {
        cmdline
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    };
    match crate::process::spawn_exe_image_with_k32_cmd(image.to_vec(), Some(exit.clone()), &cmd) {
        Ok(()) => Ok(exit),
        Err(crate::process::SpawnError::NoDlls) => Err(STATUS_UNSUCCESSFUL),
        Err(crate::process::SpawnError::NoThreads) => Err(STATUS_INSUFFICIENT_RESOURCES),
    }
}

/// Blocks (yielding, no lock held) until `exit` is done.
pub fn wait_for_exit(exit: &crate::object::ProcExit) {
    while !exit.is_done() {
        crate::sched::yield_now();
    }
}

/// NT_CREATE_PROCESS (nr 11): a0 = `const WCHAR* path` (UTF-16LE, not NUL-terminated),
/// a1 = path length in BYTES (<= 520), a2 = `HANDLE* out_process` (written on success
/// only). Status: SUCCESS; OBJECT_NAME_NOT_FOUND; NAME_TOO_LONG; INVALID_PARAMETER;
/// ACCESS_VIOLATION; INSUFFICIENT_RESOURCES; INVALID_HANDLE for a kernel caller.
/// Command line (5 args; arg 4 is in r9, arg 5 on the stack at `[rsp+0x28]`):
/// a3 = `const WCHAR* cmdline` (UTF-16LE, not NUL-terminated; may be NULL), stack_arg(0)
/// = cmdline length in BYTES (even, <= 512; > 512 -> NAME_TOO_LONG, odd ->
/// INVALID_PARAMETER). NULL pointer or length 0 = the child's command line is the
/// image path. It is stored in the child's PEB->ProcessParameters->CommandLine.
fn nt_create_process(path_ptr: u64, path_len: u64, out: u64, cmd_ptr: u64, cmd_len: u64) -> u64 {
    let path_len = path_len as u32 as u64;
    let cmd_len = cmd_len as u32 as u64;
    if path_len > MAX_PATH_BYTES || cmd_len > MAX_CMDLINE_BYTES {
        return STATUS_NAME_TOO_LONG;
    }
    if !crate::sched::current_is_user() {
        return STATUS_INVALID_HANDLE;
    }
    let Some(path) = crate::usercopy::copy_from_user(path_ptr, path_len) else {
        return STATUS_ACCESS_VIOLATION;
    };
    let cmdline = if cmd_ptr == 0 || cmd_len == 0 {
        alloc::vec::Vec::new()
    } else {
        match crate::usercopy::copy_from_user(cmd_ptr, cmd_len) {
            Some(c) => c,
            None => return STATUS_ACCESS_VIOLATION,
        }
    };
    // Validate the out pointer BEFORE spawning anything.
    if !crate::usercopy::copy_to_user(out, &[0u8; 8]) {
        return STATUS_ACCESS_VIOLATION;
    }
    let exit = match create_process_cmd(&path, &cmdline) {
        Ok(e) => e,
        Err(st) => return st,
    };
    let Some(handle) =
        crate::sched::with_current_handles(|t| t.insert(crate::object::Object::Process { exit }))
    else {
        return STATUS_INVALID_HANDLE;
    };
    if !crate::usercopy::copy_to_user(out, &handle.to_le_bytes()) {
        let _ = crate::sched::with_current_handles(|t| t.close(handle)); // child keeps running
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// NT_WAIT (nr 12): a0 = process handle. Resolves the handle to its `Arc`, drops the
/// handle-table lock, THEN polls/yields: no lock is held while blocked.
fn nt_wait(handle: u64) -> u64 {
    let exit = crate::sched::with_current_handles(|t| t.process_exit(handle)).flatten();
    match exit {
        Some(e) => {
            wait_for_exit(&e);
            WAIT_OBJECT_0
        }
        None => STATUS_INVALID_HANDLE,
    }
}

/// NT_QUERY_EXIT_CODE (nr 13): a0 = process handle, a1 = `u32* out` (exit code, or
/// STILL_ACTIVE=259 while running).
fn nt_query_exit_code(handle: u64, out: u64) -> u64 {
    let exit = crate::sched::with_current_handles(|t| t.process_exit(handle)).flatten();
    match exit {
        Some(e) if crate::usercopy::copy_to_user(out, &e.exit_code().to_le_bytes()) => {
            STATUS_SUCCESS
        }
        Some(_) => STATUS_ACCESS_VIOLATION,
        None => STATUS_INVALID_HANDLE,
    }
}

/// NT_SET_FILE_POINTER (nr 9): a0 = handle, a1 = signed 64-bit distance,
/// a2 = method (0 BEGIN, 1 CURRENT, 2 END), a3 = `u64* out_new_offset`.
/// Negative result -> INVALID_PARAMETER; past-EOF clamps to the size.
fn nt_set_file_pointer(handle: u64, distance: u64, method: u64, out: u64) -> u64 {
    let r =
        crate::sched::with_current_handles(|t| t.file_seek(handle, distance as i64, method as u32));
    match r {
        Some(Ok(new)) if crate::usercopy::copy_to_user(out, &new.to_le_bytes()) => STATUS_SUCCESS,
        Some(Ok(_)) => STATUS_ACCESS_VIOLATION,
        Some(Err(st)) => st,
        None => STATUS_INVALID_HANDLE,
    }
}

/// NT_QUERY_FILE_SIZE (nr 10): a0 = handle, a1 = `u64* out_size`.
fn nt_query_file_size(handle: u64, out: u64) -> u64 {
    match crate::sched::with_current_handles(|t| t.file_size(handle)) {
        Some(Some(sz)) => {
            if crate::usercopy::copy_to_user(out, &sz.to_le_bytes()) {
                STATUS_SUCCESS
            } else {
                STATUS_ACCESS_VIOLATION
            }
        }
        _ => STATUS_INVALID_HANDLE,
    }
}

/// Core of NtWriteFile once the stack arguments are decoded.
fn nt_write_file(handle: u64, iosb: u64, buf: u64, len: u64) -> u64 {
    // `Length` is a ULONG: the upper half of its stack slot is undefined (callers only
    // write the low 32 bits).
    let len = len as u32 as u64;
    let obj = crate::sched::with_current_handles(|t| t.get(handle).cloned()).flatten();
    if obj != Some(crate::object::Object::Console) {
        return STATUS_INVALID_HANDLE;
    }
    let Some(bytes) = crate::usercopy::copy_from_user(buf, len) else {
        return STATUS_ACCESS_VIOLATION;
    };
    if let Ok(s) = core::str::from_utf8(&bytes) {
        crate::kprint!("{s}");
    }
    #[cfg(feature = "ktest")]
    {
        let mut g = LAST_WRITE.lock();
        let n = core::cmp::min(bytes.len(), 64);
        g[..n].copy_from_slice(&bytes[..n]);
        drop(g);
        LAST_WRITE_LEN.store(bytes.len() as u64, Ordering::SeqCst);
        let mut log = CONSOLE_LOG.lock();
        for &b in &bytes {
            if log.1 < 1024 {
                let i = log.1;
                log.0[i] = b;
                log.1 += 1;
            }
        }
    }
    // IO_STATUS_BLOCK (x64) is { union { NTSTATUS Status; PVOID Pointer; }; ULONG_PTR
    // Information; }. Simplification: Status is stored as a full u64 (low 4 bytes =
    // NTSTATUS, high 4 = 0) at offset 0, Information as u64 at offset 8.
    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&STATUS_SUCCESS.to_le_bytes());
    block[8..].copy_from_slice(&len.to_le_bytes());
    if !crate::usercopy::copy_to_user(iosb, &block) {
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// Test hook: runs NtWriteFile's handle resolution for `handle` with an empty
/// buffer, without needing a user process.
#[cfg(feature = "ktest")]
pub fn ntwritefile_for_test(handle: u64, _a1: u64, _a2: u64, _a3: u64) -> u64 {
    nt_write_file(handle, 0, 0, 0)
}

const NT_CURRENT_PROCESS: u64 = u64::MAX; // (HANDLE)-1
const MEM_COMMIT: u64 = 0x1000;
const MEM_RELEASE: u64 = 0x8000;

fn read_u64(va: u64) -> Option<u64> {
    let b = crate::usercopy::copy_from_user(va, 8)?;
    Some(u64::from_le_bytes(b.as_slice().try_into().ok()?))
}

/// NtAllocateVirtualMemory for the current process only. `base_ptr`/`size_ptr`
/// are in/out user pointers; the result is written back only after the pages are
/// mapped, and the mapping is undone if that write-back fails.
fn nt_allocate_virtual_memory(
    process: u64,
    base_ptr: u64,
    size_ptr: u64,
    alloc_type: u64,
    protect: u64,
) -> u64 {
    if process != NT_CURRENT_PROCESS || !crate::sched::current_is_user() {
        return STATUS_INVALID_HANDLE;
    }
    let (Some(req_base), Some(req_size)) = (read_u64(base_ptr), read_u64(size_ptr)) else {
        return STATUS_ACCESS_VIOLATION;
    };
    if alloc_type as u32 as u64 & MEM_COMMIT == 0 {
        return crate::vmem::STATUS_INVALID_PARAMETER;
    }
    let root = crate::arch_cr3_base();
    let (base, size) = match crate::vmem::alloc_in(root, req_base, req_size, protect as u32) {
        Ok(r) => r,
        Err(st) => return st,
    };
    if !crate::usercopy::copy_to_user(base_ptr, &base.to_le_bytes())
        || !crate::usercopy::copy_to_user(size_ptr, &size.to_le_bytes())
    {
        let _ = crate::vmem::free_in(root, base, size);
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// NtFreeVirtualMemory (MEM_RELEASE only; the caller supplies the exact size).
fn nt_free_virtual_memory(process: u64, base_ptr: u64, size_ptr: u64, free_type: u64) -> u64 {
    if process != NT_CURRENT_PROCESS || !crate::sched::current_is_user() {
        return STATUS_INVALID_HANDLE;
    }
    let (Some(req_base), Some(req_size)) = (read_u64(base_ptr), read_u64(size_ptr)) else {
        return STATUS_ACCESS_VIOLATION;
    };
    if free_type as u32 as u64 != MEM_RELEASE {
        return crate::vmem::STATUS_INVALID_PARAMETER;
    }
    let root = crate::arch_cr3_base();
    let (base, size) = match crate::vmem::free_in(root, req_base, req_size) {
        Ok(r) => r,
        Err(st) => return st,
    };
    // The range is already gone; a bad out-pointer only reports the fault.
    if !crate::usercopy::copy_to_user(base_ptr, &base.to_le_bytes())
        || !crate::usercopy::copy_to_user(size_ptr, &size.to_le_bytes())
    {
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

fn nt_gdi_get_fb_info(window_id: u64, out: u64) -> u64 {
    // A real window id reports its client size; 0/unknown reports the screen size.
    let dims = if window_id != 0 {
        crate::wm::window_client_size(window_id as u32)
    } else {
        None
    };
    let (w, h, bpp) = match dims {
        Some((w, h)) => (w, h, 32),
        None => match crate::fbcon::fb_info() {
            Some(d) => d,
            None => return STATUS_UNSUCCESSFUL,
        },
    };
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&w.to_le_bytes());
    buf[4..8].copy_from_slice(&h.to_le_bytes());
    buf[8..12].copy_from_slice(&bpp.to_le_bytes());
    if crate::usercopy::copy_to_user(out, &buf) {
        STATUS_SUCCESS
    } else {
        STATUS_ACCESS_VIOLATION
    }
}

/// NtUserGetKeyEvent: blocking pop of the raw key-event queue. Mirrors `nt_read_file`:
/// drains the i8042 itself and yields; no lock is held across `yield_now`.
fn nt_user_get_key_event(out: u64) -> u64 {
    // Validate the destination BEFORE consuming an event.
    if !crate::usercopy::copy_to_user(out, &[0u8; 12]) {
        return STATUS_ACCESS_VIOLATION;
    }
    let ev = loop {
        if let Some(e) = crate::kbd::try_pop_key_event() {
            break e;
        }
        crate::kbd::poll();
        crate::sched::yield_now();
    };
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&(ev.vk as u32).to_le_bytes());
    buf[4..8].copy_from_slice(&ev.ch.to_le_bytes());
    buf[8..12].copy_from_slice(&(ev.down as u32).to_le_bytes());
    if crate::usercopy::copy_to_user(out, &buf) {
        STATUS_SUCCESS
    } else {
        STATUS_ACCESS_VIOLATION
    }
}

/// NtUserGetInputEvent: blocking, ROUTED pop of the unified input queue (key or mouse).
/// The kernel decides the TARGET window per event (mouse -> topmost window under the
/// cursor; key -> the focused window; a mouse button-down raises+focuses the hit window)
/// and delivers to the calling process ONLY the events destined for its own windows, so
/// one process never sees another's input. Mouse coordinates are returned in the target
/// window's CLIENT space, and the target hwnd is written in word 5 (24-byte ABI).
///
/// Blocking: the caller first drains its own mailbox, else routes raw events from the
/// shared queue (recompositing on a button-down), else polls the i8042 and yields. No lock
/// is held across the yield, and the WM->FBCON order is preserved (route then composite).
fn nt_user_get_input_event(out: u64) -> u64 {
    use kbd::{InputEvent, MouseButton, MouseKind};
    if !crate::usercopy::copy_to_user(out, &[0u8; 24]) {
        return STATUS_ACCESS_VIOLATION;
    }
    let owner = crate::sched::current_id();
    let routed = loop {
        // 1. An event already routed to one of MY windows.
        if let Some(r) = crate::wm::take_input_for(owner) {
            break r;
        }
        // 2. Route the next raw event to its target's mailbox (may be another process's).
        if let Some(raw) = crate::kbd::try_pop_input_event() {
            if let Some((r, recompose)) = crate::wm::route_input(raw) {
                if recompose {
                    crate::wm::composite();
                }
                crate::wm::deliver(r);
            }
            continue;
        }
        // 3. Nothing ready: pump the i8042 and yield (no lock held).
        crate::kbd::poll();
        crate::sched::yield_now();
    };
    let words: [u32; 5] = match routed.ev {
        InputEvent::Key { vk, ch, down } => [0, vk as u32, ch, down as u32, 0],
        InputEvent::Mouse {
            x,
            y,
            buttons,
            kind,
        } => {
            let k = match kind {
                MouseKind::Move => 0,
                MouseKind::Down(MouseButton::Left) => 1,
                MouseKind::Up(MouseButton::Left) => 2,
                MouseKind::Down(MouseButton::Right) => 3,
                MouseKind::Up(MouseButton::Right) => 4,
                MouseKind::Down(MouseButton::Middle) => 5,
                MouseKind::Up(MouseButton::Middle) => 6,
            };
            [1, x as u32, y as u32, buttons as u32, k]
        }
    };
    let mut buf = [0u8; 24];
    for (i, w) in words.iter().enumerate() {
        buf[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    buf[20..24].copy_from_slice(&routed.hwnd.to_le_bytes());
    if crate::usercopy::copy_to_user(out, &buf) {
        STATUS_SUCCESS
    } else {
        STATUS_ACCESS_VIOLATION
    }
}

fn nt_gdi_text_out(window_id: u64, x: u64, y: u64, ptr: u64, len: u64, fg: u64, bg: u64) -> u64 {
    let len = (len as u32 as u64).min(GDI_TEXT_MAX);
    let bytes = if len == 0 {
        alloc::vec::Vec::new()
    } else {
        match crate::usercopy::copy_from_user(ptr, len) {
            Some(b) => b,
            None => return STATUS_ACCESS_VIOLATION,
        }
    };
    let owner = crate::sched::current_id();
    crate::wm::text_out_in(
        owner,
        window_id as u32,
        x as u32,
        y as u32,
        &bytes,
        fg as u32,
        bg as u32,
    );
    crate::wm::composite();
    STATUS_SUCCESS
}

/// NtUserCreateWindow(x, y, w, h, style, out_hwnd): creates the caller's window and
/// writes its id (u64) to `out_hwnd`, then composites.
fn nt_user_create_window(x: u64, y: u64, w: u64, h: u64, style: u64, out: u64) -> u64 {
    let owner = crate::sched::current_id();
    let Some(id) =
        crate::wm::create_window(owner, x as i32, y as i32, w as u32, h as u32, style as u32)
    else {
        return STATUS_INSUFFICIENT_RESOURCES;
    };
    if !crate::usercopy::copy_to_user(out, &(id as u64).to_le_bytes()) {
        crate::wm::destroy_window(id);
        return STATUS_ACCESS_VIOLATION;
    }
    crate::wm::composite();
    STATUS_SUCCESS
}

pub extern "C" fn dispatch(nr: u64, a0: u64, a1: u64, a2: u64, _a3: u64, _user_rsp: u64) -> u64 {
    match nr {
        SYS_WRITE_MARK => {
            LAST_MARK.store(a0, Ordering::SeqCst);
            #[cfg(feature = "ktest")]
            crate::ktest::on_user_mark(a0);
            0
        }
        SYS_EXIT => {
            if crate::sched::current_is_user() {
                crate::sched::exit_current();
            }
            #[cfg(target_os = "none")]
            unsafe {
                arch_x86_64::usermode::exit_to_kernel(a0)
            }
            #[cfg(not(target_os = "none"))]
            a0
        }
        NT_WRITE_FILE => {
            // NtWriteFile(FileHandle, Event, ApcRoutine, ApcContext, IoStatusBlock,
            // Buffer, Length, ByteOffset, Key): a0..a3 in registers, the rest on the
            // user stack. Event/Apc*/ByteOffset/Key are ignored in this phase.
            let (Some(iosb), Some(buf), Some(len)) = (
                stack_arg(_user_rsp, 0),
                stack_arg(_user_rsp, 1),
                stack_arg(_user_rsp, 2),
            ) else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_write_file(a0, iosb, buf, len)
        }
        NT_READ_FILE => {
            // NtReadFile(FileHandle, Event, ApcRoutine, ApcContext, IoStatusBlock,
            // Buffer, Length, ByteOffset, Key): same layout as NtWriteFile.
            let (Some(iosb), Some(buf), Some(len)) = (
                stack_arg(_user_rsp, 0),
                stack_arg(_user_rsp, 1),
                stack_arg(_user_rsp, 2),
            ) else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_read_file(a0, iosb, buf, len & 0xFFFF_FFFF)
        }
        NT_OPEN_FILE => nt_open_file(a0, a1, a2, _a3),
        NT_SET_FILE_POINTER => nt_set_file_pointer(a0, a1, a2, _a3),
        NT_QUERY_FILE_SIZE => nt_query_file_size(a0, a1),
        NT_CREATE_PROCESS => {
            let Some(cmd_len) = stack_arg(_user_rsp, 0) else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_create_process(a0, a1, a2, _a3, cmd_len)
        }
        NT_GDI_GET_FB_INFO => nt_gdi_get_fb_info(a0, a1),
        NT_GDI_FILL_RECT => {
            // NtGdiFillRect(window_id, x, y, w, h, color): a0..a3 in registers, h and
            // color on the user stack.
            let (Some(h), Some(color)) = (stack_arg(_user_rsp, 0), stack_arg(_user_rsp, 1)) else {
                return STATUS_ACCESS_VIOLATION;
            };
            let owner = crate::sched::current_id();
            crate::wm::fill_rect_in(
                owner,
                a0 as u32,
                a1 as u32,
                a2 as u32,
                _a3 as u32,
                h as u32,
                color as u32,
            );
            crate::wm::composite();
            STATUS_SUCCESS
        }
        NT_GDI_TEXT_OUT => {
            // NtGdiTextOut(window_id, x, y, str_ptr, len, fg, bg): len/fg/bg on the stack.
            let (Some(len), Some(fg), Some(bg)) = (
                stack_arg(_user_rsp, 0),
                stack_arg(_user_rsp, 1),
                stack_arg(_user_rsp, 2),
            ) else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_gdi_text_out(a0, a1, a2, _a3, len, fg, bg)
        }
        NT_USER_CREATE_WINDOW => {
            // NtUserCreateWindow(x, y, w, h, style, out_hwnd): style/out_hwnd on the stack.
            let (Some(style), Some(out)) = (stack_arg(_user_rsp, 0), stack_arg(_user_rsp, 1))
            else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_user_create_window(a0, a1, a2, _a3, style, out)
        }
        NT_USER_DESTROY_WINDOW => {
            crate::wm::destroy_window(a0 as u32);
            crate::wm::composite();
            STATUS_SUCCESS
        }
        NT_USER_FLUSH_WINDOW => {
            crate::wm::composite();
            STATUS_SUCCESS
        }
        NT_USER_GET_KEY_EVENT => nt_user_get_key_event(a0),
        NT_USER_GET_INPUT_EVENT => nt_user_get_input_event(a0),
        NT_WAIT => nt_wait(a0),
        NT_QUERY_EXIT_CODE => nt_query_exit_code(a0, a1),
        NT_ALLOCATE_VIRTUAL_MEMORY => {
            // NtAllocateVirtualMemory(Process, *Base, ZeroBits, *Size, AllocType,
            // Protect): ZeroBits (a2) is ignored; the last two are on the stack.
            let (Some(alloc_type), Some(protect)) =
                (stack_arg(_user_rsp, 0), stack_arg(_user_rsp, 1))
            else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_allocate_virtual_memory(a0, a1, _a3, alloc_type, protect)
        }
        NT_FREE_VIRTUAL_MEMORY => nt_free_virtual_memory(a0, a1, a2, _a3),
        NT_CLOSE => match crate::sched::with_current_handles(|t| t.close(a0)) {
            Some(true) => STATUS_SUCCESS,
            _ => STATUS_INVALID_HANDLE,
        },
        NT_TERMINATE_PROCESS => {
            #[cfg(any(feature = "ktest", feature = "conformance"))]
            LAST_TERMINATE_STATUS.store(a1 as u32 as u64, Ordering::SeqCst);
            if crate::sched::current_is_user() {
                crate::sched::exit_current_with(a1 as u32);
            }
            #[cfg(target_os = "none")]
            unsafe {
                arch_x86_64::usermode::exit_to_kernel(a1 as u32 as u64)
            }
            #[cfg(not(target_os = "none"))]
            {
                a1 as u32 as u64
            }
        }
        #[cfg(feature = "ktest")]
        SYS_SUM6 => match (stack_arg(_user_rsp, 0), stack_arg(_user_rsp, 1)) {
            (Some(s0), Some(s1)) => {
                let sum = a0
                    .wrapping_add(a1)
                    .wrapping_add(a2)
                    .wrapping_add(_a3)
                    .wrapping_add(s0)
                    .wrapping_add(s1);
                LAST_MARK.store(sum, Ordering::SeqCst);
                0
            }
            _ => STATUS_ACCESS_VIOLATION,
        },
        _ => u64::MAX,
    }
}
