# TukOS Plano 24 — Cursor de mouse visível

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Desenhar um **cursor de mouse visível** (sprite de seta) na tela, que **acompanha o mouse** (Plano 23) e fica **sempre por cima** do que o app desenha — sem corromper o conteúdo embaixo. Fecha o deferred do Plano 23 e torna o mouse usável ao vivo. Demonstrável: em `cargo xtask run gui-paint` o cursor de seta aparece e se move com o mouse; arrastar ainda pinta a trilha correta (o cursor não deixa rastro nem apaga a pintura). Verificado headless: um ktest move o cursor e confere que os pixels da seta estão na nova posição, que a posição antiga foi restaurada ao conteúdo de baixo, e que desenhar sob o cursor + mover o cursor preserva o desenho.

**Architecture:** O kernel (fbcon) ganha uma **camada de cursor** topmost com **save-under**: um buffer pequeno (do tamanho do sprite, ex. 12×20) guarda os pixels sob o cursor; `cursor_show_at(x,y)` salva o fundo e desenha a seta; `cursor_hide()` restaura o fundo. Como TODO desenho na tela passa pelo fbcon (o kernel), o cursor fica topmost **envolvendo cada mutação do framebuffer** (fill_rect/text_out/write_str/clear/scroll) num padrão "esconde-antes / redesenha-depois", TUDO sob a MESMA aquisição do `FBCON` lock (sem re-lock): a op recupera o fundo sob o cursor, desenha o conteúdo do app (que pode cair sob o cursor), depois re-salva o novo fundo e redesenha a seta por cima — então o conteúdo embaixo é preservado e a seta nunca é apagada. O cursor **acompanha o mouse**: quando o pacote PS/2 atualiza a posição `MOUSE{x,y}` (Plano 23, em `kbd::push_mouse_byte`), chama `fbcon::cursor_move_to(x,y)` (esconde na posição antiga, mostra na nova) — então a seta segue o mouse físico independente do app ler eventos. O cursor é **desligado por padrão** (console/shell: nunca mostrado → zero overhead e comportamento idêntico); é **ligado** no modo GUI (quando um programa GUI roda / no primeiro evento de mouse em modo GUI). Sprite da seta = bitmap monocromático público embutido (sem asset da MS). Clean room nível 0.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), x86-64. `kernel/src/fbcon.rs` (camada de cursor + save-under + wrapping das mutações), `kernel/src/kbd.rs` (mover o cursor no update da posição do mouse), talvez `crates/gfx` (lógica pura de region save/restore se extraível, host-testada). Testes: host (math de save/restore se pura), ktest (mover cursor; desenhar sob o cursor) — headless; só `cargo xtask run gui*` abre janela.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 4: cursor). Sucede o Plano 23 (mouse nas mensagens), na `main` @ ac59c29. Rumo ao desktop (design em docs/observations/ e na memória).

