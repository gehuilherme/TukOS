# TukOS — Marco 1: boot no QEMU e execução de `.exe` real do Windows

- **Data:** 2026-10-13
- **Status:** design aprovado em conversa; aguardando revisão desta spec
- **Escopo deste documento:** visão geral do projeto, política de clean room e design detalhado do Marco 1 (subprojetos 0, 1 e 3 em versão mínima)

---

## 1. Visão do projeto

TukOS é um sistema operacional escrito do zero, em Rust, que pretende ser **equivalente ao Windows em compatibilidade** (rodar aplicativos e drivers do Windows sem modificação) e **menos intrusivo** (sem telemetria, sem anúncios, sem imposições ao usuário).

Inspiração de método: o projeto PhotoCraft (reimplementação clean-room do Photoshop em Rust), do qual herdamos:

- reimplementação clean-room a partir de especificações públicas e comportamento observado;
- um **scorecard de conformidade** como métrica honesta de progresso;
- repositório pensado para colaboradores humanos e agentes de IA (`AGENTS.md`, crates pequenas, CI).

### Objetivos do projeto (longo prazo)

1. Utilizável no dia a dia por pessoas comuns, com uso geral misto (produtividade, navegação, utilitários, jogos).
2. Privacidade e controle do usuário por padrão.
3. Compatível com a maior parte do hardware, por meio de compatibilidade com o modelo de drivers do Windows.

### Decisões de arquitetura (fechadas)

| Decisão | Escolha |
|---|---|
| Kernel | Próprio, do zero. Sem kernel Linux. |
| Fidelidade ao NT | **Compatível nas fronteiras**: interface exportada do `ntdll` e ABI de drivers (WDM/KMDF, exportações equivalentes às do `ntoskrnl`). Implementação interna livre. |
| Linguagem | Rust |
| Arquitetura de CPU | Apenas x86-64 nesta fase; código dependente de CPU isolado na crate `arch-x86_64` |
| Programas 32 bits | Fora do escopo agora; subprojeto futuro equivalente ao WoW64 |
| Boot | UEFI, via bootloader Limine (BSD) neste marco; loader próprio no futuro |
| Licença | MIT OR Apache-2.0 |
| Nome | TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft no nome ou na marca. |

### Subprojetos

Cada subprojeto terá seu próprio ciclo de spec → plano → implementação.

| # | Subprojeto | Resultado demonstrável |
|---|---|---|
| 0 | Infraestrutura: repositório, build, QEMU, CI, `AGENTS.md`, `CLEAN_ROOM.md`, harness de conformidade | `cargo xtask run` dá boot numa VM |
| 1 | Kernel base: boot UEFI, memória, interrupções, scheduler, syscalls | kernel roda e alterna threads |
| 2 | Executivo: object manager, processos, I/O manager, registro | processos e handles estilo NT |
| 3 | `ntdll` + loader de PE | `.exe` de console do Windows roda sem modificação |
| 4 | Subsistema Win32: `kernel32`, `user32`, `gdi32`, gerenciador de janelas | programa com janela abre |
| 5 | Modelo de drivers WDM/KMDF | driver `.sys` real carrega |
| 6 | Drivers e sistemas de arquivos: NVMe/AHCI, NTFS/FAT, USB, rede, vídeo | instala e roda em hardware real |
| 7 | Shell, desktop, instalador, privacidade | usável no dia a dia |

**Marco 1 = subprojetos 0, 1 e 3 em versão mínima, mais o executivo mínimo necessário (parte do 2).**

---

## 2. Política de clean room

Esta seção vira o arquivo `CLEAN_ROOM.md` na raiz do repositório.

### Proibido sempre

- Consultar, possuir no ambiente de trabalho ou usar qualquer **código-fonte vazado** da Microsoft (incluindo vazamentos do Windows XP/Server 2003 e o Windows Research Kernel).
- Copiar código, shaders, recursos gráficos, ícones, sons ou textos da Microsoft.

### Níveis de fonte

