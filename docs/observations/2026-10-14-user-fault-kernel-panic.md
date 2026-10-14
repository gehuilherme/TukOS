# Fault de usuario (Ring 3) derruba o kernel com panic

Data: 2026-10-14 (Plano 14, Task 1). Observacao propria, sem material da Microsoft.

## Reprodutor

- Feature de debug `repro-user-fault` (kernel/Cargo.toml, implica `ktest`).
- `process::spawn_faulting_pe()` (kernel/src/process.rs): PE minimo, `.text` =
  `48 8B 04 25 01 00 00 00` (`mov rax, [0x1]`) + `ud2`; o VA 0x1 de usuario nao esta mapeado.
- ktest `user_fault_terminates_not_panics` (ultimo da lista): sem a feature e no-op (Ok);
  com a feature spawna o PE e cede a CPU.
- Execucao: `TUKOS_EXTRA_FEATURES=repro-user-fault cargo xtask ktest`
  (o xtask passa a rodar so a variante `ktest` com as features extras).

## Panic capturado

```
@@TUKOS-PANIC excecao 14 PAGE_FAULT err=0x4 rip=0x401000 cr2=0x1
TrapFrame { vector: 0xe, error_code: 0x4, rip: 0x401000, cs: 0x23,
            rflags: 0x10202, rsp: 0x404000, ss: 0x1b, ... } @ kernel\src\trap.rs:65
```

- vetor 14 (PAGE_FAULT); err=0x4 = P=0 (pagina ausente), W/R=0 (leitura), U/S=1 (modo usuario).
- `cs=0x23` -> RPL (cs & 3) = 3, ou seja CPL 3; `ss=0x1b` (RPL 3); rip=0x401000 e o entry do PE de usuario; cr2=0x1.

## Causa raiz confirmada

`kernel/src/trap.rs::handle` so trata TIMER/SPURIOUS/BREAKPOINT; qualquer outra excecao cai em
`panic!` (linha 65) sem olhar o privilegio de origem. Um fault de usuario chega a `handle`
(o stub faz swapgs na entrada de Ring 3) e derruba o kernel inteiro.

## Fix proposto (Task 2)

Em `handle`, antes do panic: se `frame.cs & 3 == 3`, tratar como fault de processo de
usuario: terminar + recolher (reap) o processo/thread corrente (status de excecao) e
escalonar outro, sem panic. Panic apenas para CPL 0. Remover a feature `repro-user-fault`
e transformar o ktest em teste de regressao ativo.
