# TukOS Plano 17 — CreateProcess + espera + exit code

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Um programa pode **criar outro processo**, esperá-lo terminar e ler o exit code. `kernel32!CreateProcessA/W` carrega um `.exe` do initrd num novo processo (via o grafo exe→kernel32→ntdll já existente), herdando os handles padrão do pai (stdout→mesma console, stdin→teclado); `WaitForSingleObject` bloqueia até o filho sair; `GetExitCodeProcess` devolve o código. Demonstrável e conformável: um programa pai (feito pelo TukOS/MSVC) cria `hello.exe` do initrd, espera, e imprime o exit code — idêntico ao Windows real. É a gestão de processos do subprojeto 2 e a base do shell (Plano 18).

**Architecture:** Hoje o kernel só spawna o processo inicial embutido. Adiciona-se: um syscall simplificado `NT_CREATE_PROCESS(path_utf16, len, out_process_handle)` que resolve o `.exe` no initrd (fs::lookup), monta um novo `AddressSpace` com o exe+kernel32+ntdll (reusa `spawn_exe_image_with_k32`/o caminho multi-módulo), constrói PEB/Ldr/ProcessParameters **herdando os std handles do pai** (StandardInput=CONSOLE_INPUT, Standard/ErrorOutput=CONSOLE), cria a thread de usuário na entrada (ntdll!LdrInitializeThunk), e devolve um **handle de processo** ao pai. O object manager ganha `Object::Process { exit_status, state }`. `NT_WAIT(handle)` bloqueia (yield) até o processo-alvo terminar; quando um processo sai (`exit_current`/fault contido do Plano 14), o seu `Object::Process` recebe o exit status e vira sinalizado. `NT_QUERY_EXIT_CODE(handle, out)` devolve o status. `kernel32`: `CreateProcessA/W` (preenche `PROCESS_INFORMATION` com o handle+pid), `WaitForSingleObject`, `GetExitCodeProcess`, `CloseHandle` (já). Clean room nível 0: semântica de CreateProcess/WaitForSingleObject/GetExitCodeProcess da documentação pública (MSDN); o syscall e a gestão são do TukOS; sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `crates/ntdll`/`kernel32`, QEMU + OVMF, MSVC no host. Testes headless.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 2: gestão de processos). Sucede o Plano 16, já na `main`.

