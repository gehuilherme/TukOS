# TukOS Plano 9 — Loader de imports em modo usuário (ntdll) + conformidade com binário real

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Completar o degrau 3a. A resolução de imports migra do kernel para um **loader em modo usuário no `ntdll`** (`LdrInitializeThunk`): o kernel mapeia o `.exe` + o `ntdll`, constrói a lista `PEB->Ldr` com os dois módulos, e inicia a thread no thunk do `ntdll` (não mais na entrada do `.exe`); o thunk percorre `PEB->Ldr`, acha o `.exe`, resolve seus imports contra os exports do `ntdll` (por nome **e por ordinal**), preenche a IAT em modo usuário, e salta para a entrada do `.exe`. Por fim, um harness de conformidade compila um `.exe` **real com MSVC** (freestanding, importando só o `ntdll` do TukOS), captura a saída esperada no Windows real, roda o mesmo binário no TukOS, e compara — o **primeiro binário compilado por um compilador real da Microsoft a rodar no TukOS**.

**Architecture:** O `ntdll` (cdylib `x86_64-pc-windows-msvc`, hoje só stubs de syscall `Nt*`) ganha um export `LdrInitializeThunk` sem imports próprios que roda em modo usuário. Ele lê o `TEB` via `GS` → `PEB` → `PEB->Ldr` (estruturas que o kernel preenche, offsets da `winternl.h` pública) e usa a crate `pe` (reusada, sem alocação) para percorrer a import directory do `.exe` e a export directory do `ntdll`. Para cada import resolve `ntdll_base + rva_do_export` (por nome via tabela de nomes, por ordinal via `AddressOfFunctions[ordinal - OrdinalBase]`) e grava no slot da IAT (a seção da IAT já é gravável; W^X mantido). Terminada a resolução, salta para `exe_base + AddressOfEntryPoint`. O kernel deixa de resolver imports (`resolve_imports` do Plano 8 vira caminho de teste/legado) e passa a montar `PEB->Ldr` e a iniciar a thread no `LdrInitializeThunk`. O harness de conformidade (`cargo xtask conformance`) compila com `cl.exe`/`link.exe` um `.exe` freestanding (`/NODEFAULTLIB /ENTRY /SUBSYSTEM:CONSOLE`) linkado contra a import lib do nosso `ntdll`, roda no Windows real para a saída de referência, embute o binário no TukOS, roda no QEMU, e compara a serial.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `pe` (reusada no `ntdll`, sem alocação), `ntdll`, `mm`, `arch-x86_64`, QEMU + OVMF, e MSVC (`cl.exe`/`link.exe`) no host Windows para o harness.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 5: loader de imports em modo usuário no `ntdll`, `PEB->Ldr`, conformidade com binário real). Sucede o Plano 8 (resolução de imports no kernel), já na `main`.

