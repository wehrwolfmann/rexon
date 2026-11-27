#!/usr/bin/env bash
# Check of the fair queue of scripts/build-queue on a fake "build".
#
# The script is copied into a temporary tree, cargo is replaced by a script with sleep,
# systemd-run — by a stub that refuses (the real slice is not touched).
# Checked: arrival order with one and two slots, a dead slot
# holder, a dead waiter, Ctrl+C and timeout of a waiter, a foreign flock on slot 0
# (~/.local/bin/heavy-run); the token bank: the pool reaches cargo, three makes
# share it, kill -9 and the refill, PSI pressure (fake PSI files), makepkg,
# the keeper retiring and coming back. Run: scripts/tests/build-queue-fifo.sh

set -uo pipefail
set -m    # background jobs get their own groups — SIGINT arrives as Ctrl+C in a terminal

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/build-queue"
T="$(mktemp -d)"
LOG="$T/log"
pids=()
cleanup() {
    for p in "${pids[@]}"; do kill -9 -- "-$p" 2>/dev/null; done
    pkill -f -- "$T/scripts/build-queue --bank" 2>/dev/null
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
if [ "\$2" = env ]; then
    echo "MAKEFLAGS=\${MAKEFLAGS:-} CARGO_MAKEFLAGS=\${CARGO_MAKEFLAGS:-} CARGO_BUILD_JOBS=\${CARGO_BUILD_JOBS:-unset}" >"$T/env.\$1"
fi
if [ "\$2" = hang ]; then exec sleep 300; fi
sleep "\$3"
echo "end \$1 \$(date +%s.%N)" >>"$LOG"
EOF
chmod +x "$T/bin/"*
export PATH="$T/bin:$PATH"
unset BUILD_QUEUE_TARGET
# The token bank of this test queue lives in the temporary tree, never next to the
# live pool; its keeper retires quickly once the test is over.
export BUILD_QUEUE_POOL_DIR="$T/pool" BUILD_QUEUE_BANK_IDLE=2 BUILD_QUEUE_PSI_DIR="$T/psi"
mkdir -p "$T/psi"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' >"$T/psi/io"
cp "$T/psi/io" "$T/psi/memory"

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

# --- Token bank ------------------------------------------------------------
psi() { printf 'some avg10=%s.00 avg60=0.00 avg300=0.00 total=0\n' "$2" >"$T/psi/$1"; }
status() { cat "$T/pool/state" 2>/dev/null; }
held_is() { status | grep -q " held=$1 "; }
keepers() { pgrep -f -- "$T/scripts/build-queue --bank" | wc -l; }
keepers_eq() { [ "$(keepers)" -eq "$1" ]; }
# A make project: <n> jobs of <seconds>, each logging start/end like the fake cargo.
mkproj() {
    local i
    mkdir -p "$T/mk"
    { printf 'J := $(addprefix j,'; for ((i = 1; i <= $1; i++)); do printf '%s ' "$i"; done; printf ')\nall: $(J)\n'
      printf 'j%%:\n\t@echo "start $@ $$(date +%%s.%%N)" >>%s; sleep %s; echo "end $@ $$(date +%%s.%%N)" >>%s\n' "$LOG" "$2" "$LOG"
    } >"$T/mk/Makefile"
}
# bqx <slots> <command…> — a run through --exec in the background
bqx() { local n="$1"; shift; BUILD_QUEUE_SLOTS="$n" "$T/scripts/build-queue" --exec "$@" 2>>"$T/err.x" & pids+=("$!"); }
# The keeper of sections 1-8 (default size) retires first; the next one is small
# and stays for the pool sections.
wait_for 10 keepers_eq 0 || fail "the first keeper did not retire"
export BUILD_QUEUE_POOL_TOKENS=2 BUILD_QUEUE_BANK_SLOTS=3 BUILD_QUEUE_BANK_IDLE=60
psi io 0; psi memory 0

echo "9. The pool reaches cargo; the fixed -j goes away"
BUILD_QUEUE_SLOTS=2 "$T/scripts/build-queue" P env 0 2>/dev/null
grep -q "MAKEFLAGS=--jobserver-auth=fifo:$T/pool/jobserver.fifo " "$T/env.P" && ok "MAKEFLAGS points at the pool" || fail "env: $(cat "$T/env.P")"
grep -q "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$T/pool/jobserver.fifo " "$T/env.P" && ok "CARGO_MAKEFLAGS too" || fail "env: $(cat "$T/env.P")"
grep -q "CARGO_BUILD_JOBS=unset" "$T/env.P" && ok "no CARGO_BUILD_JOBS with the pool" || fail "env: $(cat "$T/env.P")"
[ "$(stat -c %a "$T/pool/jobserver.fifo")" = 600 ] && ok "pool is 0600" || fail "mode $(stat -c %a "$T/pool/jobserver.fifo")"
BUILD_QUEUE_POOL=0 BUILD_QUEUE_SLOTS=2 "$T/scripts/build-queue" Q env 0 2>/dev/null
grep -q "CARGO_BUILD_JOBS=2" "$T/env.Q" && ! grep -q fifo "$T/env.Q" && ok "BUILD_QUEUE_POOL=0: the old -j2" || fail "env: $(cat "$T/env.Q")"
reset

echo "10. Three makes with -j8 share one pool"
mkproj 8 0.6
for n in 1 2 3; do mkdir -p "$T/m$n"; cp "$T/mk/Makefile" "$T/m$n/"; bqx 3 make -s -j8 -C "$T/m$n"; done
wait
[ "$(grep -c '^start' "$LOG")" -eq 24 ] && ok "24 jobs done" || fail "jobs: $(grep -c '^start' "$LOG")"
[ "$(max_parallel)" -le 5 ] && ok "at once $(max_parallel) <= 2 tokens + 3 implicit" || fail "at once $(max_parallel)"
keepers_eq 1 && ok "one keeper" || fail "keepers: $(keepers)"
[ "$(grep -c 'started: 2 tokens' "$T/pool/bank.log")" -eq 1 ] && ok "pool seeded once" || fail "bank.log: $(cat "$T/pool/bank.log")"
reset

echo "11. kill -9 loses tokens, the idle keeper restores them"
mkproj 3 3
bqx 1 make -s -C "$T/mk"; wait_for 5 started j1
sleep 0.5
mpid=$(ps -eo pid,comm,args | awk -v mk="$T/mk" '$2 == "make" && index($0, mk) { print $1; exit }')
kill -9 "$mpid"
wait_for 12 grep -q "found 0 tokens, expected 2" "$T/pool/bank.log" && ok "loss noticed and restored" || fail "bank.log: $(cat "$T/pool/bank.log")"
reset
mkproj 6 1
bqx 1 make -s -C "$T/mk"; wait
[ "$(max_parallel)" -eq 3 ] && ok "full pool again: 3 at once" || fail "at once $(max_parallel)"
reset

echo "12. Pressure: the keeper holds tokens back and returns them"
psi io 90
wait_for 6 held_is 2 && ok "I/O pressure: 2 of 2 held" || fail "state: $(status)"
mkproj 6 0.5
bqx 1 make -s -C "$T/mk"; wait
[ "$(max_parallel)" -eq 1 ] && ok "under pressure 1 at once" || fail "at once $(max_parallel)"
psi io 0
wait_for 6 held_is 0 && ok "pressure gone: tokens back" || fail "state: $(status)"
psi memory 25
wait_for 6 held_is 2 && ok "memory pressure holds them too" || fail "state: $(status)"
psi memory 0
wait_for 6 held_is 0 || fail "state: $(status)"
reset

echo "13. makepkg gets the pool through MAKEPKG_CONF"
printf 'MAKEFLAGS="-j16"\nFOO=bar\n' >"$T/makepkg.conf"
MAKEPKG_CONF="$T/makepkg.conf" BUILD_QUEUE_SLOTS=1 "$T/scripts/build-queue" --exec \
    bash -c 'source "$MAKEPKG_CONF"; echo "$FOO $MAKEFLAGS"' >"$T/out.mp" 2>/dev/null
[ "$(cat "$T/out.mp")" = "bar --jobserver-auth=fifo:$T/pool/jobserver.fifo" ] && ok "config kept, MAKEFLAGS = pool" || fail "got: $(cat "$T/out.mp")"
reset

echo "14. The keeper retires when idle and comes back"
pkill -f -- "$T/scripts/build-queue --bank"; wait_for 5 keepers_eq 0
export BUILD_QUEUE_BANK_IDLE=2
bqx 1 true; wait
wait_for 8 keepers_eq 0 && ok "keeper gone after idle" || fail "keepers: $(keepers)"
bqx 1 true; wait
keepers_eq 1 && ok "started again on demand" || fail "keepers: $(keepers); $(tail -3 "$T/pool/bank.log"); $(cat "$T/err.x")"
unset BUILD_QUEUE_POOL_TOKENS BUILD_QUEUE_BANK_SLOTS

if [ "$fails" -eq 0 ]; then echo "ALL PASSED"; else echo "FAILURES: $fails"; exit 1; fi
