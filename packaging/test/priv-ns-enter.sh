#!/bin/sh
# Level 3 test stand for "one password per launch" (see docs/testing.md): the
# real rexon-priv-helper as "root" in a user namespace, without root on the
# host. It is run ONLY like this (tests/priv_userns.rs does it):
#
#   PKEXEC_UID=0 unshare -r --mount priv-ns-enter.sh <stand> <helper> [args…]
#
# `unshare -r` gives uid 0 inside (outside it is the same user), `--mount`
# gives its own mount points: the substitutions below are seen only by the
# helper and its children, nothing changes on the host. The <stand>
# directory is prepared by the test:
#
#   realbin/  — empty: the real /usr/bin is mounted here (for the fakes' shebangs)
#   fakebin/  — mounted over /usr/bin: fake pacman, loginctl, systemctl
#   etc/      — mounted over /etc: os-release, passwd
#   share/    — mounted over /usr/share: the theme layout writes here
#   local/    — (if present) mounted over /usr/local: fonts "for all users"
#   vartmp/   — mounted over /var/tmp: the helper's working directories
#   nojournal/ — mounted over /run/systemd/journal: the helper's log goes
#                to stderr (into the stand's file), not to the machine's journald
#
# All `mount`s happen before `exec`: the pid does not change (unshare without
# --fork, the shell is replaced by the helper), the helper's parent is the
# window, as after pkexec; the window's socket stays on fd 0/1.
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
# Last: after it /usr/bin contains only the fakes.
"$T/realbin/mount" --bind "$T/fakebin" /usr/bin
exec "$H" "$@"
