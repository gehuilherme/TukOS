# TukOS Plano 2c — Modo usuário: SYSCALL/SYSRET e uma ida ao Ring 3

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Uma thread de kernel do TukOS entra em Ring 3, executa código de usuário que faz chamadas `syscall`, o kernel as atende e devolve o controle à thread de kernel.

**Architecture:** Primeiro um bloco por-CPU acessível por `GS` (com `swapgs`) e `gdt::set_rsp0`, para que a entrada de syscall e as exceções em Ring 3 tenham uma pilha de kernel. Depois a configuração de SYSCALL/SYSRET (MSRs `EFER`, `STAR`, `LSTAR`, `FMASK`), o stub de entrada em assembly e uma tabela de dispatch. Por fim, uma ida síncrona ao Ring 3: o kernel monta uma página de código de usuário e uma pilha de usuário, salva seu próprio contexto no bloco por-CPU, faz `iretq` para Ring 3 com **interrupções desligadas** (sem preempção durante a excursão, para esta primeira versão ser determinística), o código de usuário faz uma syscall de "escrita" e uma de "saída", e a de saída restaura o contexto do kernel salvo.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `limine` 0.5, `spin` 0.10, `mm`, `arch-x86_64` (do projeto), QEMU + OVMF.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 4: "Modo usuário: entrada e saída via `SYSCALL/SYSRET`, com uma tabela de dispatch de syscalls"). Sucede o Plano 2b (interrupções e scheduler), já na `main`.

**Fora deste plano (vai para o Plano 3):** espaços de endereçamento por processo (PML4 por processo e parametrizar `map_page` por raiz), preempção enquanto em Ring 3, processos e handles de verdade, `ntdll`/loader de PE. Esta primeira ida ao Ring 3 é síncrona, com interrupções desligadas, no espaço de endereços ativo (compartilhado).

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64, um núcleo. Código dependente de CPU vive em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft.
- Clean room nível 0 apenas: Intel SDM Vol. 3A (capítulos 5 "Protection", 6) e Vol. 2 (`SYSCALL`, `SYSRET`, `SWAPGS`); AMD64 APM Vol. 2 (fast system call). O conjunto de syscalls é do TukOS, não do Windows.
- Commits de comportamento de hardware declaram `Source: docs <documento>`.
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- Seletores (do Plano 1, não mudam): KERNEL_CS 0x08, KERNEL_DS 0x10, USER_DS 0x1b, USER_CS 0x23, TSS_SEL 0x28. STAR deriva deles: `STAR[47:32]=KERNEL_CS`, `STAR[63:48]=KERNEL_DS`.
- MSRs: `IA32_EFER=0xC000_0080` (bit 0 = SCE), `IA32_STAR=0xC000_0081`, `IA32_LSTAR=0xC000_0082`, `IA32_FMASK=0xC000_0084`, `IA32_GS_BASE=0xC000_0101`, `IA32_KERNEL_GS_BASE=0xC000_0102`.
- Números de syscall do TukOS nesta fase: `0 = SYS_EXIT`, `1 = SYS_WRITE_MARK` (de teste: registra o argumento).
- Endereços virtuais de usuário (metade baixa, canônicos): código em `0x0000_0000_0040_0000`, pilha em `0x0000_0000_0050_0000` (topo em `0x0000_0000_0051_0000`).
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **`swapgs` na direção errada ou em dobro** deixa `GS` apontando para memória de usuário no kernel (ou vice-versa), e o primeiro acesso `gs:[...]` corrompe ou falha; a entrada de syscall faz exatamente um `swapgs` ao entrar e um ao sair, e o retorno ao Ring 3 deixa o `GS` do kernel em `KERNEL_GS_BASE`. Coberto por `syscall_roundtrip_preserves_gs` (Task 3).
2. **`rsp0` não definido antes de entrar em Ring 3** faria uma exceção em Ring 3 usar uma pilha inválida e triple-fault; `enter_user` define `rsp0` antes do `iretq`. Coberto por `rsp0_set_before_user` (Task 1) e pelo próprio fato de o roundtrip (Task 3) completar.
3. **`STAR`/`FMASK` mal codificados** levam a `SYSRET` a carregar seletores errados (CS/SS trocados) ou a entrada de syscall a rodar com `IF` ligado; `STAR` usa KERNEL_CS/KERNEL_DS e `FMASK` limpa `IF` e `DF`. Coberto por `star_encodes_kernel_selectors` e `fmask_clears_if_and_df` (Task 2).
4. **A página de código de usuário mapeada sem `USER`, ou como `NO_EXECUTE`** causa #PF/#GP ao entrar no Ring 3; o código é mapeado `USER` e executável, a pilha `USER|WRITABLE|NO_EXECUTE`. Coberto pelo roundtrip completar (Task 3).
5. **A syscall de saída que faz `sysret` em vez de restaurar o contexto do kernel** nunca devolveria o controle à thread de kernel; `SYS_EXIT` restaura `kernel_return_rsp` e retorna de `enter_user`, sem `sysret`. Coberto por `enter_user_returns_after_exit` (Task 3).

