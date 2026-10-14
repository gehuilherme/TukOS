# TukOS Plano 6 — ABI de syscall do Windows, executivo (objetos/handles) e NtWriteFile real

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** O TukOS atende chamadas de sistema na convenção de registradores do Windows x64 (argumentos em `r10, rdx, r8, r9` e os demais lidos da pilha do usuário), tem um gerenciador de objetos com tabela de handles por processo e um handle de console, e implementa a `NtWriteFile` com a assinatura NT completa (9 parâmetros, escrevendo o buffer validado no console e preenchendo o `IO_STATUS_BLOCK`).

**Architecture:** Primeiro os pré-requisitos de segurança/robustez que a revisão do Plano 5 fixou: `translate_in` passa a exigir o bit `USER` (e rejeitar páginas HUGE) para a cópia de usuário, e o loader de PE devolve erro em vez de dar panic em imagens malformadas. Depois a entrada de syscall muda para a convenção do Windows x64 e passa o `rsp` do usuário ao dispatch, que lê os argumentos 5+ da pilha do usuário por cópia validada. Em seguida um gerenciador de objetos mínimo com uma tabela de handles por processo e um objeto de console. Por fim, a `NtWriteFile` com a assinatura NT real resolve o handle, valida e escreve o buffer, e preenche o `IO_STATUS_BLOCK`. Tudo testado com um `.exe` feito pelo TukOS que chama a `NtWriteFile` exatamente como o `ntdll` faria.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `limine` 0.5, `spin` 0.10, `mm`, `arch-x86_64`, `pe`, QEMU + OVMF.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 4 "Executivo": gerenciador de objetos, tabela de handles; "Syscalls": a interface exportada do `ntdll` e a semântica NT). Sucede o Plano 5 (TEB/PEB + syscalls NT), já na `main`.

**Fora deste plano (vai para o Plano 7, degrau 3a completo):** o `ntdll.dll` como crate Rust cross-compilada para `x86_64-pc-windows-msvc`, o loader de imports em modo usuário (`LdrInitializeThunk`), o parsing das tabelas de import/export na crate `pe`, a resolução de imports, e o harness de conformidade que compila um `.exe` de console real com MSVC no Windows. Este plano usa um `.exe` **sem imports**, feito pelo TukOS, que faz `syscall` direto com a convenção do Windows — prova a ABI, o executivo e a `NtWriteFile` real sem precisar do `ntdll`.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64, um núcleo. Código dependente de CPU vive em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft.
- Clean room nível 0 apenas (ver `CLEAN_ROOM.md` e `AGENTS.md`): a convenção de registradores de syscall do Windows x64, a assinatura da `NtWriteFile`, o `IO_STATUS_BLOCK` e os `NTSTATUS` vêm da documentação pública do Microsoft Learn e do `winternl.h`/`ntstatus.h` públicos. Os **números** de syscall são do TukOS. Nenhum binário da Microsoft é desassemblado; o `.exe` de teste é feito pelo TukOS.
- Commits de comportamento de ABI/protocolo declaram a fonte: `Source: docs <documento>` (ABI de syscall: `Source: docs Microsoft Learn x64 syscall calling convention`; NtWriteFile: `Source: docs Microsoft Learn NtWriteFile / IO_STATUS_BLOCK`).
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- Convenção de syscall do Windows x64 (documentação pública): número da syscall em `eax`; argumentos 1–4 em `r10, rdx, r8, r9` (o `ntdll` faz `mov r10, rcx`); argumentos 5+ na pilha do usuário a partir de `[rsp_do_usuario + 0x28]` (8 do endereço de retorno + 32 de shadow space). `NTSTATUS`: `STATUS_SUCCESS = 0`, `STATUS_ACCESS_VIOLATION = 0xC0000005`, `STATUS_INVALID_HANDLE = 0xC0000008`.
- Números de syscall do TukOS (expandidos): `0 = SYS_EXIT` (legado de teste), `1 = SYS_WRITE_MARK` (legado de teste), `2 = NT_WRITE_FILE`, `3 = NT_TERMINATE_PROCESS`, `4 = NT_CLOSE`.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **Uma syscall com argumentos na pilha cujo ponteiro de pilha do usuário é inválido** não pode faultar o kernel nem ler memória do kernel; o dispatch lê os argumentos 5+ por cópia validada (`copy_from_user`), rejeitando pilha não mapeada ou na metade alta. Coberto por `stack_args_rejected_when_user_rsp_bad` (Task 2).
2. **`copy_from_user` sobre uma página da metade baixa sem o bit `USER`** (ou uma página HUGE) leria memória de kernel; `translate_in`/a tradução usada pela cópia exige `USER` em todos os níveis e rejeita HUGE. Coberto por `translate_user_requires_user_bit` e `translate_user_rejects_huge` (Task 1).
3. **Um PE malformado passado ao loader** deve devolver `LoadError`, nunca dar panic no kernel; o loader usa aritmética checada e propaga erro. Coberto por `load_pe_rejects_malformed_without_panic` (Task 1).
4. **`NtWriteFile` com um handle inválido** retorna `STATUS_INVALID_HANDLE` sem escrever; com handle de console válido escreve e preenche `IoStatusBlock.Information` com o número de bytes. Coberto por `ntwritefile_bad_handle` e `ntwritefile_fills_iosb` (Task 4).
5. **Uma entrada de handle fora da faixa da tabela ou já fechada** deve resolver para "inválido", não indexar fora dos limites nem reusar um objeto fechado. Coberto por `handle_table_bounds_and_close` (Task 3).

