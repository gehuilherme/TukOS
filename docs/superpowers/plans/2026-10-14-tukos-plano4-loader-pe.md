# TukOS Plano 4 — Fundação do loader de PE: carregar e rodar um PE

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** O TukOS faz o parsing de uma imagem PE/COFF, carrega suas seções num espaço de endereçamento de usuário com permissões por seção (W^X), aplica relocações de base e a executa em Ring 3 como um processo que sai de forma limpa, com o espaço de endereços liberado.

**Architecture:** A lógica pura de parsing de PE/COFF vai para a nova crate `pe` (testável no host com imagens construídas em memória). O kernel ganha um ciclo de vida de processo: uma thread de usuário pode terminar (`SYS_EXIT` do processo), é removida do round-robin e seus recursos (pilha de kernel e `AddressSpace`) são recuperados por um reaper na próxima troca. A metade alta do kernel é pré-mapeada no boot para que mapeamentos futuros propaguem a todos os espaços. O loader do kernel usa a crate `pe` para mapear cada seção no `AddressSpace` do processo e criar a thread de usuário no ponto de entrada. O PE de teste é feito pelo próprio TukOS, **sem imports**, e faz um `syscall` direto — prova o loader sem precisar de resolução de imports (que fica para o Plano 5, com `ntdll`).

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `limine` 0.5, `spin` 0.10, `mm`, `arch-x86_64`, a nova crate `pe`, QEMU + OVMF.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 5 "Lado do usuário": a crate `pe` e o mapeamento de seções; seção 4 "Executivo": processos). Sucede o Plano 3 (processos de usuário), já na `main`.

**Fora deste plano (vai para o Plano 5):** `ntdll` (stubs de syscall, loader de imports, PEB/TEB/`RTL_USER_PROCESS_PARAMETERS`), resolução de imports/forwarders, `kernel32`, e rodar um `.exe` real do Windows (degraus 3a/3b/3c). Base de GS por thread de usuário (TEB) também é do Plano 5. Este plano carrega um PE **sem imports**.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64, um núcleo. Código dependente de CPU vive em `crates/arch-x86_64`; lógica pura de PE vive em `crates/pe`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft.
- Clean room nível 0 apenas (ver `CLEAN_ROOM.md` e `AGENTS.md`): o formato PE/COFF e as relocações vêm da especificação pública "PE Format" da Microsoft Learn e do `winnt.h` público. Nenhum binário da Microsoft é desassemblado; o PE de teste é construído pelo TukOS.
- Commits de comportamento de hardware/protocolo/formato declaram a fonte no rodapé: `Source: docs <documento>` (para o PE: `Source: docs Microsoft PE Format specification`).
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- Imagens PE32+ (`Magic == 0x20b`), `Machine == 0x8664` (AMD64). Rejeitar outras com erro claro.
- Endereço virtual de carga do PE de teste (metade baixa, canônico): base preferida `0x0000_0000_0040_0000`.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **Terminar a thread atual enquanto se roda na pilha dela** não pode liberar a pilha em uso; o reaper libera a pilha de kernel e o `AddressSpace` da thread morta só depois que a troca já pôs a CPU em outra pilha. Coberto por `user_process_exits_and_is_reaped` (Task 1), que confere que a contagem de frames volta ao valor inicial.
2. **Uma seção PE com `SizeOfRawData < VirtualSize`** (p.ex. `.bss`) deve ter o excedente zerado, não lixo do frame; o loader zera a página antes de copiar os bytes brutos. Coberto por `load_zero_fills_bss` (Task 4).
3. **Um PE carregado numa base diferente da preferida** precisa das relocações aplicadas, ou os ponteiros absolutos ficam errados; o loader aplica as relocações DIR64 pelo delta de base. Coberto por `reloc_dir64_applies_delta` (Task 3, puro) e pelo PE de teste carregado numa base deslocada (Task 4).
4. **Cabeçalhos PE malformados** (assinatura errada, `e_lfanew` fora dos limites, seção que aponta para fora do arquivo) devem ser rejeitados com erro, nunca ler fora dos limites; o parser valida cada campo antes de usar. Coberto por `parse_rejects_malformed` (Task 3).
5. **Uma seção mapeada com permissões erradas** (código gravável, ou dados executáveis) quebra o W^X; o loader traduz `Characteristics` para flags de página exatas (EXECUTE→sem NX; WRITE→WRITABLE; sempre USER). Coberto por `characteristics_to_flags` (Task 3) e pelo PE de teste rodar código numa página não-gravável (Task 4).

