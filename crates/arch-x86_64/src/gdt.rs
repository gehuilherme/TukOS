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

pub const IST_DOUBLE_FAULT: u8 = 1;
pub const IST_NMI: u8 = 2;
pub const IST_MACHINE_CHECK: u8 = 3;

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

    pub const fn with_ists(ist1: u64, ist2: u64, ist3: u64) -> Self {
        let mut tss = Self::new();
        tss.ist = [ist1, ist2, ist3, 0, 0, 0, 0];
        tss
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

#[cfg(target_os = "none")]
mod hw {
    use super::*;
    use crate::cpu::{self, DescriptorTablePointer};
    use core::arch::asm;
    use core::mem::size_of;

    const IST_STACK_SIZE: usize = 16 * 1024;

    #[repr(C, align(16))]
    struct Stack([u8; IST_STACK_SIZE]);

    static mut DF_STACK: Stack = Stack([0; IST_STACK_SIZE]);
    static mut NMI_STACK: Stack = Stack([0; IST_STACK_SIZE]);
    static mut MC_STACK: Stack = Stack([0; IST_STACK_SIZE]);
    static mut TSS: Tss = Tss::new();
    static mut GDT: [u64; 7] = [0; 7];

    /// Top addresses of the IST stacks: double fault, NMI, machine check.
    pub fn ist_tops() -> [u64; 3] {
        let top = |base: u64| base + IST_STACK_SIZE as u64;
        [
            top((&raw const DF_STACK) as u64),
            top((&raw const NMI_STACK) as u64),
            top((&raw const MC_STACK) as u64),
        ]
    }

    /// # Safety
    /// Sets the Ring 0 stack pointer used on a privilege change into the kernel.
    pub unsafe fn set_rsp0(top: u64) {
        unsafe {
            let tss = &raw mut TSS;
            (*tss).rsp[0] = top;
        }
    }

    pub fn rsp0() -> u64 {
        unsafe {
            let tss = &raw const TSS;
            (*tss).rsp[0]
        }
    }

    /// Loads the kernel GDT and TSS and reloads every segment register.
    ///
    /// # Safety
    /// Must run once, on the boot CPU, before interrupts are enabled.
    pub unsafe fn init() {
        unsafe {
            let [df_top, nmi_top, mc_top] = ist_tops();
            let tss = &raw mut TSS;
            tss.write(Tss::with_ists(df_top, nmi_top, mc_top));

            let [tss_lo, tss_hi] = tss_descriptor(tss as u64, (size_of::<Tss>() - 1) as u32);
            let gdt = &raw mut GDT;
            gdt.write([
                0,
                KERNEL_CODE,
                KERNEL_DATA,
                USER_DATA,
                USER_CODE,
                tss_lo,
                tss_hi,
            ]);

            let ptr = DescriptorTablePointer {
                limit: (size_of::<[u64; 7]>() - 1) as u16,
                base: gdt as u64,
            };
            cpu::lgdt(&ptr);

            // Far return to reload CS.
            asm!(
                "push {sel}",
                "lea {tmp}, [rip + 2f]",
                "push {tmp}",
                "retfq",
                "2:",
                sel = in(reg) KERNEL_CS as u64,
                tmp = lateout(reg) _,
            );
            asm!(
                "mov ds, {0:x}",
                "mov es, {0:x}",
                "mov ss, {0:x}",
                "mov fs, {0:x}",
                "mov gs, {0:x}",
                in(reg) KERNEL_DS,
                options(nostack, preserves_flags),
            );
            asm!("ltr {0:x}", in(reg) TSS_SEL, options(nostack, preserves_flags));
        }
    }
}

#[cfg(target_os = "none")]
pub use hw::{init, ist_tops, rsp0, set_rsp0};

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
    fn with_ists_sets_first_three_slots() {
        let tss = Tss::with_ists(0x1000, 0x2000, 0x3000);
        let ist = tss.ist;
        assert_eq!(&ist[..3], &[0x1000, 0x2000, 0x3000]);
        assert!(ist[3..].iter().all(|&v| v == 0));
    }

    #[test]
    fn ist_indices_are_distinct_and_one_based() {
        assert_eq!(IST_DOUBLE_FAULT, 1);
        assert_eq!(IST_NMI, 2);
        assert_eq!(IST_MACHINE_CHECK, 3);
    }

    #[test]
    fn with_ist1_sets_only_first_ist_slot() {
        let tss = Tss::with_ist1(0xdead_b000);
        let ist = tss.ist;
        assert_eq!(ist[0], 0xdead_b000);
        assert!(ist[1..].iter().all(|&v| v == 0));
    }
}
