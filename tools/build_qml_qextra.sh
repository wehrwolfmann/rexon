#!/usr/bin/env bash
# Build of the QExtra QML plugin (hypengw/QExtra, MIT OR Apache-2.0) — the real
# `QE.AnimatedImage` and `QE.ImageCache` for the wallpaper tab: an ffmpeg decoder,
# a shared memory cap for frames, a shared tick for identical animations. The third
# in the `build_qml_*` family.
#
# There are no QExtra sources in the tree. The tree holds only our wrapper
# `tools/qextra_plugin/` (a shared plugin library, ~20 lines of glue) and
# `lito.lock` next to it: it pins the revision of QExtra and of all its
# dependencies — a slice of the `lito.lock` of the waywallen release the
# vendored markup came with. The build runs with `--locked`: a diverged lock is a
# refusal, not a silent build from the head.
#
# ⚠️ It is built OUTSIDE the tree, in a directory with a Latin-only path
# (`~/.cache/rexon-qextra-build`): lito crashes on non-Latin paths, and the tree
# may live in `~/Загрузки`. Do not build in `/tmp` — the build directory weighs
# ~740 MB, and `/tmp` often lives in memory.
#
# ⚠️ The plugin calls a private Qt API (`QRhi`) and fits only the Qt minor
# version it was built with. A `QT_VERSION` mark is written next to it;
# Rexon compares it with the process's Qt (`qml_modules::choose_qextra`) and on
# a mismatch takes the QML stand-in from `qml_ui/QExtra/`.
#
# Requirements: lito >= 0.8.7 (the LITO variable or PATH), clang >= 22, lld,
# cmake, ninja, qt6-base, qt6-declarative, qt6-grpc, ffmpeg, libva, curl,
# glslang, vulkan-headers, libpulse.
#
# Variables (optional): LITO — path to lito; QEXTRA_BUILD — the build
# directory; QEXTRA_JOBS — the number of threads (4).
# For a package build (it has no network and makepkg sets the flags):
#   QEXTRA_SOURCE_BUNDLE — the directory of a lito source bundle (inside `v1/git/…`
#     and `v1/registry/source/…`, the `lito lock export` layout): the build runs
#     with `--frozen` and never goes to the network;
#   QEXTRA_ENV_FLAGS=1 — add the environment's CFLAGS, CXXFLAGS and LDFLAGS
#     (`lito --use-env-flags`), including `-ffile-prefix-map`; the keys
#     held by the release profile (-O*, -g*, LTO, strip) and
#     _FORTIFY_SOURCE are cut by the script itself — flags of any makepkg.conf fit.
#
# Heavy: the first build takes ~2–3 minutes on 4 threads. On a busy machine —
#   systemd-run --user --scope -p MemoryMax=10G nice tools/build_qml_qextra.sh
set -euo pipefail

# 0.8.7: the wavsen 7564374 manifest (the waywallen 0.5.0 slice) writes the
# `condition` field on a dependency, and 0.8.5 rejects it with «unknown field: condition».
LITO_MIN="0.8.7"
DEST="${1:-$HOME/.local/share/rexon/qml_modules}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HERE/qextra_plugin"
WORK="${QEXTRA_BUILD:-$HOME/.cache/rexon-qextra-build}"
JOBS="${QEXTRA_JOBS:-4}"
LITO_BIN="${LITO:-lito}"

if LC_ALL=C grep -q '[^ -~]' <<<"$WORK"; then
    echo "каталог сборки должен быть путём из латиницы (lito на других падает): $WORK" >&2
    echo "задайте QEXTRA_BUILD" >&2
    exit 2
fi
if ! command -v "$LITO_BIN" >/dev/null 2>&1; then
    echo "lito не найден ($LITO_BIN) — нужен lito >= $LITO_MIN; путь задаётся переменной LITO" >&2
    exit 2
fi
LITO_VER="$("$LITO_BIN" --version 2>/dev/null | grep -oE '[0-9]+(\.[0-9]+)+' | head -1 || true)"
if [ -z "$LITO_VER" ]; then
    echo "не удалось узнать версию lito: $(command -v "$LITO_BIN")" >&2
    exit 2
fi
if [ "$(printf '%s\n%s\n' "$LITO_MIN" "$LITO_VER" | sort -V | head -1)" != "$LITO_MIN" ]; then
    echo "lito $LITO_VER не годится ($(command -v "$LITO_BIN")): нужен lito >= $LITO_MIN" >&2
    echo "(lito-qt 0.1.1 зовёт lito.version_matches, которого в прежних выпусках нет);" >&2
    echo "путь к годному задаётся переменной LITO" >&2
    exit 2
fi

PIN="$(grep -oE 'QExtra\.git#[0-9a-f]+' "$SRC/lito.lock" | cut -d'#' -f2)"
echo "== QExtra @${PIN:-?} (lito $LITO_VER, -j $JOBS) → $DEST =="

# The wrapper is copied into the build directory anew, the `build/` directory in it
# stays: a repeated build without changes takes seconds.
mkdir -p "$WORK/wrapper"
for f in lito.toml lito.lock build.lua plugin.hpp plugin.cpp plugin.cppm; do
    cmp -s "$SRC/$f" "$WORK/wrapper/$f" || cp "$SRC/$f" "$WORK/wrapper/$f"
