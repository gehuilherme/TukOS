//! Global Descriptor Table and Task State Segment encodings (Intel SDM Vol. 3A, 3.4.5 and 8.7).

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS: u16 = 0x18 | 3;
pub const USER_CS: u16 = 0x20 | 3;
pub const TSS_SEL: u16 = 0x28;

/// 64-bit code, DPL 0, present.
pub const KERNEL_CODE: u64 = 0x00AF_9A00_0000_FFFF;
/// Writable data, DPL 0, present.
pub const KERNEL_DATA: u64 = 0x00CF_9200_0000_FFFF;
/// Writable data, DPL 3, present.
pub const USER_DATA: u64 = 0x00CF_F200_0000_FFFF;
/// 64-bit code, DPL 3, present.
pub const USER_CODE: u64 = 0x00AF_FA00_0000_FFFF;

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct Tss {
    _reserved0: u32,
    pub rsp: [u64; 3],
    _reserved1: u64,
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    pub iomap_base: u16,
}

impl Tss {
    pub const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp: [0; 3],
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iomap_base: core::mem::size_of::<Tss>() as u16,
        }
    }

    pub const fn with_ist1(top: u64) -> Self {
        let mut tss = Self::new();
        tss.ist = [top, 0, 0, 0, 0, 0, 0];
        tss
    }
}

impl Default for Tss {
    fn default() -> Self {
        Self::new()
    }
}

/// Encodes a 16-byte available 64-bit TSS descriptor (type 0x9, present).
pub const fn tss_descriptor(base: u64, limit: u32) -> [u64; 2] {
    let limit = limit as u64;
    let lo = (limit & 0xFFFF)
        | ((base & 0xFF_FFFF) << 16)
        | (0x89 << 40)
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    let hi = base >> 32;
    [lo, hi]
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn dpl(desc: u64) -> u64 {
        (desc >> 45) & 0b11
    }
    const fn long_mode(desc: u64) -> bool {
        desc & (1 << 53) != 0
    }
    const fn present(desc: u64) -> bool {
        desc & (1 << 47) != 0
    }

    #[test]
    fn code_and_data_descriptors_have_expected_privilege() {
        assert!(present(KERNEL_CODE) && long_mode(KERNEL_CODE) && dpl(KERNEL_CODE) == 0);
        assert!(present(KERNEL_DATA) && dpl(KERNEL_DATA) == 0);
        assert!(present(USER_DATA) && dpl(USER_DATA) == 3);
        assert!(present(USER_CODE) && long_mode(USER_CODE) && dpl(USER_CODE) == 3);
    }

    #[test]
    fn selectors_follow_sysret_layout() {
        // SYSRET with STAR[63:48] = KERNEL_DS loads SS = base + 8 and CS = base + 16 (RPL 3).
        assert_eq!(USER_DS, (KERNEL_DS + 8) | 3);
        assert_eq!(USER_CS, (KERNEL_DS + 16) | 3);
    }

    #[test]
    fn tss_is_104_bytes() {
        assert_eq!(core::mem::size_of::<Tss>(), 104);
    }

    #[test]
    fn tss_descriptor_encodes_base_limit_and_type() {
        let [lo, hi] = tss_descriptor(0x1122_3344_5566_7788, 0x67);
        assert_eq!(lo, 0x5500_8966_7788_0067);
        assert_eq!(hi, 0x1122_3344);
    }

    #[test]
    fn with_ist1_sets_only_first_ist_slot() {
        let tss = Tss::with_ist1(0xdead_b000);
        let ist = tss.ist;
        assert_eq!(ist[0], 0xdead_b000);
        assert!(ist[1..].iter().all(|&v| v == 0));
    }
}
