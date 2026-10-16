//! Pure graphics math (rect clamping), testable on the host.
#![no_std]

/// Clamps a rect to a `width` x `height` surface. Returns half-open
/// `(x0, y0, x1, y1)`, or `None` when nothing is on-screen. Never overflows.
pub fn clamp_rect(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    width: u32,
    height: u32,
) -> Option<(usize, usize, usize, usize)> {
    if x >= width || y >= height {
        return None;
    }
    let x1 = (x as u64 + w as u64).min(width as u64) as usize;
    let y1 = (y as u64 + h as u64).min(height as u64) as usize;
    if x1 == x as usize || y1 == y as usize {
        return None;
    }
    Some((x as usize, y as usize, x1, y1))
}

/// The clipped source+destination of blitting a `w` x `h` surface placed at screen
/// position `(x, y)` (top-left, may be negative) onto a `screen_w` x `screen_h`
/// framebuffer. `src_*` are the offset into the source buffer, `dst_*` the offset
/// into the framebuffer, `w`/`h` the common copy size. `None` when the surface is
/// fully off-screen or has a zero dimension. Never overflows (i64 math).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blit {
    pub src_x: usize,
    pub src_y: usize,
    pub dst_x: usize,
    pub dst_y: usize,
    pub w: usize,
    pub h: usize,
}

/// Computes the clipped blit rectangles for a surface at `(x, y)` of size `w` x `h`
/// onto a `screen_w` x `screen_h` framebuffer. Handles surfaces partly or fully off
/// any edge (negative `x`/`y` clip the source start; oversize clips the extent).
pub fn blit_clip(x: i32, y: i32, w: u32, h: u32, screen_w: u32, screen_h: u32) -> Option<Blit> {
    if w == 0 || h == 0 || screen_w == 0 || screen_h == 0 {
        return None;
    }
    let (x, y) = (i64::from(x), i64::from(y));
    let (w, h) = (i64::from(w), i64::from(h));
    let (sw, sh) = (i64::from(screen_w), i64::from(screen_h));
    let dx0 = x.max(0);
    let dy0 = y.max(0);
    let dx1 = (x + w).min(sw);
    let dy1 = (y + h).min(sh);
    if dx1 <= dx0 || dy1 <= dy0 {
        return None;
    }
    Some(Blit {
        src_x: (dx0 - x) as usize,
        src_y: (dy0 - y) as usize,
        dst_x: dx0 as usize,
        dst_y: dy0 as usize,
        w: (dx1 - dx0) as usize,
        h: (dy1 - dy0) as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::{Blit, blit_clip, clamp_rect};

    fn blit(src: (usize, usize), dst: (usize, usize), wh: (usize, usize)) -> Option<Blit> {
        Some(Blit {
            src_x: src.0,
            src_y: src.1,
            dst_x: dst.0,
            dst_y: dst.1,
            w: wh.0,
            h: wh.1,
        })
    }

    #[test]
    fn blit_fully_on_screen() {
        assert_eq!(
            blit_clip(100, 80, 200, 150, 1024, 768),
            blit((0, 0), (100, 80), (200, 150))
        );
    }
    #[test]
    fn blit_off_right_edge() {
        assert_eq!(
            blit_clip(900, 0, 200, 100, 1024, 768),
            blit((0, 0), (900, 0), (124, 100))
        );
    }
    #[test]
    fn blit_off_bottom_edge() {
        assert_eq!(
            blit_clip(0, 700, 100, 150, 1024, 768),
            blit((0, 0), (0, 700), (100, 68))
        );
    }
    #[test]
    fn blit_off_top_edge() {
        assert_eq!(
            blit_clip(10, -20, 100, 100, 1024, 768),
            blit((0, 20), (10, 0), (100, 80))
        );
    }
    #[test]
    fn blit_off_left_edge() {
        assert_eq!(
            blit_clip(-30, 0, 100, 100, 1024, 768),
            blit((30, 0), (0, 0), (70, 100))
        );
    }
    #[test]
    fn blit_off_top_left_corner() {
        assert_eq!(
            blit_clip(-10, -10, 100, 100, 1024, 768),
            blit((10, 10), (0, 0), (90, 90))
        );
    }
    #[test]
    fn blit_fully_off_right_is_none() {
        assert_eq!(blit_clip(1024, 0, 100, 100, 1024, 768), None);
    }
    #[test]
    fn blit_fully_off_left_is_none() {
        assert_eq!(blit_clip(-100, 0, 50, 50, 1024, 768), None);
    }
    #[test]
    fn blit_larger_than_screen_covers_all() {
        assert_eq!(
            blit_clip(0, 0, 2000, 2000, 1024, 768),
            blit((0, 0), (0, 0), (1024, 768))
        );
    }
    #[test]
    fn blit_zero_dimension_is_none() {
        assert_eq!(blit_clip(0, 0, 0, 10, 100, 100), None);
        assert_eq!(blit_clip(0, 0, 10, 0, 100, 100), None);
    }

    #[test]
    fn in_bounds_exact() {
        assert_eq!(clamp_rect(10, 20, 5, 6, 100, 100), Some((10, 20, 15, 26)));
    }
    #[test]
    fn origin_at_edge_is_none() {
        assert_eq!(clamp_rect(100, 0, 5, 5, 100, 50), None);
        assert_eq!(clamp_rect(0, 50, 5, 5, 100, 50), None);
    }
    #[test]
    fn huge_width_clamps() {
        assert_eq!(clamp_rect(0, 0, u32::MAX, 1, 100, 50), Some((0, 0, 100, 1)));
    }
    #[test]
    fn last_row_only() {
        assert_eq!(clamp_rect(0, 49, 10, 50, 100, 50), Some((0, 49, 10, 50)));
    }
    #[test]
    fn all_max_is_none() {
        assert_eq!(
            clamp_rect(u32::MAX, u32::MAX, u32::MAX, u32::MAX, 100, 50),
            None
        );
    }
    #[test]
    fn zero_size_is_none() {
        assert_eq!(clamp_rect(5, 5, 0, 3, 100, 50), None);
        assert_eq!(clamp_rect(5, 5, 3, 0, 100, 50), None);
    }
}
