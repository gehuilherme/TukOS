# TukOS Plano 2b — Interrupções e multitarefa de threads de kernel

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** O kernel do TukOS recebe interrupções de timer do LAPIC com segurança e alterna preemptivamente entre threads de kernel; e pode ser validado também no VMware.

**Architecture:** Primeiro os locks passam a desabilitar interrupções enquanto seguros (`IrqMutex`), porque a partir deste plano um tratador de interrupção pode rodar no meio de código que segura um lock. Depois a TSS ganha pilhas IST próprias para NMI e #MC. Em seguida o LAPIC é mapeado e programado para um timer periódico no vetor 32, com a tabela de stubs estendida para cobrir vetores de IRQ. Por fim um scheduler round-robin troca o contexto entre threads de kernel, de forma cooperativa (`yield_now`) e preemptiva (a cada tick do timer). O alvo VMware vem por último e é independente.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `limine` 0.5.0, `spin` 0.10, `mm` e `arch-x86_64` (do próprio projeto), QEMU + OVMF, VMware Workstation Pro (`vmrun`).

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 4: "Tempo e threads: timer do LAPIC e um scheduler preemptivo simples (round-robin) com threads de kernel"). Sucede o Plano 2 (memória), já na `main`.

**Fora deste plano (vai para o Plano 2c):** modo usuário via SYSCALL/SYSRET, `swapgs` e base de GS por CPU, `gdt::set_rsp0`, espaços de endereçamento por processo. Threads de kernel não trocam de privilégio, então não precisam de nada disso.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0` em todos os `Cargo.toml`.
- Apenas x86-64, um núcleo de CPU. Código dependente de CPU vive em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft.
- Clean room nível 0 apenas: Intel SDM Vol. 3A (capítulos 6 "Interrupt and Exception Handling", 7 "Task Management", 11 "Advanced Programmable Interrupt Controller"), System V AMD64 ABI, documentação do VMware Workstation (`vmrun`, formato `.vmx`).
- Commits de comportamento de hardware/protocolo declaram `Source: docs <documento>`.
- Commits terminam com `Co-Authored-By:` nomeando o modelo que de fato escreveu o commit.
- Código e comentários em inglês; mensagens ao usuário e docs em português.
- Vetor do timer: 32 (`0x20`). Vetor espúrio do LAPIC: 255 (`0xFF`). Base física padrão do LAPIC: lida do MSR `IA32_APIC_BASE` (`0x1B`).
- Marcadores de serial existentes não mudam: `@@TUKOS-KTEST-PASS/FAIL/DONE`, `@@TUKOS-PANIC`, `@@TUKOS-BACKTRACE`.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **Lock tomado por uma thread e de novo por um tratador de interrupção** travaria o único núcleo para sempre; todo lock usado tanto em código normal quanto em tratador (serial, console, frames, heap) precisa desabilitar interrupções enquanto seguro e restaurar o estado anterior ao soltar, inclusive quando aninhado. Coberto por `irq_mutex_restores_prior_state` e `nested_guards_restore_in_order` (Task 1).
2. **Interrupção que chega sem EOI** deixa o LAPIC sem entregar o próximo tick; o tratador do timer sempre envia EOI. Coberto pelo ktest `timer_ticks_advance`, que exige vários ticks (Task 3).
3. **Primeira execução de uma thread nova** precisa começar com uma pilha montada que faça o `switch_context` "retornar" para um trampolim que liga as interrupções e chama a função da thread; uma pilha mal montada derruba o kernel na primeira troca. Coberto por `spawned_thread_runs` (Task 4).
4. **Threads que nunca cedem a CPU** só são justas se o timer as preemptar; sem preempção, a segunda thread nunca roda. Coberto por `preemption_runs_both_threads`, em que nenhuma das duas chama `yield_now` (Task 4).
5. **Alvo VMware sem `vmrun` instalado ou em máquina sem VMware** deve falhar com mensagem clara dizendo o que procurou, não com erro críptico, e não pode afetar `cargo xtask ktest`. Coberto por `find_vmrun_reports_missing` (Task 5).

---

### Task 1: Locks seguros para interrupção (`IrqMutex`)

**Files:**
- Create: `crates/arch-x86_64/src/interrupts.rs`
- Modify: `crates/arch-x86_64/src/lib.rs`, `crates/arch-x86_64/src/cpu.rs` (adicionar `sti` e `read_rflags`)
- Modify: `kernel/src/serial.rs`, `kernel/src/fbcon.rs`, `kernel/src/mem/frames.rs`, `kernel/src/mem/heap.rs`, `kernel/src/panic.rs`

**Interfaces:**
- Produces (em `crates/arch-x86_64/src/interrupts.rs`):
  - `pub const fn if_set(rflags: u64) -> bool` — bit 9 (IF)
  - `pub fn are_enabled() -> bool`; `pub fn enable()`; `pub fn disable()`
  - `pub struct IrqMutex<T>` com `pub const fn new(value: T) -> Self`, `pub fn lock(&self) -> IrqMutexGuard<'_, T>`, `pub fn try_lock(&self) -> Option<IrqMutexGuard<'_, T>>`, `pub unsafe fn force_unlock(&self)`
  - `IrqMutexGuard<'_, T>` implementa `Deref`/`DerefMut`; ao ser solto, libera o lock e **depois** reabilita interrupções somente se elas estavam ligadas antes do `lock`
