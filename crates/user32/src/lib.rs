//! TukOS user32.dll: a small multi-window shim with a message loop. Painting goes to
//! the kernel through the `NtGdi*` syscalls (the framebuffer is never mapped into
//! user space); each window draws into its own kernel backbuffer (HDC carries the window id).
//!
//! Signatures and struct layouts follow the public winuser.h/windef.h documentation.
//! HWND/HDC/HBRUSH are opaque handles. The only import is `ntdll.dll` (`raw-dylib`):
//! user32 shares the HBRUSH encoding with gdi32 by value (it decodes the COLORREF a
//! gdi32 brush carries), so it never needs to call into gdi32, and the loader binds
//! the GUI exe's gdi32/kernel32 imports to those DLLs directly.
//!
//! SINGLE-THREAD MODEL: a TukOS process has exactly one user thread, so all window
//! state lives in process-global statics with no locking (the existing kernel32/ntdll
//! make the same assumption). A fixed table of classes (name -> WndProc) and of windows
//! (hwnd = kernel window id -> WndProc), one message queue. HDC = `DC_TAG | window id`.
//!
//! CreateWindowExA COMPAT RULE: if `X`, `Y`, `nWidth` or `nHeight` is `CW_USEDEFAULT`
//! (what every pre-Plano-25 GUI program passes) the window is created with the kernel's
//! `STYLE_COMPAT` flag = full-screen at the origin, no frame (identical pixels to the old
//! single-window model). Any explicit position/size is honoured by the kernel (a size
//! >= the screen is full-screen there too).
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
    // NtGdiGetFbInfo(window_id, out): writes [width:u32, height:u32, bpp:u32] for the
    // window's client area (window_id 0 = screen size) (kernel syscall 14).
    fn NtGdiGetFbInfo(window_id: u32, out: *mut u32) -> i32;
    // NtGdiFillRect(window_id, x, y, w, h, color): paints a 0x00RRGGBB rectangle into the
    // window's backbuffer (window_id 0 = implicit full-screen compat window) (syscall 15).
    fn NtGdiFillRect(window_id: u32, x: u32, y: u32, w: u32, h: u32, color: u32) -> i32;
    // NtUserGetInputEvent(out): BLOCKS in the kernel until a ROUTED input event for one of
    // this process's windows, then writes 6 x u32 (syscall 18):
    //   key   = [0, vk, ch, down, 0, hwnd];
    //   mouse = [1, x, y (CLIENT coords), buttons (bit0 L, bit1 R, bit2 M), kind, hwnd]
    // with kind 0 Move, 1/2 left down/up, 3/4 right down/up, 5/6 middle down/up. word 5 is
    // the target window id (mouse -> window under cursor; key -> focused window).
    fn NtUserGetInputEvent(out: *mut u32) -> i32;
    // NtUserCreateWindow(x, y, w, h, style, out_hwnd) (syscall 19): style bit0 = compat
    // full-screen; writes the window id (u64) to out_hwnd.
    fn NtUserCreateWindow(x: i32, y: i32, w: u32, h: u32, style: u32, out: *mut u64) -> i32;
    // NtUserDestroyWindow(hwnd) (syscall 20).
    fn NtUserDestroyWindow(hwnd: u32) -> i32;
    // NtUserFlushWindow(hwnd) (syscall 21): composites the windows onto the screen.
    fn NtUserFlushWindow(hwnd: u32) -> i32;
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

// CreateWindow constants (winuser.h) and the kernel's compat style flag.
const CW_USEDEFAULT: i32 = i32::MIN; // 0x80000000
const STYLE_COMPAT: u32 = 1;

// Opaque handle encodings. hwnd == the kernel window id. HDC = DC_TAG | window id
// (id 0 = the implicit full-screen window); kept in sync with crates/gdi32/src/lib.rs.
const DC_TAG: isize = 0x4400_0000;
fn dc_for(hwnd: HWND) -> HDC {
    DC_TAG | (hwnd & 0x00FF_FFFF)
}
fn window_id_of_dc(hdc: HDC) -> u32 {
    if hdc & !0x00FF_FFFF == DC_TAG {
        (hdc & 0x00FF_FFFF) as u32
    } else {
        0
    }
}
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
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
}

const NO_MSG: Queued = Queued {
    hwnd: 0,
    message: 0,
    wparam: 0,
    lparam: 0,
};

const QCAP: usize = 16; // power of two: see the mask in `post`/`take`
const QMASK: usize = QCAP - 1;

const MAX_CLASSES: usize = 16;
const MAX_WINDOWS: usize = 32;
const CLASS_NAME_MAX: usize = 32;

#[derive(Clone, Copy)]
struct Class {
    used: bool,
    name: [u8; CLASS_NAME_MAX],
    wndproc: usize,
}

#[derive(Clone, Copy)]
struct Win {
    hwnd: HWND, // 0 = free slot
    wndproc: usize,
}

struct State {
    classes: [Class; MAX_CLASSES],
    /// WndProc of the most recently registered class (fallback when a class name is not found).
    last_wndproc: usize,
    windows: [Win; MAX_WINDOWS],
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
    /// Target window of the last `WM_KEYDOWN` (so its `WM_CHAR` goes to the same window).
    last_key_hwnd: HWND,
}

