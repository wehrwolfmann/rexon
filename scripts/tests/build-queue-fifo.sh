#!/usr/bin/env bash
# Check of the fair queue of scripts/build-queue on a fake "build".
#
# The script is copied into a temporary tree, cargo is replaced by a script with sleep,
# systemd-run — by a stub that refuses (the real slice is not touched).
# Checked: arrival order with one and two slots, a dead slot
# holder, a dead waiter, Ctrl+C and timeout of a waiter, a foreign flock on slot 0
# (~/.local/bin/heavy-run). Run: scripts/tests/build-queue-fifo.sh

set -uo pipefail
set -m    # background jobs get their own groups — SIGINT arrives as Ctrl+C in a terminal

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
# cargo <name> run <seconds> | cargo <name> hang
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
ok()   { echo "  ok: $*"; }
fail() { echo "  FAIL: $*"; fails=$((fails + 1)); }

# bq <slots> <name> <mode> [seconds] — start in the background, PID in $!
bq() {
    BUILD_QUEUE_SLOTS="$1" "$T/scripts/build-queue" "$2" "$3" "${4:-0}" 2>"$T/err.$2" &
    pids+=("$!")
}
tickets() { local n=0 f; for f in "$T/.build-queue.d"/t*; do [ -e "$f" ] && n=$((n + 1)); done; echo "$n"; }
wait_for() {    # wait_for <seconds> <condition…>
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

echo "1. One slot: arrival order"
bq 1 A run 1.5; wait_for 5 started A
for n in B C D E; do
    k=$(tickets)
    bq 1 "$n" run 0.6
    wait_for 5 tickets_eq $((k + 1)) || fail "$n did not get in the queue"
done
wait
[ "$(order)" = "A B C D E " ] && ok "order $(order)" || fail "order $(order)"
[ "$(max_parallel)" -eq 1 ] && ok "no more than 1 at once" || fail "at once $(max_parallel)"
grep -q "in queue: 4 of 4" "$T/err.E" && ok "E saw «in queue: 4 of 4»" || fail "no position for E: $(cat "$T/err.E")"
grep -qE "^build-queue: in queue: [123] of [1-4]$" "$T/err.E" && ok "E saw the queue move" || fail "E did not see progress"
grep -q "all 1 slots are busy, waiting…" "$T/err.B" && grep -q "slot 0 is free" "$T/err.B" \
    && ok "the usual messages are in place" || fail "messages of B: $(cat "$T/err.B")"
[ "$(tickets)" -eq 0 ] && ok "the queue is empty" || fail "tickets left: $(tickets)"
reset

echo "2. Two slots: the first two in order"
bq 2 A run 2; bq 2 B run 2
wait_for 5 started A; wait_for 5 started B
for n in C D E F; do
    k=$(tickets)
    bq 2 "$n" run 1
    wait_for 5 tickets_eq $((k + 1)) || fail "$n did not get in the queue"
done
wait
o="$(order)"; o="${o#A }"; o="${o#B }"; o="${o#A }"
[ "$o" = "C D E F " ] && ok "order after A and B: $o" || fail "order $(order)"
[ "$(max_parallel)" -eq 2 ] && ok "exactly 2 at once" || fail "at once $(max_parallel)"
reset

echo "3. The slot holder died (kill -9)"
bq 1 A hang; a=$!; wait_for 5 started A
bq 1 B run 0.2; wait_for 5 tickets_eq 1
sleep 0.5
started B && fail "B started before A died"
kill -9 "$a"
wait_for 4 started B && ok "B got the slot after A died" || fail "B did not get the slot"
wait "$a"; [ $? -eq 137 ] && ok "exit code of A is 137" || fail "exit code of A is not 137"
reset

echo "4. A waiter died (kill -9)"
bq 1 A run 1.5; wait_for 5 started A
bq 1 B run 0.2; b=$!; wait_for 5 tickets_eq 1
bq 1 C run 0.2; wait_for 5 tickets_eq 2
kill -9 "$b"
wait_for 5 started C && ok "C went past the dead B" || fail "C hung"
started B && fail "the dead B started after all"
wait_for 3 tickets_eq 0 && ok "ticket of B removed" || fail "tickets left: $(tickets)"
reset

echo "5. Ctrl+C on a waiter"
bq 1 A run 1.5; wait_for 5 started A
bq 1 B run 0.2; b=$!; wait_for 5 tickets_eq 1
bq 1 C run 0.2; wait_for 5 tickets_eq 2
kill -INT -- "-$b"
wait "$b"; rc=$?
[ "$rc" -eq 130 ] && ok "exit code of B is 130" || fail "exit code of B is $rc"
tickets_eq 1 && ok "ticket of B removed at once" || fail "tickets: $(tickets)"
wait_for 5 started C && ok "C got the slot" || fail "C hung"
started B && fail "B started after Ctrl+C after all"
reset

echo "6. timeout from outside on a waiter"
bq 1 A run 2; wait_for 5 started A
BUILD_QUEUE_SLOTS=1 timeout 1 "$T/scripts/build-queue" B run 0.2 2>/dev/null; rc=$?
[ "$rc" -eq 124 ] && ok "timeout exit code 124" || fail "exit code $rc"
tickets_eq 0 && ok "ticket removed" || fail "tickets: $(tickets)"
wait
started B && fail "B started after timeout after all"
reset

echo "7. A foreign flock on slot 0 (like heavy-run)"
flock "$T/.build-queue.lock" sleep 1.5 & pids+=("$!")
sleep 0.2
t0=$(date +%s.%N)
bq 1 A run 0.1; wait_for 5 started A
t1=$(awk '$1 == "start" { print $3 }' "$LOG")
awk -v a="$t0" -v b="$t1" 'BEGIN { exit !(b - a > 1.0) }' && ok "A waited for the foreign lock" || fail "A did not wait"
reset

echo "8. No arguments — help and exit code 2"
out=$("$T/scripts/build-queue" 2>&1); rc=$?
[ "$rc" -eq 2 ] && grep -q "Build queue" <<<"$out" && ok "help, exit code 2" || fail "exit code $rc"

if [ "$fails" -eq 0 ]; then echo "ALL PASSED"; else echo "FAILURES: $fails"; exit 1; fi
