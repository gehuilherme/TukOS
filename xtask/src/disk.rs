//! Builds a raw UEFI-bootable disk image (GPT + one FAT32 EFI System Partition).

use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::path::Path;

const DISK_SIZE: u64 = 64 * 1024 * 1024;
const SECTOR: u64 = 512;

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Creates a 64 MiB raw image at `out`: protective MBR, GPT, and an EFI System
/// partition (starting at 1 MiB) formatted FAT32 holding the tree of `esp_dir`.
pub fn build_uefi_image(esp_dir: &Path, out: &Path) -> crate::Result<()> {
    let mut file: File = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(out)
        .map_err(s)?;
    file.set_len(DISK_SIZE).map_err(s)?;

    let sectors = u32::try_from(DISK_SIZE / SECTOR - 1).map_err(s)?;
    gpt::mbr::ProtectiveMBR::with_lb_size(sectors)
        .overwrite_lba0(&mut file)
        .map_err(s)?;

    let mut disk = gpt::GptConfig::new()
        .writable(true)
        .create_from_device(file, None)
        .map_err(s)?;
    // Leave room for the GPT structures at both ends of the disk.
    let part_size = DISK_SIZE - 2 * 1024 * 1024;
    let id = disk
        .add_partition("EFI System", part_size, gpt::partition_types::EFI, 0, None)
        .map_err(s)?;
    let part = disk
        .partitions()
        .get(&id)
        .ok_or("particao nao criada")?
        .clone();
    let start = part.bytes_start(*disk.logical_block_size()).map_err(s)?;
    let len = part.bytes_len(*disk.logical_block_size()).map_err(s)?;
    let mut file = disk.write().map_err(s)?;

    // Build the FAT32 filesystem in memory, then write it at the partition offset.
    let mut buf = Cursor::new(vec![0u8; usize::try_from(len).map_err(s)?]);
    fatfs::format_volume(
        &mut buf,
        fatfs::FormatVolumeOptions::new()
            .fat_type(fatfs::FatType::Fat32)
            .volume_label(*b"TUKOS      "),
    )
    .map_err(s)?;
    {
        let fs = fatfs::FileSystem::new(&mut buf, fatfs::FsOptions::new()).map_err(s)?;
        copy_tree(esp_dir, &fs.root_dir(), out)?;
        fs.unmount().map_err(s)?;
    }
    file.seek(SeekFrom::Start(start)).map_err(s)?;
    file.write_all(buf.get_ref()).map_err(s)?;
    file.flush().map_err(s)?;
    Ok(())
}

fn copy_tree<T: fatfs::ReadWriteSeek>(
    src: &Path,
    dst: &fatfs::Dir<'_, T>,
    skip: &Path,
) -> crate::Result<()> {
    for entry in fs::read_dir(src).map_err(s)? {
        let entry = entry.map_err(s)?;
        if entry.path() == skip {
            continue; // the image being written may live inside the source tree
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map_err(s)?.is_dir() {
            let sub = dst.create_dir(&name).map_err(s)?;
            copy_tree(&entry.path(), &sub, skip)?;
        } else {
            let mut f = dst.create_file(&name).map_err(s)?;
            f.truncate().map_err(s)?;
            f.write_all(&fs::read(entry.path()).map_err(s)?)
                .map_err(s)?;
            f.flush().map_err(s)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_has_gpt_and_bootloader() {
        let dir = std::env::temp_dir().join("tukos-disk-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("EFI/BOOT")).unwrap();
        std::fs::write(dir.join("EFI/BOOT/BOOTX64.EFI"), b"fake").unwrap();
        let out = dir.join("disk.img");
        build_uefi_image(&dir, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        // Protective MBR signature and GPT header at LBA 1.
        assert_eq!(&bytes[510..512], &[0x55, 0xAA]);
        assert_eq!(&bytes[512..520], b"EFI PART");
    }
}
