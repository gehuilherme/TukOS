use std::path::PathBuf;

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=-T{dir}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");
    embed_conformance(&dir, "hello.exe", "conformance.exe");
    embed_conformance(&dir, "hello32.exe", "conformance32.exe");

    // Embed the ntdll.dll produced by `cargo xtask ntdll` (we only copy the
    // artifact: no nested cargo, so no target-lock contention). If it is
    // missing (e.g. Linux CI), embed an empty file instead of failing.
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("ntdll.dll");
    let src = PathBuf::from(&dir)
        .join("..")
        .join("target")
        .join("x86_64-pc-windows-msvc")
        .join("release")
        .join("ntdll.dll");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=../crates/ntdll/src");
    if std::fs::copy(&src, &out).is_err() {
        println!(
            "cargo:warning=ntdll.dll not found; embedding empty (run `cargo xtask ntdll` on Windows)"
        );
        std::fs::write(&out, []).expect("write empty ntdll.dll");
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("kernel32.dll");
    let src = PathBuf::from(&dir)
        .join("..")
        .join("target")
        .join("x86_64-pc-windows-msvc")
        .join("release")
        .join("kernel32.dll");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=../crates/kernel32/src");
    if std::fs::copy(&src, &out).is_err() {
        println!(
            "cargo:warning=kernel32.dll not found; embedding empty (run `cargo xtask kernel32` on Windows)"
        );
        std::fs::write(&out, []).expect("write empty kernel32.dll");
    }
}

// Embed the conformance program built by `cargo xtask conformance` (empty if absent).
fn embed_conformance(dir: &str, name: &str, out_name: &str) {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(out_name);
    let src = PathBuf::from(dir)
        .join("..")
        .join("target")
        .join("conformance")
        .join(name);
    println!("cargo:rerun-if-changed={}", src.display());
    if std::fs::copy(&src, &out).is_err() {
        std::fs::write(&out, []).expect("write empty conformance exe");
    }
}