---

### Task 1: `translate_in` exige USER + robustez do loader

**Files:**
- Modify: `kernel/src/mem/paging.rs` (variante `translate_user_in` que exige USER e rejeita HUGE), `kernel/src/usercopy.rs` (usar a nova variante)
- Modify: `kernel/src/loader.rs` (aritmética checada; sem panic), `kernel/src/process.rs` (propagar `LoadError`), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`paging.rs`): `pub fn translate_user_in(root: u64, virt: u64) -> Option<u64>` — como `translate_in`, mas retorna `None` se qualquer nível (ou a folha) não tiver o bit `USER`, ou se encontrar uma entrada HUGE (bit 7) num nível intermediário. `copy_from_user` passa a usá-la.
- `load_pe`/`image_top`: toda soma com `rva`/`virtual_size`/`load_base` usa `checked_add`/`checked_mul`, devolvendo `LoadError` em overflow; nada de `.expect`/índice que faça panic. `spawn_pe`/`spawn_test_pe`/etc. devolvem `Result<usize, LoadError>` ou tratam o erro (não `.expect`).

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`, adicionar:
```rust
    ("translate_user_requires_user_bit", translate_user_requires_user_bit),
    ("translate_user_rejects_huge", translate_user_rejects_huge),
    ("load_pe_rejects_malformed_without_panic", load_pe_rejects_malformed_without_panic),
```
e:
```rust
fn translate_user_requires_user_bit() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    use mm::paging::PageFlags;
    let space = AddressSpace::new_user();
    let user_frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    let kern_frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    unsafe {
        space.map(0x10_0000, user_frame, PageFlags::USER.union(PageFlags::WRITABLE)).map_err(|_| "map user")?;
        // A lower-half page WITHOUT the USER bit (kernel-only), as a future kernel
        // mapping might create. translate_user_in must refuse it.
        space.map(0x20_0000, kern_frame, PageFlags::WRITABLE).map_err(|_| "map kern")?;
    }
    if crate::mem::paging::translate_user_in(space.root(), 0x10_0000).is_none() {
        return Err("pagina USER deveria ser aceita");
    }
    if crate::mem::paging::translate_user_in(space.root(), 0x20_0000).is_some() {
        return Err("pagina sem USER foi aceita (vazamento de kernel)");
    }
    Ok(())
}

fn translate_user_rejects_huge() -> Result<(), &'static str> {
    // A synthetic PML-walk: map a normal USER page, then verify translate_user_in
    // returns None for an address whose intermediate entry we mark HUGE.
    // (Implemented via a helper that sets a HUGE bit on an intermediate table entry
    // in a fresh space; see the test helper in process.rs / a paging test hook.)
    crate::mem::paging::test_huge_rejected()
}