- Produces (em `cpu.rs`): `pub fn sti()`, `pub fn read_rflags() -> u64`

- [ ] **Step 1: Testes que falham (host)**

`crates/arch-x86_64/src/interrupts.rs`:
```rust
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
        assert!(!state.get(), "inner unlock must not re-enable while outer is held");
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
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64 interrupts`
Expected: FAIL de compilação (`if_set`, `IrqMutex`, `FakeIf` não existem).

- [ ] **Step 3: Implementar**

Em `cpu.rs`, acrescentar:
```rust
/// Enables maskable interrupts. Not `nomem`: it orders memory like `cli`.
#[inline]
pub fn sti() {
    unsafe { asm!("sti", options(nostack)) };
}

#[inline]
pub fn read_rflags() -> u64 {
    let v: u64;
    unsafe { asm!("pushfq", "pop {}", out(reg) v, options(nomem, preserves_flags)) };
    v
}
```

Acima dos testes em `interrupts.rs`:
```rust
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
        Self { inner: Mutex::new(value) }
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
        IrqMutexGuard { guard: Some(self.inner.lock()), restore, state }
    }

    pub fn try_lock_with<'a, S: IfState>(&'a self, state: &'a S) -> Option<IrqMutexGuard<'a, T, S>> {
        let restore = state.get();
        state.set(false);
        match self.inner.try_lock() {
            Some(guard) => Some(IrqMutexGuard { guard: Some(guard), restore, state }),
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
```

Em `crates/arch-x86_64/Cargo.toml`, adicionar `[dependencies] spin = "0.10"`. Em `lib.rs`, adicionar `pub mod interrupts;`.

- [ ] **Step 4: Migrar os locks do kernel**

Trocar `spin::Mutex` por `arch_x86_64::interrupts::IrqMutex` em:
- `kernel/src/serial.rs`: `pub static SERIAL: IrqMutex<Serial> = IrqMutex::new(Serial);`
- `kernel/src/fbcon.rs`: `pub static FBCON: IrqMutex<Option<FbCon>> = IrqMutex::new(None);`
- `kernel/src/mem/frames.rs`: `static STATE: IrqMutex<Option<State>> = IrqMutex::new(None);`
- `kernel/src/mem/heap.rs`: `struct LockedHeap(IrqMutex<FreeListHeap>);` e `LockedHeap(IrqMutex::new(FreeListHeap::new()))`

