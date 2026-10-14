# TukOS Plano 5 — TEB/PEB por thread e syscalls NT de console

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cada thread de usuário do TukOS tem um TEB alcançável por `GS` (modelo NT) e um PEB; o kernel oferece as syscalls NT `NtWriteFile` (escreve um buffer de usuário validado no console) e `NtTerminateProcess` (encerra o processo). Um `.exe` de teste, feito pelo TukOS, lê seu próprio TEB, escreve pela `NtWriteFile` e termina pela `NtTerminateProcess`.

**Architecture:** Primeiro os consertos pendentes do Plano 4: a relocação DIR64 que cruza fronteira de página passa a escrever corretamente, e a pilha de usuário é posicionada acima da imagem carregada (não num endereço fixo que colidiria com as seções). Depois cada thread de usuário ganha um TEB e um PEB mapeados no seu espaço; a base de GS de usuário (via `IA32_KERNEL_GS_BASE`, trocada pelo `swapgs` na entrada do Ring 3) passa a apontar para o TEB da thread, por thread, em `switch_to_next`. Por fim, as syscalls NT de console: o kernel copia o buffer do usuário com validação de ponteiro (metade baixa, mapeado) antes de escrever, estabelecendo a fronteira "não confiar no ponteiro do usuário".

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `limine` 0.5, `spin` 0.10, `mm`, `arch-x86_64`, `pe`, QEMU + OVMF.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 5: PEB/TEB — `GS` aponta para o TEB; criação de processo no modelo NT). Sucede o Plano 4 (loader de PE), já na `main`.

**Fora deste plano (vai para o Plano 6):** o `ntdll.dll` real (crate Rust cross-compilada para `x86_64-pc-windows-msvc`), o loader de imports em modo usuário (`LdrInitializeThunk`), a resolução de imports/forwarders, o parsing das tabelas de import/export na crate `pe`, e rodar um `.exe` **real do Windows** (degrau 3a) com o harness de conformidade. Este plano usa um `.exe` **sem imports**, feito pelo TukOS, que chama a interface NT por `syscall` direto — prova as syscalls NT e o TEB sem precisar do `ntdll`.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64, um núcleo. Código dependente de CPU vive em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft.
- Clean room nível 0 apenas (ver `CLEAN_ROOM.md` e `AGENTS.md`): os offsets do TEB/PEB vêm do `winternl.h` público e da documentação pública do Microsoft Learn; os números de syscall são **do TukOS**, não do Windows; nenhum binário da Microsoft é desassemblado; o `.exe` de teste é feito pelo TukOS.
- Commits de comportamento de hardware/protocolo/ABI declaram a fonte: `Source: docs <documento>` (para TEB/PEB: `Source: docs Microsoft winternl.h public TEB/PEB layout`).
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- Offsets públicos do x64 usados: `NT_TIB.StackBase = 0x08`, `NT_TIB.StackLimit = 0x10`, `NT_TIB.Self = 0x30`, `TEB.ProcessEnvironmentBlock = 0x60`; `PEB.ImageBaseAddress = 0x10`.
- Números de syscall do TukOS: `0 = SYS_EXIT`, `1 = SYS_WRITE_MARK` (existentes, usados pelos demos/testes), `2 = NT_WRITE_FILE`, `3 = NT_TERMINATE_PROCESS` (novos).
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **`NtWriteFile` com um ponteiro de buffer que aponta para a metade alta (kernel) ou para memória não mapeada** não pode ler a memória do kernel nem faultar; o kernel valida que todo o intervalo `[buffer, buffer+len)` está na metade baixa e mapeado no espaço atual antes de copiar. Coberto por `ntwritefile_rejects_kernel_pointer` e `ntwritefile_rejects_unmapped` (Task 3).
2. **Uma relocação DIR64 cujo alvo cruza a fronteira de página** deve escrever os 8 bytes corretamente nas duas páginas (ou ser traduzida por página); hoje escreveria fora do frame HHDM. Coberto por `reloc_dir64_crosses_page` (Task 1).
3. **A pilha de usuário num endereço fixo que colide com uma seção da imagem** faria o `map` falhar com `AlreadyMapped`; a pilha é posicionada acima do fim da imagem. Coberto por `stack_placed_above_image` (Task 1).
4. **O TEB de uma thread lido por outra** (base de GS não trocada por thread) daria o TEB errado em Ring 3; `switch_to_next` grava `IA32_KERNEL_GS_BASE` com o TEB da thread de destino. Coberto por `user_reads_own_teb` (Task 2).
5. **`NtTerminateProcess` chamada por um processo** encerra e recupera os recursos; chamada fora de um processo (excursão) não deve corromper nada. Coberto por `nt_terminate_exits_and_reaps` (Task 3), reutilizando o reaper do Plano 4.