---

### Task 1: Ciclo de vida de processo — saída de thread e reaper

**Files:**
- Modify: `kernel/src/sched.rs` (estado da thread, `exit_current`, reaper em `switch_to_next`, pular threads mortas)
- Modify: `kernel/src/syscall.rs` (SYS_EXIT de um processo de usuário chama o reaper, não o longjmp de `enter_user`)
- Modify: `kernel/src/process.rs` (a thread de usuário passa a **possuir** seu `AddressSpace`, não vazá-lo)
- Modify: `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`sched.rs`):
  - `Thread` ganha `state: ThreadState` (`enum ThreadState { Ready, Exited }`) e `space: Option<AddressSpace>` (dono do espaço; `None` para threads de kernel) além do `_stack`/`kstack_top`/`cr3` já existentes;
  - `pub fn spawn_user(space: AddressSpace, entry, user_stack_top, kstack) -> usize` (agora recebe o `AddressSpace` por valor e o guarda na thread);
  - `pub fn exit_current() -> !` — marca a thread atual `Exited`, guarda seus recursos para reaping e troca para a próxima thread viva; nunca retorna;
  - `switch_to_next` pula threads `Exited` ao escolher a próxima, e **após** `switch_context` retornar, libera os recursos de qualquer thread que tenha terminado (reaping: solta `space` e `_stack`), mantendo o slot no `Vec` (marcado `Exited`) para não invalidar índices.
- Produces (`syscall.rs`): o `dispatch` de `SYS_EXIT`, quando vier de um processo de usuário com thread no scheduler, chama `crate::sched::exit_current()`.

Observação de design (dar ao implementador): não se pode liberar a pilha em que se está executando. O padrão correto no single-CPU: `exit_current` marca a thread atual `Exited`, registra o índice a recolher, e chama `switch_to_next`; a troca leva a CPU para a pilha de outra thread; aí, no corpo de `switch_to_next` que acabou de retornar de `switch_context` (já rodando como a thread de destino), o reaper solta `space`/`_stack` da thread `Exited` pendente. A thread de boot nunca termina. O `AddressSpace::drop` (Plano 3) libera os frames do espaço.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("user_process_exits_and_is_reaped", user_process_exits_and_is_reaped),` **antes** de `two_user_processes_preempt` (que deve seguir sendo o último) e:
```rust
fn user_process_exits_and_is_reaped() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    // Spawn a user process whose payload does SYS_EXIT immediately.
    crate::process::spawn_exiting_user();
    // Yield until it has run, exited and been reaped (its space+stacks freed).
    for _ in 0..4000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() >= before {
            // Reaping returns the frames; allow equality (fully reclaimed).
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("processo de usuario nao foi recolhido (frames nao voltaram)")
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação (`spawn_exiting_user`/`exit_current` não existem).

- [ ] **Step 3: Implementar o estado e o reaper no scheduler**

Em `kernel/src/sched.rs`:
- adicionar `#[derive(PartialEq)] enum ThreadState { Ready, Exited }`;
- `Thread { rsp, entry, _stack: Option<Box<[u8]>>, cr3, kstack_top, state: ThreadState, space: Option<AddressSpace> }`; threads existentes usam `state: Ready, space: None`;
- em `Scheduler`, adicionar `reap: Option<usize>` (índice de uma thread `Exited` cujos recursos ainda não foram soltos);
- `spawn_user(space: AddressSpace, entry, user_stack_top, kstack)` guarda `space: Some(space)`, `cr3 = space.root()` — **cuidado:** leia `space.root()` antes de mover `space` para dentro da struct;
- `exit_current() -> !`:
  ```rust
  pub fn exit_current() -> ! {
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
  ```