---

### Task 1: Bloco por-CPU acessível por GS e `set_rsp0`

**Files:**
- Create: `crates/arch-x86_64/src/percpu.rs`
- Modify: `crates/arch-x86_64/src/lib.rs`, `crates/arch-x86_64/src/cpu.rs` (adicionar `swapgs`), `crates/arch-x86_64/src/gdt.rs` (adicionar `set_rsp0`)
- Modify: `kernel/src/ktest.rs` (novo teste)

**Interfaces:**
- Produces (`percpu.rs`):
  - `#[repr(C)] pub struct PerCpu { pub kernel_rsp: u64, pub user_rsp_scratch: u64, pub kernel_return_rsp: u64 }`
  - `pub const OFF_KERNEL_RSP: usize = 0; pub const OFF_USER_RSP_SCRATCH: usize = 8; pub const OFF_KERNEL_RETURN_RSP: usize = 16;`
  - `pub const fn new() -> PerCpu`
  - (só `target_os = "none"`) `pub unsafe fn install(block: &'static PerCpu)` — escreve o endereço em `IA32_GS_BASE` **e** `IA32_KERNEL_GS_BASE`; `pub fn gs_base() -> u64` (lê `IA32_GS_BASE`)
- Produces (`cpu.rs`): `pub unsafe fn swapgs()`.
- Produces (`gdt.rs`): (só `target_os = "none"`) `pub unsafe fn set_rsp0(top: u64)` — grava `TSS.rsp[0]`; `pub fn rsp0() -> u64`.

- [ ] **Step 1: Testes que falham**

Em `percpu.rs`:
```rust
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
```

Em `kernel/src/ktest.rs`, adicionar `("rsp0_set_before_user", rsp0_set_before_user),` e:
```rust
fn rsp0_set_before_user() -> Result<(), &'static str> {
    // A kernel stack top we control for this check.
    static STACK: [u8; 4096] = [0; 4096];
    let top = (&STACK as *const _ as u64) + 4096;
    unsafe { arch_x86_64::gdt::set_rsp0(top) };
    if arch_x86_64::gdt::rsp0() != top {
        return Err("rsp0 nao refletiu o valor gravado");
    }
    // Per-CPU block must be installed (GS base non-zero) by kmain before tests.
    if arch_x86_64::percpu::gs_base() == 0 {
        return Err("GS base nao instalado");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64 percpu` e `cargo xtask ktest` → falhas de compilação.

- [ ] **Step 3: Implementar**

