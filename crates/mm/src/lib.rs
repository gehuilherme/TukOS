//! Pure, host-testable memory-management logic: physical frame allocation,
//! virtual-address and page-table-entry arithmetic, and the kernel heap free list.
//! No hardware access lives here — the kernel wires this to real memory.
#![cfg_attr(not(test), no_std)]

pub mod frame;
