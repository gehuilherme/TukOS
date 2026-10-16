//! Window manager: per-window off-screen backbuffers + a compositor that paints the
//! desktop background, each visible window (by z-order) and a simple frame, with the
//! Plano 24 mouse cursor on top (see `fbcon::compose`).
//!
//! Each window owns a `Vec<u32>` backbuffer of `w*h` client pixels in KERNEL memory
//! (never mapped to user). GDI draws into it by id via syscalls (clamped, no OOB);
//! `composite()` blits every window onto the framebuffer. A full-screen-at-origin
//! window (compat) draws no frame and composites byte-identically to the old direct
//! framebuffer path, so the existing single-window GUI programs are unchanged.

extern crate alloc;

use alloc::vec::Vec;
use arch_x86_64::interrupts::IrqMutex;
use kbd::{InputEvent, MouseKind};
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};

const WEIGHT: FontWeight = FontWeight::Regular;
const HEIGHT: RasterHeight = RasterHeight::Size16;

/// At most this many live windows; `create_window` past it fails cleanly.
pub const MAX_WINDOWS: usize = 32;

/// Desktop background painted behind every window (clean-room colour).
const DESKTOP_BG: u32 = 0x0020_3040;
/// 1px frame drawn around non-full-screen windows (clean-room aesthetic).
const FRAME_RGB: u32 = 0x00C0_C0C0;

/// Style flag forcing compat full-screen (x=0,y=0, screen-sized, no frame).
pub const STYLE_COMPAT: u32 = 0x0000_0001;

/// One window: an off-screen client surface placed at `(x, y)` with z-order `z`.
pub struct Window {
    pub id: u32,
    pub owner_tid: usize,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub z: u32,
    pub focused: bool,
    pub visible: bool,
    /// Full-screen-at-origin compat: composites with no frame, byte-identical to
    /// the old direct-to-framebuffer path.
    pub fullscreen: bool,
    /// `w*h` client pixels (0x00RRGGBB), row-major `backbuffer[y*w + x]`.
    pub backbuffer: Vec<u32>,
}

struct WmState {
    windows: Vec<Window>,
    next_id: u32,
    next_z: u32,
}

static WM: IrqMutex<WmState> = IrqMutex::new(WmState {
    windows: Vec::new(),
    next_id: 1, // id 0 is reserved ("no window" / the implicit default)
    next_z: 1,
});

/// Clamps a requested window size to the screen (never larger than the framebuffer).
fn clamp_size(w: u32, h: u32, sw: u32, sh: u32) -> (u32, u32) {
    (w.min(sw).max(1), h.min(sh).max(1))
}

/// Policy for a non-full-screen window's top-left: a legitimately partially-off-screen
/// window is allowed (the blit/frame math clips it), but an absurd coordinate is
/// rejected so the compositor's frame arithmetic can never approach `i32` extremes.
/// The window origin must lie within `[-screen, 2*screen]` on each axis (computed in
/// `i64` so the bound check itself never overflows).
fn origin_in_range(x: i32, y: i32, sw: u32, sh: u32) -> bool {
    let in1 = |v: i32, s: u32| {
        let (v, s) = (i64::from(v), i64::from(s));
        v >= -s && v <= 2 * s
    };
    in1(x, sw) && in1(y, sh)
}

impl WmState {
    fn find(&self, id: u32) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }
}