---

### Task 1: Consertos do Plano 4 — relocação DIR64 entre páginas e pilha sensível à imagem

**Files:**
- Modify: `kernel/src/loader.rs` (relocação por página; cálculo do topo da imagem)
- Modify: `kernel/src/process.rs` (pilha acima da imagem), `kernel/src/ktest.rs`

**Interfaces:**
- `load_pe` passa a: (a) aplicar cada relocação DIR64 traduzindo o `u64` por partes seguras à fronteira de página (lê/escreve respeitando que os 8 bytes podem estar em dois frames distintos); (b) expor `pub fn image_top(image: &[u8], load_base: u64) -> Result<u64, LoadError>` que retorna `load_base + align_up(maior (rva+virtual_size) entre as seções, PAGE)` — o primeiro endereço livre acima da imagem.
- `process.rs`: `spawn_pe` posiciona a pilha de usuário em `image_top(...)` (mais uma página de guarda opcional), não num VA fixo.

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`, adicionar:
```rust
    ("reloc_dir64_crosses_page", reloc_dir64_crosses_page),
    ("stack_placed_above_image", stack_placed_above_image),
```
e:
```rust
fn reloc_dir64_crosses_page() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    // Build a PE with a DIR64 relocation whose 8-byte target straddles a page
    // boundary (target RVA 0x1FFC, so bytes span page 0x1000 and 0x2000), loaded
    // at a base != image_base so delta != 0.
    let image = crate::process::build_reloc_cross_page_pe();
    let space = AddressSpace::new_user();
    let load_base = 0x80_0000u64; // != preferred image_base in the fixture
    crate::loader::load_pe(&space, &image, load_base).map_err(|_| "load falhou")?;
    // The relocated u64 must equal (original + delta), read across the two pages.
    let lo_phys = space.translate(load_base + 0x1000).ok_or("pagina 1 nao mapeada")?;
    let hi_phys = space.translate(load_base + 0x2000).ok_or("pagina 2 nao mapeada")?;
    let lo = crate::mem::frames::phys_to_virt(lo_phys) as *const u8;
    let hi = crate::mem::frames::phys_to_virt(hi_phys) as *const u8;
    // The value lives at offset 0xFFC in page 1 (4 bytes) + offset 0 in page 2 (4 bytes).
    let mut bytes = [0u8; 8];
    unsafe {
        for i in 0..4 {
            bytes[i] = core::ptr::read_volatile(lo.add(0xFFC + i));
        }
        for i in 0..4 {
            bytes[4 + i] = core::ptr::read_volatile(hi.add(i));
        }
    }
    let got = u64::from_le_bytes(bytes);
    let expected = crate::process::RELOC_FIXTURE_ORIGINAL.wrapping_add(load_base.wrapping_sub(crate::process::RELOC_FIXTURE_IMAGE_BASE));
    if got != expected {
        return Err("relocacao entre paginas incorreta");
    }
    Ok(())
}

