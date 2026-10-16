//! TukOS user32.dll: a single-window shim with a message loop. Painting goes to
//! the kernel through the `NtGdi*` syscalls (the framebuffer is never mapped into
//! user space); the window's client area is the whole screen.
//!
//! Signatures and struct layouts follow the public winuser.h/windef.h documentation.
//! HWND/HDC/HBRUSH are opaque handles. The only import is `ntdll.dll` (`raw-dylib`):
//! user32 shares the HBRUSH encoding with gdi32 by value (it decodes the COLORREF a
//! gdi32 brush carries), so it never needs to call into gdi32, and the loader binds
//! the GUI exe's gdi32/kernel32 imports to those DLLs directly.
//!
//! SINGLE-WINDOW / SINGLE-THREAD MODEL: a TukOS process has exactly one user thread,
//! so all window state lives in process-global statics with no locking (the existing
//! kernel32/ntdll make the same assumption). One window class, one window, one message
//! queue.
//!
//! `GetMessageA` is BLOCKING (Plano 22; it replaces Plano 21's non-blocking behaviour,
//! where an empty queue meant `WM_QUIT`). Priority order: (1) `PostQuitMessage` was
//! called -> `WM_QUIT` (returns 0); (2) a `WM_CHAR` produced by `TranslateMessage` is
//! pending; (3) a queued `WM_PAINT` (`ShowWindow`/`UpdateWindow`); (4) otherwise it
//! calls `NtUserGetInputEvent`, which blocks inside the kernel until an input event (key
//! or mouse) exists (no user-mode spinning). A mouse event becomes `WM_MOUSEMOVE` /
//! `WM_xBUTTONDOWN` / `WM_xBUTTONUP` with `lParam = (y << 16) | (x & 0xFFFF)` (client =
//! screen coordinates) and `wParam` = the `MK_*` flags of the buttons held. A key-DOWN becomes `WM_KEYDOWN` (wParam = virtual key) and
//! its character is remembered for `TranslateMessage`. Key-UP events are SKIPPED (no
//! `WM_KEYUP` is delivered): a text app only needs `WM_CHAR` from key-downs.
#![no_std]
#![allow(non_snake_case)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { NtTerminateProcess(-1, 0xC000_7108u32 as i32) };
    loop {
        unsafe { core::arch::asm!("ud2", options(nomem, nostack)) };
    }
}

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

#[unsafe(no_mangle)]
static _fltused: i32 = 0;

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtTerminateProcess(process: isize, status: i32) -> i32;
    // NtGdiGetFbInfo(out): writes [width:u32, height:u32, bpp:u32] (kernel syscall 14).
    fn NtGdiGetFbInfo(out: *mut u32) -> i32;
    // NtGdiFillRect(x, y, w, h, color): paints a 0x00RRGGBB rectangle (syscall 15).
    fn NtGdiFillRect(x: u32, y: u32, w: u32, h: u32, color: u32) -> i32;
    // NtUserGetInputEvent(out): BLOCKS in the kernel until an input event, then writes
    // 5 x u32 (syscall 18): key   = [0, vk, ch, down, 0];
    //                       mouse = [1, x, y, buttons (bit0 L, bit1 R, bit2 M), kind]
    // with kind 0 Move, 1/2 left down/up, 3/4 right down/up, 5/6 middle down/up.
    fn NtUserGetInputEvent(out: *mut u32) -> i32;
}

pub type HWND = isize;
pub type HDC = isize;
pub type HBRUSH = isize;
pub type HICON = isize;
pub type HCURSOR = isize;
pub type HINSTANCE = isize;
pub type HMENU = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type UINT = u32;
pub type BOOL = i32;
pub type ATOM = u16;
pub type LONG = i32;

/// WndProc: `LRESULT CALLBACK WndProc(HWND, UINT, WPARAM, LPARAM)`.
pub type WndProc = extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT;

// Messages used by the single-window model (winuser.h).
const WM_DESTROY: u32 = 0x0002;
const WM_PAINT: u32 = 0x000F;
const WM_QUIT: u32 = 0x0012;
const WM_KEYDOWN: u32 = 0x0100;
const WM_CHAR: u32 = 0x0102;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;

// wParam key-state flags of the mouse messages (winuser.h).
const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
const MK_MBUTTON: usize = 0x0010;

// Opaque handle sentinels (non-null, process-unique).
const THE_HWND: HWND = 0x00AB_0001;
const SCREEN_HDC: HDC = 0x00DC_0001;
const DUMMY_CURSOR: HCURSOR = 0x00CC_0001;
const DUMMY_ICON: HICON = 0x00EE_0001;