`crates/arch-x86_64/src/percpu.rs`:
```rust
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
        PerCpu { kernel_rsp: 0, user_rsp_scratch: 0, kernel_return_rsp: 0 }
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
    /// `block` must outlive all CPU use (a `'static`); call once on the boot CPU.
    pub unsafe fn install(block: &'static PerCpu) {
        let addr = block as *const PerCpu as u64;
        unsafe {
            write_msr(IA32_GS_BASE, addr);
            write_msr(IA32_KERNEL_GS_BASE, addr);
        }
    }

    pub fn gs_base() -> u64 {
        unsafe { read_msr(IA32_GS_BASE) }
    }
}

#[cfg(target_os = "none")]
pub use hw::{gs_base, install};
```

Em `cpu.rs`:
```rust
/// # Safety
/// Exchanges GS base with IA32_KERNEL_GS_BASE; only valid at defined kernel/user
/// transitions, exactly once per transition.
#[inline]
pub unsafe fn swapgs() {
    unsafe { asm!("swapgs", options(nostack, preserves_flags)) };
}
```

Em `gdt.rs`, `mod hw`, acrescentar (a `TSS` já tem `pub rsp: [u64; 3]`):
```rust
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
```
e `pub use hw::{init, ist_tops, rsp0, set_rsp0};`.

Em `lib.rs`, `pub mod percpu;`.

Em `kernel/src/main.rs`, instalar o bloco por-CPU logo após `gdt::init()`/`idt::init()`:
```rust
    static PERCPU: arch_x86_64::percpu::PerCpu = arch_x86_64::percpu::PerCpu::new();
    unsafe { arch_x86_64::percpu::install(&PERCPU) };
```
(coloque o `static PERCPU` em escopo de módulo se preferir; precisa ser `'static`.)

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p arch-x86_64` e `cargo xtask ktest` → passam (ktest agora 16 testes).

- [ ] **Step 5: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(arch): bloco por-CPU via GS e gdt::set_rsp0

Source: docs Intel SDM Vol. 3A (SWAPGS, IA32_KERNEL_GS_BASE); 7.2.1 (TSS rsp0)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Configuração de SYSCALL/SYSRET, stub de entrada e dispatch

**Files:**
- Create: `crates/arch-x86_64/src/syscall.rs` (codificações puras + stub de entrada em asm)
- Modify: `crates/arch-x86_64/src/lib.rs`
- Create: `kernel/src/syscall.rs`
- Modify: `kernel/src/main.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`syscall.rs`, puras): `pub const fn star_value(kernel_cs: u16, kernel_ds: u16) -> u64` (`((kernel_ds as u64) << 48) | ((kernel_cs as u64) << 32)`); `pub const FMASK_VALUE: u64 = 0x600` (limpa IF bit 9 e DF bit 10); `pub const EFER: u32 = 0xC000_0080; pub const STAR: u32 = 0xC000_0081; pub const LSTAR: u32 = 0xC000_0082; pub const FMASK: u32 = 0xC000_0084;`
- Produces (`syscall.rs`, `target_os = "none"`): símbolo `tukos_syscall_entry` (asm); `pub unsafe fn init(entry: u64)` — liga `EFER.SCE`, grava `STAR`, `LSTAR=entry`, `FMASK`; `pub fn set_dispatch(f: extern "C" fn(u64, u64, u64, u64, u64) -> u64)`.
- Produces (`kernel/src/syscall.rs`): `pub fn init()` (registra o dispatch e chama `arch::syscall::init(tukos_syscall_entry)`); `extern "C" fn dispatch(nr, a0, a1, a2, a3) -> u64`; `pub static LAST_MARK: AtomicU64`; `pub const SYS_EXIT: u64 = 0; pub const SYS_WRITE_MARK: u64 = 1;`.

- [ ] **Step 1: Testes que falham (host)**

Em `syscall.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_encodes_kernel_selectors() {
        // KERNEL_CS=0x08, KERNEL_DS=0x10. SYSCALL loads CS=0x08/SS=0x10;
        // SYSRET (64-bit) loads CS=0x10+16|3=0x23, SS=0x10+8|3=0x1b.
        let star = star_value(0x08, 0x10);
        assert_eq!((star >> 32) & 0xFFFF, 0x08);
        assert_eq!((star >> 48) & 0xFFFF, 0x10);
    }

    #[test]
    fn fmask_clears_if_and_df() {
        assert!(FMASK_VALUE & (1 << 9) != 0, "limpa IF");
        assert!(FMASK_VALUE & (1 << 10) != 0, "limpa DF");
    }
}
```

