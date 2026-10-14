//! Conformance harness: builds a real MSVC-compiled freestanding `.exe`, runs it
//! on the host Windows (reference) and on TukOS (QEMU), and compares the output.

use crate::util::{run, workspace_root};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const BEGIN: &str = "@@TUKOS-CONFORMANCE-BEGIN";
const END: &str = "@@TUKOS-CONFORMANCE-END";
const TIMEOUT: Duration = Duration::from_secs(60);

/// Text the TukOS kernel printed between the BEGIN and END markers, with CRLF
/// normalised to LF. `None` if either marker is missing.
pub fn extract_output(serial: &str) -> Option<String> {
    let b = serial.find(BEGIN)? + BEGIN.len();
    let rest = &serial[b..];
    // The BEGIN line itself ends at the next newline.
    let nl = rest.find('\n')? + 1;
    let body = &rest[nl..];
    let e = body.find(END)?;
    Some(body[..e].replace("\r\n", "\n"))
}

/// Locates `vcvars64.bat` via vswhere (VC tools component). `None` when MSVC is absent.
fn find_vcvars() -> Option<PathBuf> {
    let pf86 = std::env::var_os("ProgramFiles(x86)")?;
    let vswhere = Path::new(&pf86).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }
    let out = Command::new(vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let install = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if install.is_empty() {
        return None;
    }
    let vcvars = Path::new(&install).join(r"VC\Auxiliary\Build\vcvars64.bat");
    vcvars.is_file().then_some(vcvars)
}

/// Compiles `conformance/<dir>/<name>.c` and links it against `lib` into `target/conformance/<name>.exe`.
fn build_exe(
    vcvars: &Path,
    root: &Path,
    dir: &str,
    name: &str,
    lib: &Path,
) -> crate::Result<PathBuf> {
    if !lib.is_file() {
        return Err(format!("import lib ausente: {}", lib.display()));
    }
    let outdir = root.join("target").join("conformance");
    std::fs::create_dir_all(&outdir).map_err(|e| e.to_string())?;
    let src = root.join("conformance").join(dir).join(format!("{name}.c"));
    let bat = outdir.join(format!("build-{name}.bat"));
    let script = format!(
        "@echo off\r\n\
         call \"{vcvars}\" >nul || exit /b 1\r\n\
         cd /d \"{outdir}\" || exit /b 1\r\n\
         cl /nologo /c /GS- /O1 \"{src}\" || exit /b 1\r\n\
         link /nologo /NODEFAULTLIB /SUBSYSTEM:CONSOLE /ENTRY:tukos_main /OUT:{name}.exe {name}.obj \"{lib}\" || exit /b 1\r\n",
        vcvars = vcvars.display(),
        outdir = outdir.display(),
        src = src.display(),
        lib = lib.display(),
    );
    std::fs::write(&bat, script).map_err(|e| e.to_string())?;
    run(Command::new("cmd").arg("/C").arg(&bat))?;
    let exe = outdir.join(format!("{name}.exe"));
    exe.is_file()
        .then_some(exe)
        .ok_or_else(|| format!("{name}.exe nao foi gerado"))
}

/// One case: run `exe` on the real Windows, boot the kernel built with `features`, compare.
fn run_case(label: &str, exe: &Path, features: &[&str]) -> crate::Result<()> {
    // Reference: the same binary on the real Windows (binds to the real DLLs).
    let reference = Command::new(exe)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("falha ao executar {}: {e}", exe.display()))?;
    if !reference.status.success() {
        return Err(format!(
            "{label}: {} no Windows terminou com {}",
            exe.display(),
            reference.status
        ));
    }
    let expected = String::from_utf8_lossy(&reference.stdout).replace("\r\n", "\n");

    // TukOS: the kernel embeds the exes (build.rs) and runs one in conformance mode.
    let esp = crate::image::build(features)?;
    let (outcome, serial) = crate::qemu::run_captured(&esp, TIMEOUT)?;
    let actual = extract_output(&serial).ok_or_else(|| {
        format!(
            "{label}: marcadores de conformidade ausentes (QEMU: {outcome:?})
--- serial ---
{serial}"
        )
    })?;
    if outcome != crate::qemu::Outcome::Success {
        return Err(format!(
            "{label}: QEMU terminou com {outcome:?}
--- serial ---
{serial}"
        ));
    }
    if actual != expected {
        return Err(format!(
            "{label}: saidas diferem
--- Windows ({} bytes) ---
{expected:?}
--- TukOS ({} bytes) ---
{actual:?}",
            expected.len(),
            actual.len()
        ));
    }
    println!(
        "conformance[{label}]: PASS ({} bytes identicos): {expected:?}",
        expected.len()
    );
    Ok(())
}

pub fn run_conformance() -> crate::Result<()> {
    if !cfg!(windows) {
        println!("cargo:warning=conformance: so roda no Windows com MSVC; pulando");
        return Ok(());
    }
    let Some(vcvars) = find_vcvars() else {
        println!("cargo:warning=conformance: MSVC (vcvars64.bat) nao encontrado; pulando");
        return Ok(());
    };
    let root = workspace_root();
    // Both DLLs must exist (kernel/build.rs embeds them) before either kernel build.
    let ntdll_lib = crate::ntdll::build()?.with_file_name("ntdll.dll.lib");
    let k32_lib = crate::kernel32::build()?.with_file_name("kernel32.dll.lib");
    // hello.exe links ntdll only; hello32.exe links kernel32 only (kernel32 imports ntdll itself).
    let hello = build_exe(&vcvars, &root, "hello", "hello", &ntdll_lib)?;
    let hello32 = build_exe(&vcvars, &root, "hello32", "hello32", &k32_lib)?;
    run_case("ntdll", &hello, &["conformance"])?;
    run_case("kernel32", &hello32, &["conformance32"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_between_markers() {
        let s = "boot noise\r\n@@TUKOS-CONFORMANCE-BEGIN\r\nHello\r\n@@TUKOS-CONFORMANCE-END\r\nx";
        assert_eq!(extract_output(s).as_deref(), Some("Hello\n"));
    }

    #[test]
    fn empty_body_and_missing_markers() {
        let s = "@@TUKOS-CONFORMANCE-BEGIN\n@@TUKOS-CONFORMANCE-END\n";
        assert_eq!(extract_output(s).as_deref(), Some(""));
        assert_eq!(extract_output("@@TUKOS-CONFORMANCE-BEGIN\nabc"), None);
        assert_eq!(extract_output("nada"), None);
    }
}
