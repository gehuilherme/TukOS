//! In-kernel self-tests, built only with the `ktest` feature.

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

type TestFn = fn() -> Result<(), &'static str>;

const TESTS: &[(&str, TestFn)] = &[
    (
        "limine_base_revision_supported",
        limine_base_revision_supported,
    ),
    ("gdt_selectors_loaded", gdt_selectors_loaded),
    ("tss_loaded", tss_loaded),
    ("idt_loaded", idt_loaded),
    ("breakpoint_returns", breakpoint_returns),
    ("fbcon_draws_glyph", fbcon_draws_glyph),
    ("fb_fill_and_read", fb_fill_and_read),
    ("frame_alloc_roundtrip", frame_alloc_roundtrip),
    ("map_translate_unmap", map_translate_unmap),
    ("map_creates_missing_tables", map_creates_missing_tables),
    ("heap_box_and_vec", heap_box_and_vec),
    ("ist_stacks_installed", ist_stacks_installed),
    ("timer_ticks_advance", timer_ticks_advance),
    ("keyboard_poll_decodes", keyboard_poll_decodes),
    ("spawned_thread_runs", spawned_thread_runs),
    ("yield_alternates_threads", yield_alternates_threads),
    ("preemption_runs_both_threads", preemption_runs_both_threads),
    ("rsp0_set_before_user", rsp0_set_before_user),
    (
        "enter_user_returns_after_exit",
        enter_user_returns_after_exit,
    ),
    (
        "syscall_roundtrip_preserves_gs",
        syscall_roundtrip_preserves_gs,
    ),
    (
        "syscall_dispatch_records_mark",
        syscall_dispatch_records_mark,
    ),
    ("ring0_exception_keeps_gs", ring0_exception_keeps_gs),
    (
        "kernel_mapped_in_new_address_space",
        kernel_mapped_in_new_address_space,
    ),
    (
        "higher_half_pml4_fully_reserved",
        higher_half_pml4_fully_reserved,
    ),
    (
        "address_space_drop_frees_user_frames",
        address_space_drop_frees_user_frames,
    ),
    ("user_process_runs_in_ring3", user_process_runs_in_ring3),
    // Must stay LAST: the spawned user processes never exit.
    (
        "user_process_exits_and_is_reaped",
        user_process_exits_and_is_reaped,
    ),
    (
        "single_thread_after_exit_no_fault",
        single_thread_after_exit_no_fault,
    ),
    ("pe_loads_runs_and_exits", pe_loads_runs_and_exits),
    ("load_zero_fills_bss", load_zero_fills_bss),
    (
        "ntwritefile_prints_user_buffer",
        ntwritefile_prints_user_buffer,
    ),
    (
        "ntwritefile_rejects_kernel_pointer",
        ntwritefile_rejects_kernel_pointer,
    ),
    ("ntwritefile_rejects_unmapped", ntwritefile_rejects_unmapped),
    ("nt_terminate_exits_and_reaps", nt_terminate_exits_and_reaps),
    // Plan 14: a Ring-3 fault must terminate the process, not panic the kernel.
    (
        "user_fault_terminates_not_panics",
        user_fault_terminates_not_panics,
    ),
    (
        "user_ud_terminates_not_panics",
        user_ud_terminates_not_panics,
    ),
    ("ntwritefile_fills_iosb", ntwritefile_fills_iosb),
    ("ntwritefile_bad_handle", ntwritefile_bad_handle),
    ("console_read_returns_input", console_read_returns_input),
    ("exe_imports_ntdll_and_runs", exe_imports_ntdll_and_runs),
    (
        "usermode_loader_rejects_bad_iat",
        usermode_loader_rejects_bad_iat,
    ),
    ("peb_ldr_list_well_formed", peb_ldr_list_well_formed),
    ("exe_uses_kernel32_and_runs", exe_uses_kernel32_and_runs),
    ("heap_alloc_write_free", heap_alloc_write_free),
    ("peb_ldr_three_modules", peb_ldr_three_modules),
    (
        "exe_calls_forwarded_kernel32_export",
        exe_calls_forwarded_kernel32_export,
    ),
    ("exit_code_is_reported", exit_code_is_reported),
    ("create_wait_exit", create_wait_exit),
    ("child_fault_reports_status", child_fault_reports_status),
    ("create_process_errors", create_process_errors),
    ("child_sees_command_line", child_sees_command_line),
    ("program_sees_argv", program_sees_argv),
    (
        "loader_rejects_missing_dependency",
        loader_rejects_missing_dependency,
    ),
    (
        "usermode_loader_rejects_bad_iat_dir12",
        usermode_loader_rejects_bad_iat_dir12,
    ),
    ("two_user_processes_preempt", two_user_processes_preempt),
    ("reloc_dir64_crosses_page", reloc_dir64_crosses_page),
    ("stack_placed_above_image", stack_placed_above_image),
    ("user_reads_own_teb", user_reads_own_teb),
    (
        "translate_user_requires_user_bit",
        translate_user_requires_user_bit,
    ),
    (
        "copy_to_user_rejects_readonly_page",
        copy_to_user_rejects_readonly_page,
    ),
    ("translate_user_rejects_huge", translate_user_rejects_huge),
    ("ntdll_embedded_parses", ntdll_embedded_parses),
    ("kernel32_embedded_parses", kernel32_embedded_parses),
    ("kernel32_exports_crt_surface", kernel32_exports_crt_surface),
    ("crt_hello_runs", crt_hello_runs),
    ("program_reads_file", program_reads_file),
    ("program_spawns_child", program_spawns_child),
    ("echo_reads_and_prints", echo_reads_and_prints),
    ("shell_runs_command", shell_runs_command),
    ("shell_unknown_and_help", shell_unknown_and_help),
    ("shell_echo_args", shell_echo_args),
    ("shell_quoted_arg_survives", shell_quoted_arg_survives),
    ("two_modules_no_overlap", two_modules_no_overlap),
    (
        "import_resolve_rejects_bad_iat_slot",
        import_resolve_rejects_bad_iat_slot,
    ),
    (
        "import_resolve_unknown_function",
        import_resolve_unknown_function,
    ),
    (
        "load_pe_rejects_malformed_without_panic",
        load_pe_rejects_malformed_without_panic,
    ),
    (
        "syscall_reads_six_args_reg_and_stack",
        syscall_reads_six_args_reg_and_stack,
    ),
    (
        "stack_args_rejected_when_user_rsp_bad",
        stack_args_rejected_when_user_rsp_bad,
    ),
    ("file_open_read_eof", file_open_read_eof),
    ("file_seek_and_size", file_seek_and_size),
    ("file_open_rejects_bad_paths", file_open_rejects_bad_paths),
    ("file_path_normalization", file_path_normalization),
    (
        "file_syscalls_reject_kernel_thread",
        file_syscalls_reject_kernel_thread,
    ),
    (
        "handle_table_bounds_and_close",
        handle_table_bounds_and_close,
    ),
    ("valloc_basic", valloc_basic),
    ("valloc_rejects_overlap", valloc_rejects_overlap),
    ("valloc_rejects_kernel_range", valloc_rejects_kernel_range),
    // Deve ser o ULTIMO: esgota a tabela de threads (Plan 18 Task 1).
    ("thread_slots_recycle", thread_slots_recycle),
];

static SURVIVOR_RAN: AtomicU64 = AtomicU64::new(0);

fn survivor() {
    SURVIVOR_RAN.fetch_add(1, Ordering::SeqCst);
    crate::sched::exit_current();
}

/// Spawns a faulting user process via `spawn`, then checks: the kernel survives
/// (we are still running), the process is reaped (frames back to baseline) and
/// another thread still runs afterwards.
fn fault_contained(spawn: fn() -> usize) -> Result<(), &'static str> {
    // Let earlier leftovers finish being reaped so `before` is stable.
    for _ in 0..4000 {
        crate::sched::yield_now();
    }
    let before = crate::mem::frames::free_frame_count();
    spawn();
    let mut reaped = false;
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            reaped = true;
            break;
        }
    }
    if !reaped {
        return Err("processo que faltou nao foi terminado/reapeado");
    }
    let n = SURVIVOR_RAN.load(Ordering::SeqCst);
    crate::sched::spawn(survivor);
    for _ in 0..100 {
        if SURVIVOR_RAN.load(Ordering::SeqCst) > n {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("outra thread nao executou apos o fault de usuario")
}

fn user_fault_terminates_not_panics() -> Result<(), &'static str> {
    fault_contained(crate::process::spawn_faulting_pe)
}

fn user_ud_terminates_not_panics() -> Result<(), &'static str> {
    fault_contained(crate::process::spawn_ud_pe)
}

fn ntdll_embedded_parses() -> Result<(), &'static str> {
    let bytes = crate::ntdll_image::NTDLL_BYTES;
    if bytes.is_empty() {
        return Ok(()); // ntdll not embedded on this host (no MSVC): skip.
    }
    let pe = pe::PeFile::parse(bytes).map_err(|_| "ntdll.dll embutido nao e PE valido")?;
    if pe.export_rva_by_name("NtWriteFile").is_none() {
        return Err("ntdll embutido nao exporta NtWriteFile");
    }
    Ok(())
}

fn kernel32_embedded_parses() -> Result<(), &'static str> {
    let bytes = crate::kernel32_image::KERNEL32_BYTES;
    if bytes.is_empty() {
        return Ok(()); // kernel32 not embedded on this host (no MSVC): skip.
    }
    let pe = pe::PeFile::parse(bytes).map_err(|_| "kernel32.dll embutido nao e PE valido")?;
    for name in ["GetStdHandle", "WriteFile", "ExitProcess"] {
        if pe.export_rva_by_name(name).is_none() {
            return Err("kernel32 embutido nao exporta uma das 3 APIs");
        }
    }
    let mut dlls = 0;
    let imports = pe.imports().map_err(|_| "imports ilegiveis")?;
    for dll in imports {
        dlls += 1;
        if !dll.name.eq_ignore_ascii_case("ntdll.dll") {
            return Err("kernel32 importa uma DLL que nao e ntdll.dll");
        }
        let (mut wf, mut tp) = (false, false);
        for t in dll.thunks {
            if let pe::ImportKind::Name { name: n, .. } = t.kind {
                wf |= n == "NtWriteFile";
                tp |= n == "NtTerminateProcess";
            }
        }
        if !(wf && tp) {
            return Err("kernel32 nao importa NtWriteFile+NtTerminateProcess");
        }
    }
    if dlls != 1 {
        return Err("kernel32 deve ter exatamente 1 DLL importada");
    }
    Ok(())
}

/// The 72 kernel32 imports of a `/MT` MSVC `hello.exe` (docs/observations/
/// 2026-10-14-crt-imports-survey.md), plus the extra names Plan 13 exports.
const CRT_SURFACE: &[&str] = &[
    "QueryPerformanceCounter",
    "GetCurrentProcessId",
    "GetCurrentThreadId",
    "GetSystemTimeAsFileTime",
    "InitializeSListHead",
    "RtlCaptureContext",
    "RtlLookupFunctionEntry",
    "RtlVirtualUnwind",
    "IsDebuggerPresent",
    "UnhandledExceptionFilter",
    "SetUnhandledExceptionFilter",
    "GetStartupInfoW",
    "IsProcessorFeaturePresent",
    "GetModuleHandleW",
    "WriteConsoleW",
    "RtlUnwindEx",
    "GetLastError",
    "SetLastError",
    "EnterCriticalSection",
    "LeaveCriticalSection",
    "DeleteCriticalSection",
    "InitializeCriticalSectionAndSpinCount",
    "TlsAlloc",
    "TlsGetValue",
    "TlsSetValue",
    "TlsFree",
    "FreeLibrary",
    "GetProcAddress",
    "LoadLibraryExW",
    "RaiseException",
    "GetStdHandle",
    "WriteFile",
    "GetModuleFileNameW",
    "GetCurrentProcess",
    "ExitProcess",
    "TerminateProcess",
    "GetModuleHandleExW",
    "GetCommandLineA",
    "GetCommandLineW",
    "HeapAlloc",
    "HeapFree",
    "CompareStringW",
    "LCMapStringW",
    "GetFileType",
    "FindClose",
    "FindFirstFileExW",
    "FindNextFileW",
    "IsValidCodePage",
    "GetACP",
    "GetOEMCP",
    "GetCPInfo",
    "MultiByteToWideChar",
    "WideCharToMultiByte",
    "GetEnvironmentStringsW",
    "FreeEnvironmentStringsW",
    "SetEnvironmentVariableW",
    "SetStdHandle",
    "GetStringTypeW",
    "GetProcessHeap",
    "FlushFileBuffers",
    "GetConsoleOutputCP",
    "GetConsoleMode",
    "GetFileSizeEx",
    "SetFilePointerEx",
    "HeapSize",
    "HeapReAlloc",
    "CloseHandle",
    "CreateFileW",
    // extras
    "QueryPerformanceFrequency",
    "GetTickCount",
    "GetTickCount64",
    "GetModuleHandleA",
    "GetModuleFileNameA",
    "LoadLibraryW",
    "InitializeCriticalSection",
    "InitializeCriticalSectionEx",
    "FlsAlloc",
    "FlsGetValue",
    "FlsSetValue",
    "FlsFree",
];

