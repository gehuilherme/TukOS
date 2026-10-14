#![no_std]
#![no_main]

mod apic;
#[macro_use]
mod console;
mod fbcon;
#[cfg(feature = "ktest")]
mod ktest;
mod mem;
mod panic;
mod qemu;
mod sched;
mod serial;
mod syscall;
mod trap;
mod user_demo;

use limine::BaseRevision;
use limine::request::{
    FramebufferRequest, HhdmRequest, MemoryMapRequest, RequestsEndMarker, RequestsStartMarker,
};

#[used]
#[unsafe(link_section = ".requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

/// Physical base of the active PML4 (CR3 with flag bits cleared).
pub fn arch_cr3_base() -> u64 {
    arch_x86_64::cpu::read_cr3() & 0x000F_FFFF_FFFF_F000
}

static mut PERCPU: arch_x86_64::percpu::PerCpu = arch_x86_64::percpu::PerCpu::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    serial::init();
    // Install the trap machinery first so no early exception is silent.
    arch_x86_64::trap::set_handler(trap::handle);
    unsafe {
        arch_x86_64::gdt::init();
        arch_x86_64::idt::init();
        arch_x86_64::percpu::install(&raw const PERCPU);
    }
    fbcon::init_from_limine();
    kprintln!("TukOS {} iniciando", env!("CARGO_PKG_VERSION"));
    assert!(
        BASE_REVISION.is_supported(),
        "revisao base do Limine nao suportada"
    );

    mem::init();
    kprintln!("memoria: {} frames livres", mem::frames::free_frame_count());
    sched::init();
    apic::init();
    arch_x86_64::interrupts::mask_legacy_pic();
    arch_x86_64::interrupts::enable();
    kprintln!(
        "interrupcoes ligadas (timer no vetor {})",
        arch_x86_64::lapic::TIMER_VECTOR
    );

    syscall::init();
    kprintln!("TukOS pronto");

    #[cfg(feature = "ktest")]
    ktest::run_all();

    #[cfg(feature = "ktest-panic")]
    {
        // Prove the panic path works even while the serial lock is held.
        let _guard = serial::SERIAL.lock();
        panic!("ktest-panic deliberado");
    }

    #[cfg(feature = "ktest-fault")]
    unsafe {
        core::ptr::read_volatile(0x0000_7fff_0000_0000 as *const u64);
    }

    #[cfg(feature = "ktest-backtrace")]
    {
        fn deep(n: u32) {
            if n == 0 {
                panic!("backtrace deliberado");
            }
            deep(n - 1);
            // Prevent tail-call optimization from flattening the frames.
            core::hint::black_box(n);
        }
        deep(3);
    }

    #[allow(unreachable_code)]
    loop {
        arch_x86_64::cpu::hlt();
    }
}
