/*
 * TukOS Plano 22 / Task 2 keyboard GUI autotest program.
 *
 * CONSOLE subsystem, `int main` (same /MT path as test_win). Registers a class,
 * creates one window and runs GetMessage/TranslateMessage/DispatchMessage. On WM_PAINT
 * it fills (0,0)-(320,200) with RGB(32,32,48). On WM_CHAR: '\r' -> PostQuitMessage(0);
 * any other char is drawn with TextOutA at x = 20 + 16*n, y = 25 (n = chars so far),
 * white RGB(255,255,255) on RGB(32,32,48), opaque.
 *
 * Compiled /MT linking the SDK user32.lib/gdi32.lib; bound to OUR DLLs at runtime.
 */
#include <windows.h>

static int g_n = 0;

static LRESULT CALLBACK WndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        RECT bg = {0, 0, 320, 200};
        HBRUSH bb = CreateSolidBrush(RGB(32, 32, 48));
        FillRect(dc, &bg, bb);
        DeleteObject(bb);
        EndPaint(h, &ps);
        return 0;
    }
    case WM_KEYDOWN:
        if (w == VK_ESCAPE) PostQuitMessage(0);
        return 0;
    case WM_CHAR: {
        if (w == '\r') {
            PostQuitMessage(0);
        } else if (w == 0x08) {
            if (g_n > 0) {
                char sp = ' ';
                HDC dc = GetDC(h);
                g_n--;
                SetBkMode(dc, OPAQUE);
                SetBkColor(dc, RGB(32, 32, 48));
                SetTextColor(dc, RGB(255, 255, 255));
                TextOutA(dc, 20 + 16 * g_n, 25, &sp, 1);
                ReleaseDC(h, dc);
            }
        } else {
            char c = (char)w;
            HDC dc = GetDC(h);
            SetBkMode(dc, OPAQUE);
            SetBkColor(dc, RGB(32, 32, 48));
            SetTextColor(dc, RGB(255, 255, 255));
            TextOutA(dc, 20 + 16 * g_n, 25, &c, 1);
            ReleaseDC(h, dc);
            g_n++;
        }
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
    wc.lpszClassName = "TukOSTestType";
    if (!RegisterClassA(&wc)) return 1;

    hwnd = CreateWindowExA(0, "TukOSTestType", "TukOS Type", WS_OVERLAPPEDWINDOW,
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