fn kernel32_exports_crt_surface() -> Result<(), &'static str> {
    let bytes = crate::kernel32_image::KERNEL32_BYTES;
    if bytes.is_empty() {
        return Ok(()); // kernel32 not embedded on this host (no MSVC): skip.
    }
    let pe = pe::PeFile::parse(bytes).map_err(|_| "kernel32.dll embutido nao e PE valido")?;
    for name in CRT_SURFACE {
        if pe.export_rva_by_name(name).is_none() {
            crate::kprintln!("kernel32 nao exporta: {}", name);
            return Err("kernel32 nao exporta um nome da superficie do CRT");
        }
    }
    // Still only ntdll is imported.
    let imports = pe.imports().map_err(|_| "imports ilegiveis")?;
    for dll in imports {
        if !dll.name.eq_ignore_ascii_case("ntdll.dll") {
            return Err("kernel32 importa uma DLL que nao e ntdll.dll");
        }
    }
    Ok(())
}

/// A real `/MT` MSVC `hello.exe` (target/conformance/crt/hello.exe; empty if not built).
static CRT_HELLO_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/crt_hello.exe"));

/// Boots the real `/MT` CRT hello through the multi-module loader (exe+kernel32+ntdll) and
/// checks the whole run: raw stdout "Hello, TukOS!\r\n" (15 bytes: CRT text mode turns
/// `\n` into `\r\n`; the conformance harness normalises to 14), exit 0, frames reaped.
fn crt_hello_runs() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() || CRT_HELLO_EXE.is_empty() {
        return Ok(()); // dlls/hello.exe not embedded on this host: skip.
    }
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(CRT_HELLO_EXE.to_vec());
    for _ in 0..40000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            // 0xC000_7001..=0xC000_700A are the user-mode loader's death sentinels.
            if (0xC000_7001..=0xC000_700A).contains(&st) {
                crate::kprintln!("loader status {:#x}", st);
                return Err("crt hello: o loader morreu (import nao resolvido?)");
            }
            if st != 0 {
                crate::kprintln!("crt hello terminou com status {:#x}", st);
                return Err("crt hello: exit code != 0");
            }
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 15
                || !crate::syscall::last_write_is(b"Hello, TukOS!\r\n")
            {
                return Err("crt hello: stdout diferente de Hello, TukOS!");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("crt hello nao terminou/recolheu os frames")
}

/// The `/MT` echo.exe (target/conformance/crt/echo.exe; empty if not built).
static ECHO_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/echo.exe"));

/// End to end: command line "echo teste" -> GetCommandLine -> CRT argv -> printf -> WriteFile.
fn echo_reads_and_prints() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() || ECHO_EXE.is_empty() {
        return Ok(()); // dlls/echo.exe not embedded on this host: skip.
    }
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::console_log_reset();
    let before = crate::mem::frames::free_frame_count();
    let cmd: alloc::vec::Vec<u16> = "echo teste".encode_utf16().collect();
    crate::process::spawn_exe_image_with_k32_cmd(ECHO_EXE.to_vec(), None, &cmd)
        .map_err(|_| "echo: spawn falhou")?;
    for _ in 0..40000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != 0 {
                crate::kprintln!("echo terminou com status {:#x}", st);
                return Err("echo: exit code != 0 (loader falhou)");
            }
            // printf writes "teste" then CRLF (text mode); the console log holds both.
            crate::syscall::console_log_find(b"teste\r\n", 0)
                .ok_or("echo: stdout diferente do esperado (teste)")?;
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("echo nao terminou/recolheu os frames")
}

/// The `/MT` argv.exe (target/conformance/crt/argv.exe; empty if not built).
static ARGV_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/argv.exe"));

/// End to end: the kernel hands "x alpha beta" to the child's PEB, kernel32's
/// `GetCommandLineA` returns it and the MSVC CRT builds argv from it.
fn program_sees_argv() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() || ARGV_EXE.is_empty() {
        return Ok(()); // dlls/argv.exe not embedded on this host: skip.
    }
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::console_log_reset();
    let before = crate::mem::frames::free_frame_count();
    let cmd: alloc::vec::Vec<u16> = "x alpha beta".encode_utf16().collect();
    crate::process::spawn_exe_image_with_k32_cmd(ARGV_EXE.to_vec(), None, &cmd)
        .map_err(|_| "argv: spawn falhou")?;
    for _ in 0..40000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != 0 {
                crate::kprintln!("argv terminou com status {:#x}", st);
                return Err("argv: exit code != 0");
            }
            // CRT text mode: LF becomes CRLF; the console log holds the whole output.
            crate::syscall::console_log_find(b"3 x alpha beta\r\n", 0)
                .ok_or("argv: stdout diferente de 3 x alpha beta")?;
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("argv nao terminou/recolheu os frames")
}

/// The `/MT` shell.exe (target/conformance/crt/shell.exe; empty if not built).
static SHELL_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shell.exe"));

/// Scripted shell session: the keyboard is pre-fed "hello\nexit\n"; the Ring-3 shell prints
/// its prompt, reads "hello", CreateProcess(hello.exe) FROM RING 3, waits, prints the next
/// prompt, reads "exit" and returns. Asserts the console shows prompt -> Hello -> prompt,
/// and that both processes were reaped (frames back to baseline).
fn shell_runs_command() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls()
        || SHELL_EXE.is_empty()
        || crate::fs::lookup("hello.exe").is_none_or(<[u8]>::is_empty)
    {
        return Ok(()); // dlls/shell.exe/hello.exe not embedded on this host: skip.
    }
    while crate::kbd::try_pop_input().is_some() {}
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::console_log_reset();
    crate::kbd::test_push_input(b"hello\nexit\n");
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(SHELL_EXE.to_vec());
    for _ in 0..200_000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX && crate::mem::frames::free_frame_count() == before {
            if (0xC000_7001..=0xC000_700A).contains(&st) || st != 0 {
                crate::kprintln!("shell terminou com status {:#x}", st);
                return Err("shell: exit code != 0 (loader ou CreateProcess falhou)");
            }
            let p1 =
                crate::syscall::console_log_find(b"tukos> ", 0).ok_or("shell: prompt ausente")?;
            let h = crate::syscall::console_log_find(b"Hello, TukOS!\r\n", p1)
                .ok_or("shell: saida do hello.exe ausente")?;
            // A second prompt after the child's output proves the shell waited and looped.
            crate::syscall::console_log_find(b"tukos> ", h).ok_or("shell: sem 2o prompt")?;
            if crate::syscall::console_log_find(b"comando nao encontrado", 0).is_some() {
                return Err("shell: CreateProcess(hello.exe) falhou");
            }
            return Ok(());
        }
    }
    Err("shell nao terminou/recolheu os frames")
}

/// Scripted shell session with builtins: keyboard pre-fed help, naoexiste, exit. Asserts the help listing
/// (hello/echo/exit), the "comando nao encontrado: naoexiste" line, and that the shell exited
/// cleanly (status 0, frames back to baseline).
fn shell_unknown_and_help() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() || SHELL_EXE.is_empty() {
        return Ok(()); // dlls/shell.exe not embedded on this host: skip.
    }
    while crate::kbd::try_pop_input().is_some() {}
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::console_log_reset();
    crate::kbd::test_push_input(b"help\nnaoexiste\nexit\n");
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(SHELL_EXE.to_vec());
    for _ in 0..200_000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX && crate::mem::frames::free_frame_count() == before {
            if st != 0 {
                return Err("shell: exit code != 0");
            }
            let p1 =
                crate::syscall::console_log_find(b"tukos> ", 0).ok_or("shell: prompt ausente")?;
            let h =
                crate::syscall::console_log_find(b"hello", p1).ok_or("shell: help sem 'hello'")?;
            crate::syscall::console_log_find(b"echo", p1).ok_or("shell: help sem 'echo'")?;
            crate::syscall::console_log_find(b"exit", h).ok_or("shell: help sem 'exit'")?;
            crate::syscall::console_log_find(b"comando nao encontrado: naoexiste\r\n", p1)
                .ok_or("shell: mensagem de comando desconhecido ausente")?;
            return Ok(());
        }
    }
    Err("shell nao terminou/recolheu os frames")
}

/// Scripted shell session with arguments: pre-fed "echo teste" + exit. The shell passes the
/// whole line as the child's command line, so echo.exe prints "teste" (and not the old
/// stdin-reading "voce digitou").
fn shell_echo_args() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls()
        || SHELL_EXE.is_empty()
        || crate::fs::lookup("echo.exe").is_none_or(<[u8]>::is_empty)
    {
        return Ok(()); // dlls/shell.exe/echo.exe not embedded on this host: skip.
    }
    while crate::kbd::try_pop_input().is_some() {}
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::console_log_reset();
    crate::kbd::test_push_input(b"echo teste\nexit\n");
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(SHELL_EXE.to_vec());
    for _ in 0..200_000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX && crate::mem::frames::free_frame_count() == before {
            if st != 0 {
                crate::kprintln!("shell terminou com status {:#x}", st);
                return Err("shell: exit code != 0");
            }
            let p1 =
                crate::syscall::console_log_find(b"tukos> ", 0).ok_or("shell: prompt ausente")?;
            let t = crate::syscall::console_log_find(b"teste\r\n", p1)
                .ok_or("shell: echo nao imprimiu 'teste' (argumentos nao chegaram)")?;
            if crate::syscall::console_log_find(b"voce digitou", 0).is_some() {
                return Err("shell: echo antigo (voce digitou) ainda em uso");
            }
            crate::syscall::console_log_find(b"tukos> ", t).ok_or("shell: sem 2o prompt")?;
            return Ok(());
        }
    }
    Err("shell nao terminou/recolheu os frames")
}

/// Regression (fix-gp-shell-input): the shell input `help "teste"` (a builtin with a
/// quoted argument — the exact line that #GP'd the kernel on real hardware) must never
/// crash the kernel. The shell handles the quoted builtin and exits cleanly. The
/// iret-frame repair that prevents the #GP is unit-tested in
/// `arch_x86_64::trap::sanitize_return_frame`; here we check the end-to-end shell run.
fn shell_quoted_arg_survives() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() || SHELL_EXE.is_empty() {
        return Ok(()); // dlls/shell.exe not embedded on this host: skip.
    }
    while crate::kbd::try_pop_input().is_some() {}
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::console_log_reset();
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(SHELL_EXE.to_vec());
    crate::kbd::test_push_input(b"help \"teste\"\nexit\n");
    for _ in 0..200_000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX && crate::mem::frames::free_frame_count() == before {
            if st != 0 {
                crate::kprintln!("shell terminou com status {:#x}", st);
                return Err("shell: exit code != 0");
            }
            // The quoted builtin must have been handled (help listing present).
            crate::syscall::console_log_find(b"comandos:", 0)
                .ok_or("shell: help nao listou os comandos")?;
            return Ok(());
        }
    }
    Err("shell nao terminou/recolheu os frames")
}

const EXE_BASE: u64 = 0x40_0000;
const NTDLL_BASE: u64 = 0x1000_0000;

