//! Per-CPU data reachable through GS. Intel SDM Vol. 3A, SWAPGS / IA32_KERNEL_GS_BASE.

#[repr(C)]
pub struct PerCpu {
    /// Kernel stack top loaded on syscall entry.
    pub kernel_rsp: u64,
    /// Scratch slot where the syscall entry stub saves the user rsp.
    pub user_rsp_scratch: u64,
    /// Kernel rsp to restore when a user excursion exits (set by enter_user).
    pub kernel_return_rsp: u64,
}

pub const OFF_KERNEL_RSP: usize = 0;
pub const OFF_USER_RSP_SCRATCH: usize = 8;
pub const OFF_KERNEL_RETURN_RSP: usize = 16;

impl PerCpu {
    pub const fn new() -> PerCpu {
        PerCpu {
            kernel_rsp: 0,
            user_rsp_scratch: 0,
            kernel_return_rsp: 0,
        }
    }
}

impl Default for PerCpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "none")]
mod hw {
    use super::PerCpu;
    use crate::cpu::{read_msr, write_msr};

    const IA32_GS_BASE: u32 = 0xC000_0101;
    const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

    /// # Safety
    /// `block` must point to a writable `'static` block that outlives all CPU use;
    /// call once on the boot CPU.
    pub unsafe fn install(block: *const PerCpu) {
        let addr = block as u64;
        unsafe {
            write_msr(IA32_GS_BASE, addr);
            write_msr(IA32_KERNEL_GS_BASE, addr);
        }
    }

    /// Sets the kernel stack top used by the SYSCALL entry stub (via GS).
    ///
    /// # Safety
    /// Must run in Ring 0 with the kernel GS base active (not between swapgs
    /// pairs) and with interrupts disabled.
    pub unsafe fn set_kernel_rsp(top: u64) {
        unsafe {
            core::arch::asm!(
                "mov gs:[{off}], {top}",
                off = const super::OFF_KERNEL_RSP,
                top = in(reg) top,
                options(nostack, preserves_flags)
            );
        }
    }

    pub fn gs_base() -> u64 {
        unsafe { read_msr(IA32_GS_BASE) }
    }
}

#[cfg(target_os = "none")]
pub use hw::{gs_base, install, set_kernel_rsp};

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn offsets_match_constants() {
        assert_eq!(offset_of!(PerCpu, kernel_rsp), OFF_KERNEL_RSP);
        assert_eq!(offset_of!(PerCpu, user_rsp_scratch), OFF_USER_RSP_SCRATCH);
        assert_eq!(offset_of!(PerCpu, kernel_return_rsp), OFF_KERNEL_RETURN_RSP);
    }

    #[test]
    fn new_is_zeroed() {
        let p = PerCpu::new();
        assert_eq!(p.kernel_rsp, 0);
        assert_eq!(p.user_rsp_scratch, 0);
        assert_eq!(p.kernel_return_rsp, 0);
    }
}
