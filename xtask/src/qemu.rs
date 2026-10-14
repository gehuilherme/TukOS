//! Locating and driving QEMU.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// How a QEMU run ended. `isa-debug-exit` turns a write of `v` into exit code `(v << 1) | 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failure,
    Timeout,
    Other(i32),
}

pub fn outcome_from_exit_code(code: Option<i32>) -> Outcome {
    match code {
        Some(33) => Outcome::Success,
        Some(35) => Outcome::Failure,
        Some(c) => Outcome::Other(c),
        None => Outcome::Other(-1),
    }
}

/// Whether QEMU should run without a window.
pub fn headless(ci: Option<&str>, tukos_headless: Option<&str>) -> bool {
    ci.is_some_and(|v| !v.is_empty()) || tukos_headless == Some("1")
}

pub fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

pub fn missing_message(what: &str, tried: &[PathBuf], hint: &str) -> String {
    let mut msg = format!("{what} não encontrado. Caminhos testados:\n");
    for p in tried {
        msg.push_str(&format!("  - {}\n", p.display()));
    }
    msg.push_str(hint);
    msg
}

const QEMU_HINT: &str = "Instale o QEMU (Windows: `winget install --id SoftwareFreedomConservancy.QEMU -e`; \
Debian/Ubuntu: `sudo apt install qemu-system-x86`) ou defina TUKOS_QEMU.";
const OVMF_HINT: &str = "Instale o firmware OVMF (vem com o QEMU no Windows; Debian/Ubuntu: \
`sudo apt install ovmf`) ou defina TUKOS_OVMF.";

pub fn find_qemu() -> crate::Result<PathBuf> {
    if let Ok(p) = std::env::var("TUKOS_QEMU") {
        return Ok(PathBuf::from(p));
    }
    let on_path = Command::new("qemu-system-x86_64")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if on_path {
        return Ok(PathBuf::from("qemu-system-x86_64"));
    }
    let candidates = vec![PathBuf::from(r"C:\Program Files\qemu\qemu-system-x86_64.exe")];
    first_existing(&candidates).ok_or_else(|| {
        let mut tried = vec![PathBuf::from("qemu-system-x86_64 (PATH)")];
        tried.extend(candidates);
        missing_message("QEMU", &tried, QEMU_HINT)
    })
}

pub fn find_ovmf(qemu: &Path) -> crate::Result<PathBuf> {
    if let Ok(p) = std::env::var("TUKOS_OVMF") {
        return Ok(PathBuf::from(p));
    }
    let mut candidates = Vec::new();
    if let Some(dir) = qemu.parent().filter(|d| !d.as_os_str().is_empty()) {
        candidates.push(dir.join("share").join("edk2-x86_64-code.fd"));
    }
    candidates.extend(
        [
            r"C:\Program Files\qemu\share\edk2-x86_64-code.fd",
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/ovmf/OVMF.fd",
            "/usr/share/qemu/edk2-x86_64-code.fd",
        ]
        .map(PathBuf::from),
    );
    first_existing(&candidates).ok_or_else(|| missing_message("OVMF", &candidates, OVMF_HINT))
}

pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn base_args(ovmf: &Path, esp: &Path) -> Vec<String> {
    vec![
        "-machine".into(),
        "q35".into(),
        "-m".into(),
        "512M".into(),
        "-drive".into(),
        format!("if=pflash,format=raw,readonly=on,file={}", ovmf.display()),
        "-drive".into(),
        format!("format=raw,file=fat:rw:{}", esp.display()),
        "-serial".into(),
        "stdio".into(),
        "-device".into(),
        "isa-debug-exit,iobase=0xf4,iosize=0x04".into(),
        "-no-reboot".into(),
    ]
}

pub fn run_interactive(esp: &Path) -> crate::Result<()> {
    let qemu = find_qemu()?;
    let ovmf = find_ovmf(&qemu)?;
    let status = Command::new(&qemu)
        .args(base_args(&ovmf, esp))
        .status()
        .map_err(|e| format!("falha ao iniciar {}: {e}", qemu.display()))?;
    match outcome_from_exit_code(status.code()) {
        Outcome::Failure => Err("o kernel reportou falha (veja a serial acima)".into()),
        _ => Ok(()),
    }
}

