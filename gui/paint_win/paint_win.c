/*
 * TukOS Plano 23 / Task 3 mouse paint demo (`cargo xtask run gui-paint`).
 *
 * CONSOLE subsystem, `int main` (same /MT path as test_type/test_click). One window;
 * WM_PAINT fills (0,0)-(640,480) with RGB(32,32,48). The brush is a fixed 6x6 square of
 * RGB(255,255,0) (yellow) with its top-left at the mouse position (x = LOWORD(lParam),
 * y = HIWORD(lParam)). It is stamped on WM_LBUTTONDOWN and on WM_MOUSEMOVE while
 * MK_LBUTTON is set in wParam, so dragging leaves a trail. ESC (WM_KEYDOWN VK_ESCAPE) quits.
 *
 * Compiled /MT linking the SDK user32.lib/gdi32.lib; bound to OUR DLLs at runtime.
 */
#include <windows.h>

#define BRUSH 6

static void stamp(HWND h, LPARAM l) {
    int x = (int)LOWORD(l);
    int y = (int)HIWORD(l);
    HDC dc = GetDC(h);
    RECT sq;
    HBRUSH ink = CreateSolidBrush(RGB(255, 255, 0));
    sq.left = x;
    sq.top = y;
    sq.right = x + BRUSH;
    sq.bottom = y + BRUSH;
    FillRect(dc, &sq, ink);
    DeleteObject(ink);
    ReleaseDC(h, dc);
}

static LRESULT CALLBACK WndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        RECT bg = {0, 0, 640, 480};
        HBRUSH bb = CreateSolidBrush(RGB(32, 32, 48));
        FillRect(dc, &bg, bb);
        DeleteObject(bb);
        EndPaint(h, &ps);
        return 0;
    }
    case WM_LBUTTONDOWN:
        stamp(h, l);
        return 0;
    case WM_MOUSEMOVE:
        if (w & MK_LBUTTON) stamp(h, l);
        return 0;
    case WM_KEYDOWN:
        if (w == VK_ESCAPE) PostQuitMessage(0);
        return 0;
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
    wc.lpszClassName = "TukOSPaint";
    if (!RegisterClassA(&wc)) return 1;

    hwnd = CreateWindowExA(0, "TukOSPaint", "TukOS Paint", WS_OVERLAPPEDWINDOW,
                           CW_USEDEFAULT, CW_USEDEFAULT, 640, 480,
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
