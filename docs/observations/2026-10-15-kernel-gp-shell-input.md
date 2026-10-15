# #GP do kernel por input do shell (fix-gp-shell-input)

**Data:** 2026-10-15. **Reportado por:** o usuário, testando o terminal (digitou `help "teste"` no shell após vários comandos).

## Sintoma

```
tukos> help "teste"
comandos: hello echo help exit
KERNEL PANIC: excecao 13 GENERAL_PROTECTION err=0x18 rip=0xffffffff8002c008 cr2=0x0
TrapFrame { ... rcx=0x100010ca r11=0x246 rbp=0x200029ff rsi=0x10019a00 ... cs=0x8 ... }
```

Um `#GP` (vetor 13) **em modo kernel** (`cs=0x8`, CPL 0), intermitente (apareceu uma vez após muitos comandos). Input de usuário NUNCA pode derrubar o kernel.

## Diagnóstico (confirmado)

- Desmontando o NOSSO kernel (`llvm-objdump` do ELF com a feature `shell`), `rip=0xffffffff8002c008` é a **instrução `iretq` em `tukos_isr_common`** (o caminho comum de retorno de interrupção). `error_code=0x18` identifica um **seletor da GDT (índice 3 = segmento de dados de usuário)**.
- `iretq` retornando a Ring 3 faz `#GP(0x18)` porque um seletor no frame de iret é `0x18` (o seletor de dados de usuário **sem o RPL 3**) em vez de `SS=0x1b`/`CS=0x23`. Ou seja, o frame de retorno a Ring 3 tinha um seletor corrompido.
- **`tukos_isr_common` está correto** (entra/sai simétrico, swapgs condicional pelo RPL do CS) — ele apenas faz `iretq` do frame que a CPU empilhou.
- **Toda a configuração estática de seletores está correta**: `gdt::USER_DS=0x1b`, `USER_CS=0x23` (DPL 3); `STAR` → `SYSRET` carrega `CS=0x23`/`SS=0x1b`; o stub `tukos_syscall_entry` (swapgs/troca de pilha) está certo; `enter_user` empilha `0x1b`/`0x23`; o frame inicial da thread em `sched.rs` usa `USER_CS`/`USER_DS`. Nenhum lugar usa o `0x18` cru.
- Logo, **não é um seletor estático errado — é uma corrupção/corrida rara** do frame de retorno (o shell rodou ~89 `help "teste"` sem falhar num harness de stress com timer acelerado por 180s; o usuário bateu uma vez em uso real). A fonte exata da corrupção (qual escrita deixa `0x18` no frame) **ainda não foi isolada/reproduzida** — o evento é raro demais para reproduzir no harness disponível no prazo.

## Correção (defense-in-depth, na raiz do crash)

Um frame de retorno bem-formado no TukOS só pode ter `CS=KERNEL_CS(0x08)` (retorno ao kernel) ou `CS=USER_CS(0x23)` e `SS=USER_DS(0x1b)` (retorno a Ring 3) — não há outro seletor válido. Então `handle` (o handler de trap do kernel) **sanitiza o frame** no início, antes de qualquer `iretq`: se o frame não é um retorno ao kernel limpo nem o retorno de usuário canônico, ele é um frame de retorno a Ring 3 corrompido e é **reparado para os únicos seletores de usuário válidos** (`arch_x86_64::trap::sanitize_return_frame`). O handler escreve sobre a MESMA memória que o `iretq` recarrega, então o reparo tem efeito. O caminho normal (frames limpos) fica **intocado** (zero impacto). Se outro estado do frame também estiver corrompido, o retorno reparado degrada para um **fault de Ring 3 contido** (Plano 14: o processo morre, o kernel sobrevive) em vez de um panic do kernel. Um contador `SEGMENT_FIXUPS` registra quando o reparo dispara (0 em operação normal), e a primeira ocorrência loga na serial — visibilidade para futura investigação da causa-raiz.

## Verificação

- Testes de host provam a lógica do reparo: `sanitize_leaves_clean_frames_untouched`, `sanitize_repairs_corrupted_selectors` (SS=0x18 e CS=0x18 → reparados para 0x23/0x1b; frames limpos → intocados).
- `cargo xtask ktest`: todas as 4 variantes passam (incl. `shell_quoted_arg_survives`, que roda o shell com exatamente `help "teste"` e confere que o kernel sobrevive e o shell sai limpo).
- `cargo xtask conformance`: 7/7 casos passam (sem regressão no caminho normal).
- clippy (`-D warnings`, ambos os gates) e `fmt --all --check` limpos.

## Pendente (rastreado)

A **fonte exata** da corrupção do frame (qual código deixa `0x18` num frame de retorno — candidatos: corrida timer/context-switch, overflow de pilha de kernel do shell, janela swapgs) não foi reproduzida/isolada. O reparo defensivo impede o crash do kernel independentemente da fonte; o contador `SEGMENT_FIXUPS` + o log da 1ª ocorrência ajudam a pegar o momento se recorrer. Investigar a causa-raiz quando houver reprodução confiável.
