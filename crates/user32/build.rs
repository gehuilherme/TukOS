fn main() {
    // No CRT: the DLL has no entry point and no default libraries (mirrors
    // crates/gdi32, crates/kernel32, crates/ntdll). Without this the linker links
    // the system user32.lib + MSVC CRT, which TukOS cannot resolve at load time.
    println!("cargo:rustc-cdylib-link-arg=/NOENTRY");
    println!("cargo:rustc-cdylib-link-arg=/NODEFAULTLIB");
}
