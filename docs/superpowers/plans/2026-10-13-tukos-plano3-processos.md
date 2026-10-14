# TukOS Plano 3 — Processos de usuário preemptivos e espaços de endereçamento

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** O TukOS cria processos de usuário, cada um com seu próprio espaço de endereçamento (PML4), roda-os em Ring 3 e os preempta pelo timer, tomando interrupções e exceções com segurança enquanto estão em Ring 3.

**Architecture:** A troca de contexto cooperativa já existente (`switch_context`, que salva os callee-saved e troca a pilha de kernel) continua sendo a espinha dorsal. Uma thread de usuário difere de uma thread de kernel em três coisas: (1) sua pilha de kernel inicial é montada para que o primeiro `switch_context` "retorne" num trampolim que faz `iretq` para o Ring 3 com `IF` ligado; (2) ela tem um `cr3` próprio (PML4 do processo) que `switch_to_next` carrega na troca; (3) ela tem uma pilha de kernel dedicada cujo topo vira `rsp0` na troca, para a próxima interrupção em Ring 3 cair na pilha certa. A entrada de trap passa a fazer `swapgs` condicional conforme o RPL do CS salvo, para uma interrupção em Ring 3 rodar com o GS do kernel. O mapeador é parametrizado por raiz de PML4, e um `AddressSpace` novo clona a metade alta (kernel) numa PML4 nova.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `limine` 0.5, `spin` 0.10, `mm`, `arch-x86_64` (do projeto), QEMU + OVMF.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 4, item "Executivo": processos; e a seção 4.4 "espaço de endereçamento por processo"). Sucede o Plano 2c (modo usuário), já na `main`.

**Fora deste plano (planos seguintes):** gerenciador de objetos e tabela de handles, `ntdll` e o loader de PE (rodar um `.exe` real), sistema de arquivos e disco. Este plano entrega a base de processos: espaços de endereçamento isolados e multitarefa preemptiva de código de usuário. Os payloads de usuário ainda são montados pelo kernel (position-independent), não carregados de um `.exe`.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64, um núcleo. Código dependente de CPU vive em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft.
- Clean room nível 0 apenas: Intel SDM Vol. 3A (capítulos 4 "Paging", 6 "Interrupt Handling", SWAPGS); AMD64 APM Vol. 2.
- Commits de comportamento de hardware declaram `Source: docs <documento>`.
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- Seletores (não mudam): KERNEL_CS 0x08, KERNEL_DS 0x10, USER_DS 0x1b, USER_CS 0x23.
- A metade alta do espaço de endereços (índice 256..512 da PML4, VAs `0xffff_8000_...` acima) é do kernel e é compartilhada por todos os processos. A metade baixa (índice 0..256) é do usuário.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **Trocar de `cr3` para uma PML4 que não mapeia a pilha de kernel atual** triple-faulta na próxima instrução; `AddressSpace::new_user` clona todas as entradas da metade alta da PML4 do kernel, então a pilha de kernel e o código continuam mapeados em todo processo. Coberto por `kernel_mapped_in_new_address_space` (Task 2).
2. **`swapgs` na entrada/saída de trap sem checar o RPL do CS salvo** faria uma exceção em Ring 0 trocar o GS indevidamente (ou uma em Ring 3 rodar com o GS do usuário); a entrada faz `swapgs` só se o CS salvo tem RPL 3, e a saída idem. Coberto por `ring0_exception_keeps_gs` (Task 1) e pelo demo preemptivo (Task 4).
3. **Preemptar uma thread de usuário sem `set_rsp0` para a pilha de kernel dela** faz a interrupção seguinte cair numa pilha errada; `switch_to_next` chama `set_rsp0(target.kernel_stack_top)` em toda troca para threads de usuário. Coberto por `two_user_processes_preempt` completar (Task 4).
4. **A primeira entrada de uma thread de usuário com uma pilha de kernel mal montada** não chega ao Ring 3; a pilha é montada com o frame de `switch_context` seguido de um frame de `iretq` que o trampolim consome. Coberto por `user_process_runs_in_ring3` (Task 3).
5. **Liberar um `AddressSpace` sem liberar os frames das tabelas e das páginas de usuário** vaza memória a cada processo; `AddressSpace::drop` percorre só a metade baixa e libera as tabelas e os frames folha, nunca tocando a metade alta compartilhada. Coberto por `address_space_drop_frees_user_frames` (Task 2).

