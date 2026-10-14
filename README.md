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
| `cargo xtask vmware` | dá boot no VMware Workstation (janela visível) e mostra a serial; `TUKOS_VMRUN` aponta o `vmrun` se não estiver no caminho padrão |
| `cargo xtask test` | roda os testes unitários no host |
| `cargo xtask ntdll` | compila a `ntdll.dll` real; exige host Windows com as MSVC build tools e o target `x86_64-pc-windows-msvc` (os testes de host do ntdll no xtask têm o mesmo requisito e são ignorados fora do Windows) |

Variáveis opcionais: `TUKOS_QEMU` (caminho do `qemu-system-x86_64`) e `TUKOS_OVMF` (caminho do firmware OVMF). `TUKOS_HEADLESS=1` roda o QEMU sem janela; a CI já roda sem janela via `CI`.

## Licença

MIT OR Apache-2.0, à sua escolha.
