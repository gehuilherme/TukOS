# TukOS Plano 15 — Entrada de teclado + console interativo (passo 1 do terminal)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development (ou executing-plans). Steps usam checkbox (`- [ ]`).

**Goal:** Dar ao TukOS **entrada de teclado**. Hoje só há saída (`WriteFile`→console). Este plano lê o teclado PS/2, decodifica scancodes em caracteres, mantém um buffer de entrada de console com eco e edição de linha mínima, e expõe isso por `NtReadFile` no handle de entrada padrão (+ `kernel32!ReadFile`/`ReadConsoleA` e `GetStdHandle(STD_INPUT)`). Demonstração: no `cargo xtask run` interativo, teclas digitadas **aparecem na tela** (eco), e um pequeno programa (feito pelo TukOS e/ou MSVC) lê uma linha e a imprime de volta — o primeiro passo rumo a um terminal usável.

**Architecture:** O controlador i8042 (PS/2) tem a tecla no port de dados 0x60; o status no 0x64. Primeiro passo pragmático: **polling** do i8042 a cada tick do timer (drena o output buffer de 1 byte) — evita o IOAPIC por ora (IRQ1 dedicada fica como refinamento futuro). Os scancodes (set 1) viram caracteres por uma tabela (com Shift, Enter, Backspace) em **lógica pura testável no host** (uma crate/módulo). Os chars entram num **ring buffer de console** protegido por `IrqMutex`. O caminho de leitura: `NtReadFile` no handle de entrada (STD_INPUT) **bloqueia** (faz `yield` até haver dado), ecoa os chars digitados na console (para o usuário ver o que digita) e trata Backspace; devolve ao chamador. `kernel32` ganha `ReadFile`/`ReadConsoleA` sobre `NtReadFile`, e `GetStdHandle(-10)` devolve o StandardInput (o kernel popula `ProcessParameters->StandardInput` = um CONSOLE_INPUT handle, análogo ao StandardOutput do Plano 11). Clean room nível 0: i8042/scancodes/ReadFile de documentação pública (osdev/MSDN), sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `crates/arch-x86_64` (portas i8042), nova crate de decode (ou módulo) testável no host, `crates/ntdll`/`kernel32`, QEMU + OVMF. Testes: `cargo xtask ktest` (headless, lógica/caminho de leitura com buffer pré-preenchido ou scancodes sintéticos) + demo interativa `cargo xtask run`.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 2: I/O manager — entrada de console). Sucede o Plano 14, já na `main`. Rumo ao "terminal A" que o usuário perguntou.

