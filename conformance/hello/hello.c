/* Freestanding console program for the TukOS conformance harness.
 * Imports ONLY ntdll (our own import library). No CRT, no kernel32.
 * The stdout handle comes from PEB->ProcessParameters->StandardOutput
 * (public winternl.h / RTL_USER_PROCESS_PARAMETERS layout), which both the
 * real Windows loader and the TukOS kernel fill in. */

typedef long NTSTATUS;
typedef void *HANDLE;

typedef struct IO_STATUS_BLOCK {
    union {
        NTSTATUS Status;
        void *Pointer;
    };
    unsigned __int64 Information;
} IO_STATUS_BLOCK;

__declspec(dllimport) NTSTATUS NtWriteFile(
    HANDLE FileHandle, HANDLE Event, void *ApcRoutine, void *ApcContext,
    IO_STATUS_BLOCK *IoStatusBlock, void *Buffer, unsigned long Length,
    __int64 *ByteOffset, unsigned long *Key);

__declspec(dllimport) NTSTATUS NtTerminateProcess(HANDLE ProcessHandle,
                                                  NTSTATUS ExitStatus);

unsigned __int64 __readgsqword(unsigned long Offset);
#pragma intrinsic(__readgsqword)

static const char msg[] = "Hello from MSVC on TukOS\n";

void tukos_main(void)
{
    unsigned char *peb = (unsigned char *)__readgsqword(0x60);          /* TEB.Peb */
    unsigned char *params = *(unsigned char **)(peb + 0x20);            /* PEB.ProcessParameters */
    HANDLE out = *(HANDLE *)(params + 0x28);                            /* .StandardOutput */
    IO_STATUS_BLOCK iosb;

    iosb.Status = 0;
    iosb.Information = 0;
    NtWriteFile(out, 0, 0, 0, &iosb, (void *)msg, sizeof(msg) - 1, 0, 0);
    NtTerminateProcess((HANDLE)-1, 0);
    for (;;) {
    }
}
