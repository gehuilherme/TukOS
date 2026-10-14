# TukOS Plano 11 — Memória virtual + heap de usuário (fundação para o CRT)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dar ao modo usuário a capacidade de **alocar memória** — pré-requisito do CRT e de qualquer programa real. Primeiro corrige a ABI do exit status do `NtTerminateProcess` (dívida do Plano 10). Depois adiciona as syscalls `NtAllocateVirtualMemory`/`NtFreeVirtualMemory` (o kernel mapeia páginas `USER|RW|NX` sob demanda no espaço do processo, com validação), e um **heap de usuário** no `ntdll` (`RtlAllocateHeap`/`RtlFreeHeap`) exposto pelo `kernel32` (`GetProcessHeap`/`HeapAlloc`/`HeapFree`). Um `.exe` (feito pelo TukOS e também um real do MSVC) aloca memória, escreve nela, imprime via `WriteFile` e libera — tudo em modo usuário, com o heap servido pelo `NtAllocateVirtualMemory`.

**Architecture:** O kernel ganha `NtAllocateVirtualMemory(ProcessHandle, *BaseAddress, ZeroBits, *RegionSize, AllocationType, Protect)` e `NtFreeVirtualMemory` que operam no `AddressSpace` do processo atual: arredondam para página, validam a faixa (dentro do espaço de usuário, sem sobrepor mapeamentos existentes, sem tocar a metade alta do kernel), mapeiam páginas zeradas `USER|WRITABLE|NO_EXECUTE`, e devolvem a base/tamanho via `copy_to_user`. O `ntdll` ganha um heap simples (free-list/bump sobre blocos obtidos do `NtAllocateVirtualMemory`) atrás de `RtlAllocateHeap`/`RtlFreeHeap`; como o `ntdll` não tem allocator global, o heap é um estado estático simples protegido para single-thread (o TukOS ainda é single-thread em modo usuário). O `kernel32` expõe `GetProcessHeap` (devolve o handle do heap do processo, inicializado sob demanda), `HeapAlloc`/`HeapFree` (chamam o `ntdll`). A dívida do exit status é corrigida em todo o stack para a assinatura real `NtTerminateProcess(ProcessHandle, ExitStatus)`.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crates `pe`, `ntdll`, `kernel32`, `mm`, `arch-x86_64`, QEMU + OVMF, MSVC no host Windows para o harness.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 6: memória virtual de usuário; heap do `ntdll`/`kernel32`). Sucede o Plano 10 (loader multi-módulo + kernel32), já na `main`.

**Fora deste plano (vai para o Plano 12, degrau 3b completo):** o CRT completo (`libcmt`) e um `hello.exe` compilado com o CRT do MSVC; TLS/`__security_init_cookie`; `GetModuleHandle`/`GetProcAddress`/`GetStartupInfo` e o resto da superfície que o `mainCRTStartup` exige (surveyar com `dumpbin /imports`); SEH em modo usuário; multithread. Aqui o heap é single-thread e as syscalls de VM são o mínimo para o heap.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly.
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64. Código dependente de CPU em `crates/arch-x86_64`.
- Nome do projeto: TukOS. `ntdll.dll`/`kernel32.dll`, `Nt*`/`Rtl*` e os nomes Win32 são interface de interoperabilidade, permitidos.
- **Clean room nível 0 apenas:** assinaturas de `NtAllocateVirtualMemory`/`NtFreeVirtualMemory`/`RtlAllocateHeap`/`HeapAlloc` da documentação pública (winternl.h/MSDN); semântica de heap própria (não copiar o layout do heap da Microsoft); nenhum disassembly nem binário da Microsoft.
- `ntdll`/`kernel32` continuam sem imports além do declarado (kernel32→ntdll) e o heap do `ntdll` só usa `NtAllocateVirtualMemory` (sem allocator global do Rust).
- **Invariante de segurança:** `NtAllocateVirtualMemory` só mapeia páginas `USER|WRITABLE|NO_EXECUTE` na metade baixa (usuário) do espaço do processo atual; nunca sobrepõe um mapeamento existente; nunca mapeia na metade alta do kernel; valida/rounda `BaseAddress`/`RegionSize` com aritmética checada; todo ponteiro de saída passa por `copy_to_user`. A invariante de escrita na IAT do Plano 8/9/10 permanece.
- `build.rs`/`xtask`: sem MSVC o `ntdll`/`kernel32` não compilam e os testes/harness dependentes se auto-pulam; build nunca quebra (Linux CI verde).
- **Gate de teste (corrigir no Task 1):** `cargo xtask test` passa a rodar também `-p pe -p ntdll`; o lint CI inclui `-p pe`. Além de `cargo fmt --all -- --check`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, e `cargo xtask ktest` ao fim de cada tarefa.
- Commits declaram a fonte (`Source: docs winternl.h (...)` / `Source: docs Microsoft PE Format specification`) e terminam com `Co-Authored-By:` do modelo.
- Código e comentários em inglês; mensagens e docs em português.