**Fora deste plano:** teclado por IRQ1/IOAPIC (aqui é polling no tick); layouts não-US / teclas mortas / Unicode além de ASCII; `ReadConsoleInput`/eventos de console ricos; mouse; um shell/REPL completo (vem depois, com file I/O e CreateProcess). Edição de linha mínima (eco + backspace), sem histórico/setas.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64; acesso a portas/i8042 em `crates/arch-x86_64`.
- Lógica pura (scancode→char, ring buffer) em crate/módulo testável no host (`cargo xtask test`); comportamento de hardware (polling do i8042, caminho de IRQ/tick) em ktest do kernel.
- Invariantes mantidas: faults de usuário contidos (Plano 14); `NtReadFile` valida ponteiros de usuário com `copy_to_user` (como `NtWriteFile`); o buffer de console é protegido por `IrqMutex` (lido no tick e no caminho de leitura — nunca segurar o lock atravessando um `yield`).
- Testes headless (não reintroduzir janela); o `cargo xtask run` interativo é a única coisa com janela.
- `kernel32`/`ntdll` sem heap do Rust; `kernel32` importa só `ntdll`.
- Clean room nível 0; sem binário/disassembly da MS. Comentários em inglês; docs/mensagens em português.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (`-p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings` e `-p xtask -p arch-x86_64 -p mm -p pe [-p <nova crate>] -- -D warnings`), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs <osdev i8042 / MSDN ReadFile>`; `Co-Authored-By:` do modelo. Ao fim, atualizar `ROADMAP.md` (subprojeto 2: entrada de console).

## Review Focus

1. **Decode scancode→char correto** (set 1; Shift/Enter/Backspace; ignora key-release 0x80+ e teclas desconhecidas sem pânico). Coberto por testes de host.
2. **Ring buffer seguro** (sem corrida leitura-no-tick vs consumo-na-leitura; não perde/duplica; cheio descarta o mais novo sem travar; `IrqMutex` nunca atravessa `yield`). Coberto por testes de host + ktest.
3. **`NtReadFile` bloqueante e seguro:** valida o buffer de usuário (USER+WRITABLE via `copy_to_user`), faz `yield` até haver dado, ecoa o que foi digitado, trata Backspace, devolve a contagem e preenche o `IO_STATUS_BLOCK`. Não trava o escalonador (libera o lock antes do `yield`). Coberto por ktest (buffer pré-preenchido/scancodes sintéticos).
4. **Caminho Win32:** `GetStdHandle(-10)` devolve o StandardInput populado pelo kernel; `ReadFile`/`ReadConsoleA` chamam `NtReadFile`; `kernel32` importa só `ntdll`. Coberto por ktest + a demo.
5. **Demo interativa real:** `cargo xtask run`, digitar ecoa; um programa lê uma linha e imprime de volta. (Manual; documentar os passos e o resultado.)

---

### Task 1: Decode de scancode + ring buffer de console (lógica pura) + polling do i8042

**Files:**
- Create: `crates/kbd/` (ou `kernel/src/kbd.rs` + testes) — decode set 1 → char + estado de Shift; um ring buffer de bytes.
- Modify: `crates/arch-x86_64/src/*` (ler portas 0x60/0x64 com segurança), `kernel/src/apic.rs` ou o tick (poll do i8042 por tick), `kernel/src/ktest.rs`

**Interfaces:**
- `pub fn decode(scancode: u8, state: &mut KbdState) -> Option<u8>` — set 1: faz/solta Shift atualiza `state`; key-make de tecla imprimível → ASCII (minúsculo/maiúsculo conforme Shift), Enter→`\n`, Backspace→`0x08`; key-release (bit 7) e teclas não mapeadas → `None`. Sem pânico.
- `ConsoleInput` ring buffer (`push(u8)` no produtor/tick, `pop()`/`is_empty()` no consumidor), protegido por `IrqMutex`; cheio → descarta o byte novo.
- Hook de polling: a cada tick (ou num ponto seguro), se o status do i8042 (0x64 bit0) indicar dado, ler 0x60, `decode`, e `push` no buffer.

- [ ] **Step 1: Testes que falham** — host: `decode` cobre a/A com Shift, Enter, Backspace, release ignorado, scancode desconhecido → None; ring buffer FIFO, vazio, cheio-descarta. ktest: injetar scancodes sintéticos no caminho de poll e ver chars no buffer.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** o decode, o buffer, o acesso às portas e o poll no tick.
- [ ] **Step 4: Rodar e ver passar** — `cargo xtask test` + `cargo xtask ktest` ok.
- [ ] **Step 5: Commit** (`feat(kbd): decode de scancode PS/2 (set 1) + ring buffer de console + poll i8042`, `Source: docs osdev PS/2 keyboard / i8042`).

---

### Task 2: `NtReadFile` de console (bloqueante, com eco) + `ReadFile`/`ReadConsoleA`/`GetStdHandle(STD_INPUT)`

**Files:**
- Modify: `kernel/src/syscall.rs` (NT_READ_FILE), `kernel/src/object.rs` (um CONSOLE_INPUT handle), `kernel/src/process.rs`/`teb.rs` (popular `ProcessParameters->StandardInput`), `crates/ntdll/src/lib.rs` (stub `NtReadFile`), `crates/kernel32/src/lib.rs` (`ReadFile`/`ReadConsoleA`; `GetStdHandle(-10)`), `kernel/src/ktest.rs`

**Interfaces:**
- `NtReadFile(FileHandle, ..., IoStatusBlock, Buffer, Length, ...)` (assinatura NT análoga ao `NtWriteFile`): resolve o handle; se for CONSOLE_INPUT, **bloqueia** (libera locks e faz `yield_now` em laço) até haver ≥1 byte; lê até `Length` (ou até `\n`, estilo linha — decidir e documentar), **ecoando** cada char na console (e tratando Backspace: apaga do eco e do que será devolvido); copia para o buffer de usuário com `copy_to_user`; preenche o `IO_STATUS_BLOCK` (Status + Information=contagem). Número de syscall novo (ex.: NT_READ_FILE=7; confirmar livre).
- kernel popula `ProcessParameters->StandardInput` = CONSOLE_INPUT handle; `kernel32!GetStdHandle(-10)` devolve-o; `ReadFile`/`ReadConsoleA` chamam `NtReadFile`.

- [ ] **Step 1: Teste que falha** — ktest `console_read_returns_input`: pré-preenche o ring buffer com "oi\n" (via o caminho de poll sintético da Task 1), roda um processo de usuário que faz `NtReadFile(stdin, buf, n)` e assere que recebe "oi\n" (e que o eco saiu na console). Skip se precisar de dlls ausentes.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** o dispatch bloqueante (sem segurar `IrqMutex` atravessando `yield`), o eco, o handle de input, o `ProcessParameters->StandardInput`, e os wrappers do `kernel32`/`ntdll`.
- [ ] **Step 4: Rodar e ver passar** — ktest ok; `kernel32` importa só `ntdll` (dumpbin).
- [ ] **Step 5: Commit** (`feat(kernel): NtReadFile de console bloqueante + eco; kernel32 ReadFile/ReadConsole/GetStdHandle(STD_INPUT)`, `Source: docs MSDN (ReadFile/ReadConsole)`).

---

### Task 3: Demo interativa "echo" + ktests de ponta-a-ponta + ROADMAP

**Files:**
- Create: `conformance/echo/echo.c` (ou um PE de teste do TukOS): lê uma linha do stdin e a imprime de volta.
- Modify: `xtask` (um alvo/doc para a demo interativa), `kernel/src/ktest.rs`, `ROADMAP.md`

**Interfaces:**
- Um programa `echo` (TukOS-built e/ou MSVC `/MT` via `fgets`/`printf`) que: lê uma linha (`ReadFile`/`fgets` no stdin), imprime "voce digitou: <linha>" no stdout, sai 0.
- Demo: `cargo xtask run` → digitar "teste\n" → ver o eco enquanto digita + "voce digitou: teste". Documentar os passos (manual; interativo).
- ktest automatizado: com o ring buffer pré-preenchido, rodar o `echo` pelo caminho multi-módulo e assere a saída (sem depender de digitação real).

- [ ] **Step 1: Teste que falha** — ktest `echo_reads_and_prints` (buffer pré-preenchido) falha antes.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** o `echo.c`/PE + o embed/boot + o ktest.
- [ ] **Step 4: Rodar e ver passar** — ktest ok; rodar a demo interativa manualmente e registrar o resultado no report. Atualizar `ROADMAP.md` (subprojeto 2: entrada de console presente; marco atual menciona teclado+eco). Se compilar um `echo.exe` /MT, adicionar um caso de conformidade com entrada pré-definida (comparar a saída com o Windows real, alimentando o stdin do processo de referência).
- [ ] **Step 5: Commit** (`feat: demo echo (le do teclado e imprime) + ktests e2e; ROADMAP`, `Source: docs MSDN`).

---

## Notas de fechamento

- Marco: o TukOS lê o teclado; teclas ecoam na tela; um programa lê uma linha e responde — a base de um terminal interativo. 
- Refinamentos futuros: teclado por IRQ1/IOAPIC (em vez de polling no tick); edição de linha rica (setas/histórico); `ReadConsoleInput`/eventos; layouts/Unicode; e então um **shell/REPL** (precisa de file I/O + `CreateProcess` para executar programas — plano posterior).
- Deferreds remanescentes de planos anteriores: P9 #1/#4, P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked self-only; TOCTOU futura; first-fit O(n)), P13 (não-isolamento entre testes; warnings dead-code sob features conformance), P14 (sem variante #GP explícita; exit code de processo faltado só logado).
- Depois: resto do subprojeto 2 (file I/O via NtCreateFile + um FS embutido/initrd; registro) e subprojeto 4 (user32/gdi32/janela).
