/* Survey de imports: programa /MT que le um arquivo fixo e imprime. */
#include <stdio.h>

int main(void)
{
    char buf[256];
    size_t n;
    FILE *f = fopen("C:\\cat.txt", "rb");
    if (!f) {
        puts("fileio: open failed");
        return 1;
    }
    while ((n = fread(buf, 1, sizeof buf, f)) > 0) {
        fwrite(buf, 1, n, stdout);
    }
    fclose(f);
    return 0;
}
