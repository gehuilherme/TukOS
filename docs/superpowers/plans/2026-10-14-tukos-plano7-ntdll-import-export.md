# TukOS Plano 7 — Parsing de import/export e o ntdll.dll real (cross-compilado)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A crate `pe` faz o parsing das tabelas de import e export de um PE; o TukOS compila um `ntdll.dll` real (crate Rust cross-compilada para `x86_64-pc-windows-msvc`) com stubs de syscall `Nt*` que fazem `mov r10,rcx; mov eax,<número>; syscall; ret`; e a export table desse `ntdll.dll` é lida pela crate `pe` (encontrando `NtWriteFile`/`NtTerminateProcess`). Também corrige o `copy_to_user` para respeitar o bit `WRITABLE`.

**Architecture:** Primeiro o conserto que a revisão do Plano 6 fixou: `copy_to_user` passa a exigir `WRITABLE` na página de destino (uma escrita numa página de usuário somente-leitura vira `STATUS_ACCESS_VIOLATION`). Depois a crate `pe` ganha o parsing das tabelas de import (nome da DLL, IAT, entradas por nome/ordinal) e export (nome → RVA), lógica pura testável no host. Por fim, o `ntdll.dll`: uma crate `crates/ntdll` compilada para o alvo do Windows produz uma DLL real com os stubs de syscall exportados; o `xtask` a compila, e um teste de host usa a crate `pe` para ler a export table da DLL gerada e confirmar que `NtWriteFile` está lá. Nenhum carregamento de dois módulos no kernel ainda — isso é o Plano 8.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), alvo `x86_64-pc-windows-msvc` (nativo do host) para o `ntdll`, crates `pe`, `mm`, `arch-x86_64`, QEMU + OVMF para o kernel.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 5: a crate `pe` compartilhada entre kernel e `ntdll`; o `ntdll` como crate Rust `cdylib` para `x86_64-pc-windows-msvc` com stubs de syscall; resolução de imports). Sucede o Plano 6 (ABI NT/executivo), já na `main`.

**Fora deste plano (vai para o Plano 8 e 9):** o kernel mapear `.exe` + `ntdll` e resolver os imports (IAT), e iniciar no thunk do `ntdll` (Plano 8); o loader de imports em modo usuário completo (`LdrInitializeThunk`), um `.exe` **real compilado com MSVC** e o harness de conformidade (Plano 9). Este plano entrega as peças puras e o artefato `ntdll.dll`, sem ainda rodá-lo no kernel.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable. Proibido usar recursos nightly. (Exceção documentada: funções `naked` são estáveis a partir do 1.88 via `#[unsafe(naked)]` + `naked_asm!`; podem ser usadas no `ntdll`. Se por algum motivo não estiverem disponíveis, usar `core::arch::global_asm!` com símbolos exportados.)
- Edition `2024`, licença `MIT OR Apache-2.0`.
- Apenas x86-64. A crate `ntdll` é compilada para `x86_64-pc-windows-msvc` (não entra em `default-members`).
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft. **Atenção:** a DLL se chama `ntdll.dll` e exporta nomes `Nt*` porque é a interface binária que os programas esperam (uso nominativo/interoperabilidade, como o Wine/ReactOS); isso é permitido pela política de clean room. Não copiar código da Microsoft.
- Clean room nível 0 apenas: o formato PE (tabelas de import/export) da especificação pública "PE Format"; as assinaturas/semântica `Nt*` da documentação pública do Microsoft Learn; os **números** de syscall são do TukOS (devem bater com os do kernel: `NT_WRITE_FILE=2`, `NT_TERMINATE_PROCESS=3`, `NT_CLOSE=4`). Nenhum binário da Microsoft é desassemblado; o `ntdll.dll` é escrito do zero em Rust.
- Commits declaram a fonte: `Source: docs Microsoft PE Format specification` (import/export) e `Source: docs Microsoft Learn x64 syscall calling convention` (stubs).
- Commits terminam com `Co-Authored-By:` nomeando o modelo que escreveu o commit.
- Código e comentários em inglês; mensagens e docs em português.
- `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -p pe -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` passam ao fim de cada tarefa. O clippy do `ntdll` roda no seu próprio alvo: `cargo clippy -p ntdll --target x86_64-pc-windows-msvc -- -D warnings`.

## Review Focus