**Fora deste plano (degraus seguintes):** `kernel32.dll` / CRT completo (o `.exe` de conformidade é **freestanding** e importa só o `ntdll`, chamando `Nt*` diretamente — CRT/`kernel32` ficam para um degrau posterior); forwarders de export entre DLLs (o `ntdll` não encadeia); TLS callbacks; `.pdata`/SEH em modo usuário; carregamento de DLLs além do `ntdll`.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64. Código dependente de CPU em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft (o nome `ntdll.dll`, o export `LdrInitializeThunk` e os exports `Nt*` são interface de interoperabilidade, permitidos).
- **Clean room nível 0 apenas:** formato PE, layout de `PEB`/`PEB_LDR_DATA`/`LDR_DATA_TABLE_ENTRY`/`TEB` e offsets vêm da `winternl.h`/`ntpsapi.h` **públicas** e da especificação PE pública; **nenhum** disassembly do `ntdll` real nem binário da Microsoft. A **import lib** linkada pelo `.exe` de conformidade é a gerada pelo **nosso** `ntdll` (não a `ntdll.lib` da Microsoft).
- O `ntdll` **não pode ter imports próprios** e **não pode alocar heap** (não há allocator em modo usuário ainda) — a crate `pe` deve oferecer acessores sem alocação (iteração por índice sobre a imagem mapeada); se hoje aloca, adicionar métodos no-alloc.
- `build.rs`/`xtask`: em hosts sem MSVC (Linux CI) o `ntdll` não compila, o `NTDLL_BYTES` fica vazio, e os testes que dependem dele se auto-pulam; o build **nunca** quebra. O harness de conformidade só roda no Windows com MSVC (pula/avisa fora disso).
- Commits declaram a fonte: `Source: docs Microsoft PE Format specification` e/ou `Source: docs winternl.h (PEB/TEB/LDR public layout)`.
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **A resolução em modo usuário grava a IAT sem corromper nada:** o `ntdll` valida que cada slot da IAT está dentro de uma seção gravável da imagem do `.exe` antes de gravar (mesma invariante do Plano 8, agora em modo usuário); um `.exe` malformado só pode corromper as próprias páginas graváveis, nunca o kernel nem o `ntdll`. Coberto por um teste com slot inválido (Task 3).
2. **Resolução por ordinal correta:** `AddressOfFunctions[ordinal - OrdinalBase]`; ordinal fora da faixa é tratado sem pânico. Coberto por teste de ordinal no `pe` (Task 1) e no roundtrip (Task 3).
3. **`PEB->Ldr` bem-formado e legível em modo usuário:** a lista encadeada aponta para os dois módulos com `DllBase`/`EntryPoint`/`BaseDllName` corretos; o `ntdll` distingue o `.exe` (cabeça da lista / flag de imagem) do próprio `ntdll` (por `BaseDllName`). Offsets conferidos contra a `winternl.h` pública. Coberto por `peb_ldr_list_well_formed` (Task 2).
4. **O thunk sem imports e sem alocação:** `LdrInitializeThunk` não importa nada, não aloca, e não depende de estado não inicializado; salta para a entrada do `.exe` só após preencher a IAT. Coberto pelo roundtrip (Task 3).
5. **Conformidade:** o binário MSVC freestanding roda idêntico no Windows real e no TukOS (mesma saída). O harness pula limpo sem MSVC. A saída de referência é capturada do Windows real, não hardcoded. Coberto por `cargo xtask conformance` (Task 4).

---

### Task 1: Resolução de export por ordinal (e acessores sem alocação) na crate `pe`

**Files:**
- Modify: `crates/pe/src/lib.rs`, `crates/pe/tests/` (ou testes inline)

**Interfaces:**
- Produces (`pe`):
  - `pub fn export_rva_by_ordinal(&self, ordinal: u16) -> Option<u32>` — resolve `AddressOfFunctions[ordinal - OrdinalBase]`, devolvendo `None` se fora da faixa ou se o RVA for 0.
  - `pub fn resolve_export(&self, entry: &ImportEntry) -> Option<u32>` — por `ImportKind::Name{name,..}` usa `export_rva_by_name`; por `ImportKind::Ordinal(n)` usa `export_rva_by_ordinal(n)`.
  - Acessores **sem alocação** para a import directory: um iterador/índice sobre `ImportDll`/`ImportEntry` que não construa `Vec` (p.ex. `import_dll_count()`, `import_dll_at(i)`, `import_entry_count(dll)`, `import_entry_at(dll, j)`), para o `ntdll` (sem heap) percorrer os imports. Se `imports()` hoje aloca, mantenha-o (kernel/host) e adicione o caminho no-alloc. Documente qual caminho o `ntdll` usa.

- [ ] **Step 1: Testes que falham**

