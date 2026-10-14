/* Freestanding console program for the TukOS conformance harness.
 * Imports ONLY kernel32 (our own import library); kernel32 itself imports ntdll.
 * No CRT. STD_OUTPUT_HANDLE = -11. */

__declspec(dllimport) void *__stdcall GetStdHandle(long nStdHandle);
__declspec(dllimport) int __stdcall WriteFile(void *h, const void *buf,
                                              unsigned long len,
                                              unsigned long *written,
                                              void *ovl);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int code);

static const char msg[] = "Hello from MSVC via kernel32\n";

void tukos_main(void)
{
    unsigned long n;
    WriteFile(GetStdHandle(-11), msg, sizeof(msg) - 1, &n, 0);
    ExitProcess(0);
}