## Review Focus

1. **Exit status correto ponta-a-ponta:** `NtTerminateProcess(handle, status)` lê o status do slot correto (a1); `kernel32!ExitProcess(code)` e `ntdll` `die()`/`RtlExitUserProcess` produzem o código certo; nenhum caminho de 1-arg quebrou. Coberto por `exit_code_is_reported` (Task 1).
2. **`NtAllocateVirtualMemory` seguro:** não mapeia sobre um mapeamento existente, nem na metade alta, nem com overflow de `BaseAddress+RegionSize`; páginas `USER|RW|NX`; zera a memória; `BaseAddress=NULL` deixa o kernel escolher a base. Coberto por `valloc_basic` + `valloc_rejects_overlap`/`valloc_rejects_kernel_range` (Task 2); **review Opus**.
3. **Heap correto:** `HeapAlloc` devolve blocos distintos e alinhados; escrever num bloco não corrompe outro; `HeapFree` permite reuso; esgotar um bloco pede mais ao `NtAllocateVirtualMemory`. Coberto por `heap_alloc_write_free` (Task 3); **review Opus** (correção do alocador).
4. **Sem allocator global/heap compartilhado indevido:** o heap do `ntdll` é por-processo e single-thread; nenhum estado vaza entre processos (cada processo tem seu espaço e inicializa o próprio heap). Coberto pela checagem de isolamento (Task 3).
5. **Conformidade:** um `.exe` MSVC que faz `HeapAlloc`+`WriteFile`+`HeapFree`+`ExitProcess` roda idêntico no Windows real e no TukOS; o exit code bate. Coberto por `cargo xtask conformance` estendido (Task 4).

---

### Task 1 (FIX-FIRST): ABI do exit status do `NtTerminateProcess` + cobertura de teste

**Files:**
- Modify: `kernel/src/syscall.rs` (ler o status de a1), `crates/ntdll/src/lib.rs` (`die()`/`NtTerminateProcess`/`RtlExitUserProcess` consistentes com 2-arg), `crates/kernel32/src/lib.rs` (`ExitProcess`), `kernel/src/ktest.rs`, `xtask/src/main.rs` (gate de teste), `.github/workflows/ci.yml` (lint/test com `-p pe -p ntdll`)

**Interfaces:**
- `NtTerminateProcess(ProcessHandle=arg0, ExitStatus=arg1)`: o dispatch do kernel lê o `ExitStatus` de **a1** (rdx), não de a0 (r10=handle). O exit status do processo passa a ser `a1`.
- `ntdll` `die(status)`: chama `NtTerminateProcess(NtCurrentProcess=-1, status)` (2-arg, status em rdx) — não mais status em r10. `RtlExitUserProcess(code)` idem. O stub `NtTerminateProcess` já faz `mov r10,rcx` (handle) e os args seguem a ABI; garanta que `status` chega em a1.
- `kernel32!ExitProcess(code)` chama `NtTerminateProcess(-1, code)` (já faz; agora o kernel lê o slot certo).
- `xtask test` roda `-p xtask -p arch-x86_64 -p mm -p pe -p ntdll`; o lint CI inclui `-p pe`.

