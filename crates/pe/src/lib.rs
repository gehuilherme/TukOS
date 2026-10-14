//! PE/COFF (PE32+) parser: pure logic, no allocation.
//!
//! Offsets and constants come from the public "PE Format" specification (clean room).
#![cfg_attr(not(test), no_std)]

pub const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const SCN_MEM_READ: u32 = 0x4000_0000;
pub const SCN_MEM_WRITE: u32 = 0x8000_0000;
pub const REL_BASED_ABSOLUTE: u16 = 0;
pub const REL_BASED_DIR64: u16 = 10;

const DOS_MAGIC: u16 = 0x5A4D;
const PE_SIGNATURE: u32 = 0x0000_4550;
const MACHINE_AMD64: u16 = 0x8664;
const PE32_PLUS_MAGIC: u16 = 0x20b;
const COFF_SIZE: usize = 20;
const OPT_FIXED_SIZE: usize = 112;
const SECTION_HEADER_SIZE: usize = 40;
const RELOC_DIR_INDEX: usize = 5;
const EXPORT_DIR_INDEX: usize = 0;
const IMPORT_DIR_INDEX: usize = 1;
const IMPORT_DESC_SIZE: usize = 20;
const EXPORT_DIR_SIZE: usize = 40;
const ORDINAL_FLAG: u64 = 1 << 63;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeError {
    TooSmall,
    BadDosMagic,
    BadPeSignature,
    NotPe32Plus,
    NotAmd64,
    BadLfanew,
    SectionOutOfBounds,
    BadRelocBlock,
    BadImportTable,
    BadExportTable,
}

/// One imported function: how it is named plus the IAT slot the loader must fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportEntry<'a> {
    pub kind: ImportKind<'a>,
    /// RVA of this entry's slot in the import address table (FirstThunk + i*8).
    pub iat_rva: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind<'a> {
    Ordinal(u16),
    Name { hint: u16, name: &'a str },
}

/// Iterator over the entries of one imported DLL.
#[derive(Debug, Clone, Copy)]
pub struct ImportThunks<'a> {
    file: PeFile<'a>,
    thunk_rva: u32,
    iat_rva: u32,
    index: u32,
}

impl<'a> Iterator for ImportThunks<'a> {
    type Item = ImportEntry<'a>;

    fn next(&mut self) -> Option<ImportEntry<'a>> {
        let kind = self
            .file
            .thunk_at(self.thunk_rva, self.index)
            .ok()
            .flatten()?;
        let iat_rva = self.iat_rva.checked_add(self.index.checked_mul(8)?)?;
        self.index += 1;
        Some(ImportEntry { kind, iat_rva })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImportDll<'a> {
    pub name: &'a str,
    pub thunks: ImportThunks<'a>,
}

/// Iterator over the import descriptors (validated up front by `imports`).
#[derive(Debug, Clone, Copy)]
pub struct ImportIter<'a> {
    file: PeFile<'a>,
    dir_rva: u32,
    index: u32,
}

impl<'a> Iterator for ImportIter<'a> {
    type Item = ImportDll<'a>;

    fn next(&mut self) -> Option<ImportDll<'a>> {
        if self.dir_rva == 0 {
            return None;
        }
        let dll = self.file.import_desc(self.dir_rva, self.index).ok()??;
        self.index += 1;
        Some(dll)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Export<'a> {
    pub name: &'a str,
    pub rva: u32,
}

/// Iterator over named exports (validated up front by `exports`).
#[derive(Debug, Clone, Copy)]
pub struct ExportIter<'a> {
    file: PeFile<'a>,
    funcs_rva: u32,
    names_rva: u32,
    ords_rva: u32,
    num_funcs: u32,
    num_names: u32,
    index: u32,
}

impl<'a> Iterator for ExportIter<'a> {
    type Item = Export<'a>;

