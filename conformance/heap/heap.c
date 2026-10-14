/* Freestanding console program for the TukOS conformance harness.
 * Exercises the process heap (GetProcessHeap/HeapAlloc/HeapFree) end-to-end,
 * then exits with code 7. Imports ONLY kernel32. No CRT. */

__declspec(dllimport) void *__stdcall GetProcessHeap(void);
__declspec(dllimport) void *__stdcall HeapAlloc(void *h, unsigned long flags, unsigned __int64 size);
__declspec(dllimport) int __stdcall HeapFree(void *h, unsigned long flags, void *p);
__declspec(dllimport) void *__stdcall GetStdHandle(long nStdHandle);
__declspec(dllimport) int __stdcall WriteFile(void *h, const void *buf,
                                              unsigned long len,
                                              unsigned long *written,
                                              void *ovl);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int code);

static const char msg[] = "Heap OK via kernel32\n"; /* 21 bytes */
#define N 21

void tukos_main(void)
{
    unsigned long n = 0;
    unsigned long i;
    void *h = GetProcessHeap();
    char *p = (char *)HeapAlloc(h, 0, N);
    if (!p)
        ExitProcess(1);
    for (i = 0; i < N; i++)
        p[i] = msg[i];
    WriteFile(GetStdHandle(-11), p, N, &n, 0);
    HeapFree(h, 0, p);
    ExitProcess(7);
}