Em `crates/pe`, testes de host: (a) `export_rva_by_ordinal` resolve o ordinal certo num PE sintético com export por ordinal e devolve `None` fora da faixa / RVA 0; (b) o iterador no-alloc de imports produz os mesmos `(dll, entry)` que `imports()` num PE sintético com import table. Reuse os construtores de PE sintético já existentes nos testes da crate.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p pe` → falha de compilação (métodos não existem).

- [ ] **Step 3: Implementar**

Implemente `export_rva_by_ordinal` (lendo `OrdinalBase` e `AddressOfFunctions`), `resolve_export`, e os acessores no-alloc, todos com aritmética checada e sem pânico sobre bytes atacáveis (mesmo padrão defensivo do resto da crate).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p pe` e `cargo clippy -p pe -- -D warnings` → ok.

- [ ] **Step 5: Commit**

```bash
git add crates/pe
git commit -m "feat(pe): resolucao de export por ordinal e acessores de import sem alocacao

Source: docs Microsoft PE Format specification (export directory, ordinals)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: O kernel constrói `PEB->Ldr` e inicia a thread no thunk do `ntdll`

**Files:**
- Modify: `kernel/src/process.rs` (montar `PEB_LDR_DATA` + `LDR_DATA_TABLE_ENTRY` para `.exe` e `ntdll`, apontar `PEB->Ldr`, iniciar na entrada do `ntdll`), `kernel/src/loader.rs` (expor base/entry/size dos módulos carregados), `kernel/src/teb.rs` (campos do `PEB`/`Ldr` se necessário), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`process.rs`):
  - montagem da lista de módulos: numa página gravável de usuário, um `PEB_LDR_DATA` (com `InLoadOrderModuleList` cabeça) e dois `LDR_DATA_TABLE_ENTRY` (campos `DllBase`, `EntryPoint = base + AddressOfEntryPoint`, `SizeOfImage`, `BaseDllName` como `UNICODE_STRING` UTF-16 — `"<exe>"` e `"ntdll.dll"`), encadeados em load order com o `.exe` primeiro; `PEB->Ldr` aponta para o `PEB_LDR_DATA`. Offsets públicos (`winternl.h`): `PEB.Ldr` @0x18; `PEB_LDR_DATA.InLoadOrderModuleList` @0x10; `LDR_DATA_TABLE_ENTRY`: `InLoadOrderLinks`@0x00, `DllBase`@0x30, `EntryPoint`@0x38, `SizeOfImage`@0x40, `FullDllName`@0x48, `BaseDllName`@0x58.
  - `spawn_exe_with_ntdll` passa a iniciar a thread de usuário em `ntdll_base + rva(LdrInitializeThunk)` (export resolvido via `pe::export_rva_by_name`), **não** mais em `resolve_imports` + entrada do `.exe`. A entrada do `.exe` fica acessível ao thunk via `PEB->Ldr` (primeiro módulo). `resolve_imports` (Plano 8) permanece como caminho de teste de kernel, não no caminho de produção.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, `("peb_ldr_list_well_formed", peb_ldr_list_well_formed),`: monta o espaço de um processo (`.exe` + `ntdll`), constrói `PEB->Ldr`, e — lendo pela HHDM/`copy_from_user` — verifica: `PEB->Ldr != 0`; a lista tem dois módulos; o primeiro tem `DllBase == exe_base` e `EntryPoint == exe_base + entry`; existe um módulo com `BaseDllName == "ntdll.dll"` e `DllBase == ntdll_base`. Pula se `NTDLL_BYTES` vazio.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Defina as structs (layout C, offsets públicos) e a montagem da lista numa página de usuário gravável; aponte `PEB->Ldr`; ajuste `spawn_exe_with_ntdll` para iniciar no `LdrInitializeThunk`. Mantenha W^X e a colocação de pilha/TEB/PEB acima da imagem.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `peb_ldr_list_well_formed` passa (ou pula). Demais ok. (Neste ponto `exe_imports_ntdll_and_runs` pode falhar temporariamente — será religado na Task 3, quando o thunk existir; se necessário marque-o para a Task 3 ou mantenha o caminho antigo atrás de um flag até a Task 3.)

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): monta PEB->Ldr com .exe+ntdll e inicia a thread no LdrInitializeThunk

Source: docs winternl.h (PEB/PEB_LDR_DATA/LDR_DATA_TABLE_ENTRY public layout)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: `LdrInitializeThunk` — o loader de imports em modo usuário no `ntdll`

**Files:**
- Modify: `crates/ntdll/src/lib.rs` (novo export `LdrInitializeThunk`), `crates/ntdll/Cargo.toml` (dependência da crate `pe`, sem alocação), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`ntdll`): `#[unsafe(no_mangle)] pub extern "system" fn LdrInitializeThunk() -> !` — roda em modo usuário, sem imports próprios e sem heap:
  1. lê o `TEB` via `GS` (`gs:[0x30]` = `NtTib.Self`), `TEB->PEB` (`gs:[0x60]`), `PEB->Ldr` (@0x18), e percorre `InLoadOrderModuleList`;
  2. identifica o módulo imagem (`.exe`, primeiro da lista) e o `ntdll` (por `BaseDllName == "ntdll.dll"`), lendo `DllBase`/`EntryPoint`;
  3. usa a crate `pe` (acessores no-alloc da Task 1) sobre a imagem do `.exe` para percorrer os imports de `ntdll.dll`; para cada entrada resolve `ntdll_base + resolve_export(entry)` (nome ou ordinal) e grava no slot da IAT (`exe_base + iat_rva`), **validando** que o slot cai numa região gravável da imagem antes de gravar (bounds-check em modo usuário; não há `copy_to_user`, mas confira os limites da imagem e das seções via `pe`);
  4. salta para `exe_base + AddressOfEntryPoint` (a entrada do `.exe`), sem retornar.
  - `crates/ntdll` passa a depender de `pe` (verifique que `pe` compila para `x86_64-pc-windows-msvc`, `no_std`, sem `alloc`).

