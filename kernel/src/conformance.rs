//! Conformance boot mode: runs the embedded MSVC-built `.exe` through the normal
//! user-mode loader path and lets its output reach the serial between markers.

/// The conformance program built by `cargo xtask conformance` (empty if absent).
#[cfg(not(any(feature = "conformance32", feature = "conformance_heap")))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance.exe"));
/// With `conformance32` the program is hello32.exe (imports kernel32), run as exe+kernel32+ntdll.
#[cfg(all(feature = "conformance32", not(feature = "conformance_heap")))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance32.exe"));
/// With `conformance_heap` the program is heap.exe (HeapAlloc/HeapFree, exits 7).
#[cfg(feature = "conformance_heap")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance_heap.exe"));

pub fn run() -> ! {
    if CONFORMANCE_EXE.is_empty()
        || crate::ntdll_image::NTDLL_BYTES.is_empty()
        || (cfg!(feature = "conformance32") && crate::kernel32_image::KERNEL32_BYTES.is_empty())
    {
        kprintln!("@@TUKOS-PANIC conformance: exe ou ntdll nao embutidos");
        crate::qemu::exit(crate::qemu::ExitCode::Failure);
    }
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, core::sync::atomic::Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    kprintln!("@@TUKOS-CONFORMANCE-BEGIN");
    #[cfg(not(feature = "conformance32"))]
    crate::process::spawn_exe_image_with_ntdll(CONFORMANCE_EXE.to_vec());
    #[cfg(feature = "conformance32")]
    crate::process::spawn_exe_image_with_k32(CONFORMANCE_EXE.to_vec());
    // The program is done once its thread exited and its frames were reaped.
    for _ in 0..200_000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            kprintln!("@@TUKOS-CONFORMANCE-END");
            let st =
                crate::syscall::LAST_TERMINATE_STATUS.load(core::sync::atomic::Ordering::SeqCst);
            kprintln!("@@TUKOS-CONFORMANCE-EXIT {}", st as u32 as i32);
            crate::qemu::exit(crate::qemu::ExitCode::Success);
        }
    }
    kprintln!("@@TUKOS-CONFORMANCE-END");
    kprintln!("@@TUKOS-PANIC conformance: o programa nao terminou");
    crate::qemu::exit(crate::qemu::ExitCode::Failure);
}
