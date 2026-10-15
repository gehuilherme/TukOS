# TukOS Plano 20 — Passagem de argumentos (command line → GetCommandLine → argv)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Fazer `echo teste` imprimir `teste` — e, em geral, um comando do shell receber seus **argumentos**. Hoje o shell dá `CreateProcess` só com o NOME do programa (Plano 19 adiou argv), e o `echo.exe` (Plano 15) lê stdin em vez de args, então `echo teste` sai vazio. Este plano implementa a **linha de comando** ponta-a-ponta: o shell passa a linha completa; o kernel a guarda no `PEB->ProcessParameters->CommandLine`; `kernel32!GetCommandLineA/W` a devolve; o CRT monta `argv`; e o `echo.c` reescrito imprime `argv`. Fecha uma limitação visível do terminal e habilita programas reais que usam argumentos.

**Architecture:** `kernel32!CreateProcessA(lpApplicationName=NULL, lpCommandLine="echo teste", ...)` (padrão Win32: sem appName, o 1º token da command line é o programa). O wrapper acha o programa (1º token → `<cmd>.exe` no initrd), e passa a **command line inteira** ao kernel via `NT_CREATE_PROCESS` (estendido com um ponteiro+len de command line UTF-16). O kernel, ao montar o `ProcessParameters` do filho, grava a `CommandLine` como `UNICODE_STRING` (buffer numa página de usuário do filho) no offset público `RTL_USER_PROCESS_PARAMETERS.CommandLine` (0x70). `kernel32!GetCommandLineW` devolve `ProcessParameters->CommandLine.Buffer`; `GetCommandLineA` devolve uma cópia ANSI (num buffer estático do processo). O CRT (libcmt) lê `GetCommandLineA/W` e monta `argv`/`argc` sozinho — então `main(argc, argv)` do `echo.c` recebe os tokens. Clean room nível 0: layout do `RTL_USER_PROCESS_PARAMETERS`/`UNICODE_STRING` e semântica de `GetCommandLine` da documentação pública; sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `kernel32`/`ntdll`, MSVC no host, QEMU + OVMF. Testes: ktest com teclado pré-alimentado + conformidade (`echo teste` vs Windows).

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 2: process parameters/argv). Sucede o Plano 19, já na `main`.

