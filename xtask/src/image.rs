//! Builds the kernel and assembles an EFI System Partition directory for QEMU's vvfat.

use crate::util::{run, workspace_root};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LIMINE_URL: &str = "https://codeberg.org/Limine/Limine.git";
const LIMINE_BRANCH: &str = "v11.x-binary";
const LIMINE_COMMIT: &str = "deba356b4a70ee183ec3214864f0ba2ffe510b8c";

const LIMINE_CONF: &str = "\
timeout: 0
serial: yes

/TukOS
    protocol: limine
    path: boot():/boot/tukos-kernel
";

pub fn build(features: &[&str]) -> crate::Result<PathBuf> {
    let root = workspace_root();
    let kernel = build_kernel(&root, features)?;
    let bootx64 = ensure_limine(&root)?;
    let variant = if features.is_empty() { "default".to_string() } else { features.join("+") };
    let esp = root.join("target").join(format!("esp-{variant}"));
    assemble_esp(&esp, &kernel, &bootx64)?;
    Ok(esp)
}

fn build_kernel(root: &Path, features: &[&str]) -> crate::Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).args(["build", "-p", "tukos-kernel", "--target", "x86_64-unknown-none"]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    run(&mut cmd)?;
    Ok(root.join("target/x86_64-unknown-none/debug/tukos-kernel"))
}

fn ensure_limine(root: &Path) -> crate::Result<PathBuf> {
    let dir = root.join("target").join("limine").join(LIMINE_COMMIT);
    let efi = dir.join("BOOTX64.EFI");
    if efi.is_file() {
        return Ok(efi);
    }
    let _ = fs::remove_dir_all(&dir);
    run(Command::new("git").args(["clone", "--branch", LIMINE_BRANCH, "--single-branch", "--depth", "50", LIMINE_URL]).arg(&dir))?;
    run(Command::new("git").arg("-C").arg(&dir).args(["checkout", "--quiet", LIMINE_COMMIT])).map_err(|e| {
        format!("commit {LIMINE_COMMIT} não encontrado em {LIMINE_BRANCH}; atualize LIMINE_COMMIT ({e})")
    })?;
    if efi.is_file() { Ok(efi) } else { Err(format!("{} não existe após o clone do Limine", efi.display())) }
}

fn assemble_esp(esp: &Path, kernel: &Path, bootx64: &Path) -> crate::Result<()> {
    let io = |e: std::io::Error| format!("erro montando a ESP em {}: {e}", esp.display());
    let _ = fs::remove_dir_all(esp);
    fs::create_dir_all(esp.join("EFI/BOOT")).map_err(io)?;
    fs::create_dir_all(esp.join("boot")).map_err(io)?;
    fs::copy(bootx64, esp.join("EFI/BOOT/BOOTX64.EFI")).map_err(io)?;
    fs::copy(kernel, esp.join("boot/tukos-kernel")).map_err(io)?;
    fs::write(esp.join("limine.conf"), LIMINE_CONF).map_err(io)?;
    Ok(())
}
