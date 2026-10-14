//! Global physical frame allocator, backed by `mm::frame::BitmapAllocator`.
//! The bitmap itself is carved out of the largest usable region and addressed
//! through the HHDM, so no chicken-and-egg allocation is needed.

use limine::memory_map::{Entry, EntryType};
use mm::frame::{BitmapAllocator, bitmap_words_for, total_frames_for};
use arch_x86_64::interrupts::IrqMutex;

pub use mm::frame::FRAME_SIZE;

struct State {
    allocator: BitmapAllocator<'static>,
    hhdm: u64,
}

static STATE: IrqMutex<Option<State>> = IrqMutex::new(None);

pub fn hhdm_offset() -> u64 {
    STATE.lock().as_ref().expect("frames nao inicializado").hhdm
}

pub fn phys_to_virt(phys: u64) -> u64 {
    phys + hhdm_offset()
}

pub fn free_frame_count() -> usize {
    STATE
        .lock()
        .as_ref()
        .map(|s| s.allocator.free_count())
        .unwrap_or(0)
}

pub fn alloc_frame() -> Option<u64> {
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    state.allocator.alloc().map(|f| f as u64 * FRAME_SIZE)
}

pub fn free_frame(phys: u64) {
    if let Some(state) = STATE.lock().as_mut() {
        state.allocator.free((phys / FRAME_SIZE) as usize);
    }
}

/// Builds the allocator from the Limine memory map.
///
/// Strategy: find the highest region end to size the bitmap, place the bitmap at
/// the start of the largest usable region (reached via the HHDM), then mark every
/// usable frame free and immediately reserve the frames the bitmap occupies.
pub fn init(hhdm: u64, map: &[&Entry]) {
    let highest_end = map.iter().map(|e| e.base + e.length).max().unwrap_or(0);
    let total_frames = total_frames_for(highest_end);
    let words = bitmap_words_for(total_frames);
    let bitmap_bytes = (words * core::mem::size_of::<u64>()) as u64;

    // Largest usable region holds the bitmap.
    let host = map
        .iter()
        .filter(|e| e.entry_type == EntryType::USABLE)
        .max_by_key(|e| e.length)
        .expect("nenhuma regiao usavel no mapa de memoria");
    assert!(
        host.length >= bitmap_bytes,
        "maior regiao usavel nao comporta o bitmap de frames"
    );

    let bitmap_phys = host.base;
    let bitmap_ptr = (bitmap_phys + hhdm) as *mut u64;
    let bitmap: &'static mut [u64] = unsafe { core::slice::from_raw_parts_mut(bitmap_ptr, words) };

    let mut allocator = BitmapAllocator::new(bitmap, total_frames);
    for e in map.iter().filter(|e| e.entry_type == EntryType::USABLE) {
        allocator.mark_region_usable(e.base, e.length);
    }

    // Reserve the frames the bitmap itself occupies.
    let bitmap_frames = bitmap_bytes.div_ceil(FRAME_SIZE);
    let first = bitmap_phys / FRAME_SIZE;
    for f in first..first + bitmap_frames {
        allocator.mark_frame_used(f as usize);
    }

    *STATE.lock() = Some(State { allocator, hhdm });
}