fn load_pe_rejects_malformed_without_panic() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    let space = AddressSpace::new_user();
    // Truncated / bad image: must return Err, not panic.
    if crate::loader::load_pe(&space, &[0u8; 8], 0x40_0000).is_ok() {
        return Err("load_pe aceitou imagem malformada");
    }
    Ok(())
}
```
`crate::mem::paging::test_huge_rejected()` é um helper `#[cfg(feature = "ktest")]` em `paging.rs` que cria um espaço, mapeia uma página USER, marca o bit HUGE (1<<7) na entrada de nível 2 correspondente e confirma que `translate_user_in` devolve `None`, depois desfaz. (Se preferir, exponha um teste puro equivalente; o essencial é cobrir o caminho HUGE.)

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Em `paging.rs`, `translate_user_in` espelha `translate_in` mas, a cada nível intermediário, após `entry_present`, checa:
- se `mm::paging::entry_flags(entry).contains(PageFlags::HUGE)` → `return None` (não suportamos páginas grandes de usuário aqui);
- se `!entry_flags(entry).contains(PageFlags::USER)` → `return None`.
E na folha, exige `USER`. Use os helpers já existentes de `mm::paging`.

Em `usercopy.rs`, trocar `translate_in` por `translate_user_in`.

Em `loader.rs`, substituir as somas cruas por `checked_*` devolvendo `LoadError::Overflow` (novo variante) quando estourar; remover qualquer `.expect`. Em `process.rs`, as funções de spawn que faziam `load_pe(...).expect(...)` passam a propagar/registrar o erro (num kernel de teste, podem devolver `Result` e o chamador em `ktest` trata; os demos podem `expect` com mensagem clara, mas o caminho `load_pe` em si não deve dar panic internamente).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → os três novos testes passam; os existentes (`pe_loads_runs_and_exits` etc.) continuam. `cargo test -p pe` ok.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): translate_user_in exige bit USER e rejeita HUGE; loader sem panic

Source: docs Intel SDM Vol. 3A, secao 4.5 (bit U/S e page size)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Convenção de syscall do Windows x64 e argumentos na pilha

**Files:**
- Modify: `crates/arch-x86_64/src/syscall.rs` (stub de entrada + assinatura do dispatch)
- Modify: `kernel/src/syscall.rs` (nova assinatura; ler args da pilha), `kernel/src/process.rs` (payload de teste), `kernel/src/ktest.rs`

