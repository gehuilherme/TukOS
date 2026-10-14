//! Round-robin scheduler for kernel threads on a single CPU.
//! A new thread's stack is pre-built so the first `switch_context` into it
//! "returns" into `thread_start`, which enables interrupts and calls the entry.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use arch_x86_64::interrupts::IrqMutex;

const STACK_SIZE: usize = 64 * 1024;
/// Fixed thread-table capacity. The table never reallocates, so the raw `save`
/// pointer handed to `switch_context` stays valid.
const MAX_THREADS: usize = 64;

struct Thread {
    rsp: u64,
    entry: Option<fn()>,
    // Keeps the stack allocation alive; None for the boot thread.
    _stack: Option<Box<[u8]>>,
}

struct Scheduler {
    threads: Vec<Thread>,
    current: usize,
}

static SCHED: IrqMutex<Option<Scheduler>> = IrqMutex::new(None);

pub fn init() {
    let boot = Thread {
        rsp: 0,
        entry: None,
        _stack: None,
    };
    let mut threads = Vec::with_capacity(MAX_THREADS);
    threads.push(boot);
    *SCHED.lock() = Some(Scheduler {
        threads,
        current: 0,
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
    });
    s.threads.len() - 1
}

/// Switches to the next thread. The scheduler guard is dropped before the
/// switch so the resumed thread can take the lock; interrupts must be disabled.
fn switch_to_next() {
    let (save, load) = {
        let mut guard = SCHED.lock();
        let Some(s) = guard.as_mut() else { return };
        if s.threads.len() < 2 {
            return;
        }
        let from = s.current;
        let to = (from + 1) % s.threads.len();
        s.current = to;
        let save = &mut s.threads[from].rsp as *mut u64;
        let load = s.threads[to].rsp;
        (save, load)
    };
    unsafe { arch_x86_64::context::switch_context(save, load) };
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
