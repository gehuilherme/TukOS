# Política de clean room do TukOS

## Proibido sempre

- Consultar, possuir no ambiente de trabalho ou usar qualquer **código-fonte vazado** da Microsoft (incluindo vazamentos do Windows XP/Server 2003 e o Windows Research Kernel).
- Copiar código, shaders, recursos gráficos, ícones, sons ou textos da Microsoft.

## Níveis de fonte

| Nível | Fontes | Quem pode usar | Onde o resultado fica |
|---|---|---|---|
| **0 — Documentação pública** | Microsoft Learn, WDK/SDK headers públicos (`winternl.h` etc.), especificações (PE/COFF, UEFI, ACPI), livros técnicos | Todos | Repositório público `tukos` |
| **0 — Caixa-preta** | Saída e código de saída de **programas de teste nossos** executados no Windows real | Todos | `tukos/conformance/expected/` |
| **1 — Observação externa** | Metadados de interface (`dumpbin /exports`, `/headers`, `/imports` — **nunca `/disasm`**), rastreamento nas fronteiras de API (API Monitor, Process Monitor, ETW) ao rodar programas nossos, estrutura do sistema (WinObj, registro, serviços, `System32`) | Sessões de observação; resultado (descrição de comportamento) pode ser lido por todos | Repositório público `tukos`, em `docs/observations/` |
| **2 — Engenharia reversa** | Disassembly/descompilação (Ghidra, WinDbg) de binários da instalação licenciada do Windows do mantenedor | **Somente sessões sujas isoladas** | Specs no repositório **privado** `tukos-specs`; material bruto (projetos Ghidra, listagens) em pasta local fora de qualquer git |

## Processo do nível 2

1. **Pedido de spec:** registra a função ou comportamento a especificar e **por que os níveis 0 e 1 não bastaram**.
2. **Sessão suja:** um subagente isolado, sem contexto compartilhado com sessões limpas, analisa o binário e escreve uma spec de comportamento contendo apenas:
   - entradas, saídas, efeitos colaterais e ordem observável das operações;
   - códigos de erro (`NTSTATUS` / `GetLastError`) e em que condições ocorrem;
   - layouts de estruturas (offsets e tamanhos de campos) necessários à interoperabilidade.
3. **Revisor de spec:** outra sessão isolada rejeita a spec se ela contiver código, pseudocódigo, tradução instrução a instrução, ou nomes internos não públicos.
4. **Sessão limpa:** implementa somente a partir da spec aprovada. Nunca acessa material bruto.
5. **Papéis humanos:** o mantenedor atua **apenas no lado limpo** — lê specs aprovadas, revisa e dirige, mas não vê disassembly. Pode, portanto, escrever e revisar código em qualquer área.
6. O nível 2 nunca é necessário para o Marco 1 e só é usado quando os níveis 0 e 1 se mostrarem insuficientes.

## Proveniência

Todo commit que implementa comportamento compatível declara a fonte no rodapé da mensagem:

```
Source: docs <URL ou documento>
Source: blackbox <teste em conformance/>
Source: observation <arquivo em docs/observations/>
Source: spec-L2 <id da spec em tukos-specs>
```

Isso permite localizar e reescrever tudo que veio do nível 2, caso uma avaliação jurídica futura recomende.

## Avaliação jurídica

Antes da divulgação pública do projeto, recomenda-se consulta a um advogado de propriedade intelectual sobre a Lei 9.609/98 (Lei de Software), a cláusula de engenharia reversa do EULA do Windows e patentes. Clean room protege contra alegação de cópia de código, não contra patentes nem violação contratual.