fn two_modules_no_overlap() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    let nt = crate::ntdll_image::NTDLL_BYTES;
    if nt.is_empty() {
        return Ok(());
    }
    let exe = crate::process::build_import_pe("NtWriteFile", true);
    let space = AddressSpace::new_user();
    crate::loader::load_pe(&space, &exe, EXE_BASE).map_err(|_| "load exe falhou")?;
    crate::loader::load_pe(&space, nt, NTDLL_BASE).map_err(|_| "load ntdll falhou")?;
    let exe_top = crate::loader::image_top(&exe, EXE_BASE).map_err(|_| "top exe")?;
    let nt_top = crate::loader::image_top(nt, NTDLL_BASE).map_err(|_| "top ntdll")?;
    if exe_top > NTDLL_BASE || nt_top <= NTDLL_BASE {
        return Err("faixas de VA dos modulos se sobrepoem");
    }
    crate::loader::resolve_imports(&space, &exe, EXE_BASE, nt, NTDLL_BASE)
        .map_err(|_| "resolve falhou")?;
    // IAT slot (RVA 0x1060) must now hold NTDLL_BASE + export RVA.
    let rva = pe::PeFile::parse(nt)
        .map_err(|_| "parse ntdll")?
        .export_rva_by_name("NtWriteFile")
        .ok_or("sem NtWriteFile")?;
    let phys = space
        .translate(EXE_BASE + 0x1000)
        .ok_or("IAT nao mapeada")?;
    let p = (crate::mem::frames::phys_to_virt(phys) + 0x60) as *const u64;
    if unsafe { core::ptr::read_volatile(p) } != NTDLL_BASE + rva as u64 {
        return Err("IAT nao recebeu o endereco resolvido");
    }
    Ok(())
}

fn import_resolve_rejects_bad_iat_slot() -> Result<(), &'static str> {
    use crate::loader::LoadError;
    use crate::mem::space::AddressSpace;
    let nt = crate::ntdll_image::NTDLL_BYTES;
    if nt.is_empty() {
        return Ok(());
    }
    // IAT lives in a read+execute (non-writable) section.
    let exe = crate::process::build_import_pe("NtWriteFile", false);
    let space = AddressSpace::new_user();
    crate::loader::load_pe(&space, &exe, EXE_BASE).map_err(|_| "load exe falhou")?;
    crate::loader::load_pe(&space, nt, NTDLL_BASE).map_err(|_| "load ntdll falhou")?;
    match crate::loader::resolve_imports(&space, &exe, EXE_BASE, nt, NTDLL_BASE) {
        Err(LoadError::BadIatSlot) => Ok(()),
        _ => Err("slot de IAT nao gravavel deveria dar BadIatSlot"),
    }
}

fn import_resolve_unknown_function() -> Result<(), &'static str> {
    use crate::loader::LoadError;
    use crate::mem::space::AddressSpace;
    let nt = crate::ntdll_image::NTDLL_BYTES;
    if nt.is_empty() {
        return Ok(());
    }
    let exe = crate::process::build_import_pe("NoSuchFunction", true);
    let space = AddressSpace::new_user();
    crate::loader::load_pe(&space, &exe, EXE_BASE).map_err(|_| "load exe falhou")?;
    crate::loader::load_pe(&space, nt, NTDLL_BASE).map_err(|_| "load ntdll falhou")?;
    match crate::loader::resolve_imports(&space, &exe, EXE_BASE, nt, NTDLL_BASE) {
        Err(LoadError::UnresolvedImport) => Ok(()),
        _ => Err("funcao inexistente deveria dar UnresolvedImport"),
    }
}

const CAT: &[u8] = b"arquivo de teste do TukOS\n";

fn utf16(s: &str) -> alloc::vec::Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn read_all(t: &mut crate::object::HandleTable, h: u64, want: usize) -> alloc::vec::Vec<u8> {
    let data = t.file_peek(h, want).unwrap_or(&[]);
    t.file_commit(h, data.len());
    data.to_vec()
}

/// A table laid out like a real process's: console = 4, console input = 8.
fn process_like_table() -> crate::object::HandleTable {
    use crate::object::{HandleTable, Object};
    let mut t = HandleTable::new();
    assert_eq!(t.insert(Object::Console), crate::object::CONSOLE_HANDLE);
    assert_eq!(
        t.insert(Object::ConsoleInput),
        crate::object::CONSOLE_INPUT_HANDLE
    );
    t
}

fn file_open_read_eof() -> Result<(), &'static str> {
    let mut t = process_like_table();
    let h1 = crate::syscall::open_file_in(&mut t, &utf16("C:\\cat.txt"), 0x8000_0000)
        .map_err(|_| "open C:\\cat.txt falhou")?;
    if h1 == crate::object::CONSOLE_HANDLE || h1 == crate::object::CONSOLE_INPUT_HANDLE {
        return Err("handle colidiu com console");
    }
    let h2 = crate::syscall::open_file_in(&mut t, &utf16("c:/CAT.TXT"), 0x8000_0000)
        .map_err(|_| "segundo open falhou")?;
    if h1 == h2 {
        return Err("dois opens devolveram o mesmo handle");
    }
    // Partial read on h1 must not move h2 (per-handle offset).
    if read_all(&mut t, h1, 7) != CAT[..7] {
        return Err("leitura parcial errada");
    }
    if read_all(&mut t, h2, 4096) != CAT {
        return Err("leitura total errada (offset nao e por handle?)");
    }
    if !read_all(&mut t, h2, 4096).is_empty() {
        return Err("segunda leitura deveria dar EOF(0)");
    }
    if read_all(&mut t, h1, 4096) != CAT[7..] {
        return Err("resto da leitura parcial errado");
    }
    if !t.close(h1) || !t.close(h2) {
        return Err("close de arquivo falhou");
    }
    if t.get(h1).is_some() || t.file_peek(h1, 1).is_some() {
        return Err("handle de arquivo fechado ainda resolve");
    }
    // Slot reuse: no leak.
    let h3 = crate::syscall::open_file_in(&mut t, &utf16("\\??\\C:\\cat.txt"), 0x8000_0000)
        .map_err(|_| "open NT-path falhou")?;
    if h3 != h1 {
        return Err("slot do handle fechado nao foi reutilizado");
    }
    Ok(())
}

fn file_seek_and_size() -> Result<(), &'static str> {
    let mut t = process_like_table();
    let h = crate::syscall::open_file_in(&mut t, &utf16("C:\\cat.txt"), 0x8000_0000)
        .map_err(|_| "open falhou")?;
    if t.file_size(h) != Some(CAT.len() as u64) {
        return Err("tamanho errado");
    }
    let _ = read_all(&mut t, h, 4096);
    if t.file_seek(h, 0, 0) != Ok(0) {
        return Err("seek BEGIN falhou");
    }
    if read_all(&mut t, h, 4096) != CAT {
        return Err("re-leitura apos seek errada");
    }
    if t.file_seek(h, -5, 2) != Ok(CAT.len() as u64 - 5) {
        return Err("seek END-5 errado");
    }
    if read_all(&mut t, h, 4096) != CAT[CAT.len() - 5..] {
        return Err("leitura apos seek END errada");
    }
    if t.file_seek(h, 3, 0) != Ok(3) || t.file_seek(h, 2, 1) != Ok(5) {
        return Err("seek CURRENT errado");
    }
    if t.file_seek(h, 1000, 0) != Ok(CAT.len() as u64) {
        return Err("seek alem do fim deveria fixar em len");
    }
    if !read_all(&mut t, h, 10).is_empty() {
        return Err("leitura apos o fim deveria ser 0");
    }
    if t.file_seek(h, -1, 0).is_ok() || t.file_seek(h, i64::MIN, 1).is_ok() {
        return Err("seek negativo deveria falhar");
    }
    if t.file_seek(h, 0, 7).is_ok() {
        return Err("metodo invalido deveria falhar");
    }
    if t.file_seek(h, i64::MAX, 1).is_err() || t.file_seek(h, i64::MAX, 2).is_err() {
        return Err("overflow deveria fixar, nao falhar/panicar");
    }
    if t.file_seek(crate::object::CONSOLE_HANDLE, 0, 0).is_ok() || t.file_size(8).is_some() {
        return Err("seek/size em console deveria falhar");
    }
    Ok(())
}

fn file_open_rejects_bad_paths() -> Result<(), &'static str> {
    use crate::syscall::open_file_in;
    const NF: u64 = 0xC000_0034;
    let mut t = process_like_table();
    let ro = 0x8000_0000;
    if open_file_in(&mut t, &utf16("C:\\nao_existe.txt"), ro) != Err(NF) {
        return Err("ausente deveria dar OBJECT_NAME_NOT_FOUND");
    }
    for bad in [
        "C:\\..\\cat.txt",
        "..\\cat.txt",
        "C:\\x\\..\\cat.txt",
        "C:/../cat.txt",
    ] {
        if open_file_in(&mut t, &utf16(bad), ro) != Err(NF) {
            return Err("travessia .. deveria ser rejeitada");
        }
    }
    if open_file_in(&mut t, &utf16("D:\\cat.txt"), ro) != Err(NF) {
        return Err("outro drive deveria dar not found");
    }
    if open_file_in(&mut t, &utf16("C:\\caf\u{e9}.txt"), ro) != Err(NF) {
        return Err("nao-ASCII deveria dar not found");
    }
    if open_file_in(&mut t, &[0x00, 0xD8, 0x41, 0x00], ro) != Err(NF) {
        return Err("surrogate solto deveria dar not found");
    }
    if open_file_in(&mut t, &utf16("C:\\cat.txt\0"), ro) != Err(NF) {
        return Err("NUL embutido deveria dar not found");
    }
    if open_file_in(&mut t, b"c", ro) != Err(0xC000_000D) {
        return Err("tamanho impar deveria dar INVALID_PARAMETER");
    }
    let long = utf16(&"a".repeat(300));
    if open_file_in(&mut t, &long, ro) != Err(0xC000_0106) {
        return Err("caminho longo deveria dar NAME_TOO_LONG");
    }
    // Read-only filesystem: write access is refused.
    if open_file_in(&mut t, &utf16("C:\\cat.txt"), 0x4000_0000) != Err(0xC000_0022) {
        return Err("acesso de escrita deveria dar ACCESS_DENIED");
    }
    if t.get(12).is_some() {
        return Err("falhas nao deveriam alocar handles");
    }
    Ok(())
}

fn file_path_normalization() -> Result<(), &'static str> {
    use crate::fs::lookup;
    let want = Some(CAT);
    for p in [
        "cat.txt",
        "C:\\cat.txt",
        "c:/cat.txt",
        "\\??\\C:\\Cat.TXT",
        "C:\\.\\cat.txt",
        "C:\\\\cat.txt",
    ] {
        if lookup(p) != want {
            return Err("caminho equivalente nao resolveu");
        }
    }
    for p in [
        "",
        "C:\\",
        "C:\\..",
        "..",
        "c:\\cat.txt\\x",
        "cat.txt\\..\\cat.txt",
    ] {
        if lookup(p).is_some() {
            return Err("caminho invalido resolveu");
        }
    }
    Ok(())
}

fn file_syscalls_reject_kernel_thread() -> Result<(), &'static str> {
    use crate::syscall::{NT_OPEN_FILE, NT_QUERY_FILE_SIZE, NT_SET_FILE_POINTER, dispatch};
    // A kernel thread has no handle table: nothing may panic or succeed.
    if dispatch(NT_OPEN_FILE, 0, 0, 0, 0x8000_0000, 0) == 0 {
        return Err("open sem processo deveria falhar");
    }
    if dispatch(NT_SET_FILE_POINTER, 12, 0, 0, 0, 0) == 0 {
        return Err("seek sem processo deveria falhar");
    }
    if dispatch(NT_QUERY_FILE_SIZE, 12, 0, 0, 0, 0) == 0 {
        return Err("size sem processo deveria falhar");
    }
    Ok(())
}

fn handle_table_bounds_and_close() -> Result<(), &'static str> {
    use crate::object::{HandleTable, Object};
    let mut t = HandleTable::new();
    if t.get(0).is_some() {
        return Err("handle 0 deveria ser invalido");
    }
    if t.get(0x1000).is_some() {
        return Err("handle fora da faixa deveria ser invalido");
    }
    let h = t.insert(Object::Console);
    if !matches!(t.get(h), Some(Object::Console)) {
        return Err("handle valido nao resolveu");
    }
    if !t.close(h) {
        return Err("close de handle valido falhou");
    }
    if t.get(h).is_some() {
        return Err("handle fechado ainda resolve");
    }
    if t.close(h) {
        return Err("close duplo deveria falhar");
    }
    Ok(())
}