**Fora deste plano:** aspas/escapes complexos no parsing de argv (o CRT já faz o parsing padrão do Windows a partir da string — nós só entregamos a string; casos de aspas seguem a regra do CRT); variáveis de ambiente (`GetEnvironmentStrings` segue mínimo); redirecionamento/pipes; `CommandLineToArgvW` (o CRT do `main` não usa). O 1º token é o nome do programa (sem PATH, `<cmd>.exe` no initrd).

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (winternl/MSDN públicos; sem MS). Invariantes mantidas (P14 faults contidos; P15 teclado; P16 initrd; P17 CreateProcess/espera; P18 reciclagem; ponteiros validados; ULONG as u32; `kernel32` importa só `ntdll`).
- A `CommandLine` do filho vai numa página de usuário do filho (como o resto do `ProcessParameters`/TEB/PEB); `GetCommandLineA` copia p/ um buffer estático do processo (sem heap do Rust em kernel32). Tamanho limitado (ex.: ≤512 bytes); linha maior é truncada de forma segura (documentar).
- Testes headless; `cargo xtask run`/`vmware` interativos. Sem MSVC auto-pula; Linux CI verde.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (kernel ktest + xtask/arch/mm/pe/kbd -D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs winternl.h (RTL_USER_PROCESS_PARAMETERS) / MSDN (GetCommandLine)`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP se fizer sentido.

## Review Focus

1. **CommandLine entregue com segurança:** o kernel copia a string do pai (`copy_from_user`, tamanho limitado) e a grava numa página de usuário do filho como `UNICODE_STRING` bem-formado (Length/MaximumLength/Buffer coerentes, NUL-terminado); nenhum overflow/ponteiro inválido; o filho só vê a própria command line.
2. **GetCommandLineA/W corretos:** W devolve o `Buffer` do `ProcessParameters->CommandLine` (ponteiro estável durante a vida do processo); A devolve uma cópia ANSI estável; ambos consistentes com o que o shell passou. O CRT monta `argv` a partir disso.
3. **CreateProcessA fiel:** com `lpApplicationName=NULL`, o 1º token da `lpCommandLine` é o programa; a command line INTEIRA (incl. o nome) vai ao filho; `argv[0]` = nome, `argv[1..]` = args. Linha vazia/só-nome → `argc==1`.
4. **echo funciona:** `echo.c` reescrito imprime `argv[1..]` separados por espaço + `\n`; `echo teste` → `teste`; `echo a b c` → `a b c`; `echo` (sem args) → linha vazia. Coberto por ktest + conformidade.
5. **Regressão:** hello/help/exit e os 7 casos de conformidade seguem; `kernel32` importa só `ntdll`.

---

### Task 1: Kernel — `ProcessParameters->CommandLine` + `NT_CREATE_PROCESS` com command line

**Files:** Modify `kernel/src/process.rs` (montar a CommandLine no ProcessParameters do filho), `kernel/src/teb.rs` (offset CommandLine@0x70 + escrever o UNICODE_STRING), `kernel/src/syscall.rs` (NT_CREATE_PROCESS estendido), `crates/ntdll/src/lib.rs` (stub), `kernel/src/ktest.rs`

**Interfaces:**
- `NT_CREATE_PROCESS` ganha (além de path) um ponteiro+len de **command line** UTF-16 (ou reusa um struct de params): `(out_handle, image_path_utf16, image_len, cmdline_utf16, cmdline_len)` — escolher a ordem (regs + stack_arg) e documentar p/ a Task 2. O kernel `copy_from_user` a cmdline (≤512 bytes), e ao montar o `ProcessParameters` do filho grava `CommandLine` = UNICODE_STRING apontando p/ um buffer (UTF-16, NUL-terminado) numa página de usuário do filho, no offset `RTL_USER_PROCESS_PARAMETERS.CommandLine` = 0x70 (Length/MaximumLength em bytes). Se cmdline vazia, usar o próprio nome do programa.
- Um ktest que cria um processo com uma command line conhecida e confere (via leitura do espaço do filho / um programa de teste que lê o PEB->ProcessParameters->CommandLine) que ela chegou.

- [ ] **Step 1-4:** teste que falha; implementar; ver passar. Validar com `copy_from_user`/`copy_to_user`; aritmética checada; `kernel32` intacto.
- [ ] **Step 5: Commit** (`feat(kernel): CommandLine no ProcessParameters do filho (NT_CREATE_PROCESS com cmdline)`, `Source: docs winternl.h (RTL_USER_PROCESS_PARAMETERS)`).

---

### Task 2: kernel32 — `CreateProcessA` passa a command line + `GetCommandLineA/W` reais

**Files:** Modify `crates/kernel32/src/lib.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- `CreateProcessA`: se `lpApplicationName` é NULL, o 1º token de `lpCommandLine` é o programa (`<token>.exe` no initrd); passa a **command line inteira** (ANSI→UTF-16) ao `NT_CREATE_PROCESS` (além do nome do programa resolvido). Se `lpApplicationName` dado, usa-o como programa e `lpCommandLine` como a linha (ou o nome, se cmdline NULL). Bounded; documentar.
- `GetCommandLineW() -> *const u16`: lê `PEB->ProcessParameters->CommandLine.Buffer` (via GS→PEB→PP@0x20→CommandLine@0x70). `GetCommandLineA() -> *const u8`: converte p/ ANSI num buffer estático do processo (uma vez, cacheado). Substituem os stubs atuais (que devolviam "hello.exe" fixo).
- kernel32 importa só ntdll.

- [ ] **Step 1-4:** ktest `program_sees_argv`: um programa de teste (TukOS/MSVC) criado com cmdline "prog arg1 arg2" imprime `argc`/`argv` (ou um marcador) — assere que recebeu os args. Ver falhar/implementar/passar. dumpbin kernel32 só ntdll.
- [ ] **Step 5: Commit** (`feat(kernel32): CreateProcessA passa a command line; GetCommandLineA/W reais`, `Source: docs MSDN (GetCommandLine)`).

---

### Task 3: Shell passa a linha + `echo` reescrito (argv) + conformidade + ROADMAP

**Files:** Modify `conformance/shell/shell.c` (passar a linha completa ao CreateProcess), `conformance/echo/echo.c` (imprimir argv), `xtask/src/conformance.rs` (caso `echo` com args), `kernel/src/ktest.rs`, `ROADMAP.md`

**Interfaces:**
- `shell.c`: ao executar um comando, chamar `CreateProcessA(NULL, linha_completa, ...)` (a linha digitada inteira, trimada) em vez de só o nome — assim o filho recebe `argv` com o comando + args. (O 1º token continua sendo o programa.)
- `echo.c`: `int main(int argc, char** argv){ for i in 1..argc { print argv[i]; print ' ' ou '\n' no fim } }` — imprime os args separados por espaço + `\n`. (Deixa de ler stdin.) Mantém o comportamento esperado de um `echo`.
- Conformidade: um caso `echo` onde o programa é chamado com args fixos ("echo teste") e a saída ("teste\n") é comparada Windows vs TukOS. (O shell em si não conforma, mas o `echo.exe` com uma command line fixa, sim — ou um ktest determinístico do shell com teclado pré-alimentado "echo teste\n".)

- [ ] **Step 1-4:** ktest `shell_echo_args` (teclado pré-alimentado "echo teste\nexit\n") assere que a saída inclui "teste" (não vazio). `echo.c` via conformidade com args fixos → "teste\n" bate com Windows. Ver falhar/implementar/passar. Todas as variantes + conformidades ok.
- [ ] **Step 5: Commit** (`feat: shell passa argumentos; echo imprime argv; conformidade`, `Source: docs MSDN`; incluir ROADMAP se atualizar).

---

## Notas de fechamento

- Marco: `echo teste` imprime `teste` no terminal do TukOS — comandos recebem argumentos de verdade (command line → GetCommandLine → argv do CRT), como no Windows.
- Residual/futuro: parsing de aspas/escapes é o do CRT (entregamos a string); variáveis de ambiente ricas; `CommandLineToArgvW`; redirecionamento/pipes; PATH.
- Deferreds remanescentes: P9/P10/P11/P13/P14/P15 (teclado PS/2 polling — relevante p/ o teste do usuário no VMware, que pode usar teclado USB)/P16/P17 (build_space panica; Wait sem timeout)/P18 (reap-queue; 64 simultâneas).
- Depois: possivelmente um **driver de teclado USB/IRQ** (se o VMware não expõe PS/2, p/ o usuário testar no VMware real); e os subprojetos 4 (user32/gdi32/janela) e 7 (desktop).
