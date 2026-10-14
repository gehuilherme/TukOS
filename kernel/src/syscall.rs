//! TukOS system call dispatch.

#[cfg(feature = "ktest")]
use arch_x86_64::interrupts::IrqMutex;
use core::sync::atomic::{AtomicU64, Ordering};

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE_MARK: u64 = 1;

pub const NT_WRITE_FILE: u64 = 2;
pub const NT_TERMINATE_PROCESS: u64 = 3;
pub const STATUS_SUCCESS: u64 = 0;
pub const STATUS_ACCESS_VIOLATION: u64 = 0xC000_0005;

#[cfg(feature = "ktest")]
pub static LAST_WRITE_LEN: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "ktest")]
static LAST_WRITE: IrqMutex<[u8; 64]> = IrqMutex::new([0; 64]);
#[cfg(feature = "ktest")]
pub fn last_write_is(expected: &[u8]) -> bool {
    let g = LAST_WRITE.lock();
    expected.len() <= 64 && g[..expected.len()] == *expected
}

pub static LAST_MARK: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    arch_x86_64::syscall::set_dispatch(dispatch);
    unsafe {
        arch_x86_64::syscall::init(
            arch_x86_64::syscall::tukos_syscall_entry as *const () as usize as u64,
        )
    };
}

pub extern "C" fn dispatch(nr: u64, a0: u64, a1: u64, a2: u64, _a3: u64) -> u64 {
    match nr {
        SYS_WRITE_MARK => {
            LAST_MARK.store(a0, Ordering::SeqCst);
            #[cfg(feature = "ktest")]
            crate::ktest::on_user_mark(a0);
            0
        }
        SYS_EXIT => {
            if crate::sched::current_is_user() {
                crate::sched::exit_current();
            }
            #[cfg(target_os = "none")]
            unsafe {
                arch_x86_64::usermode::exit_to_kernel(a0)
            }
            #[cfg(not(target_os = "none"))]
            a0
        }
        NT_WRITE_FILE => {
            // a0 = handle (console, ignored for now), a1 = buffer, a2 = length.
            match crate::usercopy::copy_from_user(a1, a2) {
                Some(bytes) => {
                    if let Ok(s) = core::str::from_utf8(&bytes) {
                        crate::kprint!("{s}");
                    }
                    #[cfg(feature = "ktest")]
                    {
                        let mut g = LAST_WRITE.lock();
                        let n = core::cmp::min(bytes.len(), 64);
                        g[..n].copy_from_slice(&bytes[..n]);
                        drop(g);
                        LAST_WRITE_LEN.store(bytes.len() as u64, Ordering::SeqCst);
                    }
                    STATUS_SUCCESS
                }
                None => STATUS_ACCESS_VIOLATION,
            }
        }
        NT_TERMINATE_PROCESS => {
            if crate::sched::current_is_user() {
                crate::sched::exit_current();
            }
            #[cfg(target_os = "none")]
            unsafe {
                arch_x86_64::usermode::exit_to_kernel(a0)
            }
            #[cfg(not(target_os = "none"))]
            a0
        }
        _ => u64::MAX,
    }
}
