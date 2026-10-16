//! TukOS gdi32.dll: a thin GDI shim. Drawing is done by the kernel through the
//! `NtGdi*` syscalls (the framebuffer is never mapped into user space).
//!
//! Signatures follow the public wingdi.h documentation. HDC/HBRUSH are opaque
//! handles. The only import is `ntdll.dll` (`raw-dylib`).
#![no_std]
#![allow(non_snake_case)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU32, Ordering};

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
    // NtGdiTextOut(window_id, x, y, text, len, fg, bg): draws into the window's
    // backbuffer (window_id 0 = implicit full-screen compat window) (syscall 16).
    fn NtGdiTextOut(
        window_id: u32,
        x: u32,
        y: u32,
        text: *const u8,
        len: u32,
        fg: u32,
        bg: u32,
    ) -> i32;
}

pub type HDC = isize;
pub type HGDIOBJ = isize;
pub type HBRUSH = isize;
pub type COLORREF = u32;
pub type BOOL = i32;

pub const TRANSPARENT: i32 = 1;
pub const OPAQUE: i32 = 2;
const CLR_INVALID: u32 = 0xFFFF_FFFF;

// The shim has a single implicit DC state (one screen DC, single-threaded
// programs); every HDC shares it.
static TEXT_COLOR: AtomicU32 = AtomicU32::new(0x0000_0000); // black
static BK_COLOR: AtomicU32 = AtomicU32::new(0x00FF_FFFF); // white
static BK_MODE: AtomicU32 = AtomicU32::new(OPAQUE as u32);

// Brush handles: tag in the top byte, COLORREF in the low 24 bits.
// Stock objects use small ids below 0x100.
const BRUSH_TAG: isize = 0x4200_0000;

const WHITE_BRUSH: i32 = 0;
const LTGRAY_BRUSH: i32 = 1;
const GRAY_BRUSH: i32 = 2;
const DKGRAY_BRUSH: i32 = 3;
const BLACK_BRUSH: i32 = 4;
const NULL_BRUSH: i32 = 5;

/// COLORREF is 0x00BBGGRR; the framebuffer wants 0x00RRGGBB.
fn colorref_to_rgb(c: u32) -> u32 {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// Decodes a brush handle created here into its COLORREF.
pub fn brush_colorref(h: HBRUSH) -> Option<COLORREF> {
    if h & !0x00FF_FFFF == BRUSH_TAG {
        Some((h & 0x00FF_FFFF) as u32)
    } else {
        None
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn CreateSolidBrush(color: COLORREF) -> HBRUSH {
    BRUSH_TAG | (color & 0x00FF_FFFF) as isize
}

#[unsafe(no_mangle)]
pub extern "system" fn GetStockObject(i: i32) -> HGDIOBJ {
    let c = match i {
        WHITE_BRUSH => 0x00FF_FFFF,
        LTGRAY_BRUSH => 0x00C0_C0C0,
        GRAY_BRUSH => 0x0080_8080,
        DKGRAY_BRUSH => 0x0040_4040,
        BLACK_BRUSH => 0x0000_0000,
        NULL_BRUSH => return 0,
        _ => return 0,
    };
    CreateSolidBrush(c)
}

/// Brushes are value handles with no backing storage: nothing to free.
#[unsafe(no_mangle)]
pub extern "system" fn DeleteObject(_obj: HGDIOBJ) -> BOOL {
    1
}

/// Returns the previous mode (0 on an invalid mode, per the docs).
#[unsafe(no_mangle)]
pub extern "system" fn SetBkMode(_hdc: HDC, mode: i32) -> i32 {
    if mode != TRANSPARENT && mode != OPAQUE {
        return 0;
    }
    BK_MODE.swap(mode as u32, Ordering::Relaxed) as i32
}

/// Returns the previous text color.
#[unsafe(no_mangle)]
pub extern "system" fn SetTextColor(_hdc: HDC, color: COLORREF) -> COLORREF {
    TEXT_COLOR.swap(color & 0x00FF_FFFF, Ordering::Relaxed)
}

/// Returns the previous background color.
#[unsafe(no_mangle)]
pub extern "system" fn SetBkColor(_hdc: HDC, color: COLORREF) -> COLORREF {
    if color == CLR_INVALID {
        return CLR_INVALID;
    }
    BK_COLOR.swap(color & 0x00FF_FFFF, Ordering::Relaxed)
}

#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // Win32 ABI: raw pointer, validated by the kernel
pub extern "system" fn TextOutA(_hdc: HDC, x: i32, y: i32, text: *const u8, count: i32) -> BOOL {
    if text.is_null() || count <= 0 || x < 0 || y < 0 {
        return (count == 0 && !text.is_null()) as BOOL;
    }
    let fg = colorref_to_rgb(TEXT_COLOR.load(Ordering::Relaxed));
    let bg = if BK_MODE.load(Ordering::Relaxed) == TRANSPARENT as u32 {
        // TODO: no read-back syscall yet; transparent text paints over black.
        0
    } else {
        colorref_to_rgb(BK_COLOR.load(Ordering::Relaxed))
    };
    // Window id 0: the process's implicit full-screen compat window (Task 1).
    let st = unsafe { NtGdiTextOut(0, x as u32, y as u32, text, count as u32, fg, bg) };
    (st >= 0) as BOOL
}