- [ ] **Step 1: Teste que falha**

`kernel/src/ktest.rs` `("exit_code_is_reported", exit_code_is_reported)`: um `.exe` de teste que chama `ExitProcess(0x2A)` (ou `NtTerminateProcess(-1, 0x2A)`) e o kernel registra o exit status do processo = `0x2A` (exponha o último exit status via um estático testável, análogo a `LAST_WRITE_LEN`). Com o bug atual, viria `-1`/`0xFFFFFFFF`. Ajuste o teste do forwarder do Plano 10 (`RtlExitUserProcess(0x5A5A)`) para a ABI 2-arg se necessário.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ntdll && cargo xtask kernel32 && cargo xtask ktest` → `exit_code_is_reported` falha (vem -1).

- [ ] **Step 3: Implementar**

Corrija o dispatch (`ExitStatus` de a1), o `die()`/`RtlExitUserProcess` para 2-arg, e confirme `ExitProcess`. Rode também o survey: `exit_code_is_reported` e o teste do forwarder passam. Adicione `-p pe -p ntdll` ao `xtask test` e `-p pe` ao lint CI.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` (exit_code_is_reported + forwarder + todos passam), `cargo xtask test` (inclui pe/ntdll) ok, clippy/fmt ok.

- [ ] **Step 5: Commit**

```bash
git add kernel crates/ntdll crates/kernel32 xtask .github
git commit -m "fix(kernel): NtTerminateProcess le o ExitStatus do slot correto (arg1); gate roda pe+ntdll

Source: docs winternl.h (NtTerminateProcess signature)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: `NtAllocateVirtualMemory` / `NtFreeVirtualMemory`

**Files:**
- Modify: `kernel/src/syscall.rs` (novos números de syscall + dispatch), `kernel/src/mem/space.rs`/`paging.rs` (mapear/desmapear faixas no `AddressSpace` com validação), `crates/ntdll/src/lib.rs` (stubs `NtAllocateVirtualMemory`/`NtFreeVirtualMemory`), `kernel/src/ktest.rs`

**Interfaces:**
- Números novos (após os existentes, ex.: NT_ALLOCATE_VIRTUAL_MEMORY=5, NT_FREE_VIRTUAL_MEMORY=6; confirme os livres).
- `NtAllocateVirtualMemory(ProcessHandle, *mut BaseAddress, ZeroBits, *mut RegionSize, AllocationType, Protect) -> NTSTATUS`: só suporta o processo atual (handle=-1 ou o self handle); lê `*BaseAddress`/`*RegionSize` via `copy_from_user`; arredonda base p/ baixo e size p/ cima (4 KiB); se `*BaseAddress==0`, o kernel escolhe uma base livre na metade baixa (acima da imagem/heap existente, p.ex. varrendo o `AddressSpace`); **valida**: faixa inteiramente na metade baixa de usuário, sem overflow, sem sobrepor páginas já mapeadas; mapeia páginas zeradas `USER|WRITABLE|NO_EXECUTE`; escreve a base/size resultantes via `copy_to_user`; devolve `STATUS_SUCCESS` ou um NTSTATUS de erro (`STATUS_INVALID_PARAMETER`/`STATUS_CONFLICTING_ADDRESSES`/`STATUS_NO_MEMORY`). `Protect` neste plano: trata `PAGE_READWRITE` (NX); outros → aceita como RW ou `STATUS_INVALID_PARAMETER` (documente; sem páginas executáveis de usuário aqui).
- `NtFreeVirtualMemory(ProcessHandle, *mut BaseAddress, *mut RegionSize, FreeType)`: desmapeia a faixa (valida que está mapeada e é de usuário), devolvendo os frames.
- `ntdll`: stubs `mov r10,rcx; mov eax,5|6; syscall; ret` (como os outros Nt*).

- [ ] **Step 1: Testes que falham**

`kernel/src/ktest.rs`: (a) `valloc_basic` — um `.exe` de usuário (ou um caminho de teste no kernel que roda no espaço de um processo) chama `NtAllocateVirtualMemory(NULL, size=0x3000)`, escreve um padrão nas 3 páginas, lê de volta (via `copy_from_user`) e confere; depois `NtFreeVirtualMemory` e os frames voltam; (b) `valloc_rejects_overlap` — alocar numa base já mapeada (a imagem) → `STATUS_CONFLICTING_ADDRESSES`, nada mapeado; (c) `valloc_rejects_kernel_range` — base na metade alta → `STATUS_INVALID_PARAMETER`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Implemente o dispatch + a lógica no `AddressSpace` (varredura p/ base livre, checagem de sobreposição consultando o mapeamento, map/unmap por página `USER|RW|NX`, zero-fill pela HHDM). Aritmética checada. Reaproveite os helpers de `paging.rs`.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ntdll && cargo xtask ktest` → os três testes passam. Demais ok.

