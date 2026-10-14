//! Everything that depends on the x86-64 CPU. Pure encodings are host-testable.
//! `cpu` and `port` compile on any x86-64 host (they are not gated); the
//! hardware-initialisation code in `gdt`, `idt` and `trap` is gated on
//! `target_os = "none"`.
#![cfg_attr(not(test), no_std)]

pub mod context;
pub mod cpu;
pub mod gdt;
pub mod idt;
pub mod interrupts;
pub mod lapic;
pub mod percpu;
pub mod port;
pub mod trap;