fn stack_placed_above_image() -> Result<(), &'static str> {
    // image_top of the standard test PE must be above its last section end, and
    // the chosen user-stack VA must not collide with any mapped image page.
    let image = crate::process::test_pe_bytes();
    let base = 0x40_0000u64;
    let top = crate::loader::image_top(&image, base).map_err(|_| "image_top falhou")?;
    if top <= base + 0x1000 {
        return Err("image_top nao ficou acima da imagem");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação (`build_reloc_cross_page_pe`/`image_top`/`test_pe_bytes` não existem ou a relocação está errada).

- [ ] **Step 3: Implementar a relocação por página e `image_top`**

Em `kernel/src/loader.rs`, substituir o corpo da relocação DIR64 para tratar a fronteira de página. Em vez de obter um único `phys` e escrever 8 bytes a partir dele, leia e escreva byte a byte (ou em dois pedaços) traduzindo cada endereço virtual `va..va+8` pela sua página:
```rust
// Reads/writes a little-endian u64 at user virtual address `va` in `space`,
// crossing a page boundary safely. Returns false if any byte is unmapped.
fn rmw_u64(space: &AddressSpace, va: u64, f: impl FnOnce(u64) -> u64) -> bool {
    let mut bytes = [0u8; 8];
    for i in 0..8u64 {
        let a = va + i;
        let Some(phys) = space.translate(a & !(PAGE - 1)) else { return false };
        let p = (phys_to_virt(phys) + (a & (PAGE - 1))) as *const u8;
        bytes[i as usize] = unsafe { core::ptr::read_volatile(p) };
    }
    let v = f(u64::from_le_bytes(bytes)).to_le_bytes();
    for i in 0..8u64 {
        let a = va + i;
        let Some(phys) = space.translate(a & !(PAGE - 1)) else { return false };
        let p = (phys_to_virt(phys) + (a & (PAGE - 1))) as *mut u8;
        unsafe { core::ptr::write_volatile(p, v[i as usize]) };
    }
    true
}
```
e, no laço de relocação, para `REL_BASED_DIR64`: `if !rmw_u64(space, load_base + r.rva as u64, |v| v.wrapping_add(delta)) { return Err(LoadError::Map); }`.

Acrescentar `image_top`:
```rust
pub fn image_top(image: &[u8], load_base: u64) -> Result<u64, LoadError> {
    let pe = PeFile::parse(image).map_err(LoadError::Parse)?;
    let mut end = 0u64;
    for sec in pe.sections() {
        let sec_end = sec.rva as u64 + sec.virtual_size as u64;
        if sec_end > end {
            end = sec_end;
        }
    }
    let aligned = (end + PAGE - 1) & !(PAGE - 1);
    Ok(load_base + aligned)
}
```

Em `kernel/src/process.rs`: `spawn_pe` calcula `let stack_va = loader::image_top(image, load_base)? + PAGE;` (uma página de folga) e usa `stack_va` para a pilha e `stack_va + STACK_PAGES*PAGE` para o topo, em vez do `USER_STACK_VA` fixo. Remova/мantenha `USER_STACK_VA` só para os payloads antigos que não usam pilha (demos), ou migre-os também. Adicionar `pub fn test_pe_bytes() -> &'static [u8]` (devolve o PE de teste já existente) e, para o teste de relocação, `build_reloc_cross_page_pe()` + as constantes `pub const RELOC_FIXTURE_ORIGINAL: u64` e `pub const RELOC_FIXTURE_IMAGE_BASE: u64`. O PE da fixture tem: uma seção de 2 páginas no RVA 0x1000, com um `u64 = RELOC_FIXTURE_ORIGINAL` gravado no offset de arquivo correspondente ao RVA 0x1FFC, e um diretório de relocação base com um bloco para a página RVA 0x1000 contendo uma entrada `DIR64` no offset 0xFFC. `RELOC_FIXTURE_IMAGE_BASE` é a base preferida do fixture (ex.: 0x40_0000), diferente do `load_base` de teste (0x80_0000).

Nota para o implementador: montar a tabela de relocação do fixture exige escrever um bloco de relocação base válido (PageRVA=0x1000, BlockSize=0x0A = 8 + uma entrada de 2 bytes, entrada = `(10 << 12) | 0xFFC`), e apontar o data directory 5 para ele. Reutilize o `build_pe` da Task 4 como base e acrescente a seção de relocação. Confirme com o parser da crate `pe` (que já itera relocações) que o fixture é válido antes de usá-lo no ktest.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `reloc_dir64_crosses_page` e `stack_placed_above_image` passam; `pe_loads_runs_and_exits` continua passando (agora com a pilha acima da imagem). Demais variantes ok.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "fix(kernel): relocacao DIR64 entre paginas e pilha de usuario acima da imagem

Source: docs Microsoft PE Format specification (base relocations)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: TEB/PEB por thread e base de GS de usuário

**Files:**
- Modify: `crates/arch-x86_64/src/percpu.rs` (helper `set_user_gs_base`)
- Modify: `kernel/src/sched.rs` (Thread com `teb_va`; `switch_to_next` grava a base de GS de usuário)
- Create: `kernel/src/teb.rs` (montagem do TEB/PEB no espaço do processo)
- Modify: `kernel/src/process.rs` (criar TEB/PEB ao spawnar), `kernel/src/main.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`percpu.rs`, `target_os = "none"`): `pub unsafe fn set_user_gs_base(teb_va: u64)` — grava `teb_va` em `IA32_KERNEL_GS_BASE` (o valor que o `swapgs` de entrada no Ring 3 porá em `GS`).
- Produces (`sched.rs`): `Thread` ganha `teb_va: u64` (0 = sem TEB, threads de kernel). `spawn_user` recebe `teb_va`. `switch_to_next`, para a thread de destino com `teb_va != 0`, chama `set_user_gs_base(teb_va)` (antes de `switch_context`, junto com cr3/rsp0).
- Produces (`teb.rs`): `pub const TEB_VA`, `pub const PEB_VA` (VAs de usuário fixos acima da pilha, ou passados); `pub fn setup(space: &AddressSpace, teb_va: u64, peb_va: u64, image_base: u64, stack_base: u64, stack_limit: u64) -> Result<(), ...>` — mapeia uma página USER|WRITABLE|NO_EXECUTE para o TEB e outra para o PEB, e preenche `NtTib.StackBase`/`StackLimit`/`Self` e `ProcessEnvironmentBlock` no TEB e `ImageBaseAddress` no PEB (offsets da seção Global Constraints), escrevendo pela janela HHDM dos frames.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("user_reads_own_teb", user_reads_own_teb),` e:
```rust
fn user_reads_own_teb() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    // The test PE reads gs:[0x30] (NtTib.Self) and passes it to SYS_WRITE_MARK;
    // the kernel records it. It must equal the thread's TEB VA.
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let teb_va = crate::process::spawn_teb_probe_pe();
    for _ in 0..8000 {
        crate::sched::yield_now();
        let m = crate::syscall::LAST_MARK.load(Ordering::SeqCst);
        if m != 0 {
            return if m == teb_va {
                Ok(())
            } else {
                Err("gs:[0x30] (TEB Self) != TEB VA da thread")
            };
        }
    }
    Err("probe de TEB nao executou")
}
```
`spawn_teb_probe_pe()` cria o processo com um payload que faz `mov rax, gs:[0x30]; mov rdi, rax; mov rax, 1 /*SYS_WRITE_MARK*/; syscall; mov rax, 0; syscall` e retorna o `teb_va` escolhido.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

`percpu.rs`, em `mod hw`:
```rust
/// # Safety
/// Sets the GS base that `swapgs` installs on the next Ring 3 entry (the user
/// TEB). Call on the boot CPU before entering a user thread.
pub unsafe fn set_user_gs_base(teb_va: u64) {
    unsafe { write_msr(IA32_KERNEL_GS_BASE, teb_va) };
}
```
e exporte em `pub use hw::{...}`.

`kernel/src/teb.rs`: `setup` mapeia dois frames (TEB e PEB) no `space` em `teb_va`/`peb_va` (USER|WRITABLE|NO_EXECUTE), zera cada página pela HHDM, e escreve:
- TEB: em `teb_va + 0x08` = `stack_base`; `+0x10` = `stack_limit`; `+0x30` = `teb_va` (Self); `+0x60` = `peb_va`.
- PEB: em `peb_va + 0x10` = `image_base`.
Use escrita pela HHDM do frame (`phys_to_virt`), nunca pelo VA de usuário.

`kernel/src/sched.rs`: `Thread.teb_va`; `spawn_user(space, entry, user_stack_top, kstack, teb_va)`; em `switch_to_next`, no bloco que roda antes de `switch_context` (já define rsp0/kernel_rsp/cr3), adicionar: `if target_teb_va != 0 { arch_x86_64::percpu::set_user_gs_base(target_teb_va); }` (capture `target_teb_va` junto dos demais campos do destino).

`kernel/src/process.rs`: ao spawnar um processo (spawn_pe/spawn_teb_probe_pe), escolher `teb_va`/`peb_va` acima da pilha (ex.: `stack_top` alinhado + páginas), chamar `teb::setup(...)` e passar `teb_va` a `sched::spawn_user`. Payload do probe conforme Step 1.

`kernel/src/main.rs`: `mod teb;`.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `user_reads_own_teb` passa (gs:[0x30] == TEB VA). Demais ok.

Depuração: se o probe faultar ao ler `gs:[0x30]`, confirme que `set_user_gs_base` roda antes da entrada no Ring 3, que o TEB está mapeado USER no espaço do processo, e que o `swapgs` do `tukos_user_entry` traz o TEB para GS (a base de GS de usuário é `IA32_KERNEL_GS_BASE` antes do `swapgs`).

- [ ] **Step 5: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(kernel): TEB/PEB por thread e base de GS de usuario por thread

Source: docs Microsoft winternl.h public TEB/PEB layout; Intel SDM (SWAPGS)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Syscalls NT de console — `NtWriteFile` e `NtTerminateProcess`

**Files:**
- Create: `kernel/src/usercopy.rs` (cópia validada do buffer de usuário)
- Modify: `kernel/src/syscall.rs` (os dois novos números e a dispatch), `kernel/src/main.rs`, `kernel/src/process.rs` (payload de teste), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`usercopy.rs`): `pub fn copy_from_user(va: u64, len: u64) -> Option<alloc::vec::Vec<u8>>` — valida que `[va, va+len)` está inteiramente na metade baixa (`va + len <= 0x0000_8000_0000_0000`), traduz cada página no espaço ativo e copia pela HHDM; devolve `None` se qualquer byte estiver na metade alta ou não mapeado. Limite de `len` sensato (ex.: rejeitar `len > 1<<20`).
- Produces (`syscall.rs`): `pub const NT_WRITE_FILE: u64 = 2; pub const NT_TERMINATE_PROCESS: u64 = 3;`. Dispatch:
  - `NT_WRITE_FILE(handle=a0, buffer=a1, len=a2)`: `copy_from_user(a1, a2)`; se `None`, retorna `STATUS_ACCESS_VIOLATION` (`0xC0000005`); senão escreve os bytes no console (`kprint!` com a string, assumindo UTF-8/ASCII do teste) e, sob `ktest`, registra o buffer num static para o teste conferir; retorna 0 (`STATUS_SUCCESS`).
  - `NT_TERMINATE_PROCESS(code=a0)`: se `current_is_user()`, `exit_current()`; senão `exit_to_kernel(a0)`.
