//! Conformance harness: builds a real MSVC-compiled freestanding `.exe`, runs it
//! on the host Windows (reference) and on TukOS (QEMU), and compares the output.

use crate::util::{run, workspace_root};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const BEGIN: &str = "@@TUKOS-CONFORMANCE-BEGIN";
const END: &str = "@@TUKOS-CONFORMANCE-END";
const EXIT_MARK: &str = "@@TUKOS-CONFORMANCE-EXIT";
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

/// Exit status the TukOS kernel reported via the EXIT marker. `None` if absent/unparsable.
pub fn extract_exit(serial: &str) -> Option<i32> {
    let i = serial.find(EXIT_MARK)? + EXIT_MARK.len();
    serial[i..].lines().next()?.trim().parse().ok()
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

/// Compiles `conformance/crt/hello.c` WITH the static MSVC CRT (`/MT /GS-`, default
/// `mainCRTStartup` entry, libcmt) into `target/conformance/crt/hello.exe`.
fn build_crt_exe(vcvars: &Path, root: &Path) -> crate::Result<PathBuf> {
    let outdir = root.join("target").join("conformance").join("crt");
    std::fs::create_dir_all(&outdir).map_err(|e| e.to_string())?;
    let src = root.join("conformance").join("crt").join("hello.c");
    let bat = outdir.join("build-crt.bat");
    let script = format!(
        "@echo off\r\n\
         call \"{vcvars}\" >nul || exit /b 1\r\n\
         cd /d \"{outdir}\" || exit /b 1\r\n\
         cl /nologo /MT /GS- \"{src}\" /link /nologo /OUT:hello.exe || exit /b 1\r\n",
        vcvars = vcvars.display(),
        outdir = outdir.display(),
        src = src.display(),
    );
    std::fs::write(&bat, script).map_err(|e| e.to_string())?;
    run(Command::new("cmd").arg("/C").arg(&bat))?;
    let exe = outdir.join("hello.exe");
    exe.is_file()
        .then_some(exe)
        .ok_or_else(|| "crt/hello.exe nao foi gerado".to_string())
}

/// One case: run `exe` on the real Windows, boot the kernel built with `features`, compare.
fn run_case(label: &str, exe: &Path, features: &[&str]) -> crate::Result<()> {
    // Reference: the same binary on the real Windows (binds to the real DLLs).
    let reference = Command::new(exe)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("falha ao executar {}: {e}", exe.display()))?;
    let want_code = reference
        .status
        .code()
        .ok_or_else(|| format!("{label}: sem exit code no Windows"))?;
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
    let actual_code = extract_exit(&serial).ok_or_else(|| {
        format!(
            "{label}: marcador de exit ausente
--- serial ---
{serial}"
        )
    })?;
    if actual_code != want_code {
        return Err(format!(
            "{label}: exit code difere (Windows {want_code}, TukOS {actual_code})"
        ));
    }
    println!(
        "conformance[{label}]: PASS ({} bytes identicos, exit={actual_code}): {expected:?}",
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
    let heap = build_exe(&vcvars, &root, "heap", "heap", &k32_lib)?;
    let crt = build_crt_exe(&vcvars, &root)?;
    run_case("ntdll", &hello, &["conformance"])?;
    run_case("kernel32", &hello32, &["conformance32"])?;
    run_case("heap", &heap, &["conformance_heap"])?;
    run_case("crt", &crt, &["conformance_crt"])?;
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
    fn extracts_exit() {
        assert_eq!(
            extract_exit(
                "x
@@TUKOS-CONFORMANCE-EXIT 7
y"
            ),
            Some(7)
        );
        assert_eq!(extract_exit("nada"), None);
    }

    #[test]
    fn empty_body_and_missing_markers() {
        let s = "@@TUKOS-CONFORMANCE-BEGIN\n@@TUKOS-CONFORMANCE-END\n";
        assert_eq!(extract_output(s).as_deref(), Some(""));
        assert_eq!(extract_output("@@TUKOS-CONFORMANCE-BEGIN\nabc"), None);
        assert_eq!(extract_output("nada"), None);
    }
}