**Fora deste plano:** backbuffer por-janela / compositor completo / múltiplas janelas / z-order / foco / hit-testing (próximo arco); cursores customizados por janela (SetCursor/LoadCursor real — hoje LoadCursorA é stub); formas de cursor (I-beam, hand); hotspot configurável além do canto; aceleração; roda/scroll; o desktop do subprojeto 7.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (sprite de seta = bitmap próprio/público; sem asset/binário da MS).
- **Correção acima de velocidade:** o conteúdo sob o cursor é SEMPRE preservado; a seta é SEMPRE topmost após qualquer desenho; sem rastro ("trail") ao mover; sem apagar a pintura do app. Toda a sequência esconde/desenha/mostra de uma op roda sob UMA aquisição do `FBCON` lock (sem re-lock, sem deadlock). O save-under respeita os limites da tela (cursor perto da borda: clampa o sprite/região salva).
- **NÃO regredir:** console/shell (cursor desligado → fbcon idêntico ao de hoje, mesma saída dos ktests de console/conformance), os 7 conformances, TODOS os ktests (incl. window_paints/gui_hello_window/window_keyboard_echo/window_mouse_click/gui_paint_window — estes rodam com ou sem cursor; se o cursor ligado muda os pixels que eles conferem, ligar o cursor só onde não colide, ou garantir que o cursor não cobre as regiões que os ktests amostram). O pixel readback desses ktests NÃO pode ser poluído pela seta.
- Mover o cursor a partir de `kbd::poll`/`push_mouse_byte` (contexto do tick ISR e de thread) usa o `FBCON` IrqMutex (interrupts off enquanto travado) — sem segurar outro lock atravessando; sem deadlock com as mutações de GDI que também travam FBCON.
- Testes HEADLESS; só `cargo xtask run gui*`/`vmware gui*` abrem janela — agentes NÃO rodam esses.
- Gates por tarefa: `cargo fmt --all -- --check`, clippy (-D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs` (sprite próprio); `Co-Authored-By:` do modelo. Ao fim, ROADMAP. Claude NÃO dá push.

## Review Focus

1. **Save-under correto:** o fundo sob o cursor é salvo antes de desenhar a seta e restaurado exatamente ao esconder; mover não deixa rastro; perto das bordas clampa (sem escrita/leitura fora do framebuffer). Buffer dimensionado pro sprite.
2. **Topmost sem corromper:** toda mutação do fbcon (fill_rect/text_out/write_str/clear/scroll) esconde o cursor antes e o redesenha depois, numa única aquisição do lock; o conteúdo do app que cai sob o cursor é preservado (restaurado→desenhado→re-salvo); a seta nunca é apagada nem duplicada.
3. **Sem regressão de console/ktests:** cursor desligado por padrão → console/shell/conformance idênticos; os ktests de GUI com readback de pixel não leem pixels da seta (cursor fora das regiões amostradas, ou desligado nesses ktests, ou as amostras escolhidas não colidem).
4. **Sem deadlock/corrida:** cursor_move_to (do tick/poll) e as mutações de GDI (de thread) serializam pelo FBCON IrqMutex; nenhuma segura outro lock atravessando; a sequência interna não re-trava FBCON.
5. **Demonstrável + verificável:** ktest move o cursor e confere seta na nova posição + fundo restaurado na antiga + desenho sob o cursor preservado; visualmente a seta aparece e segue o mouse no `cargo xtask run gui-paint`.

---

### Task 1: Camada de cursor no fbcon (sprite + save-under + topmost)

**Files:** Modify `kernel/src/fbcon.rs` (camada de cursor: sprite da seta, save-under buffer, `cursor_enable()`/`cursor_disable()`/`cursor_show_at`/`cursor_hide`/`cursor_move_to`; envolver cada mutação com hide/redraw sob o mesmo lock); talvez `crates/gfx` (math pura de região, host-testada); Modify `kernel/src/ktest.rs`

**Interfaces:**
- Sprite: um bitmap de seta pequeno (ex. 12×19), máscara (pixels da seta = branco/preto com borda), embutido como `const`. Hotspot no canto superior-esquerdo (ponta da seta).
- `FbCon` ganha: `cursor_enabled: bool`, `cursor_x/y`, `cursor_shown: bool`, e um `save_under: [u32; W*H]`. `cursor_show()` (salva o fundo clampado na posição, desenha a seta por cima), `cursor_hide()` (restaura o fundo), `cursor_move_to(x,y)` (hide no antigo, atualiza pos, show no novo), `cursor_enable/disable`. TODAS as mutações públicas (`fill_rect`, `text_out`, `write_str`, `clear`, `scroll`) passam a: se `cursor_shown` → `cursor_hide()`; fazer a mutação; se `cursor_enabled` → `cursor_show()` — tudo dentro do bloqueio existente do FBCON (refatorar p/ métodos internos sem re-lock). `pixel(x,y)` (usado por ktests) lê o framebuffer cru como hoje.
- Clampagem: perto da borda, salvar/desenhar só a parte visível do sprite.

- [ ] **Step 1: Teste que falha** — host (se a math de clamp de região for extraída pura): salvar/restaurar região clampada. ktest `cursor_save_under`: habilita o cursor; pinta um retângulo de cor A sob onde o cursor vai; `cursor_show_at(x,y)`; confere que os pixels da seta aparecem em (x,y) e que fora do sprite o A continua; `cursor_hide()`; confere que (x,y) voltou a A (fundo restaurado); `cursor_move_to` p/ nova pos e confere seta nova + antiga restaurada. Deixa o cursor desligado + framebuffer limpo ao fim (p/ não poluir outros ktests).
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. clippy/fmt. Confirmar que os ktests de console/GUI existentes seguem verdes (cursor desligado por padrão).
- [ ] **Step 5: Commit** (`feat(kernel/fbcon): camada de cursor de mouse (sprite + save-under topmost)`, `Source: docs`).

---

### Task 2: O cursor acompanha o mouse

**Files:** Modify `kernel/src/kbd.rs` (chamar `fbcon::cursor_move_to` quando a posição do mouse muda), `kernel/src/process.rs`/`conformance.rs` ou onde o modo GUI inicia (habilitar o cursor ao rodar um programa GUI); Modify `kernel/src/ktest.rs`

**Interfaces:** em `push_mouse_byte`, após atualizar `MOUSE{x,y}` com o pacote, chamar `crate::fbcon::cursor_move_to(x, y)` (só tem efeito se o cursor está habilitado). Habilitar o cursor (`fbcon::cursor_enable()` + show inicial no centro) quando um processo GUI é iniciado (feature gui/gui-type/gui-paint, ou no spawn_exe_image_with_gui). O cursor NÃO é habilitado no shell/console.

- [ ] **Step 1: Teste que falha** — ktest `cursor_follows_mouse`: habilita o cursor (ou simula início de GUI), injeta pacotes de mouse de um movimento conhecido (via o hook do Plano 23), e confere que os pixels da seta estão na nova posição do mouse e a posição anterior foi restaurada ao fundo. Limpa ao fim.
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. Confirmar window_mouse_click/gui_paint_window ainda verdes (o cursor habilitado nesses caminhos não pode poluir os pixels que eles amostram — se poluir, escolher amostras/posições que não colidem com a seta, ou posicionar o cursor fora da área de teste).
- [ ] **Step 5: Commit** (`feat(kernel): o cursor de mouse acompanha o mouse (cursor_move_to no update de posicao)`, `Source: docs`).

---

### Task 3: Cursor nos demos + ROADMAP

**Files:** Modify `kernel`/`xtask` se preciso (garantir cursor ligado nos boots gui/gui-type/gui-paint), `kernel/src/ktest.rs` (consolidar se útil), `ROADMAP.md`

- [ ] **Step 1-4:** garantir que nos 3 boots GUI (`gui`/`gui-type`/`gui-paint`) o cursor aparece e segue o mouse (sem rodar os targets com janela — confirmar pelo caminho de código + ktest). Se algum ktest de GUI passou a amostrar pixels onde a seta inicial fica, reposicionar a seta inicial (ex. canto) ou as amostras. Atualizar `ROADMAP.md` (subprojeto 4: **cursor de mouse visível** ✅; Marco atual; o que falta: múltiplas janelas / compositor / z-order / foco / cursores customizados / scroll). Nota: o cursor é topmost via save-under no modelo de janela única; o compositor por-janela vem com o gerenciador de janelas.
- [ ] **Step 5: Commit** (`feat: cursor de mouse visível nos demos GUI + ROADMAP`, `Source: docs`).

---

## Notas de fechamento

- Marco (subprojeto 4): **cursor de mouse visível** — a seta aparece, segue o mouse e fica por cima sem corromper o que o app desenha; verificado por ktest (save-under + follow) e visível no `cargo xtask run gui-paint`. O mouse ficou usável ao vivo.
- Residual/futuro: backbuffer por-janela + compositor + múltiplas janelas + z-order + foco + hit-testing (o gerenciador de janelas — próximo arco rumo ao desktop); cursores customizados (SetCursor/LoadCursor real); formas de cursor; scroll/roda; duplo-clique; o desktop do subprojeto 7.
- Deferreds remanescentes (de antes): objeto de espera real p/ os syscalls bloqueantes (nr17/nr18/NtReadFile); bound do anchor-loop do ktest mouse_events (/255→/128); RtlUnwind real (fputs/puts); config byte do i8042 p/ HW real; refrescar survey doc do P21; BRUSH_TAG duplicado; causa-raiz do frame #GP.
- Depois: o gerenciador de janelas (múltiplas janelas + compositor + foco) rumo ao desktop (subprojeto 7); ou resto do subprojeto 2 (escrita de arquivos, diretórios, registro).
