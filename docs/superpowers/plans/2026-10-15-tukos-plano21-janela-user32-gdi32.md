# TukOS Plano 21 — Uma janela abre (user32 + gdi32 mínimos no framebuffer)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Iniciar o subprojeto 4: um programa **GUI do Windows** (`/MT`, com `WinMain`/message loop) abre uma **janela** e desenha nela — `RegisterClass` + `CreateWindow` + `ShowWindow` + loop `GetMessage`/`DispatchMessage`, com a `WndProc` pintando no `WM_PAINT` (um retângulo preenchido + texto). No TukOS a "janela" é renderizada direto no **framebuffer** (sem gerenciador de janelas/compositing ainda — uma janela única). Demonstrável: no `cargo xtask run`/VMware aparece a janela com o texto; e um ktest **lê o framebuffer pelo kernel** e confere os pixels desenhados (verificação automatizada). É o marco "programa com janela abre" do roadmap.

**Architecture:** Duas crates novas, `crates/user32` e `crates/gdi32` (cdylib `x86_64-pc-windows-msvc`, `#![no_std]`, sem heap do Rust, como o `ntdll`/`kernel32`), importando `ntdll`/`kernel32`. O programa GUI importa `user32`/`gdi32` (+ `kernel32`/CRT). **Desenho por syscall (não mapeia o LFB no usuário):** o kernel desenha no framebuffer por conta do `gdi32` via syscalls novos — `NtGdiGetFbInfo(out *FB_INFO{width,height,bpp})`, `NtGdiFillRect(x,y,w,h,color)`, `NtGdiTextOut(x,y,ptr,len,fg,bg)` — reaproveitando o acesso ao framebuffer do `fbcon` e a fonte `noto-sans-mono` já no kernel; o kernel **clampa** cada operação aos limites do FB. O `gdi32` é um shim fino sobre esses syscalls (sem fonte embutida, sem MMIO mapeado no usuário). *(Motivo: o LFB é memória de dispositivo (MMIO); mapeá-lo no espaço do processo faria o teardown do processo (`vmem::free_in`/saída) devolver esses frames de dispositivo ao alocador de RAM e corrompê-lo. Um back-buffer mapeado pode vir num plano futuro quando a performance — desktop/jogos — exigir.)* O `user32` mantém um **modelo de janela única**: `RegisterClassA` guarda a `WndProc`; `CreateWindowExA` cria a janela (área cliente = a tela inteira por ora) e devolve um `HWND`; `ShowWindow`/`UpdateWindow` disparam um `WM_PAINT`; o loop `GetMessageA`→`TranslateMessage`→`DispatchMessageA` entrega as mensagens à `WndProc`; `DefWindowProcA` trata o resto; `PostQuitMessage` encerra o loop. `HDC` é um handle opaco de "tela" (um único DC); `BeginPaint`/`GetDC` devolvem esse handle e as funções de `gdi32` o interpretam como a tela. Input (teclado) vira `WM_CHAR`/`WM_KEYDOWN` a partir do buffer de console (Plano 15) — mínimo. Clean room nível 0: assinaturas de `user32`/`gdi32` (RegisterClass/CreateWindow/GetMessage/BeginPaint/FillRect/TextOut/...) e estruturas (`WNDCLASS`, `MSG`, `PAINTSTRUCT`, `RECT`) da documentação pública (MSDN/winuser.h/wingdi.h); sem binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, novas crates `user32`/`gdi32`, `kernel32`/`ntdll`, MSVC no host, QEMU + OVMF + framebuffer. Testes: ktest que roda o programa GUI e lê o framebuffer pelo kernel + demo visual `cargo xtask run`.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 4: user32/gdi32/janela). Sucede o Plano 20, já na `main`. Rumo ao desktop (design em docs/observations/ e na memória).

