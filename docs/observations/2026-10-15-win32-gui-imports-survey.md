# Survey dos imports de um programa Win32 GUI minimo (/MT)

Data: 2026-10-15. Fonte: `dumpbin /imports` executado SOMENTE sobre o nosso proprio
`gui/hello_win/hello_win.exe` (compilado de `gui/hello_win/hello_win.c`, `cl /MT /W3`,
`/SUBSYSTEM:WINDOWS`, linkado com `user32.lib gdi32.lib` da SDK apenas para compilar).
Nenhum binario da Microsoft foi inspecionado. Assinaturas e structs abaixo vem da
documentacao publica (MSDN) e dos headers publicos da SDK (`winuser.h`, `wingdi.h`, `windef.h`).

Toolchain: VS 2019 BuildTools (MSVC 14.29), achado via `vswhere` como em `xtask/src/conformance.rs`.

## Imports por DLL (autoridade: tabela de imports do exe)

### USER32.dll (13)

| Funcao | Assinatura (MSDN) |
|---|---|
| RegisterClassA | `ATOM RegisterClassA(const WNDCLASSA *lpWndClass)` |
| CreateWindowExA | `HWND CreateWindowExA(DWORD dwExStyle, LPCSTR lpClassName, LPCSTR lpWindowName, DWORD dwStyle, int X, int Y, int nWidth, int nHeight, HWND hWndParent, HMENU hMenu, HINSTANCE hInstance, LPVOID lpParam)` |
| ShowWindow | `BOOL ShowWindow(HWND hWnd, int nCmdShow)` |
| UpdateWindow | `BOOL UpdateWindow(HWND hWnd)` |
| GetMessageA | `BOOL GetMessageA(LPMSG lpMsg, HWND hWnd, UINT wMsgFilterMin, UINT wMsgFilterMax)` (retorna <0 erro, 0 em WM_QUIT) |
| TranslateMessage | `BOOL TranslateMessage(const MSG *lpMsg)` |
| DispatchMessageA | `LRESULT DispatchMessageA(const MSG *lpMsg)` |
| DefWindowProcA | `LRESULT DefWindowProcA(HWND hWnd, UINT Msg, WPARAM wParam, LPARAM lParam)` |
| PostQuitMessage | `void PostQuitMessage(int nExitCode)` |
| LoadCursorA | `HCURSOR LoadCursorA(HINSTANCE hInstance, LPCSTR lpCursorName)` (IDC_ARROW = `MAKEINTRESOURCE(32512)`) |
| BeginPaint | `HDC BeginPaint(HWND hWnd, LPPAINTSTRUCT lpPaint)` |
| EndPaint | `BOOL EndPaint(HWND hWnd, const PAINTSTRUCT *lpPaint)` |
| FillRect | `int FillRect(HDC hDC, const RECT *lprc, HBRUSH hbr)` |

### GDI32.dll (6)

| Funcao | Assinatura (MSDN) |
|---|---|
| CreateSolidBrush | `HBRUSH CreateSolidBrush(COLORREF color)` |
| GetStockObject | `HGDIOBJ GetStockObject(int i)` (WHITE_BRUSH = 0) |
| DeleteObject | `BOOL DeleteObject(HGDIOBJ ho)` |
| SetBkMode | `int SetBkMode(HDC hdc, int mode)` (TRANSPARENT = 1, OPAQUE = 2) |
| SetTextColor | `COLORREF SetTextColor(HDC hdc, COLORREF color)` |
| TextOutA | `BOOL TextOutA(HDC hdc, int x, int y, LPCSTR lpString, int c)` |

### Onde moram FillRect / BeginPaint / EndPaint

Resolvido pela tabela real: **os tres estao em USER32.dll** (FillRect, BeginPaint, EndPaint).
Nao ha nenhum import de gdi32 para eles. `CreateSolidBrush`/`GetStockObject`/`DeleteObject`/
`SetBkMode`/`SetTextColor`/`TextOutA` estao em GDI32.dll.

### KERNEL32.dll (65)

Nenhuma outra DLL e importada (so USER32, GDI32, KERNEL32). Os imports de KERNEL32 vem
quase todos do runtime C estatico (/MT); a lista completa esta no dump abaixo. O app em si
nao chama nenhum diretamente. Hipotese (nao verificada import a import): o startup
`WinMainCRTStartup` usa `GetStartupInfoW` para obter `nCmdShow`; `GetModuleHandleW`,
`GetStartupInfoW` e `GetCommandLineA/W` ja existem em `crates/kernel32/src/lib.rs`. Os demais
imports do CRT (`FindFirstFileExW`, `LCMapStringW`, `GetStringTypeW`, `LoadLibraryExW`, etc.)
provavelmente so precisam existir no export do nosso kernel32 para o loader resolver o IAT
(stubs bastam se nao forem executados); a conferencia fica para a T4. Ponto de atencao: o CRT
de WinMain usa `WinMainCRTStartup`, diferente do `mainCRTStartup` dos testes de conformidade atuais.

## Structs (layout publico, x64)

