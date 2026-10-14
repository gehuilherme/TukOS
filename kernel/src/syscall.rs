//! TukOS system call dispatch.

use core::sync::atomic::{AtomicU64, Ordering};

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE_MARK: u64 = 1;

pub static LAST_MARK: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    arch_x86_64::syscall::set_dispatch(dispatch);
    unsafe {
        arch_x86_64::syscall::init(
            arch_x86_64::syscall::tukos_syscall_entry as *const () as usize as u64,
        )
    };
}

pub extern "C" fn dispatch(nr: u64, a0: u64, _a1: u64, _a2: u64, _a3: u64) -> u64 {
    match nr {
        SYS_WRITE_MARK => {
            LAST_MARK.store(a0, Ordering::SeqCst);
            0
        }
        SYS_EXIT => {
            // Handled specially by the Ring 3 return path in Task 3; if reached
            // here (e.g. a direct call), it is a no-op returning the exit code.
            a0
        }
        _ => u64::MAX,
    }
}