    fn next(&mut self) -> Option<Export<'a>> {
        if self.index >= self.num_names {
            return None;
        }
        let e = self.file.export_at(self, self.index).ok()?;
        self.index += 1;
        Some(e)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Section<'a> {
    pub rva: u32,
    pub virtual_size: u32,
    /// The section's raw bytes as they appear in the file. May be longer than
    /// `virtual_size` (file-alignment padding) or empty (no raw data, e.g. .bss);
    /// consumers must bound their use to `virtual_size`.
    pub raw: &'a [u8],
    pub characteristics: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reloc {
    pub rva: u32,
    pub kind: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PeFile<'a> {
    bytes: &'a [u8],
    opt_header_off: usize,
    section_table_off: usize,
    num_sections: usize,
    reloc_dir_rva: u32,
    reloc_dir_size: u32,
}

fn rd_u16(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn rd_u32(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd_u64(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off.checked_add(8)?)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

impl<'a> PeFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<PeFile<'a>, PeError> {
        if bytes.len() < 0x40 {
            return Err(PeError::TooSmall);
        }
        if rd_u16(bytes, 0) != Some(DOS_MAGIC) {
            return Err(PeError::BadDosMagic);
        }
        let lfanew = rd_u32(bytes, 0x3C).ok_or(PeError::TooSmall)? as usize;
        // Signature (4) + COFF (20) must fit.
        if lfanew
            .checked_add(4 + COFF_SIZE)
            .is_none_or(|end| end > bytes.len())
        {
            return Err(PeError::BadLfanew);
        }
        if rd_u32(bytes, lfanew) != Some(PE_SIGNATURE) {
            return Err(PeError::BadPeSignature);
        }
        let coff = lfanew + 4;
        let machine = rd_u16(bytes, coff).ok_or(PeError::TooSmall)?;
        let num_sections = rd_u16(bytes, coff + 2).ok_or(PeError::TooSmall)? as usize;
        let opt_size = rd_u16(bytes, coff + 16).ok_or(PeError::TooSmall)? as usize;
        let opt = coff + COFF_SIZE;
        let magic = rd_u16(bytes, opt).ok_or(PeError::TooSmall)?;
        if magic != PE32_PLUS_MAGIC {
            return Err(PeError::NotPe32Plus);
        }
        if machine != MACHINE_AMD64 {
            return Err(PeError::NotAmd64);
        }
        if opt_size < OPT_FIXED_SIZE || opt.checked_add(opt_size).is_none_or(|e| e > bytes.len()) {
            return Err(PeError::TooSmall);
        }
        let section_table_off = opt + opt_size;
        let table_end = num_sections
            .checked_mul(SECTION_HEADER_SIZE)
            .and_then(|n| n.checked_add(section_table_off));
        if table_end.is_none_or(|e| e > bytes.len()) {
            return Err(PeError::SectionOutOfBounds);
        }
        let num_dirs = rd_u32(bytes, opt + 108).ok_or(PeError::TooSmall)? as usize;
        let dir_off = opt + OPT_FIXED_SIZE + RELOC_DIR_INDEX * 8;
        let (reloc_dir_rva, reloc_dir_size) = if num_dirs > RELOC_DIR_INDEX
            && OPT_FIXED_SIZE + (RELOC_DIR_INDEX + 1) * 8 <= opt_size
        {
            (
                rd_u32(bytes, dir_off).ok_or(PeError::TooSmall)?,
                rd_u32(bytes, dir_off + 4).ok_or(PeError::TooSmall)?,
            )
        } else {
            (0, 0)
        };
        Ok(PeFile {
            bytes,
            opt_header_off: opt,
            section_table_off,
            num_sections,
            reloc_dir_rva,
            reloc_dir_size,
        })
    }

    pub fn entry_point_rva(&self) -> u32 {
        rd_u32(self.bytes, self.opt_header_off + 16).unwrap_or(0)
    }

    pub fn image_base(&self) -> u64 {
        rd_u64(self.bytes, self.opt_header_off + 24).unwrap_or(0)
    }

    pub fn size_of_image(&self) -> u32 {
        rd_u32(self.bytes, self.opt_header_off + 56).unwrap_or(0)
    }

    fn section_at(&self, i: usize) -> Option<Section<'a>> {
        if i >= self.num_sections {
            return None;
        }
        let h = self.section_table_off + i * SECTION_HEADER_SIZE;
        let virtual_size = rd_u32(self.bytes, h + 8)?;
        let rva = rd_u32(self.bytes, h + 12)?;
        let raw_size = rd_u32(self.bytes, h + 16)? as usize;
        let raw_ptr = rd_u32(self.bytes, h + 20)? as usize;
        let characteristics = rd_u32(self.bytes, h + 36)?;
        let raw = raw_ptr
            .checked_add(raw_size)
            .and_then(|end| self.bytes.get(raw_ptr..end))
            .unwrap_or(&[]);
        Some(Section {
            rva,
            virtual_size,
            raw,
            characteristics,
        })
    }

    pub fn sections(&self) -> impl Iterator<Item = Section<'a>> + use<'a> {
        let this = *self;
        (0..this.num_sections).filter_map(move |i| this.section_at(i))
    }

    pub fn relocations(&self) -> Result<impl Iterator<Item = Reloc> + use<'a>, PeError> {
        let data = self.reloc_dir_data()?;
        // All blocks are validated up front; the iterator assumes valid data.
        let mut pos = 0usize;
        while pos < data.len() {
            let size = rd_u32(data, pos + 4).ok_or(PeError::BadRelocBlock)? as usize;
            if size < 8
                || !size.is_multiple_of(2)
                || pos.checked_add(size).is_none_or(|e| e > data.len())
            {
                return Err(PeError::BadRelocBlock);
            }
            pos += size;
        }
        Ok(RelocIter {
            data,
            block_end: 0,
            cursor: 0,
            page: 0,
        })
    }

