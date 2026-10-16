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
    cursor_enabled: bool,
    cursor_shown: bool,
    cursor_x: usize,
    cursor_y: usize,
    save_under: [u32; CUR_W * CUR_H],
}

const CUR_W: usize = 12;
const CUR_H: usize = 19;
const CUR_BLACK: u32 = 0x0000_0000;
const CUR_WHITE: u32 = 0x00FF_FFFF;

/// Seta propria (clean-room): 'X' contorno preto, 'W' miolo branco, '.' transparente.
/// O ponto ativo e o canto superior esquerdo.
const CURSOR_SPRITE: [&[u8; CUR_W]; CUR_H] = [
    b"X...........",
    b"XX..........",
    b"XWX.........",
    b"XWWX........",
    b"XWWWX.......",
    b"XWWWWX......",
    b"XWWWWWX.....",
    b"XWWWWWWX....",
    b"XWWWWWWWX...",
    b"XWWWWWWWWX..",
    b"XWWWWWWWWWX.",
    b"XWWWWWXXXXXX",
    b"XWWXWWX.....",
    b"XWX.XWWX....",
    b"XX..XWWX....",
    b"X....XWWX...",
    b".....XWWX...",
    b"......XX....",
    b"............",
];

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
        self.cursor_hide_for_draw();
        self.fill_rect_inner(x, y, w, h, color);
        self.cursor_show_after_draw();
    }

    fn fill_rect_inner(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let Some((x, y, x1, y1)) =
            gfx::clamp_rect(x, y, w, h, self.width as u32, self.height as u32)
        else {
            return;
        };
        for yy in y..y1 {
            for xx in x..x1 {
                self.put_raw(xx, yy, color & 0x00FF_FFFF);
            }
        }
    }

    /// Draws ASCII `bytes` at pixel (x, y), blending `bg` to `fg` by glyph
    /// intensity; every pixel is clamped to the framebuffer.
    pub fn text_out(&mut self, x: u32, y: u32, bytes: &[u8], fg: u32, bg: u32) {
        self.cursor_hide_for_draw();
        self.text_out_inner(x, y, bytes, fg, bg);
        self.cursor_show_after_draw();
    }

    fn text_out_inner(&mut self, x: u32, y: u32, bytes: &[u8], fg: u32, bg: u32) {
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
        self.cursor_hide_for_draw();
        unsafe { core::ptr::write_bytes(self.base, 0, self.pitch * self.height) };
        self.col = 0;
        self.row = 0;
        self.cursor_show_after_draw();
    }

    // --- Camada de cursor (todos assumem o lock do FBCON ja tomado) ---

    fn cursor_hide_for_draw(&mut self) {
        if self.cursor_shown {
            self.cursor_restore();
        }
    }

    fn cursor_show_after_draw(&mut self) {
        if self.cursor_enabled {
            self.cursor_save_and_draw();
        }
    }

    /// Sub-retangulo visivel do sprite na posicao atual: (x0, y0, x1, y1) absolutos.
    fn cursor_visible(&self) -> Option<(usize, usize, usize, usize)> {
        let (cx, cy) = (
            u32::try_from(self.cursor_x).ok()?,
            u32::try_from(self.cursor_y).ok()?,
        );
        let (x0, y0, x1, y1) = gfx::clamp_rect(
            cx,
            cy,
            CUR_W as u32,
            CUR_H as u32,
            self.width as u32,
            self.height as u32,
        )?;
        Some((x0, y0, x1, y1))
    }

    fn cursor_save_and_draw(&mut self) {
        if let Some((x0, y0, x1, y1)) = self.cursor_visible() {
            for y in y0..y1 {
                for x in x0..x1 {
                    let (sx, sy) = (x - self.cursor_x, y - self.cursor_y);
                    self.save_under[sy * CUR_W + sx] = self.pixel(x, y);
                    match CURSOR_SPRITE[sy][sx] {
                        b'X' => self.put_raw(x, y, CUR_BLACK),
                        b'W' => self.put_raw(x, y, CUR_WHITE),
                        _ => {}
                    }
                }
            }
        }
        self.cursor_shown = true;
    }

    fn cursor_restore(&mut self) {
        if let Some((x0, y0, x1, y1)) = self.cursor_visible() {
            for y in y0..y1 {
                for x in x0..x1 {
                    let (sx, sy) = (x - self.cursor_x, y - self.cursor_y);
                    if CURSOR_SPRITE[sy][sx] != b'.' {
                        let v = self.save_under[sy * CUR_W + sx];
                        self.put_raw(x, y, v);
                    }
                }
            }
        }
        self.cursor_shown = false;
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
        self.cursor_hide_for_draw();
        self.write_str_inner(s);
        self.cursor_show_after_draw();
    }

    fn write_str_inner(&mut self, s: &str) {
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

/// Liga a camada de cursor (nao desenha ate `cursor_show_at`/`cursor_move_to`).
#[allow(dead_code)]
pub fn cursor_enable() {
    if let Some(con) = FBCON.lock().as_mut() {
        con.cursor_enabled = true;
    }
}

#[allow(dead_code)]
pub fn cursor_disable() {
    if let Some(con) = FBCON.lock().as_mut() {
        con.cursor_hide_for_draw();
        con.cursor_enabled = false;
    }
}

#[allow(dead_code)]
pub fn cursor_show_at(x: u32, y: u32) {
    if let Some(con) = FBCON.lock().as_mut() {
        if !con.cursor_enabled {
            return;
        }
        con.cursor_hide_for_draw();
        con.cursor_x = x as usize;
        con.cursor_y = y as usize;
        con.cursor_save_and_draw();
    }
}

#[allow(dead_code)]
pub fn cursor_hide() {
    if let Some(con) = FBCON.lock().as_mut() {
        con.cursor_hide_for_draw();
    }
}

/// Esconde no ponto antigo, move e mostra no novo, sob um unico lock.
#[allow(dead_code)]
pub fn cursor_move_to(x: u32, y: u32) {
    cursor_show_at(x, y);
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
        cursor_enabled: false,
        cursor_shown: false,
        cursor_x: 0,
        cursor_y: 0,
        save_under: [0; CUR_W * CUR_H],
    };
    con.clear();
    *FBCON.lock() = Some(con);
}

pub fn write_fmt(args: fmt::Arguments) {
    if let Some(con) = FBCON.lock().as_mut() {
        let _ = fmt::Write::write_fmt(con, args);
    }
}