- Produces (`process.rs`): um PE de teste que faz `NtWriteFile(stdout, "TukOS\n", 6)` e `NtTerminateProcess(0)`; `spawn_nt_hello_pe()`.

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`, adicionar (antes de `two_user_processes_preempt`):
```rust
    ("ntwritefile_prints_user_buffer", ntwritefile_prints_user_buffer),
    ("ntwritefile_rejects_kernel_pointer", ntwritefile_rejects_kernel_pointer),
    ("ntwritefile_rejects_unmapped", ntwritefile_rejects_unmapped),
    ("nt_terminate_exits_and_reaps", nt_terminate_exits_and_reaps),
```
e:
```rust
fn ntwritefile_rejects_kernel_pointer() -> Result<(), &'static str> {
    // A higher-half (kernel) pointer must be rejected, not read.
    if crate::usercopy::copy_from_user(0xffff_ff80_0000_0000, 8).is_some() {
        return Err("copy_from_user aceitou ponteiro de kernel");
    }
    Ok(())
}

fn ntwritefile_rejects_unmapped() -> Result<(), &'static str> {
    // A low (user-range) but unmapped address must be rejected.
    if crate::usercopy::copy_from_user(0x0000_0000_1234_0000, 8).is_some() {
        return Err("copy_from_user aceitou endereco nao mapeado");
    }
    Ok(())
}

