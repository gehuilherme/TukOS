# TukOS Plano 8 — Carregar .exe + ntdll e resolver imports

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** O kernel do TukOS mapeia um `.exe` e o `ntdll.dll` no mesmo espaço de endereços de um processo, resolve os imports do `.exe` contra os exports do `ntdll` (preenchendo a IAT com validação de limites), e executa o `.exe`; um `.exe` feito pelo TukOS que importa `ntdll!NtWriteFile`/`NtTerminateProcess` chama essas funções pela IAT, que caem nos stubs do `ntdll`, que fazem `syscall` ao kernel, imprimindo "TukOS" e terminando.

**Architecture:** O `ntdll.dll` real (compilado no Plano 7) é entregue ao kernel em tempo de build (a `build.rs` do kernel localiza/compila a DLL e a embute; em hosts sem MSVC ela fica vazia e o teste de dois módulos é pulado). O loader ganha a capacidade de carregar dois módulos no mesmo `AddressSpace` (o `.exe` na sua base, o `ntdll` numa base deslocada com relocações aplicadas) e de resolver os imports: para cada entrada de import do `.exe`, acha o export correspondente no `ntdll` e grava `base_do_ntdll + rva_do_export` no slot da IAT, validando que o slot está dentro de uma seção gravável da imagem. Por fim, um `.exe` de teste com uma import table real (feito pelo TukOS) é carregado com sua dependência `ntdll` e executado.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `pe`, `ntdll`, `mm`, `arch-x86_64`, QEMU + OVMF.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 5: o kernel mapeia `.exe` + `ntdll`; a crate `pe` para resolução de imports). Sucede o Plano 7 (pe import/export + ntdll), já na `main`.

**Fora deste plano (vai para o Plano 9, degrau 3a completo):** o loader de imports em **modo usuário** no `ntdll` (`LdrInitializeThunk`, percorrendo `PEB->Ldr`) — este plano resolve os imports **pelo kernel** (abordagem em estágio, a ser migrada para o `ntdll` no Plano 9); e um `.exe` **real compilado com MSVC** + o harness de conformidade. Aqui o `.exe` é feito pelo TukOS e importa por **nome** (resolução por ordinal fica para quando um binário real a exigir).

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64. Código dependente de CPU em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft (o nome `ntdll.dll` e os exports `Nt*` são interface de interoperabilidade, permitidos).
- Clean room nível 0 apenas: formato PE (import/export, IAT) da especificação pública; o `.exe` de teste e o `ntdll` são feitos pelo TukOS; nenhum binário da Microsoft.
- `build.rs` do kernel: ao localizar/compilar o `ntdll.dll`, **não** pode quebrar o build em hosts sem MSVC (Linux CI) — embute um slice vazio e o teste de dois módulos se auto-pula.
- Commits declaram a fonte: `Source: docs Microsoft PE Format specification` (import resolution/IAT).
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **Um slot de IAT (`iat_rva`) que aponte para fora de uma seção, ou para uma seção não-gravável** não pode ser escrito; a resolução valida que `[iat_rva, iat_rva+8)` está dentro de uma seção gravável da imagem mapeada antes de gravar o endereço. Coberto por `import_resolve_rejects_bad_iat_slot` (Task 2).
2. **Um import para uma DLL que não é o `ntdll`, ou para uma função que o `ntdll` não exporta** deve ser tratado sem corromper nada (slot preenchido com um stub de "não implementado" ou a resolução falha de forma limpa). Coberto por `import_resolve_unknown_function` (Task 2).
3. **O `ntdll` carregado numa base diferente da preferida** precisa das relocações aplicadas e o endereço do export calculado sobre a base real; a resolução usa `base_do_ntdll + rva_do_export`. Coberto pelo roundtrip (Task 3) com o `ntdll` numa base deslocada.
4. **As duas imagens se sobrepondo no espaço de endereços** (bases/tamanhos mal escolhidos) corromperia uma com a outra; o `.exe` e o `ntdll` são mapeados em faixas distintas e não sobrepostas. Coberto por `two_modules_no_overlap` (Task 2).
5. **O `ntdll.dll` não embutido (host sem MSVC)** não pode quebrar o build nem fazer o teste falhar; o teste de dois módulos verifica `ntdll` embutido e, se vazio, retorna sucesso (pulado). Coberto pela lógica de skip em `exe_imports_ntdll_and_runs` (Task 3).

