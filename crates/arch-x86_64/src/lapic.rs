//! Local APIC register offsets and value encodings (xAPIC, MMIO).
//! Intel SDM Vol. 3A, chapter 11.

pub const IA32_APIC_BASE: u32 = 0x1B;

pub const REG_EOI: usize = 0xB0;
pub const REG_SPURIOUS: usize = 0xF0;
pub const REG_LVT_TIMER: usize = 0x320;
pub const REG_TIMER_INITIAL: usize = 0x380;
pub const REG_TIMER_CURRENT: usize = 0x390;
pub const REG_TIMER_DIVIDE: usize = 0x3E0;

pub const TIMER_VECTOR: u8 = 32;
pub const SPURIOUS_VECTOR: u8 = 0xFF;
pub const TIMER_DIVIDE_BY_16: u32 = 0x3;

pub const fn apic_base_phys(msr: u64) -> u64 {
    msr & 0x000F_FFFF_FFFF_F000
}

pub const fn spurious_value(vector: u8) -> u32 {
    (1 << 8) | vector as u32
}

pub const fn lvt_timer_periodic(vector: u8) -> u32 {
    (1 << 17) | vector as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apic_base_masks_flags() {
        // Default base 0xFEE00000 with BSP (bit 8) and enable (bit 11) set.
        assert_eq!(apic_base_phys(0xFEE0_0900), 0xFEE0_0000);
    }

    #[test]
    fn spurious_sets_enable_bit_and_vector() {
        assert_eq!(spurious_value(0xFF), 0x1FF);
    }

    #[test]
    fn periodic_timer_is_unmasked_with_vector() {
        let v = lvt_timer_periodic(32);
        assert_eq!(v & 0xFF, 32);
        assert!(v & (1 << 17) != 0, "modo periodico");
        assert!(v & (1 << 16) == 0, "nao mascarado");
    }
}
