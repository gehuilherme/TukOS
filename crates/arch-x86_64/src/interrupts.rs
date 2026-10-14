//! Interrupt-state helpers and a spinlock that keeps interrupts disabled while held.
//! Intel SDM Vol. 3A, 6.8.1 (masking maskable interrupts via RFLAGS.IF).

use core::cell::Cell;
use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};

const IF_BIT: u64 = 1 << 9;

pub const fn if_set(rflags: u64) -> bool {
    rflags & IF_BIT != 0
}

pub fn are_enabled() -> bool {
    if_set(crate::cpu::read_rflags())
}

pub fn enable() {
    crate::cpu::sti();
}

pub fn disable() {
    crate::cpu::cli();
}

/// Source of the interrupt-enable flag. The real CPU in the kernel; a fake on the host.
pub trait IfState {
    fn get(&self) -> bool;
    fn set(&self, on: bool);
}

pub struct Cpu;

impl IfState for Cpu {
    fn get(&self) -> bool {
        are_enabled()
    }
    fn set(&self, on: bool) {
        if on { enable() } else { disable() }
    }
}

/// Host-test stand-in for RFLAGS.IF.
pub struct FakeIf(Cell<bool>);

impl FakeIf {
    pub const fn new(on: bool) -> Self {
        Self(Cell::new(on))
    }
}

impl IfState for FakeIf {
    fn get(&self) -> bool {
        self.0.get()
    }
    fn set(&self, on: bool) {
        self.0.set(on)
    }
}

pub struct IrqMutex<T> {
    inner: Mutex<T>,
}

pub struct IrqMutexGuard<'a, T, S: IfState = Cpu> {
    guard: Option<MutexGuard<'a, T>>,
    restore: bool,
    state: &'a S,
}

static CPU: Cpu = Cpu;

impl<T> IrqMutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(value),
        }
    }

    pub fn lock(&self) -> IrqMutexGuard<'_, T> {
        self.lock_with(&CPU)
    }

    pub fn try_lock(&self) -> Option<IrqMutexGuard<'_, T>> {
        self.try_lock_with(&CPU)
    }

    pub fn lock_with<'a, S: IfState>(&'a self, state: &'a S) -> IrqMutexGuard<'a, T, S> {
        let restore = state.get();
        state.set(false);
        IrqMutexGuard {
            guard: Some(self.inner.lock()),
            restore,
            state,
        }
    }

    pub fn try_lock_with<'a, S: IfState>(
        &'a self,
        state: &'a S,
    ) -> Option<IrqMutexGuard<'a, T, S>> {
        let restore = state.get();
        state.set(false);
        match self.inner.try_lock() {
            Some(guard) => Some(IrqMutexGuard {
                guard: Some(guard),
                restore,
                state,
            }),
            None => {
                state.set(restore);
                None
            }
        }
    }

    /// # Safety
    /// Only for the panic path, where the previous holder will never run again.
    pub unsafe fn force_unlock(&self) {
        unsafe { self.inner.force_unlock() };
    }
}

impl<T, S: IfState> Deref for IrqMutexGuard<'_, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard.as_ref().expect("guard ativo")
    }
}

impl<T, S: IfState> DerefMut for IrqMutexGuard<'_, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("guard ativo")
    }
}

impl<T, S: IfState> Drop for IrqMutexGuard<'_, T, S> {
    fn drop(&mut self) {
        // Release the lock first, then restore interrupts.
        self.guard.take();
        if self.restore {
            self.state.set(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_bit_is_bit_9() {
        assert!(if_set(1 << 9));
        assert!(!if_set(0));
        assert!(!if_set(!(1u64 << 9)));
    }

    // Host tests drive the guard through a fake interrupt-state cell so the
    // restore logic is exercised without touching real RFLAGS.
    #[test]
    fn irq_mutex_restores_prior_state() {
        let state = FakeIf::new(true);
        let m = IrqMutex::new(5u32);
        {
            let g = m.lock_with(&state);
            assert_eq!(*g, 5);
            assert!(!state.get(), "interrupts must be off while locked");
        }
        assert!(state.get(), "interrupts must come back on after unlock");
    }

    #[test]
    fn stays_disabled_if_it_was_disabled() {
        let state = FakeIf::new(false);
        let m = IrqMutex::new(());
        drop(m.lock_with(&state));
        assert!(!state.get());
    }

    #[test]
    fn nested_guards_restore_in_order() {
        let state = FakeIf::new(true);
        let a = IrqMutex::new(1u8);
        let b = IrqMutex::new(2u8);
        let ga = a.lock_with(&state);
        let gb = b.lock_with(&state);
        assert!(!state.get());
        drop(gb);
        assert!(
            !state.get(),
            "inner unlock must not re-enable while outer is held"
        );
        drop(ga);
        assert!(state.get());
    }

    #[test]
    fn try_lock_fails_while_held() {
        let state = FakeIf::new(true);
        let m = IrqMutex::new(0u8);
        let g = m.lock_with(&state);
        assert!(m.try_lock_with(&state).is_none());
        drop(g);
        assert!(m.try_lock_with(&state).is_some());
    }
}
