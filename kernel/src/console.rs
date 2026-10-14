//! Kernel log output, fanned out to every available sink.

use core::fmt::{self, Write};

pub fn _print(args: fmt::Arguments) {
    let _ = crate::serial::SERIAL.lock().write_fmt(args);
    crate::fbcon::write_fmt(args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => { $crate::console::_print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => { $crate::kprint!("{}\n", format_args!($($arg)*)) };
}
