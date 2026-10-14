//! Kernel trap handling. Only exceptions exist at this stage.

use arch_x86_64::trap::TrapFrame;
use core::sync::atomic::{AtomicU64, Ordering};

pub static BREAKPOINTS: AtomicU64 = AtomicU64::new(0);

const BREAKPOINT: u64 = 3;

const NAMES: [&str; 32] = [
    "DIVIDE_ERROR", "DEBUG", "NMI", "BREAKPOINT", "OVERFLOW", "BOUND_RANGE", "INVALID_OPCODE",
    "DEVICE_NOT_AVAILABLE", "DOUBLE_FAULT", "COPROCESSOR_SEGMENT_OVERRUN", "INVALID_TSS",
    "SEGMENT_NOT_PRESENT", "STACK_FAULT", "GENERAL_PROTECTION", "PAGE_FAULT", "RESERVED_15",
    "X87_FLOATING_POINT", "ALIGNMENT_CHECK", "MACHINE_CHECK", "SIMD_FLOATING_POINT",
    "VIRTUALIZATION", "CONTROL_PROTECTION", "RESERVED_22", "RESERVED_23", "RESERVED_24",
    "RESERVED_25", "RESERVED_26", "RESERVED_27", "HYPERVISOR_INJECTION", "VMM_COMMUNICATION",
    "SECURITY", "RESERVED_31",
];

pub fn exception_name(vector: u64) -> &'static str {
    NAMES.get(vector as usize).copied().unwrap_or("UNKNOWN")
}

pub fn handle(frame: &mut TrapFrame) {
    if frame.vector == BREAKPOINT {
        BREAKPOINTS.fetch_add(1, Ordering::SeqCst);
        return;
    }
    let cr2 = arch_x86_64::cpu::read_cr2();
    panic!(
        "excecao {} {} err=0x{:x} rip=0x{:x} cr2=0x{:x}\n{:#x?}",
        frame.vector,
        exception_name(frame.vector),
        frame.error_code,
        frame.rip,
        cr2,
        frame
    );
}
