/*
 * TukOS Plano 21 / Task 4: the "Hello, TukOS!" Win32 GUI demo.
 *
 * CONSOLE subsystem, `int main` (reuses the working /MT mainCRTStartup path; NOT
 * /SUBSYSTEM:WINDOWS / WinMain). A real Win32 GUI program: RegisterClassA,
 * CreateWindowExA, ShowWindow, GetMessage/TranslateMessage/DispatchMessage loop.
 * On WM_PAINT it paints once, then PostQuitMessage(0) so it exits 0 (deterministic).
 *
 * Painted (client coords == screen coords on TukOS), checked by the ktest
 * `gui_hello_window`:
 *   background  (0,0)-(320,200)   RGB(32,32,48)   -> framebuffer 0x00202030
 *   rectangle   (10,10)-(200,60)  RGB(0,120,215)   -> framebuffer 0x000078D7
 *   text        "Hello, TukOS!" at (20,25), white RGB(255,255,255) -> 0x00FFFFFF,
 *               opaque background = the rectangle colour, so some pixel inside the
 *               text box (x 20..150, y 25..45) is exactly 0x00FFFFFF.
 *
 * Compiled /MT linking the REAL SDK user32.lib/gdi32.lib/kernel32.lib; at runtime
 * TukOS binds the imports to our own DLLs by name.
 */
#include <windows.h>

static LRESULT CALLBACK WndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        RECT bg = {0, 0, 320, 200};
        RECT r = {10, 10, 200, 60};
        HBRUSH bb = CreateSolidBrush(RGB(32, 32, 48));
        HBRUSH rb = CreateSolidBrush(RGB(0, 120, 215));
        FillRect(dc, &bg, bb);
        FillRect(dc, &r, rb);
        DeleteObject(bb);
        DeleteObject(rb);
        SetBkMode(dc, OPAQUE);
        SetBkColor(dc, RGB(0, 120, 215));
        SetTextColor(dc, RGB(255, 255, 255));
        TextOutA(dc, 20, 25, "Hello, TukOS!", 13);
        EndPaint(h, &ps);
        /* Paint once, then end the loop deterministically. */
        PostQuitMessage(0);
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcA(h, m, w, l);
}

int main(void) {
    WNDCLASSA wc = {0};
    MSG msg;
    HWND hwnd;

    wc.lpfnWndProc = WndProc;
    wc.hInstance = GetModuleHandleA(NULL);
    wc.hCursor = LoadCursorA(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "HelloTukOS";
    if (!RegisterClassA(&wc)) return 1;

    hwnd = CreateWindowExA(0, "HelloTukOS", "Hello", WS_OVERLAPPEDWINDOW,
                           CW_USEDEFAULT, CW_USEDEFAULT, 320, 200,
                           NULL, NULL, wc.hInstance, NULL);
    if (!hwnd) return 2;

    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);

    while (GetMessageA(&msg, NULL, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }
    return (int)msg.wParam;
}
