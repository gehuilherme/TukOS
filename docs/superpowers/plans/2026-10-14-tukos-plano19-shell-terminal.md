# TukOS Plano 19 — Shell/REPL interativo (o terminal)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Um **terminal usável**: um programa shell roda como o primeiro processo, imprime um prompt, lê uma linha do teclado (com eco), interpreta o comando, executa o programa correspondente do initrd via `CreateProcess`, espera ele terminar, e repete. Builtins mínimos (`help`, `exit`). Demonstração: no `cargo xtask run` interativo, você digita `hello` e vê "Hello, TukOS!"; digita `echo oi` e vê o eco; `help`/`exit` funcionam. É a junção de tudo deste marco — teclado (P15), file I/O/initrd (P16), CreateProcess+espera (P17), reciclagem de slots (P18) — e o "terminal A" que o usuário perguntou.

**Architecture:** `conformance/shell/shell.c` é um programa `/MT` (CRT) no initrd que o kernel inicia como processo inicial (uma feature/modo de boot `shell`, ou o default quando presente). Loop: escreve o prompt (`"tukos> "`) com `WriteFile`/`printf`; lê uma linha com `fgets`/`ReadFile` do stdin (o `NtReadFile` de console bloqueante com eco do Plano 15); faz trim e separa o 1º token (comando) do resto (args, ignorados por ora salvo eco); se for builtin (`help`, `exit`), trata; senão monta `"<cmd>.exe"` e chama `CreateProcessA` (resolve no initrd via `NT_OPEN_FILE`/`NT_CREATE_PROCESS` do P16/P17), `WaitForSingleObject(INFINITE)`, e volta ao prompt; comando desconhecido → "comando nao encontrado: <cmd>". O filho compartilha a console (stdout do filho aparece na mesma tela). Programas disponíveis no initrd: os já existentes (`hello.exe`, `echo.exe`/`fileio.exe` adaptados como comandos). Clean room nível 0: só o nosso `shell.c` + programas nossos; sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `kernel32`/`ntdll`, MSVC no host p/ compilar o shell e os comandos, QEMU + OVMF. Testes: ktest com teclado pré-alimentado (headless) + demo interativa `cargo xtask run`.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 7: shell; e a interação dos subprojetos 2). Sucede o Plano 18, já na `main`.