---

### Task 1: `swapgs` condicional na entrada e saída de trap

**Files:**
- Modify: `crates/arch-x86_64/src/trap.rs` (`tukos_isr_common`)
- Modify: `kernel/src/ktest.rs` (novo teste)

**Interfaces:**
- Produces: a entrada de exceção/IRQ faz `swapgs` sse o CS salvo pela CPU tiver RPL 3, e desfaz antes do `iretq`. Sem mudança de assinatura Rust.

Contexto do layout atual de `tukos_isr_common` (não mudar a ordem dos pushes): ao entrar em `tukos_isr_common`, o `rsp` aponta para o `vector` empilhado pelo stub; acima dele estão `error_code` (`[rsp+8]`), e o frame da CPU `rip` (`[rsp+16]`), `cs` (`[rsp+24]`), `rflags` (`[rsp+32]`). Depois dos 15 pushes de GPR, antes do `add rsp, 16` final, o `rsp` aponta para o `vector`; após `add rsp, 16` aponta para `rip`, e o `cs` fica em `[rsp+8]`.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("ring0_exception_keeps_gs", ring0_exception_keeps_gs),` e:
```rust
fn ring0_exception_keeps_gs() -> Result<(), &'static str> {
    // A Ring 0 exception (int3) must NOT swapgs: the kernel GS base must be
    // unchanged across it. (The conditional swapgs only fires for RPL 3.)
    let before = arch_x86_64::percpu::gs_base();
    arch_x86_64::cpu::int3();
    let after = arch_x86_64::percpu::gs_base();
    if before != after {
        return Err("excecao em Ring 0 trocou o GS indevidamente");
    }
    Ok(())
}
```
(Este teste passa trivialmente hoje porque não há `swapgs` na entrada; ele vira uma rede de segurança contra um `swapgs` incondicional introduzido nesta tarefa — se alguém esquecer de condicionar ao RPL, o `int3` em Ring 0 trocaria o GS e o teste falha.)

- [ ] **Step 2: Rodar e ver passar (ainda sem swapgs) e depois guardar como rede**

Run: `cargo xtask ktest` → passa (17/19 + ...). Confirme que o teste existe e passa antes de mexer no asm.

- [ ] **Step 3: Implementar o `swapgs` condicional**

Em `crates/arch-x86_64/src/trap.rs`, dentro do `global_asm!`, no início de `tukos_isr_common` (antes do primeiro `push rax`), inserir:
```asm
    // Conditional swapgs on entry: swap only if we came from Ring 3 (CS RPL == 3).
    // At this point rsp -> vector; CPU-pushed CS is at [rsp+24].
    test byte ptr [rsp+24], 3
    jz 1f
    swapgs
1:
```
E imediatamente antes do `iretq` final (depois do `add rsp, 16`, quando `rsp -> rip` e o CS está em `[rsp+8]`):
```asm
    // Conditional swapgs on exit: swap back only if returning to Ring 3.
    test byte ptr [rsp+8], 3
    jz 2f
    swapgs
2:
    iretq
```
Use rótulos numéricos locais que não colidam com os já usados no arquivo (o arquivo usa `2f` em outro `global_asm!` separado, mas rótulos numéricos são locais ao bloco `global_asm!`; ainda assim, se houver conflito dentro deste mesmo bloco, renumere para rótulos livres, p.ex. `3f`/`4f`). Não altere a ordem dos pushes/pops nem o `add rsp, 16`.

Nota de corretude para o implementador: o `test byte ptr [rsp+24], 3` lê o byte baixo do CS salvo (RPL nos 2 bits baixos). `jz` pula o `swapgs` quando o RPL é 0 (Ring 0). Na saída, o CS a retornar está em `[rsp+8]` após o `add rsp, 16`. A operação `swapgs` só é válida em Ring 0, e aqui sempre estamos em Ring 0 durante o handler, então é seguro. Limitação conhecida (registrar, para um plano futuro): um NMI que chegue na janela de um `swapgs` ainda é um problema clássico; o NMI já usa IST próprio (Plano 2b), mas a reentrância de `swapgs` por NMI não é tratada — fora do escopo deste plano.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → todas as variantes passam; `ring0_exception_keeps_gs` e `breakpoint_returns` continuam ok (um `int3` em Ring 0 não faz `swapgs`). `cargo test -p arch-x86_64` ok.

- [ ] **Step 5: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(arch): swapgs condicional na entrada e saida de trap (RPL do CS)

Source: docs Intel SDM Vol. 3A (SWAPGS; IRET entre niveis)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Mapeador parametrizado por raiz e `AddressSpace`

**Files:**
- Modify: `kernel/src/mem/paging.rs` (funções `_in(root, ...)` + wrappers na raiz ativa)
- Create: `kernel/src/mem/space.rs`
- Modify: `kernel/src/mem/mod.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`paging.rs`):
  - `pub unsafe fn map_page_in(root: u64, virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError>`
  - `pub unsafe fn unmap_page_in(root: u64, virt: u64) -> Result<(), MapError>`
  - `pub fn translate_in(root: u64, virt: u64) -> Option<u64>`
  - os atuais `map_page`/`unmap_page`/`translate` passam a ser finos wrappers que chamam `*_in(crate::arch_cr3_base(), ...)`.
