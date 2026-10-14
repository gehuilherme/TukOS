//! QEMU `isa-debug-exit` device (iobase 0xf4). On real hardware the write is ignored.

#[repr(u32)]
pub enum ExitCode {
    #[allow(dead_code)] // constructed only by the `ktest` feature
    Success = 0x10,
    Failure = 0x11,
}

pub fn exit(code: ExitCode) -> ! {
    unsafe { arch_x86_64::port::outl(0xf4, code as u32) };
    loop {
        arch_x86_64::cpu::hlt();
    }
}