fn syscall_reads_six_args_reg_and_stack() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    crate::process::spawn_sum6_pe(); // SYS_SUM6 with args 1..=6 (4 reg + 2 stack)
    for _ in 0..8000 {
        crate::sched::yield_now();
        let v = crate::syscall::LAST_MARK.load(Ordering::SeqCst);
        if v != 0 {
            return if v == 21 {
                Ok(())
            } else {
                Err("soma de 6 args (4 reg + 2 pilha) incorreta")
            };
        }
    }
    Err("SYS_SUM6 nao executou")
}

fn stack_args_rejected_when_user_rsp_bad() -> Result<(), &'static str> {
    if crate::syscall::stack_arg(0xffff_ff80_0000_0000, 0).is_some() {
        return Err("stack_arg aceitou rsp de kernel");
    }
    if crate::syscall::stack_arg(0x0000_0000_7777_0000, 0).is_some() {
        return Err("stack_arg aceitou rsp nao mapeado");
    }
    Ok(())
}

fn translate_user_requires_user_bit() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    use mm::paging::PageFlags;
    let space = AddressSpace::new_user();
    let user_frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    let kern_frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    unsafe {
        space
            .map(
                0x10_0000,
                user_frame,
                PageFlags::USER.union(PageFlags::WRITABLE),
            )
            .map_err(|_| "map user")?;
        // A lower-half page WITHOUT the USER bit (kernel-only): must be refused.
        space
            .map(0x20_0000, kern_frame, PageFlags::WRITABLE)
            .map_err(|_| "map kern")?;
    }
    if crate::mem::paging::translate_user_in(space.root(), 0x10_0000).is_none() {
        return Err("pagina USER deveria ser aceita");
    }
    if crate::mem::paging::translate_user_in(space.root(), 0x20_0000).is_some() {
        return Err("pagina sem USER foi aceita (vazamento de kernel)");
    }
    Ok(())
}

fn copy_to_user_rejects_readonly_page() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    use mm::paging::PageFlags;
    let space = AddressSpace::new_user();
    let ro = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    let rw = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    unsafe {
        space
            .map(0x10_0000, ro, PageFlags::USER)
            .map_err(|_| "map ro")?;
        space
            .map(0x20_0000, rw, PageFlags::USER.union(PageFlags::WRITABLE))
            .map_err(|_| "map rw")?;
    }
    if crate::mem::paging::translate_user_writable_in(space.root(), 0x10_0000).is_some() {
        return Err("pagina RO deveria ser rejeitada para escrita");
    }
    if crate::mem::paging::translate_user_writable_in(space.root(), 0x20_0000).is_none() {
        return Err("pagina RW deveria ser aceita para escrita");
    }
    Ok(())
}

fn translate_user_rejects_huge() -> Result<(), &'static str> {
    crate::mem::paging::test_huge_rejected()
}

fn load_pe_rejects_malformed_without_panic() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    let space = AddressSpace::new_user();
    // Truncated / bad image: must return Err, not panic.
    if crate::loader::load_pe(&space, &[0u8; 8], 0x40_0000).is_ok() {
        return Err("load_pe aceitou imagem malformada");
    }
    Ok(())
}

fn user_reads_own_teb() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    // The test PE reads gs:[0x30] (NtTib.Self) and passes it to SYS_WRITE_MARK;
    // the kernel records it. It must equal the thread's TEB VA.
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let teb_va = crate::process::spawn_teb_probe_pe();
    for _ in 0..8000 {
        crate::sched::yield_now();
        let m = crate::syscall::LAST_MARK.load(Ordering::SeqCst);
        if m != 0 {
            return if m == teb_va {
                Ok(())
            } else {
                Err("gs:[0x30] (TEB Self) != TEB VA da thread")
            };
        }
    }
    Err("probe de TEB nao executou")
}

fn ist_stacks_installed() -> Result<(), &'static str> {
    use arch_x86_64::{gdt, idt};
    let tops = gdt::ist_tops();
    if tops.iter().any(|&t| t == 0 || t % 16 != 0) {
        return Err("topo de pilha IST nulo ou desalinhado");
    }
    if tops[0] == tops[1] || tops[1] == tops[2] || tops[0] == tops[2] {
        return Err("pilhas IST compartilhadas");
    }
    if idt::entry(8).ist != gdt::IST_DOUBLE_FAULT
        || idt::entry(2).ist != gdt::IST_NMI
        || idt::entry(18).ist != gdt::IST_MACHINE_CHECK
    {
        return Err("vetores 8/2/18 nao apontam para a IST correta");
    }
    Ok(())
}

fn heap_box_and_vec() -> Result<(), &'static str> {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    let b = Box::new(0xDEAD_BEEF_u64);
    if *b != 0xDEAD_BEEF {
        return Err("Box nao preservou o valor");
    }
    let mut v: Vec<u64> = Vec::new();
    for i in 0..1000 {
        v.push(i);
    }
    if v.iter().sum::<u64>() != (0..1000).sum() {
        return Err("Vec corrompeu dados ao crescer");
    }
    Ok(())
}

fn map_translate_unmap() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let phys = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    // A free slot in the non-canonical-safe test window well above the kernel heap.
    let virt = 0xffff_f000_0000_0000u64;
    let flags = PageFlags::PRESENT
        .union(PageFlags::WRITABLE)
        .union(PageFlags::NO_EXECUTE);
    unsafe { crate::mem::paging::map_page(virt, phys, flags).map_err(|_| "map falhou")? };
    if crate::mem::paging::translate(virt) != Some(phys) {
        return Err("translate nao devolveu o frame mapeado");
    }
    unsafe {
        core::ptr::write_volatile(virt as *mut u64, 0x1122_3344_5566_7788);
        if core::ptr::read_volatile(virt as *const u64) != 0x1122_3344_5566_7788 {
            return Err("pagina mapeada nao e gravavel");
        }
    }
    unsafe { crate::mem::paging::unmap_page(virt).map_err(|_| "unmap falhou")? };
    if crate::mem::paging::translate(virt).is_some() {
        return Err("translate ainda resolve apos unmap");
    }
    crate::mem::frames::free_frame(phys);
    Ok(())
}

fn map_creates_missing_tables() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let before = crate::mem::frames::free_frame_count();
    let phys = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    // An address whose P3/P2/P1 tables certainly do not exist yet.
    let virt = 0xffff_e800_0000_0000u64;
    unsafe {
        crate::mem::paging::map_page(virt, phys, PageFlags::PRESENT.union(PageFlags::WRITABLE))
            .map_err(|_| "map falhou ao criar tabelas")?;
    }
    if crate::mem::paging::translate(virt) != Some(phys) {
        return Err("tabelas intermediarias nao foram criadas corretamente");
    }
    unsafe { crate::mem::paging::unmap_page(virt).map_err(|_| "unmap falhou")? };
    crate::mem::frames::free_frame(phys);
    // map consumed frames for the intermediate tables; we don't require them back,
    // but the count must not have grown.
    if crate::mem::frames::free_frame_count() > before {
        return Err("contagem de frames livres cresceu inesperadamente");
    }
    Ok(())
}

fn frame_alloc_roundtrip() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    if before == 0 {
        return Err("nenhum frame livre apos init");
    }
    let hhdm = crate::mem::frames::hhdm_offset();
    let f1 = crate::mem::frames::alloc_frame().ok_or("alloc_frame devolveu None")?;
    let f2 = crate::mem::frames::alloc_frame().ok_or("alloc_frame devolveu None")?;
    if f1 == f2 {
        return Err("alloc_frame devolveu o mesmo frame duas vezes");
    }
    if f1 % crate::mem::frames::FRAME_SIZE != 0 || f2 % crate::mem::frames::FRAME_SIZE != 0 {
        return Err("frame nao alinhado a 4096");
    }
    // The frame must be writable through the HHDM.
    let ptr = (f1 + hhdm) as *mut u64;
    unsafe {
        core::ptr::write_volatile(ptr, 0xA5A5_A5A5_DEAD_BEEF);
        if core::ptr::read_volatile(ptr) != 0xA5A5_A5A5_DEAD_BEEF {
            return Err("frame nao e gravavel via HHDM");
        }
    }
    crate::mem::frames::free_frame(f1);
    crate::mem::frames::free_frame(f2);
    if crate::mem::frames::free_frame_count() != before {
        return Err("contagem de frames livres nao voltou ao valor inicial");
    }
    Ok(())
}

fn gdt_selectors_loaded() -> Result<(), &'static str> {
    use arch_x86_64::{cpu, gdt};
    if cpu::read_cs() != gdt::KERNEL_CS {
        return Err("CS diferente de KERNEL_CS");
    }
    if cpu::read_ss() != gdt::KERNEL_DS {
        return Err("SS diferente de KERNEL_DS");
    }
    Ok(())
}

fn tss_loaded() -> Result<(), &'static str> {
    if arch_x86_64::cpu::str() == arch_x86_64::gdt::TSS_SEL {
        Ok(())
    } else {
        Err("TR diferente de TSS_SEL")
    }
}

fn idt_loaded() -> Result<(), &'static str> {
    let p = arch_x86_64::cpu::sidt();
    let (limit, base) = (p.limit, p.base);
    if limit != 256 * 16 - 1 {
        return Err("limite da IDT incorreto");
    }
    if base != arch_x86_64::idt::table_address() {
        return Err("base da IDT nao aponta para a tabela do kernel");
    }
    Ok(())
}

fn breakpoint_returns() -> Result<(), &'static str> {
    let before = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    arch_x86_64::cpu::int3();
    let after = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    if after == before + 1 {
        Ok(())
    } else {
        Err("handler de breakpoint nao contou a excecao")
    }
}

fn ring0_exception_keeps_gs() -> Result<(), &'static str> {
    // A Ring 0 exception (int3) must NOT swapgs: the kernel GS base must be
    // unchanged across it. (The conditional swapgs only fires for RPL 3.)
    let before = arch_x86_64::percpu::gs_base();
    arch_x86_64::cpu::int3();
    let after = arch_x86_64::percpu::gs_base();
    if before != after {
        return Err("excecao em Ring 0 trocou o GS indevidamente");
    }
    Ok(())
}

fn limine_base_revision_supported() -> Result<(), &'static str> {
    if crate::BASE_REVISION.is_supported() {
        Ok(())
    } else {
        Err("revisao base nao suportada")
    }
}

pub fn run_all() -> ! {
    let (mut passed, mut failed) = (0u32, 0u32);
    for (name, test) in TESTS {
        match test() {
            Ok(()) => {
                kprintln!("@@TUKOS-KTEST-PASS {name}");
                passed += 1;
            }
            Err(why) => {
                kprintln!("@@TUKOS-KTEST-FAIL {name} {why}");
                failed += 1;
            }
        }
    }
    kprintln!("@@TUKOS-KTEST-DONE passed={passed} failed={failed}");
    crate::qemu::exit(if failed == 0 {
        crate::qemu::ExitCode::Success
    } else {
        crate::qemu::ExitCode::Failure
    })
}

fn fbcon_draws_glyph() -> Result<(), &'static str> {
    let mut guard = crate::fbcon::FBCON.lock();
    let con = guard.as_mut().ok_or("sem framebuffer")?;
    let (w, h) = (
        crate::fbcon::FbCon::cell_width(),
        crate::fbcon::FbCon::cell_height(),
    );
    let cell_lit = |con: &crate::fbcon::FbCon| (0..h).any(|y| (0..w).any(|x| con.pixel(x, y) != 0));

    con.clear();
    if cell_lit(con) {
        return Err("celula deveria estar apagada apos clear");
    }
    con.write_str("#");
    if !cell_lit(con) {
        return Err("glifo nao foi desenhado");
    }
    con.clear();
    Ok(())
}