1. **`copy_to_user` para uma página de usuário somente-leitura** (sem `WRITABLE`) deve falhar com `STATUS_ACCESS_VIOLATION`, não escrever; hoje escreve. Coberto por `copy_to_user_rejects_readonly_page` (Task 1).
2. **Uma import table malformada** (nome de DLL fora dos limites, thunk array sem terminador, RVA de hint/name fora do arquivo) deve ser rejeitada pelo parser, nunca ler fora dos limites. Coberto por `import_parse_rejects_malformed` (Task 2).
3. **Uma export table com `NumberOfNames` grande ou RVAs fora do arquivo** não pode indexar fora dos limites; o parser valida cada campo. Coberto por `export_parse_bounds` (Task 2).
4. **Os stubs de syscall do `ntdll` com o número errado** chamariam a syscall errada; o número de cada stub deve bater com o do kernel (`NtWriteFile`→2, `NtTerminateProcess`→3). Coberto por `ntdll_exports_expected_syscall_numbers` (Task 3), que lê os bytes do stub exportado e confere o imediato do `mov eax`.
5. **A DLL gerada não ser um PE válido ou não exportar os nomes esperados** quebraria o Plano 8; um teste de host faz o parsing da `ntdll.dll` gerada com a crate `pe` e confirma `NtWriteFile`/`NtTerminateProcess` na export table. Coberto por `ntdll_dll_exports_nt_functions` (Task 3).

---

### Task 1: `copy_to_user` respeita o bit WRITABLE

**Files:**
- Modify: `kernel/src/mem/paging.rs` (variante que exige WRITABLE), `kernel/src/usercopy.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Produces (`paging.rs`): `pub fn translate_user_writable_in(root: u64, virt: u64) -> Option<u64>` — como `translate_user_in`, mas a folha também exige `WRITABLE`. `copy_to_user` passa a usá-la (a validação de escrita exige página gravável).
- `copy_from_user` continua com `translate_user_in` (leitura só exige USER).

- [ ] **Step 1: Teste que falha**

Em `kernel/src/ktest.rs`, adicionar `("copy_to_user_rejects_readonly_page", copy_to_user_rejects_readonly_page),` e:
```rust
fn copy_to_user_rejects_readonly_page() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    use mm::paging::PageFlags;
    // We validate against the ACTIVE space, so map into it would be intrusive; instead
    // test the translate variant directly on a fresh space's root.
    let space = AddressSpace::new_user();
    let ro = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    let rw = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    unsafe {
        space.map(0x10_0000, ro, PageFlags::USER).map_err(|_| "map ro")?; // USER, not writable
        space.map(0x20_0000, rw, PageFlags::USER.union(PageFlags::WRITABLE)).map_err(|_| "map rw")?;
    }
    if crate::mem::paging::translate_user_writable_in(space.root(), 0x10_0000).is_some() {
        return Err("pagina RO deveria ser rejeitada para escrita");
    }
    if crate::mem::paging::translate_user_writable_in(space.root(), 0x20_0000).is_none() {
        return Err("pagina RW deveria ser aceita para escrita");
    }
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest` → falha de compilação.

- [ ] **Step 3: Implementar**

Em `paging.rs`, `translate_user_writable_in` espelha `translate_user_in` (exige USER e rejeita HUGE nos níveis), mas na folha exige também `WRITABLE`. Em `usercopy.rs`, `copy_to_user` troca a tradução por `translate_user_writable_in`; se a tradução de qualquer página for `None`, a escrita falha (o chamador em `NtWriteFile` já retorna `STATUS_ACCESS_VIOLATION` quando `copy_to_user` devolve `false`).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest` → `copy_to_user_rejects_readonly_page` passa; `ntwritefile_fills_iosb` continua passando (o `IoStatusBlock` do PE de teste está numa página gravável). Demais ok.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "fix(kernel): copy_to_user exige pagina gravavel (STATUS_ACCESS_VIOLATION em RO)

Source: docs Intel SDM Vol. 3A, secao 4.6 (bit R/W de acesso)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Crate `pe` — parsing das tabelas de import e export

**Files:**
- Modify: `crates/pe/src/lib.rs` (import/export parsing + testes)