static mut STATE: State = State {
    classes: [Class {
        used: false,
        name: [0; CLASS_NAME_MAX],
        wndproc: 0,
    }; MAX_CLASSES],
    last_wndproc: 0,
    windows: [Win {
        hwnd: 0,
        wndproc: 0,
    }; MAX_WINDOWS],
    queue: [NO_MSG; QCAP],
    head: 0,
    tail: 0,
    quit: false,
    quit_code: 0,
    pending_char: 0,
    last_vk: 0,
    last_ch: 0,
    last_key_hwnd: 0,
};

fn state() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

/// Enqueues a message (drops it silently if the tiny queue is full; the deterministic
/// single-window flow never enqueues more than a couple at a time).
fn post(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) {
    unsafe {
        let s = &mut *core::ptr::addr_of_mut!(STATE);
        // Masking with QMASK (QCAP is a power of two) keeps every index provably < QCAP,
        // so no `panic_bounds_check` is emitted (that would drag in the MSVC CRT).
        let next = (s.tail + 1) & QMASK;
        if next == s.head {
            return;
        }
        s.queue[s.tail & QMASK] = Queued {
            hwnd,
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

/// `(width, height)` of the window's client area, 0,0 if unavailable.
fn client_size(window_id: u32) -> (u32, u32) {
    let mut info = [0u32; 3];
    if unsafe { NtGdiGetFbInfo(window_id, info.as_mut_ptr()) } >= 0 {
        (info[0], info[1])
    } else {
        (0, 0)
    }
}

/// Copies a NUL-terminated ANSI class name (lowercased) into a fixed buffer. A pointer
/// below 0x10000 is an ATOM (MAKEINTATOM), not a string: it yields an empty name.
fn read_class_name(p: *const u8) -> [u8; CLASS_NAME_MAX] {
    let mut out = [0u8; CLASS_NAME_MAX];
    if (p as usize) < 0x1_0000 {
        return out;
    }
    let mut i = 0;
    while i < CLASS_NAME_MAX - 1 {
        let c = unsafe { *p.add(i) };
        if c == 0 {
            break;
        }
        out[i] = c.to_ascii_lowercase();
        i += 1;
    }
    out
}

/// WndProc registered for the class `name`, falling back to the last registered class.
fn find_class_proc(name: &[u8; CLASS_NAME_MAX]) -> usize {
    let s = state();
    for c in s.classes.iter() {
        if c.used && c.name == *name {
            return c.wndproc;
        }
    }
    s.last_wndproc
}

/// WndProc of window `hwnd` (0 if unknown).
fn wndproc_of(hwnd: HWND) -> usize {
    let s = state();
    for w in s.windows.iter() {
        if w.hwnd != 0 && w.hwnd == hwnd {
            return w.wndproc;
        }
    }
    0
}

/// Fallback input target (the most recently created live window), used only if the kernel
/// delivers an event with no target hwnd. Normally the kernel routes each event to a
/// specific window (mouse -> window under the cursor; key -> focused window) and reports
/// that hwnd, so `next_message` uses the kernel's target directly.
fn input_target() -> HWND {
    let s = state();
    let mut best = 0;
    for w in s.windows.iter() {
        if w.hwnd != 0 {
            best = w.hwnd;
        }
    }
    best
}

// --- window class / creation -------------------------------------------------------

const WNDCLASS_LPSZCLASSNAME: usize = 64;

/// `RegisterClassA`: stores the class WndProc under its name. Returns a nonzero ATOM.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // Win32 ABI: raw pointer from the caller
pub extern "system" fn RegisterClassA(wnd_class: *const u8) -> ATOM {
    if wnd_class.is_null() {
        return 0;
    }
    let proc =
        unsafe { core::ptr::read_unaligned(wnd_class.add(WNDCLASS_LPFNWNDPROC) as *const usize) };
    let name_ptr = unsafe {
        core::ptr::read_unaligned(wnd_class.add(WNDCLASS_LPSZCLASSNAME) as *const *const u8)
    };
    let name = read_class_name(name_ptr);
    let s = state();
    s.last_wndproc = proc;
    // Re-registering a name replaces it; otherwise take the first free slot.
    let mut slot = None;
    for (i, c) in s.classes.iter().enumerate() {
        if c.used && c.name == name {
            slot = Some(i);
            break;
        }
        if !c.used && slot.is_none() {
            slot = Some(i);
        }
    }
    let Some(i) = slot else {
        return 0;
    };
    s.classes[i] = Class {
        used: true,
        name,
        wndproc: proc,
    };
    (i + 1) as ATOM
}

/// `RegisterClassExA`: `lpfnWndProc` is at the same offset 8 (after cbSize+style).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn RegisterClassExA(wnd_class_ex: *const u8) -> ATOM {
    RegisterClassA(wnd_class_ex)
}

/// `CreateWindowExA`: creates a kernel window (see the module header for the compat
/// rule) and records `{hwnd, class WndProc}`. Returns the HWND, or NULL on failure.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn CreateWindowExA(
    _ex_style: u32,
    class_name: *const u8,
    _window_name: *const u8,
    _style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    _parent: HWND,
    _menu: HMENU,
    _instance: HINSTANCE,
    _param: *const u8,
) -> HWND {
    let compat =
        x == CW_USEDEFAULT || y == CW_USEDEFAULT || w == CW_USEDEFAULT || h == CW_USEDEFAULT;
    let style = if compat { STYLE_COMPAT } else { 0 };
    let mut id: u64 = 0;
    let st = unsafe { NtUserCreateWindow(x, y, w.max(1) as u32, h.max(1) as u32, style, &mut id) };
    if st < 0 || id == 0 {
        return 0;
    }
    let proc = find_class_proc(&read_class_name(class_name));
    let s = state();
    for slot in s.windows.iter_mut() {
        if slot.hwnd == 0 {
            *slot = Win {
                hwnd: id as HWND,
                wndproc: proc,
            };
            return id as HWND;
        }
    }
    // Table full (cannot happen while it matches the kernel cap): undo.
    unsafe { NtUserDestroyWindow(id as u32) };
    0
}

/// `DestroyWindow`: destroys the kernel window and forgets it. Returns TRUE if it existed.
#[unsafe(no_mangle)]
pub extern "system" fn DestroyWindow(hwnd: HWND) -> BOOL {
    let s = state();
    for slot in s.windows.iter_mut() {
        if hwnd != 0 && slot.hwnd == hwnd {
            slot.hwnd = 0;
            unsafe { NtUserDestroyWindow(hwnd as u32) };
            return 1;
        }
    }
    0
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
pub extern "system" fn ShowWindow(hwnd: HWND, _cmd_show: i32) -> BOOL {
    post(hwnd, WM_PAINT, 0, 0);
    0
}

/// `UpdateWindow`: enqueues a `WM_PAINT` if the window needs repainting.
#[unsafe(no_mangle)]
pub extern "system" fn UpdateWindow(hwnd: HWND) -> BOOL {
    post(hwnd, WM_PAINT, 0, 0);
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
                hwnd: m.hwnd,
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
            hwnd: 0,
            message: WM_QUIT,
            wparam: s.quit_code,
            lparam: 0,
        };
    }
    if s.pending_char != 0 {
        let c = s.pending_char;
        s.pending_char = 0;
        let hwnd = if s.last_key_hwnd != 0 {
            s.last_key_hwnd
        } else {
            input_target()
        };
        return Queued {
            hwnd,
            message: WM_CHAR,
            wparam: c as usize,
            lparam: 0,
        };
    }
    if let Some(p) = take() {
        return p;
    }
    loop {
        let mut ev = [0u32; 6];
        if unsafe { NtUserGetInputEvent(ev.as_mut_ptr()) } < 0 {
            // Cannot wait for input: end the loop rather than spin.
            return Queued {
                hwnd: 0,
                message: WM_QUIT,
                wparam: 0,
                lparam: 0,
            };
        }
        // The kernel tags each event with its target window; fall back to the newest
        // window only if it somehow reported none.
        let target = if ev[5] != 0 {
            ev[5] as HWND
        } else {
            input_target()
        };
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
                hwnd: target,
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
        s.last_key_hwnd = target;
        return Queued {
            hwnd: target,
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
    let mut proc = wndproc_of(m.hwnd);
    if proc == 0 {
        proc = state().last_wndproc;
    }
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

/// `BeginPaint`: returns the window's HDC (carries the window id) and fills the
/// PAINTSTRUCT (rcPaint = the window's client area via `NtGdiGetFbInfo`).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn BeginPaint(hwnd: HWND, paint: *mut PAINTSTRUCT) -> HDC {
    let dc = dc_for(hwnd);
    let (w, h) = client_size(window_id_of_dc(dc));
    if !paint.is_null() {
        unsafe {
            core::ptr::write(
                paint,
                PAINTSTRUCT {
                    hdc: dc,
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
    dc
}

/// `GetDC`: the window's HDC (hwnd 0 = the implicit full-screen window).
#[unsafe(no_mangle)]
pub extern "system" fn GetDC(hwnd: HWND) -> HDC {
    dc_for(hwnd)
}

/// `ReleaseDC`: nothing to release.
#[unsafe(no_mangle)]
pub extern "system" fn ReleaseDC(_hwnd: HWND, _hdc: HDC) -> i32 {
    1
}

/// `EndPaint`: composites the window onto the screen (`NtUserFlushWindow`).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn EndPaint(hwnd: HWND, _paint: *const PAINTSTRUCT) -> BOOL {
    unsafe { NtUserFlushWindow((hwnd & 0x00FF_FFFF) as u32) };
    1
}

/// `FillRect`: fills `*lprc` with the brush's colour via `NtGdiFillRect`. Returns
/// nonzero on success. A brush this process did not create via gdi32 paints black.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn FillRect(hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32 {
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
    let id = window_id_of_dc(hdc);
    unsafe { NtGdiFillRect(id, left as u32, top as u32, w, h, rgb) };
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
