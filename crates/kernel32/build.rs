fn main() {
    // No CRT: the DLL has no entry point and no default libraries.
    println!("cargo:rustc-cdylib-link-arg=/NOENTRY");
    println!("cargo:rustc-cdylib-link-arg=/NODEFAULTLIB");
    // A real forwarder export (module.function syntax): the export table entry
    // points at the string "ntdll.NtClose" inside the export directory, so the
    // loader must chase it into ntdll. Arity matches (1 arg -> 1 arg). Exercised
    // end-to-end by the `exe_calls_forwarded_kernel32_export` ktest.
    println!("cargo:rustc-cdylib-link-arg=/EXPORT:TukosCloseForward=ntdll.NtClose");
}
