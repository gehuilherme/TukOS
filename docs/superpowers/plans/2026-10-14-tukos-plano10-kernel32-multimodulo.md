# TukOS Plano 10 — Loader multi-módulo + kernel32 mínimo sobre o ntdll

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dar o primeiro passo do degrau 3b: rodar um `.exe` que usa `kernel32.dll` (não só o `ntdll`). O `LdrInitializeThunk` passa a carregar e resolver um **grafo de dependências** (`exe → kernel32 → ntdll`), tratando **forwarder exports**. Introduzimos um `kernel32.dll` **do TukOS** (mínimo) que importa o `ntdll` e exporta `GetStdHandle`, `WriteFile` e `ExitProcess`. Um `.exe` de teste que importa `kernel32!WriteFile`/`GetStdHandle`/`ExitProcess` roda, imprime "TukOS", e sai pela cadeia `kernel32 → ntdll → syscall`. Por fim, o harness de conformidade compila com MSVC um `.exe` freestanding que importa o **nosso** `kernel32` e compara a saída no Windows real e no TukOS.

**Architecture:** O loader de modo usuário (`ntdll!LdrInitializeThunk`, do Plano 9) ganha duas capacidades: (1) resolver imports de **vários** módulos — para cada módulo carregado que tenha import directory, resolve seus imports contra os outros módulos da lista `PEB->Ldr` (achando a DLL exportadora por `BaseDllName`), resolvendo dependências antes dos dependentes; (2) **forwarder exports** — quando o RVA de um export cai dentro do range da export directory, o valor é uma string `"OUTRADLL.Funcao"`; o loader segue o forward até o módulo/função final. O kernel passa a mapear o `.exe` **e todas as suas DLLs de dependência** (`kernel32`, `ntdll`) e a montar `PEB->Ldr` com os três módulos. O `kernel32.dll` é um novo cdylib `x86_64-pc-windows-msvc` `#![no_std]` (como o `ntdll`): exporta `GetStdHandle` (lê `PEB->ProcessParameters`), `WriteFile` (importa e chama `ntdll!NtWriteFile`), `ExitProcess` (chama `ntdll!NtTerminateProcess(NtCurrentProcess=-1, code)`); ele **importa o ntdll** (exercitando o grafo de dependências). A crate `pe` ganha detecção de forwarder (`resolve_export → Rva|Forwarder`).

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `pe`, `ntdll`, novo `kernel32`, `mm`, `arch-x86_64`, QEMU + OVMF, MSVC no host Windows para o harness.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 5/6: camada `kernel32` sobre o `ntdll`; loader multi-módulo). Sucede o Plano 9 (loader de imports em modo usuário + conformidade), já na `main`.

**Fora deste plano (vai para o Plano 11, degrau 3b completo):** CRT completo (`libcmt`/`msvcrt`) e um `hello.exe` real com CRT; `NtAllocateVirtualMemory` + heap de usuário (`HeapAlloc`); TLS callbacks e SEH em modo usuário; a maioria das APIs do `kernel32` (só `GetStdHandle`/`WriteFile`/`ExitProcess` aqui); carregamento de DLLs vindas de disco (os módulos continuam embutidos/montados pelo kernel).

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64. Código dependente de CPU em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft (`ntdll.dll`/`kernel32.dll`, os exports `Nt*` e os nomes Win32 como `WriteFile`/`GetStdHandle`/`ExitProcess` são interface de interoperabilidade, permitidos).
- **Clean room nível 0 apenas:** formato PE (forwarders, imports/exports), layout de `PEB`/`LDR`/`ProcessParameters` da `winternl.h` pública, semântica de `GetStdHandle`/`WriteFile`/`ExitProcess` da documentação pública. **Nenhum** disassembly de binário da Microsoft; o `kernel32` e o `ntdll` são feitos pelo TukOS; o `.exe` de conformidade linka as import libs do **nosso** `kernel32`/`ntdll`, nunca as da Microsoft.
- `kernel32` e `ntdll` **não podem alocar heap** nem ter imports além do declarado (`kernel32` importa só o `ntdll`; o `ntdll` não importa nada). Verificar com `dumpbin`/`pe`.
- `build.rs`/`xtask`: em hosts sem MSVC o `kernel32`/`ntdll` não compilam, os bytes embutidos ficam vazios, e os testes/harness que dependem deles se auto-pulam; o build **nunca** quebra (Linux CI verde).
- Invariante de segurança (do Plano 8/9): toda escrita na IAT em modo usuário fica **confinada à imagem do módulo sendo resolvido**, numa região gravável e **não executável** (`iat_slot_ok`); nenhum PE malformado pode escrever fora da própria imagem.
- Commits declaram a fonte: `Source: docs Microsoft PE Format specification` e/ou `Source: docs winternl.h (PEB/LDR/ProcessParameters public layout)`.
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa.

