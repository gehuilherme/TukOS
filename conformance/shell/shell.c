/* TukOS shell: reads a command line from the keyboard, runs "<cmd>.exe" from the
   initrd via CreateProcess, waits for it, and repeats. Builtins: help, exit (EOF also ends).
   Only the first token is used: arguments are NOT passed to the child yet. */
#include <windows.h>
#include <stdio.h>
#include <string.h>

#define CMD_MAX 200

/* First whitespace-delimited token of `line` into `cmd` (bounded, NUL-terminated). */
static void first_token(const char *line, char *cmd, size_t cap) {
    size_t n = 0;
    while (*line == ' ' || *line == '\t') line++;
    while (*line && *line != ' ' && *line != '\t' && *line != '\r' && *line != '\n') {
        if (n + 1 < cap) cmd[n++] = *line;
        line++;
    }
    cmd[n] = '\0';
}

int main(void) {
    char line[256];
    for (;;) {
        char cmd[CMD_MAX + 1];
        char path[CMD_MAX + 8];
        STARTUPINFOA si = { sizeof si };
        PROCESS_INFORMATION pi;

        printf("tukos> ");
        fflush(stdout);
        if (!fgets(line, sizeof line, stdin)) break; /* EOF */
        first_token(line, cmd, sizeof cmd);
        if (cmd[0] == '\0') continue;
        if (strcmp(cmd, "exit") == 0) break;
        if (strcmp(cmd, "help") == 0) {
            printf("comandos: hello echo help exit\n");
            continue;
        }

        _snprintf(path, sizeof path, "%s.exe", cmd);
        path[sizeof path - 1] = '\0';
        if (CreateProcessA(path, NULL, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
            WaitForSingleObject(pi.hProcess, INFINITE);
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
        } else {
            printf("comando nao encontrado: %s\n", cmd);
        }
    }
    return 0;
}