fn fb_fill_and_read() -> Result<(), &'static str> {
    use crate::fbcon::{fb_info, fill_rect, pixel, text_out};
    let (w, h, bpp) = fb_info().ok_or("sem framebuffer")?;
    if bpp != 32 || w < 64 || h < 64 {
        return Err("fb_info inesperado");
    }
    let (a, b) = (0x0011_2233, 0x00FF_0000);
    let keep_a = |pts: &[(u32, u32)]| pts.iter().all(|&(x, y)| pixel(x, y) == Some(a));
    fill_rect(0, 0, w, h, a);
    // Basic fill + exact edges.
    fill_rect(10, 10, 5, 5, b);
    if pixel(10, 10) != Some(b) || pixel(14, 14) != Some(b) {
        return Err("rect nao preenchido");
    }
    if !keep_a(&[(15, 15), (9, 9), (15, 10), (10, 15)]) {
        return Err("rect vazou para fora");
    }
    fill_rect(10, 10, 5, 5, a);
    // Off-screen origins must write nothing at all (no wrap into other rows).
    fill_rect(w, 0, 10, 10, b);
    fill_rect(w + 5, 3, 1000, 1000, b);
    fill_rect(0, h, 10, 10, b);
    fill_rect(5, h + 7, u32::MAX, u32::MAX, b);
    fill_rect(u32::MAX, u32::MAX, u32::MAX, u32::MAX, b);
    if !keep_a(&[
        (0, 0),
        (5, 1),
        (w - 1, 0),
        (0, 1),
        (w - 1, 3),
        (0, h - 1),
        (5, h - 1),
    ]) {
        return Err("fill fora da tela corrompeu pixels dentro");
    }
    // Oversized fill at the far corner sets only the 2x2 in-bounds region.
    fill_rect(w - 2, h - 2, 100, 100, b);
    let in2 = [
        (w - 2, h - 2),
        (w - 1, h - 2),
        (w - 2, h - 1),
        (w - 1, h - 1),
    ];
    if !in2.iter().all(|&(x, y)| pixel(x, y) == Some(b)) {
        return Err("2x2 do canto nao preenchido");
    }
    if !keep_a(&[
        (w - 3, h - 3),
        (w - 3, h - 1),
        (w - 1, h - 3),
        (0, h - 1),
        (0, 0),
    ]) {
        return Err("fill no canto vazou");
    }
    // Huge rect from origin fills everything without faulting.
    fill_rect(0, 0, u32::MAX, u32::MAX, b);
    if pixel(0, 0) != Some(b) || pixel(w - 1, h - 1) != Some(b) {
        return Err("fill gigante nao cobriu a tela");
    }
    if pixel(w, 0).is_some() || pixel(0, h).is_some() {
        return Err("pixel fora do fb legivel");
    }
    // Text: only the glyph box may change.
    fill_rect(0, 0, w, h, a);
    let (cw, ch) = (
        crate::fbcon::FbCon::cell_width() as u32,
        crate::fbcon::FbCon::cell_height() as u32,
    );
    text_out(w + 10, h + 10, b"AB", b, 0);
    text_out(w, 0, b"AB", b, 0);
    text_out(0, h, b"AB", b, 0);
    if !keep_a(&[
        (0, 0),
        (w - 1, 0),
        (0, h - 1),
        (w - 1, h - 1),
        (w / 2, h / 2),
    ]) {
        return Err("texto fora da tela escreveu");
    }
    text_out(w - 3, h - 3, b"AB", b, 0);
    if !keep_a(&[
        (w - 4, h - 4),
        (w - 4, h - 1),
        (w - 1, h - 4),
        (0, h - 1),
        (0, 0),
    ]) {
        return Err("texto no canto escreveu fora da caixa");
    }
    // Background painted inside the glyph box, nothing outside it.
    text_out(40, 40, b"Hi", b, 0x0000_00FF);
    let outside = [
        (39, 40),
        (40, 39),
        (40 + 2 * cw, 40),
        (40, 40 + ch),
        (40 + 2 * cw - 1, 40 + ch),
        (39, 40 + ch - 1),
    ];
    if !keep_a(&outside) {
        return Err("texto escreveu fora da caixa do glifo");
    }
    if pixel(40, 40) == Some(a) {
        return Err("texto nao desenhado");
    }
    if let Some(con) = crate::fbcon::FBCON.lock().as_mut() {
        con.clear();
    }
    Ok(())
}

fn timer_ticks_advance() -> Result<(), &'static str> {
    let start = crate::apic::TICKS.load(Ordering::SeqCst);
    // Wait for several ticks; hlt sleeps until the next interrupt.
    for _ in 0..1000 {
        if crate::apic::TICKS.load(Ordering::SeqCst) >= start + 5 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("timer nao avancou 5 ticks")
}

fn keyboard_poll_decodes() -> Result<(), &'static str> {
    // Esvazia a fila (o poll real pode ter capturado ruido).
    while crate::kbd::try_pop_input().is_some() {}
    // shift-make, 'a', shift-break, 'a', Enter, key-release (ignorado)
    for sc in [0x2Au8, 0x1E, 0xAA, 0x1E, 0x1C, 0x9E] {
        crate::kbd::push_scancode_byte(sc);
    }
    let want = *b"Aa\n";
    for w in want {
        if crate::kbd::try_pop_input() != Some(w) {
            return Err("caractere decodificado inesperado");
        }
    }
    if crate::kbd::try_pop_input().is_some() {
        return Err("fila deveria estar vazia");
    }
    crate::kbd::test_push_input(b"hi");
    if crate::kbd::try_pop_input() != Some(b'h') || crate::kbd::try_pop_input() != Some(b'i') {
        return Err("test_push_input falhou");
    }
    Ok(())
}

static RAN: AtomicU64 = AtomicU64::new(0);
static PING: AtomicU64 = AtomicU64::new(0);
static SPIN_A: AtomicU64 = AtomicU64::new(0);
static SPIN_B: AtomicU64 = AtomicU64::new(0);

fn mark_ran() {
    RAN.store(1, Ordering::SeqCst);
    loop {
        crate::sched::yield_now();
    }
}

fn spawned_thread_runs() -> Result<(), &'static str> {
    crate::sched::spawn(mark_ran);
    for _ in 0..100 {
        if RAN.load(Ordering::SeqCst) == 1 {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("thread criada nunca executou")
}

fn ping() {
    loop {
        PING.fetch_add(1, Ordering::SeqCst);
        crate::sched::yield_now();
    }
}

fn yield_alternates_threads() -> Result<(), &'static str> {
    crate::sched::spawn(ping);
    let before = PING.load(Ordering::SeqCst);
    for _ in 0..10 {
        crate::sched::yield_now();
    }
    if PING.load(Ordering::SeqCst) > before {
        Ok(())
    } else {
        Err("yield_now nao passou a CPU para a outra thread")
    }
}

fn spin_a() {
    loop {
        SPIN_A.fetch_add(1, Ordering::SeqCst);
    }
}

fn spin_b() {
    loop {
        SPIN_B.fetch_add(1, Ordering::SeqCst);
    }
}

fn preemption_runs_both_threads() -> Result<(), &'static str> {
    crate::sched::spawn(spin_a);
    crate::sched::spawn(spin_b);
    // Neither thread yields; only the timer can make both progress.
    for _ in 0..2000 {
        if SPIN_A.load(Ordering::SeqCst) > 0 && SPIN_B.load(Ordering::SeqCst) > 0 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("o timer nao preemptou as threads que nao cedem a CPU")
}

fn rsp0_set_before_user() -> Result<(), &'static str> {
    // A kernel stack top we control for this check.
    static STACK: [u8; 4096] = [0; 4096];
    let top = (&STACK as *const _ as u64) + 4096;
    unsafe { arch_x86_64::gdt::set_rsp0(top) };
    if arch_x86_64::gdt::rsp0() != top {
        return Err("rsp0 nao refletiu o valor gravado");
    }
    // Per-CPU block must be installed (GS base non-zero) by kmain before tests.
    if arch_x86_64::percpu::gs_base() == 0 {
        return Err("GS base nao instalado");
    }
    Ok(())
}

fn syscall_dispatch_records_mark() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    // Call the dispatch function directly (no privilege change): proves the
    // table wiring and SYS_WRITE_MARK semantics independent of the asm stub.
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let r = crate::syscall::dispatch(crate::syscall::SYS_WRITE_MARK, 0xCAFE, 0, 0, 0, 0);
    if r != 0 {
        return Err("SYS_WRITE_MARK deveria retornar 0");
    }
    if crate::syscall::LAST_MARK.load(Ordering::SeqCst) != 0xCAFE {
        return Err("SYS_WRITE_MARK nao registrou o argumento");
    }
    Ok(())
}

fn enter_user_returns_after_exit() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let code = crate::user_demo::run();
    if code != 0x2A {
        return Err("codigo de saida do usuario incorreto");
    }
    if crate::syscall::LAST_MARK.load(Ordering::SeqCst) != 0xCAFE {
        return Err("a syscall de escrita do usuario nao chegou ao kernel");
    }
    Ok(())
}

fn syscall_roundtrip_preserves_gs() -> Result<(), &'static str> {
    // After a full user excursion, the kernel GS base must still point at the
    // per-CPU block (a mismatched swapgs count would leave it wrong).
    let before = arch_x86_64::percpu::gs_base();
    let _ = crate::user_demo::run();
    if arch_x86_64::percpu::gs_base() != before {
        return Err("GS base do kernel mudou apos a excursao (swapgs desbalanceado)");
    }
    Ok(())
}

fn kernel_mapped_in_new_address_space() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let space = crate::mem::space::AddressSpace::new_user();
    // A kernel higher-half address (the heap window) must resolve in the new space
    // to the same physical frame as in the active space.
    let probe = 0xffff_ff80_0000_0000u64; // heap window base (mapped by the heap)
    let active = crate::mem::paging::translate(probe);
    let in_new = crate::mem::paging::translate_in(space.root(), probe);
    if active.is_none() {
        return Err("endereco de prova nao mapeado no espaco ativo");
    }
    if in_new != active {
        return Err("metade alta do kernel nao foi clonada no novo espaco");
    }
    // A low (user) address must be unmapped in the fresh space.
    if crate::mem::paging::translate_in(space.root(), 0x40_0000).is_some() {
        return Err("metade baixa deveria estar vazia");
    }
    let _ = PageFlags::USER;
    Ok(())
}

fn address_space_drop_frees_user_frames() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let before = crate::mem::frames::free_frame_count();
    {
        let space = crate::mem::space::AddressSpace::new_user();
        let frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
        unsafe {
            space
                .map(0x40_0000, frame, PageFlags::USER.union(PageFlags::WRITABLE))
                .map_err(|_| "map no espaco novo falhou")?;
        }
        // Dropping `space` must free: the mapped user frame, the intermediate
        // tables, and the PML4 — leaving the count where it started.
    }
    let after = crate::mem::frames::free_frame_count();
    if after != before {
        return Err("Drop do AddressSpace nao liberou todos os frames de usuario");
    }
    Ok(())
}

fn user_process_runs_in_ring3() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // Payload: SYS_WRITE_MARK(0xBEEF) in a loop.
    crate::process::spawn_demo_user();
    // The boot thread yields; the user process must run in Ring 3 and reach the kernel.
    for _ in 0..2000 {
        if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 0xBEEF {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("o processo de usuario nao executou em Ring 3")
}

static USER_A_RAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static USER_B_RAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Called from the syscall dispatcher for every SYS_WRITE_MARK.
pub fn on_user_mark(mark: u64) {
    use core::sync::atomic::Ordering;
    match mark {
        0xA => {
            USER_A_RAN.fetch_add(1, Ordering::SeqCst);
        }
        0xB => {
            USER_B_RAN.fetch_add(1, Ordering::SeqCst);
        }
        _ => {}
    }
}

fn user_process_exits_and_is_reaped() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    // Spawn a user process whose payload does SYS_EXIT immediately.
    crate::process::spawn_exiting_user();
    // Yield until it has run, exited and been reaped (its space+stacks freed).
    for _ in 0..4000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("processo de usuario nao foi recolhido (frames nao voltaram)")
}

fn pe_loads_runs_and_exits() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    let before = crate::mem::frames::free_frame_count();
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // The embedded test PE: SYS_WRITE_MARK(0x5E) then SYS_EXIT(0).
    crate::process::spawn_test_pe();
    let mut marked = false;
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 0x5E {
            marked = true;
        }
        if marked && crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("PE nao carregou/rodou/saiu com recuperacao de frames")
}