**Fora deste plano:** o shell interativo (Plano 18); passar argumentos/linha de comando ao filho (cmdline mínima/vazia aqui — o filho não lê argv ainda; documentar); herança de handles além dos 3 std; `CreateProcess` a partir de disco real (é do initrd); job objects, prioridades, múltiplas threads por processo; término forçado de outro processo (`TerminateProcess(outro)`); redirecionamento de stdio (pipes). O pai e o filho compartilham a mesma console (sem isolamento de terminal).

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (MSDN/winternl públicos; sem binário/disassembly da MS). REA só nos nossos artefatos.
- Invariantes mantidas: disciplina de teardown dos Planos 4/12/14 (nunca liberar CR3/stack/fxarea ativos; reaper após switch; self-switch to==from; faults de usuário contidos); escritas na IAT confinadas por-módulo; `copy_from/to_user` validam ponteiros; args ULONG lidos como `u32`; `kernel32` importa só `ntdll`; sem heap do Rust em ntdll/kernel32.
- **Novo cuidado:** criar um processo aloca frames/AddressSpace; o handle de processo no pai NÃO deve manter vivo o AddressSpace do filho após o filho ser reapeado (o `Object::Process` guarda só o exit status, não a imagem); um filho que falha (fault) é contido (Plano 14) e seu exit status fica disponível ao pai. Esperar não pode travar o escalonador (yield; libera locks antes do yield). Sem corrida entre o filho sair+reapear e o pai ler o exit code (o status é gravado no Object::Process ANTES do reap liberar os frames).
- Testes headless; `build.rs`/`xtask`: sem MSVC auto-pula; Linux CI verde.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (kernel ktest + xtask/arch/mm/pe/kbd -D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs MSDN (CreateProcess/WaitForSingleObject/GetExitCodeProcess)`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP (subprojeto 2: gestão de processos).

## Review Focus

1. **Ciclo de vida seguro:** criar→rodar→filho sai/falha→exit status gravado no Object::Process ANTES do reaper liberar os frames do filho→pai lê; sem UAF, sem leak de AddressSpace/frames, sem liberar recurso ativo; um filho que dá fault é contido e reporta um status. Coberto por ktests (criar+esperar+exit code; filho que falha).
2. **Espera não trava o escalonador:** `WaitForSingleObject` faz yield sem segurar lock; outras threads correm; não há deadlock se o filho e o pai forem as únicas threads (o filho roda, sai, o pai acorda). Coberto por ktest.
3. **Herança de handles:** o filho recebe StandardInput=CONSOLE_INPUT e Standard/ErrorOutput=CONSOLE (sua saída vai à mesma console; pode ler o teclado). Coberto pelo roundtrip (o filho hello imprime na console do pai).
4. **Isolamento:** o novo processo tem seu próprio AddressSpace (PML4 próprio, metade alta do kernel clonada); não vaza memória/handles do pai (exceto os std herdados por valor); a IAT do filho é resolvida no espaço dele. Coberto por ktest.
5. **Conformidade:** um pai `/MT` que `CreateProcess` um filho, `WaitForSingleObject`, `GetExitCodeProcess` e imprime o código roda idêntico no Windows (onde o filho é um .exe real ao lado) e no TukOS (filho no initrd). Pula limpo sem MSVC.

---

### Task 1: Survey dos imports de um pai CreateProcess/Wait + triagem

**Files:** Create `conformance/spawn/parent.c` (+ `conformance/spawn/child.c`), `docs/observations/2026-10-14-createprocess-imports-survey.md`

- [ ] **Step 1:** `child.c` = um /MT que imprime "filho ok\n" e `return 42;`. `parent.c` = um /MT que faz `CreateProcessA("child.exe", ...)`, `WaitForSingleObject(pi.hProcess, INFINITE)`, `GetExitCodeProcess(&code)`, `printf("filho saiu com %u\n", code)`, `CloseHandle`, return 0.
- [ ] **Step 2:** `cl /MT /GS- parent.c` e `child.c`; `dumpbin /imports parent.exe` (só o NOSSO binário) → registrar os novos imports (CreateProcessA/W, WaitForSingleObject, GetExitCodeProcess, CloseHandle✓, provavelmente GetStartupInfoW✓). child.exe imports já cobertos (printf/exit).
- [ ] **Step 3:** triar em `docs/observations/...` (já temos / stub / load-bearing) + assinatura MSDN (incl. STARTUPINFO/PROCESS_INFORMATION layout). Definir o split T2 (kernel) vs T3 (kernel32). Nota: `CreateProcessA` vs `W` — o que o fonte usa.
- [ ] **Step 4/5:** sem implementar; commit (`docs(spawn): survey dos imports de CreateProcess/Wait (/MT)`, `Source: blackbox conformance/spawn/`).

---

### Task 2: Kernel — `Object::Process` + `NT_CREATE_PROCESS`/`NT_WAIT`/`NT_QUERY_EXIT_CODE`

**Files:** Modify `kernel/src/object.rs` (Object::Process), `kernel/src/process.rs` (criar processo a partir do initrd reusando o caminho multi-módulo; herdar std handles), `kernel/src/sched.rs` (registrar o exit status no Object::Process ao sair/reapear; estado sinalizado), `kernel/src/syscall.rs` (os 3 syscalls), `crates/ntdll/src/lib.rs` (stubs), `kernel/src/ktest.rs`

**Interfaces:**
- `Object::Process { exit: Option<u32> (None enquanto roda), ... }` compartilhado entre o handle do pai e a thread do filho (ex.: `Arc`/um índice numa tabela de processos do kernel) — quando a thread do filho sai (`exit_current` com status) ou é contida por fault (Plano 14), grava `exit = Some(status)` ANTES do reaper liberar os frames.
- `NT_CREATE_PROCESS(path_utf16, len_bytes, out_process_handle)` (nr livre, ex. 11): fs::lookup do `.exe`; monta o AddressSpace+PEB/Ldr/ProcessParameters (herda CONSOLE/CONSOLE_INPUT) via o caminho de `spawn_exe_image_with_k32`; cria a thread no LdrInitializeThunk; insere um `Object::Process` na HandleTable do pai; devolve o handle. Ausente → STATUS_OBJECT_NAME_NOT_FOUND.
- `NT_WAIT(process_handle)` (nr ex. 12): bloqueia (yield sem lock) até `exit` ser `Some`; devolve 0 (WAIT_OBJECT_0).
- `NT_QUERY_EXIT_CODE(process_handle, out_u32)` (nr ex. 13): escreve `exit.unwrap_or(STILL_ACTIVE=259)`.
- ntdll: stubs.

- [ ] **Step 1: Teste que falha** — ktest `create_wait_exit`: cria um processo a partir de um `.exe` de teste no initrd que sai com um código conhecido; espera; lê o exit code == o esperado; confirma frames recuperados após o reap (sem leak) e que o pai continua. Também `child_fault_reports_status`: um filho que dá fault (Plano 14) → o pai acorda e lê um status de erro, kernel vivo. 
- [ ] **Step 2-4:** ver falhar; implementar; ver passar; sem leak; escalonador vivo.
- [ ] **Step 5: Commit** (`feat(kernel): Object::Process + NT_CREATE_PROCESS/NT_WAIT/NT_QUERY_EXIT_CODE (initrd)`, `Source: docs MSDN`).

---

### Task 3: kernel32 `CreateProcessA/W`/`WaitForSingleObject`/`GetExitCodeProcess`

**Files:** Modify `crates/kernel32/src/lib.rs`, `kernel/src/ktest.rs`

**Interfaces:** `CreateProcessA/W(appName, cmdLine, ...PROCESS_INFORMATION* out)` → resolve o nome do programa (de appName ou do 1º token de cmdLine), chama `NT_CREATE_PROCESS`, preenche `PROCESS_INFORMATION{hProcess, hThread, dwProcessId, dwThreadId}` (hThread/ids podem ser sintéticos), devolve BOOL. `WaitForSingleObject(h, ms)` → `NT_WAIT` (só INFINITE suportado neste plano; outros valores documentar) → WAIT_OBJECT_0. `GetExitCodeProcess(h, out)` → `NT_QUERY_EXIT_CODE`. `CloseHandle` (já) fecha o handle de processo. kernel32 importa só ntdll.

- [ ] **Step 1: Teste que falha** — ktest `program_spawns_child`: um `.exe` de teste (TukOS/MSVC) cria um filho do initrd que imprime algo e sai com código X; o pai espera, lê X, imprime; assere a saída combinada + o código.
- [ ] **Step 2-4:** ver falhar; implementar; ver passar; `dumpbin` kernel32 só ntdll.
- [ ] **Step 5: Commit** (`feat(kernel32): CreateProcess/WaitForSingleObject/GetExitCodeProcess sobre NT_*`, `Source: docs MSDN`).

---

### Task 4: Conformidade — pai cria filho, espera, imprime exit code + ROADMAP

**Files:** Modify `xtask/src/conformance.rs` (caso `spawn`: embute parent.exe + child.exe no initrd; no Windows roda parent.exe com child.exe ao lado), `kernel/src/conformance.rs`/`build.rs`, `ROADMAP.md`

- [ ] **Step 1-4:** caso `spawn`: compila parent.c + child.c `/MT`; no Windows roda parent.exe (com child.exe no mesmo dir), captura stdout+exit; no TukOS embute AMBOS no initrd (child.exe acessível por "child.exe") e roda parent pelo caminho multi-módulo; compara stdout ("filho ok\n" + "filho saiu com 42\n") + exit. Mantém os 6 casos anteriores. Pula limpo sem MSVC. Atualizar `ROADMAP.md` (subprojeto 2: gestão de processos presente; Marco/Em aberto).
- [ ] **Step 5: Commit** (`feat(xtask): conformidade de CreateProcess (pai cria filho, espera, exit code); ROADMAP`, `Source: docs MSDN`).

---

## Notas de fechamento

- Marco: um programa cria outro, espera e lê o exit code — gestão de processos (subprojeto 2) presente; idêntico ao Windows. A base do shell está pronta.
- Residual/futuro: argumentos/linha de comando ao filho; pipes/redirecionamento; TerminateProcess de outro; job objects; CreateProcess de disco real.
- Deferreds remanescentes: P9 #1/#4, P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked; TOCTOU; first-fit O(n)), P13 (não-isolamento entre testes; dead-code sob features conformance), P14 (#GP sem teste; exit code só logado — agora exposto via GetExitCodeProcess), P15 (teclado polling; sem launcher de echo), P16 (arquivo >1MB num read; sem write/dirs).
- **Plano 18 = shell/REPL interativo** (lê o teclado via ReadFile, parseia um comando, `CreateProcess` do programa no initrd, `WaitForSingleObject`, imprime) rodando como o primeiro programa — **o terminal usável** que o usuário perguntou.
