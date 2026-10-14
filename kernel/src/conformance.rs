//! Conformance boot mode: runs the embedded MSVC-built `.exe` through the normal
//! user-mode loader path and lets its output reach the serial between markers.

/// The conformance program built by `cargo xtask conformance` (empty if absent).
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance.exe"));

pub fn run() -> ! {
    if CONFORMANCE_EXE.is_empty() || crate::ntdll_image::NTDLL_BYTES.is_empty() {
        kprintln!("@@TUKOS-PANIC conformance: exe ou ntdll nao embutidos");
        crate::qemu::exit(crate::qemu::ExitCode::Failure);
    }
    let before = crate::mem::frames::free_frame_count();
    kprintln!("@@TUKOS-CONFORMANCE-BEGIN");
    crate::process::spawn_exe_image_with_ntdll(CONFORMANCE_EXE.to_vec());
    // The program is done once its thread exited and its frames were reaped.
    for _ in 0..200_000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            kprintln!("@@TUKOS-CONFORMANCE-END");
            crate::qemu::exit(crate::qemu::ExitCode::Success);
        }
    }
    kprintln!("@@TUKOS-CONFORMANCE-END");
    kprintln!("@@TUKOS-PANIC conformance: o programa nao terminou");
    crate::qemu::exit(crate::qemu::ExitCode::Failure);
}
