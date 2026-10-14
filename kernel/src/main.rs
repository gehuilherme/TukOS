#![no_std]
#![no_main]

#[macro_use]
mod console;
#[cfg(feature = "ktest")]
mod ktest;
mod panic;
mod qemu;
mod serial;

use limine::BaseRevision;
use limine::request::{RequestsEndMarker, RequestsStartMarker};

#[used]
#[unsafe(link_section = ".requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    serial::init();
    kprintln!("TukOS {} iniciando", env!("CARGO_PKG_VERSION"));
    assert!(BASE_REVISION.is_supported(), "revisao base do Limine nao suportada");

    kprintln!("TukOS pronto");

    #[cfg(feature = "ktest")]
    ktest::run_all();

    #[cfg(feature = "ktest-panic")]
    panic!("ktest-panic deliberado");

    #[allow(unreachable_code)]
    loop {
        arch_x86_64::cpu::hlt();
    }
}