`kernel/src/panic.rs` continua chamando `SERIAL.force_unlock()` e `FBCON.try_lock()`; com `IrqMutex` as assinaturas são as mesmas. Remova imports de `spin::Mutex` que ficarem sem uso.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test -p arch-x86_64` → todos passam (incluindo os 5 novos).
Run: `cargo xtask ktest` → 4 variantes ok (nada muda de comportamento: interrupções ainda estão desligadas).

- [ ] **Step 6: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(arch): IrqMutex que desabilita interrupcoes enquanto seguro

Source: docs Intel SDM Vol. 3A, 6.8.1
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Pilhas IST para NMI e #MC

**Files:**
- Modify: `crates/arch-x86_64/src/gdt.rs` (TSS com IST1/IST2/IST3), `crates/arch-x86_64/src/idt.rs` (vetores 2 e 18 usam IST2/IST3)
- Modify: `kernel/src/ktest.rs` (novo teste)

**Interfaces:**
- Produces: `gdt::Tss::with_ists(ist1: u64, ist2: u64, ist3: u64) -> Tss`; constantes `gdt::IST_DOUBLE_FAULT: u8 = 1`, `gdt::IST_NMI: u8 = 2`, `gdt::IST_MACHINE_CHECK: u8 = 3`; `gdt::ist_tops() -> [u64; 3]` (só `target_os = "none"`); `idt::entry(vector: usize) -> IdtEntry` (só `target_os = "none"`).

- [ ] **Step 1: Testes que falham**

Em `gdt.rs`, módulo de testes:
```rust
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
```

Em `kernel/src/ktest.rs`, adicionar `("ist_stacks_installed", ist_stacks_installed),` a `TESTS` e:
```rust
fn ist_stacks_installed() -> Result<(), &'static str> {
    use arch_x86_64::{gdt, idt};
    let tops = gdt::ist_tops();
    if tops.iter().any(|&t| t == 0 || t % 16 != 0) {
        return Err("topo de pilha IST nulo ou desalinhado");
    }
    if tops[0] == tops[1] || tops[1] == tops[2] || tops[0] == tops[2] {
        return Err("pilhas IST compartilhadas");
    }
    if idt::entry(8).ist != gdt::IST_DOUBLE_FAULT
        || idt::entry(2).ist != gdt::IST_NMI
        || idt::entry(18).ist != gdt::IST_MACHINE_CHECK
    {
        return Err("vetores 8/2/18 nao apontam para a IST correta");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64 gdt` e `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Em `gdt.rs` (parte pura, acima de `mod hw`):
```rust
pub const IST_DOUBLE_FAULT: u8 = 1;
pub const IST_NMI: u8 = 2;
pub const IST_MACHINE_CHECK: u8 = 3;
```
e em `impl Tss`:
```rust
    pub const fn with_ists(ist1: u64, ist2: u64, ist3: u64) -> Self {
        let mut tss = Self::new();
        tss.ist = [ist1, ist2, ist3, 0, 0, 0, 0];
        tss
    }
```

Em `mod hw` de `gdt.rs`: substituir a pilha única por três pilhas de 16 KiB alinhadas em 16 (`DF_STACK`, `NMI_STACK`, `MC_STACK`), montar a TSS com `Tss::with_ists(df_top, nmi_top, mc_top)`, e expor:
```rust
    pub fn ist_tops() -> [u64; 3] {
        let top = |base: u64| base + IST_STACK_SIZE as u64;
        [
            top((&raw const DF_STACK) as u64),
            top((&raw const NMI_STACK) as u64),
            top((&raw const MC_STACK) as u64),
        ]
    }
```
com `pub use hw::{init, ist_tops};` (renomeie `DF_STACK_SIZE` para `IST_STACK_SIZE`). `init` usa `ist_tops()` para montar a TSS.

Em `mod hw` de `idt.rs`: escolher o IST por vetor —
```rust
            let ist = match vector {
                8 => crate::gdt::IST_DOUBLE_FAULT,
                2 => crate::gdt::IST_NMI,
                18 => crate::gdt::IST_MACHINE_CHECK,
                _ => 0,
            };
```
e expor `pub fn entry(vector: usize) -> IdtEntry { unsafe { (*(&raw const IDT))[vector] } }`, com `pub use hw::{entry, init, table_address};`.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p arch-x86_64` e `cargo xtask ktest` → passam (`variante ktest ok (11 teste(s))`).

- [ ] **Step 5: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(arch): pilhas IST separadas para double fault, NMI e machine check

Source: docs Intel SDM Vol. 3A, 6.14.5 (Interrupt Stack Table)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: LAPIC, timer periódico e interrupções ligadas

**Files:**
- Modify: `crates/mm/src/paging.rs` (flag `CACHE_DISABLE`), `crates/arch-x86_64/src/cpu.rs` (`read_msr`, `write_msr`)
- Modify: `crates/arch-x86_64/src/trap.rs` (stubs e tabela para vetores 0..48), `crates/arch-x86_64/src/idt.rs` (instalar 48 vetores)
- Create: `crates/arch-x86_64/src/lapic.rs` (offsets e codificações puras)
- Create: `kernel/src/apic.rs`
- Modify: `kernel/src/main.rs`, `kernel/src/trap.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`mm::paging`): `PageFlags::CACHE_DISABLE` (bit 4).
- Produces (`cpu.rs`): `pub unsafe fn read_msr(msr: u32) -> u64`, `pub unsafe fn write_msr(msr: u32, value: u64)`.
- Produces (`lapic.rs`, pura): `pub const IA32_APIC_BASE: u32 = 0x1B`; offsets `REG_EOI = 0xB0`, `REG_SPURIOUS = 0xF0`, `REG_LVT_TIMER = 0x320`, `REG_TIMER_INITIAL = 0x380`, `REG_TIMER_CURRENT = 0x390`, `REG_TIMER_DIVIDE = 0x3E0`; `pub const TIMER_VECTOR: u8 = 32`; `pub const SPURIOUS_VECTOR: u8 = 0xFF`; `pub const fn apic_base_phys(msr: u64) -> u64` (bits 12..51); `pub const fn spurious_value(vector: u8) -> u32` (bit 8 ligado + vetor); `pub const fn lvt_timer_periodic(vector: u8) -> u32` (bit 17 ligado, bit 16 desligado, vetor); `pub const TIMER_DIVIDE_BY_16: u32 = 0x3`.
- Produces (`kernel/src/apic.rs`): `pub fn init()` (mapeia o LAPIC com `CACHE_DISABLE`, habilita, programa o timer), `pub fn eoi()`, `pub static TICKS: AtomicU64`.
- `trap::tukos_isr_table` passa a ter 48 entradas; vetores 32..47 não empurram código de erro.

