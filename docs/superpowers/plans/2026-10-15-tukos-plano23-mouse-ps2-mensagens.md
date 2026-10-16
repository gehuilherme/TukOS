# TukOS Plano 23 — Mouse PS/2 nas mensagens de janela

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Dar **mouse** à janela do subprojeto 4: o kernel lê o mouse PS/2 (dispositivo auxiliar do i8042), mantém uma posição de cursor + estado dos botões, e entrega os eventos como **mensagens de janela** — `WM_MOUSEMOVE` (com x,y em `lParam`), `WM_LBUTTONDOWN`/`WM_LBUTTONUP` (e direito/meio), à `WndProc`. Para o pump poder esperar **tecla OU mouse** com um único syscall bloqueante, a entrada é **unificada** numa fila de eventos (tagged: tecla | mouse) + um syscall `NtUserGetInputEvent` que substitui/generaliza o `NtUserGetKeyEvent` do Plano 22. Demonstrável: um programinha de "paint" — arrastar com o botão desenha pontos/linha na janela; clicar desenha onde clicou — rodável no `cargo xtask run gui-paint`; e um ktest injeta pacotes de mouse e o **kernel lê o framebuffer** confirmando que o app respondeu (pontos desenhados nas posições esperadas). Continua os Planos 21 (janela pinta) e 22 (teclado nas mensagens).

**Architecture:** O i8042 já é lido no tick (`apic::on_timer`→`kbd::poll`), hoje DESCARTANDO bytes do mouse. Este plano **habilita o dispositivo auxiliar** (comandos i8042: enable aux `0xA8`, "write to aux" `0xD4`, mouse `0xF4` enable data reporting; ler o status bit AUXDATA p/ separar mouse de teclado) e decodifica o **pacote de 3 bytes** do mouse padrão (byte0: botões L/R/M + sinais/overflow de X/Y + bit 3 sempre 1; byte1: dx; byte2: dy — complemento de dois com sinal do byte0). O kernel mantém `MOUSE { x, y, buttons }` (x,y clampados à tela do framebuffer) e, a cada pacote completo, empurra eventos (move com x,y absolutos; botão down/up) numa fila. **Entrada unificada:** a fila `KEY_EVENTS` do Plano 22 vira uma fila de `InputEvent` tagged (`Key{vk,ch,down}` | `Mouse{x,y,buttons,kind}`); o syscall `NtUserGetInputEvent` (reusa nr 17, generaliza o `NtUserGetKeyEvent`) bloqueia (poll+yield, padrão do `NtReadFile`) e devolve o próximo `InputEvent` tagged via `copy_to_user`. O `user32` traduz: evento Key→`WM_KEYDOWN`/`WM_CHAR` (como no P22); evento Mouse→`WM_MOUSEMOVE`/`WM_*BUTTON*` (x,y em `lParam` = `(y<<16)|(x&0xFFFF)`, botões em `wParam`). **Sem cursor visível ainda** (precisa de compositor/save-under → fica p/ o plano do gerenciador de janelas); a visibilidade vem do app desenhar onde o mouse age (o demo de paint). Decode do pacote é lógica PURA host-testável. Clean room nível 0: estrutura do pacote PS/2 (hardware público) + WM_*/MK_* (winuser.h público); sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), x86-64. `kernel` (mouse i8042 + fila unificada + syscall), `crates/kbd` ou um `crates/mouse`/módulo (decode do pacote PS/2, host-testado), `crates/user32` (mensagens de mouse), `crates/ntdll` (stub generalizado), MSVC no host (programa de paint), QEMU + framebuffer (QEMU emula PS/2 mouse). Testes: host (decode do pacote), ktest (injeta pacotes → eventos; injeta → framebuffer mostra os pontos) — headless; só `cargo xtask run gui-paint` abre janela.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 4: entrada de mouse). Sucede os Planos 21/22, na `main` @ bfe65fc. Rumo ao desktop (design em docs/observations/ e na memória).

