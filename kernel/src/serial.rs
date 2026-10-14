//! 16550 UART on COM1, used for diagnostics and the test protocol.

use arch_x86_64::port::{inb, outb};
use core::fmt;
use spin::Mutex;

const COM1: u16 = 0x3F8;

pub struct Serial;

impl Serial {
    fn write_byte(&mut self, b: u8) {
        unsafe {
            while inb(COM1 + 5) & 0x20 == 0 {}
            outb(COM1, b);
        }
    }
}

impl fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        s.bytes().for_each(|b| self.write_byte(b));
        Ok(())
    }
}

pub static SERIAL: Mutex<Serial> = Mutex::new(Serial);

/// 115200 baud, 8N1, FIFO enabled.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1, 0x01);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0xC7);
        outb(COM1 + 4, 0x0B);
    }
}
