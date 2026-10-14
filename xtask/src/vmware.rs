//! Boots TukOS in VMware Workstation through `vmrun`, with the serial port written to a file.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const BOOT_WAIT: Duration = Duration::from_secs(30);

pub fn missing_vmrun_message(tried: &[PathBuf]) -> String {
    let mut msg = String::from("vmrun nao encontrado. Caminhos testados:\n");
    for p in tried {
        msg.push_str(&format!("  - {}\n", p.display()));
    }
    msg.push_str("Instale o VMware Workstation ou defina TUKOS_VMRUN.");
    msg
}

pub fn find_vmrun() -> crate::Result<PathBuf> {
    if let Ok(p) = std::env::var("TUKOS_VMRUN") {
        return Ok(PathBuf::from(p));
    }
    let candidates = vec![
        PathBuf::from(r"C:\Program Files\VMware\VMware Workstation\vmrun.exe"),
        PathBuf::from(r"C:\Program Files (x86)\VMware\VMware Workstation\vmrun.exe"),
        PathBuf::from("/usr/bin/vmrun"),
    ];
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| missing_vmrun_message(&candidates))
}

pub fn vmx_contents(disk_vmdk: &str, serial_file: &str) -> String {
    format!(
        r#".encoding = "UTF-8"
config.version = "8"
virtualHW.version = "21"
displayName = "TukOS"
guestOS = "other-64"
firmware = "efi"
memsize = "512"
numvcpus = "1"
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.fileName = "{disk_vmdk}"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.fileName = "{serial_file}"
"#
    )
}

pub fn run(esp: &Path) -> crate::Result<()> {
    let vmrun = find_vmrun()?;
    let dir = crate::util::workspace_root().join("target").join("vmware");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let raw = dir.join("tukos.img");
    crate::disk::build_uefi_image(esp, &raw)?;
    let vmdk = dir.join("tukos.vmdk");
    write_flat_vmdk_descriptor(&raw, &vmdk)?;

    let vmx = dir.join("tukos.vmx");
    let serial = dir.join("serial.log");
    std::fs::write(&vmx, vmx_contents("tukos.vmdk", "serial.log")).map_err(|e| e.to_string())?;

    crate::util::run(
        Command::new(&vmrun)
            .args(["-T", "ws", "start"])
            .arg(&vmx)
            .arg("gui"),
    )?;
    let start = Instant::now();
    let mut text = String::new();
    while start.elapsed() < BOOT_WAIT {
        std::thread::sleep(Duration::from_secs(2));
        text = std::fs::read_to_string(&serial).unwrap_or_default();
        if text.contains("TukOS pronto") || text.contains("@@TUKOS-PANIC") {
            break;
        }
    }
    let _ = Command::new(&vmrun)
        .args(["-T", "ws", "stop"])
        .arg(&vmx)
        .arg("hard")
        .status();
    println!("--- serial (VMware) ---\n{text}");
    if text.contains("TukOS pronto") {
        Ok(())
    } else {
        Err("o TukOS nao chegou a 'TukOS pronto' no VMware (veja a serial acima)".into())
    }
}

/// Writes a monolithic-flat VMDK descriptor pointing at the raw image.
fn write_flat_vmdk_descriptor(raw: &Path, vmdk: &Path) -> crate::Result<()> {
    let size = std::fs::metadata(raw).map_err(|e| e.to_string())?.len();
    let sectors = size / 512;
    let name = raw.file_name().unwrap().to_string_lossy();
    let desc = format!(
        "# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\ncreateType=\"monolithicFlat\"\n\nRW {sectors} FLAT \"{name}\" 0\n\nddb.adapterType = \"lsilogic\"\nddb.virtualHWVersion = \"21\"\n"
    );
    std::fs::write(vmdk, desc).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmx_boots_uefi_with_serial_to_file() {
        let vmx = vmx_contents("tukos.vmdk", "serial.log");
        assert!(vmx.contains("firmware = \"efi\""));
        assert!(vmx.contains("tukos.vmdk"));
        assert!(vmx.contains("serial0.fileType = \"file\""));
        assert!(vmx.contains("serial0.fileName = \"serial.log\""));
        assert!(
            !vmx.to_lowercase().contains("windows"),
            "guestOS nao pode se passar por Windows"
        );
    }

    #[test]
    fn find_vmrun_reports_missing() {
        let msg = missing_vmrun_message(&[PathBuf::from("/nao/existe/vmrun")]);
        assert!(msg.contains("vmrun"));
        assert!(msg.contains("/nao/existe/vmrun"));
        assert!(msg.contains("TUKOS_VMRUN"));
    }
}
