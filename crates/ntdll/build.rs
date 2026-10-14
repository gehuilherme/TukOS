fn main() {
    // No CRT: the DLL has no entry point and no default libraries.
    println!("cargo:rustc-cdylib-link-arg=/NOENTRY");
    println!("cargo:rustc-cdylib-link-arg=/NODEFAULTLIB");
}