- [ ] **Step 1: Testes que falham (host)**

Em `lapic.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apic_base_masks_flags() {
        // Default base 0xFEE00000 with BSP (bit 8) and enable (bit 11) set.
        assert_eq!(apic_base_phys(0xFEE0_0900), 0xFEE0_0000);
    }

    #[test]
    fn spurious_sets_enable_bit_and_vector() {
        assert_eq!(spurious_value(0xFF), 0x1FF);
    }

    #[test]
    fn periodic_timer_is_unmasked_with_vector() {
        let v = lvt_timer_periodic(32);
        assert_eq!(v & 0xFF, 32);
        assert!(v & (1 << 17) != 0, "modo periodico");
        assert!(v & (1 << 16) == 0, "nao mascarado");
    }
}
```

Em `mm/src/paging.rs`, acrescentar ao módulo de testes:
```rust
    #[test]
    fn cache_disable_is_bit_4() {
        assert_eq!(PageFlags::CACHE_DISABLE.bits(), 1 << 4);
    }
```

Em `kernel/src/ktest.rs`, adicionar `("timer_ticks_advance", timer_ticks_advance),` a `TESTS` e:
```rust
fn timer_ticks_advance() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    let start = crate::apic::TICKS.load(Ordering::SeqCst);
    // Wait for several ticks; hlt sleeps until the next interrupt.
    for _ in 0..1000 {
        if crate::apic::TICKS.load(Ordering::SeqCst) >= start + 5 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("timer nao avancou 5 ticks")
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64 lapic`, `cargo test -p mm` e `cargo xtask ktest` → falhas de compilação.

- [ ] **Step 3: Implementar a parte pura**

`crates/arch-x86_64/src/lapic.rs` (acima dos testes):
```rust
//! Local APIC register offsets and value encodings (xAPIC, MMIO).
//! Intel SDM Vol. 3A, chapter 11.

pub const IA32_APIC_BASE: u32 = 0x1B;

pub const REG_EOI: usize = 0xB0;
pub const REG_SPURIOUS: usize = 0xF0;
pub const REG_LVT_TIMER: usize = 0x320;
pub const REG_TIMER_INITIAL: usize = 0x380;
pub const REG_TIMER_CURRENT: usize = 0x390;
pub const REG_TIMER_DIVIDE: usize = 0x3E0;

pub const TIMER_VECTOR: u8 = 32;
pub const SPURIOUS_VECTOR: u8 = 0xFF;
pub const TIMER_DIVIDE_BY_16: u32 = 0x3;

pub const fn apic_base_phys(msr: u64) -> u64 {
    msr & 0x000F_FFFF_FFFF_F000
}

pub const fn spurious_value(vector: u8) -> u32 {
    (1 << 8) | vector as u32
}

pub const fn lvt_timer_periodic(vector: u8) -> u32 {
    (1 << 17) | vector as u32
}
```
Em `lib.rs`, `pub mod lapic;`.

Em `mm/src/paging.rs`, em `impl PageFlags`: `pub const CACHE_DISABLE: PageFlags = PageFlags(1 << 4);`

Em `cpu.rs`:
```rust
/// # Safety
/// `msr` must be a valid MSR for this CPU.
#[inline]
pub unsafe fn read_msr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    ((hi as u64) << 32) | lo as u64
}

/// # Safety
/// `msr` must be a valid, writable MSR and `value` legal for it.
#[inline]
pub unsafe fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nostack, preserves_flags))
    };
}
```

- [ ] **Step 4: Estender os stubs de interrupção para vetores 32..47**

Em `crates/arch-x86_64/src/trap.rs`, no `global_asm!`:
- acrescentar os vetores `32,33,...,47` à lista `.irp` que usa `TUKOS_ISR_NOERR`;
- estender a lista `.irp` de `tukos_isr_table` para `0,1,...,47`;
- trocar a declaração para `pub static tukos_isr_table: [u64; 48];` e o comentário para "vectors 0..48".

Em `idt.rs`, o loop de `init` já percorre `tukos_isr_table.iter()`, então passa a instalar os 48 vetores sem outra mudança.

- [ ] **Step 5: Implementar o LAPIC no kernel**