/// Creates a window owned by `owner`. Returns its id, or `None` if the cap is hit or
/// the framebuffer is unavailable. Full-screen compat (size >= screen or `STYLE_COMPAT`)
/// is forced to x=0,y=0, screen-sized, no frame; its backbuffer starts black (matching
/// the cleared framebuffer). Normal windows are clamped to the screen and start black.
pub fn create_window(owner: usize, x: i32, y: i32, w: u32, h: u32, style: u32) -> Option<u32> {
    let (sw, sh) = crate::fbcon::fb_info().map(|(w, h, _)| (w, h))?;
    let mut st = WM.lock();
    if st.windows.len() >= MAX_WINDOWS {
        return None;
    }
    let compat = style & STYLE_COMPAT != 0 || (w >= sw && h >= sh);
    let (x, y, w, h, fullscreen) = if compat {
        (0, 0, sw, sh, true)
    } else {
        // Reject absurd origins (DoS guard): a window entirely implausibly off-screen is
        // refused; legitimately partially-off-screen windows (origin within [-screen,
        // 2*screen]) are kept and clipped by the compositor.
        if !origin_in_range(x, y, sw, sh) {
            return None;
        }
        let (cw, ch) = clamp_size(w, h, sw, sh);
        (x, y, cw, ch, false)
    };
    let len = (w as usize).checked_mul(h as usize)?;
    let id = st.next_id;
    let z = st.next_z;
    st.next_id = st.next_id.wrapping_add(1).max(1);
    st.next_z = st.next_z.wrapping_add(1);
    st.windows.push(Window {
        id,
        owner_tid: owner,
        x,
        y,
        w,
        h,
        z,
        focused: true,
        visible: true,
        fullscreen,
        backbuffer: alloc::vec![0u32; len],
    });
    Some(id)
}

/// Destroys the window `id` (frees its backbuffer). No-op if it does not exist.
pub fn destroy_window(id: u32) {
    let mut st = WM.lock();
    if let Some(i) = st.find(id) {
        st.windows.remove(i);
    }
}

/// Destroys every window owned by thread `owner` (called when the thread exits so its
/// backbuffers are freed). No framebuffer touch.
pub fn destroy_windows_of(owner: usize) {
    let mut st = WM.lock();
    st.windows.retain(|w| w.owner_tid != owner);
}

/// The owner's implicit window (compat full-screen), created on first use. Used by the
/// GDI syscalls when the caller passes window id 0 (the single-window compat path).
pub fn implicit_window(owner: usize) -> Option<u32> {
    {
        let st = WM.lock();
        if let Some(w) = st.windows.iter().find(|w| w.owner_tid == owner) {
            return Some(w.id);
        }
    }
    create_window(owner, 0, 0, u32::MAX, u32::MAX, STYLE_COMPAT)
}

/// Resolves a GDI window id for `owner`: a real id if it exists, else (id 0 or an
/// unknown id) the owner's implicit full-screen window, created on demand.
fn resolve_for(owner: usize, id: u32) -> Option<u32> {
    if id != 0 && WM.lock().find(id).is_some() {
        return Some(id);
    }
    implicit_window(owner)
}

/// `(client_w, client_h)` of window `id`, or `None` if it does not exist.
pub fn window_client_size(id: u32) -> Option<(u32, u32)> {
    let st = WM.lock();
    st.find(id).map(|i| (st.windows[i].w, st.windows[i].h))
}

/// Fills a rect (client coords) in window `id`'s backbuffer with `color`, clamped to
/// the backbuffer. `id` 0 (or unknown) targets `owner`'s implicit full-screen window.
pub fn fill_rect_in(owner: usize, id: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    let Some(id) = resolve_for(owner, id) else {
        return;
    };
    let mut st = WM.lock();
    let Some(i) = st.find(id) else {
        return;
    };
    let win = &mut st.windows[i];
    let (bw, bh) = (win.w, win.h);
    let Some((x0, y0, x1, y1)) = gfx::clamp_rect(x, y, w, h, bw, bh) else {
        return;
    };
    let color = color & 0x00FF_FFFF;
    let bw = bw as usize;
    for yy in y0..y1 {
        for xx in x0..x1 {
            win.backbuffer[yy * bw + xx] = color;
        }
    }
}

