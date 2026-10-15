#include <stdio.h>
/* Prints argc and every argv entry: "<argc> <argv0> <argv1> ...\n". */
int main(int argc, char **argv) {
    int i;
    printf("%d", argc);
    for (i = 0; i < argc; i++) printf(" %s", argv[i]);
    printf("\n");
    return 0;
}
