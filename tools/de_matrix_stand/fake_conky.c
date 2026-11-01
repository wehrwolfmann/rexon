/* Подставной conky для стенда кнопок разделов (inner_sections.sh): живёт
 * минуту под именем `conky` с тем же argv (`-d` — уходит в фон, как
 * настоящий), чтобы «Запустить»/«Остановить» и чтение «что работает» из
 * /proc проверялись без X и без настоящего виджета. */
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
    for (int i = 1; i < argc; i++)
        if (strcmp(argv[i], "-d") == 0 && fork() > 0) return 0;
    sleep(60);
    return 0;
}
