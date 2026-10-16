# TukOS Plano 25 — Gerenciador de janelas (compositor + múltiplas janelas + foco)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Fundação do **desktop**: cada janela desenha num **backbuffer** próprio (off-screen); o kernel **compõe** o fundo-de-tela + as janelas (por z-order, com moldura: barra de título + borda) + o cursor (Plano 24) no topo, para o framebuffer. Isso permite **múltiplas janelas** ao mesmo tempo (impossível com o desenho direto na tela de hoje) e **roteamento de input**: o mouse vai para a janela sob o cursor (hit-test), o teclado para a janela com **foco**; clicar numa janela a **levanta** (traz pra frente) e **foca**. Demonstrável: um demo abre **duas janelas** sobrepostas num fundo-de-tela; clicar alterna o foco/ordem; cada uma mostra seu próprio conteúdo; o cursor fica por cima. Verificado headless: ktests compõem janelas e conferem o framebuffer (conteúdo da janela A no rect dela, B no dela, fundo em volta, clique troca o topo). **Compatível pra trás:** os programas GUI existentes (hello_win/test_*/paint_win) continuam funcionando — uma janela full-screen-na-origem compõe exatamente os mesmos pixels de antes, então os ktests atuais (window_paints/gui_hello_window/window_keyboard_echo/window_mouse_click/gui_paint_window) seguem verdes.

**Architecture:** Hoje o modelo é **janela única** e o GDI desenha DIRETO no framebuffer (fbcon) via NtGdiFillRect/TextOut; user32 ignora posição/tamanho (janela = tela toda). Este plano introduz **janelas com superfície**: o kernel ganha um objeto `Window { id, owner, x, y, w, h, z, focused, backbuffer: Vec<u32> (w*h), visible }` e um **compositor** `composite()` que, sob o FBCON lock: pinta o fundo-de-tela, desenha cada janela visível em ordem de z (backbuffer blitado em (x,y) + moldura: título + borda), e deixa o cursor do Plano 24 por cima (hide/show em volta do composite). Os syscalls de GDI (NtGdiGetFbInfo/FillRect/TextOut) passam a operar no **backbuffer da janela corrente do processo** (não na tela): `NtGdiGetFbInfo` devolve o tamanho do CLIENTE da janela; `FillRect`/`TextOut` escrevem no backbuffer (coords do cliente). Novos syscalls: `NtUserCreateWindow(x,y,w,h, style) -> hwnd` (cria a superfície), `NtUserDestroyWindow(hwnd)`, `NtUserFlushWindow(hwnd)` (marca sujo → recompõe), `NtUserNextInputEventFor(...)`/roteamento (ver abaixo). **Compat full-screen:** se um programa cria uma janela com tamanho >= tela (ou CW_USEDEFAULT no modelo antigo), ela vira full-screen-na-origem (x=0,y=0,w=W,h=H, sem moldura visível) → compõe idêntico ao de hoje. **Input routing:** a entrada unificada (Plano 23, InputEvent Key|Mouse) é roteada pelo **gerenciador**: evento de mouse → acha a janela sob (cursor_x,cursor_y) por z-order (hit-test) e marca-a como destino; botão-down → levanta+foca essa janela; evento de teclado → janela focada. Cada processo, no `GetMessage`, recebe só os eventos destinados à(s) SUA(s) janela(s) (coords de mouse convertidas pra cliente). O `user32` deixa de ser janela-única: mantém uma lista de janelas do processo, cada `CreateWindowExA` chama `NtUserCreateWindow` e guarda o hwnd+WndProc; `GetMessageA` consome o input roteado e entrega `MSG` com o hwnd certo; `BeginPaint/EndPaint` miram o backbuffer da janela + `NtUserFlushWindow` no EndPaint. Clean room nível 0 (WM_*, estilos WS_*, moldura/estética próprias; sem asset/binário da MS).

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), x86-64. `kernel` (objeto Window + compositor + syscalls + roteamento), `crates/gfx` (blit/clip puro host-testado), `crates/user32` (multi-janela + roteamento), `crates/ntdll` (stubs), MSVC no host (demo de 2 janelas), QEMU + framebuffer. Testes: host (blit/clip/hit-test puros), ktest (compor janelas + conferir framebuffer; rotear input) — headless; só `cargo xtask run gui*` abre janela.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 4: gerenciador de janelas; subprojeto 7: desktop). Sucede os Planos 21-24, na `main` @ 581f57f. Rumo ao desktop (design em docs/observations/ e na memória [[tukos-desktop-design]]).