| Nível | Fontes | Quem pode usar | Onde o resultado fica |
|---|---|---|---|
| **0 — Documentação pública** | Microsoft Learn, WDK/SDK headers públicos (`winternl.h` etc.), especificações (PE/COFF, UEFI, ACPI), livros técnicos | Todos | Repositório público `tukos` |
| **0 — Caixa-preta** | Saída e código de saída de **programas de teste nossos** executados no Windows real | Todos | `tukos/conformance/expected/` |
| **1 — Observação externa** | Metadados de interface (`dumpbin /exports`, `/headers`, `/imports` — **nunca `/disasm`**), rastreamento nas fronteiras de API (API Monitor, Process Monitor, ETW) ao rodar programas nossos, estrutura do sistema (WinObj, registro, serviços, `System32`) | Sessões de observação; resultado (descrição de comportamento) pode ser lido por todos | Repositório público `tukos`, em `docs/observations/` |
| **2 — Engenharia reversa** | Disassembly/descompilação (Ghidra, WinDbg) de binários da instalação licenciada do Windows do mantenedor | **Somente sessões sujas isoladas** | Specs no repositório **privado** `tukos-specs`; material bruto (projetos Ghidra, listagens) em pasta local fora de qualquer git |

### Processo do nível 2

1. **Pedido de spec:** registra a função ou comportamento a especificar e **por que os níveis 0 e 1 não bastaram**.
2. **Sessão suja:** um subagente isolado, sem contexto compartilhado com sessões limpas, analisa o binário e escreve uma spec de comportamento contendo apenas:
   - entradas, saídas, efeitos colaterais e ordem observável das operações;
   - códigos de erro (`NTSTATUS` / `GetLastError`) e em que condições ocorrem;
   - layouts de estruturas (offsets e tamanhos de campos) necessários à interoperabilidade.
3. **Revisor de spec:** outra sessão isolada rejeita a spec se ela contiver código, pseudocódigo, tradução instrução a instrução, ou nomes internos não públicos.
4. **Sessão limpa:** implementa somente a partir da spec aprovada. Nunca acessa material bruto.
5. **Papéis humanos:** o mantenedor atua **apenas no lado limpo** — lê specs aprovadas, revisa e dirige, mas não vê disassembly. Pode, portanto, escrever e revisar código em qualquer área.
6. O nível 2 nunca é necessário para o Marco 1 e só é usado quando os níveis 0 e 1 se mostrarem insuficientes.

### Proveniência

Todo commit que implementa comportamento compatível declara a fonte no rodapé da mensagem:

```
Source: docs <URL ou documento>
Source: blackbox <teste em conformance/>
Source: observation <arquivo em docs/observations/>
Source: spec-L2 <id da spec em tukos-specs>
```

Isso permite localizar e reescrever tudo que veio do nível 2, caso uma avaliação jurídica futura recomende.

### Avaliação jurídica

Antes da divulgação pública do projeto, recomenda-se consulta a um advogado de propriedade intelectual sobre a Lei 9.609/98 (Lei de Software), a cláusula de engenharia reversa do EULA do Windows e patentes. Clean room protege contra alegação de cópia de código, não contra patentes nem violação contratual.

---

## 3. Estrutura do repositório e infraestrutura

```
tukos/
├── README.md  ROADMAP.md  AGENTS.md  CLEAN_ROOM.md  LICENSE-MIT  LICENSE-APACHE
├── Cargo.toml                 # workspace
├── rust-toolchain.toml        # toolchain fixada
├── kernel/                    # binário do kernel (alvo x86_64-unknown-none)
├── crates/
│   ├── arch-x86_64/           # GDT, IDT, paginação, APIC, SYSCALL — tudo que depende da CPU
│   ├── mm/                    # alocador físico e estruturas de memória virtual (lógica pura, testável no host)
│   ├── pe/                    # parser e mapeador de PE/COFF (no_std, testado no host)
│   └── nt-types/              # NTSTATUS, constantes e estruturas da documentação pública
├── user/
│   ├── ntdll/                 # ntdll.dll em Rust
│   └── kernel32/              # kernel32.dll em Rust
├── conformance/
│   ├── programs/
│   │   ├── 3a/                # testes só com ntdll
│   │   ├── 3b/                # testes com kernel32, sem CRT
│   │   └── 3c/                # testes com CRT estática do MSVC
│   └── expected/              # saídas capturadas no Windows real (versionadas)
├── scorecard/                 # scorecard gerado (JSON e Markdown)
├── xtask/                     # automação de build, execução e conformidade
└── docs/
    ├── superpowers/specs/     # specs de design
    ├── adr/                   # registros de decisão de arquitetura
    └── observations/          # resultados do nível 1
```

