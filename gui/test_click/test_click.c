/*
 * TukOS Plano 23 / Task 2 mouse GUI autotest program.
 *
 * CONSOLE subsystem, `int main` (same /MT path as test_type). One window; WM_PAINT
 * fills (0,0)-(320,200) with RGB(32,32,48). WM_LBUTTONDOWN draws an 8x8 square of
 * RGB(255,0,0) with its top-left at the click (x = LOWORD(lParam), y = HIWORD(lParam)).
 * ESC (WM_KEYDOWN VK_ESCAPE) quits.
 *
 * Compiled /MT linking the SDK user32.lib/gdi32.lib; bound to OUR DLLs at runtime.
 */
#include <windows.h>

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
    case WM_LBUTTONDOWN: {
        int x = (int)LOWORD(l);
        int y = (int)HIWORD(l);
        HDC dc = GetDC(h);
        RECT sq;
        HBRUSH rb = CreateSolidBrush(RGB(255, 0, 0));
        sq.left = x;
        sq.top = y;
        sq.right = x + 8;
        sq.bottom = y + 8;
        FillRect(dc, &sq, rb);
        DeleteObject(rb);
        ReleaseDC(h, dc);
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
    HWND hwnd;

    wc.lpfnWndProc = WndProc;
    wc.hInstance = GetModuleHandleA(NULL);
    wc.hCursor = LoadCursorA(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "TukOSTestClick";
    if (!RegisterClassA(&wc)) return 1;

    hwnd = CreateWindowExA(0, "TukOSTestClick", "TukOS Click", WS_OVERLAPPEDWINDOW,
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
