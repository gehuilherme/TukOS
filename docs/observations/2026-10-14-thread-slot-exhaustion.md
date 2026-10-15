# Exaustao de slots da tabela de threads (2026-10-14)

## Sintoma
Apos ~64 threads criadas durante a vida do kernel (mesmo todas ja encerradas e
recolhidas), `sched::spawn*` nao tem mais onde alocar: `spawn` entra em
`assert!` (panic), `spawn_user_with_exit` retorna `NoThreads`, e
`NT_CREATE_PROCESS` devolve `STATUS_INSUFFICIENT_RESOURCES`.

## Reprodutor
ktest `thread_slots_exhaust` (`kernel/src/ktest.rs`, ultimo da lista). Cria UMA
thread de kernel por vez (`slot_child`: incrementa um contador e chama
`exit_current`), espera ela sair e ser recolhida, e repete ate 80 vezes. Antes de
cada spawn consulta `has_thread_capacity()`. Usa o acessor de teste
`sched::slot_usage_for_test()` (cfg ktest).

## Evidencia (saida da serial)
```
thread_slots_exhaust: slots_usados_inicio=39 max=64 falhou_na_iteracao=Some(25) slots_usados_fim=64 frames_baseline=107545 frames_agora=107545
```
- Os 39 slots iniciais foram consumidos pelos ktests anteriores (todos Exited).
- A capacidade acaba exatamente na iteracao 25 (39 + 25 = 64 = MAX_THREADS),
  de forma deterministica.
- `free_frame_count()` foi verificado igual a baseline apos CADA filho: nao e
  memoria, e o SLOT que esgota.

## Causa-raiz
`kernel/src/sched.rs::reap_pending` libera `space` e `_stack` da thread
terminada mas deixa a entrada no `Vec<Thread>` (estado `Exited`).
`threads.len()` nunca diminui; `has_thread_capacity` e os spawns testam
`len < MAX_THREADS`. Indices sao referenciados por `s.current`, `s.reap` e pelo
round-robin, entao remover do Vec nao e opcao.

## Fix proposto (minimo)
- `ThreadState::Free`: slot vazio reutilizavel.
- `reap_pending` marca o slot reapeado como `Free` (depois de soltar space/stack).
- Os spawns reutilizam o primeiro slot `Free` (senao `push` se `len < MAX_THREADS`);
  `has_thread_capacity` = existe `Free` ou `len < MAX_THREADS`.
- `switch_to_next` pula `Free` e `Exited`.
- Tarefa 2 inverte o ktest: as 80 iteracoes devem todas ter sucesso.

## Estado do teste
O ktest AFIRMA o comportamento atual (falha em `max - used0`, frames na
baseline) para que a suite fique verde ate o fix; na Task 2 vira a regressao.