- Produces (`space.rs`):
  - `pub struct AddressSpace { root: u64 }`
  - `pub fn new_user() -> AddressSpace` — aloca um frame para a PML4, zera a metade baixa (0..256) e **copia** as 256 entradas da metade alta (256..512) da PML4 do kernel ativo; retorna o espaço
  - `pub fn root(&self) -> u64`
  - `pub unsafe fn map(&self, virt, phys, flags) -> Result<(), MapError>` / `unmap` / `translate` — delegam a `*_in(self.root, ...)`
  - `impl Drop for AddressSpace` — percorre só a metade baixa (índices 0..256 da PML4) liberando frames folha e as tabelas intermediárias criadas, e por fim o frame da própria PML4; nunca libera frames da metade alta (compartilhados)

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`, adicionar:
```rust
    ("kernel_mapped_in_new_address_space", kernel_mapped_in_new_address_space),
    ("address_space_drop_frees_user_frames", address_space_drop_frees_user_frames),
```
e:
```rust
fn kernel_mapped_in_new_address_space() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let space = crate::mem::space::AddressSpace::new_user();
    // A kernel higher-half address (the heap window) must resolve in the new space
    // to the same physical frame as in the active space.
    let probe = 0xffff_ff80_0000_0000u64; // heap window base (mapped by the heap)
    let active = crate::mem::paging::translate(probe);
    let in_new = crate::mem::paging::translate_in(space.root(), probe);
    if active.is_none() {
        return Err("endereco de prova nao mapeado no espaco ativo");
    }
    if in_new != active {
        return Err("metade alta do kernel nao foi clonada no novo espaco");
    }
    // A low (user) address must be unmapped in the fresh space.
    if crate::mem::paging::translate_in(space.root(), 0x40_0000).is_some() {
        return Err("metade baixa deveria estar vazia");
    }
    let _ = PageFlags::USER;
    Ok(())
}