**Interfaces:**
- Produces (`pe`):
  - `pub struct ImportDll<'a> { pub name: &'a str, pub thunks: ImportThunks<'a> }`
  - `pub fn imports(&self) -> Result<impl Iterator<Item = ImportDll<'a>>, PeError>` — itera o diretório de import (índice 1): para cada descritor, o nome da DLL e a lista de entradas; cada entrada é `pub enum ImportEntry { Ordinal(u16), Name { hint: u16, name: &'a str }, iat_rva: u32 }` (o `iat_rva` é o RVA da entrada na IAT a ser preenchida). (Modele como achar mais limpo; o essencial: para cada import, o nome da DLL, o nome/ordinal da função, e o RVA do slot da IAT.)
  - `pub struct Export<'a> { pub name: &'a str, pub rva: u32 }`
  - `pub fn exports(&self) -> Result<impl Iterator<Item = Export<'a>>, PeError>` — itera o diretório de export (índice 0): casa `AddressOfNames[i]` → `AddressOfNameOrdinals[i]` → `AddressOfFunctions[ordinal]` = RVA; devolve `(name, rva)`.
  - Erros novos: `PeError::BadImportTable`, `PeError::BadExportTable`.
  - Um helper `pub fn rva_to_offset(&self, rva: u32) -> Option<usize>` (mapeia um RVA ao offset de arquivo pela seção que o contém) — necessário para ler strings/arrays do diretório.

- [ ] **Step 1: Testes que falham (host)**

No módulo de testes de `crates/pe/src/lib.rs`, estender o `build_pe_impl` (ou um novo helper) para montar um PE com uma import table (importando `OTHER.DLL!Foo` e `OTHER.DLL!#7` por ordinal) e uma export table (exportando `Bar` no RVA 0x1234). Depois:
```rust
    #[test]
    fn parses_import_table() {
        let pe = build_pe_with_imports();
        let f = PeFile::parse(&pe).unwrap();
        let dlls: Vec<_> = f.imports().unwrap().collect();
        assert_eq!(dlls.len(), 1);
        assert_eq!(dlls[0].name, "OTHER.DLL");
        // One by-name (Foo) and one by-ordinal (#7) entry.
        // (assert the entries per your ImportEntry shape)
    }

    #[test]
    fn parses_export_table() {
        let pe = build_pe_with_exports();
        let f = PeFile::parse(&pe).unwrap();
        let exps: Vec<_> = f.exports().unwrap().collect();
        assert!(exps.iter().any(|e| e.name == "Bar" && e.rva == 0x1234));
    }

    #[test]
    fn import_parse_rejects_malformed() {
        // A PE whose import directory RVA points outside any section → BadImportTable.
        let pe = build_pe_with_bad_import_dir();
        let f = PeFile::parse(&pe).unwrap();
        assert!(matches!(f.imports().err(), Some(PeError::BadImportTable)));
    }

    #[test]
    fn export_parse_bounds() {
        let pe = build_pe_with_bad_export_dir();
        let f = PeFile::parse(&pe).unwrap();
        assert!(matches!(f.exports().err(), Some(PeError::BadExportTable)));
    }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p pe` → falha de compilação.

- [ ] **Step 3: Implementar**

Implemente o parsing com checagem de limites em cada passo (como o parser existente). Estruturas da especificação "PE Format":
- **Import directory** (data directory índice 1): array de `IMAGE_IMPORT_DESCRIPTOR` (20 bytes cada, terminado por um descritor zerado): `OriginalFirstThunk` (u32, RVA do array de hint/name = INT), `TimeDateStamp` (u32), `ForwarderChain` (u32), `Name` (u32, RVA do nome da DLL), `FirstThunk` (u32, RVA da IAT). Para cada descritor não-nulo: lê o nome da DLL (string C em `Name`); percorre o array de thunks em `OriginalFirstThunk` (ou `FirstThunk` se o primeiro for 0), terminado por um thunk 0: se o bit 63 do thunk estiver setado → import por ordinal (`thunk & 0xFFFF`); senão → RVA de um `IMAGE_IMPORT_BY_NAME` (`u16 hint` + string C). O RVA do slot da IAT correspondente é `FirstThunk + i*8`.
- **Export directory** (índice 0): `IMAGE_EXPORT_DIRECTORY`: `...`, `Base` (u32), `NumberOfFunctions` (u32), `NumberOfNames` (u32), `AddressOfFunctions` (u32 RVA → array de u32 RVAs), `AddressOfNames` (u32 RVA → array de u32 RVAs de strings), `AddressOfNameOrdinals` (u32 RVA → array de u16). Para `i` em `0..NumberOfNames`: `name = string em AddressOfNames[i]`; `ord = AddressOfNameOrdinals[i]`; `rva = AddressOfFunctions[ord]`. Devolve `(name, rva)`.
`rva_to_offset` percorre as seções achando a que contém o RVA e devolve `PointerToRawData + (rva - VirtualAddress)`, validando limites.