done
LITO_GLOBAL=()

# Environment flags do not reach lito in full. What `--profile release`
# keeps for itself is cut: optimization (-O3), debug info (none),
# LTO (none), and at link time also strip (none). Any such key from the environment
# that diverges from the profile is rejected by lito BEFORE the build («compiler option '-O2'
# from CXXFLAGS sets optimization to '-O2', but the selected profile 'release'
# fixes it to '-O3'»): with Arch CFLAGS (-O2) the package did not build at all, and on
# CachyOS it passed only because -O3 is used there. Hardening keys
# (-fstack-clash-protection, -fcf-protection, -D_GLIBCXX_ASSERTIONS…) and the path
# map stay. `-Wl,-O1` is a linker key, not a profile one, and stays too.
#
# ⚠️ _FORTIFY_SOURCE is removed too: with it glibc declares open/openat
# as inline wrappers with internal linkage (bits/fcntl2.h), and a C++
# module cannot export such a name — rstd fails on `using ::openat`
# («with internal linkage cannot be exported»). The AUR package of lito
# itself does the same.
strip_profile_flags() {
    local -a in out=()
    local t
    read -r -a in <<<"$1"
    for t in "${in[@]}"; do
        case "$t" in
            -O*|-g*|-flto|-flto=*|-fno-lto) ;;
            -s|-Wl,--strip-all|-Wl,--strip-debug|-Wl,-s) ;;
            -D_FORTIFY_SOURCE|-D_FORTIFY_SOURCE=*|-Wp,-D_FORTIFY_SOURCE|-Wp,-D_FORTIFY_SOURCE=*) ;;
            *) out+=("$t") ;;
        esac
    done
    printf '%s' "${out[*]}"
}
if [ "${QEXTRA_ENV_FLAGS:-}" = 1 ]; then
    LITO_GLOBAL+=(--use-env-flags)
    CFLAGS="$(strip_profile_flags "${CFLAGS:-}")"
    CXXFLAGS="$(strip_profile_flags "${CXXFLAGS:-}")"
    LDFLAGS="$(strip_profile_flags "${LDFLAGS:-}")"
    export CFLAGS CXXFLAGS LDFLAGS
    echo "   флаги окружения для lito: CXXFLAGS=$CXXFLAGS"
fi
LITO_SOURCES=(--locked)
if [ -n "${QEXTRA_SOURCE_BUNDLE:-}" ]; then
    [ -d "$QEXTRA_SOURCE_BUNDLE/v1" ] || {
        echo "в наборе исходников нет каталога v1: $QEXTRA_SOURCE_BUNDLE" >&2
        exit 2
    }
    LITO_SOURCES=(--frozen --source-bundle "$QEXTRA_SOURCE_BUNDLE")
fi
(cd "$WORK/wrapper" && "$LITO_BIN" "${LITO_GLOBAL[@]}" build --profile release \
    "${LITO_SOURCES[@]}" -j "$JOBS")

SO="$WORK/wrapper/build/release/lib/qextraplugin/libqextraplugin.so"
TYPES="$WORK/wrapper/build/release/generated/qextra/lito-qml/QExtra/module.qmltypes"
for f in "$SO" "$TYPES"; do
    [ -f "$f" ] || { echo "сборка прошла, а файла нет: $f" >&2; exit 1; }
done

# The Qt version the plugin was built with — from the plugin itself: the «major,
# minor» bytes after the `qt-project!` tag in the `.note.qt.metadata` note (the
# core reads the same tag, `qml_modules::plugin_qt_version`). The full version
# of the system Qt is taken only if its major and minor match them.
at="$(LC_ALL=C grep -a -b -o 'qt-project!' "$SO" | head -1 | cut -d: -f1 || true)"
if [ -z "$at" ]; then
    echo "в плагине нет отметки Qt (.note.qt.metadata): $SO" >&2
    exit 1
fi
read -r qt_major qt_minor < <(od -A n -t u1 -j $((at + 13)) -N 2 "$SO")
QT_BUILT="$qt_major.$qt_minor"
QT_FULL="$(pkg-config --modversion Qt6Core 2>/dev/null || true)"
case "$QT_FULL" in "$QT_BUILT".*) QT_BUILT="$QT_FULL" ;; esac

# First build in a temporary directory next to it, then swap in one go: Rexon
# must not see a plugin without a mark or a mark from the previous plugin.
mkdir -p "$DEST"
NEW="$DEST/.QExtra.new.$$"
rm -rf "$NEW"; mkdir "$NEW"
cp "$SO" "$NEW/libqextraplugin.so"
cp "$TYPES" "$NEW/module.qmltypes"
cp "$SRC/qmldir" "$NEW/qmldir"
echo "$QT_BUILT" > "$NEW/QT_VERSION"
echo "${PIN:-unknown}" > "$NEW/QEXTRA_PIN"
rm -rf "$DEST/QExtra"
mv "$NEW" "$DEST/QExtra"
echo "== готово: $DEST/QExtra (Qt $QT_BUILT, $(du -h "$SO" | cut -f1)) =="