    /// Locates `rva` inside a section's raw data. Returns the file offset of the
    /// byte and the file offset one past the end of that section's raw data.
    fn locate(&self, rva: u32) -> Option<(usize, usize)> {
        for i in 0..self.num_sections {
            let h = self.section_table_off + i * SECTION_HEADER_SIZE;
            let vsize = rd_u32(self.bytes, h + 8)?;
            let va = rd_u32(self.bytes, h + 12)?;
            let raw_size = rd_u32(self.bytes, h + 16)?;
            let raw_ptr = rd_u32(self.bytes, h + 20)? as usize;
            // Treat a zero VirtualSize as "use the raw size".
            let extent = if vsize == 0 { raw_size } else { vsize };
            if rva < va || rva - va >= extent || rva - va >= raw_size {
                continue;
            }
            let end = raw_ptr.checked_add(raw_size as usize)?;
            if end > self.bytes.len() {
                return None;
            }
            let off = raw_ptr.checked_add((rva - va) as usize)?;
            return Some((off, end));
        }
        None
    }

    /// Maps an RVA to a file offset through the section that contains it.
    pub fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        self.locate(rva).map(|(off, _)| off)
    }

    /// File bytes from `rva` to the end of its section's raw data.
    fn tail_at(&self, rva: u32) -> Option<&'a [u8]> {
        let (off, end) = self.locate(rva)?;
        self.bytes.get(off..end)
    }

    /// NUL-terminated UTF-8 string at `rva` (must terminate inside the section).
    fn cstr_at(&self, rva: u32) -> Option<&'a str> {
        let t = self.tail_at(rva)?;
        let n = t.iter().position(|&c| c == 0)?;
        core::str::from_utf8(&t[..n]).ok()
    }

    fn data_dir(&self, index: usize) -> (u32, u32) {
        let opt_size = self.section_table_off - self.opt_header_off;
        let num_dirs = rd_u32(self.bytes, self.opt_header_off + 108).unwrap_or(0) as usize;
        if index >= num_dirs || OPT_FIXED_SIZE + (index + 1) * 8 > opt_size {
            return (0, 0);
        }
        let off = self.opt_header_off + OPT_FIXED_SIZE + index * 8;
        (
            rd_u32(self.bytes, off).unwrap_or(0),
            rd_u32(self.bytes, off + 4).unwrap_or(0),
        )
    }

    /// Reads thunk `i` of the array at `thunk_rva`. `Ok(None)` is the terminator.
    fn thunk_at(&self, thunk_rva: u32, i: u32) -> Result<Option<ImportKind<'a>>, PeError> {
        let bad = PeError::BadImportTable;
        let rva = thunk_rva
            .checked_add(i.checked_mul(8).ok_or(bad)?)
            .ok_or(bad)?;
        let thunk = rd_u64(self.tail_at(rva).ok_or(bad)?, 0).ok_or(bad)?;
        if thunk == 0 {
            return Ok(None);
        }
        if thunk & ORDINAL_FLAG != 0 {
            return Ok(Some(ImportKind::Ordinal((thunk & 0xFFFF) as u16)));
        }
        let name_rva = u32::try_from(thunk).map_err(|_| bad)?;
        let hint = rd_u16(self.tail_at(name_rva).ok_or(bad)?, 0).ok_or(bad)?;
        let name = self
            .cstr_at(name_rva.checked_add(2).ok_or(bad)?)
            .ok_or(bad)?;
        Ok(Some(ImportKind::Name { hint, name }))
    }

    /// Descriptor `idx` of the import directory; `Ok(None)` is the null terminator.
    fn import_desc(&self, dir_rva: u32, idx: u32) -> Result<Option<ImportDll<'a>>, PeError> {
        let bad = PeError::BadImportTable;
        let t = self.tail_at(dir_rva).ok_or(bad)?;
        let base = (idx as usize).checked_mul(IMPORT_DESC_SIZE).ok_or(bad)?;
        let oft = rd_u32(t, base).ok_or(bad)?;
        let name = rd_u32(t, base + 12).ok_or(bad)?;
        let ft = rd_u32(t, base + 16).ok_or(bad)?;
        if oft == 0 && name == 0 && ft == 0 {
            return Ok(None);
        }
        Ok(Some(ImportDll {
            name: self.cstr_at(name).ok_or(bad)?,
            thunks: ImportThunks {
                file: *self,
                thunk_rva: if oft != 0 { oft } else { ft },
                iat_rva: ft,
                index: 0,
            },
        }))
    }

    /// Iterates the import directory (data directory 1). The whole table is
    /// validated before the iterator is returned.
    pub fn imports(&self) -> Result<ImportIter<'a>, PeError> {
        let bad = PeError::BadImportTable;
        let (dir_rva, _) = self.data_dir(IMPORT_DIR_INDEX);
        let it = ImportIter {
            file: *self,
            dir_rva,
            index: 0,
        };
        if dir_rva == 0 {
            return Ok(it);
        }
        let mut idx = 0u32;
        while let Some(dll) = self.import_desc(dir_rva, idx)? {
            let mut i = 0u32;
            while self.thunk_at(dll.thunks.thunk_rva, i)?.is_some() {
                i += 1;
            }
            // The IAT slots must be addressable too.
            let iat_end = i.checked_mul(8).ok_or(bad)?;
            dll.thunks.iat_rva.checked_add(iat_end).ok_or(bad)?;
            idx += 1;
        }
        Ok(it)
    }

    fn export_at(&self, it: &ExportIter<'a>, i: u32) -> Result<Export<'a>, PeError> {
        let bad = PeError::BadExportTable;
        let elem = |base: u32, size: u32| -> Result<&'a [u8], PeError> {
            let rva = base
                .checked_add(i.checked_mul(size).ok_or(bad)?)
                .ok_or(bad)?;
            self.tail_at(rva).ok_or(bad)
        };
        let name_rva = rd_u32(elem(it.names_rva, 4)?, 0).ok_or(bad)?;
        let ord = u32::from(rd_u16(elem(it.ords_rva, 2)?, 0).ok_or(bad)?);
        if ord >= it.num_funcs {
            return Err(bad);
        }
        let f_rva = it.funcs_rva.checked_add(ord * 4).ok_or(bad)?;
        let rva = rd_u32(self.tail_at(f_rva).ok_or(bad)?, 0).ok_or(bad)?;
        Ok(Export {
            name: self.cstr_at(name_rva).ok_or(bad)?,
            rva,
        })
    }

    /// Iterates the named exports (data directory 0) as `(name, rva)`. The whole
    /// table is validated before the iterator is returned. Forwarder exports are
    /// not resolved: their `rva` is returned as stored.
    pub fn exports(&self) -> Result<ExportIter<'a>, PeError> {
        let bad = PeError::BadExportTable;
        let (dir_rva, dir_size) = self.data_dir(EXPORT_DIR_INDEX);
        let mut it = ExportIter {
            file: *self,
            funcs_rva: 0,
            names_rva: 0,
            ords_rva: 0,
            num_funcs: 0,
            num_names: 0,
            index: 0,
        };
        if dir_rva == 0 || dir_size == 0 {
            return Ok(it);
        }
        let t = self.tail_at(dir_rva).ok_or(bad)?;
        if t.len() < EXPORT_DIR_SIZE {
            return Err(bad);
        }
        it.num_funcs = rd_u32(t, 20).ok_or(bad)?;
        it.num_names = rd_u32(t, 24).ok_or(bad)?;
        it.funcs_rva = rd_u32(t, 28).ok_or(bad)?;
        it.names_rva = rd_u32(t, 32).ok_or(bad)?;
        it.ords_rva = rd_u32(t, 36).ok_or(bad)?;
        for i in 0..it.num_names {
            self.export_at(&it, i)?;
        }
        Ok(it)
    }

    fn reloc_dir_data(&self) -> Result<&'a [u8], PeError> {
        if self.reloc_dir_rva == 0 || self.reloc_dir_size == 0 {
            return Ok(&[]);
        }
        let (rva, size) = (self.reloc_dir_rva as u64, self.reloc_dir_size as u64);
        for s in self.sections() {
            let start = s.rva as u64;
            if rva >= start && rva + size <= start + s.raw.len() as u64 {
                let off = (rva - start) as usize;
                return Ok(&s.raw[off..off + size as usize]);
            }
        }
        Err(PeError::BadRelocBlock)
    }
}

