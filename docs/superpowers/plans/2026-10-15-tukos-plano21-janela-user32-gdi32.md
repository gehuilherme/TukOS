# TukOS Plano 21 — Uma janela abre (user32 + gdi32 mínimos no framebuffer)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Iniciar o subprojeto 4: um programa **GUI do Windows** (`/MT`, com `WinMain`/message loop) abre uma **janela** e desenha nela — `RegisterClass` + `CreateWindow` + `ShowWindow` + loop `GetMessage`/`DispatchMessage`, com a `WndProc` pintando no `WM_PAINT` (um retângulo preenchido + texto). No TukOS a "janela" é renderizada direto no **framebuffer** (sem gerenciador de janelas/compositing ainda — uma janela única). Demonstrável: no `cargo xtask run`/VMware aparece a janela com o texto; e um ktest **lê o framebuffer pelo kernel** e confere os pixels desenhados (verificação automatizada). É o marco "programa com janela abre" do roadmap.

**Architecture:** Duas crates novas, `crates/user32` e `crates/gdi32` (cdylib `x86_64-pc-windows-msvc`, `#![no_std]`, sem heap do Rust, como o `ntdll`/`kernel32`), importando `ntdll`/`kernel32`. O programa GUI importa `user32`/`gdi32` (+ `kernel32`/CRT). O kernel ganha acesso ao framebuffer em modo usuário: um syscall `NtTukosMapFramebuffer(out *FB_INFO)` que mapeia as páginas físicas do LFB no espaço do processo (`USER|WRITABLE|NO_EXECUTE`) e devolve base/width/height/stride/bpp. O `gdi32` desenha pixels direto no LFB (preenche retângulo, desenha texto com uma **fonte bitmap 8x16 embutida no gdi32**). O `user32` mantém um **modelo de janela única**: `RegisterClassA` guarda a `WndProc`; `CreateWindowExA` cria a janela (área cliente = o LFB inteiro por ora) e devolve um `HWND`; `ShowWindow`/`UpdateWindow` disparam um `WM_PAINT`; o loop `GetMessageA`→`TranslateMessage`→`DispatchMessageA` entrega as mensagens à `WndProc`; `DefWindowProcA` trata o resto; `PostQuitMessage` encerra o loop. Input (teclado) vira `WM_CHAR`/`WM_KEYDOWN` a partir do buffer de console (Plano 15) — mínimo. Clean room nível 0: assinaturas de `user32`/`gdi32` (RegisterClass/CreateWindow/GetMessage/BeginPaint/FillRect/TextOut/...) e estruturas (`WNDCLASS`, `MSG`, `PAINTSTRUCT`, `RECT`) da documentação pública (MSDN/winuser.h/wingdi.h); sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, novas crates `user32`/`gdi32`, `kernel32`/`ntdll`, MSVC no host, QEMU + OVMF + framebuffer. Testes: ktest que roda o programa GUI e lê o framebuffer pelo kernel + demo visual `cargo xtask run`.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 4: user32/gdi32/janela). Sucede o Plano 20, já na `main`. Rumo ao desktop (design em docs/observations/ e na memória).

