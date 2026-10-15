#include <windows.h>
#include <stdio.h>
int main(void) {
    STARTUPINFOA si = { sizeof si };
    PROCESS_INFORMATION pi;
    DWORD code = 0;
    if (!CreateProcessA("child.exe", NULL, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
        printf("CreateProcess falhou\n");
        return 1;
    }
    WaitForSingleObject(pi.hProcess, INFINITE);
    GetExitCodeProcess(pi.hProcess, &code);
    printf("filho saiu com %u\n", code);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return 0;
}