struct RelocIter<'a> {
    data: &'a [u8],
    block_end: usize,
    cursor: usize,
    page: u32,
}

impl Iterator for RelocIter<'_> {
    type Item = Reloc;

    fn next(&mut self) -> Option<Reloc> {
        loop {
            if self.cursor + 2 <= self.block_end {
                let e = rd_u16(self.data, self.cursor)?;
                self.cursor += 2;
                return Some(Reloc {
                    rva: self.page.wrapping_add((e & 0x0FFF) as u32),
                    kind: e >> 12,
                });
            }
            // Next block (starts where the previous one ended).
            let start = self.block_end;
            if start + 8 > self.data.len() {
                return None;
            }
            self.page = rd_u32(self.data, start)?;
            let size = rd_u32(self.data, start + 4)? as usize;
            self.block_end = start + size;
            self.cursor = start + 8;
        }
    }
}

pub const fn characteristics_to_flags(ch: u32) -> (bool, bool) {
    (ch & SCN_MEM_WRITE != 0, ch & SCN_MEM_EXECUTE != 0)
}

/// Adds `delta` to the `u64` at `bytes[rva..rva+8]`. Out of bounds: no-op.
pub fn apply_dir64(bytes: &mut [u8], rva: u32, delta: i64) {
    let off = rva as usize;
    let Some(end) = off.checked_add(8) else {
        return;
    };
    if let Some(s) = bytes.get_mut(off..end) {
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        let v = u64::from_le_bytes(a).wrapping_add(delta as u64);
        s.copy_from_slice(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPT_SIZE: usize = OPT_FIXED_SIZE + 16 * 8;
    const SEC_OFF: usize = 0x40 + 4 + COFF_SIZE + OPT_SIZE;
    const RAW_OFF: usize = 0x200;

    fn put(v: &mut [u8], off: usize, b: &[u8]) {
        v[off..off + b.len()].copy_from_slice(b);
    }

    fn build_pe_impl(
        image_base: u64,
        entry_rva: u32,
        text: &[u8],
        text_vsize: u32,
        ch: u32,
    ) -> Vec<u8> {
        let mut v = vec![0u8; RAW_OFF + text.len()];
        put(&mut v, 0, &DOS_MAGIC.to_le_bytes());
        put(&mut v, 0x3C, &0x40u32.to_le_bytes());
        put(&mut v, 0x40, &PE_SIGNATURE.to_le_bytes());
        let coff = 0x44;
        put(&mut v, coff, &MACHINE_AMD64.to_le_bytes());
        put(&mut v, coff + 2, &1u16.to_le_bytes());
        put(&mut v, coff + 16, &(OPT_SIZE as u16).to_le_bytes());
        let opt = coff + COFF_SIZE;
        put(&mut v, opt, &PE32_PLUS_MAGIC.to_le_bytes());
        put(&mut v, opt + 16, &entry_rva.to_le_bytes());
        put(&mut v, opt + 24, &image_base.to_le_bytes());
        put(&mut v, opt + 56, &0x2000u32.to_le_bytes());
        put(&mut v, opt + 108, &16u32.to_le_bytes());
        let s = SEC_OFF;
        put(&mut v, s, b".text\0\0\0");
        put(&mut v, s + 8, &text_vsize.to_le_bytes());
        put(&mut v, s + 12, &0x1000u32.to_le_bytes());
        put(&mut v, s + 16, &(text.len() as u32).to_le_bytes());
        put(&mut v, s + 20, &(RAW_OFF as u32).to_le_bytes());
        put(&mut v, s + 36, &ch.to_le_bytes());
        put(&mut v, RAW_OFF, text);
        v
    }

    fn build_pe(image_base: u64, entry_rva: u32, text: &[u8], text_vsize: u32, ch: u32) -> Vec<u8> {
        build_pe_impl(image_base, entry_rva, text, text_vsize, ch)
    }

    #[test]
    fn parses_headers_and_section() {
        let pe = build_pe(
            0x40_0000,
            0x1000,
            &[0x90, 0xC3],
            2,
            SCN_MEM_EXECUTE | SCN_MEM_READ,
        );
        let f = PeFile::parse(&pe).unwrap();
        assert_eq!(f.image_base(), 0x40_0000);
        assert_eq!(f.entry_point_rva(), 0x1000);
        assert_eq!(f.size_of_image(), 0x2000);
        let secs: Vec<_> = f.sections().collect();
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].rva, 0x1000);
        assert_eq!(secs[0].raw, &[0x90, 0xC3]);
        assert!(secs[0].characteristics & SCN_MEM_EXECUTE != 0);
        assert_eq!(f.relocations().unwrap().count(), 0);
    }

    #[test]
    fn characteristics_to_flags_maps_write_and_exec() {
        assert_eq!(
            characteristics_to_flags(SCN_MEM_EXECUTE | SCN_MEM_READ),
            (false, true)
        );
        assert_eq!(
            characteristics_to_flags(SCN_MEM_WRITE | SCN_MEM_READ),
            (true, false)
        );
        assert_eq!(characteristics_to_flags(SCN_MEM_READ), (false, false));
    }

    #[test]
    fn reloc_dir64_applies_delta() {
        let mut buf = [0u8; 16];
        buf[8..16].copy_from_slice(&0x1000u64.to_le_bytes());
        apply_dir64(&mut buf, 8, 0x5000);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x6000);
        apply_dir64(&mut buf, 8, -0x1000);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x5000);
        apply_dir64(&mut buf, 12, 1); // out of bounds: no-op
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(matches!(PeFile::parse(&[]), Err(PeError::TooSmall)));
        assert!(matches!(
            PeFile::parse(&[0; 128]),
            Err(PeError::BadDosMagic)
        ));
        let mut pe = build_pe(0x40_0000, 0x1000, &[0x90], 1, SCN_MEM_EXECUTE);
        pe[0x3C] = 0xFF;
        pe[0x3D] = 0xFF;
        assert!(matches!(PeFile::parse(&pe), Err(PeError::BadLfanew)));
    }

    #[test]
    fn parse_rejects_other_headers_and_truncation() {
        let good = build_pe(0x40_0000, 0x1000, &[0x90], 1, SCN_MEM_EXECUTE);
        let mut pe = good.clone();
        pe[0x40] = 0;
        assert_eq!(PeFile::parse(&pe).err(), Some(PeError::BadPeSignature));
        let mut pe = good.clone();
        pe[0x44] = 0;
        assert_eq!(PeFile::parse(&pe).err(), Some(PeError::NotAmd64));
        let mut pe = good.clone();
        pe[0x58] = 0x0b;
        pe[0x59] = 0x01;
        assert_eq!(PeFile::parse(&pe).err(), Some(PeError::NotPe32Plus));
        let mut pe = good.clone();
        pe[0x46] = 0xFF; // huge NumberOfSections
        assert_eq!(PeFile::parse(&pe).err(), Some(PeError::SectionOutOfBounds));
        // Truncations never panic.
        for n in 0..good.len() {
            let _ = PeFile::parse(&good[..n]).map(|f| f.sections().count());
        }
    }

    #[test]
    fn section_outside_file_has_empty_raw() {
        let mut pe = build_pe(0x40_0000, 0x1000, &[0x90], 1, SCN_MEM_EXECUTE);
        pe[SEC_OFF + 20..SEC_OFF + 24].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        let f = PeFile::parse(&pe).unwrap();
        assert!(f.sections().next().unwrap().raw.is_empty());
    }

    #[test]
    fn relocations_are_iterated() {
        // One 16-byte block: page 0x1000, 4 entries (2 DIR64 + 2 ABSOLUTE padding).
        let mut blk = Vec::new();
        blk.extend_from_slice(&0x1000u32.to_le_bytes());
        blk.extend_from_slice(&16u32.to_le_bytes());
        for e in [0xA010u16, 0xA020, 0x0000, 0x0000] {
            blk.extend_from_slice(&e.to_le_bytes());
        }
        let mut pe = build_pe(0x40_0000, 0x1000, &blk, 16, SCN_MEM_READ);
        let opt = 0x44 + COFF_SIZE;
        let dir = opt + OPT_FIXED_SIZE + 5 * 8;
        pe[dir..dir + 4].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[dir + 4..dir + 8].copy_from_slice(&16u32.to_le_bytes());
        let f = PeFile::parse(&pe).unwrap();
        let r: Vec<_> = f.relocations().unwrap().collect();
        assert_eq!(r.len(), 4);
        assert_eq!(
            r[0],
            Reloc {
                rva: 0x1010,
                kind: REL_BASED_DIR64
            }
        );
        assert_eq!(
            r[1],
            Reloc {
                rva: 0x1020,
                kind: REL_BASED_DIR64
            }
        );
        assert_eq!(r[2].kind, REL_BASED_ABSOLUTE);
        // Corrupted block (BlockSize 4).
        pe[RAW_OFF + 4..RAW_OFF + 8].copy_from_slice(&4u32.to_le_bytes());
        let f = PeFile::parse(&pe).unwrap();
        assert!(matches!(f.relocations(), Err(PeError::BadRelocBlock)));
    }

    fn set_dir(pe: &mut [u8], idx: usize, rva: u32, size: u32) {
        let d = 0x44 + COFF_SIZE + OPT_FIXED_SIZE + idx * 8;
        pe[d..d + 4].copy_from_slice(&rva.to_le_bytes());
        pe[d + 4..d + 8].copy_from_slice(&size.to_le_bytes());
    }

    /// Blob at RVA 0x1000: descriptor@0, INT@0x40, IAT@0x60, "OTHER.DLL"@0x80,
    /// IMAGE_IMPORT_BY_NAME (hint 5, "Foo")@0x90.
    fn import_blob() -> Vec<u8> {
        let mut b = vec![0u8; 0xA0];
        put(&mut b, 0, &0x1040u32.to_le_bytes()); // OriginalFirstThunk
        put(&mut b, 12, &0x1080u32.to_le_bytes()); // Name
        put(&mut b, 16, &0x1060u32.to_le_bytes()); // FirstThunk
        // Descriptor@0x14 stays zero (terminator).
        for base in [0x40, 0x60] {
            put(&mut b, base, &0x1090u64.to_le_bytes());
            put(&mut b, base + 8, &(ORDINAL_FLAG | 7).to_le_bytes());
        }
        put(&mut b, 0x80, b"OTHER.DLL\0");
        put(&mut b, 0x90, &5u16.to_le_bytes());
        put(&mut b, 0x92, b"Foo\0");
        b
    }

    fn build_pe_with_imports() -> Vec<u8> {
        let blob = import_blob();
        let mut pe = build_pe(0x40_0000, 0x1000, &blob, blob.len() as u32, SCN_MEM_READ);
        set_dir(&mut pe, IMPORT_DIR_INDEX, 0x1000, 0x28);
        pe
    }

    fn build_pe_with_bad_import_dir() -> Vec<u8> {
        let mut pe = build_pe_with_imports();
        set_dir(&mut pe, IMPORT_DIR_INDEX, 0x9000, 0x28);
        pe
    }

    /// Blob at RVA 0x1000: directory@0, functions@0x40, names@0x50,
    /// ordinals@0x60, "Bar"@0x70.
    fn export_blob() -> Vec<u8> {
        let mut b = vec![0u8; 0x80];
        put(&mut b, 16, &1u32.to_le_bytes()); // Base
        put(&mut b, 20, &1u32.to_le_bytes()); // NumberOfFunctions
        put(&mut b, 24, &1u32.to_le_bytes()); // NumberOfNames
        put(&mut b, 28, &0x1040u32.to_le_bytes());
        put(&mut b, 32, &0x1050u32.to_le_bytes());
        put(&mut b, 36, &0x1060u32.to_le_bytes());
        put(&mut b, 0x40, &0x1234u32.to_le_bytes());
        put(&mut b, 0x50, &0x1070u32.to_le_bytes());
        put(&mut b, 0x60, &0u16.to_le_bytes());
        put(&mut b, 0x70, b"Bar\0");
        b
    }

    fn build_pe_with_exports() -> Vec<u8> {
        let blob = export_blob();
        let mut pe = build_pe(0x40_0000, 0x1000, &blob, blob.len() as u32, SCN_MEM_READ);
        set_dir(&mut pe, EXPORT_DIR_INDEX, 0x1000, 0x80);
        pe
    }

    fn build_pe_with_bad_export_dir() -> Vec<u8> {
        let mut pe = build_pe_with_exports();
        // Names array RVA points outside every section.
        pe[RAW_OFF + 32..RAW_OFF + 36].copy_from_slice(&0x9000u32.to_le_bytes());
        pe
    }

    #[test]
    fn parses_import_table() {
        let pe = build_pe_with_imports();
        let f = PeFile::parse(&pe).unwrap();
        let dlls: Vec<_> = f.imports().unwrap().collect();
        assert_eq!(dlls.len(), 1);
        assert_eq!(dlls[0].name, "OTHER.DLL");
        let e: Vec<_> = dlls[0].thunks.collect();
        assert_eq!(e.len(), 2);
        assert_eq!(
            e[0],
            ImportEntry {
                kind: ImportKind::Name {
                    hint: 5,
                    name: "Foo"
                },
                iat_rva: 0x1060
            }
        );
        assert_eq!(
            e[1],
            ImportEntry {
                kind: ImportKind::Ordinal(7),
                iat_rva: 0x1068
            }
        );
    }

    #[test]
    fn no_import_dir_is_empty() {
        let pe = build_pe(0x40_0000, 0x1000, &[0x90], 1, SCN_MEM_READ);
        let f = PeFile::parse(&pe).unwrap();
        assert_eq!(f.imports().unwrap().count(), 0);
        assert_eq!(f.exports().unwrap().count(), 0);
    }

    #[test]
    fn parses_export_table() {
        let pe = build_pe_with_exports();
        let f = PeFile::parse(&pe).unwrap();
        let exps: Vec<_> = f.exports().unwrap().collect();
        assert!(exps.iter().any(|e| e.name == "Bar" && e.rva == 0x1234));
    }

    #[test]
    fn rva_to_offset_maps_and_bounds() {
        let pe = build_pe_with_exports();
        let f = PeFile::parse(&pe).unwrap();
        assert_eq!(f.rva_to_offset(0x1010), Some(RAW_OFF + 0x10));
        assert_eq!(f.rva_to_offset(0xFFF), None);
        assert_eq!(f.rva_to_offset(0x1080), None);
    }

    #[test]
    fn import_parse_rejects_malformed() {
        let pe = build_pe_with_bad_import_dir();
        let f = PeFile::parse(&pe).unwrap();
        assert!(matches!(f.imports().err(), Some(PeError::BadImportTable)));
    }

    #[test]
    fn export_parse_bounds() {
        let pe = build_pe_with_bad_export_dir();
        let f = PeFile::parse(&pe).unwrap();
        assert!(matches!(f.exports().err(), Some(PeError::BadExportTable)));
    }

    #[test]
    fn import_export_never_panic_on_corruption() {
        for pe in [build_pe_with_imports(), build_pe_with_exports()] {
            for pos in RAW_OFF..pe.len() {
                for val in [0x00u8, 0xFF, 0x80] {
                    let mut m = pe.clone();
                    m[pos] = val;
                    let f = PeFile::parse(&m).unwrap();
                    if let Ok(it) = f.imports() {
                        for d in it {
                            let _ = d.thunks.count();
                        }
                    }
                    if let Ok(it) = f.exports() {
                        let _ = it.count();
                    }
                }
            }
            for n in 0..pe.len() {
                if let Ok(f) = PeFile::parse(&pe[..n]) {
                    let _ = f.imports().map(|i| i.count());
                    let _ = f.exports().map(|i| i.count());
                }
            }
        }
    }
}
