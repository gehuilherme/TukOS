//! Register state saved by the exception entry stubs, and the dispatch hook.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Layout pushed by `tukos_isr_common`: GPRs (r15 at the lowest address), then the
/// vector and error code pushed by each stub, then the CPU-pushed interrupt frame.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

static HANDLER: AtomicUsize = AtomicUsize::new(0);

/// Installs the kernel function that receives every trap.
pub fn set_handler(handler: fn(&mut TrapFrame)) {
    HANDLER.store(handler as usize, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn trap_frame_layout_matches_assembly_stub() {
        assert_eq!(core::mem::size_of::<TrapFrame>(), 22 * 8);
        assert_eq!(offset_of!(TrapFrame, r15), 0);
        assert_eq!(offset_of!(TrapFrame, rax), 14 * 8);
        assert_eq!(offset_of!(TrapFrame, vector), 15 * 8);
        assert_eq!(offset_of!(TrapFrame, error_code), 16 * 8);
        assert_eq!(offset_of!(TrapFrame, rip), 17 * 8);
        assert_eq!(offset_of!(TrapFrame, ss), 21 * 8);
    }
}