Em `kernel/src/ktest.rs`, adicionar `("syscall_dispatch_records_mark", syscall_dispatch_records_mark),` e:
```rust
fn syscall_dispatch_records_mark() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    // Call the dispatch function directly (no privilege change): proves the
    // table wiring and SYS_WRITE_MARK semantics independent of the asm stub.
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let r = crate::syscall::dispatch(crate::syscall::SYS_WRITE_MARK, 0xCAFE, 0, 0, 0);
    if r != 0 {
        return Err("SYS_WRITE_MARK deveria retornar 0");
    }
    if crate::syscall::LAST_MARK.load(Ordering::SeqCst) != 0xCAFE {
        return Err("SYS_WRITE_MARK nao registrou o argumento");
    }
    Ok(())
}
```
(Para isso, `dispatch` e `LAST_MARK` precisam ser `pub` no módulo `kernel/src/syscall.rs`.)

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64 syscall` e `cargo xtask ktest` → falhas de compilação.

- [ ] **Step 3: Implementar a parte pura e os MSRs**

`crates/arch-x86_64/src/syscall.rs` (parte pura + init + stub):
```rust
//! SYSCALL/SYSRET setup and the Ring 0 entry trampoline.
//! Intel SDM Vol. 2 (SYSCALL/SYSRET), Vol. 3A chapter 5.

pub const EFER: u32 = 0xC000_0080;
pub const STAR: u32 = 0xC000_0081;
pub const LSTAR: u32 = 0xC000_0082;
pub const FMASK: u32 = 0xC000_0084;

/// RFLAGS bits cleared on syscall entry: IF (9) and DF (10).
pub const FMASK_VALUE: u64 = (1 << 9) | (1 << 10);

pub const fn star_value(kernel_cs: u16, kernel_ds: u16) -> u64 {
    ((kernel_ds as u64) << 48) | ((kernel_cs as u64) << 32)
}

#[cfg(target_os = "none")]
mod hw {
    use super::*;
    use crate::cpu::{read_msr, write_msr};
    use crate::gdt::{KERNEL_CS, KERNEL_DS};
    use core::sync::atomic::{AtomicUsize, Ordering};

    static DISPATCH: AtomicUsize = AtomicUsize::new(0);

    pub fn set_dispatch(f: extern "C" fn(u64, u64, u64, u64, u64) -> u64) {
        DISPATCH.store(f as usize, Ordering::SeqCst);
    }

    /// # Safety
    /// Call once on the boot CPU after the per-CPU block and GDT are installed.
    pub unsafe fn init(entry: u64) {
        unsafe {
            write_msr(EFER, read_msr(EFER) | 1); // SCE
            write_msr(STAR, star_value(KERNEL_CS, KERNEL_DS));
            write_msr(LSTAR, entry);
            write_msr(FMASK, FMASK_VALUE);
        }
    }

    /// Called by the asm stub with the raw syscall arguments.
    #[unsafe(no_mangle)]
    extern "C" fn tukos_syscall_dispatch(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
        let raw = DISPATCH.load(Ordering::SeqCst);
        if raw == 0 {
            return u64::MAX;
        }
        let f: extern "C" fn(u64, u64, u64, u64, u64) -> u64 = unsafe { core::mem::transmute(raw) };
        f(nr, a0, a1, a2, a3)
    }
}

#[cfg(target_os = "none")]
pub use hw::{init, set_dispatch};

