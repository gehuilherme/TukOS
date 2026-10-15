/*
 * TukOS Plano 21 / Task 3 GUI autotest program.
 *
 * CONSOLE subsystem, `int main` (reuses the working /MT mainCRTStartup path; NOT
 * /SUBSYSTEM:WINDOWS / WinMain). Deterministic: it registers a class, creates one
 * window, shows it, runs the message loop once, paints a KNOWN rectangle in a KNOWN
 * colour on WM_PAINT, then PostQuitMessage(0) so it paints exactly once and exits 0.
 *
 * Painted rectangle:  left=50, top=50, right=150, bottom=150  (client coords)
 * Brush colour:       RGB(0, 0, 255)  ->  COLORREF 0x00FF0000
 * Framebuffer pixel:  0x000000FF (blue, 0x00RRGGBB) at e.g. (100,100), inside the rect.
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
        RECT r = {50, 50, 150, 150};
        HBRUSH b = CreateSolidBrush(RGB(0, 0, 255));
        FillRect(dc, &r, b);
        DeleteObject(b);
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
    wc.lpszClassName = "TukOSTestWin";
    if (!RegisterClassA(&wc)) return 1;

    hwnd = CreateWindowExA(0, "TukOSTestWin", "TukOS Test", WS_OVERLAPPEDWINDOW,
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