```c
typedef struct tagWNDCLASSA {          // 72 bytes
    UINT      style;                   // 0
    WNDPROC   lpfnWndProc;             // 8
    int       cbClsExtra;              // 16
    int       cbWndExtra;              // 20
    HINSTANCE hInstance;               // 24
    HICON     hIcon;                   // 32
    HCURSOR   hCursor;                 // 40
    HBRUSH    hbrBackground;           // 48
    LPCSTR    lpszMenuName;            // 56
    LPCSTR    lpszClassName;           // 64
} WNDCLASSA;

typedef struct tagMSG {                // 48 bytes
    HWND   hwnd;                       // 0
    UINT   message;                    // 8
    WPARAM wParam;                     // 16
    LPARAM lParam;                     // 24
    DWORD  time;                       // 32
    POINT  pt;                         // 36
    DWORD  lPrivate;                   // 44 (campo oculto em winuser.h recente; sizeof 48 de qualquer forma)
} MSG;

typedef struct tagPAINTSTRUCT {        // 72 bytes
    HDC  hdc;                          // 0
    BOOL fErase;                       // 8
    RECT rcPaint;                      // 12
    BOOL fRestore;                     // 28
    BOOL fIncUpdate;                   // 32
    BYTE rgbReserved[32];              // 36
} PAINTSTRUCT;

typedef struct tagRECT  { LONG left, top, right, bottom; } RECT;   // 16 bytes
typedef struct tagPOINT { LONG x, y; } POINT;                      //  8 bytes
// COLORREF = DWORD 0x00bbggrr ; RGB(r,g,b) = r | g<<8 | b<<16
```

Constantes usadas: `WM_PAINT=0x000F`, `WM_DESTROY=0x0002`, `WM_QUIT=0x0012`,
`WS_OVERLAPPEDWINDOW=0x00CF0000`, `CW_USEDEFAULT=0x80000000`, `SW_*` (nCmdShow vem do
startup), `TRANSPARENT=1`, `WHITE_BRUSH=0`, `IDC_ARROW=32512`.

## Split recomendado

- **Crate `gdi32`** (cdylib, exporta): `CreateSolidBrush`, `GetStockObject`, `DeleteObject`,
  `SetBkMode`, `SetTextColor`, `TextOutA`. Precisa de fonte bitmap embutida (para TextOutA),
  DC com cor de texto/modo de fundo/brush, e tabela de objetos GDI (handles).
- **Crate `user32`** (cdylib, exporta): `RegisterClassA`, `CreateWindowExA`, `ShowWindow`,
  `UpdateWindow`, `GetMessageA`, `TranslateMessage`, `DispatchMessageA`, `DefWindowProcA`,
  `PostQuitMessage`, `LoadCursorA`, `BeginPaint`, `EndPaint`, `FillRect`.
  Observacao: `FillRect` (user32) opera sobre HDC e HBRUSH criados em gdi32 e `BeginPaint`
  devolve um HDC; portanto user32 e gdi32 precisam compartilhar a representacao de DC/handle
  de objeto (tipicamente via syscalls do kernel / estado no kernel, nao estado em DLL).
- Dependencia entre elas: user32 -> (HDC/HBRUSH do gdi32) apenas por contrato de handle;
  nenhuma importa a outra no hello_win.

Fatiamento sugerido das tasks:
- **T2 (kernel framebuffer + desenho gdi32):** surface/FB no kernel, syscalls de fill-rect e
  draw-text, DC/objetos GDI, e o crate `gdi32`.
- **T3 (user32 janela/loop):** tabela de janelas/classes, fila de mensagens por thread,
  `GetMessage`/`DispatchMessage`/`DefWindowProc`, `WM_PAINT` sintetico no primeiro
  `ShowWindow`/`UpdateWindow`, `BeginPaint`/`EndPaint`/`FillRect`, `WM_DESTROY`/`PostQuitMessage`.
- **T4 (programa + verificacao):** compilar `hello_win` linkando as NOSSAS import libs
  (`user32`/`gdi32`), rodar headless e verificar pixels do framebuffer; ajustar kernel32 para
  o startup `WinMainCRTStartup` (`GetStartupInfoW`).

## Dump bruto de `dumpbin /imports hello_win.exe`

```
USER32.dll:   TranslateMessage LoadCursorA FillRect EndPaint BeginPaint UpdateWindow
              ShowWindow CreateWindowExA RegisterClassA PostQuitMessage DefWindowProcA
              DispatchMessageA GetMessageA
GDI32.dll:    SetTextColor SetBkMode GetStockObject DeleteObject CreateSolidBrush TextOutA
KERNEL32.dll: GetConsoleMode GetConsoleOutputCP SetFilePointerEx HeapReAlloc HeapSize
              CreateFileW CloseHandle WriteConsoleW FlushFileBuffers LoadLibraryExW
              RtlCaptureContext RtlLookupFunctionEntry RtlVirtualUnwind
              UnhandledExceptionFilter SetUnhandledExceptionFilter GetCurrentProcess
              TerminateProcess IsProcessorFeaturePresent QueryPerformanceCounter
              GetCurrentProcessId GetCurrentThreadId GetSystemTimeAsFileTime
              InitializeSListHead IsDebuggerPresent GetStartupInfoW GetModuleHandleW
              GetProcessHeap RtlUnwindEx GetLastError SetLastError EnterCriticalSection
              LeaveCriticalSection DeleteCriticalSection
              InitializeCriticalSectionAndSpinCount TlsAlloc TlsGetValue TlsSetValue
              TlsFree FreeLibrary GetProcAddress RaiseException GetStdHandle WriteFile
              GetModuleFileNameW ExitProcess GetModuleHandleExW HeapAlloc HeapFree
              FindClose FindFirstFileExW FindNextFileW IsValidCodePage GetACP GetOEMCP
              GetCPInfo GetCommandLineA GetCommandLineW MultiByteToWideChar
              WideCharToMultiByte GetEnvironmentStringsW FreeEnvironmentStringsW
              SetStdHandle GetFileType GetStringTypeW LCMapStringW
```

(Importados por ordem de IAT.)
