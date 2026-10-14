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
