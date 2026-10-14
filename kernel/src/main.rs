#![no_std]
#![no_main]

#[macro_use]
mod console;
mod fbcon;
#[cfg(feature = "ktest")]
mod ktest;
mod panic;
mod qemu;
mod serial;
mod trap;

use limine::BaseRevision;
use limine::request::{FramebufferRequest, RequestsEndMarker, RequestsStartMarker};

#[used]
#[unsafe(link_section = ".requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    serial::init();
    fbcon::init_from_limine();
    kprintln!("TukOS {} iniciando", env!("CARGO_PKG_VERSION"));
    assert!(
        BASE_REVISION.is_supported(),
        "revisao base do Limine nao suportada"
    );

    unsafe {
        arch_x86_64::gdt::init();
        arch_x86_64::idt::init();
    }
    arch_x86_64::trap::set_handler(trap::handle);
    kprintln!("TukOS pronto");

    #[cfg(feature = "ktest")]
    ktest::run_all();

    #[cfg(feature = "ktest-panic")]
    panic!("ktest-panic deliberado");

    #[cfg(feature = "ktest-fault")]
    unsafe {
        core::ptr::read_volatile(0x0000_7fff_0000_0000 as *const u64);
    }

    #[allow(unreachable_code)]
    loop {
        arch_x86_64::cpu::hlt();
    }
}