- [ ] **Step 1: Teste que falha**

Religue/retome `exe_imports_ntdll_and_runs` (Plano 8) agora pelo caminho de modo usuário: o `.exe` de teste é iniciado no `LdrInitializeThunk`, que resolve os imports e salta para a entrada; o teste continua exigindo `LAST_WRITE_LEN==6`, `last_write_is("TukOS\n")`, e frames recuperados. Adicione `("usermode_loader_rejects_bad_iat", ...)`: um `.exe` cujo descriptor aponta um slot de IAT numa seção de código — o thunk deve recusar sem corromper (p.ex. terminar o processo via `NtTerminateProcess` com código de erro, detectado pelo teste). Pula sem `ntdll`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ntdll && cargo xtask ktest` → `exe_imports_ntdll_and_runs` falha (thunk ainda não resolve) e/ou falha de compilação.

- [ ] **Step 3: Implementar**

Implemente `LdrInitializeThunk` como acima. Cuidado: o thunk é `extern "system"` e nunca retorna; não use `panic!` (sem runtime) — em erro, chame `NtTerminateProcess` com um código distinto. Mantenha o `ntdll` sem imports (verifique com `dumpbin`/a crate `pe` que o `ntdll` não ganhou import directory). A crate `pe` deve ser usada só pelos acessores no-alloc.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ntdll && cargo xtask ktest` → `exe_imports_ntdll_and_runs` passa pelo caminho de modo usuário; `usermode_loader_rejects_bad_iat` passa; demais ok. `cargo xtask run` mostra "TukOS" (resolução feita pelo `ntdll`, não pelo kernel).

Depuração: `-d int` + serial; confira que o thunk leu `PEB->Ldr` certo, achou as bases, gravou a IAT, e saltou para a entrada do `.exe`; que o `ntdll` não tem imports.

- [ ] **Step 5: Commit**

