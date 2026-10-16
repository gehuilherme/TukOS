use std::path::PathBuf;

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=-T{dir}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");
    embed_conformance(&dir, "hello.exe", "conformance.exe");
    embed_conformance(&dir, "hello32.exe", "conformance32.exe");
    embed_conformance(&dir, "heap.exe", "conformance_heap.exe");
    embed_conformance(&dir, "crt/hello.exe", "crt_hello.exe");
    embed_conformance(&dir, "crt/echo.exe", "echo.exe");
    embed_conformance(&dir, "crt/argv.exe", "argv.exe");
    embed_conformance(&dir, "crt/fileio.exe", "fileio.exe");
    embed_conformance(&dir, "crt/parent.exe", "parent.exe");
    embed_conformance(&dir, "crt/child.exe", "child.exe");
    embed_conformance(&dir, "crt/shell.exe", "shell.exe");
    embed_conformance(&dir, "gui/test_win.exe", "test_win.exe");
    embed_conformance(&dir, "gui/hello_win.exe", "hello_win.exe");
    embed_conformance(&dir, "gui/test_type.exe", "test_type.exe");
    embed_conformance(&dir, "gui/test_click.exe", "test_click.exe");
    embed_conformance(&dir, "gui/paint_win.exe", "paint_win.exe");
    embed_conformance(&dir, "gui/test_pos.exe", "test_pos.exe");
    embed_conformance(&dir, "gui/desk_demo.exe", "desk_demo.exe");

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

    embed_dll(&dir, "gdi32.dll", "gdi32");
    embed_dll(&dir, "user32.dll", "user32");
}

// Embed a Windows DLL built by `cargo xtask <name>` (empty if absent, e.g. Linux CI).
fn embed_dll(dir: &str, file: &str, crate_name: &str) {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(file);
    let src = PathBuf::from(dir)
        .join("..")
        .join("target")
        .join("x86_64-pc-windows-msvc")
        .join("release")
        .join(file);
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=../crates/{crate_name}/src");
    if std::fs::copy(&src, &out).is_err() {
        println!(
            "cargo:warning={file} not found; embedding empty (run `cargo xtask {crate_name}` on Windows)"
        );
        std::fs::write(&out, []).expect("write empty dll");
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