**Interfaces:**
- O stub `tukos_syscall_entry` passa a seguir a convenção do Windows: número em `eax` (já é), argumentos 1–4 em `r10, rdx, r8, r9`, e **passa ao dispatch o `rsp` do usuário** (salvo em `gs:[OFF_USER_RSP_SCRATCH]`) para que o kernel leia os argumentos 5+ da pilha. A assinatura do dispatch vira `fn(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64, user_rsp: u64) -> u64` (a0..a3 = r10, rdx, r8, r9).
- `kernel/src/syscall.rs`: `dispatch` recebe `user_rsp`; um helper `fn stack_arg(user_rsp: u64, index: u64) -> Option<u64>` lê o argumento `index` (0-based além dos 4 de registrador) de `[user_rsp + 0x28 + index*8]` via `copy_from_user`.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar:
```rust
    ("syscall_reads_six_args_reg_and_stack", syscall_reads_six_args_reg_and_stack),
    ("stack_args_rejected_when_user_rsp_bad", stack_args_rejected_when_user_rsp_bad),
```
e (usando uma syscall de teste temporária `SYS_SUM6 = 100` que soma 6 argumentos: 4 de registrador + 2 da pilha, e grava o resultado em `LAST_MARK`):
```rust
fn syscall_reads_six_args_reg_and_stack() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    crate::process::spawn_sum6_pe(); // calls SYS_SUM6 with args 1..=6 (4 reg + 2 stack)
    for _ in 0..8000 {
        crate::sched::yield_now();
        let v = crate::syscall::LAST_MARK.load(Ordering::SeqCst);
        if v != 0 {
            return if v == 21 { Ok(()) } else { Err("soma de 6 args (4 reg + 2 pilha) incorreta") };
        }
    }
    Err("SYS_SUM6 nao executou")
}

fn stack_args_rejected_when_user_rsp_bad() -> Result<(), &'static str> {
    // stack_arg over a kernel/unmapped user_rsp must return None (no fault/leak).
    if crate::syscall::stack_arg(0xffff_ff80_0000_0000, 0).is_some() {
        return Err("stack_arg aceitou rsp de kernel");
    }
    if crate::syscall::stack_arg(0x0000_0000_7777_0000, 0).is_some() {
        return Err("stack_arg aceitou rsp nao mapeado");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar o stub e o dispatch**

Em `crates/arch-x86_64/src/syscall.rs`, reescrever a reordenação do stub para a convenção do Windows e passar o `rsp` do usuário como 6º argumento C. A ABI de syscall do Windows entrega: `eax`=nr, `r10`=a0, `rdx`=a1, `r8`=a2, `r9`=a3 (o `ntdll` copia `rcx`→`r10` antes do `syscall`). O dispatch C `tukos_syscall_dispatch(rdi=nr, rsi=a0, rdx=a1, rcx=a2, r8=a3, r9=user_rsp)`. Como o stub salva o `rsp` do usuário em `gs:[OFF_USER_RSP_SCRATCH]` logo na entrada, use esse valor para o 6º argumento. Esboço (verifique a ordem para não sobrescrever um registrador antes de lê-lo; o `rsp` do usuário salvo deve ser lido **antes** de trocar `rsp`):
```asm
tukos_syscall_entry:
    swapgs
    mov gs:[{off_user_rsp}], rsp      // save user rsp (arg source for stack args)
    mov rsp, gs:[{off_kernel_rsp}]
    and rsp, -16
    push rcx                          // user RIP (for sysretq)
    push r11                          // user RFLAGS
    // Windows x64 syscall ABI: eax=nr, r10,rdx,r8,r9 = a0..a3.
    // Map to C ABI tukos_syscall_dispatch(rdi=nr, rsi=a0, rdx=a1, rcx=a2, r8=a3, r9=user_rsp):
    mov r9,  gs:[{off_user_rsp}]      // user_rsp (6th C arg)
    mov rcx, r8                       // a2 (r8) -> C 4th (rcx)
    // careful ordering below; a3 currently in r9? no, a3 is in r9 register from the syscall ABI...
    // (Implementer: resolve the register shuffle precisely. The source regs are
    //  eax=nr, r10=a0, rdx=a1, r8=a2, r9=a3; the dest C regs are rdi,rsi,rdx,rcx,r8,r9.
    //  A correct non-clobbering sequence, computing user_rsp into a temp first:)
    // Suggested exact sequence:
    //   mov r11_saved? no. Use the stack-saved user_rsp.
    ...
    call tukos_syscall_dispatch
    pop r11
    pop rcx
    mov rsp, gs:[{off_user_rsp}]
    swapgs
    sysretq