// HBRUSH encoding shared with gdi32::CreateSolidBrush: tag in the top bits, COLORREF
// (0x00BBGGRR) in the low 24. Kept in sync with crates/gdi32/src/lib.rs.
const BRUSH_TAG: isize = 0x4200_0000;

/// COLORREF (0x00BBGGRR) carried by a gdi32 brush handle, if `h` is one.
fn brush_colorref(h: HBRUSH) -> Option<u32> {
    if h & !0x00FF_FFFF == BRUSH_TAG {
        Some((h & 0x00FF_FFFF) as u32)
    } else {
        None
    }
}

/// COLORREF (0x00BBGGRR) -> framebuffer pixel (0x00RRGGBB).
fn colorref_to_rgb(c: u32) -> u32 {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

// --- public structs (x64 layout, from the survey) ---------------------------------

#[repr(C)]
pub struct RECT {
    pub left: LONG,
    pub top: LONG,
    pub right: LONG,
    pub bottom: LONG,
}

#[repr(C)]
pub struct POINT {
    pub x: LONG,
    pub y: LONG,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

// WNDCLASSA / WNDCLASSEXA: only `lpfnWndProc` matters, and it sits at offset 8 in
// both (after `style`/`cbSize+style`). We read it through the raw pointer.
const WNDCLASS_LPFNWNDPROC: usize = 8;

// --- process-global state (single thread, no locking) -----------------------------

#[derive(Clone, Copy)]
struct Queued {
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
}

const NO_MSG: Queued = Queued {
    message: 0,
    wparam: 0,
    lparam: 0,
};

const QCAP: usize = 16; // power of two: see the mask in `post`/`take`
const QMASK: usize = QCAP - 1;

struct State {
    wndproc: usize,
    queue: [Queued; QCAP],
    head: usize,
    tail: usize,
    /// `PostQuitMessage` was called: the next `GetMessageA` reports `WM_QUIT`.
    quit: bool,
    quit_code: usize,
    /// Pending `WM_CHAR` character (0 = none), set by `TranslateMessage`.
    pending_char: u32,
    /// Virtual key and character of the last `WM_KEYDOWN` handed out.
    last_vk: u32,
    last_ch: u32,
}

static mut STATE: State = State {
    wndproc: 0,
    queue: [NO_MSG; QCAP],
    head: 0,
    tail: 0,
    quit: false,
    quit_code: 0,
    pending_char: 0,
    last_vk: 0,
    last_ch: 0,
};

fn state() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

/// Enqueues a message (drops it silently if the tiny queue is full; the deterministic
/// single-window flow never enqueues more than a couple at a time).
fn post(message: u32, wparam: WPARAM, lparam: LPARAM) {
    unsafe {
        let s = &mut *core::ptr::addr_of_mut!(STATE);
        // Masking with QMASK (QCAP is a power of two) keeps every index provably < QCAP,
        // so no `panic_bounds_check` is emitted (that would drag in the MSVC CRT).
        let next = (s.tail + 1) & QMASK;
        if next == s.head {
            return;
        }
        s.queue[s.tail & QMASK] = Queued {
            message,
            wparam,
            lparam,
        };
        s.tail = next;
    }
}

/// Dequeues the next message, or None when the queue is empty.
fn take() -> Option<Queued> {
    unsafe {
        let s = &mut *core::ptr::addr_of_mut!(STATE);
        if s.head == s.tail {
            return None;
        }
        let m = s.queue[s.head & QMASK];
        s.head = (s.head + 1) & QMASK;
        Some(m)
    }
}

fn set_wndproc(p: usize) {
    unsafe { (*core::ptr::addr_of_mut!(STATE)).wndproc = p };
}

fn get_wndproc() -> usize {
    unsafe { (*core::ptr::addr_of!(STATE)).wndproc }
}

/// `(width, height)` of the screen (the window's client area), 0,0 if unavailable.
fn screen_size() -> (u32, u32) {
    let mut info = [0u32; 3];
    if unsafe { NtGdiGetFbInfo(info.as_mut_ptr()) } >= 0 {
        (info[0], info[1])
    } else {
        (0, 0)
    }
}

// --- window class / creation -------------------------------------------------------

/// `RegisterClassA`: stores the class WndProc (single class). Returns a nonzero ATOM.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // Win32 ABI: raw pointer from the caller
pub extern "system" fn RegisterClassA(wnd_class: *const u8) -> ATOM {
    if wnd_class.is_null() {
        return 0;
    }
    let proc =
        unsafe { core::ptr::read_unaligned(wnd_class.add(WNDCLASS_LPFNWNDPROC) as *const usize) };
    set_wndproc(proc);
    1
}

/// `RegisterClassExA`: `lpfnWndProc` is at the same offset 8 (after cbSize+style).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn RegisterClassExA(wnd_class_ex: *const u8) -> ATOM {
    RegisterClassA(wnd_class_ex)
}

/// `CreateWindowExA`: creates the one window (client area = whole screen). Returns a
/// non-null opaque HWND.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn CreateWindowExA(
    _ex_style: u32,
    _class_name: *const u8,
    _window_name: *const u8,
    _style: u32,
    _x: i32,
    _y: i32,
    _w: i32,
    _h: i32,
    _parent: HWND,
    _menu: HMENU,
    _instance: HINSTANCE,
    _param: *const u8,
) -> HWND {
    THE_HWND
}

