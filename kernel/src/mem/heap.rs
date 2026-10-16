//! Kernel heap: a fixed virtual window backed by freshly allocated frames, with a
//! `FreeListHeap` behind a spinlock installed as the global allocator.

use arch_x86_64::interrupts::IrqMutex;
use core::alloc::{GlobalAlloc, Layout};
use mm::heap::FreeListHeap;
use mm::paging::PageFlags;

/// Heap virtual window: starting here (above the kernel image, below the HHDM).
/// Plano 25 raised this from 512 pages (2 MiB) to 8 MiB so a per-window
/// full-screen backbuffer (e.g. 1024x768x4 = 3 MiB) fits on the kernel heap; the
/// window still lives entirely inside one PML4 entry, so the higher-half mapping
/// is unaffected. The backing frames are mapped once at init, so heap alloc/free
/// never changes `free_frame_count` (the per-test frame accounting is unaffected).
const HEAP_START: u64 = 0xffff_ff80_0000_0000;
const HEAP_PAGES: u64 = 2048;
const HEAP_SIZE: usize = (HEAP_PAGES * 4096) as usize;

struct LockedHeap(IrqMutex<FreeListHeap>);

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.0.lock().alloc(layout.size(), layout.align()) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.0.lock().dealloc(ptr, layout.size(), layout.align()) }
    }
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap(IrqMutex::new(FreeListHeap::new()));

/// Maps the heap window to fresh frames and initializes the free list.
pub fn init() {
    for page in 0..HEAP_PAGES {
        let virt = HEAP_START + page * 4096;
        let phys = crate::mem::frames::alloc_frame().expect("sem frames para o heap");
        let flags = PageFlags::WRITABLE.union(PageFlags::NO_EXECUTE);
        unsafe {
            crate::mem::paging::map_page(virt, phys, flags).expect("falha ao mapear o heap");
        }
    }
    unsafe { ALLOCATOR.0.lock().init(HEAP_START as usize, HEAP_SIZE) };
}