#[cfg(target_os = "none")]
unsafe extern "C" {
    pub fn tukos_syscall_entry();
}
```

O stub de entrada (no mesmo arquivo, em `global_asm!`, só `target_os = "none"`). Convenção de entrada do `SYSCALL`: `rcx`=RIP do usuário, `r11`=RFLAGS do usuário, `rax`=número, `rdi,rsi,rdx,r10`=args (ABI de syscall: o 4º arg vem em `r10`, não `rcx`). O stub faz `swapgs`, troca para a pilha do kernel, chama o dispatch com a ABI C (`rdi=nr, rsi=a0, rdx=a1, rcx=a2, r8=a3`), e volta com `sysretq`:
```rust
#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.global tukos_syscall_entry
tukos_syscall_entry:
    swapgs
    mov gs:[{off_user_rsp}], rsp         // save user rsp
    mov rsp, gs:[{off_kernel_rsp}]       // load kernel stack
    push rcx                             // user RIP
    push r11                             // user RFLAGS
    // Reorder into the C ABI for tukos_syscall_dispatch(nr,a0,a1,a2,a3):
    mov r9,  r8                          // a3 (5th C arg) <- syscall r8? no: a3 is in r10
    mov r8,  r10                         // a3 -> C 5th arg (r8)
    mov rcx, rdx                         // a2 -> C 4th arg (rcx)
    mov rdx, rsi                         // a1 -> C 3rd arg (rdx)
    mov rsi, rdi                         // a0 -> C 2nd arg (rsi)
    mov rdi, rax                         // nr -> C 1st arg (rdi)
    call tukos_syscall_dispatch
    // return value in rax
    pop r11                              // user RFLAGS
    pop rcx                              // user RIP
    mov rsp, gs:[{off_user_rsp}]         // restore user rsp
    swapgs
    sysretq
"#,
    off_user_rsp = const crate::percpu::OFF_USER_RSP_SCRATCH,
    off_kernel_rsp = const crate::percpu::OFF_KERNEL_RSP,
);
```
Nota para o implementador: a linha `mov r9, r8` acima é um engano de rascunho — **remova-a**. A reordenação correta dos argumentos da ABI de syscall (`rax`=nr, `rdi,rsi,rdx,r10`=a0..a3) para a ABI C de `tukos_syscall_dispatch` (`rdi,rsi,rdx,rcx,r8`) é: `r8<-r10`, `rcx<-rdx`, `rdx<-rsi`, `rsi<-rdi`, `rdi<-rax`, **nessa ordem**, para não sobrescrever um registrador antes de lê-lo. Verifique a ordem com cuidado (é um ponto clássico de bug) e cubra-a implicitamente pelo roundtrip da Task 3, que passa argumentos reais. `SYS_EXIT` (Task 3) não usa este caminho de `sysretq`.

Em `lib.rs`, `pub mod syscall;`.

- [ ] **Step 4: Implementar o dispatch do kernel**

`kernel/src/syscall.rs`:
```rust
//! TukOS system call dispatch.

use core::sync::atomic::{AtomicU64, Ordering};

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE_MARK: u64 = 1;

pub static LAST_MARK: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    arch_x86_64::syscall::set_dispatch(dispatch);
    unsafe { arch_x86_64::syscall::init(arch_x86_64::syscall::tukos_syscall_entry as usize as u64) };
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
```

Em `kernel/src/main.rs`: `mod syscall;` e chamar `syscall::init();` após instalar o bloco por-CPU (Task 1) e antes de `kprintln!("TukOS pronto")`.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test -p arch-x86_64`, `cargo xtask ktest` → passam (ktest 17 testes). A chamada direta a `dispatch` valida a fiação; o stub de asm é exercido na Task 3.

- [ ] **Step 6: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(kernel): configuracao de SYSCALL/SYSRET, stub de entrada e dispatch

Source: docs Intel SDM Vol. 2 (SYSCALL/SYSRET); Vol. 3A cap. 5
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Ida ao Ring 3 e volta pela syscall de saída