/// Draws ASCII `bytes` at client `(x, y)` into window `id`'s backbuffer, blending `bg`
/// to `fg` by glyph intensity (the kernel font), clamped. `id` 0 = implicit window.
pub fn text_out_in(owner: usize, id: u32, x: u32, y: u32, bytes: &[u8], fg: u32, bg: u32) {
    let Some(id) = resolve_for(owner, id) else {
        return;
    };
    let mut st = WM.lock();
    let Some(i) = st.find(id) else {
        return;
    };
    let win = &mut st.windows[i];
    let (bw, bh) = (win.w as usize, win.h as usize);
    let (cw, ch) = (get_raster_width(WEIGHT, HEIGHT), HEIGHT.val());
    for (ci, &b) in bytes.iter().enumerate() {
        let c = if (0x20..0x7F).contains(&b) {
            b as char
        } else {
            '?'
        };
        let glyph = get_raster(c, WEIGHT, HEIGHT)
            .or_else(|| get_raster('?', WEIGHT, HEIGHT))
            .expect("'?' existe na fonte");
        let x0 = (x as usize).saturating_add(ci.saturating_mul(cw));
        if x0 >= bw {
            break;
        }
        for (gy, line) in glyph.raster().iter().enumerate().take(ch) {
            let py = (y as usize).saturating_add(gy);
            if py >= bh {
                break;
            }
            for (gx, &v) in line.iter().enumerate() {
                let px = x0.saturating_add(gx);
                if px >= bw {
                    break;
                }
                win.backbuffer[py * bw + px] = crate::fbcon::blend(fg, bg, v);
            }
        }
    }
}

/// Topmost visible window containing screen point `(x, y)`, or `None`. (Input routing,
/// Task 3; included here as part of the wm interface.)
#[allow(dead_code)]
pub fn hit_test(x: i32, y: i32) -> Option<u32> {
    let st = WM.lock();
    st.windows
        .iter()
        .filter(|w| {
            w.visible && x >= w.x && y >= w.y && x < w.x + w.w as i32 && y < w.y + w.h as i32
        })
        .max_by_key(|w| w.z)
        .map(|w| w.id)
}

/// Raises window `id` to the top of the z-order and gives it focus. (Task 3.)
#[allow(dead_code)]
pub fn raise_and_focus(id: u32) {
    let mut st = WM.lock();
    if let Some(i) = st.find(id) {
        let z = st.next_z;
        st.next_z = st.next_z.wrapping_add(1);
        st.windows[i].z = z;
        for (j, w) in st.windows.iter_mut().enumerate() {
            w.focused = j == i;
        }
    }
}

/// A routed input event: the window it targets, that window's owner thread, and the
/// event itself with any mouse coordinates already converted to the window's CLIENT
/// space (screen - window origin). Produced by `route_input`.
#[derive(Clone, Copy)]
pub struct RoutedInput {
    pub owner: usize,
    pub hwnd: u32,
    pub ev: InputEvent,
}

