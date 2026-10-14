# TukOS Plano 12 — Corrigir o #PF intermitente no reaping/scheduler

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:systematic-debugging (ROOT CAUSE antes de qualquer fix) junto com superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Eliminar o `#PF` (page fault) intermitente do kernel que aparece ~1 em 4-6 execuções de `cargo xtask conformance`, **após** o marcador de fim, quando o único processo de usuário já saiu/foi reapeado. O bug é **pré-existente** (não introduzido pelo Plano 11; `sched.rs`/`space.rs`/`frames.rs` são idênticos à main anterior) e precisa de **causa-raiz confirmada** (não só a hipótese estática) antes de qualquer correção, um **reprodutor determinístico**, e verificação de que a correção acaba com o #PF sem regressão.

**Hipótese de partida (a CONFIRMAR, não assumir — Iron Law):** em `kernel/src/sched.rs` `switch_to_next` (~L195-203), quando o único thread Ready é o atual (a thread de usuário `Exited` ainda está no `Vec`, então `len>=2` e não há early-return), o código calcula `to == from` e chama `switch_context(save=&rsp[cur], load=rsp[cur])`, que **não** é no-op: salva o novo `rsp` mas recarrega um `rsp` salvo ANTIGO e `ret`a para um frame obsoleto de `switch_to_next`/`yield_now` na pilha do `boot`; se o corpo do loop já sobrescreveu aquela região, o `pop`+`ret` cai em lixo → `#PF`. Possível correção: `switch_to_next` tratar `to==from` como no-op verdadeiro (early-return quando o alvo escolhido é o thread atual) e/ou reapear a thread `Exited` antes de escolher o próximo. **O investigador deve confirmar o mecanismo real com evidência antes de corrigir.**

**Architecture:** puramente de kernel (scheduler/context-switch/reaper). Sem mudança de ABI nem de superfície de usuário. Código sensível (asm de troca de contexto) — mudança mínima e revisada.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `crates/arch-x86_64` (context switch), QEMU + OVMF. Reprodução via `cargo xtask ktest`/`conformance` (headless).

**Spec:** n/a (correção de defeito). Sucede o Plano 11 (heap de usuário), já na `main`. O bug foi destacado pela revisão final do Plano 11.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. Apenas x86-64; código de CPU em `crates/arch-x86_64`.
- **systematic-debugging obrigatório:** Fase 1 (causa-raiz com evidência) antes de propor fix; reprodutor determinístico antes da correção; UMA mudança mínima que ataca a raiz; verificação.
- Não introduzir regressão: todas as variantes de `cargo xtask ktest` seguem passando; `cargo xtask conformance` (quando há MSVC) passa de forma **estável** (0 falhas em N execuções).
- `cargo fmt --all -- --check`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings` limpos ao fim de cada tarefa.
- Clean room nível 0 (sem material da Microsoft). Comentários em inglês; docs/mensagens em português.
- Commits declaram a fonte (`Source: observation docs/observations/<arquivo>` para o diagnóstico; ou `Source: blackbox`/`docs` conforme o caso) e terminam com `Co-Authored-By:` do modelo.

## Review Focus

1. **A causa-raiz foi CONFIRMADA com evidência** (instrumentação/log/assert mostrando o `to==from` ou o mecanismo real), não só deduzida. O reprodutor falha de forma determinística ANTES do fix.
2. **O fix ataca a raiz, é mínimo, e não quebra a troca de contexto:** o caminho `to==from` (ou o reaping) é corrigido sem alterar o salvamento/restауração de callee-saved nem o alinhamento de pilha do `switch_context`; o caso normal (2+ threads distintos) continua idêntico.
3. **Sem regressão:** todas as variantes de ktest passam; o cenário "única thread restante após exit" é exercitado deterministicamente e NÃO dá #PF; `conformance` roda estável em N repetições.
4. **Reaping correto:** a thread `Exited` é removida/reapeada sem liberar um PML4/stack/fxarea ainda em uso (invariante do Plano 4/9: nunca liberar o CR3 ativo nem a fxarea do thread corrente).

---

### Task 1: Reproduzir deterministicamente + confirmar a causa-raiz (SEM corrigir ainda)

**Files:**
- Create: `docs/observations/2026-10-14-scheduler-reap-pf.md` (registro do diagnóstico)
- Modify (temporário/instrumentação): `kernel/src/sched.rs`, `kernel/src/ktest.rs` (um ktest reprodutor)

**Objetivo:** Fase 1-3 do systematic-debugging. NÃO corrigir o bug nesta tarefa — só provar o mecanismo.

- [ ] **Step 1: Reprodutor determinístico**

Escreva um ktest (ex.: `("single_thread_after_exit_no_fault", ...)`) que recrie o cenário sem depender do MSVC/conformance: spawna UMA thread de usuário (ou kernel) que sai imediatamente, e então o escalonador continua rodando com a thread `Exited` ainda no `Vec` e o `boot`/idle como único Ready — forçando o caminho `switch_to_next` a escolher `to==from`. Rode via `cargo xtask ktest` e demonstre o fault/instabilidade de forma consistente (se não faltar deterministicamente, instrumente — veja Step 2 — e rode em laço até evidenciar). Documente a taxa e os passos exatos.

- [ ] **Step 2: Evidência da causa-raiz**

Instrumente `switch_to_next`/`switch_context` para registrar (serial) quando `to==from`, o `rsp` salvo vs. carregado, e o estado do `Vec` de threads (quantos Ready/Exited). Rode o reprodutor e o `cargo xtask conformance` (se houver MSVC) e capture a evidência mostrando que o #PF coincide com o self-switch `to==from` (ou revele o mecanismo REAL, se for outro — p.ex. reaping liberando a fxarea/stack do thread corrente, ou a ordem reap vs. switch). Se a evidência apontar outra raiz, documente-a — a hipótese é só um ponto de partida.

- [ ] **Step 3: Registrar o diagnóstico**

Escreva `docs/observations/2026-10-14-scheduler-reap-pf.md` com: sintoma, reprodutor, evidência coletada, causa-raiz CONFIRMADA, e a correção proposta (mínima). Este documento é a fonte do commit do fix (`Source: observation ...`).

- [ ] **Step 4: Rodar e ver o estado**

Run: `cargo xtask ktest` — o reprodutor evidencia o bug (falha/instrumentação). Nenhuma correção ainda. A instrumentação pode ficar atrás de um feature/flag de debug se poluir a saída normal; remova ou silencie antes do fim do plano.

- [ ] **Step 5: Commit (diagnóstico + reprodutor)**

```bash
git add docs/observations kernel
git commit -m "test(sched): reprodutor deterministico do #PF de reaping + diagnostico da causa-raiz