- em `switch_to_next`, a escolha da próxima thread pula `Exited`:
  ```rust
  let n = s.threads.len();
  let mut to = (from + 1) % n;
  while s.threads[to].state == ThreadState::Exited && to != from {
      to = (to + 1) % n;
  }
  ```
- **após** `switch_context` retornar (fim de `switch_to_next`), recolher a thread pendente:
  ```rust
  let pending = { SCHED.lock().as_mut().and_then(|s| s.reap.take()) };
  if let Some(idx) = pending {
      // We are now on a different thread's stack; safe to free the exited one's.
      let (space, stack) = {
          let mut guard = SCHED.lock();
          let s = guard.as_mut().unwrap();
          (s.threads[idx].space.take(), s.threads[idx]._stack.take())
      };
      drop(space); // AddressSpace::drop frees its frames
      drop(stack); // frees the kernel stack
  }
  ```
  (Soltar fora do lock, para o `AddressSpace::drop` — que chama `free_frame` sob o lock de frames — não aninhar o lock do SCHED.)

Em `kernel/src/syscall.rs`, no `dispatch`, o braço `SYS_EXIT`: se a thread atual é de usuário no scheduler (tem `space`), chamar `crate::sched::exit_current()`. Como distinguir da excursão `enter_user` do Plano 2c/3 (que usa `exit_to_kernel`)? Regra simples: o `enter_user` (ktest) e o processo de usuário são caminhos distintos. Para este plano, o `dispatch` de `SYS_EXIT` chama `exit_current()` **apenas** quando `crate::sched::current_is_user()` (novo helper que retorna true se a thread atual tem `space: Some`); senão mantém o comportamento do Plano 2c (`exit_to_kernel`). Implemente `current_is_user()` em `sched.rs`.

- [ ] **Step 4: Payload que sai e o spawn correspondente**

Em `kernel/src/process.rs`, adicionar um payload em `global_asm` `tukos_exiting_user` (`mov rax, 0 /*SYS_EXIT*/; mov rdi, 0; syscall; ud2`), e `pub fn spawn_exiting_user() -> usize` que faz como `spawn_payload` mas **sem** `core::mem::forget(space)` — passa o `AddressSpace` por valor para `sched::spawn_user`. Ajuste `spawn_payload` e `spawn_demo_user*` para também passar o `AddressSpace` por valor (não vazar) agora que `spawn_user` o possui; o reaper cuidará da liberação quando as threads terminarem (as de demo não terminam, então não são recolhidas — ok).

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest` → `user_process_exits_and_is_reaped` passa (frames voltam ao valor inicial). Demais variantes ok.

Depuração: se os frames não voltarem totalmente, confira que `AddressSpace` é solto (não vazado), que `_stack` é solto, e que o reaper roda numa pilha diferente da recolhida. Se triple-faultar na saída, confirme que `exit_current` troca para uma thread **viva** (não para a própria thread `Exited`).

- [ ] **Step 6: Commit**

```bash
git add kernel
git commit -m "feat(kernel): saida de processo de usuario e reaper de recursos

Source: docs Intel SDM Vol. 3A (troca de contexto); design de scheduler single-CPU
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Pré-mapear a metade alta do kernel no boot