**Fora deste plano:** cursor de mouse VISÍVEL (sprite com save-under — precisa de compositor; vai com o gerenciador de janelas); roda do mouse (scroll/WM_MOUSEWHEEL); duplo-clique; captura/hover/leave; aceleração/sensibilidade; gerenciador de janelas / múltiplas janelas / foco / hit-testing entre janelas; mouse USB HID (só PS/2, que o QEMU emula); o desktop do subprojeto 7.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (protocolo PS/2 do mouse = hardware público; WM_MOUSEMOVE/WM_LBUTTONDOWN/MK_* = winuser.h/MSDN público; sem MS). `user32`/`ntdll` sem heap do Rust; `user32` importa só o permitido.
- Invariantes mantidas (faults contidos P14; ponteiros de usuário só via usercopy — `NtUserGetInputEvent` escreve o evento por `copy_to_user`). A fila de input é um anel limitado (descarta o mais antigo). O x,y do mouse é sempre clampado às dimensões do framebuffer (sem posição fora da tela).
- NÃO regredir: **o teclado do Plano 22** (os eventos Key devem continuar chegando como `WM_KEYDOWN`/`WM_CHAR`; `window_keyboard_echo` verde), o anel de console/`NtReadFile`/shell (Plano 15), os 7 conformances, os ktests (incl. `window_paints`/`gui_hello_window`/`window_keyboard_echo`), o boot padrão do shell + `run gui`/`run gui-type`. A generalização `NtUserGetKeyEvent`→`NtUserGetInputEvent` deve manter o teclado funcionando (atualizar o user32 junto; se o nr 17 mudar de forma, kernel+ntdll+user32 em sincronia).
- O `kbd::poll` passa a separar bytes de teclado vs mouse pelo bit AUXDATA do status do i8042; NÃO pode perder nem embaralhar os dois streams. Habilitar o aux não pode quebrar o teclado.
- `NtUserGetInputEvent` bloqueante: poll+yield, SEM segurar lock atravessando o yield (padrão do `NtReadFile`/Plano 22). Para o ktest ser determinístico, o programa de paint sai num sentinela (ESC via WM_KEYDOWN, ou um nº de eventos).
- Testes HEADLESS (`cargo xtask test` + `cargo xtask ktest`); só `cargo xtask run gui-paint`/`vmware gui-paint` abrem janela — agentes NÃO rodam esses.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (-D warnings nos gates do projeto + kernel ktest + crates no alvo certo), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs winuser.h (MSDN)` / `Source: docs ps2-mouse (hardware público)`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP (subprojeto 4: mouse). Claude NÃO dá push (o user empurra a main).

## Review Focus

1. **Mouse i8042 seguro e sem regressão de teclado:** habilitar o aux (0xA8/0xD4/0xF4) e separar os streams pelo bit AUXDATA não perde nem embaralha bytes; o teclado (P15 console + P22 eventos) continua idêntico; sem travar no tick (poll limitado por iteração como já é).
2. **Decode do pacote correto e puro:** a montagem do pacote de 3 bytes (sincronização pelo bit 3 do byte0 sempre 1; sinais/overflow; dx/dy complemento de dois) é lógica pura host-testada; x,y clampados à tela; casos adversariais (overflow, pacote dessincronizado) sem panic.
3. **Entrada unificada correta:** a fila vira `InputEvent` tagged; `NtUserGetInputEvent` bloqueia sem lock atravessando o yield, escreve o evento tagged via `copy_to_user`; o teclado segue funcionando após a generalização (kernel+ntdll+user32 em sincronia; `window_keyboard_echo` verde).
4. **Mensagens de mouse corretas:** `WM_MOUSEMOVE`/`WM_LBUTTONDOWN`/`WM_LBUTTONUP` (e R/M) com x,y em `lParam=(y<<16)|(x&0xFFFF)` e botões em `wParam` (MK_LBUTTON etc.), despachados à WndProc; `user32` importa só o permitido.
5. **Demonstrável + verificável:** um app de paint desenha onde o mouse age; um ktest injeta pacotes de mouse de um movimento/clique conhecido e o kernel lê o framebuffer confirmando os pontos desenhados nas posições esperadas; visualmente funciona no `cargo xtask run gui-paint`.

---

### Task 1: Mouse PS/2 no kernel + entrada unificada + `NtUserGetInputEvent`

**Files:** Modify `kernel/src/kbd.rs` (ou novo `kernel/src/mouse.rs` + o módulo de input unificado) — habilitar aux do i8042, decodificar pacote, estado do mouse, fila `InputEvent` unificada; `crates/kbd` ou `crates/mouse` (decode do pacote PURO host-testado); `kernel/src/syscall.rs` (NtUserGetInputEvent, generaliza o nr 17); `crates/ntdll/src/lib.rs` (stub generalizado)

**Interfaces:**
- Decode PURO (host-testado): monta o pacote de 3 bytes (resync pelo bit 3 do byte0) → `MousePacket{ dx: i16, dy: i16, left: bool, right: bool, middle: bool }` (aplica sinais/overflow). Sem panic em qualquer sequência.
- Kernel: habilita o aux no init (0xA8 enable aux; 0xD4+0xF4 enable reporting; set sample rate default ok); `poll` lê o status, roteia pelo bit AUXDATA (0x20) p/ o acumulador de pacote do mouse vs o teclado; a cada pacote, atualiza `MOUSE{x,y,buttons}` (x+=dx, y-=dy, clamp à tela) e empurra `InputEvent::Mouse{x,y,buttons,kind: Move|Down(btn)|Up(btn)}` na fila unificada (compara botões com o estado anterior p/ gerar down/up; move sempre que dx|dy≠0).
- `InputEvent` tagged (`Key{vk,ch,down}` | `Mouse{x,y,buttons,kind}`); a fila do P22 generalizada. `NtUserGetInputEvent` (nr 17): a0 = `*mut INPUT_EVENT_ABI` (um layout tagged: `kind_tag:u32` + union/campos fixos `a:u32,b:u32,c:u32` reinterpretados por tag — definir um ABI simples e documentado). BLOQUEIA (poll+yield, sem lock atravessando) até um evento; escreve via `copy_to_user`. Mantém o teclado: eventos Key continuam fluindo. Stub ntdll naked.

- [ ] **Step 1: Teste que falha** — host: decode do pacote (movimento +x/-y, cada botão, overflow, resync). ktest `mouse_events`: injeta bytes de pacotes de mouse (via hook de teste análogo ao de teclado, marcando AUXDATA) e confere os `InputEvent::Mouse` (posição acumulada/clamp, down/up por mudança de botão); e que injetar teclas AINDA gera `InputEvent::Key` (teclado não regrediu).
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. clippy/fmt; confirmar P15 console + P22 `window_keyboard_echo` verdes.
- [ ] **Step 5: Commit** (`feat(kernel): mouse PS/2 + entrada unificada (InputEvent) + NtUserGetInputEvent`, `Source: docs ps2-mouse; winuser.h`).

---

### Task 2: `user32` entrega mensagens de mouse (+ mantém teclado)

**Files:** Modify `crates/user32/src/lib.rs`; Modify `kernel/src/ktest.rs`

**Interfaces:** o pump passa a consumir `InputEvent` tagged do `NtUserGetInputEvent`: `Key`→`WM_KEYDOWN`(+`WM_CHAR` via TranslateMessage, como no P22); `Mouse`→ `WM_MOUSEMOVE` (0x0200) / `WM_LBUTTONDOWN` (0x0201) / `WM_LBUTTONUP` (0x0202) / `WM_RBUTTONDOWN` (0x0204) / `WM_RBUTTONUP` (0x0205) (meio opcional), com `lParam=(y<<16)|(x&0xFFFF)` e `wParam` = flags MK_* (MK_LBUTTON=0x0001, MK_RBUTTON=0x0002, ...) do estado atual dos botões. A prioridade do `GetMessageA` (quit→WM_CHAR pendente→WM_PAINT→bloqueia no input) se mantém; o evento de input bloqueante agora pode ser tecla OU mouse. `user32` importa só o permitido.

- [ ] **Step 1: Teste que falha** — ktest `window_mouse_click` (RED): um programa GUI de teste (MSVC, console int main) cuja WndProc em `WM_LBUTTONDOWN` desenha um ponto/quadradinho de cor conhecida na posição (x,y) do clique (de `lParam`); o ktest injeta um movimento p/ (X,Y) conhecido + um botão-down, roda, e o kernel lê o framebuffer confirmando o ponto em (X,Y); sai via sentinela. Reapear a thread.
- [ ] **Step 2-4:** ver falhar; implementar; ver passar; teste de imports do user32. Confirmar `window_keyboard_echo`/`window_paints`/`gui_hello_window` ainda verdes.
- [ ] **Step 5: Commit** (`feat(user32): WM_MOUSEMOVE/WM_LBUTTONDOWN/UP a partir dos InputEvent de mouse`, `Source: docs winuser.h`).

---

### Task 3: Demo de paint + `cargo xtask run gui-paint` + ROADMAP

**Files:** Create `gui/paint_win/paint_win.c`; Modify `xtask`/`kernel` (feature `gui_paint` + `boot_feature` "gui-paint"; embutir), `kernel/src/ktest.rs`, `ROADMAP.md`

- [ ] **Step 1-4:** `paint_win.c` (console-subsystem `/MT int main`, como test_type): cria janela; WndProc desenha pontos/quadradinhos em `WM_MOUSEMOVE` enquanto o botão esquerdo está pressionado (checa MK_LBUTTON no wParam) e em `WM_LBUTTONDOWN` (um ponto no clique); sai no ESC (WM_KEYDOWN VK_ESCAPE). Compilar `/MT` (bind nos NOSSOS DLLs em runtime); embutir. Feature `gui_paint=["gui"]` + `cargo xtask run gui-paint`/`vmware gui-paint` boota o paint_win (manter gui/gui-type/shell). ktest `gui_paint_window` (se distinto do `window_mouse_click` da T2; senão consolidar): injeta um arrasto conhecido → framebuffer mostra a trilha. Atualizar `ROADMAP.md` (subprojeto 4: **mouse** — mover/clicar entrega mensagens, app de paint responde; Marco atual; o que falta: cursor visível, roda, duplo-clique, gerenciador de janelas).
- [ ] **Step 5: Commit** (`feat: app de paint com mouse (gui-paint) + demo + ROADMAP`, `Source: docs winuser.h`).

---

## Notas de fechamento

- Marco (subprojeto 4): a janela ganhou **mouse** — mover/clicar/arrastar entrega `WM_MOUSEMOVE`/`WM_LBUTTONDOWN`/`WM_LBUTTONUP` à `WndProc`, um app de paint desenha com o mouse, verificado por injeção de pacotes + leitura do framebuffer e visível no `cargo xtask run gui-paint`. O caminho `PS/2 mouse → InputEvent unificado → NtUserGetInputEvent → user32 → WndProc` está de pé, e o teclado (P22) segue junto no mesmo pump.
- Residual/futuro: **cursor de mouse visível** (sprite + save-under / compositor) — vai com o gerenciador de janelas; roda/scroll (WM_MOUSEWHEEL); duplo-clique; captura/hover/leave; gerenciador de janelas / múltiplas janelas / foco / hit-testing; mouse USB HID; o desktop do subprojeto 7.
- Deferreds remanescentes (de antes): objeto de espera real p/ os syscalls bloqueantes (NtReadFile + NtUserGetInputEvent — acordar thread morta); RtlUnwind real (fputs/puts); refrescar survey doc do P21; ownership por-janela / chunk de fills grandes; BRUSH_TAG duplicado; causa-raiz do frame #GP.
- Depois: gerenciador de janelas (múltiplas janelas + cursor visível + foco) rumo ao desktop (subprojeto 7); ou resto do subprojeto 2 (escrita de arquivos, diretórios, registro).
