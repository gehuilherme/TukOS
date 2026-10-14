//! Bitmap physical frame allocator. One bit per 4 KiB frame; 1 = used, 0 = free.
//! Storage is caller-provided so the kernel can place it in a real memory region.

/// Size of a physical frame, in bytes.
pub const FRAME_SIZE: u64 = 4096;

/// Number of frames needed to cover addresses up to (but not including) `highest_addr`.
pub const fn total_frames_for(highest_addr: u64) -> usize {
    highest_addr.div_ceil(FRAME_SIZE) as usize
}

/// Number of `u64` words needed to hold one bit per frame.
pub const fn bitmap_words_for(total_frames: usize) -> usize {
    total_frames.div_ceil(64)
}

pub struct BitmapAllocator<'a> {
    bitmap: &'a mut [u64],
    total_frames: usize,
    free: usize,
}

impl<'a> BitmapAllocator<'a> {
    /// Creates an allocator with every frame marked used. `bitmap` must hold at
    /// least `bitmap_words_for(total_frames)` words.
    pub fn new(bitmap: &'a mut [u64], total_frames: usize) -> Self {
        for word in bitmap.iter_mut() {
            *word = u64::MAX;
        }
        Self {
            bitmap,
            total_frames,
            free: 0,
        }
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn free_count(&self) -> usize {
        self.free
    }

    fn is_used(&self, frame: usize) -> bool {
        self.bitmap[frame / 64] & (1u64 << (frame % 64)) != 0
    }

    fn set_used(&mut self, frame: usize) {
        self.bitmap[frame / 64] |= 1u64 << (frame % 64);
    }

    fn set_free_bit(&mut self, frame: usize) {
        self.bitmap[frame / 64] &= !(1u64 << (frame % 64));
    }

    /// Marks every frame fully contained in `[base, base + length)` as free.
    pub fn mark_region_usable(&mut self, base: u64, length: u64) {
        if length == 0 {
            return;
        }
        let first = base.div_ceil(FRAME_SIZE);
        let end = (base + length) / FRAME_SIZE; // exclusive; floor keeps only full frames
        let mut frame = first as usize;
        let last = end as usize;
        while frame < last && frame < self.total_frames {
            if self.is_used(frame) {
                self.set_free_bit(frame);
                self.free += 1;
            }
            frame += 1;
        }
    }

    pub fn mark_frame_used(&mut self, frame: usize) {
        if frame < self.total_frames && !self.is_used(frame) {
            self.set_used(frame);
            self.free -= 1;
        }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        if self.free == 0 {
            return None;
        }
        for (word_index, word) in self.bitmap.iter().enumerate() {
            if *word != u64::MAX {
                let bit = word.trailing_ones() as usize;
                let frame = word_index * 64 + bit;
                if frame >= self.total_frames {
                    break;
                }
                self.set_used(frame);
                self.free -= 1;
                return Some(frame);
            }
        }
        None
    }

    pub fn free(&mut self, frame: usize) {
        if frame < self.total_frames && self.is_used(frame) {
            self.set_free_bit(frame);
            self.free += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allocator(total_frames: usize) -> (Vec<u64>, usize) {
        (vec![0u64; bitmap_words_for(total_frames)], total_frames)
    }

    #[test]
    fn total_frames_for_rounds_up() {
        assert_eq!(total_frames_for(0), 0);
        assert_eq!(total_frames_for(1), 1);
        assert_eq!(total_frames_for(FRAME_SIZE), 1);
        assert_eq!(total_frames_for(FRAME_SIZE + 1), 2);
    }

    #[test]
    fn bitmap_words_for_rounds_up_to_u64() {
        assert_eq!(bitmap_words_for(0), 0);
        assert_eq!(bitmap_words_for(1), 1);
        assert_eq!(bitmap_words_for(64), 1);
        assert_eq!(bitmap_words_for(65), 2);
    }

    #[test]
    fn new_starts_fully_used() {
        let (mut words, total) = allocator(128);
        let a = BitmapAllocator::new(&mut words, total);
        assert_eq!(a.free_count(), 0);
        assert_eq!(a.total_frames(), 128);
    }

    #[test]
    fn mark_region_frees_contained_frames_and_alloc_roundtrips() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        // Frames 2,3,4 fully inside [2*4096, 5*4096).
        a.mark_region_usable(2 * FRAME_SIZE, 3 * FRAME_SIZE);
        assert_eq!(a.free_count(), 3);
        let f = a.alloc().unwrap();
        assert!((2..=4).contains(&f));
        assert_eq!(a.free_count(), 2);
        a.free(f);
        assert_eq!(a.free_count(), 3);
    }

    #[test]
    fn mark_region_handles_unaligned_bounds() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        // [4096+1, 4096+1 + 3*4096): only frames 2 and 3 are fully contained.
        a.mark_region_usable(FRAME_SIZE + 1, 3 * FRAME_SIZE);
        assert_eq!(a.free_count(), 2);
    }

    #[test]
    fn alloc_only_returns_usable_frames() {
        let (mut words, total) = allocator(64);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(10 * FRAME_SIZE, 2 * FRAME_SIZE); // frames 10,11
        let mut seen = Vec::new();
        while let Some(f) = a.alloc() {
            assert!(f == 10 || f == 11);
            assert!(!seen.contains(&f), "frame devolvido duas vezes");
            seen.push(f);
        }
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn mark_frame_used_removes_from_free_pool() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, 16 * FRAME_SIZE);
        a.mark_frame_used(5);
        assert_eq!(a.free_count(), 15);
        for _ in 0..15 {
            assert_ne!(a.alloc(), Some(5));
        }
        assert_eq!(a.alloc(), None);
    }

    #[test]
    fn double_free_is_idempotent() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, 4 * FRAME_SIZE);
        let f = a.alloc().unwrap();
        a.free(f);
        a.free(f);
        assert_eq!(a.free_count(), 4);
    }

    #[test]
    fn free_out_of_range_is_ignored() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, 4 * FRAME_SIZE);
        a.free(9999);
        assert_eq!(a.free_count(), 4);
    }

    #[test]
    fn total_frames_uses_highest_region_end() {
        // A gap then a high region: total frames must cover up to the highest end.
        let total = total_frames_for(100 * FRAME_SIZE);
        let (mut words, _) = allocator(total);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, FRAME_SIZE);
        a.mark_region_usable(99 * FRAME_SIZE, FRAME_SIZE);
        assert_eq!(a.free_count(), 2);
        assert_eq!(a.total_frames(), 100);
    }
}
