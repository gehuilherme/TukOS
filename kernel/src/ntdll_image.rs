//! The real `ntdll.dll` (built by `cargo xtask ntdll`), embedded at build time.
//! Empty when the host could not provide it (no MSVC); see `build.rs`.

pub static NTDLL_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ntdll.dll"));
