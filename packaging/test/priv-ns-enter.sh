#!/bin/sh
# Стенд уровня 3 «один пароль на запуск» (см. docs/testing.md): настоящий
# rexon-priv-helper от «root» в пространстве имён пользователя, без root на
# хосте. Запускается ТОЛЬКО так (это делает tests/priv_userns.rs):
#
#   PKEXEC_UID=0 unshare -r --mount priv-ns-enter.sh <стенд> <помощник> [аргументы…]
#
# `unshare -r` даёт uid 0 внутри (снаружи это тот же пользователь), `--mount`
# — свои точки монтирования: подмены ниже видит только помощник и его
# потомки, на хосте не меняется ничего. Каталог стенда <стенд> готовит тест:
#
#   realbin/  — пустой: сюда встаёт настоящий /usr/bin (для шебангов подмен)
#   fakebin/  — встаёт на /usr/bin: подменные pacman, loginctl, systemctl
#   etc/      — встаёт на /etc: os-release, passwd
#   share/    — встаёт на /usr/share: сюда пишет раскладка тем
#   local/    — (если есть) встаёт на /usr/local: шрифты «для всех»
#   vartmp/   — встаёт на /var/tmp: рабочие каталоги помощника
#   nojournal/ — встаёт на /run/systemd/journal: журнал помощника уходит
#                в stderr (в файл стенда), а не в journald машины
#
# Все `mount` — до `exec`: pid не меняется (unshare без --fork, оболочка
# заменяется помощником), родитель помощника — окно, как после pkexec;
# сокет окна остаётся на fd 0/1.
set -eu
T=$1
H=$2
shift 2
mount --bind /usr/bin "$T/realbin"
mount --bind "$T/etc" /etc
mount --bind "$T/share" /usr/share
if [ -d "$T/local" ]; then
    mount --bind "$T/local" /usr/local
fi
mount --bind "$T/vartmp" /var/tmp
if [ -d /run/systemd/journal ]; then
    mount --bind "$T/nojournal" /run/systemd/journal
fi
# Последним: после него в /usr/bin только подмены.
"$T/realbin/mount" --bind "$T/fakebin" /usr/bin
exec "$H" "$@"