## Review Focus

1. **Resolução multi-módulo:** o loader resolve os imports de cada módulo (exe e kernel32) contra a DLL correta (achada por `BaseDllName` em `PEB->Ldr`); dependências (`ntdll`) são resolvidas antes dos dependentes; uma import de uma DLL ausente da lista falha de forma limpa (termina com sentinela). Coberto por `exe_uses_kernel32_and_runs` (Task 3) e por um teste de DLL ausente.
2. **Forwarder exports:** um export cujo RVA cai na export directory é uma string `"DLL.Func"`; o loader segue o forward (inclusive por nome e por `#ordinal`) até o alvo; um ciclo/forward inválido termina sem laço infinito. Coberto por teste de forwarder no `pe` (Task 1) e um forwarder real no `kernel32` (Task 3).
3. **Confinamento da IAT (invariante do Plano 8/9) agora por-módulo:** ao resolver o kernel32, a escrita fica confinada à imagem do kernel32; ao resolver o exe, à do exe. A validação usa a `SizeOfImage`/seções do módulo **correto**, não de um fixo. Coberto por `usermode_loader_rejects_bad_iat` (adaptado) + um teste de dir-12 hostil (Plano 9 deferred #2).
4. **`kernel32` sem heap, imports só do ntdll:** `GetStdHandle` lê o `PEB`; `WriteFile`/`ExitProcess` chamam o `ntdll`; nenhuma alocação; só o `ntdll` no import directory. Coberto por verificação com `pe`/`dumpbin` (Task 2).
5. **Conformidade:** um `.exe` MSVC que importa o nosso `kernel32` roda idêntico no Windows real e no TukOS. Pula limpo sem MSVC. Coberto por `cargo xtask conformance` estendido (Task 4).

---

### Task 1: Forwarder exports na crate `pe`

**Files:**
- Modify: `crates/pe/src/lib.rs`, testes de host

**Interfaces:**
- Produces (`pe`):
  - `pub enum Resolved { Rva(u32), Forwarder(&'a str) }` (ou similar) e `pub fn resolve_export_ex(&self, name_or_ord) -> Option<Resolved>`: se o RVA do export cai dentro de `[export_dir_rva, export_dir_rva + export_dir_size)`, lê a string C ASCII nesse RVA e devolve `Forwarder(s)`; senão `Rva(rva)`. Mantenha `export_rva_by_name`/`resolve_export` existentes (devolvem o RVA cru) para compatibilidade, ou adapte os chamadores.
  - helper para separar `"DLL.Func"` / `"DLL.#ordinal"` em (dll, nome|ordinal), sem alocação (devolve sub-slices).

- [ ] **Step 1: Testes que falham**

Em `crates/pe`, testes de host: (a) um PE sintético com um export **forwarder** (RVA apontando p/ uma string `"NTDLL.NtFoo"` dentro da export directory) → `resolve_export_ex` devolve `Forwarder("NTDLL.NtFoo")`; (b) um export normal → `Rva`; (c) o split de `"DLL.Func"` e `"DLL.#42"`; (d) forward malformado (string sem `.`/sem terminador dentro dos limites) → `None`/erro limpo, sem pânico. Reuse os construtores de export sintético.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p pe` → falha de compilação.

- [ ] **Step 3: Implementar**

Implemente a detecção de forwarder e o split, com aritmética checada e sem pânico sobre bytes atacáveis.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p pe` e `cargo clippy -p pe -- -D warnings` → ok. Confirme que `pe` ainda compila `no_std` p/ `x86_64-unknown-none` e `x86_64-pc-windows-msvc`.

- [ ] **Step 5: Commit**

```bash
git add crates/pe
git commit -m "feat(pe): deteccao de forwarder exports (DLL.Func)

Source: docs Microsoft PE Format specification (export directory, forwarders)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: `kernel32.dll` mínimo do TukOS (sobre o ntdll)

**Files:**
- Create: `crates/kernel32/` (Cargo.toml, build.rs análogos ao `crates/ntdll`, src/lib.rs)
- Modify: `rust-toolchain.toml` (já tem o target msvc), `xtask/src/ntdll.rs` ou um novo `xtask/src/kernel32.rs` + `xtask` subcomando/integração para compilá-lo, `kernel/build.rs` + `kernel/src/ntdll_image.rs` (ou um novo módulo) p/ embutir o `kernel32.dll`

**Interfaces:**
- Produces (`crates/kernel32`): cdylib `x86_64-pc-windows-msvc`, `#![no_std]`, `/NODEFAULTLIB /NOENTRY`, sem heap. Exporta:
  - `GetStdHandle(n: i32) -> isize`: lê `PEB->ProcessParameters` (via GS) e devolve `StandardOutput` (n=-11), `StandardInput` (-10) ou `StandardError` (-12). Sem imports.
  - `WriteFile(handle, buf, len, written_out, overlapped) -> i32`: monta o `IO_STATUS_BLOCK` e chama `ntdll!NtWriteFile(handle, 0,0,0, &iosb, buf, len, 0,0)`; grava `*written_out = iosb.Information`; devolve 1/0. **Importa** `NtWriteFile` do `ntdll`.
  - `ExitProcess(code: u32) -> !`: chama `ntdll!NtTerminateProcess((HANDLE)-1, code)`. **Importa** `NtTerminateProcess` do `ntdll`.
  - provê `memcpy`/`memset`/... como o `ntdll` se o linker exigir.
- Produces (kernel): `pub static KERNEL32_BYTES: &[u8]` (embutido como o `NTDLL_BYTES`; vazio sem MSVC). `xtask` compila `ntdll` **e** `kernel32` antes do kernel no Windows.

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, `("kernel32_embedded_parses", ...)`: se `KERNEL32_BYTES` vazio, pula; senão parseia com `pe`, confirma os exports `GetStdHandle`/`WriteFile`/`ExitProcess` e que o **único** import é `ntdll.dll` (`NtWriteFile`, `NtTerminateProcess`).

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação (`KERNEL32_BYTES` não existe).

- [ ] **Step 3: Implementar**

Crie a crate `kernel32` (espelhe `crates/ntdll`: build.rs com `/NOENTRY /NODEFAULTLIB`, `panic=abort` já no workspace). Implemente os três exports. Integre no `xtask` (compilar) e no `kernel/build.rs` (embutir). As declarações dos imports do `ntdll` geram o import directory do `kernel32` — confirme que só o `ntdll` aparece.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ntdll && cargo xtask kernel32 && cargo xtask ktest` (ajuste os subcomandos) → `kernel32_embedded_parses` passa (ou pula). `dumpbin /exports`/`/imports` confirmam exports e o único import `ntdll`.

- [ ] **Step 5: Commit**

```bash
git add crates/kernel32 kernel xtask rust-toolchain.toml
git commit -m "feat(kernel32): kernel32.dll minimo (GetStdHandle/WriteFile/ExitProcess) sobre o ntdll

Source: docs winternl.h (PEB/ProcessParameters public layout)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Loader multi-módulo — um `.exe` usa o `kernel32` e roda

**Files:**
- Modify: `crates/ntdll/src/lib.rs` (`LdrInitializeThunk`: resolver vários módulos + forwarders), `kernel/src/process.rs` (mapear exe+kernel32+ntdll e montar `PEB->Ldr` com 3 módulos), `kernel/src/loader.rs` (se preciso p/ dir-12 por-módulo), `crates/arch-x86_64/src/cpu.rs` (limpar CR0.TS — Plano 9 deferred #3), `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`ntdll` `LdrInitializeThunk`): após achar todos os módulos em `PEB->Ldr`, para CADA módulo com import directory (exe e kernel32), resolve seus imports: para cada import DLL, acha o módulo exportador por `BaseDllName` (case-insensitive) na lista; resolve cada função por nome/ordinal via `pe::resolve_export_ex` sobre a imagem carregada do exportador; se vier `Forwarder("DLL.Func")`, acha `DLL` na lista e resolve `Func` lá (com limite de saltos p/ evitar ciclo); grava o endereço no slot da IAT do módulo importador, validando o slot **contra a imagem desse módulo** (`iat_slot_ok` por-módulo). Ordene a resolução p/ dependências antes dos dependentes (ntdll sem imports; depois kernel32; depois exe) — ou resolva repetidamente até estabilizar. Import de DLL ausente → termina com sentinela.
- Produces (`process.rs`): o kernel mapeia o `.exe`, o `kernel32` e o `ntdll` em bases distintas não sobrepostas (todas relocadas conforme preciso), monta `PEB->Ldr` com os 3 (ordem de load: exe, kernel32, ntdll), popula `ProcessParameters` (StandardOutput=CONSOLE_HANDLE), e inicia no `LdrInitializeThunk`.
- Produces (`cpu.rs`): `enable_sse` limpa `CR0.TS` (`clts`) — Plano 9 deferred #3.

- [ ] **Step 1: Testes que falham**

Em `kernel/src/ktest.rs`:
```rust
    ("exe_uses_kernel32_and_runs", exe_uses_kernel32_and_runs),
    ("loader_rejects_missing_dependency", loader_rejects_missing_dependency),
    ("usermode_loader_rejects_bad_iat_dir12", usermode_loader_rejects_bad_iat_dir12),
```
(a) um `.exe` de teste (feito pelo TukOS) importando `kernel32!GetStdHandle`/`WriteFile`/`ExitProcess` roda via a cadeia kernel32→ntdll, imprime "TukOS\n" (len 6) e é reapeado; (b) um `.exe` importando uma DLL não presente termina com a sentinela esperada, sem corromper; (c) um `.exe` com a IAT numa seção não-gravável, porém com uma **data-dir-12 hostil** cobrindo uma seção de **código**, é rejeitado (fecha o gap do Plano 9 deferred #2). Pula se `KERNEL32_BYTES`/`NTDLL_BYTES` vazios.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ntdll && cargo xtask kernel32 && cargo xtask ktest` → falha.

- [ ] **Step 3: Implementar**

Estenda `LdrInitializeThunk` p/ multi-módulo + forwarders (com limite de saltos). Ajuste `process.rs` p/ 3 módulos. Faça `iat_slot_ok` validar contra a imagem do módulo importador (passe base+size+seções do módulo certo). Adicione um forwarder real no `kernel32` (ex.: um export `RtlExitUserProcess` forwardado p/ `ntdll.NtTerminateProcess`, ou um alias de teste documentado) p/ exercitar o caminho de forward ponta-a-ponta. Limpe `CR0.TS` em `enable_sse`.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ntdll && cargo xtask kernel32 && cargo xtask ktest` → os três testes passam (ou pulam). Demais ok. `cargo xtask run` mostra "TukOS" impresso por um `.exe` que chamou `kernel32!WriteFile` (que chamou `ntdll!NtWriteFile`).

Depuração: `-d int` + serial; confira a ordem de resolução, as bases dos 3 módulos, o forward seguido corretamente, e que a IAT de cada módulo foi preenchida.

- [ ] **Step 5: Commit**

```bash
git add crates/ntdll crates/arch-x86_64 kernel
git commit -m "feat(ntdll): loader multi-modulo com forwarders; um .exe usa o kernel32 e roda

Source: docs Microsoft PE Format specification (imports/forwarders)
Source: docs winternl.h (PEB/LDR public layout)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Conformidade — um `.exe` MSVC que importa o `kernel32` roda no TukOS

**Files:**
- Create: `conformance/hello32/hello32.c`
- Modify: `xtask/src/conformance.rs` (compilar/linkar contra o nosso `kernel32.dll.lib`; rodar o caso kernel32), `kernel/src/conformance.rs`/`process.rs` (embutir e rodar o novo exe com exe+kernel32+ntdll)

**Interfaces:**
- Produces: `conformance/hello32/hello32.c` — console freestanding (`/NODEFAULTLIB`) que declara `GetStdHandle`/`WriteFile`/`ExitProcess` e chama `WriteFile(GetStdHandle(-11), "Hello from MSVC via kernel32\n", len, &n, 0); ExitProcess(0);`. Linka o **nosso** `kernel32.dll.lib` (que por sua vez referencia `ntdll.dll`). `cargo xtask conformance` compila, roda no Windows real (bind no `kernel32.dll`/`ntdll.dll` reais por nome — interop) p/ a referência, roda os mesmos bytes no TukOS (exe+kernel32+ntdll, via `LdrInitializeThunk`), e compara byte-a-byte. Mantém o caso do Plano 9 (só ntdll) e adiciona o caso kernel32.

- [ ] **Step 1: Teste/comando que falha**

Adicione `hello32.c` e o caso kernel32 ao `xtask conformance`; sem a implementação, falha/ausente.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask conformance` → o caso kernel32 falha (não implementado).

- [ ] **Step 3: Implementar**

Compile `hello32.c` linkando `kernel32.dll.lib` + `ntdll.dll.lib`; rode no Windows p/ a referência; embuta e rode no TukOS com os 3 módulos; compare. Reutilize a infra do Plano 9 (descoberta de MSVC, boot headless, captura de serial entre marcadores).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask conformance` (Windows + MSVC) → ambos os casos (ntdll puro e kernel32) passam, saídas idênticas. Sem MSVC/não-Windows: pula limpo. `cargo xtask ktest` e demais gates ok.

- [ ] **Step 5: Commit**

```bash
git add xtask kernel conformance
git commit -m "feat(xtask): conformidade de um .exe MSVC que importa o kernel32 do TukOS

Source: docs Microsoft PE Format specification (imports/IAT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os gates de Global Constraints passam. Marco (degrau 3b, 1º passo): um `.exe` (feito pelo TukOS e também um real do MSVC) importa o `kernel32.dll` do TukOS, que importa o `ntdll.dll` do TukOS; o loader de modo usuário resolve o grafo `exe→kernel32→ntdll` (com forwarders), e a chamada `WriteFile`→`NtWriteFile`→`syscall` imprime a saída, idêntica à do Windows real.
- Fecha os deferred #2 (ktest de dir-12 hostil) e #3 (CR0.TS) do Plano 9. Deferred #1 (re-proteger a IAT read-only pós-bind) e #4 (double-map com SizeOfHeaders forjado) seguem para um passe de endurecimento futuro.
- Plano 11 (degrau 3b completo): `NtAllocateVirtualMemory` + heap de usuário (`HeapAlloc`/`RtlAllocateHeap`), mais Nt*/kernel32 (o suficiente para o CRT mínimo), e um `hello.exe` **compilado com o CRT do MSVC** (`libcmt`) rodando no TukOS e conformado — o primeiro programa C "normal" do Windows no TukOS.