**Files:**
- Create: `crates/arch-x86_64/src/usermode.rs` (asm: `enter_user`, `tukos_user_return`)
- Modify: `crates/arch-x86_64/src/lib.rs`
- Create: `kernel/src/user_demo.rs` (payload de usuário + montagem da página + teste)
- Modify: `kernel/src/syscall.rs` (SYS_EXIT dispara o retorno), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`usermode.rs`, `target_os = "none"`):
  - `pub unsafe fn enter_user(entry: u64, user_stack_top: u64) -> u64` — salva os registradores callee-saved e o `rsp` do kernel em `gs:[OFF_KERNEL_RETURN_RSP]`, define `gs:[OFF_KERNEL_RSP]` como a pilha de syscall (a própria pilha atual do kernel abaixo do frame salvo), monta um frame de `iretq` (SS=USER_DS, RSP=user_stack_top, RFLAGS=0x2 com IF desligado, CS=USER_CS, RIP=entry), faz `swapgs` e `iretq`. Retorna o código de saída quando `SYS_EXIT` restaura o contexto.
  - símbolo `tukos_user_return` (asm): restaura `rsp` de `gs:[OFF_KERNEL_RETURN_RSP]`, desempilha os callee-saved, coloca o código de saída em `rax` e `ret` (fazendo `enter_user` retornar).
- Produces (`kernel/src/user_demo.rs`): `pub fn run() -> u64` — mapeia a página de código (USER, executável) copiando o payload, mapeia a pilha (USER|WRITABLE|NO_EXECUTE), chama `enter_user`.
- Modifica `SYS_EXIT` para, em vez de retornar normalmente, chamar `arch_x86_64::usermode::exit_to_kernel(code)` que salta para `tukos_user_return`.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar:
```rust
    ("enter_user_returns_after_exit", enter_user_returns_after_exit),
    ("syscall_roundtrip_preserves_gs", syscall_roundtrip_preserves_gs),
```
e:
```rust
fn enter_user_returns_after_exit() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let code = crate::user_demo::run();
    if code != 0x2A {
        return Err("codigo de saida do usuario incorreto");
    }
    if crate::syscall::LAST_MARK.load(Ordering::SeqCst) != 0xCAFE {
        return Err("a syscall de escrita do usuario nao chegou ao kernel");
    }
    Ok(())
}

fn syscall_roundtrip_preserves_gs() -> Result<(), &'static str> {
    // After a full user excursion, the kernel GS base must still point at the
    // per-CPU block (a mismatched swapgs count would leave it wrong).
    let before = arch_x86_64::percpu::gs_base();
    let _ = crate::user_demo::run();
    if arch_x86_64::percpu::gs_base() != before {
        return Err("GS base do kernel mudou apos a excursao (swapgs desbalanceado)");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação (`crate::user_demo` não existe).

- [ ] **Step 3: Implementar o asm de entrada/saída do Ring 3**

`crates/arch-x86_64/src/usermode.rs`:
```rust
//! Entering Ring 3 and returning via the exit syscall. Intel SDM Vol. 3A ch. 6
//! (IRET to a less-privileged level) and Vol. 2 (SYSRET).

#[cfg(target_os = "none")]
use crate::gdt::{USER_CS, USER_DS};
#[cfg(target_os = "none")]
use crate::percpu::{OFF_KERNEL_RETURN_RSP, OFF_KERNEL_RSP};

#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.global enter_user
enter_user:
    // rdi = user entry RIP, rsi = user stack top.
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    // Save kernel rsp so the exit path can longjmp back here, and use the
    // current kernel stack as the syscall-entry stack.
    mov gs:[{off_ret}], rsp
    mov gs:[{off_ksp}], rsp
    // Build the iretq frame for Ring 3.
    push {user_ds}        // SS
    push rsi              // RSP (user stack top)
    push 0x2              // RFLAGS: reserved bit 1 set, IF clear (no preemption)
    push {user_cs}        // CS
    push rdi              // RIP (user entry)
    swapgs                // kernel GS -> user; per-CPU now in KERNEL_GS_BASE
    iretq

