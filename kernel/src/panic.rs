use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch_x86_64::cpu::cli();
    // The panic may have interrupted a holder of the lock; nothing else runs now.
    unsafe { crate::serial::SERIAL.force_unlock() };
    let mut serial = crate::serial::SERIAL.lock();
    let _ = write!(serial, "@@TUKOS-PANIC {}", info.message());
    if let Some(loc) = info.location() {
        let _ = write!(serial, " @ {}:{}", loc.file(), loc.line());
    }
    let _ = writeln!(serial);
    drop(serial);
    crate::qemu::exit(crate::qemu::ExitCode::Failure)
}
