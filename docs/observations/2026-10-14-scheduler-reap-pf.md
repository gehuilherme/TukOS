# 2026-10-14 - #PF intermitente apos o fim do processo (scheduler to==from)

## Sintoma
`cargo xtask conformance` falha em ~9/20 execucoes (rodadas anteriores: ~1 em 4-6) com
`@@TUKOS-PANIC excecao 14 PAGE_FAULT err=0x11 rip=<endereco do kernel>` DEPOIS de
`@@TUKOS-CONFORMANCE-END` / `-EXIT 0`. err=0x11 = busca de instrucao em pagina presente
(NX): o `ret` do `switch_context` cai em lixo. O `rip` varia entre execucoes
(0x...27ff8, 0x...2f4e8, 0x...26950): dado da pilha antiga.

## Mecanismo (confirmado)
`kernel/src/sched.rs::switch_to_next`: com so a thread corrente viva e as demais ainda no
`Vec` como `Exited`, `s.threads.len() >= 2` (nao retorna cedo), o laco `while` pula as
Exited e da a volta para `to == from`. Entao `save = &threads[from].rsp` e
`load = threads[from].rsp` sao o MESMO slot, lido ANTES de `switch_context` grava-lo. O
`switch_context` empilha os callee-saved, grava o rsp novo em `*save`, e carrega o `load`
ANTIGO (stale, de outro ponto/profundidade da pilha do boot). Os 6 pops + `ret` leem uma
regiao de pilha ja sobrescrita -> retorno para endereco invalido -> #PF.
Dispara quando o timer (`on_tick`) ou `yield_now` roda na thread boot apos o reaping, ex.:
durante os `kprintln!` entre o reaping e `qemu::exit`.
Intermitencia: se o boot estiver na mesma profundidade de pilha da vez anterior, o frame
stale coincide por acaso e nao falha.

## Reprodutor deterministico
`TUKOS_EXTRA_FEATURES=repro-sched-pf TUKOS_HEADLESS=1 cargo xtask ktest`
(ktest `single_thread_after_exit_no_fault`): aposenta as outras threads
(`sched::debug_retire_others`) e faz `yield_now` do boot em profundidades de pilha
diferentes. Falha 100% (1/1 e repetido), saida:

    @@REPRO stale saved_rsp=0xffff80001dd3eaf8 self_switches=0
    @@SCHED-SELF-SWITCH from=to=0 n=7 exited=6 cur_rsp=0xffff80001dd3e5f0 load_rsp(stale)=0xffff80001dd3eaf8
    @@TUKOS-PANIC excecao 14 PAGE_FAULT err=0x11 rip=0x1 cr2=0x1

Sem a feature, o corpo do ktest e pulado (o kernel cai), mantendo `cargo xtask ktest` verde.

## Evidencia no conformance
Com `TUKOS_EXTRA_FEATURES=sched-debug` (log `@@SCHED-SELF-SWITCH`), 20 execucoes: em TODAS
as execucoes que falharam com #PF a linha SELF-SWITCH (`n=2 exited=1`, `cur_rsp` !=
`load_rsp(stale)`, ex.: cur=0xffff80001dd3eca0 vs stale=0xffff80001dd3ea38) aparece
imediatamente antes do PANIC. O log tambem polui os marcadores (falso fail), por isso a taxa
de falha limpa foi medida sem a feature: 9/20.

## Correcao minima proposta (NAO implementada aqui)
Em `switch_to_next`, se `to == from` (a unica thread viva e a corrente) simplesmente
retornar sem `switch_context` (e sem fxsave/cr3). Opcionalmente tambem retornar quando
nao ha outra thread Ready. O ktest passa a rodar sem o gate `repro-sched-pf`, afirmando
que nao ha fault.

## Instrumentacao temporaria (remover na task 2)
`SELF_SWITCHES`/`SELF_SWITCH_*` e bloco `to == from` em sched.rs, `debug_retire_others`,
`debug_current_saved_rsp`, features `sched-debug`/`repro-sched-pf`, `TUKOS_EXTRA_FEATURES`
em xtask/src/image.rs.
