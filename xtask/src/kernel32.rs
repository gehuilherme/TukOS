use std::path::PathBuf;
use std::process::Command;

const TARGET: &str = "x86_64-pc-windows-msvc";

/// Builds `crates/kernel32` for Windows x64 (release) and returns the DLL path.
pub fn build() -> crate::Result<PathBuf> {
    crate::util::run(Command::new("cargo").args([
        "build",
        "-p",
        "kernel32",
        "--target",
        TARGET,
        "--release",
    ]))?;
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join(TARGET)
        .join("release")
        .join("kernel32.dll");
    if dll.exists() {
        Ok(dll)
    } else {
        Err(format!("kernel32.dll nao encontrada em {}", dll.display()))
    }
}

#[cfg(test)]
mod tests {
    fn built() -> Vec<u8> {
        let dll = super::build().expect("build kernel32");
        std::fs::read(&dll).expect("read kernel32.dll")
    }

    // Needs the MSVC linker, so only run on Windows hosts.
    #[test]
    #[cfg_attr(not(target_os = "windows"), ignore)]
    fn kernel32_dll_exports_win32_functions() {
        let bytes = built();
        let pe = pe::PeFile::parse(&bytes).expect("kernel32.dll e um PE valido");
        let exports: Vec<_> = pe
            .exports()
            .expect("export table")
            .map(|e| e.name.to_string())
            .collect();
        for want in ["GetStdHandle", "WriteFile", "ExitProcess"] {
            assert!(exports.iter().any(|n| n == want), "exports: {exports:?}");
        }
    }

    // Needs the MSVC linker, so only run on Windows hosts.
    #[test]
    #[cfg_attr(not(target_os = "windows"), ignore)]
    fn kernel32_imports_only_ntdll() {
        let bytes = built();
        let pe = pe::PeFile::parse(&bytes).expect("parse");
        let dlls: Vec<_> = pe
            .imports()
            .expect("imports")
            .map(|d| d.name.to_ascii_lowercase())
            .collect();
        assert_eq!(dlls, ["ntdll.dll"]);
    }
}
