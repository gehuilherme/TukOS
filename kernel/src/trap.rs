//! Kernel trap handling. Only exceptions exist at this stage.

use arch_x86_64::trap::TrapFrame;
use core::sync::atomic::{AtomicU64, Ordering};

pub static BREAKPOINTS: AtomicU64 = AtomicU64::new(0);

/// Counts how many times `handle` had to repair a corrupted return-to-Ring-3 iret
/// frame (see the defense-in-depth block in `handle`). 0 in normal operation.
pub static SEGMENT_FIXUPS: AtomicU64 = AtomicU64::new(0);

const BREAKPOINT: u64 = 3;

const NAMES: [&str; 32] = [
    "DIVIDE_ERROR",
    "DEBUG",
    "NMI",
    "BREAKPOINT",
    "OVERFLOW",
    "BOUND_RANGE",
    "INVALID_OPCODE",
    "DEVICE_NOT_AVAILABLE",
    "DOUBLE_FAULT",
    "COPROCESSOR_SEGMENT_OVERRUN",
    "INVALID_TSS",
    "SEGMENT_NOT_PRESENT",
    "STACK_FAULT",
    "GENERAL_PROTECTION",
    "PAGE_FAULT",
    "RESERVED_15",
    "X87_FLOATING_POINT",
    "ALIGNMENT_CHECK",
    "MACHINE_CHECK",
    "SIMD_FLOATING_POINT",
    "VIRTUALIZATION",
    "CONTROL_PROTECTION",
    "RESERVED_22",
    "RESERVED_23",
    "RESERVED_24",
    "RESERVED_25",
    "RESERVED_26",
    "RESERVED_27",
    "HYPERVISOR_INJECTION",
    "VMM_COMMUNICATION",
    "SECURITY",
    "RESERVED_31",
];

pub fn exception_name(vector: u64) -> &'static str {
    if vector >= 32 {
        return "IRQ";
    }
    NAMES.get(vector as usize).copied().unwrap_or("UNKNOWN")
}

/// NTSTATUS a waiting parent sees for a child killed by a Ring-3 exception
/// (never 0 and never STILL_ACTIVE=259).
pub fn fault_status(vector: u64) -> u32 {
    match vector {
        0 => 0xC000_0094,  // INTEGER_DIVIDE_BY_ZERO
        6 => 0xC000_001D,  // ILLEGAL_INSTRUCTION
        12 => 0xC000_00FD, // STACK_OVERFLOW
        _ => 0xC000_0005,  // ACCESS_VIOLATION (#PF, #GP, others)
    }
}

pub fn handle(frame: &mut TrapFrame) {
    // Defense-in-depth: repair a corrupted return-to-Ring-3 iret frame so a rare,
    // not-yet-isolated state corruption (a stray 0x18 selector in CS/SS) can never
    // #GP the kernel at `iretq`. See `arch_x86_64::trap::sanitize_return_frame`.
    // Clean frames (the normal path) are untouched. Root cause tracked in
    // docs/observations/2026-10-15-kernel-gp-shell-input.md.
    if arch_x86_64::trap::sanitize_return_frame(frame)
        && SEGMENT_FIXUPS.fetch_add(1, Ordering::SeqCst) == 0
    {
        crate::kprintln!("trap: frame de retorno a Ring 3 reparado (selector corrompido)");
    }

    if frame.vector == arch_x86_64::lapic::TIMER_VECTOR as u64 {
        crate::apic::on_timer();
        return;
    }
    if frame.vector == arch_x86_64::lapic::SPURIOUS_VECTOR as u64 {
        return; // spurious: no EOI per SDM 11.9
    }
    if frame.vector == BREAKPOINT {
        BREAKPOINTS.fetch_add(1, Ordering::SeqCst);
        return;
    }
    let cr2 = arch_x86_64::cpu::read_cr2();
    if frame.cs & 3 == 3 {
        // Ring-3 fault: terminate the faulting process, keep the kernel alive.
        // No kernel lock is held (we came from user mode); the trap frame is
        // abandoned exactly like on SYS_EXIT.
        crate::kprintln!(
            "falha de usuario: {} (vetor {}) err=0x{:x} rip=0x{:x} cr2=0x{:x}; processo terminado",
            exception_name(frame.vector),
            frame.vector,
            frame.error_code,
            frame.rip,
            cr2
        );
        crate::sched::exit_current_with(fault_status(frame.vector));
    }
    // CPL 0: a kernel fault is our bug.
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
