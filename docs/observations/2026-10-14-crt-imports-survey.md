# Survey: imports de um hello.exe compilado com o CRT estatico do MSVC (/MT)

Data: 2026-10-14. Plano 13, Task 1.

## Metodo (clean room)

- Fonte: `conformance/crt/hello.c` (`printf("Hello, TukOS!\n"); return 0;`).
- Build: `cl /nologo /MT /GS- hello.c` (MSVC 2019 BuildTools, x64), `mainCRTStartup` padrao.
- Unica inspecao: `dumpbin /imports` e `/dependents` do **nosso** `hello.exe` (le o import
  directory = nomes publicos da API Win32). Nenhum binario da Microsoft foi desmontado.
  O `hello.exe` roda no Windows real e imprime `Hello, TukOS!`.
- A coluna "uso no CRT" e **hipotese** derivada do nome da API + documentacao publica; a
  confirmacao vem dos ktests/serial (qual import e de fato chamado), nao de disassembly.
- `/GS-` nao elimina o startup do cookie: o `mainCRTStartup` ainda executa `__security_init_cookie`
  (codigo do proprio CRT estatico), que usa GetSystemTimeAsFileTime, GetCurrentThreadId,
  GetCurrentProcessId e QueryPerformanceCounter. Nao e import novo, mas essas 4 sao exercitadas.

## `dumpbin /dependents`

Somente `KERNEL32.dll`. **Nao ha import de ntdll.dll, nem VirtualAlloc/VirtualFree/VirtualQuery,
EncodePointer/DecodePointer, HeapCreate ou GetEnvironmentVariable.** 72 funcoes importadas.

## Imports completos (72) - KERNEL32.dll

QueryPerformanceCounter, GetCurrentProcessId, GetCurrentThreadId, GetSystemTimeAsFileTime,
InitializeSListHead, RtlCaptureContext, RtlLookupFunctionEntry, RtlVirtualUnwind,
IsDebuggerPresent, UnhandledExceptionFilter, SetUnhandledExceptionFilter, GetStartupInfoW,
IsProcessorFeaturePresent, GetModuleHandleW, WriteConsoleW, RtlUnwindEx, GetLastError,
SetLastError, EnterCriticalSection, LeaveCriticalSection, DeleteCriticalSection,
InitializeCriticalSectionAndSpinCount, TlsAlloc, TlsGetValue, TlsSetValue, TlsFree,
FreeLibrary, GetProcAddress, LoadLibraryExW, RaiseException, GetStdHandle, WriteFile,
GetModuleFileNameW, GetCurrentProcess, ExitProcess, TerminateProcess, GetModuleHandleExW,
GetCommandLineA, GetCommandLineW, HeapAlloc, HeapFree, CompareStringW, LCMapStringW,
GetFileType, FindClose, FindFirstFileExW, FindNextFileW, IsValidCodePage, GetACP, GetOEMCP,
GetCPInfo, MultiByteToWideChar, WideCharToMultiByte, GetEnvironmentStringsW,
FreeEnvironmentStringsW, SetEnvironmentVariableW, SetStdHandle, GetStringTypeW,
GetProcessHeap, FlushFileBuffers, GetConsoleOutputCP, GetConsoleMode, GetFileSizeEx,
SetFilePointerEx, HeapSize, HeapReAlloc, CloseHandle, CreateFileW

## Triagem

Fonte de implementacao para tudo: MSDN (referencia Win32 de cada funcao) e `winnt.h`/`winternl.h`
para estruturas. "hot" = provavelmente chamado num hello; "cold" = referenciado, mas so em
caminho de erro/outras APIs.

### (a) Ja temos (7)

GetStdHandle, WriteFile, ExitProcess, GetProcessHeap, HeapAlloc, HeapFree, CloseHandle
(`crates/kernel32`). `HeapAlloc` precisa aceitar HEAP_ZERO_MEMORY e blocos grandes (buffer de stdout).

### (b) Stub benigno