Source: observation docs/observations/2026-10-14-scheduler-reap-pf.md
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Corrigir a causa-raiz (mínimo) + verificar estabilidade

**Files:**
- Modify: `kernel/src/sched.rs` (e/ou `crates/arch-x86_64/src/*` se o `switch_context` precisar), `kernel/src/ktest.rs` (transformar o reprodutor em teste de regressão verde)

**Interfaces:** sem mudança de API pública. O comportamento: quando o alvo escolhido é o thread atual, `switch_to_next` não executa um `switch_context` destrutivo (early-return/no-op); a thread `Exited` é reapeada de forma segura (sem liberar CR3/stack/fxarea em uso). Preserve a semântica do caso 2+ threads distintos.

- [ ] **Step 1: Teste que falha**

Garanta que o ktest reprodutor da Task 1 (`single_thread_after_exit_no_fault`) esteja no formato de regressão: PASS = o cenário roda sem #PF e o escalonador continua são (a thread restante segue executável; o processo foi reapeado; frames recuperados). Antes do fix ele falha/fica instável.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → o reprodutor falha (ou evidencia o fault) de forma determinística.

- [ ] **Step 3: Implementar o fix mínimo**

Aplique a correção MÍNIMA que ataca a causa-raiz confirmada na Task 1 (p.ex.: `switch_to_next` detecta `to==from` e retorna sem `switch_context`; e/ou reapeia a thread `Exited` antes de selecionar, de modo que o único Ready vire o caminho idle já existente). NÃO altere o salvamento de callee-saved nem o alinhamento de pilha do `switch_context`. UMA mudança por vez; sem "while I'm here". Remova/silencie a instrumentação temporária da Task 1.

- [ ] **Step 4: Rodar e ver passar + estabilidade**

Run: `cargo xtask ktest` → `single_thread_after_exit_no_fault` passa; TODAS as variantes passam. Rode `cargo xtask conformance` (se houver MSVC) **N vezes** (ex.: 20-30 execuções em laço) e confirme **0** #PF. Documente a contagem (ex.: "30/30 PASS"). clippy/fmt limpos.

- [ ] **Step 5: Commit**

```bash
git add kernel crates/arch-x86_64
git commit -m "fix(sched): trata o self-switch (to==from) como no-op; elimina o #PF de reaping

Source: observation docs/observations/2026-10-14-scheduler-reap-pf.md
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim: o `#PF` intermitente não reproduz mais (N/N execuções de `conformance` limpas); existe um ktest de regressão determinístico; o diagnóstico está em `docs/observations/`. A troca de contexto no caso normal (2+ threads) permanece inalterada.
- Se a Fase 1 revelar que a causa-raiz NÃO é o self-switch (p.ex. reaping liberando recurso em uso, ou uma corrida com o timer), corrija a raiz REAL encontrada — a hipótese é só o ponto de partida; o diagnóstico documentado manda.
- Deferreds remanescentes (de planos anteriores, não deste): P9 #1 (re-proteger IAT RO pós-bind), P9 #4 (double-map SizeOfHeaders), P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked = self-only; TOCTOU futura com SMP; first-fit O(n)/segmentos nunca devolvidos).
- Depois deste plano: **Plano 13** (degrau 3b completo) — surveyar os imports do `mainCRTStartup` (dumpbin) e implementar o subconjunto do `kernel32`/`ntdll` que o CRT estático (`libcmt`) exige, rodar um `hello.exe` compilado **com o CRT** do MSVC no TukOS e conformar.
