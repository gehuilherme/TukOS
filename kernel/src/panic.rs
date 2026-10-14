use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch_x86_64::cpu::cli();
    // The panic may have interrupted a lock holder; nothing else runs now.
    unsafe { crate::serial::SERIAL.force_unlock() };
    let mut serial = crate::serial::SERIAL.lock();
    let _ = write!(serial, "@@TUKOS-PANIC {}", info.message());
    if let Some(loc) = info.location() {
        let _ = write!(serial, " @ {}:{}", loc.file(), loc.line());
    }
    let _ = writeln!(serial);
    print_backtrace(&mut serial);
    drop(serial);

    // Best-effort copy on screen, so someone watching the QEMU window sees it.
    if let Some(con) = crate::fbcon::FBCON
        .try_lock()
        .as_mut()
        .and_then(|g| g.as_mut())
    {
        let _ = write!(con, "KERNEL PANIC: {}", info.message());
    }

    crate::qemu::exit(crate::qemu::ExitCode::Failure)
}

/// Walks saved RBP frame pointers and prints return addresses. Requires
/// `-C force-frame-pointers=yes`. Bounded to avoid chasing a corrupt chain.
fn print_backtrace(serial: &mut crate::serial::Serial) {
    let mut rbp: u64;
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack)) };
    let _ = writeln!(serial, "@@TUKOS-BACKTRACE");
    for _ in 0..16 {
        if rbp == 0 || rbp & 0x7 != 0 {
            break;
        }
        // Stay in the canonical higher half (stack, HHDM, image): a wild rbp
        // must not be dereferenced inside the panic handler.
        if rbp < 0xffff_8000_0000_0000 {
            break;
        }
        // frame: [rbp] = saved rbp, [rbp+8] = return address
        let saved_rbp = unsafe { core::ptr::read_volatile(rbp as *const u64) };
        let ret = unsafe { core::ptr::read_volatile((rbp + 8) as *const u64) };
        if ret == 0 {
            break;
        }
        let _ = writeln!(serial, "  ret=0x{ret:x}");
        if saved_rbp <= rbp {
            break; // frames must grow downward in address; stop on anything else
        }
        rbp = saved_rbp;
    }
}
