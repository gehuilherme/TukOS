//! The real `gdi32.dll` (built by `cargo xtask gdi32`), embedded at build time.
//! Empty when the host could not provide it (no MSVC); see `build.rs`.

pub static GDI32_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gdi32.dll"));
