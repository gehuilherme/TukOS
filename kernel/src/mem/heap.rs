//! Kernel heap: a fixed virtual window backed by freshly allocated frames, with a
//! `FreeListHeap` behind a spinlock installed as the global allocator.

use core::alloc::{GlobalAlloc, Layout};
use mm::heap::FreeListHeap;
use mm::paging::PageFlags;
use spin::Mutex;

/// Heap virtual window: 2 MiB starting here (above the kernel image, below the
/// HHDM). 512 pages.
const HEAP_START: u64 = 0xffff_ff80_0000_0000;
const HEAP_PAGES: u64 = 512;
const HEAP_SIZE: usize = (HEAP_PAGES * 4096) as usize;

struct LockedHeap(Mutex<FreeListHeap>);

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.0.lock().alloc(layout.size(), layout.align()) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.0.lock().dealloc(ptr, layout.size(), layout.align()) }
    }
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap(Mutex::new(FreeListHeap::new()));

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
