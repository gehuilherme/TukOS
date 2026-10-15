# Survey de imports: fopen/fread/fclose (/MT) - Plano 16

Fonte: `dumpbin /imports` e `/dependents` no `fileio.exe` compilado por nos a partir de
`conformance/fileio/fileio.c` (`cl /MT /GS-`, MSVC 14.29.30159, VS2019 BuildTools).
Nenhum binario da Microsoft foi aberto. Unica dependencia: `KERNEL32.dll`.

Programa: `fopen("C:\cat.txt","rb")`, loop `fread` (buf 256) e `fwrite(stdout)`, `fclose`.

## Imports de KERNEL32.dll (73)

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
CloseHandle
ReadFile
GetConsoleMode
ReadConsoleW
GetConsoleOutputCP
GetFileSizeEx
SetFilePointerEx
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
CreateFileW
HeapSize
HeapReAlloc
SetEndOfFile
_RDATA
_RDATA
```

## Delta vs. o kernel32 do TukOS

Dos 73 imports, **so um e novo** (`SetEndOfFile`); os demais ja estao exportados em
`crates/kernel32` (os de file I/O como stubs frios ou reais).

| Import | Estado hoje | Assinatura MSDN | Triagem |
|---|---|---|---|
| `SetEndOfFile` | ausente | `BOOL SetEndOfFile(HANDLE hFile)` | stub-benign: so `_chsize`/escrita; fopen "rb" nao chama. Exportar retornando FALSE + `ERROR_ACCESS_DENIED` (FS somente leitura) basta para resolver o IAT. |

Imports de file I/O que o CRT usa e que existem, mas precisam virar reais (load-bearing):

| Import | Estado hoje | Assinatura MSDN | Triagem / comportamento minimo |
|---|---|---|---|
| `CreateFileW` | stub: INVALID_HANDLE_VALUE + FILE_NOT_FOUND | `HANDLE CreateFileW(LPCWSTR name, DWORD access, DWORD share, LPSECURITY_ATTRIBUTES sa, DWORD disp, DWORD attrs, HANDLE tmpl)` | **load-bearing**. Converter `C:\x` em caminho NT, abrir somente leitura (OPEN_EXISTING), devolver handle de arquivo; falha: INVALID_HANDLE_VALUE + `ERROR_FILE_NOT_FOUND`(2). Escrita/CREATE_*: `ERROR_ACCESS_DENIED`(5). |
| `ReadFile` | real, via `NtReadFile` (console) | `BOOL ReadFile(HANDLE, LPVOID buf, DWORD n, LPDWORD read, LPOVERLAPPED)` | **load-bearing**: `NtReadFile` no kernel deve aceitar handle de arquivo (le do offset, avanca; 0 bytes no EOF com sucesso). |
| `SetFilePointerEx` | stub: FALSE + INVALID_HANDLE | `BOOL SetFilePointerEx(HANDLE, LARGE_INTEGER dist, PLARGE_INTEGER newpos, DWORD method)` | **load-bearing**: o CRT faz lseek (ftell/fseek/fclose); FILE_BEGIN/CURRENT/END sobre o offset do handle. |
| `GetFileSizeEx` | stub: FALSE | `BOOL GetFileSizeEx(HANDLE, PLARGE_INTEGER size)` | **load-bearing leve**: `_filelength`/`_fstat`; devolver o tamanho. |
| `GetFileType` | real: sempre FILE_TYPE_CHAR | `DWORD GetFileType(HANDLE)` | **load-bearing**: `_open_osfhandle` do CRT chama; deve devolver `FILE_TYPE_DISK`(1) para arquivo. Console continua CHAR. |
| `CloseHandle` | real, via `NtClose` | `BOOL CloseHandle(HANDLE)` | ja funciona se `NtClose` liberar Object::File. |
| `FlushFileBuffers` | real: sucesso | `BOOL FlushFileBuffers(HANDLE)` | ja temos; benigno. |
| `FindFirstFileExW`/`FindNextFileW`/`FindClose` | stubs | - | stub-benign (so `_stat`/`_findfirst`). |

## Respostas diretas

- `fopen` usa **`CreateFileW`** (nao ha `CreateFileA` no import table).
- **`GetFullPathNameW` nao e importado**; nada a stubar. Tambem nao ha `GetFileInformationByHandle(Ex)`, `GetFinalPathNameByHandleW`, `GetFileSize` nem `SetFilePointer` (sem Ex).
- Modo `"rb"` evita lookahead/BOM do modo texto; `"r"` pode gerar `SetFilePointerEx`/`ReadFile` extras na abertura.
- Demais imports (heap, TLS, FLS, locale, startup) ja cobertos pelo Plano 13.

## Split proposto

**Task 2 (kernel)**: `Object::File { dados: &'static [u8], pos }`; initrd embutido (FS read-only simples: tabela nome para bytes, p.ex. `C:\cat.txt`); `NT_OPEN_FILE` (resolve `\??\C:\cat.txt`, retorna handle, senao STATUS_OBJECT_NAME_NOT_FOUND); `NtReadFile` estendido a Object::File (copia a partir de `pos`, 0 bytes/END_OF_FILE no fim); seek (`NtSetInformationFile` FilePositionInformation ou syscall dedicada); size (`NtQueryInformationFile` FileStandardInformation); `NtClose` libera; forma de o kernel32 saber se o handle e disco (query de tipo).

**Task 3 (kernel32 + programa)**: `CreateFileW` real (wide para NT, access/disp para somente leitura), `ReadFile` (ja existe), `SetFilePointerEx`, `GetFileSizeEx`, `GetFileType` (DISK para arquivo), `SetEndOfFile` stub; programa `fileio` embutido + ktest e2e (cat.txt na serial).

## Preocupacoes

- `fread` do CRT aloca o buffer do FILE via HeapAlloc e le em blocos (4096): `ReadFile` deve aceitar leituras parciais/EOF.
- `GetFileType` retornando UNKNOWN(0) com erro faz `_open_osfhandle` falhar.
