#!/usr/bin/env bash
# Проверка честной очереди scripts/build-queue на подставной «сборке».
#
# Скрипт копируется во временное дерево, cargo подменяется скриптом со sleep,
# systemd-run — заглушкой, которая отказывает (настоящий срез не трогаем).
# Проверяется: порядок прихода при одном и двух местах, умерший держатель
# места, умерший ждущий, Ctrl+C и timeout у ждущего, чужой flock на месте 0
# (~/.local/bin/heavy-run). Запуск: scripts/tests/build-queue-fifo.sh

set -uo pipefail
set -m    # у фоновых задач свои группы — SIGINT доходит, как Ctrl+C в терминале

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/build-queue"
T="$(mktemp -d)"
LOG="$T/log"
pids=()
cleanup() {
    for p in "${pids[@]}"; do kill -9 -- "-$p" 2>/dev/null; done
    rm -rf "$T"
}
trap cleanup EXIT

mkdir -p "$T/scripts" "$T/rust" "$T/bin"
cp "$SRC" "$T/scripts/build-queue"
cat >"$T/bin/systemd-run" <<'EOF'
#!/bin/sh
exit 1
EOF
# cargo <имя> run <секунды> | cargo <имя> hang
cat >"$T/bin/cargo" <<EOF
#!/usr/bin/env bash
echo "start \$1 \$(date +%s.%N)" >>"$LOG"
if [ "\$2" = hang ]; then exec sleep 300; fi
sleep "\$3"
echo "end \$1 \$(date +%s.%N)" >>"$LOG"
EOF
chmod +x "$T/bin/"*
export PATH="$T/bin:$PATH"
unset BUILD_QUEUE_TARGET

fails=0
ok()   { echo "  ок: $*"; }
fail() { echo "  ОШИБКА: $*"; fails=$((fails + 1)); }

# bq <места> <имя> <режим> [секунды] — запустить в фоне, PID в $!
bq() {
    BUILD_QUEUE_SLOTS="$1" "$T/scripts/build-queue" "$2" "$3" "${4:-0}" 2>"$T/err.$2" &
    pids+=("$!")
}
tickets() { local n=0 f; for f in "$T/.build-queue.d"/t*; do [ -e "$f" ] && n=$((n + 1)); done; echo "$n"; }
wait_for() {    # wait_for <секунды> <условие…>
    local deadline=$(( $(date +%s) + $1 )); shift
    until "$@"; do
        [ "$(date +%s)" -lt "$deadline" ] || return 1
        sleep 0.05
    done
}
tickets_eq() { [ "$(tickets)" -eq "$1" ]; }
started() { grep -q "^start $1 " "$LOG" 2>/dev/null; }
order() { awk '$1 == "start" { printf "%s ", $2 }' "$LOG"; }
max_parallel() {
    awk '$1 == "start" { n++; if (n > m) m = n } $1 == "end" { n-- } END { print m + 0 }' "$LOG"
}
reset() {
    for p in "${pids[@]}"; do kill -9 -- "-$p" 2>/dev/null; done
    wait 2>/dev/null
    pids=()
    rm -rf "$T/.build-queue.d" "$T"/err.* "$LOG"
}

echo "1. Одно место: порядок прихода"
bq 1 A run 1.5; wait_for 5 started A
for n in B C D E; do
    k=$(tickets)
    bq 1 "$n" run 0.6
    wait_for 5 tickets_eq $((k + 1)) || fail "$n не встал в очередь"
done
wait
[ "$(order)" = "A B C D E " ] && ok "порядок $(order)" || fail "порядок $(order)"
[ "$(max_parallel)" -eq 1 ] && ok "одновременно не больше 1" || fail "одновременно $(max_parallel)"
grep -q "в очереди: 4 из 4" "$T/err.E" && ok "E видел «в очереди: 4 из 4»" || fail "нет позиции у E: $(cat "$T/err.E")"
grep -qE "^build-queue: в очереди: [123] из [1-4]$" "$T/err.E" && ok "E видел, как продвигается очередь" || fail "E не видел продвижения"
grep -q "все 1 места заняты, жду…" "$T/err.B" && grep -q "место 0 освободилось" "$T/err.B" \
    && ok "прежние сообщения на месте" || fail "сообщения B: $(cat "$T/err.B")"
