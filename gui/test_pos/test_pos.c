/*
 * TukOS Plano 25 / Task 2 GUI autotest program: a POSITIONED, smaller-than-screen
 * window. Creates a window at screen (120,90) sized 240x160, and on WM_PAINT fills the
 * client rect (10,10)-(60,60) with RGB(255,0,0) (framebuffer 0x00FF0000), then quits.
 * The kernel asserts the red rect lands at SCREEN (130..180, 100..150) and the desktop
 * background shows outside the window.
 */
#include <windows.h>

static LRESULT CALLBACK WndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        RECT r = {10, 10, 60, 60};
        HBRUSH b = CreateSolidBrush(RGB(255, 0, 0));
        FillRect(dc, &r, b);
        DeleteObject(b);
        EndPaint(h, &ps);
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
    wc.lpszClassName = "TukOSTestPos";
    if (!RegisterClassA(&wc)) return 1;

    hwnd = CreateWindowExA(0, "TukOSTestPos", "TukOS Pos", WS_OVERLAPPEDWINDOW,
                           120, 90, 240, 160,
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