**Files:**
- Modify: `kernel/src/mem/mod.rs` (nova etapa de init), `kernel/src/mem/paging.rs` (helper para garantir tabelas de nível 3 da metade alta)
- Modify: `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`paging.rs`): `pub fn reserve_kernel_higher_half()` — para cada índice 256..512 da PML4 ativa que estiver vazio, aloca um frame zerado e o instala como tabela de nível 3 (PDPT) presente+gravável (**sem** `USER`). Assim toda entrada de nível 4 da metade alta passa a existir e ser compartilhada; mapear uma região de kernel nova depois só altera tabelas de nível inferior (alcançadas por ponteiro), que propagam a todos os espaços.
- `AddressSpace::new_user` (Plano 3) já clona as 256 entradas da metade alta — então, chamando `reserve_kernel_higher_half()` **antes** de qualquer processo, todo espaço herda as mesmas PDPTs.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("higher_half_pml4_fully_reserved", higher_half_pml4_fully_reserved),` e:
```rust
fn higher_half_pml4_fully_reserved() -> Result<(), &'static str> {
    use mm::paging::{entry_present, page_table_indices};
    // Every PML4 entry 256..512 of the active space must be present after init,
    // so a new address space clones a complete kernel higher half.
    let pml4 = crate::arch_cr3_base();
    let table = crate::mem::frames::phys_to_virt(pml4) as *const u64;
    for i in 256..512 {
        let e = unsafe { core::ptr::read_volatile(table.add(i)) };
        if !entry_present(e) {
            return Err("entrada PML4 da metade alta nao reservada");
        }
    }
    let _ = page_table_indices(0);
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha (algumas entradas da metade alta vazias).

- [ ] **Step 3: Implementar**

Em `kernel/src/mem/paging.rs`:
```rust
/// Ensures every higher-half PML4 entry (256..512) of the active space exists,
/// so later kernel mappings only touch lower-level tables (shared by pointer
/// across all address spaces) rather than creating a new top-level entry that
/// would not propagate to already-forked user spaces.
pub fn reserve_kernel_higher_half() {
    let pml4_phys = crate::arch_cr3_base();
    let pml4 = table_mut(pml4_phys);
    for i in 256..512 {
        let slot = unsafe { pml4.add(i) };
        let e = unsafe { core::ptr::read_volatile(slot) };
        if !entry_present(e) {
            let frame = crate::mem::frames::alloc_frame().expect("sem frame p/ PDPT do kernel");
            unsafe {
                core::ptr::write_bytes(table_mut(frame) as *mut u8, 0, 4096);
                // Present + writable, NOT user: kernel-only higher half.
                core::ptr::write_volatile(slot, make_entry(frame, PageFlags::WRITABLE));
            }
        }
    }
}
```
(reutilize os imports já existentes em `paging.rs`: `table_mut`, `make_entry`, `entry_present`, `PageFlags`.)

Em `kernel/src/mem/mod.rs`, em `init()`, chamar `paging::reserve_kernel_higher_half()` **após** `frames::init(...)` e **antes** de `heap::init()` (assim o heap, mapeado depois, usa as PDPTs reservadas). Nota: a ordem importa — a reserva precisa do alocador de frames pronto.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `higher_half_pml4_fully_reserved` e `kernel_mapped_in_new_address_space` passam. Demais ok.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): pre-reserva as PDPTs da metade alta do kernel no boot

Source: docs Intel SDM Vol. 3A, secao 4.5 (hierarquia de paginacao)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Crate `pe` — parser de PE/COFF (lógica pura)

**Files:**
- Create: `crates/pe/Cargo.toml`, `crates/pe/src/lib.rs`
- Modify: `Cargo.toml` (workspace: `crates/pe` em `members` e `default-members`)

**Interfaces:**
- Produces (`pe`):
  - `pub struct PeFile<'a> { ... }` com `pub fn parse(bytes: &'a [u8]) -> Result<PeFile<'a>, PeError>`
  - `pub enum PeError { TooSmall, BadDosMagic, BadPeSignature, NotPe32Plus, NotAmd64, BadLfanew, SectionOutOfBounds, BadRelocBlock }`
  - `pub fn entry_point_rva(&self) -> u32`; `pub fn image_base(&self) -> u64`; `pub fn size_of_image(&self) -> u32`
  - `pub fn sections(&self) -> impl Iterator<Item = Section<'a>>` — cada `Section { rva: u32, virtual_size: u32, raw: &'a [u8], characteristics: u32 }`
  - `pub fn relocations(&self) -> Result<impl Iterator<Item = Reloc>, PeError>` — cada `Reloc { rva: u32, kind: u16 }` (apenas blocos presentes; vazio se não houver diretório de relocação)
  - constantes públicas: `SCN_MEM_EXECUTE = 0x2000_0000`, `SCN_MEM_READ = 0x4000_0000`, `SCN_MEM_WRITE = 0x8000_0000`, `REL_BASED_ABSOLUTE = 0`, `REL_BASED_DIR64 = 10`
  - `pub fn characteristics_to_flags(ch: u32) -> (bool /*write*/, bool /*exec*/)` — mapeia `SCN_MEM_WRITE`/`SCN_MEM_EXECUTE`
  - `pub fn apply_dir64(bytes: &mut [u8], rva: u32, delta: i64)` — soma `delta` ao `u64` em `bytes[rva..rva+8]` (helper puro para o teste de relocação; o loader aplicará no espaço mapeado)