```
Nota crítica ao implementador: a reordenação de registradores aqui é o ponto clássico de bug. Os registradores-fonte da ABI de syscall do Windows são `nr=eax, a0=r10, a1=rdx, a2=r8, a3=r9`; os destinos da ABI C de 6 argumentos são `rdi, rsi, rdx, rcx, r8, r9`. Note que `rdx` é fonte (a1) **e** destino (a1) — coincide, ótimo; mas `r8` é fonte (a2) e destino (a3), e `r9` é fonte (a3) e destino (user_rsp) — há conflito. Resolva movendo primeiro os que seriam sobrescritos: por exemplo `mov rcx, r8` (a2→rcx) e `mov rsi, r10` (a0→rsi) e `mov rdi, rax` (nr→rdi) antes de tocar `r8`/`r9`; depois `mov r8, r9` (a3→r8) e por fim `mov r9, gs:[off_user_rsp]`. **Derive a sequência exata e correta, e verifique com um teste real (SYS_SUM6 com valores distintos 1..6), porque um registrador trocado passa o argumento errado.** `rdx` já está no lugar (a1→rdx, idêntico). Mantenha os números de syscall/retorno existentes funcionando (os handlers atuais só usam a0..a3; a assinatura ganha `user_rsp` ignorado por eles).

Em `kernel/src/syscall.rs`: a assinatura de `dispatch` ganha `user_rsp: u64`; adicionar:
```rust
pub fn stack_arg(user_rsp: u64, index: u64) -> Option<u64> {
    // Windows x64: stack args start at [user_rsp + 0x28] (return addr + 32-byte shadow).
    let addr = user_rsp.checked_add(0x28)?.checked_add(index.checked_mul(8)?)?;
    let bytes = crate::usercopy::copy_from_user(addr, 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}
```
Adicionar o braço de teste `SYS_SUM6 = 100` sob `cfg(feature = "ktest")` no `dispatch`: `a0+a1+a2+a3 + stack_arg(user_rsp,0)? + stack_arg(user_rsp,1)?`, gravando em `LAST_MARK`. `set_dispatch` e `tukos_syscall_dispatch` passam a usar a assinatura de 6 argumentos.

Em `kernel/src/process.rs`: `spawn_sum6_pe()` com um payload que coloca `r10=1, rdx=2, r8=3, r9=4`, empilha `5` e `6` nos offsets `+0x28`/`+0x30` do `rsp` (reservando também o shadow space e um "retorno" fictício), `mov eax, 100`, `syscall`, depois `NtTerminateProcess`. Monte os bytes com cuidado; o teste confere a soma 21.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `syscall_reads_six_args_reg_and_stack` (resultado 21) e `stack_args_rejected_when_user_rsp_bad` passam; todas as syscalls existentes continuam funcionando (as variantes ktest anteriores passam). 

Depuração: se a soma vier errada, imprima cada argumento recebido pelo dispatch (serial) e compare com 1..6; o erro quase sempre está na sequência de `mov` da reordenação.

- [ ] **Step 5: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(kernel): ABI de syscall do Windows x64 com argumentos na pilha do usuario

Source: docs Microsoft Learn x64 syscall calling convention
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Gerenciador de objetos e tabela de handles

**Files:**
- Create: `kernel/src/object.rs` (objeto + tabela de handles)
- Modify: `kernel/src/sched.rs` (cada processo/thread de usuário tem uma tabela de handles), `kernel/src/main.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`object.rs`):
  - `pub enum Object { Console }` (só o console neste plano; extensível)
  - `pub struct HandleTable { ... }` com `pub fn new() -> Self`, `pub fn insert(&mut self, obj: Object) -> u64` (devolve um valor de handle), `pub fn get(&self, handle: u64) -> Option<&Object>`, `pub fn close(&mut self, handle: u64) -> bool`
  - handles são valores pequenos (ex.: múltiplos de 4 começando em 4, como o NT; `0` é inválido). Um handle de console bem conhecido é inserido na criação do processo (equivalente simplificado do `STD_OUTPUT_HANDLE`).
  - `pub const CONSOLE_HANDLE: u64` (o handle fixo que um processo recebe para o console neste plano).
- A tabela de handles é parte do processo (guardada junto do `AddressSpace`/thread de usuário no scheduler). `NtWriteFile`/`NtClose` (Task 4) a consultam pela thread atual.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("handle_table_bounds_and_close", handle_table_bounds_and_close),` e:
```rust
fn handle_table_bounds_and_close() -> Result<(), &'static str> {
    use crate::object::{HandleTable, Object};
    let mut t = HandleTable::new();
    if t.get(0).is_some() {
        return Err("handle 0 deveria ser invalido");
    }
    if t.get(0x1000).is_some() {
        return Err("handle fora da faixa deveria ser invalido");
    }
    let h = t.insert(Object::Console);
    if !matches!(t.get(h), Some(Object::Console)) {
        return Err("handle valido nao resolveu");
    }
    if !t.close(h) {
        return Err("close de handle valido falhou");
    }
    if t.get(h).is_some() {
        return Err("handle fechado ainda resolve");
    }
    if t.close(h) {
        return Err("close duplo deveria falhar");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

`kernel/src/object.rs`: `HandleTable` com um `Vec<Option<Object>>` (ou array fixo pequeno). `insert` acha um slot livre e devolve `(índice+1)*4` como valor de handle; `get`/`close` convertem o valor de volta ao índice validando a faixa e que o slot está ocupado. `CONSOLE_HANDLE` = o valor que `insert(Object::Console)` dá para o primeiro handle (ex.: 4). `Object` deriva o necessário; `HandleTable` é `Send` (usada sob lock por processo).

Em `kernel/src/sched.rs`: a thread de usuário ganha uma `handle_table: Option<HandleTable>` (ou um `Process` que a contém); ao `spawn_user`, criar a tabela e inserir o console (`CONSOLE_HANDLE`). Um helper `pub fn with_current_handles<R>(f: impl FnOnce(&mut HandleTable) -> R) -> Option<R>` dá acesso à tabela da thread atual (para a Task 4). Em processos de kernel, `None`.

Em `kernel/src/main.rs`: `mod object;`.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `handle_table_bounds_and_close` passa. Demais ok.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): gerenciador de objetos e tabela de handles por processo