/// `CreateWindowA`: `CreateWindowExA` without `dwExStyle`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn CreateWindowA(
    class_name: *const u8,
    window_name: *const u8,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    parent: HWND,
    menu: HMENU,
    instance: HINSTANCE,
    param: *const u8,
) -> HWND {
    CreateWindowExA(
        0,
        class_name,
        window_name,
        style,
        x,
        y,
        w,
        h,
        parent,
        menu,
        instance,
        param,
    )
}

/// `ShowWindow`: enqueues the first `WM_PAINT`. Returns the previous visibility (0).
#[unsafe(no_mangle)]
pub extern "system" fn ShowWindow(_hwnd: HWND, _cmd_show: i32) -> BOOL {
    post(WM_PAINT, 0, 0);
    0
}

/// `UpdateWindow`: enqueues a `WM_PAINT` if the window needs repainting.
#[unsafe(no_mangle)]
pub extern "system" fn UpdateWindow(_hwnd: HWND) -> BOOL {
    post(WM_PAINT, 0, 0);
    1
}

// --- message loop ------------------------------------------------------------------

/// `GetMessageA`: BLOCKING message pump (see the module header for the priority
/// order). Returns 0 on `WM_QUIT` (ends the loop), nonzero otherwise; -1 (error) only
/// for a null `msg`.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn GetMessageA(
    msg: *mut MSG,
    _hwnd: HWND,
    _filter_min: UINT,
    _filter_max: UINT,
) -> BOOL {
    if msg.is_null() {
        return -1;
    }
    let m = next_message();
    unsafe {
        core::ptr::write(
            msg,
            MSG {
                hwnd: THE_HWND,
                message: m.message,
                wParam: m.wparam,
                lParam: m.lparam,
                time: 0,
                pt: POINT { x: 0, y: 0 },
                lPrivate: 0,
            },
        );
    }
    i32::from(m.message != WM_QUIT)
}

/// Picks the next message by priority; blocks in the kernel when nothing is ready.
fn next_message() -> Queued {
    let s = state();
    if s.quit {
        return Queued {
            message: WM_QUIT,
            wparam: s.quit_code,
            lparam: 0,
        };
    }
    if s.pending_char != 0 {
        let c = s.pending_char;
        s.pending_char = 0;
        return Queued {
            message: WM_CHAR,
            wparam: c as usize,
            lparam: 0,
        };
    }
    if let Some(p) = take() {
        return p;
    }
    loop {
        let mut ev = [0u32; 5];
        if unsafe { NtUserGetInputEvent(ev.as_mut_ptr()) } < 0 {
            // Cannot wait for input: end the loop rather than spin.
            return Queued {
                message: WM_QUIT,
                wparam: 0,
                lparam: 0,
            };
        }
        if ev[0] == 1 {
            let message = match ev[4] {
                0 => WM_MOUSEMOVE,
                1 => WM_LBUTTONDOWN,
                2 => WM_LBUTTONUP,
                3 => WM_RBUTTONDOWN,
                4 => WM_RBUTTONUP,
                5 => WM_MBUTTONDOWN,
                6 => WM_MBUTTONUP,
                _ => continue,
            };
            let b = ev[3];
            let mut mk = 0;
            if b & 1 != 0 {
                mk |= MK_LBUTTON;
            }
            if b & 2 != 0 {
                mk |= MK_RBUTTON;
            }
            if b & 4 != 0 {
                mk |= MK_MBUTTON;
            }
            return Queued {
                message,
                wparam: mk,
                lparam: ((((ev[2] & 0xFFFF) << 16) | (ev[1] & 0xFFFF)) as i32) as isize,
            };
        }
        if ev[0] != 0 || ev[3] & 1 == 0 {
            continue; // key-up (documented choice) or unknown tag: skipped
        }
        let s = state();
        s.last_vk = ev[1];
        s.last_ch = ev[2];
        return Queued {
            message: WM_KEYDOWN,
            wparam: ev[1] as usize,
            lparam: 0,
        };
    }
}

