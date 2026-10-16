//! Conformance boot mode: runs the embedded MSVC-built `.exe` through the normal
//! user-mode loader path and lets its output reach the serial between markers.

extern crate alloc;

/// The conformance program built by `cargo xtask conformance` (empty if absent).
#[cfg(not(any(
    feature = "conformance32",
    feature = "conformance_heap",
    feature = "conformance_crt"
)))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance.exe"));
/// With `conformance32` the program is hello32.exe (imports kernel32), run as exe+kernel32+ntdll.
#[cfg(all(
    feature = "conformance32",
    not(feature = "conformance_heap"),
    not(feature = "conformance_crt")
))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance32.exe"));
/// With `conformance_crt` the program is the real `/MT` CRT hello.exe (exe+kernel32+ntdll).
#[cfg(all(
    feature = "conformance_crt",
    not(feature = "conformance_echo"),
    not(feature = "conformance_fileio"),
    not(feature = "conformance_spawn"),
    not(feature = "shell"),
    not(feature = "gui")
))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/crt_hello.exe"));
/// With `gui` the program is the GUI demo hello_win.exe (user32/gdi32), run by `spawn_exe_image_with_gui`.
#[cfg(all(
    feature = "gui",
    not(feature = "gui_type"),
    not(feature = "gui_paint"),
    not(feature = "gui_desk")
))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello_win.exe"));
/// With `gui_type` (implies `gui`) the program is the interactive typing window test_type.exe.
#[cfg(feature = "gui_type")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/test_type.exe"));
/// With `gui_paint` (implies `gui`) the program is the mouse paint demo paint_win.exe.
#[cfg(all(
    feature = "gui_paint",
    not(feature = "gui_type"),
    not(feature = "gui_desk")
))]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/paint_win.exe"));
/// With `gui_desk` (implies `gui`) the program is the two-window desktop demo desk_demo.exe.
#[cfg(feature = "gui_desk")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/desk_demo.exe"));
/// With `shell` the program is the `/MT` shell.exe (reads commands from the keyboard).
#[cfg(feature = "shell")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shell.exe"));
/// With `conformance_echo` the program is the `/MT` echo.exe; it runs with the command line "echo teste".
#[cfg(feature = "conformance_echo")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/echo.exe"));
/// With `conformance_fileio` the program is the `/MT` fileio.exe (fopen/fread of initrd `cat.txt`).
#[cfg(feature = "conformance_fileio")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fileio.exe"));
/// With `conformance_spawn` the program is the `/MT` parent.exe (spawns initrd `child.exe`).
#[cfg(feature = "conformance_spawn")]
static CONFORMANCE_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/parent.exe"));
/// With `conformance_heap` the program is heap.exe (HeapAlloc/HeapFree, exits 7).
#[cfg(all(feature = "conformance_heap", not(feature = "conformance_crt")))]
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
    #[cfg(feature = "gui")]
    {
        // Cursor de mouse visivel so no boot GUI (os ktests chamam o spawn direto).
        crate::fbcon::cursor_enable();
        if let Some((w, h, _)) = crate::fbcon::fb_info() {
            crate::fbcon::cursor_show_at(w / 2, h / 2);
        }
    }
    #[cfg(feature = "gui")]
    if crate::process::spawn_exe_image_with_gui(CONFORMANCE_EXE.to_vec(), None).is_err() {
        kprintln!("@@TUKOS-PANIC gui: spawn falhou (DLLs/exe nao embutidos?)");
        crate::qemu::exit(crate::qemu::ExitCode::Failure);
    }
    #[cfg(not(feature = "conformance32"))]
    crate::process::spawn_exe_image_with_ntdll(CONFORMANCE_EXE.to_vec());
    #[cfg(all(
        feature = "conformance32",
        not(feature = "conformance_echo"),
        not(feature = "gui")
    ))]
    crate::process::spawn_exe_image_with_k32(CONFORMANCE_EXE.to_vec());
    #[cfg(feature = "conformance_echo")]
    {
        let cmd: alloc::vec::Vec<u16> = "echo teste".encode_utf16().collect();
        let _ = crate::process::spawn_exe_image_with_k32_cmd(CONFORMANCE_EXE.to_vec(), None, &cmd);
    }
    // The program is done once its thread exited and its frames were reaped.
    // Interactive shell: waits on the human with no bound; everything else is bounded.
    let max_yields: u64 = if cfg!(feature = "shell") {
        u64::MAX
    } else {
        200_000
    };
    for _ in 0..max_yields {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            kprintln!("@@TUKOS-CONFORMANCE-END");
            let st =
                crate::syscall::LAST_TERMINATE_STATUS.load(core::sync::atomic::Ordering::SeqCst);
            kprintln!("@@TUKOS-CONFORMANCE-EXIT {}", st as u32 as i32);
            // GUI demo: keep the painted window on screen for the human (no exit).
            #[cfg(feature = "gui")]
            loop {
                crate::sched::yield_now();
                core::hint::spin_loop();
            }
            #[cfg(not(feature = "gui"))]
            crate::qemu::exit(crate::qemu::ExitCode::Success);
        }
    }
    kprintln!("@@TUKOS-CONFORMANCE-END");
    kprintln!("@@TUKOS-PANIC conformance: o programa nao terminou");
    crate::qemu::exit(crate::qemu::ExitCode::Failure);
}
