# Roadmap do TukOS

| # | Subprojeto | Resultado demonstrável | Status |
|---|---|---|---|
| 0 | Infraestrutura: repositório, build, QEMU, CI, `AGENTS.md`, `CLEAN_ROOM.md`, harness de conformidade | `cargo xtask run` dá boot numa VM | ✅ mínimo (build/QEMU/CI/docs + harness de conformidade prontos) |
| 1 | Kernel base: boot UEFI, memória, interrupções, scheduler, syscalls | kernel roda e alterna threads | ✅ feito |
| 2 | Executivo: object manager, processos, I/O manager, registro | processos e handles estilo NT | 🟡 parcial (object manager + processos/handles prontos; **faltam** I/O manager e registro) |
| 3 | `ntdll` + loader de PE | `.exe` de console do Windows roda sem modificação | ✅ feito (loader de imports em modo usuário, exports/ordinais/forwarders; `.exe` **real do MSVC compilado com o CRT estático** `/MT` roda sem modificação e confere byte-a-byte com o Windows real) |
| 4 | Subsistema Win32: `kernel32`, `user32`, `gdi32`, gerenciador de janelas | programa com janela abre | 🟡 início (`kernel32` mínimo: `GetStdHandle`/`WriteFile`/`ExitProcess`/`GetProcessHeap`/`HeapAlloc`/`HeapFree` + heap de usuário; superfície do CRT (TLS/FLS/locks/cookie/HeapSize) pronta; `user32`/`gdi32`/janelas pendentes) |
| 5 | Modelo de drivers WDM/KMDF | driver `.sys` real carrega | ⬜ pendente |
| 6 | Drivers e sistemas de arquivos: NVMe/AHCI, NTFS/FAT, USB, rede, vídeo | instala e roda em hardware real | ⬜ pendente |
| 7 | Shell, desktop, instalador, privacidade | usável no dia a dia | ⬜ pendente (design do desktop já esboçado) |

**Legenda:** ✅ feito · 🟡 em andamento/parcial · ⬜ não iniciado.

**Marco atual:** Marco 1 praticamente atingido do lado do `.exe` — boot no QEMU (0, 1) + `.exe` de console **real do MSVC com CRT completo** (`hello.c` via `printf`, `/MT`) roda no TukOS e confere stdout + exit code com o Windows real (harness: ntdll, kernel32, heap, crt). Partes de 2 (object manager, processos) e de 4 (`kernel32` com superfície do CRT + heap) prontas.

**Em aberto:** I/O manager e registro (resto do subprojeto 2); `user32`/`gdi32` e gerenciador de janelas (resto do 4).