`kernel/src/apic.rs`:
```rust
//! Local APIC bring-up: map the MMIO page uncached, enable it, and run a periodic timer.

use arch_x86_64::lapic::*;
use core::sync::atomic::{AtomicU64, Ordering};
use mm::paging::PageFlags;

/// Virtual address where the LAPIC page is mapped (free high-half slot).
const LAPIC_VIRT: u64 = 0xffff_fe00_0000_0000;
/// Initial count for the periodic timer (divide-by-16). Tuned for a visible tick rate in QEMU.
const TIMER_INITIAL_COUNT: u32 = 0x0010_0000;

pub static TICKS: AtomicU64 = AtomicU64::new(0);

fn reg(offset: usize) -> *mut u32 {
    (LAPIC_VIRT as usize + offset) as *mut u32
}

fn write(offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile(reg(offset), value) };
}

pub fn eoi() {
    write(REG_EOI, 0);
}

pub fn init() {
    let base = apic_base_phys(unsafe { arch_x86_64::cpu::read_msr(IA32_APIC_BASE) });
    let flags = PageFlags::WRITABLE
        .union(PageFlags::NO_EXECUTE)
        .union(PageFlags::CACHE_DISABLE);
    unsafe {
        crate::mem::paging::map_page(LAPIC_VIRT, base, flags).expect("falha ao mapear o LAPIC");
    }
    write(REG_SPURIOUS, spurious_value(SPURIOUS_VECTOR));
    write(REG_TIMER_DIVIDE, TIMER_DIVIDE_BY_16);
    write(REG_LVT_TIMER, lvt_timer_periodic(TIMER_VECTOR));
    write(REG_TIMER_INITIAL, TIMER_INITIAL_COUNT);
}

/// Called from the trap handler for the timer vector.
pub fn on_timer() {
    TICKS.fetch_add(1, Ordering::SeqCst);
    eoi();
}
```

Em `kernel/src/trap.rs`, no início de `handle`, antes do teste de `BREAKPOINT`:
```rust
    if frame.vector == arch_x86_64::lapic::TIMER_VECTOR as u64 {
        crate::apic::on_timer();
        return;
    }
    if frame.vector == arch_x86_64::lapic::SPURIOUS_VECTOR as u64 {
        return; // spurious: no EOI per SDM 11.9
    }
```
e no `panic!` final, usar `exception_name` só para vetores < 32 (para os demais, o texto "IRQ" já basta: ajuste `exception_name` para retornar `"IRQ"` quando `vector >= 32`).

Em `kernel/src/main.rs`: `mod apic;`; após `mem::init()` e o `kprintln!` de memória, chamar:
```rust
    apic::init();
    arch_x86_64::interrupts::enable();
    kprintln!("interrupcoes ligadas (timer no vetor {})", arch_x86_64::lapic::TIMER_VECTOR);
```

O LAPIC fica disponível porque o Limine deixa o modo xAPIC habilitado no BSP. Se o QEMU reportar o LAPIC em modo x2APIC (bit 10 do MSR), registre no relatório: este plano usa só xAPIC por MMIO.

- [ ] **Step 6: Rodar e ver passar**

Run: `cargo test -p arch-x86_64`, `cargo test -p mm`, `cargo xtask ktest` → passam (`variante ktest ok (12 teste(s))`).
Run: `cargo xtask run` → a serial mostra `interrupcoes ligadas (timer no vetor 32)` e o kernel segue ocioso sem panic.

- [ ] **Step 7: Commit**

```bash
git add crates kernel
git commit -m "feat(kernel): LAPIC com timer periodico e interrupcoes habilitadas

Source: docs Intel SDM Vol. 3A, capitulo 11 (APIC)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Scheduler round-robin de threads de kernel

**Files:**
- Create: `crates/arch-x86_64/src/context.rs` (troca de contexto em assembly)
- Modify: `crates/arch-x86_64/src/lib.rs`
- Create: `kernel/src/sched.rs`
- Modify: `kernel/src/main.rs`, `kernel/src/apic.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`context.rs`, só `target_os = "none"`): `pub unsafe extern "C" fn switch_context(save_rsp: *mut u64, load_rsp: u64)` — empilha `rbp, rbx, r12, r13, r14, r15`, salva `rsp` em `*save_rsp`, carrega `load_rsp`, desempilha na ordem inversa e `ret`. Também `pub const fn initial_stack_words() -> usize` (= 7: seis registradores + endereço de retorno).
- Produces (`kernel/src/sched.rs`): `pub fn init()` (registra a thread atual como thread 0); `pub fn spawn(entry: fn()) -> usize` (aloca pilha de 64 KiB no heap, retorna o id); `pub fn yield_now()`; `pub fn on_tick()` (chamada pelo timer: troca para a próxima thread pronta); `pub fn current_id() -> usize`.

- [ ] **Step 1: Testes que falham**

Em `context.rs` (teste puro de layout):
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn initial_frame_has_six_callee_saved_plus_return() {
        assert_eq!(super::initial_stack_words(), 7);
    }
}
```

Em `kernel/src/ktest.rs`, adicionar a `TESTS`:
```rust
    ("spawned_thread_runs", spawned_thread_runs),
    ("yield_alternates_threads", yield_alternates_threads),
    ("preemption_runs_both_threads", preemption_runs_both_threads),
