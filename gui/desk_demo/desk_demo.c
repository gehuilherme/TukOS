/*
 * TukOS Plano 25 / Task 4 desktop demo (`cargo xtask run gui-desk`).
 *
 * CONSOLE subsystem, `int main` (same /MT path as the other GUI programs). ONE process
 * creates TWO positioned, overlapping windows:
 *   A at (60,50)   240x160  client fill RGB(200,60,60)  + white bar  (10,10)-(110,30)
 *   B at (180,120) 240x160  client fill RGB(60,160,80)  + yellow bar (10,10)-(110,30)
 * B is created last, so it starts focused and on top; clicking A's exposed area raises it
 * (the kernel window manager does the raise/focus). ESC in the focused window quits.
 *
 * Compiled /MT linking the SDK user32.lib/gdi32.lib; bound to OUR DLLs at runtime.
 */
#include <windows.h>

static HWND g_a, g_b;

static void fill(HDC dc, int l, int t, int r, int b, COLORREF c) {
    RECT rc;
    HBRUSH br = CreateSolidBrush(c);
    rc.left = l;
    rc.top = t;
    rc.right = r;
    rc.bottom = b;
    FillRect(dc, &rc, br);
    DeleteObject(br);
}

static LRESULT CALLBACK WndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        if (h == g_a) {
            fill(dc, 0, 0, 240, 160, RGB(200, 60, 60));
            fill(dc, 10, 10, 110, 30, RGB(255, 255, 255));
        } else {
            fill(dc, 0, 0, 240, 160, RGB(60, 160, 80));
            fill(dc, 10, 10, 110, 30, RGB(255, 255, 0));
        }
        EndPaint(h, &ps);
        return 0;
    }
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

    wc.lpfnWndProc = WndProc;
    wc.hInstance = GetModuleHandleA(NULL);
    wc.hCursor = LoadCursorA(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "TukOSDesk";
    if (!RegisterClassA(&wc)) return 1;

    g_a = CreateWindowExA(0, "TukOSDesk", "Janela A", WS_OVERLAPPEDWINDOW,
                          60, 50, 240, 160, NULL, NULL, wc.hInstance, NULL);
    if (!g_a) return 2;
    g_b = CreateWindowExA(0, "TukOSDesk", "Janela B", WS_OVERLAPPEDWINDOW,
                          180, 120, 240, 160, NULL, NULL, wc.hInstance, NULL);
    if (!g_b) return 3;

    ShowWindow(g_a, SW_SHOW);
    ShowWindow(g_b, SW_SHOW);

    while (GetMessageA(&msg, NULL, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }
    return (int)msg.wParam;
}
