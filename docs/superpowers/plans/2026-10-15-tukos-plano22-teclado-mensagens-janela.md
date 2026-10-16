# TukOS Plano 22 — Teclado nas mensagens de janela (GUI interativa)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Tornar a janela do subprojeto 4 **interativa pelo teclado**: um programa GUI recebe as teclas como **mensagens de janela** (`WM_KEYDOWN`/`WM_KEYUP` + `WM_CHAR` via `TranslateMessage`) e responde — digitar texto aparece na janela. O `GetMessageA` passa a **bloquear** (com yield, sem travar o escalonador) até haver mensagem (um `WM_PAINT` inicial, um evento de tecla, ou `WM_QUIT`). Continua o Plano 21 (que deixou `GetMessageA` não-bloqueante, pintura única). Demonstrável: `cargo xtask run gui` abre uma janela que ecoa o que você digita e sai no Enter/ESC; e um ktest pré-alimenta scancodes e o **kernel lê o framebuffer** confirmando que o texto digitado foi renderizado.

**Architecture:** O decodificador PS/2 (Plano 15, `apic::on_timer`→`kbd::poll`) hoje empurra CHARS decodificados para o anel de console (linha, com eco, consumido pelo `NtReadFile`). Este plano acrescenta uma **fila de eventos de tecla crus** (key-down/up + char + virtual-key), alimentada pela MESMA decodificação, consumida pelo caminho GUI — independente do anel de console (o shell/console segue intacto). Um syscall novo `NtUserGetKeyEvent` (nr livre, ex. 17) devolve o próximo evento cru (ou "nenhum"). O `user32` ganha um message pump bloqueante: `GetMessageA` devolve `WM_PAINT`/`WM_QUIT` enfileirados primeiro, senão faz poll do `NtUserGetKeyEvent` (yield entre tentativas) e, havendo tecla, devolve `WM_KEYDOWN` (guardando o char); `TranslateMessage(WM_KEYDOWN)` sintetiza `WM_CHAR`; `DispatchMessageA` chama a `WndProc`; `PostQuitMessage` encerra. O `crates/kbd` ganha o mapeamento scancode→virtual-key (VK_*, winuser.h público) + o tipo de evento, com lógica pura host-testável. Clean room nível 0: VK_*/WM_*/MSG da documentação pública (winuser.h/MSDN); sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), x86-64. `kernel` (kbd input-event queue + syscall), `crates/kbd` (VK/char event decode, host-tested), `crates/user32` (message pump), `crates/ntdll` (stub), MSVC no host (programa GUI de demo), QEMU + framebuffer. Testes: host (decode de VK/char no `kbd`), ktest (feed de scancodes → syscall; feed → framebuffer mostra o texto) — tudo headless; só `cargo xtask run gui` abre janela.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 4: user32/gdi32/janela — entrada). Sucede o Plano 21 (janela abre + pinta), já na `main` @ fcd6270. Rumo ao desktop (design em docs/observations/ e na memória).

