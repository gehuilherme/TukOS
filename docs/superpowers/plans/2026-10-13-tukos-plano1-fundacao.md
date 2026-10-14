# TukOS Plano 1 — Fundação: boot, diagnóstico e infraestrutura

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cargo xtask run` dá boot do TukOS no QEMU via UEFI + Limine, com serial, console na tela, GDT/TSS/IDT, exceções tratadas, panic diagnosticável e autotestes do kernel rodando na CI.

**Architecture:** Workspace Cargo com o kernel (`x86_64-unknown-none`, fora dos `default-members`), a crate `arch-x86_64` (tudo que depende da CPU; codificação de descritores testável no host) e o `xtask` (build da partição EFI, execução do QEMU, autotestes). Os autotestes do kernel são compilados por feature (`ktest`, `ktest-panic`, `ktest-fault`), emitem marcadores `@@TUKOS-*` na serial e encerram o QEMU via `isa-debug-exit`.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem recursos nightly), crate `limine` 0.5, `spin` 0.10, `noto-sans-mono-bitmap` 0.3, bootloader Limine (branch binária), QEMU + OVMF, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seções 1–4 e 6). Este é o **Plano 1 de 4** do Marco 1. Planos seguintes: (2) memória, scheduler, modo usuário e syscalls; (3) executivo, crate `pe`, loader do `ntdll`, harness de conformidade e degrau 3a; (4) `kernel32`, exceções x64, degraus 3b/3c e scorecard.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable via `rust-toolchain.toml`. Proibido usar recursos nightly (`x86-interrupt`, `custom_test_frameworks`, `build-std`).
- Edition `2024` em todas as crates. Licença `MIT OR Apache-2.0` em todos os `Cargo.toml`.
- Apenas x86-64. Código dependente de CPU vive em `crates/arch-x86_64`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft em nomes, marcas ou textos de interface.
- Política de clean room (spec seção 2): proibido código vazado da Microsoft; este plano usa apenas fontes de nível 0 (Intel SDM, especificação do protocolo Limine, documentação do QEMU/UART 16550).
- Todo commit que implementa comportamento de hardware/protocolo declara a fonte no rodapé: `Source: docs <documento>`.
- Commits terminam com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Identificadores e comentários de código em inglês; documentação do repositório em português.
- Marcadores de serial (contrato com o `xtask`): `@@TUKOS-KTEST-PASS <nome>`, `@@TUKOS-KTEST-FAIL <nome> <motivo>`, `@@TUKOS-KTEST-DONE passed=<n> failed=<n>`, `@@TUKOS-PANIC <mensagem>`.
- Códigos `isa-debug-exit`: escrever `0x10` (sucesso) → QEMU sai com 33; escrever `0x11` (falha) → QEMU sai com 35.

## Review Focus

1. **Exceção de CPU no kernel** (ex.: page fault em endereço não mapeado) deve gerar `@@TUKOS-PANIC` com vetor, nome e CR2 — nunca triple fault/reboot silencioso. Coberto pelo kernel `ktest-fault` (Task 6) checado pelo `xtask`.
2. **O próprio caminho de panic** deve funcionar mesmo se o lock da serial estiver tomado. Coberto por `ktest-panic` (Task 5) e pelo `force_unlock` no handler.
3. **Kernel travado** (sem marcador final) não pode pendurar a CI: o `xtask` mata o QEMU após timeout e reporta `Timeout`. Coberto por `wait_with_timeout_kills_hung_process` (Task 2) e `check_ktest` com `Timeout` (Task 3).
4. **QEMU ou OVMF ausentes** devem produzir mensagem dizendo o que foi procurado e como instalar, não um erro críptico. Coberto por `missing_message_lists_paths_and_hint` (Task 2).
5. **Saída da serial suja** (texto do OVMF/Limine, sequências ANSI, `\r\n`) não pode confundir o parser. Coberto por `parse_ignores_noise_and_crlf` (Task 3).

---

### Task 1: Esqueleto do repositório e documentos

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`, `.gitattributes`
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`
- Create: `README.md`, `AGENTS.md`, `CLEAN_ROOM.md`, `ROADMAP.md`, `LICENSE-MIT`, `LICENSE-APACHE`

**Interfaces:**
- Produces: alias `cargo xtask <cmd>`; `xtask/src/main.rs` com `fn main() -> ExitCode` que despacha subcomandos (as tasks seguintes adicionam módulos e braços do `match`); tipo `pub type Result<T> = std::result::Result<T, String>;` em `main.rs`.

- [ ] **Step 1: Criar a configuração do workspace**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["xtask"]
default-members = ["xtask"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.97.1"
targets = ["x86_64-unknown-none"]
profile = "minimal"
components = ["clippy", "rustfmt"]
```

`.cargo/config.toml`:
```toml
[alias]
xtask = "run --quiet --package xtask --"

[target.x86_64-unknown-none]
rustflags = ["-C", "relocation-model=static", "-C", "code-model=kernel"]
```

`.gitignore`:
```
/target
```

`.gitattributes`:
```
* text=auto eol=lf
*.fd binary
*.EFI binary
```

- [ ] **Step 2: Criar o `xtask` mínimo**

`xtask/Cargo.toml`:
```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false
```

`xtask/src/main.rs`:
```rust
use std::process::ExitCode;

pub type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "uso: cargo xtask <build|run|ktest|test>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result: Result<()> = match args.first().map(String::as_str) {
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 3: Verificar que o workspace compila e o alias funciona**

Run: `cargo xtask`
Expected: imprime `uso: cargo xtask <build|run|ktest|test>` e sai com código 2 (verificar com `echo $?` no bash ou `$LASTEXITCODE` no PowerShell). O `rustup` instala o alvo `x86_64-unknown-none` automaticamente na primeira execução.

- [ ] **Step 4: Escrever os documentos**

`README.md`:
```markdown
# TukOS

Sistema operacional escrito do zero em Rust, com o objetivo de rodar aplicativos e drivers do Windows sem modificação — sem telemetria, sem anúncios e sem imposições ao usuário.

> TukOS não é afiliado, patrocinado ou endossado pela Microsoft. Windows é marca registrada da Microsoft Corporation. TukOS é uma reimplementação clean-room; veja [CLEAN_ROOM.md](CLEAN_ROOM.md).

**Status:** pré-alfa. Veja o [ROADMAP.md](ROADMAP.md).

## Pré-requisitos

- Rust (o `rust-toolchain.toml` seleciona a versão automaticamente)
- QEMU (`winget install --id SoftwareFreedomConservancy.QEMU -e` no Windows; `apt install qemu-system-x86 ovmf` no Debian/Ubuntu)
- git

## Comandos

| Comando | O que faz |
|---|---|
| `cargo xtask build` | compila o kernel e monta a partição EFI em `target/esp-default/` |
| `cargo xtask run` | dá boot no QEMU com a serial no terminal |
| `cargo xtask ktest` | roda os autotestes do kernel no QEMU |
| `cargo xtask test` | roda os testes unitários no host |

Variáveis opcionais: `TUKOS_QEMU` (caminho do `qemu-system-x86_64`) e `TUKOS_OVMF` (caminho do firmware OVMF).

## Licença

MIT OR Apache-2.0, à sua escolha.
```

`AGENTS.md`:
```markdown
# Guia para colaboradores e agentes de IA

## Antes de tudo

1. Leia [CLEAN_ROOM.md](CLEAN_ROOM.md). Contribuições que violem a política são revertidas.
2. Sessões e pessoas que trabalham neste repositório são **limpas**: nunca abra disassembly de binários da Microsoft, material do repositório privado `tukos-specs` que não seja spec aprovada, nem código vazado.
3. Specs de design ficam em `docs/superpowers/specs/`, planos em `docs/superpowers/plans/`.

## Fluxo

- TDD: escreva o teste, veja falhar, implemente, veja passar, faça commit.
- Lógica pura vai para crates testáveis no host (`cargo xtask test`). Comportamento que depende do hardware vira autoteste do kernel (`cargo xtask ktest`).
- Rust stable apenas. Nada de recursos nightly.
- Código dependente de CPU fica em `crates/arch-x86_64`.

## Commits

Todo commit que implementa comportamento compatível com Windows, hardware ou protocolo declara a fonte no rodapé:

    Source: docs <documento ou URL>
    Source: blackbox <teste em conformance/>
    Source: observation <arquivo em docs/observations/>
    Source: spec-L2 <id da spec aprovada>

## Contrato da serial

O `xtask` lê estes marcadores na serial (COM1):

- `@@TUKOS-KTEST-PASS <nome>` / `@@TUKOS-KTEST-FAIL <nome> <motivo>`
- `@@TUKOS-KTEST-DONE passed=<n> failed=<n>`
- `@@TUKOS-PANIC <mensagem>`
```

`CLEAN_ROOM.md`: título `# Política de clean room do TukOS`, seguido do texto **integral** da seção 2 da spec (`docs/superpowers/specs/2026-10-13-tukos-marco1-design.md`), desde `### Proibido sempre` até o fim de `### Avaliação jurídica`, com os títulos `###` rebaixados para `##`.