- [ ] **Step 5: Commit**

```bash
git add kernel crates/ntdll
git commit -m "feat(kernel): NtAllocateVirtualMemory/NtFreeVirtualMemory (USER|RW|NX, validado)

Source: docs winternl.h (NtAllocateVirtualMemory signature)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Heap de usuário — `RtlAllocateHeap`/`RtlFreeHeap` (ntdll) + `GetProcessHeap`/`HeapAlloc`/`HeapFree` (kernel32)

**Files:**
- Modify: `crates/ntdll/src/lib.rs` (heap single-thread sobre `NtAllocateVirtualMemory`), `crates/kernel32/src/lib.rs` (`GetProcessHeap`/`HeapAlloc`/`HeapFree`), `kernel/src/ktest.rs`

**Interfaces:**
- `ntdll`: um heap por-processo (estado `static mut` single-thread — o TukOS é single-thread em user mode): `RtlAllocateHeap(heap, flags, size) -> *mut u8` e `RtlFreeHeap(heap, flags, ptr) -> i32`. Implementação simples e **correta**: blocos alinhados (≥16), cabeçalho por bloco com tamanho/estado, free-list; quando falta espaço, pede um novo segmento ao `NtAllocateVirtualMemory` (ex.: 64 KiB). `flags & HEAP_ZERO_MEMORY` zera. O "handle" de heap é um ponteiro p/ a estrutura de controle do heap.
- `kernel32`: `GetProcessHeap() -> HANDLE` (inicializa o heap do processo sob demanda e devolve o handle; guarda-o num estático), `HeapAlloc(heap, flags, size)` → `RtlAllocateHeap`, `HeapFree(heap, flags, ptr)` → `RtlFreeHeap`. Importa essas `Rtl*` do `ntdll` (raw-dylib), estendendo o import directory do `kernel32` (agora `ntdll` com mais funções).

- [ ] **Step 1: Teste que falha**

`kernel/src/ktest.rs` `("heap_alloc_write_free", ...)`: um `.exe` de teste que `GetProcessHeap()`, faz vários `HeapAlloc` de tamanhos diferentes, escreve padrões distintos em cada, confere que não se sobrepõem, `HeapFree` alguns e re-aloca (reuso), e então escreve a mensagem via `WriteFile` e sai com `ExitProcess(0)`. O teste confere a saída e o reaproveitamento (ou um marcador de sucesso). Pula sem `ntdll`/`kernel32`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ntdll && cargo xtask kernel32 && cargo xtask ktest` → falha.

- [ ] **Step 3: Implementar**