**Fora deste plano:** mouse (PS/2 IRQ12 — próximo plano); gerenciador de janelas / múltiplas janelas / foco entre janelas / compositing; teclas de função/combinações complexas/IME/layouts não-US; auto-repeat por hardware; `user32`/`gdi32` completos; o desktop do subprojeto 7. Verificação por conformidade-vs-Windows não se aplica a GUI; usa-se feed de scancodes + leitura do framebuffer (automatizado) + demo visual.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (winuser.h/MSDN públicos: VK_*, WM_KEYDOWN/WM_KEYUP/WM_CHAR, MSG; sem MS). `user32`/`ntdll` sem heap do Rust; `user32` importa só o permitido (verificar com o teste de imports do xtask).
- Invariantes mantidas (faults contidos P14; IAT por-módulo; ULONG as u32; ponteiros de usuário SÓ via usercopy — `NtUserGetKeyEvent` escreve o evento por `copy_to_user`). A fila de eventos de tecla é um anel limitado (descarta o mais antigo quando cheia — um processo não-GUI que nunca lê não trava nada).
- NÃO regredir: o anel de console/`NtReadFile`/eco (Plano 15), o shell, os 7 conformances, os ktests existentes (incl. `window_paints`/`gui_hello_window`), o boot padrão do shell. A decodificação PS/2 continua alimentando o anel de console; a fila de eventos GUI é um consumidor ADICIONAL.
- `GetMessageA` bloqueante NUNCA segura lock atravessando o yield, e NUNCA trava o escalonador (yield entre polls, como o `NtReadFile` bloqueante do Plano 15). Para o ktest ser determinístico, o programa de demo SAI num sentinela (Enter/`\r` ou ESC).
- Testes HEADLESS (`cargo xtask test` + `cargo xtask ktest`); só `cargo xtask run gui`/`vmware gui` abrem janela — agentes NÃO rodam esses.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (-D warnings nos gates do projeto + kernel ktest + crates novos no alvo certo), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs winuser.h (MSDN)`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP (subprojeto 4: janela interativa pelo teclado). Claude NÃO dá push (o user empurra a main).

## Review Focus

1. **Entrada segura:** `NtUserGetKeyEvent` escreve o evento via `copy_to_user` (sem deref cru); a fila de eventos é limitada e sem corrida com o `kbd::poll` do timer (mesmo `IrqMutex`/padrão do anel de console); um processo que não lê não causa travamento nem crescimento ilimitado.
2. **Sem regressão no console:** o anel de console/`NtReadFile`/eco e o shell seguem idênticos; a decodificação PS/2 alimenta AMBOS (console char + evento GUI) sem perder bytes nem duplicar eco; os ktests do Plano 15 passam.
3. **Message pump correto:** `GetMessageA` bloqueia com yield (sem lock atravessando), prioriza `WM_QUIT`/`WM_PAINT`, entrega `WM_KEYDOWN`/`WM_CHAR` com o char certo; `TranslateMessage` produz `WM_CHAR` coerente; `PostQuitMessage` encerra o laço; não trava o escalonador. `user32` importa só o permitido.
4. **Decode de teclado correto e puro:** o mapeamento scancode→(VK, char, down/up) no `crates/kbd` é lógica pura host-testada (letras, shift, Enter/Backspace/ESC, make/break 0xF0?/0x80 do set 1); casos adversariais (scancode desconhecido → sem evento ou VK_UNKNOWN, sem panic).
5. **Demonstrável + verificável:** um programa GUI de demo mostra o texto digitado e sai no sentinela; um ktest pré-alimenta scancodes de uma palavra conhecida e o kernel lê o framebuffer confirmando que os glifos foram desenhados na posição esperada; visualmente funciona no `cargo xtask run gui`.

---

### Task 1: Fila de eventos de tecla no kernel + `NtUserGetKeyEvent` + decode VK/char no `kbd`

**Files:** Modify `crates/kbd/src/lib.rs` (scancode→evento: VK + char + down/up, host-testado), `kernel/src/kbd.rs` (fila de eventos crus alimentada pelo `poll`, ao lado do anel de console), `kernel/src/syscall.rs` (NT_USER_GET_KEY_EVENT=17), `crates/ntdll/src/lib.rs` (stub)

**Interfaces:**
- `crates/kbd`: função PURA `decode_event(scancode_bytes) -> Option<KeyEvent{ vk: u16, ch: u32 (0 se nenhum), down: bool }>` (ou equivalente), set 1 (make/break), shift aplicado ao char; VK_* públicos (A-Z/0-9/Enter=0x0D/Back=0x08/Esc=0x1B/Space=0x20/...). Host-testada.
- `kernel/src/kbd.rs`: um `KEY_EVENTS: IrqMutex<RingBuffer<KeyEvent>>` (limitado, drop-oldest) alimentado por `poll` (que JÁ decodifica p/ o anel de console — agora também empurra o evento cru); `try_pop_key_event()`.
- `NtUserGetKeyEvent` (nr 17): a0 = `*mut KEY_EVENT_ABI{ vk:u32, ch:u32, flags:u32 (bit0=down) }`. **BLOQUEANTE** (reusa o padrão do `NtReadFile` bloqueante do Plano 15): se a fila de eventos está vazia, faz `kbd::poll()` + `sched::yield_now()` em laço (SEM segurar lock atravessando o yield) até haver evento; então escreve o próximo evento via `copy_to_user` e devolve STATUS_SUCCESS. (Decisão: bloquear no kernel evita um `NtYield` novo e um spin em modo usuário; o message pump do user32 confere as filas WM_QUIT/WM_CHAR/WM_PAINT ANTES de chamar este syscall, então nunca bloqueia quando já há o que entregar.) Stub ntdll naked (`mov eax, 17`).

- [ ] **Step 1: Teste que falha** — host: `kbd` decode de VK/char (letra, shift→maiúscula, Enter/Back/Esc, break/make, scancode desconhecido). ktest `key_event_queue`: pré-alimenta scancodes (via o hook de teste já existente `test_push_input`/análogo), chama `try_pop_key_event`/o caminho do syscall e confere os eventos (e que a fila esvazia e não cresce sem limite).
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. clippy/fmt; confirmar que o anel de console/`NtReadFile` seguem funcionando (ktests do P15 verdes).
- [ ] **Step 5: Commit** (`feat(kernel+kbd): fila de eventos de tecla + NtUserGetKeyEvent (entrada GUI crua)`, `Source: docs winuser.h`).

---

### Task 2: `user32` message pump bloqueante (WM_KEYDOWN/WM_CHAR + TranslateMessage)

**Files:** Modify `crates/user32/src/lib.rs`; Modify `kernel/src/ktest.rs`

**Interfaces:** `GetMessageA` passa a BLOQUEAR, nesta ordem: (1) se `PostQuitMessage` foi chamado → devolve `WM_QUIT` e retorna 0 (encerra o laço); (2) se há um `WM_CHAR` pendente (produzido por `TranslateMessage`) → devolve-o; (3) se há um `WM_PAINT` enfileirado (ShowWindow/UpdateWindow) → devolve-o; (4) senão chama o `NtUserGetKeyEvent` (stub importado do ntdll), que **bloqueia no kernel** até haver tecla (sem spin em modo usuário), e devolve `MSG{ message: WM_KEYDOWN, wParam: vk, lParam: scancode/flags }`, guardando o char associado p/ o translate. `TranslateMessage(&MSG)` sobre um `WM_KEYDOWN` com char associado → enfileira um `WM_CHAR` (wParam=char) que o PRÓXIMO `GetMessageA` devolve (passo 2). `DispatchMessageA` chama a `WndProc(hwnd,msg,wParam,lParam)`. Documentar a mudança de não-bloqueante→bloqueante (substitui o comportamento do Plano 21). Manter o modelo de janela única. (Repaints são síncronos: a WndProc pode desenhar direto no `WM_CHAR` via GetDC/TextOut — não é preciso acordar o GetMessage bloqueado p/ repintar.)

- [ ] **Step 1: Teste que falha** — ktest `window_keyboard_echo` (RED): um programa GUI de teste (MSVC, console-subsystem int main, como test_win) com um loop GetMessage/Translate/Dispatch; a WndProc em `WM_CHAR` desenha o char na janela (TextOut numa posição que avança) e em `WM_CHAR`=='\r' chama PostQuitMessage; o ktest pré-alimenta scancodes de "Hi\r", roda, e o kernel lê o framebuffer confirmando que "Hi" foi desenhado (pixels de glifo setados na posição esperada) e o programa saiu. Reapear a thread.
- [ ] **Step 2-4:** ver falhar; implementar o pump; ver passar; `dumpbin`/teste de imports do user32 (só o permitido). Confirmar `window_paints`/`gui_hello_window` ainda verdes (pintura única ainda funciona com o GetMessage novo).
- [ ] **Step 5: Commit** (`feat(user32): message pump bloqueante + WM_KEYDOWN/WM_CHAR/TranslateMessage`, `Source: docs winuser.h`).

---

### Task 3: Programa GUI interativo de demo + ktest + run + ROADMAP

**Files:** Create `gui/type_win/type_win.c`; Modify `xtask` (compilar/embutir; `cargo xtask run gui` abre a janela interativa), `kernel/src/conformance.rs`/`build.rs`/`ktest.rs`, `ROADMAP.md`

- [ ] **Step 1-4:** `type_win.c` (console-subsystem `/MT` `int main`, como test_win/hello_win): cria a janela, loop GetMessage/Translate/Dispatch; WndProc desenha fundo + mostra os chars digitados (TextOut acumulando numa string/posição), repinta a cada `WM_CHAR`, e sai no Enter/ESC. Compilar `/MT` linkando as libs da SDK (bind nos NOSSOS DLLs em runtime); embutir no initrd + no kernel (p/ o ktest). Fazer `cargo xtask run gui` bootar o `type_win` (ou um subcomando/flag p/ escolher entre hello_win e type_win — manter hello_win acessível). ktest `gui_type_window` (se distinto do `window_keyboard_echo` da T2; senão consolidar): feed de scancodes de uma palavra → framebuffer mostra a palavra. Atualizar `ROADMAP.md` (subprojeto 4: janela **interativa pelo teclado**; Marco atual).
- [ ] **Step 5: Commit** (`feat: janela interativa — digitar aparece na janela (type_win) + demo + ROADMAP`, `Source: docs winuser.h`).

---

## Notas de fechamento

- Marco (subprojeto 4): a janela ficou **interativa pelo teclado** — digitar entrega `WM_KEYDOWN`/`WM_CHAR` à `WndProc`, o texto aparece na janela, verificado por feed de scancodes + leitura do framebuffer e visível no `cargo xtask run gui`. O caminho de entrada `PS/2 → fila de eventos → NtUserGetKeyEvent → user32 message pump → WndProc` está de pé.
- Residual/futuro: mouse (PS/2 IRQ12 → WM_MOUSEMOVE/WM_LBUTTONDOWN) = próximo plano; gerenciador de janelas / múltiplas janelas / foco; auto-repeat; teclas de função/combinações/IME/layouts; GDI mais rico; double-buffering; o desktop do subprojeto 7.
- Deferreds remanescentes (de antes): RtlUnwind real (fputs/puts); refrescar o survey doc do Plano 21 (SetBkColor/GetModuleHandleA); ownership por-janela dos syscalls de GDI; chunk de fills grandes (latência de IRQ); BRUSH_TAG duplicado user32/gdi32; a causa-raiz do frame #GP (defendido, não isolado).
- Depois: mouse; depois gerenciador de janelas → rumo ao desktop (subprojeto 7); ou resto do subprojeto 2 (escrita de arquivos, diretórios, registro).