/// `TranslateMessage`: a `WM_KEYDOWN` whose key produced a character enqueues a
/// `WM_CHAR` for the next `GetMessageA`. Returns TRUE if it did, else FALSE.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn TranslateMessage(msg: *const MSG) -> BOOL {
    if msg.is_null() {
        return 0;
    }
    let m = unsafe { &*msg };
    let s = state();
    if m.message == WM_KEYDOWN && m.wParam == s.last_vk as usize && s.last_ch != 0 {
        s.pending_char = s.last_ch;
        s.last_ch = 0;
        return 1;
    }
    0
}

/// `DispatchMessageA`: calls the stored WndProc with the message's fields.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn DispatchMessageA(msg: *const MSG) -> LRESULT {
    if msg.is_null() {
        return 0;
    }
    let m = unsafe { &*msg };
    let proc = get_wndproc();
    if proc == 0 {
        return 0;
    }
    let wndproc: WndProc = unsafe { core::mem::transmute::<usize, WndProc>(proc) };
    wndproc(m.hwnd, m.message, m.wParam, m.lParam)
}

/// `DefWindowProcA`: default handling (`WM_DESTROY` -> `PostQuitMessage(0)`; the rest 0).
#[unsafe(no_mangle)]
pub extern "system" fn DefWindowProcA(
    _hwnd: HWND,
    msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DESTROY {
        PostQuitMessage(0);
    }
    0
}

/// `PostQuitMessage`: makes the next `GetMessageA` report `WM_QUIT` (which returns 0).
#[unsafe(no_mangle)]
pub extern "system" fn PostQuitMessage(exit_code: i32) {
    let s = state();
    s.quit = true;
    s.quit_code = exit_code as usize;
}

// --- painting ----------------------------------------------------------------------

/// `BeginPaint`: returns the opaque screen HDC and fills the PAINTSTRUCT (rcPaint =
/// whole client area via `NtGdiGetFbInfo`).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn BeginPaint(_hwnd: HWND, paint: *mut PAINTSTRUCT) -> HDC {
    let (w, h) = screen_size();
    if !paint.is_null() {
        unsafe {
            core::ptr::write(
                paint,
                PAINTSTRUCT {
                    hdc: SCREEN_HDC,
                    fErase: 1,
                    rcPaint: RECT {
                        left: 0,
                        top: 0,
                        right: w as i32,
                        bottom: h as i32,
                    },
                    fRestore: 0,
                    fIncUpdate: 0,
                    rgbReserved: [0; 32],
                },
            );
        }
    }
    SCREEN_HDC
}

/// `GetDC`: the opaque screen DC (single window, client area = screen).
#[unsafe(no_mangle)]
pub extern "system" fn GetDC(_hwnd: HWND) -> HDC {
    SCREEN_HDC
}

/// `ReleaseDC`: nothing to release.
#[unsafe(no_mangle)]
pub extern "system" fn ReleaseDC(_hwnd: HWND, _hdc: HDC) -> i32 {
    1
}

/// `EndPaint`: nothing to release (the screen DC is implicit).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn EndPaint(_hwnd: HWND, _paint: *const PAINTSTRUCT) -> BOOL {
    1
}

/// `FillRect`: fills `*lprc` with the brush's colour via `NtGdiFillRect`. Returns
/// nonzero on success. A brush this process did not create via gdi32 paints black.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn FillRect(_hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32 {
    if lprc.is_null() {
        return 0;
    }
    let r = unsafe { &*lprc };
    let left = r.left.max(0);
    let top = r.top.max(0);
    if r.right <= left || r.bottom <= top {
        return 1; // empty rectangle: nothing to paint (still success)
    }
    let w = (r.right - left) as u32;
    let h = (r.bottom - top) as u32;
    let rgb = colorref_to_rgb(brush_colorref(hbr).unwrap_or(0));
    unsafe { NtGdiFillRect(left as u32, top as u32, w, h, rgb) };
    1
}

// --- resources (dummies) -----------------------------------------------------------

/// `LoadCursorA`: a non-null dummy cursor handle.
#[unsafe(no_mangle)]
pub extern "system" fn LoadCursorA(_instance: HINSTANCE, _name: *const u8) -> HCURSOR {
    DUMMY_CURSOR
}

/// `LoadIconA`: a non-null dummy icon handle.
#[unsafe(no_mangle)]
pub extern "system" fn LoadIconA(_instance: HINSTANCE, _name: *const u8) -> HICON {
    DUMMY_ICON
}