### Comandos `cargo xtask`

| Comando | O que faz |
|---|---|
| `build` | Compila kernel e DLLs e gera imagem de disco UEFI com Limine e módulos de boot |
| `run` | Dá boot da imagem no QEMU com OVMF, serial no terminal |
| `test` | Roda os testes unitários no host (`pe`, `mm`, `nt-types`) |
| `conformance` | Para cada programa de teste: um boot do QEMU, coleta a serial, compara com `expected/`, gera o scorecard |
| `capture` | (Somente em Windows) compila e executa os programas de teste no Windows real e atualiza `expected/` |

### CI (GitHub Actions)

1. **Job Windows:** compila os programas de teste com MSVC, executa `capture` e falha se a saída divergir de `expected/` versionado. Publica os `.exe` como artefatos.
2. **Job Linux:** `cargo xtask test`, `cargo xtask build`, baixa os `.exe` do job Windows e roda `cargo xtask conformance` no QEMU sem tela.
3. Publica o scorecard (Markdown) e atualiza a seção correspondente do README na branch principal.

Binários `.exe` não são versionados no repositório.

### Documentos para colaboradores

- `AGENTS.md`: como humanos e agentes de IA trabalham no repositório, incluindo a regra de que sessões limpas nunca acessam material de nível 2.
- `CLEAN_ROOM.md`: conteúdo da seção 2 desta spec.
- `ROADMAP.md`: tabela de subprojetos da seção 1.

---

## 4. Kernel (escopo do Marco 1)

### Sequência de boot

1. Limine carrega o kernel ELF em *higher half*, fornecendo: mapa de memória, framebuffer, mapeamento direto da memória física (HHDM), módulos de boot (arquivos `.exe`/`.dll`) e linha de comando.
2. Inicialização da saída de diagnóstico: serial COM1 (logs e protocolo de testes) e console de texto no framebuffer.
3. GDT, TSS e IDT. Toda exceção da CPU tem tratador; exceções em modo kernel geram panic com registradores.
4. Memória:
   - alocador de páginas físicas construído a partir do mapa do Limine;
   - paginação de 4 níveis, com um espaço de endereçamento por processo (metade superior compartilhada com o kernel);
   - heap do kernel habilitando a crate `alloc`.
5. Timer do LAPIC e scheduler preemptivo round-robin, com threads de kernel e de usuário. Apenas um núcleo de CPU neste marco.
6. Entrada em modo usuário e retorno via `SYSCALL/SYSRET`, com tabela de dispatch de syscalls.

### Executivo mínimo

