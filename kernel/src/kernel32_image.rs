//! The real `kernel32.dll` (built by `cargo xtask kernel32`), embedded at build time.
//! Empty when the host could not provide it (no MSVC); see `build.rs`.

pub static KERNEL32_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kernel32.dll"));
