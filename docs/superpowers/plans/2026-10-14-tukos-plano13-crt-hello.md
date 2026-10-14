# TukOS Plano 13 — Rodar um hello.exe compilado com o CRT do MSVC

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) ou superpowers:executing-plans. Steps usam checkbox (`- [ ]`).

**Goal:** Fechar o degrau 3b: rodar no TukOS um `hello.exe` **comum**, compilado com o **CRT estático do MSVC** (`/MT`, `libcmt`), que faça `printf`/`puts`, chame `main()` de verdade e saia pelo caminho do CRT — e conferir a saída byte-a-byte com o Windows real pelo harness de conformidade. Diferente dos planos anteriores (binários *freestanding* que importavam só o nosso `ntdll`/`kernel32`), aqui o CRT real chama uma superfície grande de `kernel32`; o plano **mapeia essa superfície empiricamente** e implementa o subconjunto necessário, de forma limpa.

**Architecture:** O `/MT` linka o CRT estaticamente (`libcmt`+`libvcruntime`+`libucrt`), então os únicos imports de DLL do `hello.exe` são para o `kernel32.dll` (e, se houver, `ntdll.dll`). O `mainCRTStartup` inicializa o CRT (cookie de segurança, `_initterm`, argv/env, heap) e chama `main()`; `printf`/`puts` descem até `WriteFile(GetStdHandle(STD_OUTPUT_HANDLE), ...)`. Nós **surveyamos** os imports com `dumpbin /imports` sobre o **nosso** `hello.exe` (lê o ABI público do binário que NÓS compilamos — não é disassembly) e implementamos os `kernel32`/`ntdll` que faltam, cada um a partir da documentação pública (winternl.h/MSDN). Muitos são stubs benignos (retornam um valor plausível e nunca são realmente exercitados por um `hello` simples); alguns são load-bearing (VirtualAlloc→`NtAllocateVirtualMemory`, GetStdHandle/WriteFile/GetFileType, heap, TLS). O objetivo é alcançar `main()` e imprimir; partes não exercitadas podem ser stub.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `pe`, `ntdll`, `kernel32`, `mm`, `arch-x86_64`, QEMU + OVMF, MSVC (`cl`/`link`/`dumpbin`) no host Windows.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (degrau 3b: CRT sobre o `kernel32`). Sucede o Plano 12 (fix do scheduler), já na `main`.

**CLEAN ROOM — restrição dura:** NÃO desmontar/descompilar NENHUM binário da Microsoft (`libcmt`, `ucrt`, `vcruntime`, `ntdll`, `kernel32`). Proibido usar as ferramentas REA/Frida/objdump/cutter/ida sobre binários da Microsoft. A ÚNICA inspeção permitida é `dumpbin /imports`/`/dependents` sobre o `hello.exe` que **nós** compilamos — isso lê a tabela de imports (nomes de API públicos), não o código da Microsoft. Toda implementação vem de documentação pública (winternl.h/MSDN) e do comportamento observável (black-box) do próprio programa. Se faltar entender um comportamento interno do CRT, isso vai para o repo privado `tukos-specs` (lado sujo), NÃO para este trabalho.

**Fora deste plano (degraus seguintes):** CRT dinâmico (`/MD`, ucrtbase.dll/vcruntime140.dll); SEH/unwind real em modo usuário (aqui, se o `__try/__except` do startup nunca dispara num `hello` limpo, bastam stubs não-exercitados); multithread/TLS real além do mínimo; `user32`/`gdi32`/janelas (subprojeto 4 cont.); I/O manager e registro (resto do subprojeto 2). Se o CRT completo não couber num plano, entrega-se o **marco honesto mais avançado** + a lista de imports remanescentes para o Plano 14.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64; código de CPU em `crates/arch-x86_64`.
- Nome do projeto TukOS; `kernel32.dll`/`ntdll.dll` e os nomes Win32/CRT são interoperabilidade permitida.
- Clean room nível 0 (ver restrição dura acima). `kernel32`/`ntdll` sem heap do Rust; sem imports além do declarado.
- Invariantes de segurança dos planos 8–11 mantidas (confinamento da IAT por-módulo; `NtAllocateVirtualMemory` só metade baixa USER|RW|NX, validado; exit status de a1).
- `build.rs`/`xtask`: sem MSVC, tudo que depende dele se auto-pula; build nunca quebra (Linux CI verde). O harness de conformidade só roda no Windows com MSVC.
- `cargo fmt --all -- --check`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` limpos ao fim de cada tarefa.
- Commits com `Source: docs <winternl.h/MSDN ...>` ou `Source: blackbox conformance/...` conforme o caso, e `Co-Authored-By:` do modelo.
- Código/comentários em inglês; docs/mensagens em português.
- Ao fim do plano: atualizar `ROADMAP.md` (status do subprojeto 3/4 + "Marco atual").

## Review Focus

1. **Survey correto e limpo:** a lista de imports vem de `dumpbin /imports` do NOSSO `hello.exe`; nenhuma inspeção de binário da Microsoft. A triagem (stub benigno vs load-bearing vs hard) é fiel.
2. **Stubs benignos não são exercitados de verdade:** um stub que retorna um valor plausível só é aceitável se o `hello` simples nunca depende do efeito real; se for exercitado, vira implementação real. Verificar com `-d int`/serial que o caminho até `main()` não bate num stub crítico.
3. **Caminhos load-bearing corretos:** `VirtualAlloc`/`VirtualQuery` sobre `NtAllocateVirtualMemory` (validado, USER|RW|NX); `GetStdHandle`/`WriteFile`/`GetFileType`/`WriteConsoleW` levam o `printf` à console; heap do CRT funciona; argv/env mínimos. Sem UB/overflow; `kernel32` segue importando só `ntdll`.
4. **Conformidade real:** o `hello.exe` `/MT` roda idêntico no Windows e no TukOS (stdout + exit code). Pula limpo sem MSVC.
5. **Estabilidade:** sem regressão nos ktests; o fix do scheduler (Plano 12) segura; conformance estável em N execuções.

---

### Task 1: Survey + triagem dos imports do CRT (sem implementar)

**Files:**
- Create: `conformance/crt/hello.c` (um `#include <stdio.h>\nint main(void){ printf("Hello, TukOS!\\n"); return 0; }`), `docs/observations/2026-10-14-crt-imports-survey.md`