`ROADMAP.md`: título `# Roadmap do TukOS`, seguido da tabela "Subprojetos" da seção 1 da spec, copiada integralmente, e da linha: `**Marco atual:** Marco 1 — boot no QEMU e execução de .exe real do Windows (subprojetos 0, 1 e 3 em versão mínima).`

`LICENSE-MIT`: texto padrão da licença MIT com a linha `Copyright (c) 2026 Os autores do TukOS`.

`LICENSE-APACHE`: baixar o texto oficial:
```bash
curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE-APACHE
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: esqueleto do workspace, xtask e documentos do projeto

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `xtask` — localizar QEMU/OVMF, códigos de saída e timeout

**Files:**
- Create: `xtask/src/qemu.rs`
- Modify: `xtask/src/main.rs` (adicionar `mod qemu;`)

**Interfaces:**
- Consumes: `crate::Result` (Task 1).
- Produces (em `xtask/src/qemu.rs`):
  - `pub enum Outcome { Success, Failure, Timeout, Other(i32) }` (deriva `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn outcome_from_exit_code(code: Option<i32>) -> Outcome`
  - `pub fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf>`
  - `pub fn missing_message(what: &str, tried: &[PathBuf], hint: &str) -> String`
  - `pub fn find_qemu() -> crate::Result<PathBuf>`
  - `pub fn find_ovmf(qemu: &Path) -> crate::Result<PathBuf>`
  - `pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>>`
  - `pub fn base_args(ovmf: &Path, esp: &Path) -> Vec<String>`
  - `pub fn run_interactive(esp: &Path) -> crate::Result<()>`
  - `pub fn run_captured(esp: &Path, timeout: Duration) -> crate::Result<(Outcome, String)>`

- [ ] **Step 1: Escrever os testes que falham**

`xtask/src/qemu.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    fn sleeper(secs: u32) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", &format!("Start-Sleep -Seconds {secs}")]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg(secs.to_string());
            c
        }
    }

    #[test]
    fn exit_codes_map_to_outcomes() {
        assert_eq!(outcome_from_exit_code(Some(33)), Outcome::Success);
        assert_eq!(outcome_from_exit_code(Some(35)), Outcome::Failure);
        assert_eq!(outcome_from_exit_code(Some(0)), Outcome::Other(0));
        assert_eq!(outcome_from_exit_code(None), Outcome::Other(-1));
    }

    #[test]
    fn first_existing_picks_first_present_path() {
        let present = std::env::current_exe().unwrap();
        let missing = PathBuf::from("/definitely/not/here/qemu");
        assert_eq!(first_existing(&[missing.clone(), present.clone()]), Some(present));
        assert_eq!(first_existing(&[missing]), None);
    }

    #[test]
    fn missing_message_lists_paths_and_hint() {
        let tried = vec![PathBuf::from("/a/ovmf.fd"), PathBuf::from("/b/ovmf.fd")];
        let msg = missing_message("OVMF", &tried, "instale o pacote ovmf");
        assert!(msg.contains("OVMF"));
        assert!(msg.contains("/a/ovmf.fd"));
        assert!(msg.contains("/b/ovmf.fd"));
        assert!(msg.contains("instale o pacote ovmf"));
    }

    #[test]
    fn wait_with_timeout_kills_hung_process() {
        let mut child = sleeper(30).spawn().unwrap();
        let start = Instant::now();
        let status = wait_with_timeout(&mut child, Duration::from_secs(1)).unwrap();
        assert!(status.is_none());
        assert!(start.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn wait_with_timeout_returns_status_of_finished_process() {
        let mut child = sleeper(0).spawn().unwrap();
        let status = wait_with_timeout(&mut child, Duration::from_secs(20)).unwrap();
        assert!(status.expect("processo deveria terminar").success());
    }

    #[test]
    fn base_args_wire_firmware_disk_serial_and_exit_device() {
        let args = base_args(Path::new("/fw/OVMF.fd"), Path::new("/esp"));
        let joined = args.join(" ");
        assert!(joined.contains("if=pflash,format=raw,readonly=on,file=/fw/OVMF.fd"));
        assert!(joined.contains("format=raw,file=fat:/esp"));
        assert!(joined.contains("-serial stdio"));
        assert!(joined.contains("isa-debug-exit,iobase=0xf4,iosize=0x04"));
        assert!(joined.contains("-no-reboot"));
    }
}
```

Adicionar `mod qemu;` no topo de `xtask/src/main.rs` (abaixo do `use`).

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p xtask`
Expected: FAIL de compilação — `cannot find function outcome_from_exit_code` (e demais).

- [ ] **Step 3: Implementar**

Acima do bloco de testes em `xtask/src/qemu.rs`:
```rust
//! Locating and driving QEMU.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// How a QEMU run ended. `isa-debug-exit` turns a write of `v` into exit code `(v << 1) | 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failure,
    Timeout,
    Other(i32),
}

pub fn outcome_from_exit_code(code: Option<i32>) -> Outcome {
    match code {
        Some(33) => Outcome::Success,
        Some(35) => Outcome::Failure,
        Some(c) => Outcome::Other(c),
        None => Outcome::Other(-1),
    }
}

pub fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

pub fn missing_message(what: &str, tried: &[PathBuf], hint: &str) -> String {
    let mut msg = format!("{what} não encontrado. Caminhos testados:\n");
    for p in tried {
        msg.push_str(&format!("  - {}\n", p.display()));
    }
    msg.push_str(hint);
    msg
}

const QEMU_HINT: &str = "Instale o QEMU (Windows: `winget install --id SoftwareFreedomConservancy.QEMU -e`; \
Debian/Ubuntu: `sudo apt install qemu-system-x86`) ou defina TUKOS_QEMU.";
const OVMF_HINT: &str = "Instale o firmware OVMF (vem com o QEMU no Windows; Debian/Ubuntu: \
`sudo apt install ovmf`) ou defina TUKOS_OVMF.";

pub fn find_qemu() -> crate::Result<PathBuf> {
    if let Ok(p) = std::env::var("TUKOS_QEMU") {
        return Ok(PathBuf::from(p));
    }
    let on_path = Command::new("qemu-system-x86_64")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if on_path {
        return Ok(PathBuf::from("qemu-system-x86_64"));
    }
    let candidates = vec![PathBuf::from(r"C:\Program Files\qemu\qemu-system-x86_64.exe")];
    first_existing(&candidates).ok_or_else(|| {
        let mut tried = vec![PathBuf::from("qemu-system-x86_64 (PATH)")];
        tried.extend(candidates);
        missing_message("QEMU", &tried, QEMU_HINT)
    })
}

pub fn find_ovmf(qemu: &Path) -> crate::Result<PathBuf> {
    if let Ok(p) = std::env::var("TUKOS_OVMF") {
        return Ok(PathBuf::from(p));
    }
    let mut candidates = Vec::new();
    if let Some(dir) = qemu.parent().filter(|d| !d.as_os_str().is_empty()) {
        candidates.push(dir.join("share").join("edk2-x86_64-code.fd"));
    }
    candidates.extend(
        [
            r"C:\Program Files\qemu\share\edk2-x86_64-code.fd",
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/ovmf/OVMF.fd",
            "/usr/share/qemu/edk2-x86_64-code.fd",
        ]
        .map(PathBuf::from),
    );
    first_existing(&candidates).ok_or_else(|| missing_message("OVMF", &candidates, OVMF_HINT))
}

pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn base_args(ovmf: &Path, esp: &Path) -> Vec<String> {
    vec![
        "-machine".into(),
        "q35".into(),
        "-m".into(),
        "512M".into(),
        "-drive".into(),
        format!("if=pflash,format=raw,readonly=on,file={}", ovmf.display()),
        "-drive".into(),
        format!("format=raw,file=fat:{}", esp.display()),
        "-serial".into(),
        "stdio".into(),
        "-device".into(),
        "isa-debug-exit,iobase=0xf4,iosize=0x04".into(),
        "-no-reboot".into(),
    ]
}

pub fn run_interactive(esp: &Path) -> crate::Result<()> {
    let qemu = find_qemu()?;
    let ovmf = find_ovmf(&qemu)?;
    let status = Command::new(&qemu)
        .args(base_args(&ovmf, esp))
        .status()
        .map_err(|e| format!("falha ao iniciar {}: {e}", qemu.display()))?;
    match outcome_from_exit_code(status.code()) {
        Outcome::Failure => Err("o kernel reportou falha (veja a serial acima)".into()),
        _ => Ok(()),
    }
}

