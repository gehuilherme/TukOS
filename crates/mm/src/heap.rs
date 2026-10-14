//! A first-fit free-list heap. Free blocks hold an intrusive `FreeNode` at their
//! start; the list is kept sorted by address so adjacent frees can coalesce.
//! Every allocation stores a `Header { origin, block_size }` immediately below its
//! payload so `dealloc` can reconstruct the exact block that was carved out.

use core::mem::{align_of, size_of};
use core::ptr;

#[repr(C)]
struct FreeNode {
    size: usize,
    next: *mut FreeNode,
}

#[repr(C)]
struct Header {
    origin: usize,
    block_size: usize,
}

const NODE_ALIGN: usize = align_of::<FreeNode>();
const NODE_SIZE: usize = size_of::<FreeNode>();
const HEADER_SIZE: usize = size_of::<Header>();

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

pub struct FreeListHeap {
    head: *mut FreeNode,
    free: usize,
}

// The heap is only ever touched behind a lock in the kernel.
unsafe impl Send for FreeListHeap {}

impl Default for FreeListHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl FreeListHeap {
    pub const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
            free: 0,
        }
    }

    pub fn free_bytes(&self) -> usize {
        self.free
    }

    /// # Safety
    /// `[start, start+size)` must be a usable, writable, exclusively-owned region.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        let aligned = align_up(start, NODE_ALIGN);
        let usable = (size - (aligned - start)) & !(NODE_ALIGN - 1);
        if usable < NODE_SIZE {
            return;
        }
        let node = aligned as *mut FreeNode;
        unsafe {
            (*node).size = usable;
            (*node).next = ptr::null_mut();
        }
        self.head = node;
        self.free = usable;
    }

    /// # Safety
    /// Standard allocator contract. Returns null when no block fits.
    pub unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        let align = align.max(NODE_ALIGN);
        let payload_len = align_up(size.max(1), NODE_ALIGN);
        let mut prev: *mut *mut FreeNode = &mut self.head;
        unsafe {
            while !(*prev).is_null() {
                let block = *prev;
                let block_addr = block as usize;
                let block_size = (*block).size;
                let payload = align_up(block_addr + HEADER_SIZE, align);
                let used_end = payload + payload_len;
                if used_end <= block_addr + block_size {
                    let remainder = block_addr + block_size - used_end;
                    let taken = if remainder >= NODE_SIZE {
                        let rem = used_end as *mut FreeNode;
                        (*rem).size = remainder;
                        (*rem).next = (*block).next;
                        *prev = rem;
                        used_end - block_addr
                    } else {
                        *prev = (*block).next;
                        block_size
                    };
                    self.free -= taken;
                    let header = (payload - HEADER_SIZE) as *mut Header;
                    (*header).origin = block_addr;
                    (*header).block_size = taken;
                    return payload as *mut u8;
                }
                prev = &mut (*block).next;
            }
        }
        ptr::null_mut()
    }

    /// # Safety
    /// `ptr` must come from `alloc` with the same `size`/`align`.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, _size: usize, _align: usize) {
        unsafe {
            let header = (ptr as usize - HEADER_SIZE) as *const Header;
            let origin = (*header).origin;
            let total = (*header).block_size;
            let block = origin as *mut FreeNode;
            (*block).size = total;
            (*block).next = ptr::null_mut();
            self.insert_sorted(block);
            self.free += total;
        }
    }

    unsafe fn insert_sorted(&mut self, block: *mut FreeNode) {
        unsafe {
            let addr = block as usize;
            let mut prev: *mut *mut FreeNode = &mut self.head;
            let mut prev_node: *mut FreeNode = ptr::null_mut();
            while !(*prev).is_null() && (*prev as usize) < addr {
                prev_node = *prev;
                prev = &mut (*prev_node).next;
            }
            (*block).next = *prev;
            *prev = block;
            Self::coalesce(block);
            if !prev_node.is_null() {
                Self::coalesce(prev_node);
            }
        }
    }

    /// Merges `block` with its successor if they are adjacent.
    unsafe fn coalesce(block: *mut FreeNode) {
        unsafe {
            let next = (*block).next;
            if !next.is_null() && block as usize + (*block).size == next as usize {
                (*block).size += (*next).size;
                (*block).next = (*next).next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 64 KiB arena on the host heap, 16-byte aligned.
    fn arena() -> (Vec<u64>, usize, usize) {
        let words = 8192; // 64 KiB
        let v = vec![0u64; words];
        let start = v.as_ptr() as usize;
        (v, start, words * 8)
    }

    #[test]
    fn alloc_returns_aligned_nonnull() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p = unsafe { h.alloc(100, 8) };
        assert!(!p.is_null());
        assert_eq!(p as usize % 8, 0);
    }

    #[test]
    fn heap_respects_alignment() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p = unsafe { h.alloc(1, 64) };
        assert!(!p.is_null());
        assert_eq!(p as usize % 64, 0);
    }

    #[test]
    fn heap_returns_null_when_exhausted() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p = unsafe { h.alloc(size * 2, 8) };
        assert!(p.is_null());
    }

    #[test]
    fn heap_reuses_freed_block() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p1 = unsafe { h.alloc(4096, 8) };
        assert!(!p1.is_null());
        unsafe { h.dealloc(p1, 4096, 8) };
        let p2 = unsafe { h.alloc(4096, 8) };
        assert_eq!(p1, p2, "o bloco liberado deveria ser reutilizado");
    }

    #[test]
    fn distinct_allocations_do_not_overlap() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let a = unsafe { h.alloc(1000, 8) } as usize;
        let b = unsafe { h.alloc(1000, 8) } as usize;
        assert!(a != 0 && b != 0);
        assert!(a + 1000 <= b || b + 1000 <= a, "alocacoes se sobrepoem");
    }
}
