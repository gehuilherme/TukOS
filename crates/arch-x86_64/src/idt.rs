//! Interrupt Descriptor Table entry encoding (Intel SDM Vol. 3A, 6.14.1).

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub selector: u16,
    pub ist: u8,
    pub type_attr: u8,
    pub offset_mid: u16,
    pub offset_high: u32,
    _reserved: u32,
}

impl IdtEntry {
    pub const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        type_attr: 0,
        offset_mid: 0,
        offset_high: 0,
        _reserved: 0,
    };

    /// Present 64-bit interrupt gate.
    pub const fn new(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: ist & 0b111,
            type_attr: 0x8E | ((dpl & 0b11) << 5),
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            _reserved: 0,
        }
    }

    pub fn handler(&self) -> u64 {
        self.offset_low as u64 | (self.offset_mid as u64) << 16 | (self.offset_high as u64) << 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_is_16_bytes() {
        assert_eq!(core::mem::size_of::<IdtEntry>(), 16);
    }

    #[test]
    fn new_splits_handler_address_and_sets_gate_type() {
        let e = IdtEntry::new(0x1122_3344_5566_7788, 0x08, 1, 0);
        assert_eq!(e.offset_low, 0x7788);
        assert_eq!(e.offset_mid, 0x5566);
        assert_eq!(e.offset_high, 0x1122_3344);
        assert_eq!(e.selector, 0x08);
        assert_eq!(e.ist, 1);
        assert_eq!(e.type_attr, 0x8E);
        assert_eq!(e.handler(), 0x1122_3344_5566_7788);
    }

    #[test]
    fn user_callable_gate_has_dpl3() {
        assert_eq!(IdtEntry::new(0x1000, 0x08, 0, 3).type_attr, 0xEE);
    }

    #[test]
    fn missing_entry_is_not_present() {
        assert_eq!(IdtEntry::MISSING.type_attr & 0x80, 0);
    }
}