**Objetivo:** descobrir a superfície EXATA, de forma limpa, antes de implementar.

- [ ] **Step 1:** compilar `hello.c` com `cl /MT /GS- hello.c` (CRT estático) linkando normalmente (deixe o `mainCRTStartup` padrão). Gere `hello.exe`. (Se `/GS-` não remover o cookie, mantenha o cookie e trate `__security_init_cookie` no survey.)
- [ ] **Step 2:** rodar `dumpbin /imports hello.exe` (e `/dependents`) e registrar TODAS as funções importadas por DLL (deve ser essencialmente `kernel32.dll`, talvez `ntdll.dll`). ESTA é a única inspeção — não desmontar nada da Microsoft.
- [ ] **Step 3:** triar cada import em `docs/observations/2026-10-14-crt-imports-survey.md`: (a) **já temos** (GetStdHandle/WriteFile/ExitProcess/Get/HeapAlloc...); (b) **stub benigno** (provável não-exercitado num hello: IsDebuggerPresent→0, GetCurrentThreadId→1, SetUnhandledExceptionFilter→0, InitializeSListHead→noop, GetStartupInfoW→zera a struct, Rtl*Unwind/RtlLookupFunctionEntry→nunca chamado sem exceção, etc.); (c) **load-bearing** (VirtualAlloc/VirtualQuery, GetStdHandle/WriteFile/GetFileType/GetConsoleMode/WriteConsoleW, GetModuleHandleW/GetModuleFileNameW, GetCommandLineA/W, heap, TLS: TlsAlloc/TlsGetValue/TlsSetValue, EncodePointer/DecodePointer, GetSystemTimeAsFileTime, QueryPerformanceCounter, GetProcAddress); (d) **hard** (SEH/unwind, se de fato exercitado). Anotar a assinatura pública (MSDN) de cada um e o comportamento mínimo.
- [ ] **Step 4:** definir o plano de implementação (quais vão em Task 2 vs 3) a partir da triagem. Nenhuma implementação ainda.
- [ ] **Step 5: Commit**