```bash
git add crates/ntdll kernel
git commit -m "feat(ntdll): LdrInitializeThunk resolve imports em modo usuario e salta para o .exe

Source: docs winternl.h (PEB/TEB/LDR public layout)
Source: docs Microsoft PE Format specification (imports/IAT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Harness de conformidade — um `.exe` real do MSVC roda no TukOS

**Files:**
- Create: `xtask/src/conformance.rs`, um diretório de fixtures (`conformance/hello/`) com o fonte C do `.exe` freestanding
- Modify: `xtask/src/main.rs` (subcomando `conformance`), `kernel/src/process.rs`/`loader.rs` (carregar um `.exe` embutido arbitrário, não só o sintético), `kernel/src/ktest.rs` ou um modo de boot dedicado

**Interfaces:**
- Produces (`xtask conformance`): `cargo xtask conformance` (só Windows + MSVC; fora disso avisa e pula) que:
  1. compila `conformance/hello/hello.c` — um console `.exe` **freestanding** (`cl /GS- /c` + `link /NODEFAULTLIB /SUBSYSTEM:CONSOLE /ENTRY:tukos_main hello.obj <nossa_ntdll.lib>`), que chama `NtWriteFile(stdout, ..., "...", ...)` e `NtTerminateProcess(0)` diretamente (protótipos `Nt*` declarados no próprio fonte; importa só o nosso `ntdll`);
  2. roda o `.exe` no Windows real para capturar a saída de referência (stdout);
  3. embute o `.exe` numa imagem do TukOS, roda no QEMU (headless), e captura a saída na serial;
  4. compara a saída do TukOS com a de referência e falha se diferirem.
  - A **import lib** vem do nosso `ntdll` (o `build.rs` do `ntdll` deve emitir `ntdll.lib` ao gerar a DLL; exponha o caminho ao `xtask`). O `.exe` NÃO linka a `ntdll.lib` da Microsoft.

- [ ] **Step 1: Teste/comando que falha**

Adicione o subcomando `conformance` ao `xtask` e um fonte `hello.c` mínimo; sem a implementação, `cargo xtask conformance` falha (comando/arquivo ausente).

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask conformance` → erro (não implementado).

- [ ] **Step 3: Implementar**

Implemente a compilação com MSVC (localize `cl.exe`/`link.exe` via `vcvars`/`vswhere` ou assuma o developer prompt; se não achar, avise e pule limpo), a execução de referência no Windows, a execução no TukOS (reutilize o caminho de boot que carrega um `.exe` embutido + `ntdll` e inicia no `LdrInitializeThunk` da Task 3), e a comparação. O `.exe` freestanding deve usar a convenção que o nosso `ntdll` espera.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask conformance` (no Windows com MSVC) → compila o `.exe`, roda nos dois, as saídas batem, PASS. Em host sem MSVC: avisa e pula (não falha o CI). `cargo xtask ktest` e os demais gates seguem passando.

- [ ] **Step 5: Commit**

```bash
git add xtask kernel conformance
git commit -m "feat(xtask): harness de conformidade roda um .exe real do MSVC no TukOS e compara a saida

Source: docs Microsoft PE Format specification (imports/IAT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os gates de Global Constraints passam. Marco (degrau 3a COMPLETO): um `.exe` **compilado pelo MSVC** importa o `ntdll` do TukOS, o kernel o carrega com o `ntdll` e monta `PEB->Ldr`, inicia a thread no `LdrInitializeThunk` do `ntdll`, que resolve os imports em **modo usuário** (nome e ordinal) e salta para a entrada do `.exe`; a saída no TukOS bate com a do Windows real. O primeiro binário real da Microsoft rodando no TukOS.
- Carry do Plano 8 (resolver no Plano 9 ou depois): (1) fixture `ntdll` sintético in-repo para os testes de import não pularem no CI Linux; (2) guard explícito de faixas disjuntas no caminho de carga (agora dividido entre kernel e `ntdll`).
- Limitações (degraus seguintes): sem `kernel32.dll`/CRT (o `.exe` é freestanding, chama `Nt*` direto); sem forwarders entre DLLs; sem TLS callbacks nem SEH em modo usuário; só o `ntdll` é carregado como dependência. Degrau 3b: object/IO reais, mais syscalls `Nt*`, `kernel32` sobre o `ntdll`, e um `hello.exe` com CRT.
