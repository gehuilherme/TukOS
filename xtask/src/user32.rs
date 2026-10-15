use std::path::PathBuf;
use std::process::Command;

const TARGET: &str = "x86_64-pc-windows-msvc";

/// Builds `crates/user32` for Windows x64 (release) and returns the DLL path.
pub fn build() -> crate::Result<PathBuf> {
    crate::util::run(Command::new("cargo").args([
        "build",
        "-p",
        "user32",
        "--target",
        TARGET,
        "--release",
    ]))?;
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join(TARGET)
        .join("release")
        .join("user32.dll");
    if dll.exists() {
        Ok(dll)
    } else {
        Err(format!("user32.dll nao encontrada em {}", dll.display()))
    }
}

#[cfg(test)]
mod tests {
    fn built() -> Vec<u8> {
        let dll = super::build().expect("build user32");
        std::fs::read(&dll).expect("read user32.dll")
    }

    // Needs the MSVC linker, so only run on Windows hosts.
    #[test]
    #[cfg_attr(not(target_os = "windows"), ignore)]
    fn user32_dll_exports_win32_functions() {
        let bytes = built();
        let pe = pe::PeFile::parse(&bytes).expect("user32.dll e um PE valido");
        let exports: Vec<_> = pe
            .exports()
            .expect("export table")
            .map(|e| e.name.to_string())
            .collect();
        // The 13 surveyed user32 functions must all be exported.
        for want in [
            "RegisterClassA",
            "RegisterClassExA",
            "CreateWindowExA",
            "ShowWindow",
            "UpdateWindow",
            "GetMessageA",
            "TranslateMessage",
            "DispatchMessageA",
            "DefWindowProcA",
            "PostQuitMessage",
            "LoadCursorA",
            "BeginPaint",
            "EndPaint",
            "FillRect",
        ] {
            assert!(exports.iter().any(|n| n == want), "exports: {exports:?}");
        }
    }

    // Needs the MSVC linker, so only run on Windows hosts.
    #[test]
    #[cfg_attr(not(target_os = "windows"), ignore)]
    fn user32_imports_only_ntdll() {
        let bytes = built();
        let pe = pe::PeFile::parse(&bytes).expect("parse");
        let dlls: Vec<_> = pe
            .imports()
            .expect("imports")
            .map(|d| d.name.to_ascii_lowercase())
            .collect();
        // user32 shares the HBRUSH encoding with gdi32 by value, so it calls no gdi32
        // function and needs no kernel32: its only import is the ntdll syscall layer.
        assert_eq!(dlls, ["ntdll.dll"]);
    }
}