**Fora deste plano:** gerenciador de janelas / múltiplas janelas / compositing / mover-redimensionar / barra de título real (uma janela única, área = tela); mouse; GDI completo (só fill rect + texto + talvez linha); fontes TrueType (uma bitmap 8x16 embutida); double-buffering; `user32`/`gdi32` completos; o desktop do subprojeto 7. Verificação por conformidade-vs-Windows não se aplica a GUI; usa-se leitura do framebuffer (automatizada) + visual.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (winuser.h/wingdi.h/MSDN públicos; sem MS). `user32`/`gdi32`/`kernel32`/`ntdll` sem heap do Rust; imports só do permitido; verificar com `dumpbin`.
- Invariantes mantidas (faults contidos P14; IAT por-módulo; ULONG as u32; ponteiros validados). O `NtTukosMapFramebuffer` mapeia só o LFB (físico conhecido do kernel) no espaço do processo, `USER|WRITABLE|NO_EXECUTE`, numa faixa de VA de usuário validada sem sobrepor imagens/pilha/heap; um app só rabisca a própria tela (modelo de app único por ora).
- `build.rs`/`xtask`: sem MSVC auto-pula; Linux CI verde. Testes headless (o ktest lê o FB via kernel); `cargo xtask run`/`vmware` mostram a janela.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (kernel ktest + xtask/arch/mm/pe/kbd -D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs winuser.h/wingdi.h (MSDN)`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP (subprojeto 4: janela abre).

## Review Focus

1. **Mapeamento do framebuffer seguro:** `NtTukosMapFramebuffer` mapeia só as páginas do LFB, `USER|RW|NX`, numa faixa de usuário válida sem sobreposição; sem overflow; só o processo atual; devolve info por `copy_to_user`.
2. **gdi32 desenha sem UB:** fill rect e text clampam ao `width`/`height`/`stride` (sem escrita fora do LFB); a fonte bitmap embutida é indexada com bounds; cores no formato do LFB (bpp). `gdi32` importa só ntdll/kernel32.
3. **user32 janela única correto:** `RegisterClassA` guarda a WndProc; `CreateWindowExA` devolve HWND; o loop `GetMessage`/`Dispatch` entrega `WM_PAINT` à WndProc e encerra em `WM_QUIT` (via `PostQuitMessage`); `DefWindowProcA` sensato; sem travar o escalonador (GetMessage bloqueia com yield sem lock). `user32` importa só ntdll/kernel32/gdi32.
4. **Demonstrável + verificável:** um programa GUI pinta um retângulo de cor conhecida numa posição conhecida + um texto; um ktest roda o programa e o **kernel lê o LFB** naquela posição e confere a cor (e opcionalmente alguns pixels do texto); visualmente a janela aparece no `cargo xtask run`.
5. **Sem regressão:** o console/shell/7 conformances seguem; ktests passam.

---

### Task 1: Survey dos imports de um programa GUI mínimo (user32/gdi32)

**Files:** Create `gui/hello_win/hello_win.c` (um Win32 GUI mínimo: `WinMain`→`RegisterClassA`+`CreateWindowExA`+`ShowWindow`+loop; `WndProc` trata `WM_PAINT` (`BeginPaint`+`FillRect`+`TextOutA`+`EndPaint`) e `WM_DESTROY` (`PostQuitMessage`)), `docs/observations/2026-10-15-win32-gui-imports-survey.md`

- [ ] **Step 1:** escrever `hello_win.c` (GUI mínimo, pinta um retângulo + "Hello, TukOS!"). Compilar `/MT` linkando `user32.lib`/`gdi32.lib` (da SDK, só p/ compilar/dumpbin — no TukOS linkaremos as NOSSAS libs depois; aqui só queremos os NOMES importados).
- [ ] **Step 2:** `dumpbin /imports hello_win.exe` → registrar TODOS os imports por DLL (user32.dll, gdi32.dll, kernel32.dll). Esta é a única inspeção (só o NOSSO binário).
- [ ] **Step 3:** triar em `docs/observations/...`: as funções de `user32` (RegisterClassA/CreateWindowExA/ShowWindow/UpdateWindow/GetMessageA/TranslateMessage/DispatchMessageA/DefWindowProcA/BeginPaint/EndPaint/PostQuitMessage/LoadCursorA/.../) e `gdi32` (FillRect? está em user32; TextOutA/SetTextColor/SetBkMode/CreateSolidBrush/GetStockObject/...), com assinaturas MSDN + as structs (WNDCLASSA/EXA, MSG, PAINTSTRUCT, RECT, POINT). Definir o split: T2 (kernel FB + gdi32 draw) vs T3 (user32 janela/loop) vs T4 (programa + verificação).
- [ ] **Step 4/5:** sem implementar; commit (`docs(gui): survey dos imports de um programa Win32 GUI (/MT)`, `Source: blackbox gui/hello_win/hello_win.c`).

---

### Task 2: Kernel `NtTukosMapFramebuffer` + crate `gdi32` (desenho no LFB)

**Files:** Modify `kernel/src/syscall.rs` (NT_MAP_FRAMEBUFFER), `kernel/src/fbcon.rs`/`mem` (expor o físico/tamanho do LFB + mapear no processo), `crates/ntdll/src/lib.rs` (stub); Create `crates/gdi32/` (cdylib)

**Interfaces:**
- `NtTukosMapFramebuffer(out_info_ptr)` (nr livre, ex. 14): mapeia as páginas do LFB no espaço do processo (`USER|WRITABLE|NO_EXECUTE`, numa faixa de VA de usuário validada) e escreve `FB_INFO { base_va: u64, width: u32, height: u32, stride_bytes: u32, bpp: u32 }` via `copy_to_user`. Idempotente/bounded.
- `crates/gdi32` (cdylib msvc no_std, importa ntdll): chama `NtTukosMapFramebuffer` uma vez (cacheia o FB_INFO); exporta um mínimo: `GetDC`/`ReleaseDC`/`BeginPaint`/`EndPaint` (devolvem um HDC que carrega o FB_INFO + a região); `FillRect`/`Rectangle` (preenche RECT com uma cor, clampado ao FB); `TextOutA` (desenha string com uma **fonte bitmap 8x16 embutida** no gdi32); `SetTextColor`/`SetBkColor`/`SetBkMode`; `CreateSolidBrush`/`GetStockObject`/`DeleteObject` (brushes simples, cor). (Alguns desses — BeginPaint/FillRect — na verdade são de user32; alinhar com o survey; implementar no crate que o import indicar.)

- [ ] **Step 1: Teste que falha** — ktest `fb_fill_and_read`: um processo de teste chama NtTukosMapFramebuffer, preenche um retângulo de cor conhecida numa posição conhecida (via gdi32 ou direto), e o **kernel lê o LFB** naquela posição e confere a cor. 
- [ ] **Step 2-4:** ver falhar; implementar o syscall + o gdi32 draw; ver passar. clippy/fmt; `dumpbin` gdi32 importa só ntdll.
- [ ] **Step 5: Commit** (`feat(kernel+gdi32): NtTukosMapFramebuffer + desenho (fill/text) no framebuffer`, `Source: docs wingdi.h`).

---

### Task 3: crate `user32` — janela única + message loop

**Files:** Create `crates/user32/`; Modify `kernel/src/ktest.rs`

**Interfaces:** `crates/user32` (cdylib msvc no_std, importa ntdll/kernel32/gdi32): modelo de janela ÚNICA. `RegisterClassA/W` guarda a `WNDCLASS.lpfnWndProc` (num estático). `CreateWindowExA` cria a janela (área cliente = o FB), guarda o estado, devolve um HWND não-nulo. `ShowWindow`/`UpdateWindow` enfileiram um `WM_PAINT`. `GetMessageA` bloqueia (yield) até haver mensagem (WM_PAINT inicial, teclado→WM_CHAR/WM_KEYDOWN do buffer de console, WM_QUIT) e a devolve no `MSG`. `TranslateMessage` (no-op/mínimo). `DispatchMessageA` chama a `WndProc(hwnd, msg, wParam, lParam)`. `DefWindowProcA` trata o resto (WM_DESTROY→marca quit; WM_PAINT→pinta nada). `PostQuitMessage` faz `GetMessage` devolver 0 (encerra o loop). `BeginPaint`/`EndPaint` (se forem de user32 pelo survey) devolvem um HDC do gdi32. `LoadCursorA`/`LoadIconA`/`RegisterClassExA` stubs plausíveis.

- [ ] **Step 1: Teste que falha** — ktest `window_paints`: um programa GUI de teste (TukOS/MSVC) RegisterClass+CreateWindow+ShowWindow+loop; a WndProc no WM_PAINT preenche um retângulo de cor X e TextOut; o programa sai (PostQuitMessage) após o 1º paint; o kernel lê o LFB e confere a cor X na posição; frames reapeados.
- [ ] **Step 2-4:** ver falhar; implementar user32; ver passar; `dumpbin` user32 importa só ntdll/kernel32/gdi32.
- [ ] **Step 5: Commit** (`feat(user32): janela unica + message loop (RegisterClass/CreateWindow/GetMessage/DispatchMessage)`, `Source: docs winuser.h`).

---

### Task 4: O programa GUI `/MT` roda, abre a janela, pinta + demo + ROADMAP

**Files:** Modify `xtask` (compilar o hello_win linkando as NOSSAS user32/gdi32/kernel32 import libs; embutir; boot mode GUI), `kernel/src/conformance.rs`/`build.rs`/`process.rs` (rodar o GUI exe), `kernel/src/ktest.rs`, `ROADMAP.md`

- [ ] **Step 1-4:** compilar `hello_win.c` `/MT` linkando `user32.dll.lib`/`gdi32.dll.lib`/`kernel32.dll.lib` (as NOSSAS); embutir no initrd; uma feature de boot `gui` que roda o hello_win como processo inicial. ktest `gui_hello_window`: roda o GUI exe pelo caminho multi-módulo (exe→user32→gdi32→kernel32→ntdll), o kernel lê o LFB e confere o retângulo + (opcional) alguns pixels do texto; frames reapeados. `cargo xtask run`/`vmware` (manual): a janela com "Hello, TukOS!" aparece — registrar no report. Atualizar `ROADMAP.md` (subprojeto 4: "programa com janela abre" ✅ mínimo; Marco atual).
- [ ] **Step 5: Commit** (`feat: um programa Win32 GUI abre uma janela e pinta no TukOS; ROADMAP`, `Source: docs winuser.h/wingdi.h`).

---

## Notas de fechamento

- Marco (subprojeto 4, 1º passo): um programa **GUI do Windows** compilado com MSVC abre uma **janela** no TukOS e desenha nela (retângulo + texto), verificado por leitura do framebuffer + visualmente. O caminho `exe→user32→gdi32→kernel32→ntdll` + o framebuffer de usuário estão de pé.
- Residual/futuro: gerenciador de janelas / múltiplas janelas / título-mover-redimensionar / compositing; mouse; GDI mais rico (linhas, bitmaps, fontes TTF); input de teclado→mensagens completo; double-buffering; e o desktop do subprojeto 7 (design esboçado).
- Deferreds remanescentes: a causa-raiz do frame corrompido (#GP, defendido mas não isolado — docs/observations/2026-10-15); RtlUnwind real (fputs/puts); P17 (build_space panica)/P18 (reap-queue)/etc.
- Depois: enriquecer o subprojeto 4 (WM, mais GDI, mouse) rumo ao desktop; ou voltar ao resto do subprojeto 2 (escrita de arquivos, registro).