/// Routes one raw input event to its target window (Task 3). A mouse event targets the
/// topmost visible window under the cursor (`hit_test`); a mouse BUTTON-DOWN additionally
/// raises+focuses that window (and the returned bool asks the caller to recomposite). A
/// key event targets the currently focused window. Mouse coordinates are converted to the
/// target window's client space. Returns `None` when there is no target (no window under
/// the cursor / no focused window), in which case the event is dropped.
///
/// Lock discipline: takes and releases the WM lock for the lookup, then (button-down only)
/// calls `raise_and_focus` which takes its own WM lock. No FBCON lock is taken here and no
/// lock is held across the call, so the WM->FBCON order (the caller composites afterwards)
/// is preserved.
pub fn route_input(ev: InputEvent) -> Option<(RoutedInput, bool)> {
    match ev {
        InputEvent::Key { .. } => {
            // Keyboard goes to the focused window (if any).
            let (owner, hwnd) = {
                let st = WM.lock();
                let w = st.windows.iter().find(|w| w.focused && w.visible)?;
                (w.owner_tid, w.id)
            };
            Some((RoutedInput { owner, hwnd, ev }, false))
        }
        InputEvent::Mouse {
            x,
            y,
            buttons,
            kind,
        } => {
            // Mouse goes to the topmost visible window under the cursor (same z as the
            // compositor). Resolve it and its origin while holding the lock, then release.
            let (id, owner, wx, wy) = {
                let st = WM.lock();
                let w = st
                    .windows
                    .iter()
                    .filter(|w| {
                        w.visible
                            && x >= w.x
                            && y >= w.y
                            && x < w.x + w.w as i32
                            && y < w.y + w.h as i32
                    })
                    .max_by_key(|w| w.z)?;
                (w.id, w.owner_tid, w.x, w.y)
            };
            // A button-down raises+focuses the hit window (lock taken inside) and asks the
            // caller to recomposite so the new z-order reaches the screen.
            let recompose = matches!(kind, MouseKind::Down(_));
            if recompose {
                raise_and_focus(id);
            }
            // Convert to client coordinates (screen - window origin).
            let ev = InputEvent::Mouse {
                x: x - wx,
                y: y - wy,
                buttons,
                kind,
            };
            Some((
                RoutedInput {
                    owner,
                    hwnd: id,
                    ev,
                },
                recompose,
            ))
        }
    }
}

/// Per-owner mailbox of routed input events. The unified raw queue (`kbd`) is a single
/// FIFO; `NtUserGetInputEvent` drains it, routes each event here, and then each caller
/// pops only the events destined for ITS windows, so one process never consumes another
/// process's input. Bounded; drops the oldest when full (like the raw queue).
static ROUTED: IrqMutex<Vec<RoutedInput>> = IrqMutex::new(Vec::new());
const ROUTED_CAP: usize = 128;

/// Enqueues a routed event into the per-owner mailbox (drops the oldest when full).
pub fn deliver(r: RoutedInput) {
    let mut q = ROUTED.lock();
    if q.len() >= ROUTED_CAP {
        q.remove(0);
    }
    q.push(r);
}

/// Pops the oldest routed event destined for `owner`, leaving other owners' events queued.
pub fn take_input_for(owner: usize) -> Option<RoutedInput> {
    let mut q = ROUTED.lock();
    let i = q.iter().position(|r| r.owner == owner)?;
    Some(q.remove(i))
}

/// Composites the whole screen: desktop background, every visible window by ascending
/// z (backbuffer blit + a 1px frame for non-full-screen windows), cursor on top. Under
/// the FBCON lock via `fbcon::compose`. Lock order is WM then FBCON (never the reverse).
pub fn composite() {
    let st = WM.lock();
    let n = st.windows.len();
    let mut order = [0usize; MAX_WINDOWS];
    for (slot, o) in order.iter_mut().enumerate().take(n) {
        *o = slot;
    }
    order[..n].sort_unstable_by_key(|&i| st.windows[i].z);
    crate::fbcon::compose(|frame| {
        frame.fill_bg(DESKTOP_BG);
        for &i in &order[..n] {
            let w = &st.windows[i];
            if !w.visible {
                continue;
            }
            frame.blit(w.x, w.y, w.w, w.h, &w.backbuffer);
            if !w.fullscreen {
                // 1px frame just outside the client rect. Saturating arithmetic so the
                // math can NEVER overflow regardless of the window's x,y (user-supplied).
                let left = w.x.saturating_sub(1);
                let top = w.y.saturating_sub(1);
                let right = w.x.saturating_add(w.w as i32);
                let bottom = w.y.saturating_add(w.h as i32);
                let span = w.w.saturating_add(2);
                frame.fill_rect(left, top, span, 1, FRAME_RGB); // top
                frame.fill_rect(left, bottom, span, 1, FRAME_RGB); // bottom
                frame.fill_rect(left, w.y, 1, w.h, FRAME_RGB); // left
                frame.fill_rect(right, w.y, 1, w.h, FRAME_RGB); // right
            }
        }
    });
}