Implemente o heap no `ntdll` (cuidado com aritmética de ponteiros, alinhamento, cabeçalhos; sem UB; sem allocator global — tudo manual sobre o segmento do `NtAllocateVirtualMemory`) e os wrappers no `kernel32`. Confirme que o `kernel32` ainda só importa o `ntdll` (agora com `RtlAllocateHeap`/`RtlFreeHeap` também).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `heap_alloc_write_free` passa. Demais ok. `dumpbin /imports` do `kernel32` segue só `ntdll`.

- [ ] **Step 5: Commit**

```bash
git add crates/ntdll crates/kernel32 kernel
git commit -m "feat(ntdll+kernel32): heap de usuario (RtlAllocateHeap/HeapAlloc) sobre NtAllocateVirtualMemory

Source: docs winternl.h (RtlAllocateHeap/HeapAlloc signatures)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Conformidade — um `.exe` MSVC que usa o heap roda no TukOS

**Files:**
- Create: `conformance/heap/heap.c`
- Modify: `xtask/src/conformance.rs` (caso heap), `kernel/src/conformance.rs`/`process.rs`/`build.rs` (embutir e rodar)

**Interfaces:**
- `conformance/heap/heap.c`: console freestanding (`/NODEFAULTLIB`) que declara `GetProcessHeap`/`HeapAlloc`/`HeapFree`/`GetStdHandle`/`WriteFile`/`ExitProcess`; faz `h=GetProcessHeap(); p=HeapAlloc(h,0,N); preenche p com uma mensagem; WriteFile(GetStdHandle(-11), p, N, &n, 0); HeapFree(h,0,p); ExitProcess(7);`. `cargo xtask conformance` adiciona o caso `heap`: compila linkando o nosso `kernel32.dll.lib`, roda no Windows (bind nos reais) p/ referência, roda no TukOS (exe+kernel32+ntdll + heap via NtAllocateVirtualMemory), compara a saída **e o exit code** (agora correto, =7). Mantém os casos `ntdll` e `kernel32`.

- [ ] **Step 1: Teste/comando que falha** — adicionar `heap.c` + o caso; sem implementação, falha.
- [ ] **Step 2: Rodar e ver falhar** — `cargo xtask conformance` → caso heap falha.
- [ ] **Step 3: Implementar** — compilar/rodar/comparar (saída + exit code); embutir e rodar no TukOS pelo caminho 3-módulos com o heap.
- [ ] **Step 4: Rodar e ver passar** — `cargo xtask conformance` (Windows+MSVC): casos ntdll, kernel32 e heap passam (saída idêntica; exit code 7 bate). Sem MSVC: pula limpo. `cargo xtask ktest` + gates ok.
- [ ] **Step 5: Commit**

```bash
git add xtask kernel conformance
git commit -m "feat(xtask): conformidade de um .exe MSVC que usa HeapAlloc no TukOS

Source: docs Microsoft PE Format specification (imports/IAT)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os gates passam. Marco: um `.exe` (TukOS e real do MSVC) aloca memória com `HeapAlloc` (servido pelo `NtAllocateVirtualMemory` do kernel), escreve e libera, imprime via `WriteFile`, e sai com o exit code correto — tudo idêntico ao Windows real. A dívida do exit status do Plano 10 é quitada; `cargo xtask test` passa a cobrir `pe`/`ntdll`.
- Deferreds remanescentes: P9 #1 (re-proteger IAT RO pós-bind), P9 #4 (double-map SizeOfHeaders), P10 (forward-cycle e2e, import/forward por ordinal e2e). Heap é single-thread (sem multithread/locks ainda).
- Plano 12 (degrau 3b completo): surveyar os imports do `mainCRTStartup` (dumpbin) e implementar o subconjunto do `kernel32`/`ntdll` que o CRT estático (`libcmt`) exige (`GetModuleHandleW`/`GetProcAddress`/`GetStartupInfoW`/`__security_init_cookie`/`QueryPerformanceCounter`/`GetSystemTimeAsFileTime`/`GetCurrentThreadId`/etc.), rodar um `hello.exe` compilado **com o CRT** do MSVC no TukOS e conformar — o primeiro programa C "normal" do Windows no TukOS.
