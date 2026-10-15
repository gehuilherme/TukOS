//! Text console drawn on the Limine framebuffer (32 bpp only).

use arch_x86_64::interrupts::IrqMutex;
use core::fmt;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};

const WEIGHT: FontWeight = FontWeight::Regular;
const HEIGHT: RasterHeight = RasterHeight::Size16;

pub struct FbCon {
    base: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    col: usize,
    row: usize,
}

// The framebuffer is only touched through the mutex below.
unsafe impl Send for FbCon {}

pub static FBCON: IrqMutex<Option<FbCon>> = IrqMutex::new(None);

impl FbCon {
    pub fn cell_width() -> usize {
        get_raster_width(WEIGHT, HEIGHT)
    }

    pub fn cell_height() -> usize {
        HEIGHT.val()
    }

    fn cols(&self) -> usize {
        self.width / Self::cell_width()
    }

    fn rows(&self) -> usize {
        self.height / Self::cell_height()
    }

    #[cfg_attr(not(feature = "ktest"), allow(dead_code))]
    pub fn pixel(&self, x: usize, y: usize) -> u32 {
        unsafe { (self.base.add(y * self.pitch + x * 4) as *const u32).read_volatile() }
    }

    fn put_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let rgb = intensity as u32 * 0x0001_0101;
        unsafe { (self.base.add(y * self.pitch + x * 4) as *mut u32).write_volatile(rgb) };
    }

    /// Writes a raw 0x00RRGGBB pixel; callers guarantee `x < width && y < height`.
    fn put_raw(&mut self, x: usize, y: usize, rgb: u32) {
        debug_assert!(x < self.width && y < self.height);
        unsafe { (self.base.add(y * self.pitch + x * 4) as *mut u32).write_volatile(rgb) };
    }

    /// Fills a rectangle, clamped to the framebuffer (no overflow, no OOB writes).
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return;
        }
        let x1 = x.saturating_add(w as usize).min(self.width);
        let y1 = y.saturating_add(h as usize).min(self.height);
        for yy in y..y1 {
            for xx in x..x1 {
                self.put_raw(xx, yy, color & 0x00FF_FFFF);
            }
        }
    }

    /// Draws ASCII `bytes` at pixel (x, y), blending `bg` to `fg` by glyph
    /// intensity; every pixel is clamped to the framebuffer.
    pub fn text_out(&mut self, x: u32, y: u32, bytes: &[u8], fg: u32, bg: u32) {
        let (cw, ch) = (Self::cell_width(), Self::cell_height());
        for (i, &b) in bytes.iter().enumerate() {
            let c = if (0x20..0x7F).contains(&b) {
                b as char
            } else {
                '?'
            };
            let glyph = get_raster(c, WEIGHT, HEIGHT)
                .or_else(|| get_raster('?', WEIGHT, HEIGHT))
                .expect("'?' existe na fonte");
            let x0 = (x as usize).saturating_add(i.saturating_mul(cw));
            if x0 >= self.width {
                break;
            }
            for (gy, line) in glyph.raster().iter().enumerate().take(ch) {
                let py = (y as usize).saturating_add(gy);
                if py >= self.height {
                    break;
                }
                for (gx, &v) in line.iter().enumerate() {
                    let px = x0.saturating_add(gx);
                    if px >= self.width {
                        break;
                    }
                    self.put_raw(px, py, blend(fg, bg, v));
                }
            }
        }
    }

    pub fn clear(&mut self) {
        unsafe { core::ptr::write_bytes(self.base, 0, self.pitch * self.height) };
        self.col = 0;
        self.row = 0;
    }

    fn draw_char(&mut self, c: char) {
        let glyph = get_raster(c, WEIGHT, HEIGHT)
            .or_else(|| get_raster('?', WEIGHT, HEIGHT))
            .expect("'?' existe na fonte");
        let (x0, y0) = (
            self.col * Self::cell_width(),
            self.row * Self::cell_height(),
        );
        for (y, line) in glyph.raster().iter().enumerate() {
            for (x, &v) in line.iter().enumerate() {
                self.put_pixel(x0 + x, y0 + y, v);
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows() {
            self.scroll();
            self.row = self.rows() - 1;
        }
    }

    fn scroll(&mut self) {
        let line = self.pitch * Self::cell_height();
        let used = line * self.rows();
        unsafe {
            core::ptr::copy(self.base.add(line), self.base, used - line);
            core::ptr::write_bytes(self.base.add(used - line), 0, line);
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => self.newline(),
                '\r' => self.col = 0,
                c => {
                    if self.col >= self.cols() {
                        self.newline();
                    }
                    self.draw_char(c);
                    self.col += 1;
                }
            }
        }
    }
}

impl fmt::Write for FbCon {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        FbCon::write_str(self, s);
        Ok(())
    }
}

/// Per-channel blend of `bg` towards `fg` by `t` (0..=255).
fn blend(fg: u32, bg: u32, t: u8) -> u32 {
    let t = t as u32;
    let mut out = 0;
    for shift in [0u32, 8, 16] {
        let f = (fg >> shift) & 0xFF;
        let b = (bg >> shift) & 0xFF;
        out |= ((b * (255 - t) + f * t) / 255) << shift;
    }
    out
}

/// `(width, height, bpp)` of the console framebuffer (bpp is always 32).
pub fn fb_info() -> Option<(u32, u32, u32)> {
    FBCON
        .lock()
        .as_ref()
        .map(|c| (c.width as u32, c.height as u32, 32))
}

pub fn fill_rect(x: u32, y: u32, w: u32, h: u32, color: u32) {
    if let Some(con) = FBCON.lock().as_mut() {
        con.fill_rect(x, y, w, h, color);
    }
}

pub fn text_out(x: u32, y: u32, bytes: &[u8], fg: u32, bg: u32) {
    if let Some(con) = FBCON.lock().as_mut() {
        con.text_out(x, y, bytes, fg, bg);
    }
}

/// Bounds-checked pixel read (0x00RRGGBB), `None` outside the framebuffer.
#[cfg_attr(not(feature = "ktest"), allow(dead_code))]
pub fn pixel(x: u32, y: u32) -> Option<u32> {
    let g = FBCON.lock();
    let con = g.as_ref()?;
    ((x as usize) < con.width && (y as usize) < con.height)
        .then(|| con.pixel(x as usize, y as usize) & 0x00FF_FFFF)
}

pub fn init_from_limine() {
    let Some(resp) = crate::FRAMEBUFFER_REQUEST.get_response() else {
        return;
    };
    let Some(fb) = resp.framebuffers().next() else {
        return;
    };
    if fb.bpp() != 32 {
        return;
    }
    let mut con = FbCon {
        base: fb.addr(),
        width: fb.width() as usize,
        height: fb.height() as usize,
        pitch: fb.pitch() as usize,
        col: 0,
        row: 0,
    };
    con.clear();
    *FBCON.lock() = Some(con);
}

pub fn write_fmt(args: fmt::Arguments) {
    if let Some(con) = FBCON.lock().as_mut() {
        let _ = fmt::Write::write_fmt(con, args);
    }
}
