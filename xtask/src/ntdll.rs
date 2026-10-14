use std::path::PathBuf;
use std::process::Command;

const TARGET: &str = "x86_64-pc-windows-msvc";

/// Builds `crates/ntdll` for Windows x64 (release) and returns the DLL path.
pub fn build() -> crate::Result<PathBuf> {
    crate::util::run(Command::new("cargo").args([
        "build",
        "-p",
        "ntdll",
        "--target",
        TARGET,
        "--release",
    ]))?;
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join(TARGET)
        .join("release")
        .join("ntdll.dll");
    if dll.exists() {
        Ok(dll)
    } else {
        Err(format!("ntdll.dll nao encontrada em {}", dll.display()))
    }
}

#[cfg(test)]
mod tests {
    fn built() -> Vec<u8> {
        let dll = super::build().expect("build ntdll");
        std::fs::read(&dll).expect("read ntdll.dll")
    }

    // Needs the MSVC linker, so only run on Windows hosts.
    #[test]
    #[cfg_attr(not(target_os = "windows"), ignore)]
    fn ntdll_dll_exports_nt_functions() {
        let bytes = built();
        let pe = pe::PeFile::parse(&bytes).expect("ntdll.dll e um PE valido");
        let exports: Vec<_> = pe
            .exports()
            .expect("export table")
            .map(|e| e.name.to_string())
            .collect();
        assert!(
            exports.iter().any(|n| n == "NtWriteFile"),
            "exports: {exports:?}"
        );
        assert!(
            exports.iter().any(|n| n == "NtTerminateProcess"),
            "exports: {exports:?}"
        );
        assert!(
            exports.iter().any(|n| n == "NtClose"),
            "exports: {exports:?}"
        );
    }

    // Needs the MSVC linker, so only run on Windows hosts.
    #[test]
    #[cfg_attr(not(target_os = "windows"), ignore)]
    fn ntdll_exports_expected_syscall_numbers() {
        let bytes = built();
        let pe = pe::PeFile::parse(&bytes).expect("parse");
        for (name, want) in [
            ("NtWriteFile", 2u32),
            ("NtTerminateProcess", 3),
            ("NtClose", 4),
        ] {
            let e = pe
                .exports()
                .unwrap()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} ausente"));
            let off = pe.rva_to_offset(e.rva).expect("rva->offset");
            let stub = bytes
                .get(off..off + 16)
                .unwrap_or_else(|| panic!("{name}: stub beyond end of file"));
            let idx = stub
                .iter()
                .position(|&b| b == 0xB8)
                .expect("mov eax,imm32 (B8)");
            let imm = u32::from_le_bytes(stub[idx + 1..idx + 5].try_into().unwrap());
            assert_eq!(imm, want, "{name}: numero de syscall");
        }
    }
}