Source: docs Microsoft Learn (handles e objetos do kernel — modelo publico)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: `NtWriteFile` com a assinatura NT completa e `NtClose`

**Files:**
- Modify: `kernel/src/syscall.rs` (NtWriteFile 9 params + NtClose), `kernel/src/process.rs` (payload NT real), `kernel/src/ktest.rs`

**Interfaces:**
- `NtWriteFile(FileHandle, Event, ApcRoutine, ApcContext, IoStatusBlock, Buffer, Length, ByteOffset, Key)` — 9 parâmetros (a0..a3 em registrador = FileHandle, Event, ApcRoutine, ApcContext; os demais na pilha do usuário: `IoStatusBlock`=arg5, `Buffer`=arg6, `Length`=arg7, `ByteOffset`=arg8, `Key`=arg9). O kernel:
  1. resolve `FileHandle` na tabela de handles da thread atual → se não for o console, retorna `STATUS_INVALID_HANDLE`;
  2. lê `Buffer`/`Length` (args 6/7 via `stack_arg` + `copy_from_user`); valida e escreve no console;
  3. escreve o `IO_STATUS_BLOCK` (duas palavras: `Status` e `Information`) no ponteiro `IoStatusBlock` (arg5) **na memória do usuário**, por uma cópia validada *para* o usuário (`copy_to_user`), com `Status = STATUS_SUCCESS` e `Information = Length`;
  4. retorna `STATUS_SUCCESS`.
- `NtClose(Handle)` fecha o handle; retorna `STATUS_SUCCESS` ou `STATUS_INVALID_HANDLE`.
- Produces (`usercopy.rs`): `pub fn copy_to_user(va: u64, data: &[u8]) -> bool` — valida (metade baixa, mapeado, USER) e escreve pela HHDM; `false` se inválido.

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`, adicionar (antes de `two_user_processes_preempt`):
```rust
    ("ntwritefile_fills_iosb", ntwritefile_fills_iosb),
    ("ntwritefile_bad_handle", ntwritefile_bad_handle),
```
e:
```rust
fn ntwritefile_fills_iosb() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    crate::syscall::IOSB_INFO.store(0, Ordering::SeqCst);
    // The test PE calls NtWriteFile(CONSOLE_HANDLE, 0,0,0, &iosb, "TukOS\n", 6, 0, 0),
    // then reads iosb.Information back and reports it via SYS_WRITE_MARK before exit.
    crate::process::spawn_ntwrite_real_pe();
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6
            && crate::syscall::last_write_is(b"TukOS\n")
        {
            // The process also reported iosb.Information via LAST_MARK.
            if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 6 {
                return Ok(());
            }
            return Err("IoStatusBlock.Information != Length");
        }
    }
    Err("NtWriteFile real nao executou")
}

fn ntwritefile_bad_handle() -> Result<(), &'static str> {
    // Directly exercise the dispatch with an invalid handle: no console write.
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    let status = crate::syscall::ntwritefile_for_test(0x9999, 0, 0, 0);
    if status != crate::syscall::STATUS_INVALID_HANDLE {
        return Err("handle invalido deveria dar STATUS_INVALID_HANDLE");
    }
    if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) != 0 {
        return Err("handle invalido nao deveria escrever");
    }
    Ok(())
}
```
(`IOSB_INFO` pode não ser necessário se o processo reportar via `LAST_MARK`; mantenha o que for mais simples. `ntwritefile_for_test` é um atalho `#[cfg(feature="ktest")]` que chama a lógica de `NtWriteFile` com um handle dado e buffer vazio, sem precisar de um processo — para cobrir o caminho de handle inválido de forma determinística.)

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Em `usercopy.rs`, `copy_to_user` espelha `copy_from_user` (metade baixa, `translate_user_in`, HHDM), mas escrevendo.

