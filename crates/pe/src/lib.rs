//! Parser de PE/COFF (PE32+) — logica pura, sem alocacao.
//!
//! Offsets e constantes vem da especificacao publica "PE Format" (clean room).
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
}

#[derive(Debug, Clone, Copy)]
pub struct Section<'a> {
    pub rva: u32,
    pub virtual_size: u32,
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
        // Signature (4) + COFF (20) precisam caber.
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
        // Validacao previa de todos os blocos; o iterador assume dados validos.
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
            // Proximo bloco (comeca onde o anterior terminou).
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

/// Soma `delta` ao `u64` em `bytes[rva..rva+8]`. Fora dos limites: no-op.
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
        apply_dir64(&mut buf, 12, 1); // fora dos limites: no-op
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
        pe[0x46] = 0xFF; // NumberOfSections enorme
        assert_eq!(PeFile::parse(&pe).err(), Some(PeError::SectionOutOfBounds));
        // Truncamentos nunca entram em panico.
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
        // Um bloco de 16 bytes: page 0x1000, 4 entradas (2 DIR64 + 2 ABSOLUTE de padding).
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
        // Bloco corrompido (BlockSize 4).
        pe[RAW_OFF + 4..RAW_OFF + 8].copy_from_slice(&4u32.to_le_bytes());
        let f = PeFile::parse(&pe).unwrap();
        assert!(matches!(f.relocations(), Err(PeError::BadRelocBlock)));
    }
}
