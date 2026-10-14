//! Everything that depends on the x86-64 CPU. Pure encodings are host-testable;
//! hardware access is gated on `target_os = "none"`.
#![cfg_attr(not(test), no_std)]

pub mod gdt;
pub mod idt;
pub mod trap;
pub mod cpu;
pub mod port;
