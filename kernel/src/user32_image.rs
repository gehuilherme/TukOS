//! The real `user32.dll` (built by `cargo xtask user32`), embedded at build time.
//! Empty when the host could not provide it (no MSVC); see `build.rs`.

pub static USER32_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/user32.dll"));
