# Roadmap do TukOS

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

**Marco atual:** Marco 1 — boot no QEMU e execução de .exe real do Windows (subprojetos 0, 1 e 3 em versão mínima).