fn load_zero_fills_bss() -> Result<(), &'static str> {
    // A section with virtual_size > raw.len() must be zero-filled by the loader.
    use crate::mem::space::AddressSpace;
    let space = AddressSpace::new_user();
    // Build a 1-page section at RVA 0x1000, raw = [0xAA], virtual_size = 0x20.
    let image = crate::process::build_bss_test_pe();
    let base = 0x40_0000u64;
    let entry = crate::loader::load_pe(&space, &image, base).map_err(|_| "load falhou")?;
    let _ = entry;
    // Read the mapped page via the space: byte 0 == 0xAA, byte 1..0x20 == 0.
    let phys = space.translate(base + 0x1000).ok_or("secao nao mapeada")?;
    let p = crate::mem::frames::phys_to_virt(phys) as *const u8;
    unsafe {
        if core::ptr::read_volatile(p) != 0xAA {
            return Err("byte bruto nao copiado");
        }
        if core::ptr::read_volatile(p.add(1)) != 0x00 {
            return Err("excedente do .bss nao foi zerado");
        }
    }
    Ok(())
}

fn ntwritefile_rejects_kernel_pointer() -> Result<(), &'static str> {
    // A higher-half (kernel) pointer must be rejected, not read.
    if crate::usercopy::copy_from_user(0xffff_ff80_0000_0000, 8).is_some() {
        return Err("copy_from_user aceitou ponteiro de kernel");
    }
    Ok(())
}

fn ntwritefile_rejects_unmapped() -> Result<(), &'static str> {
    // A low (user-range) but unmapped address must be rejected.
    if crate::usercopy::copy_from_user(0x0000_0000_1234_0000, 8).is_some() {
        return Err("copy_from_user aceitou endereco nao mapeado");
    }
    Ok(())
}

fn ntwritefile_prints_user_buffer() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::process::spawn_nt_hello_pe();
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6 {
            // "TukOS\n" is 6 bytes; the recorded bytes must match.
            if crate::syscall::last_write_is(b"TukOS\n") {
                return Ok(());
            }
            return Err("NtWriteFile registrou bytes errados");
        }
    }
    Err("NtWriteFile nao foi chamada pelo processo")
}

fn nt_terminate_exits_and_reaps() -> Result<(), &'static str> {
    // Let processes left over from earlier tests finish being reaped first: otherwise
    // their late frame release lands inside the window below and `before` is stale
    // (depends on kernel image size/timing).
    for _ in 0..4000 {
        crate::sched::yield_now();
    }
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_nt_hello_pe(); // ends with NtTerminateProcess
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("NtTerminateProcess nao recuperou os frames")
}

fn ntwritefile_fills_iosb() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // The test PE calls NtWriteFile(CONSOLE_HANDLE, 0,0,0, &iosb, "TukOS\n", 6, 0, 0),
    // then reads iosb.Information back and reports it via SYS_WRITE_MARK before exit.
    crate::process::spawn_ntwrite_real_pe();
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6
            && crate::syscall::last_write_is(b"TukOS\n")
        {
            // Wait for the mark (process reports iosb.Information after the write).
            for _ in 0..8000 {
                if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 6 {
                    return Ok(());
                }
                crate::sched::yield_now();
            }
            return Err("IoStatusBlock.Information != Length");
        }
    }
    Err("NtWriteFile real nao executou")
}

fn ntwritefile_bad_handle() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    let status = crate::syscall::ntwritefile_for_test(0x9999, 0, 0, 0);
    if status != crate::syscall::STATUS_INVALID_HANDLE {
        return Err("handle invalido deveria dar STATUS_INVALID_HANDLE");
    }
    if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 0 {
        return Err("handle invalido nao deveria escrever");
    }
    Ok(())
}

fn exe_imports_ntdll_and_runs() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if crate::ntdll_image::NTDLL_BYTES.is_empty() {
        return Ok(()); // ntdll not embedded on this host: skip.
    }
    if !crate::process::ntdll_has_thunk() {
        return Err("ntdll sem LdrInitializeThunk: caminho de modo usuario ausente");
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_with_ntdll();
    for _ in 0..12000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6
            && crate::syscall::last_write_is(b"TukOS\n")
        {
            // and it should have exited and been reaped.
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("o .exe que importa ntdll nao rodou/saiu")
}

/// Sentinel the user-mode loader passes to NtTerminateProcess for a bad IAT slot
/// (must match crates/ntdll `ST_BAD_IAT`).
const LDR_ST_BAD_IAT: u64 = 0xC000_7006;

fn usermode_loader_rejects_bad_iat() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if crate::ntdll_image::NTDLL_BYTES.is_empty() {
        return Ok(()); // ntdll not embedded on this host: skip.
    }
    if !crate::process::ntdll_has_thunk() {
        return Err("ntdll sem LdrInitializeThunk: nada para rejeitar o IAT ruim");
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_ntdll(crate::process::build_exe_with_bad_iat());
    for _ in 0..12000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != LDR_ST_BAD_IAT {
                return Err("o loader terminou com status inesperado");
            }
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 0 {
                return Err("o .exe com IAT ruim executou (escreveu)");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("IAT ruim: processo nao foi recusado/recolhido")
}

/// Both DLLs embedded (the multi-module tests are skipped otherwise).
fn have_both_dlls() -> bool {
    !crate::ntdll_image::NTDLL_BYTES.is_empty() && !crate::kernel32_image::KERNEL32_BYTES.is_empty()
}

/// Sentinels of the user-mode loader (must match crates/ntdll).
const LDR_ST_NO_DEP: u64 = 0xC000_7009;

fn exe_uses_kernel32_and_runs() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(()); // ntdll/kernel32 not embedded on this host: skip.
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    // exe -> kernel32!{GetStdHandle,WriteFile,ExitProcess} -> ntdll!{NtWriteFile,NtTerminateProcess}
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_importing_kernel32());
    for _ in 0..12000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6
            && crate::syscall::last_write_is(
                b"TukOS
",
            )
            && crate::mem::frames::free_frame_count() == before
        {
            return Ok(());
        }
    }
    Err("o .exe que usa kernel32->ntdll nao imprimiu/saiu")
}

/// Runs the stdin-reading exe with `input` pre-filled in the keyboard queue and
/// checks what it read back (LAST_WRITE) and what the console echoed.
fn run_console_read(input: &[u8], want: &[u8], want_echo: &[u8]) -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    while crate::kbd::try_pop_input().is_some() {}
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    crate::syscall::echo_reset();
    crate::kbd::test_push_input(input);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_reads_stdin());
    for _ in 0..40000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst) != u64::MAX
            && crate::mem::frames::free_frame_count() == before
        {
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != want.len() as u64
                || !crate::syscall::last_write_is(want)
            {
                return Err("NtReadFile devolveu bytes errados");
            }
            if !crate::syscall::echo_is(want_echo) {
                return Err("eco da console errado");
            }
            return Ok(());
        }
    }
    Err("o .exe que le o stdin nao terminou")
}

fn console_read_returns_input() -> Result<(), &'static str> {
    if !have_both_dlls() {
        return Ok(()); // ntdll/kernel32 not embedded on this host: skip.
    }
    run_console_read(b"oi\n", b"oi\n", b"oi\n")?;
    // Backspace erases the 'b' (echo "\b \b") and is not returned.
    run_console_read(b"ab\x08c\n", b"ac\n", b"ab\x08 \x08c\n")
}

/// End to end: a user exe opens `C:\cat.txt` (initrd) through kernel32!CreateFileW,
/// reads it with ReadFile and writes it to stdout; it exits 0 and is reaped.
fn program_reads_file() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(()); // ntdll/kernel32 not embedded on this host: skip.
    }
    let want: &[u8] = b"arquivo de teste do TukOS
";
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_reads_file());
    for _ in 0..40000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX && crate::mem::frames::free_frame_count() == before {
            if st != 0 {
                return Err(
                    "program_reads_file: o exe saiu != 0 (0xBAD = CreateFile/tipo/tamanho/Read falhou)",
                );
            }
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != want.len() as u64
                || !crate::syscall::last_write_is(want)
            {
                return Err("program_reads_file: conteudo lido diferente de cat.txt");
            }
            return Ok(());
        }
    }
    Err("program_reads_file: o exe nao terminou/foi recolhido")
}

/// End to end: a Ring-3 parent calls kernel32!CreateProcessA("C:\test\exit42.exe"),
/// WaitForSingleObject(INFINITE), GetExitCodeProcess, CloseHandle and exits with
/// `childCode + 0x1000`. The child exits 0x2A first, so the last terminate status must
/// be 0x102A (0xBAD = a wrapper failed); both are reaped (frames back to baseline).
fn program_spawns_child() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(()); // ntdll/kernel32 not embedded on this host: skip.
    }
    crate::fs::register_test_file(
        "test/exit42.exe",
        crate::process::build_exe_exit_code_2a().leak(),
    );
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_spawns_child());
    for _ in 0..80000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if (0xBAD0..0xBAE0).contains(&st) {
            return Err(match st - 0xBAD0 {
                0 => "program_spawns_child: pid sintetico zero",
                1 => "program_spawns_child: CreateProcessA falhou",
                2 => "program_spawns_child: WaitForSingleObject falhou",
                3 => "program_spawns_child: GetExitCodeProcess falhou",
                _ => "program_spawns_child: CloseHandle falhou",
            });
        }
        if st == 0x102A {
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        } else if st != u64::MAX && st != 0x2A {
            return Err("program_spawns_child: status final inesperado");
        }
    }
    Err("program_spawns_child: pai/filho nao terminaram ou frames vazaram")
}

fn heap_alloc_write_free() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(()); // ntdll/kernel32 not embedded on this host: skip.
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    // exe -> kernel32!{GetProcessHeap,HeapAlloc,HeapFree,..} -> ntdll!{RtlGetProcessHeap,
    // RtlAllocateHeap,RtlFreeHeap} -> NtAllocateVirtualMemory (Ring 3, end to end).
    // The exe exits with 0xBAD (and does not print) if any heap check fails.
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_heap_test());
    for _ in 0..40000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != 0 {
                return Err(
                    "heap: o exe falhou (0xBAD = sobreposicao/reuso/zeragem; outro = loader)",
                );
            }
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 6
                || !crate::syscall::last_write_is(
                    b"TukOS
",
                )
            {
                return Err("heap: saiu com 0 mas nao imprimiu TukOS");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("heap: o exe nao terminou/recolheu (frames vazados?)")
}

fn peb_ldr_three_modules() -> Result<(), &'static str> {
    if !have_both_dlls() {
        return Ok(());
    }
    let es =
        crate::process::build_exe_k32_space_with(crate::process::build_exe_importing_kernel32());
    let rd = |va: u64| -> Option<u64> {
        let mut b = [0u8; 8];
        for (i, o) in b.iter_mut().enumerate() {
            let a = va + i as u64;
            let phys = es.space.translate(a & !4095)?;
            *o = unsafe { *((crate::mem::frames::phys_to_virt(phys) + (a & 4095)) as *const u8) };
        }
        Some(u64::from_le_bytes(b))
    };
    let ldr = rd(es.peb_va + 0x18).ok_or("PEB ilegivel")?;
    let head = ldr + 0x10;
    // Load order: exe, kernel32, ntdll, then back to the head; DllBase at +0x30.
    let want = [
        crate::process::EXE_BASE,
        crate::process::KERNEL32_BASE,
        crate::process::NTDLL_BASE,
    ];
    let mut cur = rd(head).ok_or("head ilegivel")?;
    for w in want {
        if cur == head || rd(cur + 0x30) != Some(w) {
            return Err("ordem/base dos modulos errada em PEB->Ldr");
        }
        cur = rd(cur).ok_or("flink ilegivel")?;
    }
    if cur != head {
        return Err("lista deveria ter exatamente tres modulos");
    }
    Ok(())
}

fn exe_calls_forwarded_kernel32_export() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(());
    }
    // kernel32 exports TukosCloseForward as a FORWARDER to ntdll.NtClose; the exe's
    // IAT must end up at ntdll's stub, so NtClose(0x1234) returns STATUS_INVALID_HANDLE,
    // which the exe passes to ExitProcess.
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_calling_forwarder());
    for _ in 0..12000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != crate::syscall::STATUS_INVALID_HANDLE {
                return Err("forwarder nao levou ao NtTerminateProcess do ntdll");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("exe que chama o forwarder nao terminou/recolheu")
}

fn exit_code_is_reported() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(());
    }
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_exit_code_2a());
    for _ in 0..12000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != 0x2A {
                return Err("ExitProcess(0x2A) nao reportou 0x2A ao kernel");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("exe que chama ExitProcess(0x2A) nao terminou/recolheu")
}