fn address_space_drop_frees_user_frames() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let before = crate::mem::frames::free_frame_count();
    {
        let space = crate::mem::space::AddressSpace::new_user();
        let frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
        unsafe {
            space
                .map(0x40_0000, frame, PageFlags::USER.union(PageFlags::WRITABLE))
                .map_err(|_| "map no espaco novo falhou")?;
        }
        // Dropping `space` must free: the mapped user frame, the intermediate
        // tables, and the PML4 — leaving the count where it started.
    }
    let after = crate::mem::frames::free_frame_count();
    if after != before {
        return Err("Drop do AddressSpace nao liberou todos os frames de usuario");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Parametrizar o mapeador por raiz**

Em `kernel/src/mem/paging.rs`, refatore sem mudar o comportamento: extraia o corpo de `map_page` para `map_page_in(root, virt, phys, flags)` que usa `table_mut(root)` como a tabela de nível 4 inicial (hoje usa `table_mut(crate::arch_cr3_base())`). Idem para `unmap_page_in` e `translate_in`. Depois:
```rust
pub unsafe fn map_page(virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError> {
    unsafe { map_page_in(crate::arch_cr3_base(), virt, phys, flags) }
}
pub unsafe fn unmap_page(virt: u64) -> Result<(), MapError> {
    unsafe { unmap_page_in(crate::arch_cr3_base(), virt) }
}
pub fn translate(virt: u64) -> Option<u64> {
    translate_in(crate::arch_cr3_base(), virt)
}
```
A função interna `next_table` (que cria tabelas) já recebe a tabela por ponteiro; só o ponto de partida muda. Mantenha o `invlpg` após cada `map_page_in`/`unmap_page_in` **apenas quando `root == arch_cr3_base()`** (invalidar a TLB só faz sentido para o espaço ativo; para um espaço não ativo não há entrada de TLB). Acrescente essa condição.

- [ ] **Step 4: Implementar `AddressSpace`**

`kernel/src/mem/space.rs`:
```rust
//! A per-process address space: a fresh PML4 whose higher half (kernel) is
//! cloned from the active space and whose lower half (user) starts empty.

use crate::mem::frames::{alloc_frame, free_frame, phys_to_virt};
use crate::mem::paging::{MapError, map_page_in, translate_in, unmap_page_in};
use mm::paging::{PageFlags, entry_frame, entry_present};

/// Entries 0..256 of a PML4 cover the lower (user) half; 256..512 the higher
/// (kernel) half.
const HALF: usize = 256;

pub struct AddressSpace {
    root: u64,
}

fn table(phys: u64) -> *mut u64 {
    phys_to_virt(phys) as *mut u64
}

impl AddressSpace {
    pub fn new_user() -> AddressSpace {
        let root = alloc_frame().expect("sem frame para a PML4");
        let dst = table(root);
        let src = table(crate::arch_cr3_base());
        unsafe {
            // Lower half: empty. Higher half: clone the kernel's PML4 entries.
            for i in 0..HALF {
                dst.add(i).write(0);
            }
            for i in HALF..512 {
                dst.add(i).write(core::ptr::read_volatile(src.add(i)));
            }
        }
        AddressSpace { root }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    /// # Safety
    /// `virt` must be a page-aligned lower-half (user) address.
    pub unsafe fn map(&self, virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError> {
        unsafe { map_page_in(self.root, virt, phys, flags) }
    }

    /// # Safety
    /// `virt` must be a mapped lower-half address.
    pub unsafe fn unmap(&self, virt: u64) -> Result<(), MapError> {
        unsafe { unmap_page_in(self.root, virt) }
    }

    pub fn translate(&self, virt: u64) -> Option<u64> {
        translate_in(self.root, virt)
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        // Walk only the lower half (user), freeing leaf frames and the
        // intermediate tables, then the PML4 frame itself. The higher half is
        // shared with the kernel and must never be freed here.
        unsafe { free_lower_half(self.root) };
        free_frame(self.root);
    }
}

/// Recursively frees the lower-half (indices 0..256 at the top level) subtree.
unsafe fn free_lower_half(pml4: u64) {
    let p4 = table(pml4);
    for i in 0..HALF {
        let e4 = unsafe { core::ptr::read_volatile(p4.add(i)) };
        if !entry_present(e4) {
            continue;
        }
        unsafe { free_table(entry_frame(e4), 3) };
    }
}

/// Frees a page-table subtree rooted at `phys` of the given `level`
/// (3 = PDPT, 2 = PD, 1 = PT). At level 1 the entries are leaf frames.
unsafe fn free_table(phys: u64, level: u8) {
    let t = table(phys);
    for i in 0..512 {
        let e = unsafe { core::ptr::read_volatile(t.add(i)) };
        if !entry_present(e) {
            continue;
        }
        let child = entry_frame(e);
        if level == 1 {
            free_frame(child); // leaf user page
        } else {
            unsafe { free_table(child, level - 1) };
        }
    }
    free_frame(phys); // the table itself
}
```

Em `kernel/src/mem/mod.rs`, adicionar `pub mod space;`.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest` → `kernel_mapped_in_new_address_space` e `address_space_drop_frees_user_frames` passam. `cargo test -p arch-x86_64`/`-p mm` ok.

- [ ] **Step 6: Commit**

```bash
git add kernel
git commit -m "feat(kernel): mapeador por raiz de PML4 e AddressSpace por processo

Source: docs Intel SDM Vol. 3A, secao 4.5 (hierarquia de paginacao)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Processo de usuário e entrada em Ring 3 por thread do scheduler

**Files:**
- Create: `crates/arch-x86_64/src/context.rs` → adicionar o trampolim `tukos_user_entry` (asm)
- Modify: `kernel/src/sched.rs` (Thread com `cr3`/`kstack_top`, `spawn_user`, `switch_to_next` troca cr3+rsp0)
- Create: `kernel/src/process.rs`
- Modify: `kernel/src/main.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`context.rs`, `target_os = "none"`): símbolo `tukos_user_entry` — lido como `pub fn tukos_user_entry();` no bloco extern; é o alvo de `ret` do primeiro `switch_context` numa thread de usuário: faz `swapgs` e `iretq` consumindo o frame de Ring 3 que `spawn_user` montou logo acima na pilha de kernel.
- Produces (`sched.rs`): `Thread` ganha `cr3: u64` (0 = espaço do kernel) e `kstack_top: u64`; `pub fn spawn_user(space_root: u64, entry: u64, user_stack_top: u64, kstack: Box<[u8]>) -> usize`; `switch_to_next` passa a, para o alvo: se `cr3 != 0` e difere do atual, `write_cr3(cr3)`, e sempre `set_rsp0(kstack_top)` para threads com `kstack_top != 0`.
- Produces (`process.rs`): `pub struct Process { space: AddressSpace }`; `pub fn spawn_payload(payload: &[u8], entry_off: u64) -> usize` — cria um `AddressSpace`, mapeia código (W^X: copia e remapeia RO+X) e pilha (RW+NX), cria a thread de usuário no scheduler, e **vaza** o `AddressSpace`/`Process` por enquanto (sem exit de processo neste plano; registrar).

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("user_process_runs_in_ring3", user_process_runs_in_ring3),` e:
```rust
fn user_process_runs_in_ring3() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // Payload: SYS_WRITE_MARK(0xBEEF) then loop yielding via SYS_WRITE_MARK again.
    crate::process::spawn_demo_user();
    // The boot thread yields; the user process must run in Ring 3 and reach the kernel.
    for _ in 0..2000 {
        if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 0xBEEF {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("o processo de usuario nao executou em Ring 3")
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Trampolim de entrada de usuário**

Em `crates/arch-x86_64/src/context.rs`, acrescentar ao `global_asm!` existente:
```asm
.global tukos_user_entry
tukos_user_entry:
    // Reached via switch_context's `ret` on a user thread's first run. The
    // iretq frame (RIP,CS,RFLAGS,RSP,SS) sits just above on the kernel stack.
    swapgs
    iretq
```
E no bloco `unsafe extern "C"`:
```rust
    pub fn tukos_user_entry();
```

- [ ] **Step 4: Thread de usuário no scheduler**

Em `kernel/src/sched.rs`:
- acrescentar aos campos de `Thread`: `cr3: u64` e `kstack_top: u64` (as threads existentes usam `cr3: 0`, `kstack_top: 0`);
- `spawn_user(space_root, entry, user_stack_top, kstack)` monta a pilha de kernel `kstack` assim (do topo para baixo): o frame de `iretq` (`user_ss=USER_DS`, `user_rsp=user_stack_top`, `rflags=0x202` com IF ligado, `user_cs=USER_CS`, `user_rip=entry`), depois o endereço de retorno `tukos_user_entry`, depois 6 palavras zero (os callee-saved que `switch_context` desempilha). O `rsp` salvo da thread aponta para a palavra mais baixa (o primeiro callee-saved). Guarde `cr3 = space_root`, `kstack_top = topo de kstack`;
- em `switch_to_next`, ao computar `(save, load)`, capture também `target_cr3` e `target_kstack_top`; **após** soltar o guard do `SCHED` e **antes** de `switch_context`, faça:
  ```rust
  unsafe {
      if target_kstack_top != 0 {
          arch_x86_64::gdt::set_rsp0(target_kstack_top);
      }
      let cur = crate::arch_cr3_base();
      if target_cr3 != 0 && target_cr3 != cur {
          arch_x86_64::cpu::write_cr3(target_cr3);
      }
  }
  ```
  (A ordem importa: trocar `cr3` por último, imediatamente antes de `switch_context`; a pilha de kernel de destino está mapeada na metade alta de todo espaço, então o `switch_context` seguinte roda com a pilha válida.)

Detalhe do cálculo de `spawn_user` (análogo ao `spawn` atual, mas com o frame extra de `iretq`): seja `top = (base+len) & !0xF`. Empilhe (endereços decrescentes) `USER_DS, user_stack_top, 0x202, USER_CS, entry` (5 palavras, o frame de iretq, com `entry` no endereço mais baixo dos cinco), depois `tukos_user_entry` (o alvo do `ret`), depois 6 zeros. O `rsp` salvo aponta para o primeiro dos 6 zeros. Verifique no QEMU com `-d int` se não chegar ao Ring 3.

- [ ] **Step 5: Processo e payload de demonstração**

`kernel/src/process.rs`:
```rust
//! A user process: an address space plus (for now) a single user thread.
//! No process exit yet, so the address space is intentionally leaked.

extern crate alloc;

use crate::mem::space::AddressSpace;
use alloc::boxed::Box;
use mm::paging::PageFlags;

const USER_CODE_VA: u64 = 0x0000_0000_0040_0000;
const USER_STACK_VA: u64 = 0x0000_0000_0050_0000;
const USER_STACK_TOP: u64 = 0x0000_0000_0051_0000;
const KSTACK_SIZE: usize = 16 * 1024;

core::arch::global_asm!(
    r#"
.section .rodata
.global tukos_demo_user
.global tukos_demo_user_end
tukos_demo_user:
    mov rax, 1          // SYS_WRITE_MARK
    mov rdi, 0xBEEF
    syscall
1:
    mov rax, 1          // keep marking; the timer preempts this loop
    mov rdi, 0xBEEF
    syscall
    jmp 1b
tukos_demo_user_end:
"#
);

unsafe extern "C" {
    static tukos_demo_user: u8;
    static tukos_demo_user_end: u8;
}

fn demo_payload() -> &'static [u8] {
    let start = (&raw const tukos_demo_user) as usize;
    let end = (&raw const tukos_demo_user_end) as usize;
    unsafe { core::slice::from_raw_parts(start as *const u8, end - start) }
}

