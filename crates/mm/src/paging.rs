//! Virtual-address decomposition and x86-64 page-table entry encoding.
//! Intel SDM Vol. 3A, section 4.5 (4-Level Paging).

pub const PAGE_SIZE: u64 = 4096;

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000; // bits 51..12

/// `[p4, p3, p2, p1]` table indices for a virtual address.
pub fn page_table_indices(virt: u64) -> [usize; 4] {
    [
        ((virt >> 39) & 0x1FF) as usize,
        ((virt >> 30) & 0x1FF) as usize,
        ((virt >> 21) & 0x1FF) as usize,
        ((virt >> 12) & 0x1FF) as usize,
    ]
}

pub fn page_offset(virt: u64) -> u64 {
    virt & 0xFFF
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageFlags(u64);

impl PageFlags {
    pub const PRESENT: PageFlags = PageFlags(1 << 0);
    pub const WRITABLE: PageFlags = PageFlags(1 << 1);
    pub const USER: PageFlags = PageFlags(1 << 2);
    pub const HUGE: PageFlags = PageFlags(1 << 7);
    pub const NO_EXECUTE: PageFlags = PageFlags(1 << 63);

    pub const fn empty() -> PageFlags {
        PageFlags(0)
    }

    pub const fn union(self, other: PageFlags) -> PageFlags {
        PageFlags(self.0 | other.0)
    }

    pub const fn contains(self, other: PageFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u64 {
        self.0
    }
}

pub fn make_entry(frame_phys: u64, flags: PageFlags) -> u64 {
    (frame_phys & ADDR_MASK) | flags.bits()
}

pub fn entry_frame(entry: u64) -> u64 {
    entry & ADDR_MASK
}

pub fn entry_flags(entry: u64) -> PageFlags {
    PageFlags(entry & !ADDR_MASK)
}

pub fn entry_present(entry: u64) -> bool {
    entry & PageFlags::PRESENT.bits() != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_split_canonical_higher_half() {
        // Kernel base 0xffffffff80000000.
        let idx = page_table_indices(0xffff_ffff_8000_0000);
        assert_eq!(idx, [511, 510, 0, 0]);
        assert_eq!(page_offset(0xffff_ffff_8000_0abc), 0xabc);
    }

    #[test]
    fn indices_split_low_address() {
        assert_eq!(page_table_indices(0x0000_0000_0000_0000), [0, 0, 0, 0]);
        assert_eq!(page_table_indices(0x0000_0000_0020_1000), [0, 0, 1, 1]);
    }

    #[test]
    fn make_and_read_entry_roundtrip() {
        let frame = 0x1234_5000u64;
        let flags = PageFlags::PRESENT.union(PageFlags::WRITABLE);
        let e = make_entry(frame, flags);
        assert_eq!(entry_frame(e), frame);
        assert!(entry_present(e));
        assert!(entry_flags(e).contains(PageFlags::WRITABLE));
        assert!(!entry_flags(e).contains(PageFlags::USER));
    }

    #[test]
    fn no_execute_is_bit_63() {
        let e = make_entry(0x1000, PageFlags::PRESENT.union(PageFlags::NO_EXECUTE));
        assert!(e & (1 << 63) != 0);
        assert!(entry_flags(e).contains(PageFlags::NO_EXECUTE));
    }

    #[test]
    fn entry_frame_masks_flags_and_nx() {
        // Present+writable+NX on frame 0x2000: extracting the frame drops all of them.
        let e = make_entry(0x2000, PageFlags::PRESENT.union(PageFlags::WRITABLE).union(PageFlags::NO_EXECUTE));
        assert_eq!(entry_frame(e), 0x2000);
    }

    #[test]
    fn not_present_entry_reads_absent() {
        assert!(!entry_present(0));
        assert!(!entry_present(make_entry(0x1000, PageFlags::WRITABLE)));
    }
}
