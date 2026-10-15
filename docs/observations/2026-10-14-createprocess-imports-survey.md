# Survey dos imports de CreateProcess/Wait (/MT) - 2026-10-14

Fonte: binarios NOSSOS (`conformance/spawn/parent.c`, `child.c`), `cl /MT /GS-` (VS2019 BuildTools 14.29), `dumpbin /imports` e `/dependents`. Nenhum binario da Microsoft foi aberto.

## Decisoes do fonte

- `parent.c` usa **CreateProcessA** (nao W), `lpApplicationName = "child.exe"` (relativo ao CWD), `lpCommandLine = NULL`, `bInheritHandles = FALSE`, flags 0.
- `WaitForSingleObject(pi.hProcess, INFINITE)`: INFINITE = `0xFFFFFFFF`.
- `#include <windows.h>` compila limpo com /MT.
- `/dependents` do parent: so `KERNEL32.dll`. child.exe: so `KERNEL32.dll`.

## dumpbin /imports parent.exe (KERNEL32.dll, 71 imports)

```
CloseHandle
WaitForSingleObject
GetExitCodeProcess
CreateProcessA
QueryPerformanceCounter
GetCurrentProcessId
GetCurrentThreadId
GetSystemTimeAsFileTime
InitializeSListHead
RtlCaptureContext
RtlLookupFunctionEntry
RtlVirtualUnwind
IsDebuggerPresent
UnhandledExceptionFilter
SetUnhandledExceptionFilter
GetStartupInfoW
IsProcessorFeaturePresent
GetModuleHandleW
WriteConsoleW
RtlUnwindEx
GetLastError
SetLastError
EnterCriticalSection
LeaveCriticalSection
DeleteCriticalSection
InitializeCriticalSectionAndSpinCount
TlsAlloc
TlsGetValue
TlsSetValue
TlsFree
FreeLibrary
GetProcAddress
LoadLibraryExW
RaiseException
GetStdHandle
WriteFile
GetModuleFileNameW
GetCurrentProcess
ExitProcess
TerminateProcess
GetModuleHandleExW
GetCommandLineA
GetCommandLineW
HeapAlloc
HeapFree
CompareStringW
LCMapStringW
GetFileType
FindClose
FindFirstFileExW
FindNextFileW
IsValidCodePage
GetACP
GetOEMCP
GetCPInfo
MultiByteToWideChar
WideCharToMultiByte
GetEnvironmentStringsW
FreeEnvironmentStringsW
SetEnvironmentVariableW
SetStdHandle
GetStringTypeW
GetProcessHeap
FlushFileBuffers
GetConsoleOutputCP
GetConsoleMode
GetFileSizeEx
SetFilePointerEx
HeapSize
HeapReAlloc
CreateFileW
```

## dumpbin /imports child.exe (KERNEL32.dll, 68 imports)

```
QueryPerformanceCounter
GetCurrentProcessId
GetCurrentThreadId
GetSystemTimeAsFileTime
InitializeSListHead
RtlCaptureContext
RtlLookupFunctionEntry
RtlVirtualUnwind
IsDebuggerPresent
UnhandledExceptionFilter
SetUnhandledExceptionFilter
GetStartupInfoW
IsProcessorFeaturePresent
GetModuleHandleW
WriteConsoleW
RtlUnwindEx
GetLastError
SetLastError
EnterCriticalSection
LeaveCriticalSection
DeleteCriticalSection
InitializeCriticalSectionAndSpinCount
TlsAlloc
TlsGetValue
TlsSetValue
TlsFree
FreeLibrary
GetProcAddress
LoadLibraryExW
RaiseException
GetStdHandle
WriteFile
GetModuleFileNameW
GetCurrentProcess
ExitProcess
TerminateProcess
GetModuleHandleExW
GetCommandLineA
GetCommandLineW
HeapAlloc
HeapFree
CompareStringW
LCMapStringW
GetFileType
FindClose
FindFirstFileExW
FindNextFileW
IsValidCodePage
GetACP
GetOEMCP
GetCPInfo
MultiByteToWideChar
WideCharToMultiByte
GetEnvironmentStringsW
FreeEnvironmentStringsW
SetEnvironmentVariableW
SetStdHandle
GetStringTypeW
GetProcessHeap
FlushFileBuffers
GetConsoleOutputCP
GetConsoleMode
GetFileSizeEx
SetFilePointerEx
HeapSize
HeapReAlloc
CloseHandle
CreateFileW
```

