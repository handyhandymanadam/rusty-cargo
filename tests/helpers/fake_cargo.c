/* records argv (including argv[0]) and the environment into the file named by $SEEN, exits with $FAKE_EXIT or 7 */
#include <stdio.h>
#include <stdlib.h>
extern char **environ;
int main(int argc, char **argv) {
    FILE *f = fopen(getenv("SEEN"), "w");
    for (int i = 0; i < argc; i++) fprintf(f, "%s%s\n", i ? "arg=" : "argv0=", argv[i]);
    for (char **e = environ; *e; e++) fprintf(f, "env=%s\n", *e);
    fclose(f);
    return getenv("FAKE_EXIT") ? atoi(getenv("FAKE_EXIT")) : 7;
}
