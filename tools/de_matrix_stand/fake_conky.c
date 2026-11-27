/* A stand-in conky for the section buttons stand (inner_sections.sh): lives
 * for a minute under the name `conky` with the same argv (`-d` — goes to the background, like
 * the real one), so that «Запустить»/«Остановить» and reading «what is running» from
 * /proc are checked without X and without a real widget. */
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
    for (int i = 1; i < argc; i++)
        if (strcmp(argv[i], "-d") == 0 && fork() > 0) return 0;
    sleep(60);
    return 0;
}