/// Creates a user process running the demo payload and schedules its thread.
pub fn spawn_demo_user() -> usize {
    let space = AddressSpace::new_user();
    let code_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ codigo");
    let stack_phys = crate::mem::frames::alloc_frame().expect("sem frame p/ pilha");

    unsafe {
        // Map code writable to copy the payload in, through the HHDM of its frame,
        // then map it read-only + executable (W^X) in the user space.
        core::ptr::copy_nonoverlapping(
            demo_payload().as_ptr(),
            crate::mem::frames::phys_to_virt(code_phys) as *mut u8,
            demo_payload().len(),
        );
        space
            .map(USER_CODE_VA, code_phys, PageFlags::USER) // present+user, not writable, executable
            .expect("map codigo");
        space
            .map(
                USER_STACK_VA,
                stack_phys,
                PageFlags::USER.union(PageFlags::WRITABLE).union(PageFlags::NO_EXECUTE),
            )
            .expect("map pilha");
    }

    let kstack = alloc::vec![0u8; KSTACK_SIZE].into_boxed_slice();
    let root = space.root();
    // Leak the address space: no process teardown in this plan.
    core::mem::forget(space);
    crate::sched::spawn_user(root, USER_CODE_VA, USER_STACK_TOP, kstack)
}
```

Observação sobre o W^X: `space.map(USER_CODE_VA, code_phys, PageFlags::USER)` mapeia a página de código **somente leitura e executável** (sem `WRITABLE`, sem `NO_EXECUTE`). A cópia do payload é feita antes, pela janela HHDM do frame físico (que é de kernel, gravável), não pelo endereço de usuário — então o código nunca é gravável e executável ao mesmo tempo em Ring 3.

Em `kernel/src/main.rs`: `mod process;`.

- [ ] **Step 6: Rodar e ver passar**

Run: `cargo xtask ktest` → `user_process_runs_in_ring3` passa (`LAST_MARK == 0xBEEF`). As demais variantes ok.

Depuração: se triple-faultar, rode o QEMU com `-d int,cpu_reset` e confira: a PML4 nova tem a metade alta clonada (senão o `write_cr3` mata o kernel); a pilha de kernel da thread está montada com o frame de `iretq` + `tukos_user_entry` + 6 zeros na ordem certa; `set_rsp0` aponta para `kstack_top`; a página de código é USER e executável; `rflags=0x202` (IF ligado).

- [ ] **Step 7: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(kernel): processo de usuario com espaco proprio e entrada em Ring 3 pelo scheduler

Source: docs Intel SDM Vol. 3A cap. 6 (IRET entre niveis); 4.5 (CR3)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Preempção de dois processos de usuário

**Files:**
- Modify: `kernel/src/process.rs` (um segundo payload, ou parametrizar a marca), `kernel/src/ktest.rs`

**Interfaces:**
- Produces: `process::spawn_demo_user_marking(mark: u64) -> usize` — o payload escreve `mark` em vez de `0xBEEF` fixo, para dois processos serem distinguíveis. (Como o payload é position-independent com imediatos, gere duas variantes ou passe a marca pela pilha de usuário; a forma mais simples é ter dois payloads em asm, `tukos_demo_user_a`/`_b`, com marcas diferentes.)

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("two_user_processes_preempt", two_user_processes_preempt),` e:
```rust
static USER_A_RAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static USER_B_RAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn two_user_processes_preempt() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    USER_A_RAN.store(0, Ordering::SeqCst);
    USER_B_RAN.store(0, Ordering::SeqCst);
    // Two user processes that never yield; only the timer makes both run.
    crate::process::spawn_demo_user_pair();
    for _ in 0..4000 {
        if USER_A_RAN.load(Ordering::SeqCst) > 0 && USER_B_RAN.load(Ordering::SeqCst) > 0 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("o timer nao preemptou os dois processos de usuario")
}
```
O dispatch de `SYS_WRITE_MARK` precisa, nesta tarefa, incrementar `USER_A_RAN`/`USER_B_RAN` conforme a marca (`0xA` → A, `0xB` → B). Ajuste `kernel/src/syscall.rs::dispatch` para, em `SYS_WRITE_MARK`, além de gravar `LAST_MARK`, encaminhar a marca a um gancho de teste: adicione `#[cfg(feature = "ktest")] crate::ktest::on_user_mark(a0);` e defina `pub fn on_user_mark(mark: u64)` em `ktest.rs` que incrementa o contador conforme `mark`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação (`spawn_demo_user_pair`/`on_user_mark` não existem).