child.exe e um subconjunto estrito do parent: **nao precisa de nada alem do que o kernel32 do TukOS ja exporta** (mesmo conjunto do survey de CRT/fopen).

## Delta vs kernel32 do TukOS (`crates/kernel32/src/lib.rs`)

Dos 71 imports do parent, 68 ja existem (real ou stub). Novos: **3**.

| Import | Status | Classe | Assinatura MSDN |
|---|---|---|---|
| CreateProcessA | novo | load-bearing | `BOOL CreateProcessA(LPCSTR lpApplicationName, LPSTR lpCommandLine, LPSECURITY_ATTRIBUTES lpProcessAttributes, LPSECURITY_ATTRIBUTES lpThreadAttributes, BOOL bInheritHandles, DWORD dwCreationFlags, LPVOID lpEnvironment, LPCSTR lpCurrentDirectory, LPSTARTUPINFOA lpStartupInfo, LPPROCESS_INFORMATION lpProcessInformation)` |
| WaitForSingleObject | novo | load-bearing | `DWORD WaitForSingleObject(HANDLE hHandle, DWORD dwMilliseconds)` -> WAIT_OBJECT_0 = 0, WAIT_TIMEOUT = 0x102, WAIT_FAILED = 0xFFFFFFFF |
| GetExitCodeProcess | novo | load-bearing | `BOOL GetExitCodeProcess(HANDLE hProcess, LPDWORD lpExitCode)` (STILL_ACTIVE = 259 enquanto vivo) |

Ja temos (reaproveitados): CloseHandle (precisa passar a fechar handles de processo/thread), GetStartupInfoW, GetFileType, CreateFileW, ReadFile/WriteFile, ExitProcess, Heap*, etc.

## Layouts x64

STARTUPINFOA (104 bytes): cb@0 (DWORD), lpReserved@8, lpDesktop@16, lpTitle@24, dwX@32, dwY@36, dwXSize@40, dwYSize@44, dwXCountChars@48, dwYCountChars@52, dwFillAttribute@56, dwFlags@60, wShowWindow@64 (WORD), cbReserved2@66 (WORD), lpReserved2@72, hStdInput@80, hStdOutput@88, hStdError@96. O parent so seta `cb = 104`; o resto zero.

PROCESS_INFORMATION (24 bytes): hProcess@0 (HANDLE), hThread@8 (HANDLE), dwProcessId@16 (DWORD), dwThreadId@20 (DWORD). O parent nao inicializa `pi` antes da chamada (o kernel32 deve escrever os 4 campos).

## Proposta de split

- **Task 2 (kernel)**: syscalls `NtTukosCreateProcess(path, cmdline) -> handle de processo` carregando o PE do initrd (reuso do loader), objeto Processo (estado, exit code), `NtTukosWaitProcess(handle, timeout)` (bloqueia/cede ate o filho sair; INFINITE = 0xFFFFFFFF), `NtTukosGetExitCode(handle)`; ExitProcess do filho grava o exit code e acorda quem espera; CloseHandle de processo/thread. Handle de thread pode ser um handle placeholder (par com o processo).
- **Task 3 (kernel32)**: `CreateProcessA` (converte app name/cmdline, chama a syscall, preenche PROCESS_INFORMATION, hThread/ids), `WaitForSingleObject`, `GetExitCodeProcess` (wrappers finos), estender `CloseHandle`. Ignorar security attrs, env, cwd, inherit, flags no primeiro corte.
- Task 4: xtask conformance com parent+child no initrd, CWD fixo, esperado `filho ok` + `filho saiu com 42`.

## Observacoes

- Se o app name for NULL e so a command line for usada, parse da primeira palavra seria necessario; o fonte atual nao cobre isso.
- Sem multitarefa preemptiva, `Wait` pode ser implementado executando o filho ate terminar (run-to-completion) se o kernel ainda nao tiver scheduler; decisao da Task 2.
