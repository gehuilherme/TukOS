# TukOS Plano 16 — File I/O (ler arquivos de um FS embutido)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps usam checkbox (`- [ ]`).

**Goal:** Dar ao TukOS **leitura de arquivos**. Um programa comum (CRT `/MT`) faz `fopen`/`fread`/`fgets`/`fclose` num arquivo e imprime o conteúdo — e isso confere byte-a-byte com o Windows real. É o resto do I/O manager do subprojeto 2 e pré-requisito para um shell que carrega programas de "disco". Nesta etapa o "disco" é um **filesystem read-only embutido** (initrd: mapa caminho→bytes baked no kernel); driver de bloco/FAT/NTFS reais ficam para o subprojeto 6.

**Architecture:** O CRT `fopen` desce a `kernel32!CreateFileW`; `fread`/`fgets` a `ReadFile` (já temos no handle de console — estende-se para arquivos); `fseek` a `SetFilePointer`; `fclose` a `CloseHandle`. Em vez de reimplementar o `NtCreateFile` completo (OBJECT_ATTRIBUTES/UNICODE_STRING/`\??\C:\...`), **o nosso `kernel32!CreateFileW/A` chama um syscall simplificado do TukOS** `NT_OPEN_FILE(path_utf16_ptr, path_len, ...)` que resolve o caminho no initrd (case-insensitive, ignora a drive `C:\`) e devolve um **handle de arquivo**. O object manager ganha `Object::File { content: &'static [u8], offset }`. `NtReadFile` num handle de arquivo lê de `content[offset..]` até `Length`, avança `offset`, preenche o `IO_STATUS_BLOCK`. `SetFilePointer`/`GetFileSizeEx` operam no offset/tamanho. `GetFileType` devolve `FILE_TYPE_DISK` para arquivos (e `FILE_TYPE_CHAR` para o console, como hoje). Clean room nível 0: comportamento de `CreateFile`/`ReadFile`/`SetFilePointer` da documentação pública (MSDN); o syscall e o FS são do TukOS; nenhum binário/disassembly da MS.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), `kernel`, `crates/ntdll`/`kernel32`, QEMU + OVMF, MSVC no host p/ conformidade. Testes headless.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (subprojeto 2: I/O manager — arquivos). Sucede o Plano 15, já na `main`.

**Fora deste plano:** escrita em arquivo (read-only aqui); diretórios/enumeração (`FindFirstFile`); FS/driver de bloco reais (FAT/NTFS/AHCI/NVMe = subprojeto 6); o `NtCreateFile` completo com OBJECT_ATTRIBUTES (staged: usamos um open simplificado); `CreateProcess`/shell (Plano 17). Caminhos: só os presentes no initrd; ausente → `INVALID_HANDLE_VALUE` + `GetLastError`=ERROR_FILE_NOT_FOUND.

## Global Constraints

- Rust `1.97.1` stable; sem nightly. Edition 2024. x86-64.
- Clean room nível 0 (MSDN/winternl públicos; sem binário/disassembly da MS). REA só nos nossos artefatos.
- Invariantes mantidas: faults de usuário contidos (P14); ponteiros de usuário validados (`copy_from_user`/`copy_to_user`); args ULONG do slot lidos como `u32` (P15); `kernel32` importa só `ntdll`; `ntdll`/`kernel32` sem heap do Rust.
- Testes headless (só `cargo xtask run` abre janela). `build.rs`/`xtask`: sem MSVC, tudo que depende dele se auto-pula; Linux CI verde.
- Gates ao fim de cada tarefa: `cargo fmt --all -- --check`, clippy (`-p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings` + `-p xtask -p arch-x86_64 -p mm -p pe -p kbd -- -D warnings`), `cargo xtask test`, `cargo xtask ktest`.
- Commits: `Source: docs MSDN (CreateFile/ReadFile/SetFilePointer)`; `Co-Authored-By:` do modelo. Ao fim, atualizar `ROADMAP.md` (subprojeto 2: I/O de arquivos).

## Review Focus

1. **Resolução de caminho segura:** `NT_OPEN_FILE` lê o caminho do usuário com `copy_from_user` (comprimento limitado), normaliza (ignora `C:\`/`\??\`, case-insensitive, sem path traversal que escape o initrd), e só devolve handle para um arquivo presente; ausente → erro limpo (sem pânico, sem leitura fora dos bytes do initrd).
2. **`NtReadFile` de arquivo correto:** lê `min(Length, content.len()-offset)` de `content[offset..]`, avança `offset`, EOF devolve 0 bytes (Information=0, sucesso); `copy_to_user` valida o buffer; sem overflow de offset/len (checked). Não confunde handle de arquivo com o de console.
3. **Handles por processo:** cada `CreateFile` aloca um handle novo na `HandleTable` do processo; `CloseHandle` libera; `offset` é por-handle (duas aberturas do mesmo arquivo são independentes). Sem vazar handle/estado entre processos.
4. **Win32 fiel:** `CreateFileW/A` (abre/!encontra), `SetFilePointer`/`GetFileSizeEx` (offset/tamanho), `GetFileType`=DISK p/ arquivo, `ReadFile` (arquivo e console), `CloseHandle`; `GetLastError`=ERROR_FILE_NOT_FOUND quando ausente. `kernel32` importa só `ntdll`.
5. **Conformidade real:** um `.exe` `/MT` que lê um arquivo e o imprime roda idêntico no Windows (arquivo no disco) e no TukOS (mesmo arquivo no initrd). Pula limpo sem MSVC.

---

### Task 1: Survey dos imports de um programa que lê arquivo (fopen/fread)

**Files:** Create `conformance/fileio/fileio.c` (`fopen`+`fread`/`fgets`+`fputs`/`printf`+`fclose` de um arquivo fixo, ex. `cat.txt`), `docs/observations/2026-10-14-fileio-imports-survey.md`

- [ ] **Step 1:** `fileio.c`: abre `"C:\\cat.txt"` (ou um caminho simples), lê o conteúdo e imprime (`printf("%s", buf)` ou escreve os bytes), `fclose`, return 0.
- [ ] **Step 2:** `cl /MT /GS- fileio.c`; `dumpbin /imports fileio.exe` (só o NOSSO binário) e registrar o delta de imports vs o que já temos (CreateFileW/A, ReadFile✓, SetFilePointer(Ex), GetFileSizeEx/GetFileSize, GetFileType✓, CloseHandle✓, GetFullPathNameW?, etc.).
- [ ] **Step 3:** triar em `docs/observations/2026-10-14-fileio-imports-survey.md` (já temos / stub / load-bearing) + a assinatura MSDN de cada novo. Definir o split T2 (kernel: open/read/seek + FS) vs T3 (kernel32 wrappers + programa).
- [ ] **Step 4/5:** sem implementar; commit (`docs(fileio): survey dos imports de fopen/fread (/MT)`, `Source: blackbox conformance/fileio/fileio.c`).

---

### Task 2: Kernel — `Object::File` + initrd read-only + `NT_OPEN_FILE`/`NtReadFile`(arquivo)/seek

**Files:** Modify `kernel/src/object.rs` (variante File), `kernel/src/syscall.rs` (NT_OPEN_FILE + read de arquivo + seek/size), `kernel/src/fs.rs` (novo: initrd caminho→bytes), `kernel/build.rs` (embutir os arquivos do initrd), `crates/ntdll/src/lib.rs` (stubs), `kernel/src/ktest.rs`

**Interfaces:**
- `kernel/src/fs.rs`: um initrd read-only — `pub fn lookup(path: &str) -> Option<&'static [u8]>` sobre um mapa de (caminho normalizado → `include_bytes!`). Normalização: lowercase, remove prefixo de drive (`c:\`/`\??\c:\`), separador `\`→`/`, sem `..`.
- object.rs: `Object::File { content: &'static [u8], offset: usize }`; `HandleTable::insert` devolve um handle novo (esquema (idx+1)*4 já existente; não colidir com CONSOLE=4/CONSOLE_INPUT=8).
- syscall.rs: `NT_OPEN_FILE` (nr livre, ex. 8 — confirmar; 0..7 em uso) `(path_ptr: u16*, path_len_bytes, out_handle_ptr, desired_access, ...)`: `copy_from_user` do caminho UTF-16 (limita o tamanho), converte p/ `&str` (ASCII/UTF-16→UTF-8 simples), `fs::lookup`, se achar cria `Object::File{content, offset:0}` na HandleTable do processo e escreve o handle via `copy_to_user`, devolve STATUS_SUCCESS; ausente → STATUS_OBJECT_NAME_NOT_FOUND. `NtReadFile` (já existe p/ console): no handle de arquivo, lê `min(len, content.len()-offset)` de `content[offset..]` via `copy_to_user`, avança offset, Information=bytes; EOF→0. Um `NT_SET_FILE_POINTER`/`NT_QUERY_FILE_SIZE` (ou reusar um esquema) p/ seek/size.
- ntdll: stubs dos novos syscalls.

- [ ] **Step 1: Teste que falha** — ktest `file_open_read_eof`: abre um arquivo do initrd (`fs::lookup` via o caminho de syscall num processo de teste), lê tudo, confere o conteúdo, lê de novo→EOF(0); abrir ausente→erro; seek + re-read. 
- [ ] **Step 2-4:** ver falhar; implementar; ver passar. clippy/fmt ok.
- [ ] **Step 5: Commit** (`feat(kernel): initrd read-only + NT_OPEN_FILE/NtReadFile de arquivo + seek`, `Source: docs MSDN (NtReadFile semantics)`).

---

### Task 3: kernel32 `CreateFileW/A`/`SetFilePointer`/`GetFileSizeEx`/`GetFileType`(disk) + programa de teste

**Files:** Modify `crates/kernel32/src/lib.rs`, `kernel/src/ktest.rs` (+ processo de teste que lê arquivo)

**Interfaces:** `CreateFileW(name, access, share, sa, disp, flags, template)` → converte/encaminha ao `NT_OPEN_FILE`; devolve o handle ou INVALID_HANDLE_VALUE (-1) + SetLastError(ERROR_FILE_NOT_FOUND). `CreateFileA` idem (converte ANSI→o que o syscall espera). `SetFilePointer`/`SetFilePointerEx` → seek. `GetFileSizeEx`/`GetFileSize` → tamanho. `GetFileType` devolve FILE_TYPE_DISK p/ handle de arquivo, FILE_TYPE_CHAR p/ console. `ReadFile` (já) funciona p/ arquivo. `CloseHandle` (já) fecha. kernel32 importa só ntdll.

- [ ] **Step 1: Teste que falha** — ktest `program_reads_file`: um `.exe` de teste (TukOS-built ou /MT) abre `C:\cat.txt` (no initrd), lê e escreve o conteúdo no stdout; assere a saída. 
- [ ] **Step 2-4:** ver falhar; implementar; ver passar; `dumpbin` kernel32 só ntdll.
- [ ] **Step 5: Commit** (`feat(kernel32): CreateFile/SetFilePointer/GetFileSize/GetFileType(disk) sobre NtOpenFile`, `Source: docs MSDN`).

---

### Task 4: Conformidade — um `.exe` /MT que lê um arquivo + ROADMAP

**Files:** Modify `xtask/src/conformance.rs` (caso `fileio`: escreve `cat.txt` no lado Windows e embute o mesmo no initrd do TukOS), `kernel/src/conformance.rs`/`build.rs`, `ROADMAP.md`

- [ ] **Step 1-4:** caso `fileio`: compila `fileio.c` `/MT`; no Windows escreve `cat.txt` com um conteúdo fixo ao lado do exe e roda (lê do disco), captura stdout; no TukOS embute o MESMO `cat.txt` no initrd e roda; compara stdout + exit. Mantém os 5 casos anteriores. Pula limpo sem MSVC. Atualizar `ROADMAP.md` (subprojeto 2: I/O de arquivos presente; Marco/Em aberto).
- [ ] **Step 5: Commit** (`feat(xtask): conformidade de um .exe /MT que le um arquivo; ROADMAP`, `Source: docs MSDN`).

---

## Notas de fechamento

- Marco: um programa comum lê um arquivo e imprime o conteúdo, idêntico ao Windows — I/O de arquivos (leitura) do subprojeto 2 presente.
- Residual/futuro: escrita em arquivo; diretórios/`FindFirstFile`; `NtCreateFile` completo (OBJECT_ATTRIBUTES); FS/driver reais (subprojeto 6); registro.
- Deferreds remanescentes: P9 #1/#4, P10 (forward-cycle/ordinal e2e), P11 (NtFree untracked; TOCTOU; first-fit O(n)), P13 (não-isolamento entre testes; dead-code sob features conformance), P14 (#GP sem teste; exit code só logado), P15 (teclado por polling não-IRQ; sem launcher de echo no `run`; edição de linha mínima).
- Depois: **Plano 17 = `CreateProcess` + um shell/REPL mínimo** (lê uma linha do teclado, encontra o programa no initrd, executa) — o terminal usável que o usuário perguntou.