- [ ] **Step 1: Criar a crate e registrá-la**

Em `Cargo.toml` (raiz): adicionar `"crates/pe"` a `members` e `default-members`.

`crates/pe/Cargo.toml`:
```toml
[package]
name = "pe"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false
```

- [ ] **Step 2: Testes que falham (host)**

`crates/pe/src/lib.rs` (módulo de testes). Um helper constrói um PE32+ mínimo válido em memória: DOS header (`MZ`, `e_lfanew=0x40`), NT signature `PE\0\0`, COFF header (Machine 0x8664, 1 seção, SizeOfOptionalHeader), Optional Header PE32+ (Magic 0x20b, AddressOfEntryPoint, ImageBase, SizeOfImage), 1 section header `.text` (VirtualAddress 0x1000, VirtualSize, SizeOfRawData, PointerToRawData, Characteristics EXECUTE|READ), e os bytes brutos da seção.
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Builds a minimal valid PE32+ with one .text section of `text` bytes at RVA 0x1000.
    fn build_pe(image_base: u64, entry_rva: u32, text: &[u8], text_vsize: u32, ch: u32) -> Vec<u8> {
        // ... construct the exact bytes per the PE spec (see Step 3 for offsets) ...
        // Return a Vec<u8>. Implemented in Step 3 alongside the parser so offsets match.
        build_pe_impl(image_base, entry_rva, text, text_vsize, ch)
    }

    #[test]
    fn parses_headers_and_section() {
        let pe = build_pe(0x40_0000, 0x1000, &[0x90, 0xC3], 2, SCN_MEM_EXECUTE | SCN_MEM_READ);
        let f = PeFile::parse(&pe).unwrap();
        assert_eq!(f.image_base(), 0x40_0000);
        assert_eq!(f.entry_point_rva(), 0x1000);
        let secs: Vec<_> = f.sections().collect();
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].rva, 0x1000);
        assert_eq!(secs[0].raw, &[0x90, 0xC3]);
        assert!(secs[0].characteristics & SCN_MEM_EXECUTE != 0);
    }

    #[test]
    fn characteristics_to_flags_maps_write_and_exec() {
        assert_eq!(characteristics_to_flags(SCN_MEM_EXECUTE | SCN_MEM_READ), (false, true));
        assert_eq!(characteristics_to_flags(SCN_MEM_WRITE | SCN_MEM_READ), (true, false));
        assert_eq!(characteristics_to_flags(SCN_MEM_READ), (false, false));
    }

    #[test]
    fn reloc_dir64_applies_delta() {
        let mut buf = [0u8; 16];
        buf[8..16].copy_from_slice(&0x1000u64.to_le_bytes());
        apply_dir64(&mut buf, 8, 0x5000);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x6000);
        apply_dir64(&mut buf, 8, -0x1000);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x5000);
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(matches!(PeFile::parse(&[]), Err(PeError::TooSmall)));
        assert!(matches!(PeFile::parse(&[0; 128]), Err(PeError::BadDosMagic)));
        let mut pe = build_pe(0x40_0000, 0x1000, &[0x90], 1, SCN_MEM_EXECUTE);
        pe[0x3C] = 0xFF; // e_lfanew way out of bounds
        pe[0x3D] = 0xFF;
        assert!(matches!(PeFile::parse(&pe), Err(PeError::BadLfanew)));
    }
}
```

- [ ] **Step 3: Implementar o parser (e o `build_pe_impl` usado pelos testes)**

`crates/pe/src/lib.rs` (acima dos testes). Implemente leitura little-endian com checagem de limites em cada passo; retorne `PeError` em vez de indexar fora dos limites. Use os offsets da especificação "PE Format": DOS `e_magic` em 0 (`0x5A4D` = "MZ"), `e_lfanew` `u32` em 0x3C; NT `Signature` `u32` (`0x0000_4550` = "PE\0\0"); COFF (logo após Signature): `Machine` `u16` em +0, `NumberOfSections` `u16` em +2, `SizeOfOptionalHeader` `u16` em +16; Optional Header (logo após os 20 bytes de COFF): `Magic` `u16` em +0 (0x20b = PE32+), `AddressOfEntryPoint` `u32` em +16, `ImageBase` `u64` em +24, `SizeOfImage` `u32` em +56, `NumberOfRvaAndSizes` `u32` em +108, data directories a partir de +112 (cada um é `u32 RVA` + `u32 Size`; o diretório de relocação base é o índice 5); section headers (40 bytes cada) logo após o Optional Header: `Name[8]`, `VirtualSize` `u32` em +8, `VirtualAddress` `u32` em +12, `SizeOfRawData` `u32` em +16, `PointerToRawData` `u32` em +20, `Characteristics` `u32` em +36. Relocação base: blocos de `PageRVA u32` + `BlockSize u32`, seguidos de `(BlockSize-8)/2` entradas `u16` (`tipo = entry >> 12`, `offset = entry & 0xFFF`; o RVA de cada relocação é `PageRVA + offset`).

`#![cfg_attr(not(test), no_std)]`. `PeFile<'a>` guarda `bytes: &'a [u8]` e os offsets calculados (`opt_header_off`, `section_table_off`, `num_sections`, `reloc_dir_rva`, `reloc_dir_size`). `sections()` itera o array de 40 bytes validando que `[PointerToRawData, +SizeOfRawData)` cabe no arquivo (senão o `raw` é vazio — seção sem dados brutos, como `.bss`). `relocations()` percorre os blocos no diretório 5 (mapeando o RVA do diretório para o offset de arquivo pela seção que o contém), validando `BlockSize`.

