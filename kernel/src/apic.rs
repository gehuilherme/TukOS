//! Local APIC bring-up: map the MMIO page uncached, enable it, and run a periodic timer.

use arch_x86_64::lapic::*;
use core::sync::atomic::{AtomicU64, Ordering};
use mm::paging::PageFlags;

/// Virtual address where the LAPIC page is mapped (free high-half slot).
const LAPIC_VIRT: u64 = 0xffff_fe00_0000_0000;
/// Initial count for the periodic timer (divide-by-16). Tuned for a visible tick rate in QEMU.
const TIMER_INITIAL_COUNT: u32 = 0x0010_0000;

pub static TICKS: AtomicU64 = AtomicU64::new(0);

fn reg(offset: usize) -> *mut u32 {
    (LAPIC_VIRT as usize + offset) as *mut u32
}

fn write(offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile(reg(offset), value) };
}

pub fn eoi() {
    write(REG_EOI, 0);
}

pub fn init() {
    let msr = unsafe { arch_x86_64::cpu::read_msr(IA32_APIC_BASE) };
    if msr & (1 << 10) != 0 {
        panic!("LAPIC em modo x2APIC; TukOS so suporta xAPIC via MMIO neste estagio");
    }
    let base = apic_base_phys(msr);
    let flags = PageFlags::WRITABLE
        .union(PageFlags::NO_EXECUTE)
        .union(PageFlags::CACHE_DISABLE);
    unsafe {
        crate::mem::paging::map_page(LAPIC_VIRT, base, flags).expect("falha ao mapear o LAPIC");
    }
    write(REG_SPURIOUS, spurious_value(SPURIOUS_VECTOR));
    write(REG_TIMER_DIVIDE, TIMER_DIVIDE_BY_16);
    write(REG_LVT_TIMER, lvt_timer_periodic(TIMER_VECTOR));
    write(REG_TIMER_INITIAL, TIMER_INITIAL_COUNT);
}

/// Called from the trap handler for the timer vector.
pub fn on_timer() {
    TICKS.fetch_add(1, Ordering::SeqCst);
    eoi();
    crate::sched::on_tick();
}
