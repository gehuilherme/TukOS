# TukOS Plano 18 — Reciclar slots de thread Exited (pré-requisito do shell)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:systematic-debugging (reproduzir antes de corrigir) + superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** O reaper libera os frames/AddressSpace/stack de uma thread que sai, mas **nunca libera o slot dela na tabela de threads (Vec de 64)**. Resultado: após ~64 spawns ao longo da vida, `NT_CREATE_PROCESS`/spawn devolve `INSUFFICIENT_RESOURCES`. Um shell (Plano 19) que roda muitos comandos esgotaria isso rápido. Este plano faz o reaper **reciclar o slot**: uma thread reapeada vira um slot livre que um spawn futuro reusa — mantendo os índices estáveis (o escalonador referencia threads por índice) e a disciplina de teardown dos Planos 4/12/14 intacta.

**Causa-raiz (CONFIRMADA nas revisões do Plano 17):** `reap_pending` (sched.rs) libera `space`/`_stack` da thread marcada, mas a entrada do `Vec<Thread>` permanece (estado `Exited`); `threads.len()` nunca encolhe; `has_thread_capacity` bloqueia em 64. Os índices são usados por `s.current`, `s.reap`, e o round-robin (`to = (from+1)%n`) — por isso NÃO se pode simplesmente remover do Vec (deslocaria índices). Fix: introduzir um estado `Free`/tombstone no slot; o reaper, após liberar os recursos, marca o slot `Free`; `spawn_*` procura um slot `Free` antes de crescer o Vec; `switch_to_next` pula `Free` como já pula `Exited`.

**Architecture:** puramente de kernel (scheduler/reaper/spawn). Sem mudança de ABI nem de superfície de usuário. Código sensível (índices do escalonador + ciclo de vida) — mudança mínima e revisada, com teste de regressão que spawna/sai muitas vezes (>64) e confirma reuso sem esgotar nem corromper.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, QEMU + OVMF. Testes via `cargo xtask ktest` (headless).

**Spec:** n/a (robustez). Sucede o Plano 17, já na `main`. Pré-requisito do Plano 19 (shell).

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- systematic-debugging: reprodutor determinístico (hoje esgota em ~64) ANTES do fix; mudança mínima na raiz; verificação.
- Invariantes mantidas (Planos 3/4/12/14/17): nunca liberar CR3/stack/fxarea da thread ATIVA; reaper roda após o switch, na pilha da próxima thread; self-switch `to==from` é no-op (P12); faults de usuário contidos (P14); `Object::Process`/`ProcExit` (P17) — o exit status já foi gravado ANTES do reap, então reciclar o slot não afeta o pai que espera (ele lê o Arc, não o slot).
- O índice de uma thread pode ser REUSADO por outra após reciclagem — garantir que nada guarde um índice obsoleto esperando a mesma thread (ex.: `s.reap`, `s.current`, um wait que referencie índice em vez do Arc). O wait do Plano 17 usa o `Arc<ProcExit>` (não o índice), então é seguro — confirmar.
- Sem regressão: todas as variantes de `cargo xtask ktest` passam; `cargo xtask conformance` (7 casos) estável.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (kernel ktest + xtask/arch/mm/pe/kbd -D warnings), `cargo xtask test`, `cargo xtask ktest`.
- Testes headless. Clean room nível 0. Comentários em inglês; docs/mensagens em português.
- Commits: `Source: observation docs/observations/...`; `Co-Authored-By:` do modelo.

## Review Focus

1. **Reciclagem correta:** um slot reapeado vira `Free` e é reusado por um spawn futuro; `threads.len()` deixa de crescer sem limite; spawnar >64 ao longo da vida funciona. Sem índice obsoleto reusado por engano.
2. **Escalonador sólido:** `switch_to_next` pula `Free` (como `Exited`); o self-switch `to==from` (P12) e o guard "nenhuma thread viva" seguem; `s.current`/`s.reap` nunca apontam um slot `Free` indevidamente; nenhum slot `Free` é escalonado.
3. **Teardown intacto:** a thread ativa nunca é reciclada; o reaper libera recursos ANTES de marcar `Free`, na pilha da próxima thread (não libera CR3/stack ativos); `ProcExit` do P17 inalterado (pai que espera lê o Arc). Sem UAF/double-free/leak.
4. **Regressão:** ktests (incl. os de processo/espera/fault) e as 7 conformances passam; sem novo panic/unwrap.

---

### Task 1: Reproduzir a exaustão de slots (sem corrigir)

**Files:** Create `docs/observations/2026-10-14-thread-slot-exhaustion.md`; Modify `kernel/src/ktest.rs`