| API | Assinatura (MSDN) | Comportamento minimo | Hot? |
|---|---|---|---|
| IsDebuggerPresent | `BOOL (void)` | 0 | hot |
| SetUnhandledExceptionFilter | `LPTOP_LEVEL_EXCEPTION_FILTER (LPTOP_LEVEL_EXCEPTION_FILTER)` | NULL | talvez |
| UnhandledExceptionFilter | `LONG (EXCEPTION_POINTERS*)` | 0 (CONTINUE_SEARCH) | cold |
| InitializeSListHead | `void (PSLIST_HEADER)` | zerar 16 bytes | hot |
| GetStartupInfoW | `void (LPSTARTUPINFOW)` | zerar a struct, `cb = sizeof` | hot |
| IsProcessorFeaturePresent | `BOOL (DWORD)` | 10 (SSE2)=1, demais=0 (conservador) | hot |
| GetCurrentProcessId / GetCurrentThreadId | `DWORD (void)` | valores fixos/reais | hot (cookie) |
| GetCurrentProcess | `HANDLE (void)` | pseudo-handle -1 | hot |
| TerminateProcess | `BOOL (HANDLE, UINT)` | -> NtTerminateProcess | cold |
| RaiseException | `void (DWORD, DWORD, DWORD, const ULONG_PTR*)` | abort/ExitProcess | cold |
| RtlCaptureContext | `void (PCONTEXT)` | zera a CONTEXT | cold |
| RtlLookupFunctionEntry | `PRUNTIME_FUNCTION (DWORD64, PDWORD64, PUNWIND_HISTORY_TABLE)` | NULL | cold |
| RtlVirtualUnwind | `PEXCEPTION_ROUTINE (DWORD, DWORD64, DWORD64, PRUNTIME_FUNCTION, PCONTEXT, PVOID*, PDWORD64, PKNONVOLATILE_CONTEXT_POINTERS)` | NULL | cold |
| RtlUnwindEx | `void (PVOID, PVOID, PEXCEPTION_RECORD, PVOID, PCONTEXT, PUNWIND_HISTORY_TABLE)` | abort | cold |
| FreeLibrary | `BOOL (HMODULE)` | 1 | cold |
| FlushFileBuffers | `BOOL (HANDLE)` | 1 | talvez (flush no exit) |
| SetStdHandle | `BOOL (DWORD, HANDLE)` | 1 | cold |
| SetEnvironmentVariableW | `BOOL (LPCWSTR, LPCWSTR)` | 0 + SetLastError | cold |
| FindFirstFileExW / FindNextFileW / FindClose | MSDN | INVALID_HANDLE_VALUE / 0 / 1 | cold (glob de argv) |
| CreateFileW | `HANDLE (LPCWSTR, DWORD, DWORD, LPSECURITY_ATTRIBUTES, DWORD, DWORD, HANDLE)` | INVALID_HANDLE_VALUE | cold |
| GetFileSizeEx / SetFilePointerEx | MSDN | 0 + erro | cold |
| CompareStringW | `int (LCID, DWORD, PCNZWCH, int, PCNZWCH, int)` | comparacao ordinal | cold |
| HeapSize / HeapReAlloc | MSDN | sobre RtlSizeHeap/ReAllocateHeap ou stub | cold |
| GetModuleHandleExW | `BOOL (DWORD, LPCWSTR, HMODULE*)` | base da imagem / 0 | cold em exe |
| DeleteCriticalSection | `void (LPCRITICAL_SECTION)` | noop | cold |

### (c) Load-bearing (devem funcionar de verdade)