.global tukos_user_return
tukos_user_return:
    // rdi = exit code. Restore the kernel stack saved by enter_user and return.
    mov rsp, gs:[{off_ret}]
    mov rax, rdi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
"#,
    off_ret = const OFF_KERNEL_RETURN_RSP,
    off_ksp = const OFF_KERNEL_RSP,
    user_ds = const USER_DS as u64,
    user_cs = const USER_CS as u64,
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    pub fn enter_user(entry: u64, user_stack_top: u64) -> u64;
    pub fn tukos_user_return(exit_code: u64) -> !;
}

/// Jumps back to the kernel context saved by `enter_user`, carrying `code`.
///
/// # Safety
/// Must run on the syscall-entry kernel stack, during a user excursion started
/// by `enter_user`; `gs` must currently be the kernel per-CPU block.
#[cfg(target_os = "none")]
pub unsafe fn exit_to_kernel(code: u64) -> ! {
    unsafe { tukos_user_return(code) }
}
```
Pontos de atenção para o implementador (verificar com `-d int,cpu_reset` no QEMU se triple-faultar):
- Quando `SYS_EXIT` chega pelo stub de syscall, o stub já fez `swapgs` (GS = kernel) e já trocou para a pilha do kernel (`gs:[OFF_KERNEL_RSP]`), que é a mesma pilha onde `enter_user` salvou os callee-saved. Então `exit_to_kernel` encontra `gs:[OFF_KERNEL_RETURN_RSP]` válido e o `ret` final retorna de `enter_user`. **O caminho de `SYS_EXIT` nunca chega ao `sysretq` do stub** — `exit_to_kernel` é divergente (`!`).
- Como o stub de syscall fez `push rcx`/`push r11` antes de chamar o dispatch, e `exit_to_kernel` abandona essa pilha restaurando `rsp` de `OFF_KERNEL_RETURN_RSP`, não há desbalanceamento: a pilha de retorno é a de `enter_user`, independente do que o stub empilhou.
- O `swapgs` de `enter_user` (1) e o `swapgs` de entrada do stub (2) se equilibram: ao entrar no Ring 3, GS do kernel vai para `KERNEL_GS_BASE`; na entrada da syscall, o stub faz `swapgs` e o traz de volta para GS. Em `SYS_EXIT` não há `swapgs` de volta (não voltamos ao usuário), e o teste `syscall_roundtrip_preserves_gs` confirma que, no fim, o GS do kernel está correto.

Em `lib.rs`, `pub mod usermode;`.

- [ ] **Step 4: Payload de usuário e montagem da página**

`kernel/src/user_demo.rs`:
```rust
//! A tiny Ring 3 payload copied into a user page: it issues SYS_WRITE_MARK(0xCAFE)
//! then SYS_EXIT(0x2A). Position-independent (only `syscall` with immediates).

use mm::paging::PageFlags;

const USER_CODE_VA: u64 = 0x0000_0000_0040_0000;
const USER_STACK_VA: u64 = 0x0000_0000_0050_0000;
const USER_STACK_TOP: u64 = 0x0000_0000_0051_0000;

core::arch::global_asm!(
    r#"
.section .rodata
.global tukos_user_payload
.global tukos_user_payload_end
tukos_user_payload:
    mov rax, 1          // SYS_WRITE_MARK
    mov rdi, 0xCAFE     // mark argument
    syscall
    mov rax, 0          // SYS_EXIT
    mov rdi, 0x2A       // exit code
    syscall
    ud2                 // unreachable: SYS_EXIT does not return to user
tukos_user_payload_end:
"#
);

unsafe extern "C" {
    static tukos_user_payload: u8;
    static tukos_user_payload_end: u8;
}

fn payload_bytes() -> &'static [u8] {
    let start = (&raw const tukos_user_payload) as usize;
    let end = (&raw const tukos_user_payload_end) as usize;
    unsafe { core::slice::from_raw_parts(start as *const u8, end - start) }
}