```
e:
```rust
use core::sync::atomic::{AtomicU64, Ordering};

static RAN: AtomicU64 = AtomicU64::new(0);
static PING: AtomicU64 = AtomicU64::new(0);
static SPIN_A: AtomicU64 = AtomicU64::new(0);
static SPIN_B: AtomicU64 = AtomicU64::new(0);

fn mark_ran() {
    RAN.store(1, Ordering::SeqCst);
    loop {
        crate::sched::yield_now();
    }
}

fn spawned_thread_runs() -> Result<(), &'static str> {
    crate::sched::spawn(mark_ran);
    for _ in 0..100 {
        if RAN.load(Ordering::SeqCst) == 1 {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("thread criada nunca executou")
}

fn ping() {
    loop {
        PING.fetch_add(1, Ordering::SeqCst);
        crate::sched::yield_now();
    }
}

fn yield_alternates_threads() -> Result<(), &'static str> {
    crate::sched::spawn(ping);
    let before = PING.load(Ordering::SeqCst);
    for _ in 0..10 {
        crate::sched::yield_now();
    }
    if PING.load(Ordering::SeqCst) > before {
        Ok(())
    } else {
        Err("yield_now nao passou a CPU para a outra thread")
    }
}

fn spin_a() {
    loop {
        SPIN_A.fetch_add(1, Ordering::SeqCst);
    }
}

fn spin_b() {
    loop {
        SPIN_B.fetch_add(1, Ordering::SeqCst);
    }
}

fn preemption_runs_both_threads() -> Result<(), &'static str> {
    crate::sched::spawn(spin_a);
    crate::sched::spawn(spin_b);
    // Neither thread yields; only the timer can make both progress.
    for _ in 0..2000 {
        if SPIN_A.load(Ordering::SeqCst) > 0 && SPIN_B.load(Ordering::SeqCst) > 0 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("o timer nao preemptou as threads que nao cedem a CPU")
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64 context` e `cargo xtask ktest` → falhas de compilação.

- [ ] **Step 3: Troca de contexto em assembly**

`crates/arch-x86_64/src/context.rs`:
```rust
//! Kernel-thread context switch. Saves the callee-saved registers of the
//! System V AMD64 ABI on the current stack, swaps stacks, restores and returns.

pub const fn initial_stack_words() -> usize {
    7
}

#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.global switch_context
switch_context:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
"#
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Saves the current stack pointer into `*save_rsp` and resumes the thread whose
    /// saved stack pointer is `load_rsp`.
    pub fn switch_context(save_rsp: *mut u64, load_rsp: u64);
}
```
Em `lib.rs`, `pub mod context;`.

- [ ] **Step 4: Scheduler**

`kernel/src/sched.rs`:
```rust
//! Round-robin scheduler for kernel threads on a single CPU.
//! A new thread's stack is pre-built so the first `switch_context` into it
//! "returns" into `thread_start`, which enables interrupts and calls the entry.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use arch_x86_64::interrupts::IrqMutex;

const STACK_SIZE: usize = 64 * 1024;

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
    let boot = Thread { rsp: 0, entry: None, _stack: None };
    *SCHED.lock() = Some(Scheduler { threads: alloc::vec![boot], current: 0 });
}

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
        p.add(6).write(thread_start as usize as u64);
    }
    let mut guard = SCHED.lock();
    let s = guard.as_mut().expect("scheduler nao iniciado");
    s.threads.push(Thread { rsp, entry: Some(entry), _stack: Some(stack) });
    s.threads.len() - 1
}

/// Switches to the next thread. Interrupts stay disabled across the switch
/// (the IrqMutex guard is dropped only after we resume).
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
```

Nota para o implementador: o guard do `SCHED` é solto **antes** de `switch_context` (o bloco termina antes da chamada), então a próxima thread consegue tomar o lock. O ponteiro `save` aponta para dentro do `Vec` de threads; ele só fica inválido se o `Vec` realocar, e isso só acontece em `spawn`, que roda com o lock tomado e nunca no meio de uma troca. Se você preferir eliminar essa sutileza, reserve capacidade fixa (`Vec::with_capacity(64)`) em `init` e recuse `spawn` além disso — registre a escolha no relatório.

Em `kernel/src/apic.rs`, em `on_timer`, chamar `eoi()` **antes** de `crate::sched::on_tick()` (a troca pode não voltar a este quadro por um tempo):
```rust
pub fn on_timer() {
    TICKS.fetch_add(1, Ordering::SeqCst);
    eoi();
    crate::sched::on_tick();
}
```

Em `kernel/src/main.rs`: `mod sched;` e chamar `sched::init();` logo antes de `apic::init();`.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test -p arch-x86_64`, `cargo xtask ktest` → passam (`variante ktest ok (15 teste(s))`).

