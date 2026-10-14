//! Text console drawn on the Limine framebuffer (32 bpp only).

use core::fmt;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};
use spin::Mutex;

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

pub static FBCON: Mutex<Option<FbCon>> = Mutex::new(None);

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