Valide tudo; qualquer RVA/offset fora do arquivo → `BadImportTable`/`BadExportTable`. Os helpers de teste `build_pe_with_*` montam os bytes correspondentes.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p pe` → todos passam. `cargo clippy -p pe -- -D warnings` limpo.

- [ ] **Step 5: Commit**

```bash
git add crates/pe
git commit -m "feat(pe): parsing das tabelas de import e export

Source: docs Microsoft PE Format specification (import/export directories)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: A crate `ntdll` — DLL real cross-compilada com stubs de syscall

**Files:**
- Create: `crates/ntdll/Cargo.toml`, `crates/ntdll/src/lib.rs`, `crates/ntdll/ntdll.def` (ou exports via `#[no_mangle]`)
- Modify: `Cargo.toml` (workspace: `crates/ntdll` em `members`, **não** em `default-members`)
- Modify: `xtask/src/main.rs`, `xtask/src/ntdll.rs` (novo: comando para compilar o ntdll)
- Modify: `xtask/src/*` ou um teste de host que faça o parsing da DLL gerada

**Interfaces:**
- `crates/ntdll`: `crate-type = ["cdylib"]`, `#![no_std]`, alvo `x86_64-pc-windows-msvc`. Exporta `NtWriteFile`, `NtTerminateProcess`, `NtClose` como funções `extern "system"`/`naked` cujo corpo é `mov r10, rcx; mov eax, <número do TukOS>; syscall; ret`. Os números batem com o kernel (2, 3, 4).
- `xtask vmware`-style: `cargo xtask ntdll` compila `crates/ntdll` para `x86_64-pc-windows-msvc` em release e deixa a `ntdll.dll` num caminho conhecido (`target/x86_64-pc-windows-msvc/release/ntdll.dll`). Reportar o caminho.
- Um teste de host (em `xtask` ou numa pequena `tests/` que rode no host) compila o `ntdll` (ou usa o artefato) e usa a crate `pe` para: (a) confirmar que é um PE32+ DLL válido; (b) achar `NtWriteFile`/`NtTerminateProcess` na export table; (c) ler os bytes do stub de `NtWriteFile` (via RVA do export → offset) e verificar o imediato do `mov eax` é 2.

- [ ] **Step 1: Criar a crate ntdll**

`crates/ntdll/Cargo.toml`:
```toml
[package]
name = "ntdll"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[lib]
crate-type = ["cdylib"]

[profile.release]
# keep it small/deterministic
```
(adicionar `crates/ntdll` a `members` no `Cargo.toml` raiz, não a `default-members`.)

`crates/ntdll/src/lib.rs`:
```rust
//! TukOS ntdll.dll: thin syscall stubs with the Windows x64 convention.
//! Each `Nt*` export does `mov r10, rcx; mov eax, <tukos ssn>; syscall; ret`.
//! The syscall numbers MUST match the TukOS kernel dispatch.
#![no_std]
#![allow(non_snake_case)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

// TukOS syscall numbers (must match kernel/src/syscall.rs).
// NT_WRITE_FILE = 2, NT_TERMINATE_PROCESS = 3, NT_CLOSE = 4.

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtWriteFile() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 2", "syscall", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtTerminateProcess() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 3", "syscall", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtClose() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 4", "syscall", "ret");
}
```
Se `#[unsafe(naked)]`/`naked_asm!` não compilar no 1.97 stable, use `core::arch::global_asm!` definindo os símbolos `NtWriteFile`/etc. com as mesmas instruções e declare-os para exportação. Garanta que os três nomes apareçam na export table da DLL (um `.def` com `EXPORTS NtWriteFile NtTerminateProcess NtClose`, referenciado por `[lib] ... ` ou via `-C link-args=/DEF:ntdll.def`; na prática, `#[no_mangle] pub extern "system"` já exporta em cdylib windows-msvc — confirme lendo a export table no Step 3).

- [ ] **Step 2: Comando de build no xtask**

`xtask/src/ntdll.rs`: `pub fn build() -> crate::Result<std::path::PathBuf>` que roda `cargo build -p ntdll --target x86_64-pc-windows-msvc --release` e devolve `target/x86_64-pc-windows-msvc/release/ntdll.dll`. Em `xtask/src/main.rs`, adicionar `mod ntdll;` e o braço `Some("ntdll") => ntdll::build().map(|p| println!("ntdll.dll em {}", p.display()))`, e `ntdll` ao `USAGE`.

- [ ] **Step 3: Teste de host que faz o parsing da DLL gerada**