| API | Assinatura | Uso provavel pelo CRT | Minimo |
|---|---|---|---|
| GetCommandLineA / W | `LPSTR/LPWSTR (void)` | argc/argv | string estatica (`hello.exe`) ou do PEB |
| GetModuleFileNameW | `DWORD (HMODULE, LPWSTR, DWORD)` | argv[0] | nome literal; retorna comprimento |
| GetModuleHandleW | `HMODULE (LPCWSTR)` | NULL -> base da imagem (CRT le o PE header); nome `api-ms-win-*` -> NULL | PEB.ImageBaseAddress |
| GetEnvironmentStringsW / FreeEnvironmentStringsW | `LPWCH (void)` / `BOOL (LPWCH)` | ambiente | bloco vazio `\0\0`; Free=1 |
| WideCharToMultiByte / MultiByteToWideChar | MSDN | converter env/cmdline/locale | identidade ASCII (CP 437/1252/UTF-8) |
| GetACP / GetOEMCP / GetCPInfo / IsValidCodePage | MSDN | init de locale "C" (`_setmbcp`) | 1252/437; CPINFO single-byte; IsValid=1 |
| GetStringTypeW | `BOOL (DWORD, LPCWCH, int, LPWORD)` | tabela ctype | C1_ para ASCII (nem sempre chamado) |
| GetFileType | `DWORD (HANDLE)` | `_ioinit`/`_isatty`/`_write` | FILE_TYPE_CHAR (2) p/ console/serial |
| GetConsoleMode | `BOOL (HANDLE, LPDWORD)` | `_write` decide WriteConsoleW vs WriteFile | devolver 0 -> CRT usa WriteFile (mais simples) |
| GetConsoleOutputCP | `UINT (void)` | `_write` em console | 437/65001 |
| WriteConsoleW | `BOOL (HANDLE, const void*, DWORD, LPDWORD, LPVOID)` | so se GetConsoleMode ok | stub |
| GetLastError / SetLastError | `DWORD (void)` / `void (DWORD)` | todo o CRT | por-thread (TEB.LastErrorValue, 0x68) |
| InitializeCriticalSectionAndSpinCount / Enter / Leave | MSDN | locks de stdio/heap/locale/atexit (**hot**: printf trava o FILE) | single-thread: Init=1, Enter/Leave noop seguro |
| TlsAlloc / TlsGetValue / TlsSetValue / TlsFree | `DWORD (void)` / `LPVOID (DWORD)` / `BOOL (DWORD, LPVOID)` / `BOOL (DWORD)` | fallback do FLS (se `FlsAlloc` nao resolver), errno, per-thread data | TEB.TlsSlots[64] (0x1480) ou array estatico |
| GetProcAddress | `FARPROC (HMODULE, LPCSTR)` | resolve APIs dinamicas (Fls*, InitializeCriticalSectionEx, LCMapStringEx...) | NULL faz o CRT cair nos fallbacks; **ponto de decisao**: NULL universal vs tabela |
| LoadLibraryExW | `HMODULE (LPCWSTR, HANDLE, DWORD)` | idem p/ api-ms-win-* | NULL + ERROR_MOD_NOT_FOUND (126) |
| GetSystemTimeAsFileTime | `void (LPFILETIME)` | `__security_init_cookie` | qualquer valor |
| QueryPerformanceCounter | `BOOL (LARGE_INTEGER*)` | idem | contador crescente (rdtsc) |
| WriteFile / GetStdHandle / Heap* | (a) | `_write_nolock`, `malloc` | ja existem |

### (d) Hard

Nenhum import e "hard" no caminho feliz. Rtl{CaptureContext,LookupFunctionEntry,VirtualUnwind,UnwindEx},
RaiseException, UnhandledExceptionFilter so entram em excecao/`__fastfail`/abort. O `mainCRTStartup`
envolve `main` num `__try/__except`, mas isso so produz `.pdata`/UNWIND_INFO e tabelas de handler,
sem chamada em tempo de execucao se nao ha excecao. **O caminho SEH nao parece quente.** O cookie
(`__security_init_cookie`) e quente mas nao e hard (so precisa das 4 APIs de entropia).

## Proposta de divisao

**Task 2 (infra + startup)** - nao precisa de VirtualAlloc (o heap vem de HeapAlloc existente):
- LastError e TLS (GetLastError/SetLastError, Tls*), critical sections simples.
- Cookie/startup: GetSystemTimeAsFileTime, QueryPerformanceCounter, GetCurrentProcessId, GetCurrentThreadId,
  GetCurrentProcess, InitializeSListHead, IsProcessorFeaturePresent, IsDebuggerPresent,
  SetUnhandledExceptionFilter, UnhandledExceptionFilter, GetStartupInfoW.
- Modulos: GetModuleHandleW, GetModuleHandleExW, GetModuleFileNameW, GetProcAddress (NULL), LoadLibraryExW (NULL+126), FreeLibrary.
- Cmdline/env: GetCommandLineA/W, GetEnvironmentStringsW, FreeEnvironmentStringsW.
- Stubs frios para os 72 imports resolverem: Rtl*, RaiseException, TerminateProcess, Find*, CreateFileW,
  SetFilePointerEx, GetFileSizeEx, SetStdHandle, SetEnvironmentVariableW, HeapSize, HeapReAlloc,
  CompareStringW, DeleteCriticalSection.

**Task 3 (stdio + chegar a main)**:
- Locale/codepage: GetACP, GetOEMCP, GetCPInfo, IsValidCodePage, MultiByteToWideChar, WideCharToMultiByte,
  GetStringTypeW, LCMapStringW.
- Console/arquivo: GetFileType, GetConsoleMode (0 -> WriteFile), GetConsoleOutputCP, WriteConsoleW (stub), FlushFileBuffers.
- Heap com HEAP_ZERO_MEMORY; validar `main()` -> `ExitProcess(0)` com `Hello, TukOS!` na serial.

## Notas / incertezas

- "Hot" e hipotese; validar com stubs que logam o nome na serial.
- GetProcAddress/LoadLibraryExW -> NULL e a aposta de menor custo; se algum ponteiro for usado sem
  checagem de NULL o CRT quebra e teremos que prover o simbolo.
- Nao ha ntdll direto no PE; ntdll so e usado indiretamente por nosso kernel32.