pub fn run_captured(esp: &Path, timeout: Duration) -> crate::Result<(Outcome, String)> {
    let qemu = find_qemu()?;
    let ovmf = find_ovmf(&qemu)?;
    let mut child = Command::new(&qemu)
        .args(base_args(&ovmf, esp))
        .args(["-display", "none", "-monitor", "none"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("falha ao iniciar {}: {e}", qemu.display()))?;
    let mut stdout = child.stdout.take().expect("stdout foi configurado como piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let status = wait_with_timeout(&mut child, timeout).map_err(|e| e.to_string())?;
    let serial = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    let outcome = match status {
        Some(s) => outcome_from_exit_code(s.code()),
        None => Outcome::Timeout,
    };
    Ok((outcome, serial))
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p xtask`
Expected: PASS nos 6 testes. Avisos de `dead_code` para `run_interactive`/`run_captured`/`find_*` são esperados até a Task 5.

- [ ] **Step 5: Commit**

```bash
git add xtask
git commit -m "feat(xtask): localizar QEMU/OVMF, mapear códigos de saída e timeout

Source: docs QEMU isa-debug-exit device documentation
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `xtask` — parser da serial e verificação dos autotestes

**Files:**
- Create: `xtask/src/ktest.rs`
- Modify: `xtask/src/main.rs` (adicionar `mod ktest;`)

**Interfaces:**
- Consumes: `crate::qemu::Outcome` (Task 2).
- Produces (em `xtask/src/ktest.rs`):
  - `pub struct Report { pub passed: Vec<String>, pub failed: Vec<(String, String)>, pub done: Option<(u32, u32)>, pub panic: Option<String> }` (deriva `Debug, Default, PartialEq`)
  - `pub fn parse(serial: &str) -> Report`
  - `pub fn check_ktest(outcome: Outcome, report: &Report) -> crate::Result<()>`
  - `pub fn check_expected_panic(outcome: Outcome, report: &Report, must_contain: &[&str]) -> crate::Result<()>`
  - (Task 5 acrescenta `pub fn run_all() -> crate::Result<()>`)

- [ ] **Step 1: Escrever os testes que falham**

`xtask/src/ktest.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_collects_pass_fail_done() {
        let serial = "\
@@TUKOS-KTEST-PASS gdt_selectors_loaded
@@TUKOS-KTEST-FAIL idt_loaded base errada
@@TUKOS-KTEST-DONE passed=1 failed=1
";
        let r = parse(serial);
        assert_eq!(r.passed, vec!["gdt_selectors_loaded".to_string()]);
        assert_eq!(r.failed, vec![("idt_loaded".to_string(), "base errada".to_string())]);
        assert_eq!(r.done, Some((1, 1)));
        assert_eq!(r.panic, None);
    }

    #[test]
    fn parse_ignores_noise_and_crlf() {
        let serial = "BdsDxe: loading Boot0001\r\n\x1b[2J\x1b[01;01H@@TUKOS-KTEST-PASS a\r\nlixo @@TUKOS-KTEST-DONE passed=1 failed=0\r\n";
        let r = parse(serial);
        assert_eq!(r.passed, vec!["a".to_string()]);
        assert_eq!(r.done, Some((1, 0)));
    }

    #[test]
    fn parse_captures_panic_line() {
        let r = parse("@@TUKOS-PANIC excecao 14 PAGE_FAULT cr2=0x7fff00000000\nr15: 0\n");
        assert_eq!(r.panic.as_deref(), Some("excecao 14 PAGE_FAULT cr2=0x7fff00000000"));
        assert_eq!(r.done, None);
    }

    #[test]
    fn check_ktest_accepts_clean_run() {
        let r = Report { passed: vec!["a".into()], done: Some((1, 0)), ..Default::default() };
        assert!(check_ktest(Outcome::Success, &r).is_ok());
    }

    #[test]
    fn check_ktest_rejects_failures_timeouts_and_missing_done() {
        let failed = Report { failed: vec![("a".into(), "x".into())], done: Some((0, 1)), ..Default::default() };
        assert!(check_ktest(Outcome::Failure, &failed).is_err());
        let ok_report = Report { passed: vec!["a".into()], done: Some((1, 0)), ..Default::default() };
        assert!(check_ktest(Outcome::Timeout, &ok_report).is_err());
        let no_done = Report { passed: vec!["a".into()], ..Default::default() };
        assert!(check_ktest(Outcome::Success, &no_done).is_err());
        let empty = Report { done: Some((0, 0)), ..Default::default() };
        assert!(check_ktest(Outcome::Success, &empty).is_err());
    }

    #[test]
    fn check_expected_panic_requires_failure_and_text() {
        let r = Report { panic: Some("excecao 14 PAGE_FAULT cr2=0x7fff00000000".into()), ..Default::default() };
        assert!(check_expected_panic(Outcome::Failure, &r, &["PAGE_FAULT", "cr2=0x7fff00000000"]).is_ok());
        assert!(check_expected_panic(Outcome::Failure, &r, &["GENERAL_PROTECTION"]).is_err());
        assert!(check_expected_panic(Outcome::Success, &r, &["PAGE_FAULT"]).is_err());
        assert!(check_expected_panic(Outcome::Failure, &Report::default(), &["x"]).is_err());
    }
}
```

Adicionar `mod ktest;` em `xtask/src/main.rs`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p xtask ktest`
Expected: FAIL de compilação — `cannot find function parse`.

- [ ] **Step 3: Implementar**

Acima dos testes em `xtask/src/ktest.rs`:
```rust
//! Kernel self-test protocol: parsing serial markers and judging runs.

use crate::qemu::Outcome;

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub passed: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub done: Option<(u32, u32)>,
    pub panic: Option<String>,
}

const MARK: &str = "@@TUKOS-";

pub fn parse(serial: &str) -> Report {
    let mut r = Report::default();
    for line in serial.lines() {
        let line = line.trim_end_matches('\r');
        let Some(pos) = line.find(MARK) else { continue };
        let rest = &line[pos + MARK.len()..];
        if let Some(name) = rest.strip_prefix("KTEST-PASS ") {
            r.passed.push(name.trim().to_string());
        } else if let Some(body) = rest.strip_prefix("KTEST-FAIL ") {
            let (name, reason) = body.split_once(' ').unwrap_or((body, ""));
            r.failed.push((name.to_string(), reason.trim().to_string()));
        } else if let Some(body) = rest.strip_prefix("KTEST-DONE ") {
            r.done = parse_done(body);
        } else if let Some(msg) = rest.strip_prefix("PANIC ") {
            r.panic.get_or_insert_with(|| msg.trim().to_string());
        }
    }
    r
}

fn parse_done(body: &str) -> Option<(u32, u32)> {
    let mut passed = None;
    let mut failed = None;
    for kv in body.split_whitespace() {
        match kv.split_once('=') {
            Some(("passed", v)) => passed = v.parse().ok(),
            Some(("failed", v)) => failed = v.parse().ok(),
            _ => {}
        }
    }
    Some((passed?, failed?))
}

pub fn check_ktest(outcome: Outcome, report: &Report) -> crate::Result<()> {
    if let Some(p) = &report.panic {
        return Err(format!("panic no kernel: {p}"));
    }
    if outcome == Outcome::Timeout {
        return Err("timeout: o kernel não terminou os autotestes".into());
    }
    let Some((passed, failed)) = report.done else {
        return Err(format!("marcador KTEST-DONE ausente (resultado do QEMU: {outcome:?})"));
    };
    if failed > 0 || !report.failed.is_empty() {
        let names: Vec<_> = report.failed.iter().map(|(n, why)| format!("{n} ({why})")).collect();
        return Err(format!("{failed} autoteste(s) falharam: {}", names.join(", ")));
    }
    if passed == 0 {
        return Err("nenhum autoteste foi executado".into());
    }
    if outcome != Outcome::Success {
        return Err(format!("QEMU terminou com {outcome:?} apesar de todos os testes passarem"));
    }
    Ok(())
}

pub fn check_expected_panic(outcome: Outcome, report: &Report, must_contain: &[&str]) -> crate::Result<()> {
    let Some(msg) = &report.panic else {
        return Err(format!("panic esperado não ocorreu (resultado do QEMU: {outcome:?})"));
    };
    if outcome != Outcome::Failure {
        return Err(format!("panic deveria encerrar o QEMU com falha, mas foi {outcome:?}"));
    }
    for needle in must_contain {
        if !msg.contains(needle) {
            return Err(format!("mensagem de panic não contém {needle:?}: {msg}"));
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p xtask`
Expected: PASS em todos os testes (6 de `qemu` + 6 de `ktest`).

- [ ] **Step 5: Commit**

```bash
git add xtask
git commit -m "feat(xtask): parser dos marcadores da serial e verificação dos autotestes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `arch-x86_64` — codificação de descritores (GDT, TSS, IDT, trap frame)

**Files:**
- Create: `crates/arch-x86_64/Cargo.toml`, `crates/arch-x86_64/src/lib.rs`, `crates/arch-x86_64/src/gdt.rs`, `crates/arch-x86_64/src/idt.rs`, `crates/arch-x86_64/src/trap.rs`
- Modify: `Cargo.toml` (workspace: adicionar membro e default-member)

**Interfaces:**
- Produces (lógica pura, compila no host e no alvo):
  - `gdt::{KERNEL_CS: u16 = 0x08, KERNEL_DS: u16 = 0x10, USER_DS: u16 = 0x1b, USER_CS: u16 = 0x23, TSS_SEL: u16 = 0x28}`
  - `gdt::{KERNEL_CODE, KERNEL_DATA, USER_DATA, USER_CODE}: u64`
  - `gdt::Tss` (`#[repr(C, packed(4))]`, 104 bytes) com `pub const fn new() -> Self` e `pub const fn with_ist1(top: u64) -> Self`
  - `gdt::tss_descriptor(base: u64, limit: u32) -> [u64; 2]`
  - `idt::IdtEntry` (`#[repr(C)]`, 16 bytes) com `pub const MISSING: Self`, `pub const fn new(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self`, `pub fn handler(&self) -> u64`
  - `trap::TrapFrame` (`#[repr(C)]`, 22 campos `u64`: `r15, r14, r13, r12, r11, r10, r9, r8, rbp, rdi, rsi, rdx, rcx, rbx, rax, vector, error_code, rip, cs, rflags, rsp, ss`)
  - `trap::set_handler(h: fn(&mut TrapFrame))`

- [ ] **Step 1: Criar a crate e adicioná-la ao workspace**

Em `Cargo.toml` (raiz), trocar as listas por:
```toml
members = ["xtask", "crates/arch-x86_64"]
default-members = ["xtask", "crates/arch-x86_64"]
```

`crates/arch-x86_64/Cargo.toml`:
```toml
[package]
name = "arch-x86_64"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false
```

`crates/arch-x86_64/src/lib.rs`:
```rust
//! Everything that depends on the x86-64 CPU. Pure encodings are host-testable;
//! hardware access is gated on `target_os = "none"`.
#![cfg_attr(not(test), no_std)]

pub mod gdt;
pub mod idt;
pub mod trap;
```

- [ ] **Step 2: Escrever os testes que falham**

`crates/arch-x86_64/src/gdt.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const fn dpl(desc: u64) -> u64 {
        (desc >> 45) & 0b11
    }
    const fn long_mode(desc: u64) -> bool {
        desc & (1 << 53) != 0
    }
    const fn present(desc: u64) -> bool {
        desc & (1 << 47) != 0
    }

    #[test]
    fn code_and_data_descriptors_have_expected_privilege() {
        assert!(present(KERNEL_CODE) && long_mode(KERNEL_CODE) && dpl(KERNEL_CODE) == 0);
        assert!(present(KERNEL_DATA) && dpl(KERNEL_DATA) == 0);
        assert!(present(USER_DATA) && dpl(USER_DATA) == 3);
        assert!(present(USER_CODE) && long_mode(USER_CODE) && dpl(USER_CODE) == 3);
    }

    #[test]
    fn selectors_follow_sysret_layout() {
        // SYSRET with STAR[63:48] = KERNEL_DS loads SS = base + 8 and CS = base + 16 (RPL 3).
        assert_eq!(USER_DS, (KERNEL_DS + 8) | 3);
        assert_eq!(USER_CS, (KERNEL_DS + 16) | 3);
    }

    #[test]
    fn tss_is_104_bytes() {
        assert_eq!(core::mem::size_of::<Tss>(), 104);
    }

    #[test]
    fn tss_descriptor_encodes_base_limit_and_type() {
        let [lo, hi] = tss_descriptor(0x1122_3344_5566_7788, 0x67);
        assert_eq!(lo, 0x5500_8966_7788_0067);
        assert_eq!(hi, 0x1122_3344);
    }

    #[test]
    fn with_ist1_sets_only_first_ist_slot() {
        let tss = Tss::with_ist1(0xdead_b000);
        let ist = tss.ist;
        assert_eq!(ist[0], 0xdead_b000);
        assert!(ist[1..].iter().all(|&v| v == 0));
    }
}
```

`crates/arch-x86_64/src/idt.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_is_16_bytes() {
        assert_eq!(core::mem::size_of::<IdtEntry>(), 16);
    }

    #[test]
    fn new_splits_handler_address_and_sets_gate_type() {
        let e = IdtEntry::new(0x1122_3344_5566_7788, 0x08, 1, 0);
        assert_eq!(e.offset_low, 0x7788);
        assert_eq!(e.offset_mid, 0x5566);
        assert_eq!(e.offset_high, 0x1122_3344);
        assert_eq!(e.selector, 0x08);
        assert_eq!(e.ist, 1);
        assert_eq!(e.type_attr, 0x8E);
        assert_eq!(e.handler(), 0x1122_3344_5566_7788);
    }

    #[test]
    fn user_callable_gate_has_dpl3() {
        assert_eq!(IdtEntry::new(0x1000, 0x08, 0, 3).type_attr, 0xEE);
    }

    #[test]
    fn missing_entry_is_not_present() {
        assert_eq!(IdtEntry::MISSING.type_attr & 0x80, 0);
    }
}
```

`crates/arch-x86_64/src/trap.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn trap_frame_layout_matches_assembly_stub() {
        assert_eq!(core::mem::size_of::<TrapFrame>(), 22 * 8);
        assert_eq!(offset_of!(TrapFrame, r15), 0);
        assert_eq!(offset_of!(TrapFrame, rax), 14 * 8);
        assert_eq!(offset_of!(TrapFrame, vector), 15 * 8);
        assert_eq!(offset_of!(TrapFrame, error_code), 16 * 8);
        assert_eq!(offset_of!(TrapFrame, rip), 17 * 8);
        assert_eq!(offset_of!(TrapFrame, ss), 21 * 8);
    }
}
```

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo test -p arch-x86_64`
Expected: FAIL de compilação — `cannot find value KERNEL_CODE`, `cannot find type IdtEntry`, `cannot find type TrapFrame`.

- [ ] **Step 4: Implementar**

Topo de `crates/arch-x86_64/src/gdt.rs` (acima dos testes):
```rust
//! Global Descriptor Table and Task State Segment encodings (Intel SDM Vol. 3A, 3.4.5 and 8.7).

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS: u16 = 0x18 | 3;
pub const USER_CS: u16 = 0x20 | 3;
pub const TSS_SEL: u16 = 0x28;

/// 64-bit code, DPL 0, present.
pub const KERNEL_CODE: u64 = 0x00AF_9A00_0000_FFFF;
/// Writable data, DPL 0, present.
pub const KERNEL_DATA: u64 = 0x00CF_9200_0000_FFFF;
/// Writable data, DPL 3, present.
pub const USER_DATA: u64 = 0x00CF_F200_0000_FFFF;
/// 64-bit code, DPL 3, present.
pub const USER_CODE: u64 = 0x00AF_FA00_0000_FFFF;

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct Tss {
    _reserved0: u32,
    pub rsp: [u64; 3],
    _reserved1: u64,
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    pub iomap_base: u16,
}

impl Tss {
    pub const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp: [0; 3],
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iomap_base: core::mem::size_of::<Tss>() as u16,
        }
    }

    pub const fn with_ist1(top: u64) -> Self {
        let mut tss = Self::new();
        tss.ist = [top, 0, 0, 0, 0, 0, 0];
        tss
    }
}

/// Encodes a 16-byte available 64-bit TSS descriptor (type 0x9, present).
pub const fn tss_descriptor(base: u64, limit: u32) -> [u64; 2] {
    let limit = limit as u64;
    let lo = (limit & 0xFFFF)
        | ((base & 0xFF_FFFF) << 16)
        | (0x89 << 40)
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    let hi = base >> 32;
    [lo, hi]
}
```

Topo de `crates/arch-x86_64/src/idt.rs`:
```rust
//! Interrupt Descriptor Table entry encoding (Intel SDM Vol. 3A, 6.14.1).

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub selector: u16,
    pub ist: u8,
    pub type_attr: u8,
    pub offset_mid: u16,
    pub offset_high: u32,
    _reserved: u32,
}

impl IdtEntry {
    pub const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        type_attr: 0,
        offset_mid: 0,
        offset_high: 0,
        _reserved: 0,
    };

    /// Present 64-bit interrupt gate.
    pub const fn new(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: ist & 0b111,
            type_attr: 0x8E | ((dpl & 0b11) << 5),
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            _reserved: 0,
        }
    }

    pub fn handler(&self) -> u64 {
        self.offset_low as u64 | (self.offset_mid as u64) << 16 | (self.offset_high as u64) << 32
    }
}
```

Topo de `crates/arch-x86_64/src/trap.rs`:
```rust
//! Register state saved by the exception entry stubs, and the dispatch hook.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Layout pushed by `tukos_isr_common`: GPRs (r15 at the lowest address), then the
/// vector and error code pushed by each stub, then the CPU-pushed interrupt frame.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