- **Object manager:** objetos com contagem de referências. Tipos neste marco: `Process`, `Thread`, `File`, `Section`. Namespace mínimo com `\Device\Console` e `\SystemRoot\System32\`.
- **Tabela de handles por processo:** `HANDLE` são índices na tabela do processo, como no NT.
- **Volume de boot em memória:** volume somente-leitura construído a partir dos módulos do Limine, exposto como `\SystemRoot\System32\<arquivo>`. Substituído por disco real no subprojeto 6, sem mudanças no loader.
- **Dispositivo de console:** objeto `File` que escreve na serial e no framebuffer; é o alvo de `NtWriteFile` nos handles padrão.

### Syscalls

Os números de syscall são **definidos pelo TukOS** e conhecidos apenas pelo nosso `ntdll`. A compatibilidade exigida é com a **interface exportada pelo `ntdll`** (nomes, assinaturas e semântica das funções `Nt*`/`Zw*`), não com os números do Windows.

Syscalls do Marco 1:

| Syscall | Uso |
|---|---|
| `NtWriteFile` | escrita no console |
| `NtReadFile` | leitura de arquivos do volume de boot |
| `NtOpenFile` / `NtCreateFile` (somente abertura) | abrir DLLs |
| `NtQueryInformationFile` | tamanho de arquivo |
| `NtCreateSection` (incluindo `SEC_IMAGE`) | seção de imagem PE |
| `NtMapViewOfSection` / `NtUnmapViewOfSection` | mapear DLLs |
| `NtAllocateVirtualMemory` / `NtFreeVirtualMemory` / `NtProtectVirtualMemory` / `NtQueryVirtualMemory` | memória do processo e heap |
| `NtClose` | fechar handles |
| `NtTerminateProcess` / `NtTerminateThread` | término |
| `NtQueryInformationProcess` (classes mínimas) | informações exigidas pela CRT |
| `NtQuerySystemInformation` (classes mínimas) | informações exigidas pela CRT |
| `NtQueryPerformanceCounter` / `NtQuerySystemTime` | tempo, inicialização do cookie de segurança da CRT |

A lista final é determinada pelos testes 3c; adições seguem a mesma regra.

### Fora do escopo do Marco 1

Disco e sistemas de arquivos reais, multiprocessamento, ACPI além do necessário ao LAPIC, drivers, gráficos além do console de texto, rede, registro.

---

## 5. Lado do usuário

### Criação de processo (modelo NT)

1. O kernel cria o processo, mapeia **somente o `.exe` e o `ntdll.dll`**, cria PEB, TEB e `RTL_USER_PROCESS_PARAMETERS`, e inicia a primeira thread no ponto de entrada interno de inicialização do nosso `ntdll` (equivalente funcional ao `LdrInitializeThunk`).
2. O loader em modo usuário (`ntdll`):
   1. percorre a tabela de imports do `.exe`;
   2. carrega cada DLL via `NtOpenFile` → `NtCreateSection(SEC_IMAGE)` → `NtMapViewOfSection`;
   3. aplica relocações de base quando a imagem não fica no `ImageBase` preferido;
   4. resolve imports por nome e por ordinal, incluindo forwarders;
   5. registra cada módulo nas listas de `PEB->Ldr`;
   6. inicializa TLS estática e executa callbacks de TLS e `DllMain(DLL_PROCESS_ATTACH)` na ordem de dependência;
   7. chama o ponto de entrada do `.exe`.

A lógica de parsing e mapeamento de PE fica na crate `pe`, compartilhada entre o kernel (mapeamento de `SEC_IMAGE`) e o `ntdll` (resolução de imports).

### PEB e TEB

- `GS` aponta para o TEB do thread corrente.
- Campos implementados, com layout conforme `winternl.h` e documentação pública:
  - **TEB:** `NtTib` (incluindo `Self` e limites de pilha), `ProcessEnvironmentBlock`, `ClientId`, `LastErrorValue`, `ThreadLocalStoragePointer`.
  - **PEB:** `ImageBaseAddress`, `Ldr` (`PEB_LDR_DATA` com as três listas de módulos), `ProcessParameters`, `ProcessHeap`, `NumberOfProcessors`, versão do sistema.
  - **`RTL_USER_PROCESS_PARAMETERS`:** `StandardInput/Output/Error`, `CommandLine`, `ImagePathName`, `Environment`, `CurrentDirectory`.
- Campos não documentados exigidos por um teste são definidos por observação (nível 1) ou, se necessário, spec de nível 2, com proveniência registrada.

### DLLs

- Escritas em Rust, `no_std`, crate-type `cdylib`, alvo `x86_64-pc-windows-msvc`, linkadas com `rust-lld`.
- Exportações e ordinais controlados por arquivo `.def`.
- **`ntdll`:**
  - stubs de syscall no formato `mov r10, rcx; mov eax, <número TukOS>; syscall; ret`;
  - loader (`LdrLoadDll`, `LdrGetProcedureAddress`, `LdrGetDllHandle`);
  - heap (`RtlCreateHeap`, `RtlAllocateHeap`, `RtlReAllocateHeap`, `RtlFreeHeap`, `RtlSizeHeap`);
  - strings (`RtlInitUnicodeString`, `RtlInitAnsiString`, conversões Unicode/ANSI);
  - exceções x64: `RtlLookupFunctionEntry`, `RtlVirtualUnwind`, `RtlCaptureContext`, `RtlUnwindEx`, despacho de exceções a partir do kernel (`KiUserExceptionDispatcher` equivalente).
- **`kernel32`:**
  - degrau 3b: `GetStdHandle`, `WriteConsoleW`, `WriteFile`, `ExitProcess`, `GetLastError`, `SetLastError`;
  - degrau 3c: a lista exata vem de `dumpbin /imports` executado sobre o **nosso próprio** `hello.exe` compilado com CRT estática. Estimativa: 40 a 60 funções.

### APIs não implementadas

Todo import que o loader não consegue resolver é ligado a um stub gerado que:

1. escreve `@@TUKOS-UNIMPLEMENTED <dll>!<função>` na serial;
2. termina o processo com o código `0xE0DE0001` (código de saída TukOS reservado para "API não implementada").

O loader não falha ao encontrar um import inexistente; a falha só ocorre se a função for de fato chamada.

---

## 6. Conformidade, testes e erros

### Programas de teste

- Um arquivo C por teste em `conformance/programs/<degrau>/<nome>.c`.
- **3a:** sem CRT (`/NODEFAULTLIB`), ponto de entrada próprio, importa apenas `ntdll`.
- **3b:** sem CRT, importa `kernel32`.
- **3c:** CRT estática do MSVC (`/MT`), `printf` comum.
- Primeiros testes obrigatórios:

| Teste | Degrau | Verifica |
|---|---|---|
| `nt_hello` | 3a | `NtWriteFile` no handle de saída padrão vindo de `ProcessParameters`, `NtTerminateProcess` com código 0 |
| `nt_exitcode` | 3a | código de saída arbitrário (`42`) |
| `k32_hello` | 3b | `GetStdHandle` + `WriteConsoleW` com texto Unicode |
| `k32_lasterror` | 3b | `SetLastError`/`GetLastError` |
| `crt_hello` | 3c | `printf("Hello, TukOS\n")` |
| `crt_args` | 3c | `argc`/`argv` a partir da linha de comando |
| `crt_av` | 3c | acesso inválido termina com `0xC0000005`, igual ao Windows |

### Execução e protocolo

- Um boot do QEMU por teste. A linha de comando do kernel (configuração do Limine) indica o `.exe` a executar como primeiro processo e sua linha de comando.
- O kernel emite na serial:
  - `@@TUKOS-TEST-BEGIN <nome>`
  - a saída padrão do processo, byte a byte
  - `@@TUKOS-TEST-END <nome> status=0x<código de saída>`
- O harness compara a saída e o código de saída com `conformance/expected/<nome>.stdout` e `<nome>.exitcode`.
- Resultados:

| Resultado | Condição |
|---|---|
| `PASS` | saída e código de saída idênticos ao Windows |
| `FAIL` | saída ou código divergentes |
| `UNIMPLEMENTED` | stub de API não implementada foi chamado; registra a API |
| `TIMEOUT` | sem `TEST-END` em 10 segundos |
| `PANIC` | panic do kernel detectado na serial |

### Scorecard

- Gerado em `scorecard/scorecard.json` e `scorecard/SCORECARD.md`.
- Conteúdo: porcentagem de `PASS` por degrau e total; tabela por teste; **ranking das APIs não implementadas ordenadas pelo número de testes que bloqueiam**.
- A CI publica o resumo no README.

### Testes unitários e de integração

- `cargo xtask test` no host:
  - `pe`: parsing dos `.exe` reais de conformidade; imagens corrompidas e truncadas; alvo de fuzzing com `cargo fuzz`.
  - `mm`: invariantes do alocador físico (sem sobreposição, sem perda de páginas).
  - `nt-types`: tamanhos e offsets de estruturas conferidos com valores da documentação pública.
- Testes de integração do kernel no QEMU com framework de testes próprio, encerrando via dispositivo `isa-debug-exit`.

### Tratamento de erros

- **Panic do kernel:** despejo de registradores e backtrace na serial, linha `@@TUKOS-PANIC`, e saída do QEMU com código de falha.
- **Syscalls:** validam todo ponteiro e tamanho vindo de modo usuário e retornam `NTSTATUS`. Nenhuma entrada de modo usuário pode causar panic do kernel.
- **Falha em modo usuário:** a exceção é despachada ao processo; se não tratada, o processo termina com o mesmo `NTSTATUS` que o Windows usaria (por exemplo, `0xC0000005` para acesso inválido).
- **PE malformado:** `NtCreateSection(SEC_IMAGE)` retorna `STATUS_INVALID_IMAGE_FORMAT`.

---

## 7. Critério de conclusão do Marco 1

1. `cargo xtask run` dá boot do TukOS no QEMU com UEFI e mostra o console.
2. Todos os testes da tabela da seção 6 passam (`PASS`) em `cargo xtask conformance` e na CI.
3. Scorecard publicado no README.
4. `README.md`, `AGENTS.md`, `CLEAN_ROOM.md` e `ROADMAP.md` presentes e coerentes com esta spec.
5. Todos os commits de implementação de comportamento compatível com proveniência declarada.
