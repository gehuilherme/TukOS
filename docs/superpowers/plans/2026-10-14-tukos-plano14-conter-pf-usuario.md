# TukOS Plano 14 — Conter faults de usuário (não panicar o kernel)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:systematic-debugging (causa-raiz já confirmada — reproduzir antes de corrigir) + superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Fazer uma exceção tomada em Ring 3 (page fault, #GP, #UD, overflow de stack, deref inválido, etc.) **terminar o processo faltante** — não panicar o kernel inteiro. Hoje qualquer falha de um programa de usuário derruba o OS; isso precisa virar "o processo morre, o kernel sobrevive, as outras threads continuam". É pré-requisito de robustez para rodar apps reais (que falham).

**Causa-raiz (CONFIRMADA na revisão do Plano 13, Task 2):** `kernel/src/trap.rs` `handle()` só trata TIMER/SPURIOUS/BREAKPOINT; todo o resto (PAGE_FAULT=14, #GP=13, #UD=6, ...) cai em `panic!(...)` **incondicional**, sem checar o CPL (`cs & 3`) e sem caminho para terminar o processo. O stub ASM já faz swapgs-on-Ring3, então um fault de usuário chega em `handle` e panica. O stack de 16 páginas do Plano 13 só mascara o caso do hello.

**Architecture:** No `handle` (e/ou no stub que o chama), distinguir a origem do fault pelo `cs & 3` do frame salvo: **fault de Ring 3 (CPL=3)** nas exceções de falha (PF/GP/UD/DE/etc.) → registrar o motivo na serial e **terminar + reapear o processo atual** reusando o caminho de `NtTerminateProcess`/`exit_current` (com um NTSTATUS do tipo `STATUS_ACCESS_VIOLATION`/`STATUS_ILLEGAL_INSTRUCTION`), depois escalonar a próxima thread; **fault de Ring 0 (CPL=0)** → continua `panic!` (bug de kernel). Interrupções já existentes (timer/spurious) inalteradas. Cuidado com: o fault de usuário entra com a kernel stack da thread (rsp0 já setado no switch), o CR3 do processo ainda ativo (terminar deve seguir a disciplina do Plano 4: não liberar o CR3 ativo — o reaper roda depois do switch), e o swapgs/estado de GS correto ao sair.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `crates/arch-x86_64` (IDT/trap stub), QEMU + OVMF. Testes via `cargo xtask ktest` (headless).

**Spec:** n/a (robustez/correção). Sucede o Plano 13 (CRT), já na `main`. Destacado como HIGH pela revisão final do degrau 3b.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64; código de CPU/trap em `crates/arch-x86_64`.
- systematic-debugging: reprodutor determinístico (que hoje panica) ANTES do fix; UMA mudança mínima que ataca a raiz; verificação.
- Invariantes de processo dos Planos 3/4/12 mantidas: **nunca liberar o CR3/stack/fxarea da thread ativa** (o reaper roda após o switch); o self-switch `to==from` (Plano 12) intacto; a contenção não pode introduzir leak de frames nem deixar o escalonador inconsistente.
- Sem regressão: todas as variantes de `cargo xtask ktest` passam; `cargo xtask conformance` (com MSVC) segue estável (4 casos, 0 panic).
- `cargo fmt --all -- --check`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` limpos ao fim de cada tarefa.
- Testes headless (não reintroduzir janela). Clean room nível 0. Comentários em inglês; docs/mensagens em português.
- Commits: `Source: observation docs/observations/...` para o diagnóstico; `Co-Authored-By:` do modelo. Ao fim, atualizar `ROADMAP.md` (a linha "Em aberto" cita "endurecer o kernel: um #PF de usuário não deve causar panic" — marcar como feito).

## Review Focus

1. **Contenção correta por CPL:** fault de Ring 3 → termina só o processo faltante; fault de Ring 0 → ainda panica. A checagem usa o `cs & 3` do frame de exceção certo (não um valor obsoleto), e cobre os vetores de falha relevantes (PF/GP/UD/DE/… ), não só o PF.
2. **Disciplina de teardown:** terminar-no-fault segue o caminho seguro (não libera o CR3/stack ativo; reaper após o switch); sem leak de frames; escalonador consistente; GS/swapgs corretos ao retomar outra thread.
3. **Sem falso "contido":** um fault de kernel de verdade (bug nosso) NÃO é silenciado — continua panicando com a mensagem/serial. O motivo do fault de usuário é logado (serial) para depuração.
4. **Regressão:** ktests e conformance seguem verdes/estáveis; o hello /MT ainda roda; o fix do scheduler (Plano 12) e as invariantes de processo seguem.
5. **Guard de stack (parcial):** um overflow incremental de stack bate no guard page → #PF de usuário → termina o processo (coberto pela contenção). O caso de "pular o guard com um frame gigante" fica anotado como residual (demand-grow/múltiplos guards = futuro), não resolvido aqui.

---

### Task 1: Reproduzir o panic do kernel por fault de usuário (sem corrigir)

**Files:**
- Create: `docs/observations/2026-10-14-user-fault-kernel-panic.md`
- Modify: `kernel/src/ktest.rs` (reprodutor), `kernel/src/process.rs`/helpers de teste se preciso

**Objetivo:** systematic-debugging Fase 1 — provar o comportamento. NÃO corrigir.

- [ ] **Step 1: Reprodutor determinístico.** Um ktest (ex.: `("user_fault_terminates_not_panics", ...)`) que spawna uma thread de **usuário** cujo código faz um acesso inválido determinístico (ex.: `mov` a um endereço de usuário não mapeado, ou um `ud2`). Hoje isso deve fazer o kernel **panicar** (`@@TUKOS-PANIC`). Como um panic derruba o ktest inteiro, GATE o reprodutor atrás de uma feature de debug (ex.: `repro-user-fault`), análogo ao que o Plano 12 fez — por padrão o ktest não roda o corpo que panica; com a feature, ele roda e evidencia o panic. Documente o PANIC observado (mensagem/vetor/rip).
- [ ] **Step 2: Rodar e ver o panic.** `TUKOS_EXTRA_FEATURES=repro-user-fault cargo xtask ktest` (ou o mecanismo de feature equivalente) → `@@TUKOS-PANIC` no vetor de PF/UD a partir de CPL3.
- [ ] **Step 3: Evidência + diagnóstico.** Em `docs/observations/2026-10-14-user-fault-kernel-panic.md`: o reprodutor, o panic capturado (vetor, error code, cs:rip, CPL), a confirmação de que `trap.rs handle()` panica sem checar CPL, e o fix proposto (contenção por `cs & 3`). Fonte do commit do fix.
- [ ] **Step 4: Estado.** ktest verde por padrão (reprodutor gated). Instrumentação mínima; remover/silenciar antes do fim do plano.
- [ ] **Step 5: Commit.** `test(trap): reprodutor do panic do kernel por fault de usuario + diagnostico` (`Source: observation docs/observations/2026-10-14-user-fault-kernel-panic.md`, `Co-Authored-By: <modelo>`).

---

### Task 2: Conter o fault de usuário (terminar o processo) + regressão + verificação

**Files:**
- Modify: `kernel/src/trap.rs` (contenção por CPL), `crates/arch-x86_64/src/*` (se o stub precisar passar o frame/cs), `kernel/src/sched.rs`/`process.rs` (caminho de terminar-no-fault, se não reusar exit_current direto), `kernel/src/ktest.rs`

**Interfaces:** sem mudança de API pública. `handle` (ou o dispatcher de exceção) passa a: se o fault é de Ring 3 (`cs & 3 == 3`), logar o motivo e terminar o processo atual (NTSTATUS adequado) + escalonar o próximo; se de Ring 0, `panic!` como hoje. Terminar segue a disciplina do Plano 4/12 (reaper após switch; não libera recurso ativo).

- [ ] **Step 1: Teste que falha.** Tornar o reprodutor da Task 1 um teste de regressão DEFAULT (sem a feature de debug): `user_fault_terminates_not_panics` spawna a thread de usuário que faz o acesso inválido e assere que (a) o kernel NÃO panica, (b) o processo faltante foi terminado+reapeado (frames recuperados), (c) uma OUTRA thread de usuário/kernel continua executando normalmente depois. Antes do fix, roda → panica (RED). Acrescentar variantes p/ #UD e, se viável, #GP.
- [ ] **Step 2: Rodar e ver falhar.** `cargo xtask ktest` → panica/falha (RED).
- [ ] **Step 3: Implementar o fix mínimo.** Checagem de CPL em `handle` → terminar+reapear no caso Ring 3; panic preservado p/ Ring 0. Garantir GS/swapgs e a disciplina de teardown. Remover a instrumentação/feature de debug da Task 1.
- [ ] **Step 4: Rodar e ver passar + estabilidade.** `cargo xtask ktest` → `user_fault_terminates_not_panics` (e as variantes) passam; TODAS as variantes passam. `cargo xtask conformance` (se houver MSVC) estável em N execuções (0 panic; 4 casos ok). Um fault de KERNEL de propósito (teste manual/temporário) ainda panica — confirmar que não foi silenciado. clippy/fmt limpos. Atualizar `ROADMAP.md` ("Em aberto" → remover/!marcar o item do #PF de usuário).
- [ ] **Step 5: Commit.** `fix(trap): fault de Ring 3 termina o processo em vez de panicar o kernel` (`Source: observation docs/observations/2026-10-14-user-fault-kernel-panic.md`, `Co-Authored-By: <modelo>`; incluir a atualização do ROADMAP).

---

## Notas de fechamento

- Marco: um programa de usuário que falha (deref inválido, #UD, overflow de stack que bate no guard) é **terminado**, e o TukOS continua vivo e escalonando — robustez necessária para apps reais. Um bug de kernel ainda panica (visível).
- Residual (futuro, anotado): pular o guard page com um frame gigante (precisa de múltiplos guards / demand-grow de stack); entregar `STATUS_*` de exceção ao SEH do usuário (quando houver SEH real); core-dump/relatório de crash.
- Deferreds remanescentes de planos anteriores: P9 #1/#4, P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked self-only; TOCTOU futura; first-fit O(n)), P13 (settle-loop de não-isolamento entre testes; ~85 warnings dead-code sob features conformance).
- Depois: retomar a superfície de apps — resto do subprojeto 2 (I/O manager, registro) e subprojeto 4 (user32/gdi32/janela, rumo a "programa com janela abre").