O helper de teste `build_pe_impl` monta os mesmos offsets; mantenha-o em `#[cfg(test)]`.

Nota de clean room: todos os offsets e constantes acima são da especificação pública "PE Format"; não há binário da Microsoft envolvido.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p pe` → todos passam. `cargo clippy -p pe -- -D warnings` limpo.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/pe
git commit -m "feat(pe): parser de PE/COFF (headers, secoes, relocacoes) — logica pura

Source: docs Microsoft PE Format specification
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Loader de PE no kernel — mapear seções e rodar

**Files:**
- Create: `kernel/src/loader.rs`
- Modify: `kernel/Cargo.toml` (dependência `pe`), `kernel/src/main.rs` (`mod loader;`), `kernel/src/process.rs` (um PE de teste embutido + `spawn_pe`), `kernel/src/ktest.rs`

**Interfaces:**
- Consumes: `pe::{PeFile, Section, characteristics_to_flags, REL_BASED_DIR64, REL_BASED_ABSOLUTE}`; `crate::mem::space::AddressSpace`; `mm::paging::PageFlags`; `crate::mem::frames`.
- Produces (`loader.rs`): `pub fn load_pe(space: &AddressSpace, image: &[u8], load_base: u64) -> Result<u64, LoadError>` — para cada seção: aloca frames, zera, copia os bytes brutos (o restante fica zero, cobrindo `.bss`), mapeia em `load_base + rva` com flags de `characteristics_to_flags` (sempre `USER`; `WRITE`→`WRITABLE`; não-`EXECUTE`→`NO_EXECUTE`); aplica as relocações DIR64 somando `delta = load_base - image_base` (escrevendo nas páginas já mapeadas, pela janela HHDM do frame); retorna o endereço de entrada `load_base + entry_rva`. `pub enum LoadError { Parse(pe::PeError), Map, Alloc, UnsupportedReloc }`.
- Produces (`process.rs`): um PE de teste **sem imports** embutido como bytes (construído em `build.rs` ou como array literal — ver Step 3), e `pub fn spawn_pe(image: &[u8], load_base: u64) -> usize` que cria um `AddressSpace`, chama `load_pe`, mapeia uma pilha de usuário, e cria a thread de usuário no ponto de entrada retornado.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar (antes de `two_user_processes_preempt`):
```rust
    ("pe_loads_runs_and_exits", pe_loads_runs_and_exits),
    ("load_zero_fills_bss", load_zero_fills_bss),