Se `preemption_runs_both_threads` falhar porque o thread de boot (que roda os testes) nunca volta a rodar, isso é esperado com round-robin justo: ele volta a cada volta completa. Se mesmo assim falhar, reduza `TIMER_INITIAL_COUNT` para gerar mais ticks e registre.

- [ ] **Step 6: Commit**

```bash
git add crates kernel
git commit -m "feat(kernel): scheduler round-robin com troca de contexto e preempcao pelo timer

Source: docs System V AMD64 ABI (callee-saved registers); Intel SDM Vol. 3A, capitulo 6
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 5: Alvo VMware (`cargo xtask vmware`)

**Files:**
- Create: `xtask/src/vmware.rs`, `xtask/src/disk.rs`
- Modify: `xtask/src/main.rs`, `xtask/Cargo.toml` (dependência `fatfs` e `gpt` para montar a imagem), `README.md`

**Interfaces:**
- Produces (`disk.rs`): `pub fn build_uefi_image(esp_dir: &Path, out: &Path) -> crate::Result<()>` — cria uma imagem de disco raw com tabela GPT e uma partição de sistema EFI FAT32 contendo o conteúdo de `esp_dir`.
- Produces (`vmware.rs`): `pub fn find_vmrun() -> crate::Result<PathBuf>`; `pub fn vmx_contents(disk_vmdk: &str, serial_file: &str) -> String`; `pub fn run(esp: &Path) -> crate::Result<()>` — converte a imagem, escreve o `.vmx`, inicia via `vmrun -T ws start <vmx> nogui`, espera, lê a serial e para a VM.
- `cargo xtask vmware`: gera a imagem a partir da mesma ESP do `build` e roda no VMware, imprimindo a serial.

- [ ] **Step 1: Testes que falham (host)**

`xtask/src/vmware.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmx_boots_uefi_with_serial_to_file() {
        let vmx = vmx_contents("tukos.vmdk", "serial.log");
        assert!(vmx.contains("firmware = \"efi\""));
        assert!(vmx.contains("tukos.vmdk"));
        assert!(vmx.contains("serial0.fileType = \"file\""));
        assert!(vmx.contains("serial0.fileName = \"serial.log\""));
        assert!(!vmx.to_lowercase().contains("windows"), "guestOS nao pode se passar por Windows");
    }

    #[test]
    fn find_vmrun_reports_missing() {
        let msg = missing_vmrun_message(&[PathBuf::from("/nao/existe/vmrun")]);
        assert!(msg.contains("vmrun"));
        assert!(msg.contains("/nao/existe/vmrun"));
        assert!(msg.contains("TUKOS_VMRUN"));
    }
}
```

`xtask/src/disk.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_has_gpt_and_bootloader() {
        let dir = std::env::temp_dir().join("tukos-disk-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("EFI/BOOT")).unwrap();
        std::fs::write(dir.join("EFI/BOOT/BOOTX64.EFI"), b"fake").unwrap();
        let out = dir.join("disk.img");
        build_uefi_image(&dir, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        // Protective MBR signature and GPT header at LBA 1.
        assert_eq!(&bytes[510..512], &[0x55, 0xAA]);
        assert_eq!(&bytes[512..520], b"EFI PART");
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p xtask` → falha de compilação.

- [ ] **Step 3: Implementar a imagem de disco**

Em `xtask/Cargo.toml`: `[dependencies] fatfs = "0.3"`, `gpt = "4"`. Implemente `build_uefi_image` com uma imagem de 64 MiB: GPT com uma partição do tipo "EFI System" começando em 1 MiB, formatada FAT32 com `fatfs` e com a árvore de `esp_dir` copiada para dentro dela. Siga a documentação das crates `gpt` 4 e `fatfs` 0.3 no docs.rs para os nomes exatos das funções; o comportamento exigido é o do teste acima. Se alguma das duas crates não suportar o fluxo no Windows, registre e use a alternativa mais simples que passe no teste.

- [ ] **Step 4: Implementar o alvo VMware**

`xtask/src/vmware.rs` (acima dos testes):
```rust
//! Boots TukOS in VMware Workstation through `vmrun`, with the serial port written to a file.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const BOOT_WAIT: Duration = Duration::from_secs(30);

pub fn missing_vmrun_message(tried: &[PathBuf]) -> String {
    let mut msg = String::from("vmrun nao encontrado. Caminhos testados:\n");
    for p in tried {
        msg.push_str(&format!("  - {}\n", p.display()));
    }
    msg.push_str("Instale o VMware Workstation ou defina TUKOS_VMRUN.");
    msg
}

pub fn find_vmrun() -> crate::Result<PathBuf> {
    if let Ok(p) = std::env::var("TUKOS_VMRUN") {
        return Ok(PathBuf::from(p));
    }
    let candidates = vec![
        PathBuf::from(r"C:\Program Files\VMware\VMware Workstation\vmrun.exe"),
        PathBuf::from(r"C:\Program Files (x86)\VMware\VMware Workstation\vmrun.exe"),
        PathBuf::from("/usr/bin/vmrun"),
    ];
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| missing_vmrun_message(&candidates))
}

pub fn vmx_contents(disk_vmdk: &str, serial_file: &str) -> String {
    format!(
        r#".encoding = "UTF-8"
config.version = "8"
virtualHW.version = "21"
displayName = "TukOS"
guestOS = "other-64"
firmware = "efi"
memsize = "512"
numvcpus = "1"
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.fileName = "{disk_vmdk}"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.fileName = "{serial_file}"
"#
    )
}

pub fn run(esp: &Path) -> crate::Result<()> {
    let vmrun = find_vmrun()?;
    let dir = crate::util::workspace_root().join("target").join("vmware");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let raw = dir.join("tukos.img");
    crate::disk::build_uefi_image(esp, &raw)?;
    let vmdk = dir.join("tukos.vmdk");
    write_flat_vmdk_descriptor(&raw, &vmdk)?;

    let vmx = dir.join("tukos.vmx");
    let serial = dir.join("serial.log");
    std::fs::write(&vmx, vmx_contents("tukos.vmdk", "serial.log")).map_err(|e| e.to_string())?;

    crate::util::run(Command::new(&vmrun).args(["-T", "ws", "start"]).arg(&vmx).arg("gui"))?;
    let start = Instant::now();
    let mut text = String::new();
    while start.elapsed() < BOOT_WAIT {
        std::thread::sleep(Duration::from_secs(2));
        text = std::fs::read_to_string(&serial).unwrap_or_default();
        if text.contains("TukOS pronto") || text.contains("@@TUKOS-PANIC") {
            break;
        }
    }
    let _ = Command::new(&vmrun).args(["-T", "ws", "stop"]).arg(&vmx).arg("hard").status();
    println!("--- serial (VMware) ---\n{text}");
    if text.contains("TukOS pronto") {
        Ok(())
    } else {
        Err("o TukOS nao chegou a 'TukOS pronto' no VMware (veja a serial acima)".into())
    }
}

/// Writes a monolithic-flat VMDK descriptor pointing at the raw image.
fn write_flat_vmdk_descriptor(raw: &Path, vmdk: &Path) -> crate::Result<()> {
    let size = std::fs::metadata(raw).map_err(|e| e.to_string())?.len();
    let sectors = size / 512;
    let name = raw.file_name().unwrap().to_string_lossy();
    let desc = format!(
        "# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\ncreateType=\"monolithicFlat\"\n\nRW {sectors} FLAT \"{name}\" 0\n\nddb.adapterType = \"lsilogic\"\nddb.virtualHWVersion = \"21\"\n"
    );
    std::fs::write(vmdk, desc).map_err(|e| e.to_string())
}
```

Em `xtask/src/main.rs`: `mod disk; mod vmware;`, acrescentar `vmware` a `USAGE` e o braço:
```rust
        Some("vmware") => image::build(&[]).and_then(|esp| vmware::run(&esp)),
```

O `vmrun` abre a janela do VMware (`gui`), conforme a preferência do usuário de ver a execução. No `README.md`, acrescentar a linha `| cargo xtask vmware | dá boot no VMware Workstation (janela visível) e mostra a serial |` e a variável `TUKOS_VMRUN`.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test -p xtask` → passam.
Run: `cargo xtask vmware` → a janela do VMware abre, e a saída termina com a serial contendo `TukOS pronto`.

Se o boot no VMware falhar por motivo de ambiente (por exemplo, a versão de hardware virtual `21` não suportada pela versão instalada, ou o VMware recusar o disco), ajuste `virtualHW.version` para a maior versão que o `vmrun` aceitar e registre. Se mesmo assim não der boot por limitação do ambiente, deixe o comando implementado e testado nos testes de host, e relate o motivo como `DONE_WITH_CONCERNS` — este alvo não pode bloquear o restante do plano.

- [ ] **Step 6: Commit**

```bash
git add xtask README.md Cargo.lock
git commit -m "feat(xtask): alvo VMware com imagem GPT/FAT32 e serial em arquivo

Source: docs VMware Workstation vmrun e formato .vmx/.vmdk; UEFI spec (GPT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os cinco gates de Global Constraints passam.
- O Plano 2c cobre: modo usuário via SYSCALL/SYSRET, `swapgs` + GS por CPU, `gdt::set_rsp0`, PML4 por processo e parar de marcar tabelas intermediárias como `USER`.