/// UTF-16LE bytes of `s` (as a user would pass a path to NT_CREATE_PROCESS).
fn utf16le(s: &str) -> alloc::vec::Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Creates `path` as a child of a stand-in parent table, waits, and checks the
/// process handle + exit code + frame reclamation + Close. Returns the exit code.
fn run_child(path: &str) -> Result<u32, &'static str> {
    use crate::object::{HandleTable, Object, STILL_ACTIVE};
    let before = crate::mem::frames::free_frame_count();
    let mut parent = HandleTable::new();
    let exit =
        crate::syscall::create_process(&utf16le(path)).map_err(|_| "create_process falhou")?;
    let h = parent.insert(Object::Process { exit });
    let exit = parent
        .process_exit(h)
        .ok_or("handle de processo nao resolve")?;
    // The same Arc lives in the handle and in the child's Thread (until it exits).
    if h == 0 || parent.process_exit(h + 4).is_some() {
        return Err("handle invalido resolveu");
    }
    crate::syscall::wait_for_exit(&exit);
    let code = exit.exit_code();
    if code == STILL_ACTIVE {
        return Err("exit code ainda STILL_ACTIVE apos o wait");
    }
    drop(exit);
    if !parent.close(h) || parent.close(h) {
        return Err("close do handle de processo (ou double-close) incorreto");
    }
    // The wait returned on the parent's stack AFTER the child was reaped.
    if crate::mem::frames::free_frame_count() != before {
        return Err("frames vazaram apos create+wait+close");
    }
    Ok(code)
}

fn create_wait_exit() -> Result<(), &'static str> {
    if !have_both_dlls() {
        return Ok(());
    }
    crate::fs::register_test_file(
        "test/exit42.exe",
        crate::process::build_exe_exit_code_2a().leak(),
    );
    let code = run_child(r"C:\test\Exit42.exe")?;
    if code != 0x2A {
        return Err("ExitProcess(0x2A) do filho nao virou exit code 0x2A");
    }
    // Run it twice: nothing is left over from the first child (no leak, no reuse bug).
    if run_child("c:/test/exit42.exe")? != 0x2A {
        return Err("segundo filho: exit code errado");
    }
    Ok(())
}

fn child_fault_reports_status() -> Result<(), &'static str> {
    if !have_both_dlls() {
        return Ok(());
    }
    crate::fs::register_test_file("test/fault.exe", crate::process::build_exe_faults().leak());
    let code = run_child(r"C:\test\fault.exe")?;
    // A faulted child reports an exception status, not 0 and not STILL_ACTIVE.
    if code != 0xC000_0005 {
        return Err("filho com #PF nao reportou STATUS_ACCESS_VIOLATION");
    }
    // The kernel and a following (healthy) child still work.
    crate::fs::register_test_file(
        "test/exit42.exe",
        crate::process::build_exe_exit_code_2a().leak(),
    );
    if run_child("test/exit42.exe")? != 0x2A {
        return Err("kernel nao se recuperou do filho com fault");
    }
    Ok(())
}

/// Reads `PEB->ProcessParameters->CommandLine` of a freshly built child space and
/// returns its UTF-16 text after checking the UNICODE_STRING is well formed.
fn read_child_cmdline(es: &crate::process::ExeSpace) -> Result<alloc::vec::Vec<u16>, &'static str> {
    let rd = |va: u64, n: usize| -> Result<alloc::vec::Vec<u8>, &'static str> {
        let mut out = alloc::vec::Vec::new();
        for i in 0..n as u64 {
            let a = va + i;
            let pa = es
                .space
                .translate(a & !0xFFF)
                .ok_or("pagina do filho nao mapeada")?;
            let p = crate::mem::frames::phys_to_virt(pa) as *const u8;
            out.push(unsafe { p.add((a & 0xFFF) as usize).read() });
        }
        Ok(out)
    };
    let u64_at = |va: u64| -> Result<u64, &'static str> {
        Ok(u64::from_le_bytes(rd(va, 8)?.try_into().unwrap()))
    };
    let pp = u64_at(es.peb_va + 0x20)?;
    let us = pp + 0x70;
    let len = u16::from_le_bytes(rd(us, 2)?.try_into().unwrap());
    let max = u16::from_le_bytes(rd(us + 2, 2)?.try_into().unwrap());
    let buf = u64_at(us + 8)?;
    if max != len + 2 || len % 2 != 0 {
        return Err("UNICODE_STRING CommandLine mal formada (Length/MaximumLength)");
    }
    if buf < pp || buf + u64::from(max) > (pp & !0xFFF) + 4096 {
        return Err("Buffer da CommandLine fora da pagina de parametros");
    }
    let raw = rd(buf, usize::from(max))?;
    let units: alloc::vec::Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    if units.last() != Some(&0) {
        return Err("CommandLine sem terminador NUL");
    }
    Ok(units[..units.len() - 1].to_vec())
}

fn child_sees_command_line() -> Result<(), &'static str> {
    if !have_both_dlls() {
        return Ok(());
    }
    let exe = || crate::process::build_exe_exit_code_2a();
    let w = |s: &str| -> alloc::vec::Vec<u16> { s.encode_utf16().collect() };
    let es = crate::process::build_exe_k32_space_with_cmdline(exe(), &w("prog alpha beta"));
    if read_child_cmdline(&es)? != w("prog alpha beta") {
        return Err("CommandLine do filho != \"prog alpha beta\"");
    }
    drop(es);
    // Empty -> default image name (argv[0] exists).
    let es = crate::process::build_exe_k32_space_with_cmdline(exe(), &[]);
    if read_child_cmdline(&es)?.is_empty() {
        return Err("cmdline vazia deveria virar o nome da imagem");
    }
    drop(es);
    // Overlong -> truncated to 256 units, still well formed and inside the page.
    let long: alloc::vec::Vec<u16> = core::iter::repeat_n(0x41u16, 2000).collect();
    let es = crate::process::build_exe_k32_space_with_cmdline(exe(), &long);
    if read_child_cmdline(&es)?.len() != 256 {
        return Err("cmdline longa nao foi truncada em 256 unidades");
    }
    drop(es);
    // Syscall-level validation: odd length / too long are rejected before any spawn.
    let before = crate::mem::frames::free_frame_count();
    crate::fs::register_test_file("test/exit42.exe", exe().leak());
    let p = utf16le("test/exit42.exe");
    if crate::syscall::create_process_cmd(&p, &[0x41]).err()
        != Some(crate::syscall::STATUS_INVALID_PARAMETER)
    {
        return Err("cmdline de tamanho impar deveria ser INVALID_PARAMETER");
    }
    if crate::syscall::create_process_cmd(&p, &[0x41; 514]).err()
        != Some(crate::syscall::STATUS_NAME_TOO_LONG)
    {
        return Err("cmdline > 512 bytes deveria ser NAME_TOO_LONG");
    }
    // A real child with a command line still runs and exits normally.
    let exit = crate::syscall::create_process_cmd(&p, &utf16le("exit42 a b"))
        .map_err(|_| "create_process_cmd falhou")?;
    crate::syscall::wait_for_exit(&exit);
    if exit.exit_code() != 0x2A {
        return Err("filho com cmdline nao saiu com 0x2A");
    }
    drop(exit);
    if crate::mem::frames::free_frame_count() != before {
        return Err("create com cmdline vazou frames");
    }
    Ok(())
}

fn create_process_errors() -> Result<(), &'static str> {
    use crate::syscall::{
        STATUS_INVALID_PARAMETER, STATUS_NAME_TOO_LONG, STATUS_OBJECT_NAME_NOT_FOUND,
        create_process,
    };
    if !have_both_dlls() {
        return Ok(());
    }
    let before = crate::mem::frames::free_frame_count();
    if create_process(&utf16le("nope.exe")).err() != Some(STATUS_OBJECT_NAME_NOT_FOUND) {
        return Err("exe ausente deveria dar OBJECT_NAME_NOT_FOUND");
    }
    if create_process(&utf16le(r"..\x.exe")).err() != Some(STATUS_OBJECT_NAME_NOT_FOUND) {
        return Err("traversal deveria falhar");
    }
    if create_process(&[0x41]).err() != Some(STATUS_INVALID_PARAMETER) {
        return Err("comprimento impar deveria ser INVALID_PARAMETER");
    }
    if create_process(&[0x41; 600]).err() != Some(STATUS_NAME_TOO_LONG) {
        return Err("caminho longo deveria ser NAME_TOO_LONG");
    }
    // Wait/query on non-process handles resolve to nothing.
    let mut t = crate::object::HandleTable::new();
    let con = t.insert(crate::object::Object::Console);
    if t.process_exit(con).is_some() || t.process_exit(0x1000).is_some() {
        return Err("handle que nao e de processo resolveu como processo");
    }
    if crate::mem::frames::free_frame_count() != before {
        return Err("erro de create_process vazou frames");
    }
    Ok(())
}

fn loader_rejects_missing_dependency() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(());
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    // The exe imports user32.dll, which is not in PEB->Ldr.
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_importing_missing_dll());
    for _ in 0..12000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != LDR_ST_NO_DEP {
                return Err("dependencia ausente: status do loader inesperado");
            }
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 0 {
                return Err("exe com dependencia ausente executou");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("dependencia ausente: processo nao foi recusado/recolhido")
}