**Fora deste plano:** gerenciador de janelas / múltiplas janelas / compositing / mover-redimensionar / barra de título real (uma janela única, área = tela); mouse; GDI completo (só fill rect + texto + talvez linha); fontes TrueType (uma bitmap 8x16 embutida); double-buffering; `user32`/`gdi32` completos; o desktop do subprojeto 7. Verificação por conformidade-vs-Windows não se aplica a GUI; usa-se leitura do framebuffer (automatizada) + visual.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (winuser.h/wingdi.h/MSDN públicos; sem MS). `user32`/`gdi32`/`kernel32`/`ntdll` sem heap do Rust; imports só do permitido; verificar com `dumpbin`.
- Invariantes mantidas (faults contidos P14; IAT por-módulo; ULONG as u32; ponteiros validados). Os syscalls de GDI NÃO mapeiam o LFB no usuário: o kernel desenha no framebuffer e **clampa** cada operação (x,y,w,h, texto) aos limites do FB; `NtGdiGetFbInfo` escreve o `FB_INFO` por `copy_to_user`; `NtGdiTextOut` valida o ponteiro/len da string via usercopy. Um app só desenha na própria tela (modelo de app único por ora).
- `build.rs`/`xtask`: sem MSVC auto-pula; Linux CI verde. Testes headless (o ktest lê o FB via kernel); `cargo xtask run`/`vmware` mostram a janela.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (kernel ktest + xtask/arch/mm/pe/kbd -D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs winuser.h/wingdi.h (MSDN)`; `Co-Authored-By:` do modelo. Ao fim, ROADMAP (subprojeto 4: janela abre).

## Review Focus

1. **Syscalls de GDI seguros:** `NtGdiGetFbInfo`/`NtGdiFillRect`/`NtGdiTextOut` validam e **clampam** toda operação aos limites do FB (sem escrita fora do framebuffer, sem overflow de x+w/y+h); `NtGdiTextOut` lê a string do usuário via usercopy (sem deref cru de ponteiro de usuário); `FB_INFO` devolvido por `copy_to_user`. Nenhuma memória de dispositivo (MMIO) é mapeada no espaço do usuário.
2. **gdi32 é shim fino sem UB:** `gdi32` só repassa aos syscalls (sem fonte embutida, sem tocar pixels direto); `HDC` é um handle opaco de tela; cores/estado de DC (fg/bk) consistentes. `gdi32` importa só ntdll (e kernel32 se o survey exigir).
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

### Task 2: Syscalls de GDI no kernel + crate `gdi32` (shim de desenho)

**Files:** Modify `kernel/src/syscall.rs` (NT_GDI_GET_FB_INFO / NT_GDI_FILL_RECT / NT_GDI_TEXT_OUT — nrs livres, ex. 14/15/16), `kernel/src/fbcon.rs` (expor `fb_info()` e desenho público `fill_rect`/`text_out` clampados, reaproveitando o `put_pixel`/fonte), `crates/ntdll/src/lib.rs` (stubs dos 3 syscalls); Create `crates/gdi32/` (cdylib)

**Decisão (ruling de pré-voo):** o kernel desenha por conta do gdi32 via syscalls; o LFB NÃO é mapeado no usuário (evita devolver frames MMIO ao alocador no teardown). gdi32 é um shim fino.

**Interfaces:**
- Kernel syscalls (ABI NT como os demais; args ULONG mascarados `as u32`; o kernel clampa tudo aos limites do FB):
  - `NtGdiGetFbInfo(out_info_ptr)`: escreve `FB_INFO { width: u32, height: u32, bpp: u32 }` via `copy_to_user`.
  - `NtGdiFillRect(x, y, w, h, color_argb)`: preenche o retângulo (clampado) no framebuffer.
  - `NtGdiTextOut(x, y, str_ptr, len, fg, bg)`: lê a string (ASCII) do usuário via usercopy e desenha com a fonte `noto-sans-mono` do kernel (clampado).
  - `fbcon` ganha `pub fn fb_info()`, `pub fn fill_rect(...)`, `pub fn text_out(...)` sob o mesmo `IrqMutex` (o `pixel()` já existe p/ o ktest ler de volta).
- `crates/gdi32` (cdylib msvc no_std, importa ntdll): shim fino. Pelo survey (task-1), os exports de gdi32 são **CreateSolidBrush, GetStockObject, DeleteObject, SetBkMode, SetTextColor, TextOutA**. `TextOutA`→`NtGdiTextOut` (com fg/bk do DC); `SetTextColor`/`SetBkMode` guardam estado do DC; `CreateSolidBrush`/`GetStockObject`/`DeleteObject` devolvem/gerenciam brushes simples (cor). `HDC`/`HBRUSH` são handles opacos. (FillRect/BeginPaint/EndPaint são de USER32 pelo survey → vão na Task 3, mas também desenham via `NtGdiFillRect`/`NtGdiGetFbInfo`.)

- [ ] **Step 1: Teste que falha** — host: a matemática de clamp de `fill_rect`/`text_out` (pura) em teste de host. ktest `fb_fill_and_read`: chamar `NtGdiFillRect` (de um stub de usuário ou direto no caminho de syscall) numa posição/cor conhecida e o **kernel lê o LFB** (`fbcon::pixel`) e confere a cor; conferir clamp (rect além da borda não escreve fora).
- [ ] **Step 2-4:** ver falhar; implementar os syscalls + fbcon draw + o gdi32 shim; ver passar. clippy/fmt; `dumpbin` gdi32 importa só ntdll (e kernel32 se o survey exigir).
- [ ] **Step 5: Commit** (`feat(kernel+gdi32): syscalls de GDI (fill/text) no framebuffer + shim gdi32`, `Source: docs wingdi.h`).

---

### Task 3: crate `user32` — janela única + message loop

**Files:** Create `crates/user32/`; Modify `kernel/src/ktest.rs`

**Interfaces:** `crates/user32` (cdylib msvc no_std, importa ntdll/kernel32/gdi32): modelo de janela ÚNICA. `RegisterClassA/W` guarda a `WNDCLASS.lpfnWndProc` (num estático). `CreateWindowExA` cria a janela (área cliente = o FB), guarda o estado, devolve um HWND não-nulo. `ShowWindow`/`UpdateWindow` enfileiram um `WM_PAINT`. `GetMessageA` bloqueia (yield) até haver mensagem (WM_PAINT inicial, teclado→WM_CHAR/WM_KEYDOWN do buffer de console, WM_QUIT) e a devolve no `MSG`. `TranslateMessage` (no-op/mínimo). `DispatchMessageA` chama a `WndProc(hwnd, msg, wParam, lParam)`. `DefWindowProcA` trata o resto (WM_DESTROY→marca quit; WM_PAINT→pinta nada). `PostQuitMessage` faz `GetMessage` devolver 0 (encerra o loop). Pelo survey, `BeginPaint`/`EndPaint` e `FillRect` são de **USER32**: `BeginPaint`/`EndPaint` devolvem/soltam o `HDC` opaco de tela (via `NtGdiGetFbInfo`), preenchendo o `PAINTSTRUCT`; `FillRect` desenha via `NtGdiFillRect`. `LoadCursorA`/`LoadIconA`/`RegisterClassExA` stubs plausíveis. Os 13 imports de user32 do survey (task-1) são o conjunto a cobrir.

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
