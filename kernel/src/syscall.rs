//! TukOS system call dispatch.

#[cfg(feature = "ktest")]
use arch_x86_64::interrupts::IrqMutex;
use core::sync::atomic::{AtomicU64, Ordering};

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE_MARK: u64 = 1;

pub const NT_WRITE_FILE: u64 = 2;
pub const NT_TERMINATE_PROCESS: u64 = 3;
pub const NT_CLOSE: u64 = 4;
#[cfg(feature = "ktest")]
pub const SYS_SUM6: u64 = 100;
pub const STATUS_SUCCESS: u64 = 0;
pub const STATUS_ACCESS_VIOLATION: u64 = 0xC000_0005;
pub const STATUS_INVALID_HANDLE: u64 = 0xC000_0008;

#[cfg(feature = "ktest")]
pub static LAST_WRITE_LEN: AtomicU64 = AtomicU64::new(0);
/// Status passed to the most recent NtTerminateProcess (u64::MAX = none yet).
#[cfg(feature = "ktest")]
pub static LAST_TERMINATE_STATUS: AtomicU64 = AtomicU64::new(u64::MAX);
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

/// Reads stack argument `index` (0-based beyond the 4 register args) of a
/// Windows x64 syscall: `[user_rsp + 0x28 + index*8]` (return slot + 32-byte shadow).
pub fn stack_arg(user_rsp: u64, index: u64) -> Option<u64> {
    let addr = user_rsp
        .checked_add(0x28)?
        .checked_add(index.checked_mul(8)?)?;
    let bytes = crate::usercopy::copy_from_user(addr, 8)?;
    Some(u64::from_le_bytes(bytes.as_slice().try_into().ok()?))
}

/// Core of NtWriteFile once the stack arguments are decoded.
fn nt_write_file(handle: u64, iosb: u64, buf: u64, len: u64) -> u64 {
    let obj = crate::sched::with_current_handles(|t| t.get(handle).cloned()).flatten();
    if obj != Some(crate::object::Object::Console) {
        return STATUS_INVALID_HANDLE;
    }
    let Some(bytes) = crate::usercopy::copy_from_user(buf, len) else {
        return STATUS_ACCESS_VIOLATION;
    };
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
    // IO_STATUS_BLOCK (x64) is { union { NTSTATUS Status; PVOID Pointer; }; ULONG_PTR
    // Information; }. Simplification: Status is stored as a full u64 (low 4 bytes =
    // NTSTATUS, high 4 = 0) at offset 0, Information as u64 at offset 8.
    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&STATUS_SUCCESS.to_le_bytes());
    block[8..].copy_from_slice(&len.to_le_bytes());
    if !crate::usercopy::copy_to_user(iosb, &block) {
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// Test hook: runs NtWriteFile's handle resolution for `handle` with an empty
/// buffer, without needing a user process.
#[cfg(feature = "ktest")]
pub fn ntwritefile_for_test(handle: u64, _a1: u64, _a2: u64, _a3: u64) -> u64 {
    nt_write_file(handle, 0, 0, 0)
}

pub extern "C" fn dispatch(nr: u64, a0: u64, a1: u64, a2: u64, _a3: u64, _user_rsp: u64) -> u64 {
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
            // NtWriteFile(FileHandle, Event, ApcRoutine, ApcContext, IoStatusBlock,
            // Buffer, Length, ByteOffset, Key): a0..a3 in registers, the rest on the
            // user stack. Event/Apc*/ByteOffset/Key are ignored in this phase.
            let (Some(iosb), Some(buf), Some(len)) = (
                stack_arg(_user_rsp, 0),
                stack_arg(_user_rsp, 1),
                stack_arg(_user_rsp, 2),
            ) else {
                return STATUS_ACCESS_VIOLATION;
            };
            nt_write_file(a0, iosb, buf, len)
        }
        NT_CLOSE => match crate::sched::with_current_handles(|t| t.close(a0)) {
            Some(true) => STATUS_SUCCESS,
            _ => STATUS_INVALID_HANDLE,
        },
        NT_TERMINATE_PROCESS => {
            #[cfg(feature = "ktest")]
            LAST_TERMINATE_STATUS.store(a0, Ordering::SeqCst);
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
        #[cfg(feature = "ktest")]
        SYS_SUM6 => match (stack_arg(_user_rsp, 0), stack_arg(_user_rsp, 1)) {
            (Some(s0), Some(s1)) => {
                let sum = a0
                    .wrapping_add(a1)
                    .wrapping_add(a2)
                    .wrapping_add(_a3)
                    .wrapping_add(s0)
                    .wrapping_add(s1);
                LAST_MARK.store(sum, Ordering::SeqCst);
                0
            }
            _ => STATUS_ACCESS_VIOLATION,
        },
        _ => u64::MAX,
    }
}