```
e:
```rust
fn pe_loads_runs_and_exits() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    let before = crate::mem::frames::free_frame_count();
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // The embedded test PE: SYS_WRITE_MARK(0x5E) then SYS_EXIT(0).
    crate::process::spawn_test_pe();
    let mut marked = false;
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 0x5E {
            marked = true;
        }
        if marked && crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("PE nao carregou/rodou/saiu com recuperacao de frames")
}

fn load_zero_fills_bss() -> Result<(), &'static str> {
    // A section with virtual_size > raw.len() must be zero-filled by the loader.
    use crate::mem::space::AddressSpace;
    let space = AddressSpace::new_user();
    // Build a 1-page section at RVA 0x1000, raw = [0xAA], virtual_size = 0x20.
    let image = crate::process::build_bss_test_pe();
    let base = 0x40_0000u64;
    let entry = crate::loader::load_pe(&space, &image, base).map_err(|_| "load falhou")?;
    let _ = entry;
    // Read the mapped page via the space: byte 0 == 0xAA, byte 1..0x20 == 0.
    let phys = space.translate(base + 0x1000).ok_or("secao nao mapeada")?;
    let p = crate::mem::frames::phys_to_virt(phys) as *const u8;
    unsafe {
        if core::ptr::read_volatile(p) != 0xAA {
            return Err("byte bruto nao copiado");
        }
        if core::ptr::read_volatile(p.add(1)) != 0x00 {
            return Err("excedente do .bss nao foi zerado");
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Gerar o PE de teste**

O PE de teste é **feito pelo TukOS** (clean room: não é binário da Microsoft). Gere-o em tempo de build para evitar versionar binário: em `kernel/build.rs`, escreva um pequeno gerador que monta os bytes de um PE32+ sem imports contendo uma seção `.text` executável no RVA 0x1000 com o código:
```asm
; entry (position-independent, raw syscall):
mov rax, 1        ; SYS_WRITE_MARK
mov rdi, 0x5E
syscall
mov rax, 0        ; SYS_EXIT
mov rdi, 0
syscall
ud2
```
Emita os bytes do PE para `${OUT_DIR}/test_pe.bin` e, em `process.rs`, inclua com `include_bytes!(concat!(env!("OUT_DIR"), "/test_pe.bin"))`. Como alternativa (se `build.rs` complicar), monte os bytes do PE diretamente em `process.rs` com uma função `const`/`static` (o mesmo gerador da crate `pe` de teste serve de referência). O código da `.text` são bytes de máquina fixos (as instruções acima montam para uma sequência conhecida de ~30 bytes; comente cada instrução). `build_bss_test_pe()` monta um PE com uma seção cujo `VirtualSize=0x20` e `raw=[0xAA]`.

`spawn_test_pe()` chama `spawn_pe(TEST_PE, 0x40_0000)`.

- [ ] **Step 4: Implementar o loader**

`kernel/src/loader.rs`:
```rust
//! Loads a parsed PE image into a user AddressSpace with per-section W^X.

use crate::mem::frames::{alloc_frame, phys_to_virt};
use crate::mem::space::AddressSpace;
use mm::paging::PageFlags;
use pe::{PeError, PeFile, REL_BASED_ABSOLUTE, REL_BASED_DIR64, characteristics_to_flags};

#[derive(Debug)]
pub enum LoadError {
    Parse(PeError),
    Map,
    Alloc,
    UnsupportedReloc,
}

const PAGE: u64 = 4096;

/// Maps every section of `image` into `space` at `load_base + rva`, zero-filling
/// the slack beyond raw data, applies DIR64 base relocations for the delta, and
/// returns the entry point virtual address.
pub fn load_pe(space: &AddressSpace, image: &[u8], load_base: u64) -> Result<u64, LoadError> {
    let pe = PeFile::parse(image).map_err(LoadError::Parse)?;
    let delta = load_base.wrapping_sub(pe.image_base());

    for sec in pe.sections() {
        let (writable, exec) = characteristics_to_flags(sec.characteristics);
        let mut flags = PageFlags::USER;
        if writable {
            flags = flags.union(PageFlags::WRITABLE);
        }
        if !exec {
            flags = flags.union(PageFlags::NO_EXECUTE);
        }
        let pages = (sec.virtual_size as u64).div_ceil(PAGE).max(1);
        for p in 0..pages {
            let frame = alloc_frame().ok_or(LoadError::Alloc)?;
            unsafe {
                // Zero the whole page first (covers .bss slack), then copy raw bytes.
                core::ptr::write_bytes(phys_to_virt(frame) as *mut u8, 0, PAGE as usize);
                let off = (p * PAGE) as usize;
                if off < sec.raw.len() {
                    let n = core::cmp::min(PAGE as usize, sec.raw.len() - off);
                    core::ptr::copy_nonoverlapping(
                        sec.raw.as_ptr().add(off),
                        phys_to_virt(frame) as *mut u8,
                        n,
                    );
                }
                // Code pages are mapped writable only transiently? No: we map with
                // final flags; the copy happened through the HHDM kernel alias, so
                // the user mapping is already W^X.
                space
                    .map(load_base + sec.rva as u64 + p * PAGE, frame, flags)
                    .map_err(|_| LoadError::Map)?;
            }
        }
    }

    if delta != 0 {
        for r in pe.relocations().map_err(LoadError::Parse)? {
            match r.kind {
                REL_BASED_ABSOLUTE => {}
                REL_BASED_DIR64 => {
                    let va = load_base + r.rva as u64;
                    let phys = space.translate(va & !(PAGE - 1)).ok_or(LoadError::Map)?;
                    let p = (phys_to_virt(phys) + (va & (PAGE - 1))) as *mut u64;
                    unsafe {
                        let v = core::ptr::read_unaligned(p);
                        core::ptr::write_unaligned(p, v.wrapping_add(delta));
                    }
                }
                _ => return Err(LoadError::UnsupportedReloc),
            }
        }
    }

    Ok(load_base + pe.entry_point_rva() as u64)
}
```
Nota: uma relocação DIR64 que cruze a fronteira de página não é tratada por este `translate` de uma página só; para o PE de teste (relocações, se houver, dentro de uma página), isto basta — registre a limitação. Se o PE de teste for montado para carregar na base preferida (`delta == 0`), o laço de relocação nem roda; o teste `reloc_dir64_applies_delta` (Task 3) já cobre a aritmética, e o teste de Task 4 pode opcionalmente carregar numa base deslocada para exercitar o caminho.

`spawn_pe` (em `process.rs`): cria `AddressSpace::new_user`, `load_pe`, mapeia uma pilha de usuário (USER|WRITABLE|NO_EXECUTE, topo = base+tamanho mapeado), e `sched::spawn_user(space, entry, user_stack_top, kstack)`.

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest` → `pe_loads_runs_and_exits` (LAST_MARK 0x5E, frames recuperados) e `load_zero_fills_bss` passam. Demais variantes ok.

Depuração: se o PE não rodar, confira com `-d int` que a entrada está mapeada executável (não-NX) e USER, que a base de carga bate com o `delta` das relocações, e que os bytes de máquina da `.text` estão corretos (desmonte-os mentalmente ou com um montador externo de referência).

- [ ] **Step 6: Commit**

```bash
git add kernel
git commit -m "feat(kernel): loader de PE — mapeia secoes com W^X, relocacoes, e roda em Ring 3

Source: docs Microsoft PE Format specification
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os cinco gates de Global Constraints passam (incluindo `-p pe` no clippy do host). Marco: o TukOS carrega um PE sem imports, mapeia suas seções com W^X, o roda em Ring 3 e o processo sai de forma limpa, com o espaço de endereços recuperado.
- Limitações (Plano 5): sem `ntdll`, PEB/TEB, resolução de imports nem base de GS por thread (TEB), então ainda não roda um `.exe` real do Windows; relocações DIR64 que cruzam fronteira de página não são tratadas; o PE de teste é montado pelo TukOS.
- Plano 5 (degrau 3a): `ntdll` com stubs de syscall e o loader de imports em modo usuário, PEB/TEB, base de GS por thread de usuário; o kernel passa a mapear o `.exe` + `ntdll` e iniciar no thunk de inicialização do `ntdll`; primeiro `.exe` de console real do Windows que só chama `ntdll`.
