//! Round-robin scheduler for kernel threads on a single CPU.
//! A new thread's stack is pre-built so the first `switch_context` into it
//! "returns" into `thread_start`, which enables interrupts and calls the entry.

extern crate alloc;

use crate::mem::space::AddressSpace;
use crate::object::{CONSOLE_HANDLE, HandleTable, Object};
use alloc::boxed::Box;
use alloc::vec::Vec;
use arch_x86_64::interrupts::IrqMutex;
use core::sync::atomic::{AtomicU64, Ordering};

const STACK_SIZE: usize = 64 * 1024;
/// Fixed thread-table capacity. The table never reallocates, so the raw `save`
/// pointer handed to `switch_context` stays valid.
const MAX_THREADS: usize = 64;

#[derive(PartialEq)]
enum ThreadState {
    Ready,
    Exited,
}

struct Thread {
    rsp: u64,
    entry: Option<fn()>,
    // Keeps the stack allocation alive; None for the boot thread.
    _stack: Option<Box<[u8]>>,
    /// Physical root of the thread's address space; 0 = kernel space.
    cr3: u64,
    /// Top of the kernel stack used for Ring 3 -> 0 transitions; 0 = leave rsp0 alone.
    kstack_top: u64,
    state: ThreadState,
    /// Owned address space of a user thread; None for kernel threads.
    space: Option<AddressSpace>,
    /// User VA of the thread's TEB (reached via GS in Ring 3); 0 = none.
    teb_va: u64,
    /// Handle table of a user process; None for kernel threads.
    handle_table: Option<HandleTable>,
}

struct Scheduler {
    threads: Vec<Thread>,
    current: usize,
    /// Index of an Exited thread whose resources are not yet released.
    reap: Option<usize>,
}

/// Kernel PML4 root, captured in `init` before any user space exists.
static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);

static SCHED: IrqMutex<Option<Scheduler>> = IrqMutex::new(None);

pub fn init() {
    KERNEL_CR3.store(crate::arch_cr3_base(), Ordering::SeqCst);
    let boot = Thread {
        rsp: 0,
        entry: None,
        _stack: None,
        cr3: 0,
        kstack_top: 0,
        state: ThreadState::Ready,
        space: None,
        teb_va: 0,
        handle_table: None,
    };
    let mut threads = Vec::with_capacity(MAX_THREADS);
    threads.push(boot);
    *SCHED.lock() = Some(Scheduler {
        threads,
        current: 0,
        reap: None,
    });
}

#[allow(dead_code)] // public scheduler API; used by later tasks
pub fn current_id() -> usize {
    SCHED.lock().as_ref().map(|s| s.current).unwrap_or(0)
}

extern "C" fn thread_start() -> ! {
    let entry = {
        let guard = SCHED.lock();
        let s = guard.as_ref().expect("scheduler nao iniciado");
        s.threads[s.current].entry.expect("thread sem entrada")
    };
    arch_x86_64::interrupts::enable();
    entry();
    panic!("thread de kernel retornou");
}

pub fn spawn(entry: fn()) -> usize {
    let mut stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let top = (stack.as_mut_ptr() as u64 + STACK_SIZE as u64) & !0xF;
    // Frame consumed by switch_context: r15 r14 r13 r12 rbx rbp, then return address.
    // Leave the stack 16-byte aligned minus 8 at thread_start entry, as after a `call`.
    let frame_top = top - 8;
    let words = arch_x86_64::context::initial_stack_words() as u64;
    let rsp = frame_top - words * 8;
    unsafe {
        let p = rsp as *mut u64;
        for i in 0..6 {
            p.add(i).write(0);
        }
        p.add(6).write(thread_start as *const () as usize as u64);
    }
    let mut guard = SCHED.lock();
    let s = guard.as_mut().expect("scheduler nao iniciado");
    assert!(
        s.threads.len() < MAX_THREADS,
        "sched: limite de {MAX_THREADS} threads atingido"
    );
    s.threads.push(Thread {
        rsp,
        entry: Some(entry),
        _stack: Some(stack),
        cr3: 0,
        kstack_top: 0,
        state: ThreadState::Ready,
        space: None,
        teb_va: 0,
        handle_table: None,
    });
    s.threads.len() - 1
}

/// Spawns a thread whose first run drops to Ring 3 at `entry` with stack
/// `user_stack_top`, in `space`, which the thread now owns (freed on exit).
pub fn spawn_user(
    space: AddressSpace,
    entry: u64,
    user_stack_top: u64,
    mut kstack: Box<[u8]>,
    teb_va: u64,
) -> usize {
    let space_root = space.root();
    let top = (kstack.as_mut_ptr() as u64 + kstack.len() as u64) & !0xF;
    // Low -> high: 6 callee-saved zeros, tukos_user_entry, then the iretq frame
    // (rip, cs, rflags, rsp, ss).
    let rsp = top - 12 * 8;
    unsafe {
        let p = rsp as *mut u64;
        for i in 0..6 {
            p.add(i).write(0);
        }
        p.add(6)
            .write(arch_x86_64::context::tukos_user_entry as *const () as usize as u64);
        p.add(7).write(entry);
        p.add(8).write(arch_x86_64::gdt::USER_CS as u64);
        p.add(9).write(0x202);
        p.add(10).write(user_stack_top);
        p.add(11).write(arch_x86_64::gdt::USER_DS as u64);
    }
    let mut handles = HandleTable::new();
    let console = handles.insert(Object::Console);
    debug_assert_eq!(console, CONSOLE_HANDLE);
    let mut guard = SCHED.lock();
    let s = guard.as_mut().expect("scheduler nao iniciado");
    assert!(
        s.threads.len() < MAX_THREADS,
        "sched: limite de {MAX_THREADS} threads atingido"
    );
    s.threads.push(Thread {
        rsp,
        entry: None,
        _stack: Some(kstack),
        cr3: space_root,
        kstack_top: top,
        state: ThreadState::Ready,
        space: Some(space),
        teb_va,
        handle_table: Some(handles),
    });
    s.threads.len() - 1
}