---

### Task 1: Entregar o `ntdll.dll` ao kernel + acessar export por nome e ordinal

**Files:**
- Create/Modify: `kernel/build.rs` (localizar/compilar e copiar o `ntdll.dll` para `OUT_DIR`), `kernel/src/ntdll_image.rs` (ou um `include_bytes!` num módulo), `kernel/src/main.rs`
- Modify: `crates/pe/src/lib.rs` (acessor `export_rva_by_name` e, opcional, por ordinal)
- Modify: `kernel/src/ktest.rs`

**Interfaces:**
- Produces (kernel): `pub static NTDLL_BYTES: &[u8]` — os bytes do `ntdll.dll` embutido (vazio se o host não compilou a DLL). `build.rs`: se conseguir compilar/localizar `ntdll.dll` (host Windows com MSVC), copia para `${OUT_DIR}/ntdll.dll`; senão escreve um arquivo vazio. O módulo faz `include_bytes!(concat!(env!("OUT_DIR"), "/ntdll.dll"))`.
- Produces (`pe`): `pub fn export_rva_by_name(&self, name: &str) -> Option<u32>` — devolve o RVA do export cujo nome bate (usa o `exports()` já existente).

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("ntdll_embedded_parses", ntdll_embedded_parses),` e:
```rust
fn ntdll_embedded_parses() -> Result<(), &'static str> {
    let bytes = crate::ntdll_image::NTDLL_BYTES;
    if bytes.is_empty() {
        return Ok(()); // ntdll not embedded on this host (no MSVC): skip.
    }
    let pe = pe::PeFile::parse(bytes).map_err(|_| "ntdll.dll embutido nao e PE valido")?;
    if pe.export_rva_by_name("NtWriteFile").is_none() {
        return Err("ntdll embutido nao exporta NtWriteFile");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação (`crate::ntdll_image`/`export_rva_by_name` não existem).

- [ ] **Step 3: Implementar**

`kernel/build.rs`: tenta `cargo build -p ntdll --target x86_64-pc-windows-msvc --release` (ou localiza `target/x86_64-pc-windows-msvc/release/ntdll.dll` se já existir); se der certo, copia para `${OUT_DIR}/ntdll.dll`; se falhar (sem MSVC/target), cria `${OUT_DIR}/ntdll.dll` vazio. **Nunca** faça `panic!` por falta de MSVC — imprima um `cargo:warning=` e siga com o arquivo vazio. Adicione `cargo:rerun-if-changed=../crates/ntdll/src/lib.rs`.
`kernel/src/ntdll_image.rs`: `pub static NTDLL_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ntdll.dll"));`. Em `main.rs`, `mod ntdll_image;`.
`crates/pe`: `export_rva_by_name` itera `exports()` e devolve o `rva` do nome casado.

Nota para o implementador: a `build.rs` do kernel rodar outro `cargo build` (nested) pode gerar contenção de lock do target; use um diretório de target separado para o ntdll (`--target-dir`) se houver deadlock, ou prefira **localizar** um `ntdll.dll` já compilado por `cargo xtask ntdll` (documente que `cargo xtask ntdll` deve rodar antes do build do kernel no Windows, e a `build.rs` só copia o artefato existente, caindo para vazio se ausente). A segunda opção é mais simples e evita o nested cargo — prefira-a: `xtask` já compila o ntdll, e a `build.rs` só copia `target/x86_64-pc-windows-msvc/release/ntdll.dll` se existir. Ajuste `xtask::image::build`/`ktest` para compilar o ntdll antes do kernel no Windows.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ntdll && cargo xtask ktest` → `ntdll_embedded_parses` passa (no Windows, confirma o export; em outro host, pula). `cargo test -p pe` ok.

- [ ] **Step 5: Commit**

```bash
git add kernel crates/pe xtask
git commit -m "feat(kernel): embute o ntdll.dll (quando disponivel) e export_rva_by_name

Source: docs Microsoft PE Format specification (export directory)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Carregar dois módulos e resolver imports (IAT)

**Files:**
- Modify: `kernel/src/loader.rs` (carregar numa base dada devolvendo info; resolver imports), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`loader.rs`):
  - `pub struct Loaded { pub base: u64, pub entry: u64, pub image: &'static [u8] }` — info de um módulo carregado (ou guarde o necessário para resolução).
  - `pub fn load_image(space: &AddressSpace, image: &[u8], load_base: u64) -> Result<u64, LoadError>` — o `load_pe` atual, retornando a entrada (já existe; renomeie/encapsule se útil).
  - `pub fn resolve_imports(space: &AddressSpace, exe: &[u8], exe_base: u64, ntdll: &[u8], ntdll_base: u64) -> Result<(), LoadError>` — para cada `ImportDll` do `exe`: se o nome (case-insensitive) for `ntdll.dll`, para cada `ImportEntry`:
    - resolve o endereço: por nome → `ntdll_base + export_rva_by_name(name)`; (por ordinal → `LoadError::UnsupportedImport` neste plano, ou um acessor por ordinal se já houver);
    - **valida** que `[exe_base + iat_rva, +8)` cai numa seção **gravável** do `exe` (via `translate_user_writable_in` no espaço, ou via `pe::rva_to_offset` + checagem de característica `WRITE` da seção que contém o `iat_rva`);
    - grava o endereço resolvido no slot da IAT (pela HHDM do frame, não pelo VA de usuário);
    - se a função não existir no `ntdll`, grava o endereço de um stub de "não implementado" (um símbolo no `ntdll` que escreve `@@TUKOS-UNIMPLEMENTED` e termina — ou, mais simples neste plano, falha com `LoadError::UnresolvedImport`, registrando o nome). Escolha a opção mais simples que passe nos testes; documente.
  - `LoadError` ganha `UnsupportedImport`, `UnresolvedImport`, `BadIatSlot`.

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`, adicionar:
```rust
    ("two_modules_no_overlap", two_modules_no_overlap),
    ("import_resolve_rejects_bad_iat_slot", import_resolve_rejects_bad_iat_slot),
    ("import_resolve_unknown_function", import_resolve_unknown_function),