Em `kernel/src/syscall.rs`, implementar o braço `NT_WRITE_FILE` com a assinatura completa: `FileHandle = a0`; `IoStatusBlock = stack_arg(user_rsp, 0)`; `Buffer = stack_arg(user_rsp, 1)`; `Length = stack_arg(user_rsp, 2)` (args 5/6/7; os args 8/9 — ByteOffset/Key — são lidos mas ignorados neste plano). Resolver o handle via `crate::sched::with_current_handles(|t| t.get(FileHandle).cloned())`; se não for `Console`, `STATUS_INVALID_HANDLE`. Senão `copy_from_user(Buffer, Length)` → escreve no console (`kprint!`) e grava `IoStatusBlock` com `copy_to_user` (16 bytes: `Status`=0, `Information`=Length). Registrar sob ktest. Implementar `ntwritefile_for_test` e `NT_CLOSE`. Manter os statics de teste (`LAST_WRITE_LEN`, `LAST_WRITE`, `last_write_is`).

Em `kernel/src/process.rs`, `spawn_ntwrite_real_pe()` com um payload que: reserva espaço na pilha para o `IO_STATUS_BLOCK` (16 bytes) e para os args de pilha; coloca `rcx/r10`=CONSOLE_HANDLE (a0=FileHandle), `rdx`=0 (Event), `r8`=0 (ApcRoutine), `r9`=0 (ApcContext); empilha nos offsets `+0x28..` o ponteiro do `IoStatusBlock`, o ponteiro do buffer `msg`, `6`, `0`, `0`; `mov eax, NT_WRITE_FILE`; `syscall`; depois lê `IoStatusBlock.Information` e o reporta via `SYS_WRITE_MARK`; depois `NtTerminateProcess`. Monte os bytes com cuidado (os offsets de pilha seguem a convenção do Windows da Task 2). O `msg` "TukOS\n" fica numa página USER da imagem (alcançado por `lea rip-relative`).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `ntwritefile_fills_iosb` (console mostra `TukOS`, `IoStatusBlock.Information == 6`) e `ntwritefile_bad_handle` passam. Demais variantes ok. `cargo xtask run` mostra `TukOS` impresso.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): NtWriteFile com assinatura NT completa, IO_STATUS_BLOCK e NtClose

Source: docs Microsoft Learn NtWriteFile / IO_STATUS_BLOCK
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os cinco gates de Global Constraints passam. Marco: um `.exe` feito pelo TukOS chama a `NtWriteFile` exatamente como o `ntdll` faria (convenção do Windows, 9 argumentos, `IO_STATUS_BLOCK`), com o kernel resolvendo o handle de console, validando o buffer e preenchendo o status; e termina pela `NtTerminateProcess`.
- Limitações (Plano 7): ainda não há `ntdll.dll` real nem resolução de imports, então o `.exe` de teste é do TukOS; os argumentos 8/9 (`ByteOffset`/`Key`) são ignorados; o handle de console é fixo (sem `NtCreateFile`/`GetStdHandle` reais ainda).
- Plano 7 (degrau 3a completo): parsing das tabelas de import/export na crate `pe`; o `ntdll.dll` como crate Rust cross-compilada para `x86_64-pc-windows-msvc` com os stubs de syscall `Nt*` (`mov r10,rcx; mov eax,ssn; syscall; ret`) e o loader de imports em modo usuário (`LdrInitializeThunk`); o kernel mapeia `.exe` + `ntdll` e inicia no thunk do `ntdll`; e o harness de conformidade que compila um `.exe` de console real com MSVC no Windows, roda no Windows real para capturar a saída esperada, e compara com a saída no TukOS — o primeiro binário real do Windows a rodar no TukOS.