fn ntwritefile_prints_user_buffer() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::process::spawn_nt_hello_pe();
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6 {
            // "TukOS\n" is 6 bytes; the recorded bytes must match.
            if crate::syscall::last_write_is(b"TukOS\n") {
                return Ok(());
            }
            return Err("NtWriteFile registrou bytes errados");
        }
    }
    Err("NtWriteFile nao foi chamada pelo processo")
}

fn nt_terminate_exits_and_reaps() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_nt_hello_pe(); // ends with NtTerminateProcess
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("NtTerminateProcess nao recuperou os frames")
}
```
(`LAST_WRITE_LEN: AtomicU64` e `last_write_is(&[u8]) -> bool` ficam em `syscall.rs`, gravados por `NT_WRITE_FILE` sob `cfg(ktest)` num buffer static pequeno.)

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar a cópia validada**

`kernel/src/usercopy.rs`:
```rust
//! Validated copies of user memory. The kernel never dereferences a user
//! pointer directly: it checks the range is in the lower (user) half and that
//! every page is mapped in the active address space, then copies via the HHDM.

extern crate alloc;
use alloc::vec::Vec;

const USER_MAX: u64 = 0x0000_8000_0000_0000;
const MAX_LEN: u64 = 1 << 20;
const PAGE: u64 = 4096;

