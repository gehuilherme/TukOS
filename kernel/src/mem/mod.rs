//! Kernel memory management: physical frames, paging and the heap.

pub mod frames;
pub mod heap;
pub mod paging;

/// Reads the Limine HHDM and memory-map responses and brings up the frame allocator.
pub fn init() {
    let hhdm = crate::HHDM_REQUEST
        .get_response()
        .expect("Limine nao forneceu HHDM")
        .offset();
    let map = crate::MEMORY_MAP_REQUEST
        .get_response()
        .expect("Limine nao forneceu o mapa de memoria");
    frames::init(hhdm, map.entries());
    heap::init();
}