```
e testes que: (a) carregam dois módulos (o `.exe` de teste e o `ntdll` embutido, se presente) em bases distintas e confirmam que suas faixas de VA não se sobrepõem; (b) tentam resolver uma import cujo `iat_rva` cai numa seção não-gravável (código) e esperam `BadIatSlot`; (c) resolvem uma import para uma função inexistente no `ntdll` e esperam o comportamento escolhido (`UnresolvedImport` ou stub). Se o `ntdll` não estiver embutido, esses testes retornam `Ok(())` (pulados). Os helpers de fixture (um `.exe` com import table) ficam em `process.rs`/`loader.rs` de teste.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Implemente `resolve_imports` como acima, com a validação do slot da IAT (o ponto que a revisão do Plano 7 destacou: `iat_rva` é aritmética pura e precisa ser validado). Carregue o `ntdll` com `load_image` numa base distinta da do `.exe` (aplicando relocações — o `load_pe` já faz) e resolva. Mapeie cada módulo com W^X por seção (o `load_pe` já faz).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → os três testes passam (ou pulam sem o `ntdll`). Demais ok.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): carrega dois modulos e resolve imports do .exe contra o ntdll (IAT validada)

Source: docs Microsoft PE Format specification (imports, IAT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Um `.exe` que importa o `ntdll` roda e imprime "TukOS"

**Files:**
- Modify: `kernel/src/process.rs` (um `.exe` de teste com import table + `spawn_exe_with_ntdll`), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`process.rs`): um PE de teste feito pelo TukOS com uma **import table** que importa `ntdll.dll!NtWriteFile` e `ntdll.dll!NtTerminateProcess`; seu `.text` chama essas funções **pela IAT** (`call [rip + disp]` apontando para o slot da IAT) com a convenção do Windows x64 (args em `rcx,rdx,r8,r9` + pilha — o stub do `ntdll` faz `mov r10,rcx` e `syscall`). `pub fn spawn_exe_with_ntdll() -> ()` — cria um `AddressSpace`, carrega o `.exe` e o `ntdll` embutido, resolve os imports, monta TEB/PEB/pilha, e cria a thread de usuário na entrada do `.exe`. Se o `ntdll` não estiver embutido, o spawn não faz nada (e o teste pula).

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("exe_imports_ntdll_and_runs", exe_imports_ntdll_and_runs),` (antes de `two_user_processes_preempt`) e:
```rust
fn exe_imports_ntdll_and_runs() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    if crate::ntdll_image::NTDLL_BYTES.is_empty() {
        return Ok(()); // ntdll not embedded on this host: skip.
    }
    crate::syscall::LAST_WRITE_LEN.store(0, Ordering::SeqCst);
    let before = crate::mem::frames::free_frame_count();
    crate::process::spawn_exe_with_ntdll();
    for _ in 0..12000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_WRITE_LEN.load(Ordering::SeqCst) == 6
            && crate::syscall::last_write_is(b"TukOS\n")
        {
            // and it should have exited and been reaped.
            if crate::mem::frames::free_frame_count() == before {
                return Ok(());
            }
        }
    }
    Err("o .exe que importa ntdll nao rodou/saiu")
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar o `.exe` de teste e o spawn**

Monte o PE de teste com: uma seção `.text` executável cujo código faz `NtWriteFile(CONSOLE_HANDLE, 0,0,0, &iosb, msg, 6, 0, 0)` chamando **pela IAT** (`call qword ptr [rip + disp_para_o_slot_NtWriteFile]`), depois `NtTerminateProcess(0)` pela IAT; uma seção de dados gravável com a `msg` "TukOS\n", o `IO_STATUS_BLOCK` e a pilha (ou use a pilha de usuário já montada por `spawn`); e a **import table** (`.idata`): um descritor para `ntdll.dll` com INT/IAT apontando para as entradas por nome `NtWriteFile`/`NtTerminateProcess`. O `call [rip+disp]` chama o endereço no slot da IAT, que a resolução do kernel preencheu com o endereço do stub no `ntdll` carregado; o stub faz `syscall` ao kernel.

Isto é intrincado (montar uma import table válida + os `call [rip+disp]` com os deslocamentos certos para os slots da IAT). Monte com cuidado e verifique com `-d int`/serial. Reutilize o construtor de PE da crate `pe` (o `build_pe_with_imports` de teste) como referência para a import table, adaptando para um PE executável real. `spawn_exe_with_ntdll` carrega os dois módulos, resolve, e inicia o `.exe`.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `exe_imports_ntdll_and_runs` passa no Windows (serial mostra `TukOS`, frames recuperados); pula em host sem `ntdll`. Demais variantes ok. `cargo xtask run` (no Windows, após `cargo xtask ntdll`) mostra `TukOS` impresso por um `.exe` que chamou `ntdll!NtWriteFile`.

Depuração: se não rodar, confira com `-d int`: o slot da IAT recebeu o endereço certo (`ntdll_base + rva do NtWriteFile`); o `call [rip+disp]` aponta para o slot; o `ntdll` foi mapeado executável; as duas imagens não se sobrepõem; as relocações do `ntdll` foram aplicadas.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): um .exe que importa ntdll!NtWriteFile roda e imprime via syscall

Source: docs Microsoft PE Format specification (imports/IAT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os gates de Global Constraints passam. Marco: um `.exe` feito pelo TukOS importa `ntdll!NtWriteFile`, o chama pela IAT (resolvida pelo kernel), o stub do `ntdll` faz `syscall`, o kernel imprime "TukOS", e o `.exe` termina pelo `ntdll!NtTerminateProcess`.
- Limitações (Plano 9, degrau 3a completo): a resolução de imports é feita **pelo kernel** (não pelo `ntdll` em modo usuário); imports por **ordinal** não são resolvidos; o `.exe` é feito pelo TukOS, não compilado com MSVC; a entrada é a do `.exe` diretamente (não o thunk de inicialização do `ntdll`).
- Plano 9: migrar a resolução de imports para um loader em **modo usuário** no `ntdll` (`LdrInitializeThunk`, percorrendo `PEB->Ldr`); o kernel passa a iniciar a thread no thunk do `ntdll`, que resolve os imports e chama a entrada do `.exe`; e o harness de conformidade que compila um `.exe` de console **real com MSVC** no seu Windows, roda no Windows real para capturar a saída esperada, e compara com o TukOS — o primeiro binário real do Windows a rodar no TukOS (degrau 3a concluído).