fn usermode_loader_rejects_bad_iat_dir12() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if !have_both_dlls() {
        return Ok(());
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::LAST_TERMINATE_STATUS.store(u64::MAX, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    // IAT slot inside the R+X .text, AND data directory #12 hostilely covers .text:
    // the rejection must come from the loader's !exec gate, not from dir 12.
    crate::process::spawn_exe_image_with_k32(crate::process::build_exe_with_bad_iat_dir12());
    for _ in 0..12000 {
        crate::sched::yield_now();
        let st = crate::syscall::LAST_TERMINATE_STATUS.load(Ordering::SeqCst);
        if st != u64::MAX {
            if st != LDR_ST_BAD_IAT {
                return Err("dir12 hostil: status do loader inesperado");
            }
            if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 0 {
                return Err("exe com dir12 hostil executou");
            }
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("dir12 hostil: processo nao foi recusado/recolhido")
}

fn peb_ldr_list_well_formed() -> Result<(), &'static str> {
    if crate::ntdll_image::NTDLL_BYTES.is_empty() {
        return Ok(());
    }
    let es = crate::process::build_exe_ntdll_space();
    let rd = |va: u64, n: usize| -> Option<alloc::vec::Vec<u8>> {
        let mut v = alloc::vec::Vec::new();
        for i in 0..n as u64 {
            let a = va + i;
            let phys = es.space.translate(a & !4095)?;
            v.push(unsafe {
                *((crate::mem::frames::phys_to_virt(phys) + (a & 4095)) as *const u8)
            });
        }
        Some(v)
    };
    let r64 = |va: u64| -> Option<u64> { Some(u64::from_le_bytes(rd(va, 8)?.try_into().ok()?)) };
    let ldr = r64(es.peb_va + 0x18).ok_or("PEB ilegivel")?;
    if ldr == 0 {
        return Err("PEB.Ldr == 0");
    }
    let head = ldr + 0x10;
    let mut cur = r64(head).ok_or("head ilegivel")?;
    let mut mods = alloc::vec::Vec::new();
    while cur != head {
        if mods.len() >= 8 {
            return Err("lista nao circular");
        }
        mods.push(cur);
        cur = r64(cur).ok_or("flink ilegivel")?;
    }
    if mods.len() != 2 {
        return Err("lista deveria ter dois modulos");
    }
    if r64(head + 8) != Some(mods[1]) || r64(mods[0] + 8) != Some(head) {
        return Err("Blink inconsistente");
    }
    if r64(mods[0] + 0x30) != Some(crate::process::EXE_BASE)
        || r64(mods[0] + 0x38) != Some(es.exe_entry)
    {
        return Err("entrada 0 nao e o .exe");
    }
    let want: alloc::vec::Vec<u16> = "ntdll.dll".encode_utf16().collect();
    let mut found = false;
    for &m in &mods {
        let len = rd(m + 0x58, 2).ok_or("len")?;
        let len = u16::from_le_bytes([len[0], len[1]]) as usize;
        let buf = r64(m + 0x58 + 8).ok_or("buf")?;
        let raw = rd(buf, len).ok_or("nome ilegivel")?;
        let name: alloc::vec::Vec<u16> = raw
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if name == want && r64(m + 0x30) == Some(crate::process::NTDLL_BASE) {
            found = true;
        }
    }
    if !found {
        return Err("ntdll.dll nao encontrado na lista");
    }
    Ok(())
}

fn two_user_processes_preempt() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    USER_A_RAN.store(0, Ordering::SeqCst);
    USER_B_RAN.store(0, Ordering::SeqCst);
    // Two user processes that never yield; only the timer makes both run.
    crate::process::spawn_demo_user_pair();
    for _ in 0..4000 {
        if USER_A_RAN.load(Ordering::SeqCst) > 0 && USER_B_RAN.load(Ordering::SeqCst) > 0 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("o timer nao preemptou os dois processos de usuario")
}

fn higher_half_pml4_fully_reserved() -> Result<(), &'static str> {
    use mm::paging::entry_present;
    // Every PML4 entry 256..512 of the active space must be present after init,
    // so a new address space clones a complete kernel higher half.
    let pml4 = crate::arch_cr3_base();
    let table = crate::mem::frames::phys_to_virt(pml4) as *const u64;
    for i in 256..512 {
        let e = unsafe { core::ptr::read_volatile(table.add(i)) };
        if !entry_present(e) {
            return Err("entrada PML4 da metade alta nao reservada");
        }
    }
    Ok(())
}

fn reloc_dir64_crosses_page() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    // PE with a DIR64 relocation whose 8-byte target straddles a page boundary
    // (target RVA 0x1FFC), loaded at a base != image_base so delta != 0.
    let image = crate::process::build_reloc_cross_page_pe();
    let space = AddressSpace::new_user();
    let load_base = 0x80_0000u64;
    crate::loader::load_pe(&space, &image, load_base).map_err(|_| "load falhou")?;
    let lo_phys = space
        .translate(load_base + 0x1000)
        .ok_or("pagina 1 nao mapeada")?;
    let hi_phys = space
        .translate(load_base + 0x2000)
        .ok_or("pagina 2 nao mapeada")?;
    let lo = crate::mem::frames::phys_to_virt(lo_phys) as *const u8;
    let hi = crate::mem::frames::phys_to_virt(hi_phys) as *const u8;
    let mut bytes = [0u8; 8];
    unsafe {
        core::ptr::copy_nonoverlapping(lo.add(0xFFC), bytes.as_mut_ptr(), 4);
        core::ptr::copy_nonoverlapping(hi, bytes.as_mut_ptr().add(4), 4);
    }
    let got = u64::from_le_bytes(bytes);
    let expected = crate::process::RELOC_FIXTURE_ORIGINAL
        .wrapping_add(load_base.wrapping_sub(crate::process::RELOC_FIXTURE_IMAGE_BASE));
    if got != expected {
        return Err("relocacao entre paginas incorreta");
    }
    Ok(())
}

fn stack_placed_above_image() -> Result<(), &'static str> {
    // image_top of the standard test PE must be above its last section end.
    let image = crate::process::test_pe_bytes();
    let base = 0x40_0000u64;
    let top = crate::loader::image_top(image, base).map_err(|_| "image_top falhou")?;
    if top <= base + 0x1000 {
        return Err("image_top nao ficou acima da imagem");
    }
    Ok(())
}

// ---- NtAllocateVirtualMemory / NtFreeVirtualMemory (core in vmem.rs) ----

fn valloc_basic() -> Result<(), &'static str> {
    use crate::mem::frames::{free_frame_count, phys_to_virt};
    if crate::syscall::NT_ALLOCATE_VIRTUAL_MEMORY != 5
        || crate::syscall::NT_FREE_VIRTUAL_MEMORY != 6
    {
        return Err("numeros de syscall errados");
    }
    let before = free_frame_count();
    let space = crate::mem::space::AddressSpace::new_user();
    let root = space.root();
    let (base, size) = crate::vmem::alloc_in(root, 0, 0x3000, crate::vmem::PAGE_READWRITE)
        .map_err(|_| "alloc falhou")?;
    if size != 0x3000 || base % 4096 != 0 || base < crate::vmem::USER_MIN {
        return Err("base/size invalidos");
    }
    for i in 0..3u64 {
        let phys = crate::mem::paging::translate_user_writable_in(root, base + i * 4096)
            .ok_or("pagina nao mapeada USER|RW")?;
        let p = phys_to_virt(phys) as *mut u8;
        for off in 0..4096usize {
            if unsafe { p.add(off).read() } != 0 {
                return Err("pagina nao zerada");
            }
        }
        for off in 0..4096usize {
            unsafe { p.add(off).write((off as u8) ^ (i as u8 + 1)) };
        }
    }
    for i in 0..3u64 {
        let phys = crate::mem::paging::translate_in(root, base + i * 4096).ok_or("sumiu")?;
        let p = phys_to_virt(phys) as *const u8;
        for off in 0..4096usize {
            if unsafe { p.add(off).read() } != (off as u8) ^ (i as u8 + 1) {
                return Err("padrao divergiu");
            }
        }
    }
    // Free: pages are gone and a second free is rejected.
    crate::vmem::free_in(root, base, size).map_err(|_| "free falhou")?;
    if crate::mem::paging::translate_in(root, base).is_some() {
        return Err("pagina ainda mapeada apos free");
    }
    if crate::vmem::free_in(root, base, size) != Err(crate::vmem::STATUS_INVALID_PARAMETER) {
        return Err("free duplo deveria falhar");
    }
    // No leak: with the space dropped (tables + PML4 freed) every frame is back.
    drop(space);
    if free_frame_count() != before {
        return Err("vazamento de frames");
    }
    Ok(())
}

fn valloc_rejects_overlap() -> Result<(), &'static str> {
    use crate::mem::frames::free_frame_count;
    use mm::paging::PageFlags;
    let space = crate::mem::space::AddressSpace::new_user();
    let root = space.root();
    let frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    unsafe {
        space
            .map(EXE_BASE, frame, PageFlags::USER)
            .map_err(|_| "map")?;
    }
    let before = free_frame_count();
    // Exact page, a range that starts below and runs into it, and one that only
    // touches its last page: all must conflict.
    for (b, s) in [
        (EXE_BASE, 0x1000u64),
        (EXE_BASE - 0x2000, 0x3000),
        (EXE_BASE - 0x1000, 0x2000),
    ] {
        if crate::vmem::alloc_in(root, b, s, crate::vmem::PAGE_READWRITE)
            != Err(crate::vmem::STATUS_CONFLICTING_ADDRESSES)
        {
            return Err("sobreposicao nao foi rejeitada");
        }
    }
    if free_frame_count() != before {
        return Err("rejeicao alocou frames");
    }
    if crate::mem::paging::translate_in(root, EXE_BASE - 0x1000).is_some() {
        return Err("rejeicao deixou pagina mapeada");
    }
    Ok(())
}

fn valloc_rejects_kernel_range() -> Result<(), &'static str> {
    use crate::mem::frames::free_frame_count;
    use crate::vmem::{PAGE_READWRITE, STATUS_INVALID_PARAMETER, alloc_in, free_in};
    let space = crate::mem::space::AddressSpace::new_user();
    let root = space.root();
    let before = free_frame_count();
    let bad: [(u64, u64); 8] = [
        (0xffff_8000_0000_0000, 0x1000),   // higher half
        (0xffff_ffff_ffff_f000, 0x1000),   // top of memory
        (0x0000_7fff_ffff_f000, 0x2000),   // straddles the user limit
        (0x0000_8000_0000_0000, 0x1000),   // first non-canonical address
        (0x0000_7fff_ffff_f000, u64::MAX), // size overflow
        (0xffff_ffff_ffff_f000, 0x2000),   // base+size wraps to 0
        (0x1000, 0x1000),                  // near-null
        (0x40_0000, 0),                    // zero size
    ];
    for (b, s) in bad {
        if alloc_in(root, b, s, PAGE_READWRITE) != Err(STATUS_INVALID_PARAMETER) {
            return Err("faixa invalida aceita");
        }
        if free_in(root, b, s) != Err(STATUS_INVALID_PARAMETER) {
            return Err("free de faixa invalida aceito");
        }
    }
    // Only PAGE_READWRITE is accepted (no executable pages).
    if alloc_in(root, 0x40_0000, 0x1000, 0x40) != Err(STATUS_INVALID_PARAMETER) {
        return Err("protecao executavel aceita");
    }
    if free_frame_count() != before {
        return Err("rejeicao alocou frames");
    }
    Ok(())
}

/// Yields from a deliberately different stack depth than any earlier yield, so a
/// stale saved rsp (see docs/observations/2026-10-14-scheduler-reap-pf.md) would
/// point into overwritten stack.
#[inline(never)]
fn yield_from_depth(depth: u32) -> u64 {
    let pad = [depth as u64; 24];
    let pad = core::hint::black_box(pad);
    if depth > 0 {
        return yield_from_depth(depth - 1) + pad[3];
    }
    crate::sched::yield_now();
    pad[5]
}

/// Plan 12 regression: with the boot thread as the only live thread and the
/// others left `Exited` in the thread table, `switch_to_next` used to pick
/// to==from and restore a stale rsp (#PF). It must now return without switching.
/// See docs/observations/2026-10-14-scheduler-reap-pf.md.
fn single_thread_after_exit_no_fault() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exiting_user();
    // Let it run, exit and be reaped (frames back to baseline).
    let mut reaped = false;
    for _ in 0..4000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            reaped = true;
            break;
        }
    }
    if !reaped {
        return Err("thread encerrada nao foi recolhida (frames nao voltaram)");
    }
    crate::sched::retire_others_for_test();
    // Every yield now selects to==from; must keep running on the same stack.
    for i in 0..64u32 {
        yield_from_depth(1 + (i % 7) * 3);
    }
    if crate::mem::frames::free_frame_count() != before {
        return Err("frames mudaram apos yields com thread unica");
    }
    Ok(())
}

static SLOT_CHILD_DONE: AtomicU64 = AtomicU64::new(0);

fn slot_child() {
    SLOT_CHILD_DONE.fetch_add(1, Ordering::SeqCst);
    crate::sched::exit_current();
}

/// Regression for docs/observations/2026-10-14-thread-slot-exhaustion.md.
/// Spawns ONE short-lived kernel thread at a time, waits for it to exit and be
/// reaped (frames back to baseline), 200 times (well over MAX_THREADS = 64).
/// Reaped slots must be recycled, so every spawn succeeds.
fn thread_slots_recycle() -> Result<(), &'static str> {
    for _ in 0..100 {
        crate::sched::yield_now();
    }
    let baseline = crate::mem::frames::free_frame_count();
    let (used0, max) = crate::sched::slot_usage_for_test();
    let mut failed_at: Option<usize> = None;
    for i in 0..200usize {
        if !crate::sched::has_thread_capacity() {
            failed_at = Some(i);
            break;
        }
        let n = SLOT_CHILD_DONE.load(Ordering::SeqCst);
        crate::sched::spawn(slot_child);
        let mut done = false;
        for _ in 0..2000 {
            crate::sched::yield_now();
            if SLOT_CHILD_DONE.load(Ordering::SeqCst) > n {
                done = true;
                break;
            }
        }
        if !done {
            return Err("filho nao executou");
        }
        // Leftover spinner threads burn a timer tick per yield, so yield only until
        // the reaper has returned the frames (it runs on the first switch back).
        for _ in 0..10 {
            if crate::mem::frames::free_frame_count() == baseline {
                break;
            }
            crate::sched::yield_now();
        }
        if crate::mem::frames::free_frame_count() != baseline {
            return Err("frames nao voltaram a baseline apos reap");
        }
    }
    let (used1, _) = crate::sched::slot_usage_for_test();
    crate::kprintln!(
        "thread_slots_recycle: slots_usados_inicio={} max={} falhou_na_iteracao={:?} slots_usados_fim={} frames_baseline={} frames_agora={}",
        used0,
        max,
        failed_at,
        used1,
        baseline,
        crate::mem::frames::free_frame_count()
    );
    if failed_at.is_some() {
        return Err("spawn falhou: slots nao reciclados");
    }
    Ok(())
}