pub fn run_captured(esp: &Path, timeout: Duration) -> crate::Result<(Outcome, String)> {
    let qemu = find_qemu()?;
    let ovmf = find_ovmf(&qemu)?;
    let ci = std::env::var("CI").ok();
    let forced = std::env::var("TUKOS_HEADLESS").ok();
    let display_args: &[&str] = if headless(ci.as_deref(), forced.as_deref()) {
        &["-display", "none"]
    } else {
        &[]
    };
    let mut child = Command::new(&qemu)
        .args(base_args(&ovmf, esp))
        .args(["-monitor", "none"])
        .args(display_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("falha ao iniciar {}: {e}", qemu.display()))?;
    let mut stdout = child.stdout.take().expect("stdout foi configurado como piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let status = wait_with_timeout(&mut child, timeout).map_err(|e| e.to_string())?;
    let serial = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    let outcome = match status {
        Some(s) => outcome_from_exit_code(s.code()),
        None => Outcome::Timeout,
    };
    Ok((outcome, serial))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    fn sleeper(secs: u32) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", &format!("Start-Sleep -Seconds {secs}")]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg(secs.to_string());
            c
        }
    }

    #[test]
    fn headless_only_in_ci_or_when_requested() {
        assert!(!headless(None, None));
        assert!(headless(Some("true"), None));
        assert!(!headless(Some(""), None));
        assert!(headless(None, Some("1")));
        assert!(!headless(None, Some("0")));
    }

    #[test]
    fn exit_codes_map_to_outcomes() {
        assert_eq!(outcome_from_exit_code(Some(33)), Outcome::Success);
        assert_eq!(outcome_from_exit_code(Some(35)), Outcome::Failure);
        assert_eq!(outcome_from_exit_code(Some(0)), Outcome::Other(0));
        assert_eq!(outcome_from_exit_code(None), Outcome::Other(-1));
    }

    #[test]
    fn first_existing_picks_first_present_path() {
        let present = std::env::current_exe().unwrap();
        let missing = PathBuf::from("/definitely/not/here/qemu");
        assert_eq!(first_existing(&[missing.clone(), present.clone()]), Some(present));
        assert_eq!(first_existing(&[missing]), None);
    }

    #[test]
    fn missing_message_lists_paths_and_hint() {
        let tried = vec![PathBuf::from("/a/ovmf.fd"), PathBuf::from("/b/ovmf.fd")];
        let msg = missing_message("OVMF", &tried, "instale o pacote ovmf");
        assert!(msg.contains("OVMF"));
        assert!(msg.contains("/a/ovmf.fd"));
        assert!(msg.contains("/b/ovmf.fd"));
        assert!(msg.contains("instale o pacote ovmf"));
    }

    #[test]
    fn wait_with_timeout_kills_hung_process() {
        let mut child = sleeper(30).spawn().unwrap();
        let start = Instant::now();
        let status = wait_with_timeout(&mut child, Duration::from_secs(1)).unwrap();
        assert!(status.is_none());
        assert!(start.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn wait_with_timeout_returns_status_of_finished_process() {
        let mut child = sleeper(0).spawn().unwrap();
        let status = wait_with_timeout(&mut child, Duration::from_secs(20)).unwrap();
        assert!(status.expect("processo deveria terminar").success());
    }

    #[test]
    fn base_args_wire_firmware_disk_serial_and_exit_device() {
        let args = base_args(Path::new("/fw/OVMF.fd"), Path::new("/esp"));
        let joined = args.join(" ");
        assert!(joined.contains("if=pflash,format=raw,readonly=on,file=/fw/OVMF.fd"));
        assert!(joined.contains("format=raw,file=fat:rw:/esp"));
        assert!(joined.contains("-serial stdio"));
        assert!(joined.contains("isa-debug-exit,iobase=0xf4,iosize=0x04"));
        assert!(joined.contains("-no-reboot"));
    }
}