Como um teste de integração do `xtask` (`#[test]`) ou um pequeno harness: compila o ntdll (chamando `ntdll::build()`), lê os bytes da `ntdll.dll`, e com a crate `pe`:
```rust
#[test]
fn ntdll_dll_exports_nt_functions() {
    let dll = crate::ntdll::build().expect("build ntdll");
    let bytes = std::fs::read(&dll).expect("read ntdll.dll");
    let pe = pe::PeFile::parse(&bytes).expect("ntdll.dll e um PE valido");
    let exports: Vec<_> = pe.exports().expect("export table").map(|e| e.name.to_string()).collect();
    assert!(exports.iter().any(|n| n == "NtWriteFile"), "exports: {exports:?}");
    assert!(exports.iter().any(|n| n == "NtTerminateProcess"));
    assert!(exports.iter().any(|n| n == "NtClose"));
}

#[test]
fn ntdll_exports_expected_syscall_numbers() {
    let dll = crate::ntdll::build().expect("build ntdll");
    let bytes = std::fs::read(&dll).expect("read");
    let pe = pe::PeFile::parse(&bytes).expect("parse");
    let w = pe.exports().unwrap().find(|e| e.name == "NtWriteFile").expect("NtWriteFile");
    let off = pe.rva_to_offset(w.rva).expect("rva->offset");
    // Stub: 49 C7 C2 ?? (mov r10,rcx is 49 89 CA) ... find `mov eax, imm32` = B8 <imm32 LE>.
    // Scan a few bytes of the stub for B8 and read the imm32; must be 2.
    let stub = &bytes[off..off + 16];
    let idx = stub.windows(1).position(|b| b[0] == 0xB8).expect("mov eax,imm32 (B8)");
    let imm = u32::from_le_bytes(stub[idx + 1..idx + 5].try_into().unwrap());
    assert_eq!(imm, 2, "NtWriteFile deve usar o numero de syscall 2");
}
```
Adicione `pe = { path = "../crates/pe" }` como dependência de dev do `xtask` para o teste. Nota: `mov eax, 2` costuma montar como `B8 02 00 00 00`; a varredura acima acha o `B8`. Se o compilador usar `mov r10,rcx` como `49 89 CA` e `mov eax,2` como `B8 02 00 00 00`, o teste funciona. Ajuste a varredura se a codificação diferir, mas o objetivo é confirmar o imediato 2.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ntdll` → compila e imprime o caminho da `ntdll.dll`.
Run: `cargo test -p xtask` → `ntdll_dll_exports_nt_functions` e `ntdll_exports_expected_syscall_numbers` passam. `cargo clippy -p ntdll --target x86_64-pc-windows-msvc -- -D warnings` limpo.
Run: `cargo xtask ktest` → continua passando (o kernel não mudou nesta tarefa além do que a Task 1 já fez).

Nota de clean room: o `ntdll.dll` é escrito do zero em Rust; os nomes `Nt*` e os offsets de PE são de documentação pública; nenhum binário da Microsoft é usado.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/ntdll xtask
git commit -m "feat(ntdll): ntdll.dll real (cross-compilado) com stubs de syscall Nt*

Source: docs Microsoft Learn x64 syscall calling convention; PE Format (exports)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao fim, os gates de Global Constraints passam (incluindo o clippy do `ntdll` no seu alvo). Marco: a crate `pe` faz o parsing de import/export, e o TukOS compila um `ntdll.dll` real cujos stubs de syscall usam os números do TukOS e cuja export table a crate `pe` lê e confirma.
- Limitações (Plano 8 e 9): o kernel ainda não carrega `.exe` + `ntdll` nem resolve imports; o loader em modo usuário (`LdrInitializeThunk`) e um `.exe` real do MSVC + conformidade são posteriores.
- Plano 8: o kernel mapeia o `.exe` e o `ntdll.dll`, resolve os imports do `.exe` (preenchendo a IAT com os RVAs exportados pelo `ntdll`, por enquanto pelo kernel — a migração para um loader em modo usuário no `ntdll` vem depois), e inicia o `.exe`; um `.exe` feito pelo TukOS que importa `ntdll!NtWriteFile` chama a função pela IAT, que cai no stub do `ntdll`, que faz `syscall` ao kernel, imprime "TukOS" e termina.
- Plano 9 (degrau 3a completo): o loader de imports em modo usuário no `ntdll` (`LdrInitializeThunk`, percorrendo `PEB->Ldr`), e o harness de conformidade que compila um `.exe` de console real com MSVC no Windows, roda no Windows real para a saída esperada, e compara com o TukOS.