**Fora deste plano:** passar argumentos de verdade ao filho (o CRT do filho não recebe argv ainda — comandos sem args reais; o shell pode ecoar args mas não os repassa); pipes/redirecionamento (`|`, `>`); variáveis de ambiente; histórico/setas/edição rica de linha (edição mínima do P15: eco + backspace); cwd/caminhos (nome do comando → `<cmd>.exe` no initrd, sem PATH); rodar binários de disco real; GUI/desktop (subprojeto 7 depois). Sem conformidade vs Windows (o shell é do TukOS, não um programa do Windows).

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (só nossos programas; sem MS). Invariantes mantidas (teclado/console P15, file I/O P16, CreateProcess/espera P17, reciclagem de slots P18, faults contidos P14). O shell e os comandos são `/MT` e importam só `kernel32` (que importa só `ntdll`).
- O shell roda indefinidamente spawnando comandos — depende da reciclagem de slots (P18) p/ não esgotar; um comando que falha (fault) é contido (P14) e o shell continua (WaitForSingleObject retorna com o status de erro).
- Testes headless; `cargo xtask run` é a única janela (onde o teclado real funciona). O ktest usa teclado pré-alimentado (kbd::test_push_input) p/ dirigir o shell de forma determinística. `build.rs`/`xtask`: sem MSVC auto-pula; Linux CI verde.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (kernel ktest + xtask/arch/mm/pe/kbd -D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs MSDN`/`Source: blackbox conformance/shell/`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP (subprojeto 7: shell/terminal presente; Marco).

## Review Focus

1. **Loop do shell robusto:** lê linha (bloqueante, eco), parseia o 1º token sem overflow (buffer fixo), trata linha vazia (reimprime o prompt), EOF/erro de leitura sem travar; builtins e CreateProcess corretos; comando desconhecido reporta e continua.
2. **Spawn+espera no loop:** cada comando é CreateProcess+Wait; o shell continua após o filho sair (inclusive se o filho falhar — status de erro, shell vivo); spawns repetidos NÃO esgotam a tabela (P18); sem leak de frames/handles entre comandos (fecha os handles do PROCESS_INFORMATION).
3. **E/S de console compartilhada:** o prompt e a saída do filho aparecem na mesma console sem corromper; o eco do que é digitado funciona (P15); backspace edita a linha.
4. **Determinístico no ktest:** com o teclado pré-alimentado por um script, o shell roda os comandos e produz a saída esperada; o teste não depende de digitação real.
5. **Demo interativa:** `cargo xtask run` boota o shell; digitar comandos funciona ao vivo (manual; documentar os passos e o resultado).

---

### Task 1: O programa shell + rodar como init + ktest com script

**Files:** Create `conformance/shell/shell.c`; Modify `xtask` (compilar o shell + embutir no initrd), `kernel/src/fs.rs`/`build.rs` (chave "shell.exe" + comandos), `kernel/src/conformance.rs`/`process.rs` (feature de boot `shell`), `kernel/src/ktest.rs`

**Interfaces:**
- `shell.c` (`/MT`): `main` faz o loop { `printf("tukos> ")`+flush; `fgets(line, sizeof line, stdin)`; se NULL/vazio continua; trim; 1º token = cmd; se `exit`→return 0; se `help`→lista os comandos; senão `snprintf(path,"%s.exe",cmd)` + `CreateProcessA(path,...)`; se falhou→`printf("comando nao encontrado: %s\n", cmd)`; senão `WaitForSingleObject(pi.hProcess, INFINITE)` + `CloseHandle(hProcess/hThread)`; repete }.
- Kernel: uma feature/modo `shell` que inicia `shell.exe` (do initrd) como o processo inicial; o initrd tem `shell.exe`, `hello.exe`, e pelo menos mais um comando (ex. `echo.exe`). Reusa o caminho multi-módulo (exe→kernel32→ntdll) e o CreateProcess do P17.
- ktest `shell_runs_command`: pré-alimenta o teclado (kbd::test_push_input) com um script, ex.: `"hello\nexit\n"`; boota o shell; assere que a saída inclui o prompt, "Hello, TukOS!" (do hello.exe spawnado), e que o shell terminou no `exit` (ou rodou os dois comandos). Determinístico, sem digitação real.

- [ ] **Step 1: Teste que falha** — adicionar `shell.c`, o boot do shell, e `shell_runs_command`; sem implementação, falha/ausente.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** o shell + o boot + o embed dos comandos no initrd. Depurar com `-d int`/serial se o spawn-de-dentro-do-shell travar (é CreateProcess chamado por um processo de usuário, não pelo kernel — exercita o caminho real do P17 a partir do Ring 3).
- [ ] **Step 4: Rodar e ver passar** — `cargo xtask ntdll && cargo xtask kernel32 && cargo xtask ktest` → `shell_runs_command` passa; demais ok. `kernel32` importa só `ntdll`.
- [ ] **Step 5: Commit** (`feat: shell interativo (le comando do teclado, CreateProcess, espera) + ktest com script`, `Source: blackbox conformance/shell/shell.c`).

---

### Task 2: Builtins + comando desconhecido + `cargo xtask run` boota o shell + ROADMAP

**Files:** Modify `conformance/shell/shell.c` (builtins help/exit, mensagem de desconhecido), `xtask/src/main.rs`/`image.rs`/`qemu.rs` (o `run` interativo boota o shell), `kernel/src/ktest.rs`, `ROADMAP.md`

**Interfaces:**
- `help` lista os comandos disponíveis (hardcoded: hello, echo, help, exit); `exit` encerra o shell (o processo inicial sai → o kernel pode parar/ociar graciosamente); comando desconhecido → "comando nao encontrado: <cmd>" e continua.
- `cargo xtask run` (interativo, com janela) passa a bootar o TukOS com o shell como init (a feature `shell`), para o usuário digitar de verdade. (ktest/conformance seguem no modo normal/headless.)
- ktest `shell_unknown_and_help`: script `"help\nnaoexiste\nexit\n"` → assere a listagem do help, a mensagem de desconhecido, e o término.

- [ ] **Step 1: Teste que falha** — `shell_unknown_and_help` + os builtins; falha antes.
- [ ] **Step 2-3:** ver falhar; implementar builtins + o unknown + o `run`-boota-shell.
- [ ] **Step 4: Rodar e ver passar** — ktest ok; rodar a demo interativa (`cargo xtask run`) manualmente: digitar `help`, `hello`, `echo oi`, `naoexiste`, `exit`; registrar o resultado no report. Atualizar `ROADMAP.md`: subprojeto 7 (shell) iniciado / "Marco atual" = um terminal interativo roda comandos; subprojeto 2 completo o suficiente p/ um shell.
- [ ] **Step 5: Commit** (`feat(shell): builtins help/exit + comando desconhecido; cargo xtask run boota o shell; ROADMAP`, `Source: docs MSDN`).

---

## Notas de fechamento

- Marco (o terminal que o usuário perguntou): o TukOS boota num shell interativo; você digita um comando, ele roda o programa do initrd e mostra a saída, e volta ao prompt — um terminal usável, feito 100% sobre a pilha NT do TukOS (teclado→console→CreateProcess→espera).
- Residual/futuro: argumentos reais ao filho (argv); pipes/redirecionamento; histórico/edição rica; PATH/cwd; o reap-queue (P18 concern b) e o build_space-sem-panic (P17) antes de exes não-confiáveis; e então o subprojeto 7 completo (desktop gráfico — o design em docs/observations/ e na memória).
- Deferreds remanescentes: P9 #1/#4, P10, P11, P13, P14 (#GP sem teste), P15 (teclado polling/sem IRQ), P16 (>1MB; sem write/dirs), P17 (build_space panica; Wait sem timeout), P18 (reap-queue; 64 simultâneas panica).
- Depois: subprojeto 4 (user32/gdi32/janela — "programa com janela abre") e subprojeto 7 (desktop), rumo ao uso diário.