/// Switches to the next thread. The scheduler guard is dropped before the
/// switch so the resumed thread can take the lock; interrupts must be disabled.
fn switch_to_next() {
    let (save, load, target_cr3, target_kstack_top, target_teb_va) = {
        let mut guard = SCHED.lock();
        let Some(s) = guard.as_mut() else { return };
        if s.threads.len() < 2 {
            return;
        }
        let from = s.current;
        let n = s.threads.len();
        let mut to = (from + 1) % n;
        while s.threads[to].state == ThreadState::Exited && to != from {
            to = (to + 1) % n;
        }
        if to == from && s.threads[from].state == ThreadState::Exited {
            panic!("sched: nenhuma thread viva");
        }
        s.current = to;
        let save = &mut s.threads[from].rsp as *mut u64;
        let load = s.threads[to].rsp;
        (
            save,
            load,
            s.threads[to].cr3,
            s.threads[to].kstack_top,
            s.threads[to].teb_va,
        )
    };
    unsafe {
        if target_kstack_top != 0 {
            arch_x86_64::gdt::set_rsp0(target_kstack_top);
            // SYSCALL entry must also run on this thread's own kernel stack,
            // never on the stack of whichever thread last entered Ring 3.
            arch_x86_64::percpu::set_kernel_rsp(target_kstack_top);
        }
        // The user GS base (what swapgs installs on Ring 3 entry) is the TEB.
        if target_teb_va != 0 {
            arch_x86_64::percpu::set_user_gs_base(target_teb_va);
        }
        // Kernel threads (cr3 == 0) explicitly load the kernel PML4, so the CPU
        // never keeps translating through a user space that may be reaped next.
        // rsp0/kernel_rsp are left untouched when target_kstack_top == 0: safe
        // only because kernel threads stay in Ring 0 and the next user thread
        // refreshes rsp0 before any Ring 3 -> 0 transition.
        let cur = crate::arch_cr3_base();
        let target_root = if target_cr3 != 0 {
            target_cr3
        } else {
            KERNEL_CR3.load(Ordering::SeqCst)
        };
        if target_root != 0 && target_root != cur {
            arch_x86_64::cpu::write_cr3(target_root);
        }
    }
    unsafe { arch_x86_64::context::switch_context(save, load) };
    reap_pending();
}

/// Releases the resources of a pending Exited thread. Runs on the stack of the
/// thread that was switched to, never on the stack being freed. Interrupts are
/// disabled by the callers of `switch_to_next`.
fn reap_pending() {
    let taken = {
        let mut guard = SCHED.lock();
        let Some(s) = guard.as_mut() else { return };
        s.reap
            .take()
            .map(|idx| (s.threads[idx].space.take(), s.threads[idx]._stack.take()))
    };
    // Dropped outside the SCHED lock: AddressSpace::drop takes the frames lock.
    if let Some((space, stack)) = taken {
        drop(space);
        drop(stack);
    }
}

/// Terminates the current (user) thread: marks it Exited and switches away; its
/// space and kernel stack are released by the reaper on the next thread's stack.
pub fn exit_current() -> ! {
    arch_x86_64::interrupts::disable();
    {
        let mut guard = SCHED.lock();
        let s = guard.as_mut().expect("scheduler nao iniciado");
        let cur = s.current;
        s.threads[cur].state = ThreadState::Exited;
        s.reap = Some(cur);
    }
    switch_to_next();
    unreachable!("thread Exited foi reescalonada");
}

/// True if the current thread owns a user address space.
pub fn current_is_user() -> bool {
    let guard = SCHED.lock();
    guard
        .as_ref()
        .is_some_and(|s| s.threads[s.current].space.is_some())
}

/// Runs `f` on the current user thread's handle table; None for kernel threads.
/// Holds the SCHED lock while `f` runs: `f` must not call back into the scheduler.
#[allow(dead_code)] // used by Task 4 (NtWriteFile/NtClose)
pub fn with_current_handles<R>(f: impl FnOnce(&mut HandleTable) -> R) -> Option<R> {
    let mut guard = SCHED.lock();
    let s = guard.as_mut()?;
    let cur = s.current;
    s.threads[cur].handle_table.as_mut().map(f)
}

pub fn yield_now() {
    let was_on = arch_x86_64::interrupts::are_enabled();
    arch_x86_64::interrupts::disable();
    switch_to_next();
    if was_on {
        arch_x86_64::interrupts::enable();
    }
}

/// Called from the timer interrupt (interrupts already disabled by the gate).
pub fn on_tick() {
    switch_to_next();
}