[ "$(tickets)" -eq 0 ] && ok "очередь пуста" || fail "остались билеты: $(tickets)"
reset

echo "2. Два места: первые два по порядку"
bq 2 A run 2; bq 2 B run 2
wait_for 5 started A; wait_for 5 started B
for n in C D E F; do
    k=$(tickets)
    bq 2 "$n" run 1
    wait_for 5 tickets_eq $((k + 1)) || fail "$n не встал в очередь"
done
wait
o="$(order)"; o="${o#A }"; o="${o#B }"; o="${o#A }"
[ "$o" = "C D E F " ] && ok "порядок после A и B: $o" || fail "порядок $(order)"
[ "$(max_parallel)" -eq 2 ] && ok "одновременно ровно 2" || fail "одновременно $(max_parallel)"
reset

echo "3. Держатель места умер (kill -9)"
bq 1 A hang; a=$!; wait_for 5 started A
bq 1 B run 0.2; wait_for 5 tickets_eq 1
sleep 0.5
started B && fail "B начал раньше смерти A"
kill -9 "$a"
wait_for 4 started B && ok "B получил место после смерти A" || fail "B не получил место"
wait "$a"; [ $? -eq 137 ] && ok "код A 137" || fail "код A не 137"
reset

echo "4. Ждущий умер (kill -9)"
bq 1 A run 1.5; wait_for 5 started A
bq 1 B run 0.2; b=$!; wait_for 5 tickets_eq 1
bq 1 C run 0.2; wait_for 5 tickets_eq 2
kill -9 "$b"
wait_for 5 started C && ok "C прошёл мимо умершего B" || fail "C завис"
started B && fail "умерший B всё же начал"
wait_for 3 tickets_eq 0 && ok "билет B убран" || fail "остались билеты: $(tickets)"
reset

echo "5. Ctrl+C у ждущего"
bq 1 A run 1.5; wait_for 5 started A
bq 1 B run 0.2; b=$!; wait_for 5 tickets_eq 1
bq 1 C run 0.2; wait_for 5 tickets_eq 2
kill -INT -- "-$b"
wait "$b"; rc=$?
[ "$rc" -eq 130 ] && ok "код B 130" || fail "код B $rc"
tickets_eq 1 && ok "билет B убран сразу" || fail "билетов $(tickets)"
wait_for 5 started C && ok "C получил место" || fail "C завис"
started B && fail "B после Ctrl+C всё же начал"
reset

echo "6. timeout снаружи у ждущего"
bq 1 A run 2; wait_for 5 started A
BUILD_QUEUE_SLOTS=1 timeout 1 "$T/scripts/build-queue" B run 0.2 2>/dev/null; rc=$?
[ "$rc" -eq 124 ] && ok "код timeout 124" || fail "код $rc"
tickets_eq 0 && ok "билет убран" || fail "билетов $(tickets)"
wait
started B && fail "B после timeout всё же начал"
reset

echo "7. Чужой flock на месте 0 (как heavy-run)"
flock "$T/.build-queue.lock" sleep 1.5 & pids+=("$!")
sleep 0.2
t0=$(date +%s.%N)
bq 1 A run 0.1; wait_for 5 started A
t1=$(awk '$1 == "start" { print $3 }' "$LOG")
awk -v a="$t0" -v b="$t1" 'BEGIN { exit !(b - a > 1.0) }' && ok "A ждал чужой замок" || fail "A не ждал"
reset

echo "8. Без аргументов — справка и код 2"
out=$("$T/scripts/build-queue" 2>&1); rc=$?
[ "$rc" -eq 2 ] && grep -q "Очередь сборок" <<<"$out" && ok "справка, код 2" || fail "код $rc"

if [ "$fails" -eq 0 ]; then echo "ВСЁ ПРОШЛО"; else echo "ОШИБОК: $fails"; exit 1; fi
