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

#[cfg(test)]
mod tests {
    use super::clamp_rect;

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