```bash
git add conformance/crt docs/observations
git commit -m "docs(crt): survey dos imports de um hello.exe /MT e triagem (stub/load-bearing/hard)

Source: blackbox conformance/crt/hello.c (dumpbin /imports do nosso binario)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Batch de startup — VirtualAlloc + queries + TLS/pointer-encoding

**Files:** Modify: `crates/kernel32/src/lib.rs`, `crates/ntdll/src/lib.rs` (se precisar de novos `Nt*`), `kernel/src/syscall.rs`/`vmem.rs` (se `VirtualAlloc` exigir), `kernel/src/ktest.rs`

**Interfaces (conforme o survey da Task 1 — ajustar):**
- `VirtualAlloc`/`VirtualFree`/`VirtualQuery` no `kernel32` sobre `NtAllocateVirtualMemory`/`NtFreeVirtualMemory` (MEM_COMMIT|RESERVE, PAGE_READWRITE → USER|RW|NX; VirtualQuery devolve o mínimo que o CRT checa).
- Stubs de query de startup: `GetSystemTimeAsFileTime`, `GetCurrentProcessId`, `GetCurrentThreadId`, `QueryPerformanceCounter`/`QueryPerformanceFrequency`, `GetTickCount64`, `IsDebuggerPresent`, `InitializeSListHead`, `SetUnhandledExceptionFilter`, `GetStartupInfoW`, `GetModuleHandleW`/`GetModuleHandleExW`, `GetModuleFileNameW/A`, `GetCurrentProcess`, `TerminateProcess`, `GetProcAddress` (resolve nos módulos de `PEB->Ldr`), `EncodePointer`/`DecodePointer` (xor com um cookie fixo), TLS mínimo (`TlsAlloc`/`TlsGetValue`/`TlsSetValue`/`TlsFree` sobre um slot no TEB/estado por-thread). Cada um com a assinatura pública e o comportamento mínimo documentado.
- `kernel32` continua importando só `ntdll`.

- [ ] **Step 1: Teste que falha** — um ktest que roda um `.exe` de teste exercitando VirtualAlloc + um par de queries (ou, se já der, o próprio `hello.exe` embutido chega mais longe no startup). 
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** o batch (stubs benignos + os load-bearing VirtualAlloc/TLS). `-d int` p/ ver o CRT avançar.
- [ ] **Step 4: Rodar e ver passar** — ktest verde; `dumpbin` confirma `kernel32`→só `ntdll`.
- [ ] **Step 5: Commit** (`feat(kernel32): VirtualAlloc + stubs de startup do CRT ...`, `Source: docs MSDN/winternl.h ...`).

---

### Task 3: Stdio do CRT + chegar ao `main()` e sair

**Files:** Modify: `crates/kernel32/src/lib.rs` (stdio), `kernel/src/*` se preciso, `kernel/src/ktest.rs`

**Interfaces:**
- Caminho de saída do `printf`: `GetStdHandle(STD_OUTPUT_HANDLE)` (temos), `GetFileType` (→ FILE_TYPE_CHAR p/ console), `GetConsoleMode`/`GetConsoleOutputCP` (stubs plausíveis), `WriteConsoleW` e/ou `WriteFile` (temos) — o CRT escolhe um; implemente o que o survey indicar que o `printf` usa. `WideCharToMultiByte`/`MultiByteToWideChar` mínimos se o caminho W for usado.
- Garantir que `mainCRTStartup` chega a `main()`, `printf` imprime, e o retorno de `main` desce por `exit()`→`ExitProcess(código)` (exit status já correto desde o Plano 11).
- Se o wrapper SEH do startup (`__scrt_common_main_seh`) exigir `RtlCaptureContext`/`RtlLookupFunctionEntry`/`RtlVirtualUnwind`/`SetUnhandledExceptionFilter`, forneça stubs; eles só são acionados se uma exceção propagar — num `hello` limpo não propaga, então stubs não-exercitados bastam (documente e confirme com `-d int`).

- [ ] **Step 1: Teste que falha** — ktest `crt_hello_runs`: embute o `hello.exe` `/MT` e roda pelo caminho multi-módulo; assere que imprime "Hello, TukOS!\n" e sai com código 0 (reapeado). Pula sem MSVC.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** o stdio e o que faltar p/ alcançar/retornar de `main()`. Depurar com `-d int`/serial onde o CRT travar (anotar o import que faltou e implementar).
- [ ] **Step 4: Rodar e ver passar** — `crt_hello_runs` passa; demais ktests ok.
- [ ] **Step 5: Commit** (`feat(kernel32): stdio do CRT; um hello.exe /MT chega ao main e imprime`, `Source: docs MSDN ...`).

---

### Task 4: Conformidade do `hello.exe` /MT (Windows vs TukOS) + ROADMAP

**Files:** Modify: `xtask/src/conformance.rs` (caso `crt`), `kernel/src/conformance.rs`/`build.rs` (embutir+rodar), `ROADMAP.md`

- [ ] **Step 1/2:** adicionar o caso `crt` ao `cargo xtask conformance` (compila `conformance/crt/hello.c` com `/MT`, roda no Windows real p/ referência, roda no TukOS, compara stdout + exit code); sem a implementação, falha.
- [ ] **Step 3:** implementar o caso e a feature de boot do kernel que roda o `hello.exe` `/MT`; manter os casos ntdll/kernel32/heap.
- [ ] **Step 4:** `cargo xtask conformance` → os 4 casos passam (ntdll, kernel32, heap, crt), stdout + exit code idênticos; estável em N execuções; pula limpo sem MSVC. `cargo xtask ktest` + gates ok. **Atualizar `ROADMAP.md`** (subprojeto 3 → ✅ ".exe de console com CRT roda"; ajustar "Marco atual").
- [ ] **Step 5: Commit** (`feat(xtask): conformidade de um hello.exe /MT (CRT) no TukOS`, `Source: docs MSDN`; incluir a atualização do ROADMAP).

---

## Notas de fechamento

- Marco (degrau 3b completo): um `hello.exe` **comum compilado com o CRT do MSVC** roda no TukOS, imprime pelo `printf` e sai pelo CRT, idêntico ao Windows real — o primeiro programa C "normal" do Windows no TukOS.
- **ESCAPE HATCH honesto:** se o CRT completo não couber, entregar o ponto mais avançado real (ex.: `mainCRTStartup` chega a `main()` mas um import X ainda falta) + a lista exata de imports remanescentes em `docs/observations/`, e seguir no Plano 14. NÃO falsificar/hardcodar saída; NÃO desmontar binário da Microsoft para "descobrir" comportamento.
- Deferreds remanescentes: P9 #1/#4, P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked self-only; TOCTOU futura; first-fit O(n)).
- Depois: subprojeto 4 (user32/gdi32/janela — rumo ao "programa com janela abre") e resto do subprojeto 2 (I/O manager, registro).