static HANDLER: AtomicUsize = AtomicUsize::new(0);

/// Installs the kernel function that receives every trap.
pub fn set_handler(handler: fn(&mut TrapFrame)) {
    HANDLER.store(handler as usize, Ordering::SeqCst);
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test -p arch-x86_64`
Expected: PASS nos 10 testes (5 `gdt`, 4 `idt`, 1 `trap`).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/arch-x86_64
git commit -m "feat(arch): codificação de GDT, TSS, IDT e layout do trap frame

Source: docs Intel SDM Vol. 3A, seções 3.4.5, 6.14.1 e 8.7
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Kernel que dá boot — Limine, serial, panic e autotestes

**Files:**
- Create: `crates/arch-x86_64/src/port.rs`, `crates/arch-x86_64/src/cpu.rs`
- Modify: `crates/arch-x86_64/src/lib.rs`
- Create: `kernel/Cargo.toml`, `kernel/build.rs`, `kernel/linker.ld`, `kernel/src/main.rs`, `kernel/src/console.rs`, `kernel/src/serial.rs`, `kernel/src/qemu.rs`, `kernel/src/panic.rs`, `kernel/src/ktest.rs`
- Create: `xtask/src/image.rs`, `xtask/src/util.rs`
- Modify: `xtask/src/main.rs`, `xtask/src/ktest.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `qemu::{run_interactive, run_captured, Outcome}` (Task 2), `ktest::{parse, check_ktest, check_expected_panic}` (Task 3).
- Produces:
  - `arch_x86_64::port::{inb(port: u16) -> u8, outb(port: u16, v: u8), outl(port: u16, v: u32)}` (todas `unsafe fn`)
  - `arch_x86_64::cpu::{hlt(), cli(), read_cr2() -> u64, read_cs() -> u16, read_ss() -> u16, str() -> u16, int3(), sidt() -> DescriptorTablePointer, unsafe lgdt(&DescriptorTablePointer), unsafe lidt(&DescriptorTablePointer)}` e `DescriptorTablePointer { limit: u16, base: u64 }` (`#[repr(C, packed(2))]`)
  - Kernel: macros `kprint!`/`kprintln!`; `serial::SERIAL: spin::Mutex<Serial>`; `qemu::exit(qemu::ExitCode) -> !`; `ktest::TESTS: &[(&str, TestFn)]` onde `type TestFn = fn() -> Result<(), &'static str>`
  - Kernel features: `ktest`, `ktest-panic`, `ktest-fault`
  - `xtask`: `image::build(features: &[&str]) -> Result<PathBuf>` (retorna o diretório da ESP), `ktest::run_all() -> Result<()>`, `util::{workspace_root() -> PathBuf, run(cmd: &mut Command) -> Result<()>}`

- [ ] **Step 1: Instalar o QEMU na máquina de desenvolvimento**

Run (PowerShell): `winget install --id SoftwareFreedomConservancy.QEMU -e`
Expected: instalado em `C:\Program Files\qemu\`. Conferir: `& "C:\Program Files\qemu\qemu-system-x86_64.exe" --version` imprime a versão, e o arquivo `C:\Program Files\qemu\share\edk2-x86_64-code.fd` existe.

- [ ] **Step 2: Fixar a versão do Limine**

Run: `git ls-remote --heads https://codeberg.org/Limine/Limine.git "v*-binary"`
Expected: lista de branches `refs/heads/vN.x-binary`. Escolher a de **maior N que seja ≥ 9** (a crate `limine` 0.5 usa a revisão base 3, suportada a partir do Limine 8). Anotar o nome da branch e o hash do commit dela; os dois vão para as constantes `LIMINE_BRANCH` e `LIMINE_COMMIT` do Step 6. Se o host Codeberg estiver indisponível, usar o espelho `https://github.com/limine-bootloader/limine.git` no lugar, inclusive na constante `LIMINE_URL`.

- [ ] **Step 3: Primitivas de porta e CPU na crate `arch-x86_64`**

`crates/arch-x86_64/src/port.rs`:
```rust
//! Port-mapped I/O.

use core::arch::asm;

/// # Safety
/// Reads a hardware port; the caller must know the device semantics.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

/// # Safety
/// Writes a hardware port; the caller must know the device semantics.
#[inline]
pub unsafe fn outb(port: u16, v: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}

/// # Safety
/// Writes a hardware port; the caller must know the device semantics.
#[inline]
pub unsafe fn outl(port: u16, v: u32) {
    unsafe { asm!("out dx, eax", in("dx") port, in("eax") v, options(nomem, nostack, preserves_flags)) };
}
```

`crates/arch-x86_64/src/cpu.rs`:
```rust
//! Thin wrappers over privileged CPU instructions.

use core::arch::asm;

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

#[inline]
pub fn hlt() {
    unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
}

#[inline]
pub fn cli() {
    unsafe { asm!("cli", options(nomem, nostack)) };
}

#[inline]
pub fn int3() {
    unsafe { asm!("int3", options(nomem, nostack)) };
}

#[inline]
pub fn read_cr2() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn read_cs() -> u16 {
    let v: u16;
    unsafe { asm!("mov {0:x}, cs", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn read_ss() -> u16 {
    let v: u16;
    unsafe { asm!("mov {0:x}, ss", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// Current task register selector.
#[inline]
pub fn str() -> u16 {
    let v: u16;
    unsafe { asm!("str {0:x}", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn sidt() -> DescriptorTablePointer {
    let mut p = DescriptorTablePointer { limit: 0, base: 0 };
    unsafe { asm!("sidt [{}]", in(reg) &mut p, options(nostack, preserves_flags)) };
    p
}

/// # Safety
/// `p` must describe a valid GDT that outlives its use by the CPU.
#[inline]
pub unsafe fn lgdt(p: &DescriptorTablePointer) {
    unsafe { asm!("lgdt [{}]", in(reg) p, options(readonly, nostack, preserves_flags)) };
}

/// # Safety
/// `p` must describe a valid IDT that outlives its use by the CPU.
#[inline]
pub unsafe fn lidt(p: &DescriptorTablePointer) {
    unsafe { asm!("lidt [{}]", in(reg) p, options(readonly, nostack, preserves_flags)) };
}
```

Em `crates/arch-x86_64/src/lib.rs`, adicionar:
```rust
pub mod cpu;
pub mod port;
```

Run: `cargo test -p arch-x86_64`
Expected: PASS (os 10 testes anteriores; os novos módulos só precisam compilar).

- [ ] **Step 4: Criar a crate do kernel**

Em `Cargo.toml` (raiz), `members = ["xtask", "crates/arch-x86_64", "kernel"]` (o kernel **não** entra em `default-members`).

`kernel/Cargo.toml`:
```toml
[package]
name = "tukos-kernel"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[[bin]]
name = "tukos-kernel"
path = "src/main.rs"
test = false
bench = false

[dependencies]
arch-x86_64 = { path = "../crates/arch-x86_64" }
limine = "0.5"
spin = "0.10"

[features]
ktest = []
ktest-panic = []
ktest-fault = []
```

`kernel/build.rs`:
```rust
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=-T{dir}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");
}
```

`kernel/linker.ld`:
```
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(kmain)

PHDRS
{
    text    PT_LOAD;
    rodata  PT_LOAD;
    data    PT_LOAD;
}

SECTIONS
{
    . = 0xffffffff80000000;

    .text : { *(.text .text.*) } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));
    .rodata : { *(.rodata .rodata.*) } :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));
    .data : {
        *(.data .data.*)
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :data

    .bss : { *(.bss .bss.*) *(COMMON) } :data

    /DISCARD/ : { *(.eh_frame*) *(.note .note.*) }
}
```

`kernel/src/serial.rs`:
```rust
//! 16550 UART on COM1, used for diagnostics and the test protocol.

use arch_x86_64::port::{inb, outb};
use core::fmt;
use spin::Mutex;

const COM1: u16 = 0x3F8;

pub struct Serial;

impl Serial {
    fn write_byte(&mut self, b: u8) {
        unsafe {
            while inb(COM1 + 5) & 0x20 == 0 {}
            outb(COM1, b);
        }
    }
}

impl fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        s.bytes().for_each(|b| self.write_byte(b));
        Ok(())
    }
}

pub static SERIAL: Mutex<Serial> = Mutex::new(Serial);

/// 115200 baud, 8N1, FIFO enabled.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1, 0x01);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0xC7);
        outb(COM1 + 4, 0x0B);
    }
}
```

`kernel/src/console.rs`:
```rust
//! Kernel log output, fanned out to every available sink.

use core::fmt::{self, Write};

pub fn _print(args: fmt::Arguments) {
    let _ = crate::serial::SERIAL.lock().write_fmt(args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => { $crate::console::_print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => { $crate::kprint!("{}\n", format_args!($($arg)*)) };
}
```

`kernel/src/qemu.rs`:
```rust
//! QEMU `isa-debug-exit` device (iobase 0xf4). On real hardware the write is ignored.

#[repr(u32)]
pub enum ExitCode {
    Success = 0x10,
    Failure = 0x11,
}

pub fn exit(code: ExitCode) -> ! {
    unsafe { arch_x86_64::port::outl(0xf4, code as u32) };
    loop {
        arch_x86_64::cpu::hlt();
    }
}
```

`kernel/src/panic.rs`:
```rust
use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch_x86_64::cpu::cli();
    // The panic may have interrupted a holder of the lock; nothing else runs now.
    unsafe { crate::serial::SERIAL.force_unlock() };
    let mut serial = crate::serial::SERIAL.lock();
    let _ = write!(serial, "@@TUKOS-PANIC {}", info.message());
    if let Some(loc) = info.location() {
        let _ = write!(serial, " @ {}:{}", loc.file(), loc.line());
    }
    let _ = writeln!(serial);
    drop(serial);
    crate::qemu::exit(crate::qemu::ExitCode::Failure)
}
```

`kernel/src/ktest.rs`:
```rust
//! In-kernel self-tests, built only with the `ktest` feature.

type TestFn = fn() -> Result<(), &'static str>;

const TESTS: &[(&str, TestFn)] = &[("limine_base_revision_supported", limine_base_revision_supported)];

fn limine_base_revision_supported() -> Result<(), &'static str> {
    if crate::BASE_REVISION.is_supported() { Ok(()) } else { Err("revisao base nao suportada") }
}

pub fn run_all() -> ! {
    let (mut passed, mut failed) = (0u32, 0u32);
    for (name, test) in TESTS {
        match test() {
            Ok(()) => {
                kprintln!("@@TUKOS-KTEST-PASS {name}");
                passed += 1;
            }
            Err(why) => {
                kprintln!("@@TUKOS-KTEST-FAIL {name} {why}");
                failed += 1;
            }
        }
    }
    kprintln!("@@TUKOS-KTEST-DONE passed={passed} failed={failed}");
    crate::qemu::exit(if failed == 0 { crate::qemu::ExitCode::Success } else { crate::qemu::ExitCode::Failure })
}
```

`kernel/src/main.rs`:
```rust
#![no_std]
#![no_main]

#[macro_use]
mod console;
#[cfg(feature = "ktest")]
mod ktest;
mod panic;
mod qemu;
mod serial;

use limine::BaseRevision;
use limine::request::{RequestsEndMarker, RequestsStartMarker};

#[used]
#[unsafe(link_section = ".requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    serial::init();
    kprintln!("TukOS {} iniciando", env!("CARGO_PKG_VERSION"));
    assert!(BASE_REVISION.is_supported(), "revisao base do Limine nao suportada");

    kprintln!("TukOS pronto");

    #[cfg(feature = "ktest")]
    ktest::run_all();

    #[cfg(feature = "ktest-panic")]
    panic!("ktest-panic deliberado");

    #[allow(unreachable_code)]
    loop {
        arch_x86_64::cpu::hlt();
    }
}
```

Se a API da crate `limine` 0.5 divergir dos nomes acima (`BaseRevision`, `request::RequestsStartMarker`, `RequestsEndMarker`, `is_supported`), seguir `docs.rs/limine/0.5`; o comportamento exigido não muda.

Run: `cargo build -p tukos-kernel --target x86_64-unknown-none`
Expected: compila sem erros; existe `target/x86_64-unknown-none/debug/tukos-kernel`.

- [ ] **Step 5: Escrever o teste que falha para o `ktest` do `xtask`**

Em `xtask/src/main.rs`, adicionar `mod image;` e `mod util;`, e substituir o `match` por:
```rust
    let result: Result<()> = match args.first().map(String::as_str) {
        Some("build") => image::build(&[]).map(|esp| println!("ESP pronta em {}", esp.display())),
        Some("run") => image::build(&[]).and_then(|esp| qemu::run_interactive(&esp)),
        Some("ktest") => ktest::run_all(),
        Some("test") => util::run(std::process::Command::new("cargo").args(["test", "-p", "xtask", "-p", "arch-x86_64"])),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
```

Run: `cargo xtask ktest`
Expected: FAIL de compilação — módulos `image` e `util` e a função `ktest::run_all` não existem.

- [ ] **Step 6: Implementar `util`, `image` e `ktest::run_all`**

`xtask/src/util.rs`:
```rust
use std::path::PathBuf;
use std::process::Command;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn run(cmd: &mut Command) -> crate::Result<()> {
    let status = cmd.status().map_err(|e| format!("falha ao executar {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} terminou com {status}")) }
}
```

`xtask/src/image.rs` (substituir `LIMINE_BRANCH` e `LIMINE_COMMIT` pelos valores anotados no Step 2):
```rust
//! Builds the kernel and assembles an EFI System Partition directory for QEMU's vvfat.

use crate::util::{run, workspace_root};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LIMINE_URL: &str = "https://codeberg.org/Limine/Limine.git";
const LIMINE_BRANCH: &str = "v9.x-binary";
const LIMINE_COMMIT: &str = "COLE-AQUI-O-HASH-DO-STEP-2";

const LIMINE_CONF: &str = "\
timeout: 0
serial: yes

/TukOS
    protocol: limine
    path: boot():/boot/tukos-kernel
";

pub fn build(features: &[&str]) -> crate::Result<PathBuf> {
    let root = workspace_root();
    let kernel = build_kernel(&root, features)?;
    let bootx64 = ensure_limine(&root)?;
    let variant = if features.is_empty() { "default".to_string() } else { features.join("+") };
    let esp = root.join("target").join(format!("esp-{variant}"));
    assemble_esp(&esp, &kernel, &bootx64)?;
    Ok(esp)
}

fn build_kernel(root: &Path, features: &[&str]) -> crate::Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).args(["build", "-p", "tukos-kernel", "--target", "x86_64-unknown-none"]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    run(&mut cmd)?;
    Ok(root.join("target/x86_64-unknown-none/debug/tukos-kernel"))
}

fn ensure_limine(root: &Path) -> crate::Result<PathBuf> {
    let dir = root.join("target").join("limine").join(LIMINE_COMMIT);
    let efi = dir.join("BOOTX64.EFI");
    if efi.is_file() {
        return Ok(efi);
    }
    let _ = fs::remove_dir_all(&dir);
    run(Command::new("git").args(["clone", "--branch", LIMINE_BRANCH, "--single-branch", "--depth", "50", LIMINE_URL]).arg(&dir))?;
    run(Command::new("git").arg("-C").arg(&dir).args(["checkout", "--quiet", LIMINE_COMMIT])).map_err(|e| {
        format!("commit {LIMINE_COMMIT} não encontrado em {LIMINE_BRANCH}; atualize LIMINE_COMMIT ({e})")
    })?;
    if efi.is_file() { Ok(efi) } else { Err(format!("{} não existe após o clone do Limine", efi.display())) }
}

fn assemble_esp(esp: &Path, kernel: &Path, bootx64: &Path) -> crate::Result<()> {
    let io = |e: std::io::Error| format!("erro montando a ESP em {}: {e}", esp.display());
    let _ = fs::remove_dir_all(esp);
    fs::create_dir_all(esp.join("EFI/BOOT")).map_err(io)?;
    fs::create_dir_all(esp.join("boot")).map_err(io)?;
    fs::copy(bootx64, esp.join("EFI/BOOT/BOOTX64.EFI")).map_err(io)?;
    fs::copy(kernel, esp.join("boot/tukos-kernel")).map_err(io)?;
    fs::write(esp.join("limine.conf"), LIMINE_CONF).map_err(io)?;
    Ok(())
}
```

Adicionar ao fim de `xtask/src/ktest.rs`, acima de `#[cfg(test)]`:
```rust
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

/// Boots each self-test kernel variant and checks its serial output.
pub fn run_all() -> crate::Result<()> {
    run_variant("ktest", |o, r| check_ktest(o, r))?;
    run_variant("ktest-panic", |o, r| check_expected_panic(o, r, &["ktest-panic deliberado"]))?;
    println!("ktest: todas as variantes passaram");
    Ok(())
}

fn run_variant(feature: &str, check: impl Fn(Outcome, &Report) -> crate::Result<()>) -> crate::Result<()> {
    let esp = crate::image::build(&[feature])?;
    let (outcome, serial) = crate::qemu::run_captured(&esp, TIMEOUT)?;
    let report = parse(&serial);
    check(outcome, &report).map_err(|e| format!("variante {feature}: {e}\n--- serial ---\n{serial}"))?;
    println!("ktest: variante {feature} ok ({} teste(s))", report.passed.len());
    Ok(())
}
```

- [ ] **Step 7: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: `ktest: variante ktest ok (1 teste(s))`, `ktest: variante ktest-panic ok (0 teste(s))`, `ktest: todas as variantes passaram`, código de saída 0.

Run: `cargo xtask run`
Expected: janela do QEMU abre; o terminal mostra (entre a saída do OVMF/Limine) `TukOS 0.1.0 iniciando` e `TukOS pronto`. Fechar a janela do QEMU para encerrar.

Run: `cargo xtask test`
Expected: PASS em todos os testes do host.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(kernel): boot via Limine com serial, panic e autotestes no QEMU

Source: docs Limine boot protocol; 16550 UART datasheet; QEMU isa-debug-exit
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: GDT, TSS, IDT e tratamento de exceções

**Files:**
- Modify: `crates/arch-x86_64/src/gdt.rs`, `crates/arch-x86_64/src/idt.rs`, `crates/arch-x86_64/src/trap.rs`
- Create: `kernel/src/trap.rs`
- Modify: `kernel/src/main.rs`, `kernel/src/ktest.rs`, `xtask/src/ktest.rs`

**Interfaces:**
- Consumes: encodings da Task 4; `cpu::{lgdt, lidt, sidt, read_cs, read_ss, str, int3, read_cr2}` (Task 5).
- Produces:
  - `arch_x86_64::gdt::init()` (`unsafe fn`, só em `target_os = "none"`): carrega GDT com TSS (IST1 = pilha de double fault de 16 KiB), recarrega CS/DS/ES/SS/FS/GS e executa `ltr`.
  - `arch_x86_64::idt::init()` (`unsafe fn`, só em `target_os = "none"`): instala os vetores 0–31 (vetor 8 usa IST1) e executa `lidt`.
  - `arch_x86_64::idt::table_address() -> u64` (só em `target_os = "none"`)
  - Símbolo de montagem `tukos_trap_entry` chamado pelos stubs; chama o handler de `trap::set_handler`.
  - Kernel: `trap::handle(&mut TrapFrame)`, `trap::BREAKPOINTS: AtomicU64`, `trap::exception_name(vector: u64) -> &'static str`.

- [ ] **Step 1: Escrever os autotestes do kernel que falham**

Em `kernel/src/ktest.rs`, substituir a constante `TESTS` e adicionar as funções:
```rust
const TESTS: &[(&str, TestFn)] = &[
    ("limine_base_revision_supported", limine_base_revision_supported),
    ("gdt_selectors_loaded", gdt_selectors_loaded),
    ("tss_loaded", tss_loaded),
    ("idt_loaded", idt_loaded),
    ("breakpoint_returns", breakpoint_returns),
];

fn gdt_selectors_loaded() -> Result<(), &'static str> {
    use arch_x86_64::{cpu, gdt};
    if cpu::read_cs() != gdt::KERNEL_CS {
        return Err("CS diferente de KERNEL_CS");
    }
    if cpu::read_ss() != gdt::KERNEL_DS {
        return Err("SS diferente de KERNEL_DS");
    }
    Ok(())
}

fn tss_loaded() -> Result<(), &'static str> {
    if arch_x86_64::cpu::str() == arch_x86_64::gdt::TSS_SEL { Ok(()) } else { Err("TR diferente de TSS_SEL") }
}

fn idt_loaded() -> Result<(), &'static str> {
    let p = arch_x86_64::cpu::sidt();
    let (limit, base) = (p.limit, p.base);
    if limit != 256 * 16 - 1 {
        return Err("limite da IDT incorreto");
    }
    if base != arch_x86_64::idt::table_address() {
        return Err("base da IDT nao aponta para a tabela do kernel");
    }
    Ok(())
}

fn breakpoint_returns() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    let before = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    arch_x86_64::cpu::int3();
    let after = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    if after == before + 1 { Ok(()) } else { Err("handler de breakpoint nao contou a excecao") }
}
```

Em `xtask/src/ktest.rs`, em `run_all`, acrescentar após a variante `ktest-panic`:
```rust
    run_variant("ktest-fault", |o, r| check_expected_panic(o, r, &["PAGE_FAULT", "cr2=0x7fff00000000"]))?;
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest`
Expected: FAIL de compilação do kernel — `crate::trap` não existe e `arch_x86_64::idt::table_address` não existe.

- [ ] **Step 3: Implementar a parte de hardware na crate `arch-x86_64`**

Acrescentar em `crates/arch-x86_64/src/gdt.rs` (acima dos testes):
```rust
#[cfg(target_os = "none")]
mod hw {
    use super::*;
    use crate::cpu::{self, DescriptorTablePointer};
    use core::arch::asm;
    use core::mem::size_of;

    const DF_STACK_SIZE: usize = 16 * 1024;

    #[repr(C, align(16))]
    struct Stack([u8; DF_STACK_SIZE]);

    static mut DF_STACK: Stack = Stack([0; DF_STACK_SIZE]);
    static mut TSS: Tss = Tss::new();
    static mut GDT: [u64; 7] = [0; 7];

    /// Loads the kernel GDT and TSS and reloads every segment register.
    ///
    /// # Safety
    /// Must run once, on the boot CPU, before interrupts are enabled.
    pub unsafe fn init() {
        unsafe {
            let df_top = (&raw const DF_STACK) as u64 + DF_STACK_SIZE as u64;
            let tss = &raw mut TSS;
            tss.write(Tss::with_ist1(df_top));

            let [tss_lo, tss_hi] = tss_descriptor(tss as u64, (size_of::<Tss>() - 1) as u32);
            let gdt = &raw mut GDT;
            gdt.write([0, KERNEL_CODE, KERNEL_DATA, USER_DATA, USER_CODE, tss_lo, tss_hi]);

            let ptr = DescriptorTablePointer { limit: (size_of::<[u64; 7]>() - 1) as u16, base: gdt as u64 };
            cpu::lgdt(&ptr);

            // Far return to reload CS.
            asm!(
                "push {sel}",
                "lea {tmp}, [rip + 2f]",
                "push {tmp}",
                "retfq",
                "2:",
                sel = in(reg) KERNEL_CS as u64,
                tmp = lateout(reg) _,
            );
            asm!(
                "mov ds, {0:x}",
                "mov es, {0:x}",
                "mov ss, {0:x}",
                "mov fs, {0:x}",
                "mov gs, {0:x}",
                in(reg) KERNEL_DS,
                options(nostack, preserves_flags),
            );
            asm!("ltr {0:x}", in(reg) TSS_SEL, options(nostack, preserves_flags));
        }
    }
}

#[cfg(target_os = "none")]
pub use hw::init;
```

Acrescentar em `crates/arch-x86_64/src/trap.rs` (acima dos testes):
```rust
#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
.section .text
.macro TUKOS_ISR_NOERR n
.global tukos_isr_\n
tukos_isr_\n:
    push 0
    push \n
    jmp tukos_isr_common
.endm

.macro TUKOS_ISR_ERR n
.global tukos_isr_\n
tukos_isr_\n:
    push \n
    jmp tukos_isr_common
.endm

.irp n, 0,1,2,3,4,5,6,7,9,15,16,18,19,20,22,23,24,25,26,27,28,31
    TUKOS_ISR_NOERR \n
.endr
.irp n, 8,10,11,12,13,14,17,21,29,30
    TUKOS_ISR_ERR \n
.endr

tukos_isr_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    cld
    call tukos_trap_entry
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

.section .rodata
.balign 8
.global tukos_isr_table
tukos_isr_table:
.irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    .quad tukos_isr_\n
.endr
.section .text
"#
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Entry stub addresses for vectors 0..32, defined in the assembly above.
    pub static tukos_isr_table: [u64; 32];
}

/// Called by `tukos_isr_common` with a pointer to the saved frame.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
extern "C" fn tukos_trap_entry(frame: &mut TrapFrame) {
    let raw = HANDLER.load(Ordering::SeqCst);
    if raw == 0 {
        loop {
            crate::cpu::hlt();
        }
    }
    let handler: fn(&mut TrapFrame) = unsafe { core::mem::transmute(raw) };
    handler(frame);
}
```

Acrescentar em `crates/arch-x86_64/src/idt.rs` (acima dos testes):
```rust
#[cfg(target_os = "none")]
mod hw {
    use super::IdtEntry;
    use crate::cpu::{self, DescriptorTablePointer};
    use crate::gdt::KERNEL_CS;

    const DOUBLE_FAULT: usize = 8;

    static mut IDT: [IdtEntry; 256] = [IdtEntry::MISSING; 256];

    pub fn table_address() -> u64 {
        (&raw const IDT) as u64
    }

    /// Installs the exception vectors 0..32 and loads the IDT.
    ///
    /// # Safety
    /// Must run once, after `gdt::init`, before interrupts are enabled.
    pub unsafe fn init() {
        unsafe {
            let idt = &raw mut IDT;
            for (vector, &stub) in crate::trap::tukos_isr_table.iter().enumerate() {
                let ist = if vector == DOUBLE_FAULT { 1 } else { 0 };
                (*idt)[vector] = IdtEntry::new(stub, KERNEL_CS, ist, 0);
            }
            let ptr = DescriptorTablePointer { limit: (256 * 16 - 1) as u16, base: idt as u64 };
            cpu::lidt(&ptr);
        }
    }
}

#[cfg(target_os = "none")]
pub use hw::{init, table_address};
```

Run: `cargo test -p arch-x86_64`
Expected: PASS (os módulos `hw` não compilam no host, os 10 testes continuam passando).

- [ ] **Step 4: Implementar o handler de traps do kernel e ligar tudo no `kmain`**

`kernel/src/trap.rs`:
```rust
//! Kernel trap handling. Only exceptions exist at this stage.

use arch_x86_64::trap::TrapFrame;
use core::sync::atomic::{AtomicU64, Ordering};

pub static BREAKPOINTS: AtomicU64 = AtomicU64::new(0);

const BREAKPOINT: u64 = 3;

const NAMES: [&str; 32] = [
    "DIVIDE_ERROR", "DEBUG", "NMI", "BREAKPOINT", "OVERFLOW", "BOUND_RANGE", "INVALID_OPCODE",
    "DEVICE_NOT_AVAILABLE", "DOUBLE_FAULT", "COPROCESSOR_SEGMENT_OVERRUN", "INVALID_TSS",
    "SEGMENT_NOT_PRESENT", "STACK_FAULT", "GENERAL_PROTECTION", "PAGE_FAULT", "RESERVED_15",
    "X87_FLOATING_POINT", "ALIGNMENT_CHECK", "MACHINE_CHECK", "SIMD_FLOATING_POINT",
    "VIRTUALIZATION", "CONTROL_PROTECTION", "RESERVED_22", "RESERVED_23", "RESERVED_24",
    "RESERVED_25", "RESERVED_26", "RESERVED_27", "HYPERVISOR_INJECTION", "VMM_COMMUNICATION",
    "SECURITY", "RESERVED_31",
];

pub fn exception_name(vector: u64) -> &'static str {
    NAMES.get(vector as usize).copied().unwrap_or("UNKNOWN")
}

pub fn handle(frame: &mut TrapFrame) {
    if frame.vector == BREAKPOINT {
        BREAKPOINTS.fetch_add(1, Ordering::SeqCst);
        return;
    }
    let cr2 = arch_x86_64::cpu::read_cr2();
    panic!(
        "excecao {} {} err=0x{:x} rip=0x{:x} cr2=0x{:x}\n{:#x?}",
        frame.vector,
        exception_name(frame.vector),
        frame.error_code,
        frame.rip,
        cr2,
        frame
    );
}
```

Em `kernel/src/main.rs`:
- adicionar `mod trap;` junto aos demais módulos;
- substituir a linha em branco entre o `assert!` e `kprintln!("TukOS pronto");` por:
```rust
    unsafe {
        arch_x86_64::gdt::init();
        arch_x86_64::idt::init();
    }
    arch_x86_64::trap::set_handler(trap::handle);
```
- acrescentar após o bloco `ktest-panic`:
```rust
    #[cfg(feature = "ktest-fault")]
    unsafe {
        core::ptr::read_volatile(0x0000_7fff_0000_0000 as *const u64);
    }
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: `variante ktest ok (5 teste(s))`, `variante ktest-panic ok`, `variante ktest-fault ok`, `todas as variantes passaram`.

Run: `cargo xtask test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(kernel): GDT, TSS, IDT e tratamento de exceções com diagnóstico

Source: docs Intel SDM Vol. 3A, capítulos 3, 6 e 8
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Console de texto no framebuffer

**Files:**
- Create: `kernel/src/fbcon.rs`
- Modify: `kernel/Cargo.toml`, `kernel/src/main.rs`, `kernel/src/console.rs`, `kernel/src/ktest.rs`

**Interfaces:**
- Consumes: `FramebufferRequest` do Limine; `kprint!` (Task 5).
- Produces: `fbcon::init_from_limine()`, `fbcon::write_fmt(args: fmt::Arguments)`, `fbcon::FBCON: spin::Mutex<Option<FbCon>>`, `FbCon::{clear(&mut self), write_str(&mut self, &str), pixel(&self, x: usize, y: usize) -> u32, cell_width() -> usize, cell_height() -> usize}`.

- [ ] **Step 1: Escrever o autoteste que falha**

Em `kernel/src/ktest.rs`, acrescentar `("fbcon_draws_glyph", fbcon_draws_glyph),` ao fim de `TESTS` e a função:
```rust
fn fbcon_draws_glyph() -> Result<(), &'static str> {
    let mut guard = crate::fbcon::FBCON.lock();
    let con = guard.as_mut().ok_or("sem framebuffer")?;
    let (w, h) = (crate::fbcon::FbCon::cell_width(), crate::fbcon::FbCon::cell_height());
    let cell_lit = |con: &crate::fbcon::FbCon| (0..h).any(|y| (0..w).any(|x| con.pixel(x, y) != 0));

    con.clear();
    if cell_lit(con) {
        return Err("celula deveria estar apagada apos clear");
    }
    con.write_str("#");
    if !cell_lit(con) {
        return Err("glifo nao foi desenhado");
    }
    con.clear();
    Ok(())
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest`
Expected: FAIL de compilação — `crate::fbcon` não existe.

- [ ] **Step 3: Implementar**

Em `kernel/Cargo.toml`, em `[dependencies]`:
```toml
noto-sans-mono-bitmap = { version = "0.3", default-features = false, features = ["regular", "size_16", "unicode-basic-latin", "unicode-specials"] }
```

`kernel/src/fbcon.rs`:
```rust
//! Text console drawn on the Limine framebuffer (32 bpp only).

use core::fmt;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};
use spin::Mutex;

const WEIGHT: FontWeight = FontWeight::Regular;
const HEIGHT: RasterHeight = RasterHeight::Size16;

pub struct FbCon {
    base: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    col: usize,
    row: usize,
}

// The framebuffer is only touched through the mutex below.
unsafe impl Send for FbCon {}

pub static FBCON: Mutex<Option<FbCon>> = Mutex::new(None);

impl FbCon {
    pub fn cell_width() -> usize {
        get_raster_width(WEIGHT, HEIGHT)
    }

    pub fn cell_height() -> usize {
        HEIGHT.val()
    }

    fn cols(&self) -> usize {
        self.width / Self::cell_width()
    }

    fn rows(&self) -> usize {
        self.height / Self::cell_height()
    }

    pub fn pixel(&self, x: usize, y: usize) -> u32 {
        unsafe { (self.base.add(y * self.pitch + x * 4) as *const u32).read_volatile() }
    }

    fn put_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let rgb = intensity as u32 * 0x0001_0101;
        unsafe { (self.base.add(y * self.pitch + x * 4) as *mut u32).write_volatile(rgb) };
    }

    pub fn clear(&mut self) {
        unsafe { core::ptr::write_bytes(self.base, 0, self.pitch * self.height) };
        self.col = 0;
        self.row = 0;
    }

    fn draw_char(&mut self, c: char) {
        let glyph = get_raster(c, WEIGHT, HEIGHT)
            .or_else(|| get_raster('?', WEIGHT, HEIGHT))
            .expect("'?' existe na fonte");
        let (x0, y0) = (self.col * Self::cell_width(), self.row * Self::cell_height());
        for (y, line) in glyph.raster().iter().enumerate() {
            for (x, &v) in line.iter().enumerate() {
                self.put_pixel(x0 + x, y0 + y, v);
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows() {
            self.scroll();
            self.row = self.rows() - 1;
        }
    }

    fn scroll(&mut self) {
        let line = self.pitch * Self::cell_height();
        let used = line * self.rows();
        unsafe {
            core::ptr::copy(self.base.add(line), self.base, used - line);
            core::ptr::write_bytes(self.base.add(used - line), 0, line);
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => self.newline(),
                '\r' => self.col = 0,
                c => {
                    if self.col >= self.cols() {
                        self.newline();
                    }
                    self.draw_char(c);
                    self.col += 1;
                }
            }
        }
    }
}

impl fmt::Write for FbCon {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        FbCon::write_str(self, s);
        Ok(())
    }
}

pub fn init_from_limine() {
    let Some(resp) = crate::FRAMEBUFFER_REQUEST.get_response() else { return };
    let Some(fb) = resp.framebuffers().next() else { return };
    if fb.bpp() != 32 {
        return;
    }
    let mut con = FbCon {
        base: fb.addr(),
        width: fb.width() as usize,
        height: fb.height() as usize,
        pitch: fb.pitch() as usize,
        col: 0,
        row: 0,
    };
    con.clear();
    *FBCON.lock() = Some(con);
}

pub fn write_fmt(args: fmt::Arguments) {
    if let Some(con) = FBCON.lock().as_mut() {
        let _ = fmt::Write::write_fmt(con, args);
    }
}
```

Em `kernel/src/console.rs`, dentro de `_print`, acrescentar após a linha da serial:
```rust
    crate::fbcon::write_fmt(args);
```

Em `kernel/src/main.rs`:
- adicionar `mod fbcon;` junto aos demais módulos;
- trocar `use limine::request::{RequestsEndMarker, RequestsStartMarker};` por `use limine::request::{FramebufferRequest, RequestsEndMarker, RequestsStartMarker};`;
- acrescentar após `BASE_REVISION`:
```rust
#[used]
#[unsafe(link_section = ".requests")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();
```
- chamar `fbcon::init_from_limine();` logo após `serial::init();` (antes do primeiro `kprintln!`).

Se a crate `noto-sans-mono-bitmap` 0.3 ou `limine` 0.5 divergir nos nomes usados (`get_raster_width`, `RasterHeight::val`, `raster()`, `framebuffers()`, `addr()`, `bpp()`, `pitch()`), seguir a documentação da versão em docs.rs; o comportamento exigido não muda.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: `variante ktest ok (6 teste(s))` e as variantes de panic/fault ok.

Run: `cargo xtask run`
Expected: a janela do QEMU mostra `TukOS 0.1.0 iniciando` e `TukOS pronto` em texto claro sobre fundo preto.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(kernel): console de texto no framebuffer

Source: docs Limine boot protocol (framebuffer feature)
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: CI no GitHub Actions

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `cargo xtask test`, `cargo xtask ktest`.

- [ ] **Step 1: Criar o workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

jobs:
  linux:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - name: Dependências do sistema
        run: sudo apt-get update && sudo apt-get install -y qemu-system-x86 ovmf
      - name: Toolchain
        run: rustup show
      - name: Testes do host
        run: cargo xtask test
      - name: Lint
        run: cargo clippy -p xtask -p arch-x86_64 -- -D warnings
      - name: Autotestes do kernel no QEMU
        run: cargo xtask ktest

  windows:
    runs-on: windows-2025
    steps:
      - uses: actions/checkout@v4
      - name: Toolchain
        run: rustup show
      - name: Testes do host
        run: cargo xtask test
```

- [ ] **Step 2: Validar localmente o que a CI vai rodar**

Run: `cargo clippy -p xtask -p arch-x86_64 -- -D warnings`
Expected: sem avisos. Corrigir qualquer aviso antes de seguir.

Run: `cargo xtask test` e `cargo xtask ktest`
Expected: ambos passam.

- [ ] **Step 3: Commit**

```bash
git add .github
git commit -m "ci: testes do host e autotestes do kernel no QEMU

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

A primeira execução real da CI acontece quando o repositório for publicado no GitHub; publicar o repositório é uma decisão do mantenedor e não faz parte deste plano.