/// Maps a user code page and a user stack, copies the payload in, enters Ring 3,
/// and returns the user's exit code.
pub fn run() -> u64 {
    let code_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ codigo de usuario");
    let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha de usuario");
    unsafe {
        // User code: present + user + executable (no NX), writable while we copy.
        crate::mem::paging::map_page(
            USER_CODE_VA,
            code_phys,
            PageFlags::USER.union(PageFlags::WRITABLE),
        )
        .expect("map codigo de usuario");
        crate::mem::paging::map_page(
            USER_STACK_VA,
            stack_phys,
            PageFlags::USER.union(PageFlags::WRITABLE).union(PageFlags::NO_EXECUTE),
        )
        .expect("map pilha de usuario");

        let bytes = payload_bytes();
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), USER_CODE_VA as *mut u8, bytes.len());

        // rsp0 for any exception taken while in Ring 3.
        let kstack_top = {
            let mut x: u64;
            core::arch::asm!("mov {}, rsp", out(reg) x);
            x
        };
        arch_x86_64::gdt::set_rsp0(kstack_top);

        let code = arch_x86_64::usermode::enter_user(USER_CODE_VA, USER_STACK_TOP);

        // Unmap and free.
        let _ = crate::mem::paging::unmap_page(USER_CODE_VA);
        let _ = crate::mem::paging::unmap_page(USER_STACK_VA);
        crate::mem::frames::free_frame(code_phys);
        crate::mem::frames::free_frame(stack_phys);
        code
    }
}
```

Em `kernel/src/syscall.rs`, mudar o braço `SYS_EXIT` do `dispatch` para divergir para o kernel:
```rust
        SYS_EXIT => {
            #[cfg(target_os = "none")]
            unsafe {
                arch_x86_64::usermode::exit_to_kernel(a0)
            }
            #[cfg(not(target_os = "none"))]
            a0
        }
```
(O `cfg(not(target_os = "none"))` mantém o `dispatch` compilável no host, caso algum teste de host o use; neste plano ele só é chamado no kernel, mas mantém a função total.)

Como `dispatch` agora pode divergir, mantenha a assinatura `extern "C" fn(...) -> u64` (o braço `SYS_EXIT` no kernel nunca retorna, os demais retornam `u64`).

Em `kernel/src/main.rs`: `mod user_demo;`.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest` → `variante ktest ok (19 teste(s))` e as demais variantes ok.

Depuração, se triple-faultar: rode o QEMU manualmente com `-d int,cpu_reset -no-reboot` (veja `xtask/src/qemu.rs::base_args`) e confira, na ordem: o payload foi copiado para a página USER executável; o frame de `iretq` tem SS/CS de usuário e RFLAGS `0x2`; o stub de syscall faz exatamente um `swapgs` na entrada; `OFF_KERNEL_RSP` aponta para uma pilha de kernel válida; a reordenação de argumentos do stub (Task 2) está correta.

- [ ] **Step 6: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(kernel): ida ao Ring 3 e volta pela syscall de saida

Source: docs Intel SDM Vol. 3A cap. 6 (IRET entre niveis); Vol. 2 (SYSRET)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os cinco gates de Global Constraints passam. O marco demonstrável: um autoteste entra em Ring 3, o código de usuário faz `syscall`, o kernel registra `0xCAFE` e devolve o código de saída `0x2A`.
- Limitações desta fase (todas para o Plano 3): a excursão ao Ring 3 roda com interrupções desligadas (sem preempção); usa o espaço de endereços ativo compartilhado (sem PML4 por processo); o payload é copiado, não carregado de um `.exe`.
- Plano 3 (processos): PML4 por processo (parametrizar `map_page`/`unmap_page`/`translate` por raiz), preempção em Ring 3 (salvar/restaurar o estado completo pelo trap frame, com `swapgs` condicional na entrada das exceções conforme o RPL de CS salvo), gerenciador de objetos e tabela de handles, e então `ntdll` + loader de PE.