**Fora deste plano:** mover/arrastar janela pela barra de título; redimensionar; minimizar/maximizar/fechar pelos botões; sobreposição com transparência/alpha/sombras; taskbar/menu iniciar; múltiplos monitores; aceleração/dirty-rects finos (recompor a tela inteira por ora); WM_NCHITTEST/áreas não-cliente completas; o shell do desktop (subprojeto 7 propriamente). Este plano entrega a FUNDAÇÃO (compor + múltiplas janelas + foco/z-order + roteamento de input); mover/redimensionar/decorações ativas vêm depois.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (WM_*/WS_*/estética da moldura próprios; sem asset/binário da MS). `user32`/`ntdll` sem heap do Rust além do permitido; `user32` importa só o permitido.
- Invariantes: faults contidos P14; ponteiros de usuário só via usercopy; o backbuffer de cada janela é memória do KERNEL (não mapeada no usuário) — o GDI desenha nele via syscall (como o Plano 21: sem MMIO/kernel-mem no usuário). Todo blit/clip respeita os limites da tela e do backbuffer (sem OOB). A lista de janelas é limitada (ex. <= 32) — criar além disso falha limpo.
- **NÃO regredir:** os programas GUI existentes (uma janela full-screen-na-origem compõe os mesmos pixels) → window_paints/gui_hello_window/window_keyboard_echo/window_mouse_click/gui_paint_window VERDES; console/shell/7 conformances; o cursor do Plano 24 (agora composto por cima); o teclado/mouse dos Planos 22/23 (agora roteados por janela, mas uma só janela full-screen recebe tudo como antes).
- Compositor sob o FBCON lock (serializa com o cursor e o console); o cursor do Plano 24 fica por cima (hide antes de compor, show depois) — sem re-lock, sem deadlock. Recompor a tela inteira a cada mudança/flush (otimização de dirty-rect fica pra depois).
- Testes HEADLESS; só `cargo xtask run gui*`/`vmware gui*` abrem janela — agentes NÃO rodam esses.
- Gates por tarefa: `cargo fmt --all -- --check`, clippy (-D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs winuser.h (MSDN)` / `Source: docs`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP. Claude NÃO dá push.

## Review Focus

1. **Compositor correto e seguro:** compõe fundo + janelas (z-order) + moldura + cursor sem OOB (blit clampa backbuffer→tela); o backbuffer é memória de kernel (sem exposição ao usuário); recompor é determinístico; o cursor fica por cima. Compat: janela full-screen-na-origem → mesmos pixels de hoje (ktests existentes verdes).
2. **GDI mira o backbuffer da janela:** NtGdiGetFbInfo devolve o cliente da janela; FillRect/TextOut escrevem no backbuffer da janela corrente do processo (coords do cliente, clampadas), não na tela; sem vazar entre janelas/processos.
3. **Múltiplas janelas + foco/z-order:** >= 2 janelas coexistem; clicar levanta+foca; z-order respeitado na composição e no hit-test; lista limitada; destruir janela recompõe.
4. **Roteamento de input correto:** mouse → janela sob o cursor (hit-test por z-order), coords convertidas pra cliente; teclado → janela focada; cada processo só vê os eventos das suas janelas; uma janela full-screen única recebe tudo como nos Planos 22/23 (ktests verdes).
5. **Demonstrável + verificável:** um demo abre 2 janelas sobrepostas num fundo; um ktest compõe e confere o framebuffer (A no rect de A, B no de B, fundo em volta, cursor no topo) e o clique trocando o topo/foco; visualmente funciona no `cargo xtask run`.

---

### Task 1: Kernel — objeto Window + backbuffer + compositor (compat full-screen)

**Files:** Modify `kernel/src/fbcon.rs` (compositor + fundo + cursor por cima) e/ou novo `kernel/src/wm.rs` (lista de janelas, Window{backbuffer,...}, composite, hit-test); `kernel/src/syscall.rs` (NtUserCreateWindow/DestroyWindow/FlushWindow; NtGdi* passam a mirar o backbuffer da janela corrente); `crates/gfx` (blit/clip puro host-testado); `crates/ntdll` (stubs); Modify `kernel/src/ktest.rs`

**Interfaces:**
- `wm`: `Window { id, owner_tid, x, y, w, h, z, focused, visible, backbuffer: Vec<u32> }`; lista global `IrqMutex<Vec<Window>>` (cap <= 32). `create_window(owner, x,y,w,h,style) -> id`; `destroy_window(id)`; `window_client_size(id) -> (w,h)`; `fill_rect_in(id, ...)`/`text_out_in(id, ...)` (desenham no backbuffer, coords de cliente clampadas); `composite()` (sob FBCON lock: fundo + cada janela visível por z + moldura + cursor por cima); `hit_test(x,y) -> Option<id>`; `raise_and_focus(id)`.
- Syscalls: `NtUserCreateWindow(x,y,w,h,style, out_hwnd)` ; `NtUserDestroyWindow(hwnd)` ; `NtUserFlushWindow(hwnd)` (recompõe). `NtGdiGetFbInfo`/`NtGdiFillRect`/`NtGdiTextOut` passam a operar na **janela corrente da thread chamadora** (a última criada por ela, ou um "current window" setado) — ver o split no plano; na Task 1, a thread tem no máx. 1 janela (compat), então "janela corrente" = a janela do processo.
- **Compat full-screen:** se `w>=W && h>=H` (ou style flag de compat), a janela é criada x=0,y=0,w=W,h=H, sem moldura; `composite()` blita full-screen → mesmos pixels de hoje.
- `composite()` reusa o cursor do Plano 24 (hide antes, show depois) e NÃO é chamado quando não há janelas (console/shell: fbcon direto como hoje).

- [ ] **Step 1: Teste que falha** — host: blit/clip puro (janela parcialmente fora da tela; backbuffer→tela). ktest `wm_compose_one`: cria uma janela posicionada (ex. 100,80, 200x150), desenha um retângulo de cor A no backbuffer (coords de cliente), `composite()`, e confere via `fbcon::pixel` que A aparece na posição de TELA certa (x+..,y+..) e o fundo-de-tela aparece fora da janela. E `wm_compose_fullscreen`: janela full-screen-na-origem → conteúdo nos mesmos pixels de tela de antes (compat).
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. clippy/fmt. Confirmar que, com uma janela full-screen, os pixels batem com o modelo antigo (base p/ os ktests de GUI existentes na Task 2).
- [ ] **Step 5: Commit** (`feat(kernel/wm): objeto Window + backbuffer + compositor (fundo+janelas+cursor)`, `Source: docs winuser.h`).

---

### Task 2: user32 multi-janela + GDI no backbuffer; programas existentes verdes

**Files:** Modify `crates/user32/src/lib.rs` (lista de janelas do processo; CreateWindowEx cria via NtUserCreateWindow; BeginPaint/EndPaint miram o backbuffer + flush); Modify `kernel/src/ktest.rs`

**Interfaces:** `user32` deixa de ser janela-única: uma tabela de `{hwnd, wndproc, ...}`. `RegisterClassA` guarda a WndProc por classe. `CreateWindowExA(... x,y,nWidth,nHeight ...)` chama `NtUserCreateWindow` (passando x/y/w/h; CW_USEDEFAULT ou tamanho>=tela → compat full-screen) e guarda o hwnd. `ShowWindow`/`UpdateWindow` → WM_PAINT + flush. `BeginPaint` devolve um HDC que mira o backbuffer da janela (NtGdiGetFbInfo agora é o cliente da janela); `EndPaint` → `NtUserFlushWindow`. `GetMessageA` entrega MSG com o hwnd correto (na Task 2 ainda 1 janela por processo; o roteamento multi-janela completo é a Task 3). FillRect/TextOut (user32/gdi32) desenham no backbuffer da janela corrente. **Os programas existentes (hello_win/test_*/paint_win) criam uma janela full-screen (compat) → mesmos pixels → ktests existentes VERDES.**

- [ ] **Step 1: Teste que falha** — ktest `window_positioned_paints`: um programa GUI de teste cria uma janela em (x,y) com tamanho menor que a tela, pinta um retângulo de cor conhecida (cliente 0,0), e o kernel confere via `fbcon::pixel` que o retângulo aparece na posição de TELA (x,y) e o fundo em volta. RED→GREEN. E rodar os ktests GUI existentes (window_paints etc.) — devem seguir verdes (janela full-screen compat).
- [ ] **Step 2-4:** ver falhar; implementar; ver passar; teste de imports do user32. Confirmar TODOS os ktests GUI existentes verdes.
- [ ] **Step 5: Commit** (`feat(user32): janelas com posição/tamanho + GDI no backbuffer (compat full-screen)`, `Source: docs winuser.h`).

---

### Task 3: Múltiplas janelas + foco/z-order + roteamento de input

**Files:** Modify `kernel/src/wm.rs`/`syscall.rs` (roteamento: mouse→hit-test, teclado→foco; clique levanta+foca; entrega por janela), `crates/user32/src/lib.rs` (GetMessage roteado; coords de cliente), `kernel/src/ktest.rs`

**Interfaces:** o gerenciador roteia a entrada unificada (Plano 23): um `InputEvent::Mouse` → `hit_test(cursor_x,cursor_y)` acha a janela de topo sob o cursor; botão-down → `raise_and_focus`; o evento é marcado para aquela janela (coords convertidas: cliente = tela - (win.x,win.y)). Um `InputEvent::Key` → a janela focada. O `GetMessage`/o syscall de input passa a entregar, para a thread/processo chamador, apenas os eventos destinados às janelas dele (uma fila por janela ou um filtro por owner). z-order usado na composição E no hit-test. Destruir/levantar janela recompõe. Cap de janelas.

- [ ] **Step 1: Teste que falha** — ktest `wm_two_windows_focus`: cria duas janelas sobrepostas (A embaixo, B em cima) de processos/threads de teste (ou um harness que simula 2 janelas); compõe e confere A e B nos seus rects + sobreposição (B por cima); injeta um clique sobre a parte visível de A → A levanta/foca (confere z trocado na composição); injeta teclas → vão pra janela focada; injeta mouse sobre B → roteado pra B com coords de cliente certas. RED→GREEN.
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. Confirmar que uma janela única full-screen recebe todo o input como nos P22/23 (ktests window_keyboard_echo/window_mouse_click/gui_paint_window verdes).
- [ ] **Step 5: Commit** (`feat(kernel+user32): múltiplas janelas, foco/z-order e roteamento de input`, `Source: docs winuser.h`).

---

### Task 4: Demo de 2 janelas + compositor no boot + ROADMAP

**Files:** Create `gui/desk_demo/` (um programa que abre 2 janelas, ou dois programas), Modify `xtask`/`kernel` (feature/boot do demo; compositor ativo no boot GUI; fundo-de-tela), `kernel/src/ktest.rs`, `ROADMAP.md`

- [ ] **Step 1-4:** um demo que abre DUAS janelas num fundo-de-tela (um programa com 2 CreateWindow, ou o boot inicia 2 programas GUI), cada uma com conteúdo próprio, cursor por cima, clicar alterna foco/topo. `cargo xtask run <modo>` mostra o desktop com 2 janelas. ktest `desk_two_windows` (se distinto da Task 3; senão consolidar). Atualizar `ROADMAP.md` (subprojeto 4: **gerenciador de janelas** — compositor + múltiplas janelas + foco/z-order + roteamento ✅; Marco atual; o que falta: mover/redimensionar/decorações ativas/taskbar/minimizar/fechar, dirty-rects, o shell do desktop subprojeto 7). Manter listas consistentes.
- [ ] **Step 5: Commit** (`feat: desktop com 2 janelas (compositor + foco) + demo + ROADMAP`, `Source: docs winuser.h`).

---

## Notas de fechamento

- Marco (subprojeto 4): **gerenciador de janelas** de pé — múltiplas janelas compostas num fundo (z-order), cada uma renderizada off-screen no seu backbuffer, foco e roteamento de input (mouse por hit-test, teclado por foco, clique levanta/foca), cursor por cima; verificado por ktests (composição + roteamento) e visível no `cargo xtask run`. Compatível pra trás (janela full-screen = mesmos pixels).
- Residual/futuro: mover/arrastar (barra de título), redimensionar, minimizar/maximizar/fechar (botões + WM_NCHITTEST), sombras/alpha, taskbar/menu, dirty-rects finos, múltiplos monitores; o shell do desktop (subprojeto 7, design em [[tukos-desktop-design]]).
- Deferreds remanescentes (de antes): objeto de espera real p/ os syscalls bloqueantes (nr17/nr18/NtReadFile); RtlUnwind real (fputs/puts); config byte do i8042 p/ HW real; bound do anchor-loop do ktest mouse_events (/255→/128); refrescar survey doc do P21; BRUSH_TAG duplicado; causa-raiz do frame #GP; limpeza de error-path do cursor_save_under.
- Depois: decorações ativas + mover/redimensionar + taskbar → o desktop do subprojeto 7; ou resto do subprojeto 2 (escrita de arquivos, diretórios, registro).