- [ ] **Step 3: Implementar**

Em `kernel/src/process.rs`, adicione dois payloads em `global_asm!` (`tukos_demo_user_a` com `mov rdi, 0xA`, `tukos_demo_user_b` com `mov rdi, 0xB`, cada um em loop `SYS_WRITE_MARK`), uma função interna `spawn_payload(bytes: &[u8]) -> usize` que faz o que `spawn_demo_user` fazia (cria `AddressSpace`, mapeia código RO+X e pilha RW+NX, cria a thread), e:
```rust
pub fn spawn_demo_user_pair() {
    spawn_payload(demo_payload_a());
    spawn_payload(demo_payload_b());
}
```
Cada processo tem seu próprio `AddressSpace`, mas ambos usam os **mesmos** VAs de usuário (`0x40_0000`/`0x50_0000`) — o que é correto, porque cada um tem sua própria PML4 e os VAs só colidiriam se compartilhassem o espaço. Isto exercita o isolamento: o `cr3` troca a cada preempção entre A e B.

Em `kernel/src/ktest.rs`:
```rust
pub fn on_user_mark(mark: u64) {
    use core::sync::atomic::Ordering;
    match mark {
        0xA => { USER_A_RAN.fetch_add(1, Ordering::SeqCst); }
        0xB => { USER_B_RAN.fetch_add(1, Ordering::SeqCst); }
        _ => {}
    }
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `two_user_processes_preempt` passa (A e B rodaram, cada um no seu espaço, preemptados pelo timer). Demais variantes ok.

Nota: como no Plano 2b, os processos de A e B nunca terminam e ficam no round-robin; a thread de boot volta a cada volta e conclui os testes. Este é o último teste da suíte (nada deve ser acrescentado depois dele).

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): preempcao de dois processos de usuario em espacos isolados

Source: docs Intel SDM Vol. 3A, 4.5 (CR3 por processo)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os cinco gates de Global Constraints passam. Marco: dois processos de usuário, cada um em seu próprio espaço de endereçamento, rodam em Ring 3 e são preemptados pelo timer.
- Limitações (planos seguintes): sem exit/teardown de processo (o `AddressSpace` é vazado de propósito — `AddressSpace::drop` existe e é testado, mas `spawn` ainda não o chama); payloads montados pelo kernel, não carregados de `.exe`; sem gerenciador de objetos, handles, `ntdll` ou loader de PE; `swapgs` ainda não é reentrante a NMI; base de GS de usuário ainda igual ao bloco por-CPU (sem TEB).
- Próximo plano (4): gerenciador de objetos e tabela de handles; depois `ntdll` + loader de PE para, enfim, rodar um `hello.exe` real (degraus 3a/3b/3c da spec), reaproveitando `AddressSpace`, `spawn_user` e o dispatch de syscalls deste plano.