- [ ] **Step 1: Reprodutor determinístico.** Um ktest que faz spawn+exit+reap de muitas threads de kernel/usuário em sequência (ex.: 80 iterações, esperando cada uma sair/ser reapeada) e demonstra que a partir de ~64 o spawn passa a falhar (`NoThreads`/`INSUFFICIENT_RESOURCES`) mesmo com todas as anteriores já reapeadas (frames de volta à baseline, mas slots esgotados). Confirmar que é o slot (não frames) que esgota.
- [ ] **Step 2: Rodar e ver.** `cargo xtask ktest` com o reprodutor → a partir da ~64ª, falha de spawn (hoje). Capturar.
- [ ] **Step 3: Diagnóstico.** `docs/observations/2026-10-14-thread-slot-exhaustion.md`: sintoma, reprodutor, evidência (frames OK, slots esgotados), causa-raiz (reap não libera o slot), fix proposto (estado Free + reuso). Fonte do commit do fix.
- [ ] **Step 4: Estado.** Por padrão o ktest fica verde (o reprodutor pode afirmar o comportamento ATUAL — falha esperada em ~64 — de forma que PASSE documentando o limite, OU ficar atrás de um gate; na Task 2 ele vira a regressão que exige >64 OK). Escolher e documentar.
- [ ] **Step 5: Commit** (`test(sched): reprodutor da exaustao de slots de thread + diagnostico`, `Source: observation docs/observations/2026-10-14-thread-slot-exhaustion.md`).

---

### Task 2: Reciclar o slot no reaper + reuso no spawn + verificação

**Files:** Modify `kernel/src/sched.rs` (estado Free/tombstone; reap marca Free; spawn reusa Free; switch_to_next/capacity pulam Free), `kernel/src/ktest.rs`

**Interfaces:** sem mudança de API pública. `ThreadState` ganha `Free` (ou o slot vira `Option<Thread>`/um enum com tombstone). `reap_pending`: após liberar os recursos da thread (como hoje), marca o slot `Free` em vez de deixá-lo `Exited` ocupando a tabela. `spawn_*`: procura um slot `Free` e o reusa antes de dar `push` (crescer). `switch_to_next` e `has_thread_capacity`/seleção: tratam `Free` como vazio (pulam, nunca escalonam, contam como capacidade disponível). `s.current` nunca é `Free` (a thread corrente está viva); `s.reap` é consumido antes de reciclar.

- [ ] **Step 1: Teste que falha.** Transformar o reprodutor da Task 1 em regressão: `thread_slots_recycle` spawna+sai+reapeia > 64 (ex.: 200) threads em sequência e assere que TODAS têm sucesso (slots reusados), frames de volta à baseline, e o escalonador segue são (a thread de controle continua). Antes do fix falha (~64). 
- [ ] **Step 2: Rodar e ver falhar.** `cargo xtask ktest` → falha na ~64ª (RED).
- [ ] **Step 3: Implementar o fix mínimo.** Estado `Free` + reap marca Free + spawn reusa + switch_to_next/capacity pulam Free. NÃO alterar o salvamento de contexto, o self-switch `to==from` (P12), nem a ordem reap-após-switch. UMA mudança coesa.
- [ ] **Step 4: Rodar e ver passar.** `cargo xtask ktest` → `thread_slots_recycle` passa; TODAS as variantes passam; `cargo xtask conformance` (7 casos, incl. spawn) estável. clippy/fmt limpos.
- [ ] **Step 5: Commit** (`fix(sched): recicla slots de thread reapeados; spawn reusa slots livres`, `Source: observation docs/observations/2026-10-14-thread-slot-exhaustion.md`).

---

## Notas de fechamento

- Marco: spawns ao longo da vida não esgotam mais a tabela de threads; um shell pode rodar comandos indefinidamente. Escalonador e teardown inalterados no comportamento, só o slot passa a ser reciclado.
- Residual/futuro: crescer a tabela dinamicamente (hoje cap 64 de slots SIMULTÂNEos ainda vale — reciclagem resolve só a exaustão por vida, não >64 VIVAS ao mesmo tempo); pids reusados (um pid pode repetir após reciclagem — documentar).
- Deferreds remanescentes: P9 #1/#4, P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked; TOCTOU; first-fit O(n)), P13 (não-isolamento entre testes; dead-code sob features conformance), P14 (#GP sem teste), P15 (teclado polling; sem launcher de echo), P16 (arquivo >1MB; sem write/dirs), P17 (hThread==hProcess; Wait ignora timeout; build_space panica em PE malformado — endurecer antes de disco não-confiável).
- **Plano 19 = shell/REPL interativo** (lê o teclado via ReadFile, parseia um comando, `CreateProcess` do programa no initrd, `WaitForSingleObject`, imprime) rodando como o primeiro programa — **o terminal usável** que o usuário perguntou.