pub fn copy_from_user(va: u64, len: u64) -> Option<Vec<u8>> {
    if len == 0 {
        return Some(Vec::new());
    }
    if len > MAX_LEN {
        return None;
    }
    let end = va.checked_add(len)?;
    if end > USER_MAX {
        return None; // not entirely in the user half
    }
    let root = crate::arch_cr3_base();
    let mut out = Vec::with_capacity(len as usize);
    let mut a = va;
    while a < end {
        let page = a & !(PAGE - 1);
        let phys = crate::mem::paging::translate_in(root, page)?;
        let off = a & (PAGE - 1);
        let n = core::cmp::min(PAGE - off, end - a);
        let src = (crate::mem::frames::phys_to_virt(phys) + off) as *const u8;
        for i in 0..n {
            out.push(unsafe { core::ptr::read_volatile(src.add(i as usize)) });
        }
        a += n;
    }
    Some(out)
}
```
Em `kernel/src/main.rs`, `mod usercopy;`.

- [ ] **Step 4: Implementar as syscalls NT e o payload**

Em `kernel/src/syscall.rs`:
```rust
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
```
No `dispatch`:
```rust
        NT_WRITE_FILE => {
            // a0 = handle (console, ignored for now), a1 = buffer, a2 = length.
            match crate::usercopy::copy_from_user(a1, a2) {
                Some(bytes) => {
                    if let Ok(s) = core::str::from_utf8(&bytes) {
                        crate::kprint!("{s}");
                    }
                    #[cfg(feature = "ktest")]
                    {
                        LAST_WRITE_LEN.store(bytes.len() as u64, Ordering::SeqCst);
                        let mut g = LAST_WRITE.lock();
                        let n = core::cmp::min(bytes.len(), 64);
                        g[..n].copy_from_slice(&bytes[..n]);
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
```
(adicione os `use` necessários: `IrqMutex` de `arch_x86_64::interrupts`.)

Em `kernel/src/process.rs`, um PE de teste `nt_hello` cujo `.text` (sem imports, position-independent) faz:
```asm
lea rsi, [rip + msg]      ; buffer
; a0 = handle(1), a1 = buffer, a2 = len(6)
mov rax, 2                ; NT_WRITE_FILE
mov rdi, 1                ; stdout handle (ignored)
; rsi already = buffer
mov rdx, 6                ; length
syscall
mov rax, 3                ; NT_TERMINATE_PROCESS
mov rdi, 0
syscall
ud2
msg: .ascii "TukOS\n"
```
Monte os bytes de máquina com cuidado (o `lea rsi, [rip + msg]` precisa do deslocamento correto até `msg`; alternativamente coloque `msg` numa seção de dados com RVA conhecido e use `mov rsi, <load_base + rva_do_msg>` — porém o payload é position-independent só se usar `lea rip-relative`; para simplicidade e robustez, prefira colocar a string imediatamente após o código na mesma página e usar `lea rsi, [rip + disp]`, calculando `disp` = (endereço de msg) - (endereço da instrução seguinte ao lea)). Verifique os bytes; um deslocamento errado faz `NtWriteFile` ler lixo. `spawn_nt_hello_pe()` carrega esse PE como processo.

Nota: a string de teste é ASCII, então `from_utf8` sucede e `copy_from_user` a valida pelo espaço do processo (a string está numa página USER mapeada da imagem).

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest` → `ntwritefile_rejects_kernel_pointer`, `ntwritefile_rejects_unmapped`, `ntwritefile_prints_user_buffer` (serial mostra `TukOS`), `nt_terminate_exits_and_reaps` passam. Demais variantes ok. `cargo xtask run` mostra `TukOS` impresso pelo processo de usuário.

- [ ] **Step 6: Commit**

```bash
git add kernel
git commit -m "feat(kernel): syscalls NT NtWriteFile e NtTerminateProcess com copia validada de buffer

Source: docs Microsoft Learn (NtWriteFile, NtTerminateProcess — semantica publica)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os cinco gates de Global Constraints passam. Marco: um processo de usuário feito pelo TukOS lê seu próprio TEB via `GS`, escreve "TukOS" pela `NtWriteFile` (com o kernel validando o buffer) e termina pela `NtTerminateProcess`, com os recursos recuperados.
- Limitações (Plano 6): ainda não há `ntdll.dll` real nem resolução de imports, então o `.exe` de teste é do TukOS e faz `syscall` direto (um `.exe` real do Windows chamaria `ntdll!NtWriteFile`, que faz o mesmo `syscall`); a `NtWriteFile` usa uma assinatura simplificada (handle/buffer/len) — a assinatura completa do NT vem no Plano 6; o handle é ignorado (sem gerenciador de objetos/handles ainda).
- Plano 6 (degrau 3a completo): gerenciador de objetos e tabela de handles (para handles de console reais); o `ntdll.dll` como crate Rust cross-compilada para `x86_64-pc-windows-msvc` com stubs de syscall e o loader de imports em modo usuário; parsing das tabelas de import/export na crate `pe`; o kernel mapeia `.exe` + `ntdll` e inicia no thunk do `ntdll`; e o harness de conformidade que compila um `.exe` de console real com MSVC no Windows, captura a saída esperada e compara — o primeiro binário real do Windows a rodar no TukOS.
