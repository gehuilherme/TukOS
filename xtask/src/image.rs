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
    // The kernel's build.rs embeds target/x86_64-pc-windows-msvc/release/ntdll.dll
    // when present, so build it first. Without MSVC (e.g. Linux) this fails and
    // the kernel simply embeds an empty image.
    if let Err(e) = crate::ntdll::build() {
        eprintln!("aviso: ntdll nao compilado ({e}); kernel embute ntdll vazio");
    }
    if let Err(e) = crate::kernel32::build() {
        eprintln!("aviso: kernel32 nao compilado ({e}); kernel embute kernel32 vazio");
    }
    let kernel = build_kernel(&root, features)?;
    let bootx64 = ensure_limine(&root)?;
    let variant = if features.is_empty() {
        "default".to_string()
    } else {
        features.join("+")
    };
    let esp = root.join("target").join(format!("esp-{variant}"));
    assemble_esp(&esp, &kernel, &bootx64)?;
    Ok(esp)
}

fn build_kernel(root: &Path, features: &[&str]) -> crate::Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).args([
        "build",
        "-p",
        "tukos-kernel",
        "--target",
        "x86_64-unknown-none",
    ]);
    // DIAGNOSTICO (plano 12): TUKOS_EXTRA_FEATURES="sched-debug,repro-sched-pf" liga features extras.
    let mut feats: Vec<String> = features.iter().map(|f| f.to_string()).collect();
    if let Ok(extra) = std::env::var("TUKOS_EXTRA_FEATURES") {
        feats.extend(extra.split(',').filter(|f| !f.is_empty()).map(String::from));
    }
    if !feats.is_empty() {
        cmd.args(["--features", &feats.join(",")]);
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
    run(Command::new("git")
        .args([
            "clone",
            "--branch",
            LIMINE_BRANCH,
            "--single-branch",
            "--depth",
            "50",
            LIMINE_URL,
        ])
        .arg(&dir))?;
    run(Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["checkout", "--quiet", LIMINE_COMMIT]))
    .map_err(|e| {
        format!(
            "commit {LIMINE_COMMIT} não encontrado em {LIMINE_BRANCH}; atualize LIMINE_COMMIT ({e})"
        )
    })?;
    if efi.is_file() {
        Ok(efi)
    } else {
        Err(format!(
            "{} não existe após o clone do Limine",
            efi.display()
        ))
    }
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
