#!/usr/bin/env bash
# Сборка QML-плагина QExtra (hypengw/QExtra, MIT OR Apache-2.0) — настоящие
# `QE.AnimatedImage` и `QE.ImageCache` для вкладки обоев: декодер на ffmpeg,
# общий потолок памяти кадров, общий такт одинаковых анимаций. Третий в
# семье `build_qml_*`.
#
# Исходников QExtra в дереве нет. В дереве лежит только наша обёртка
# `tools/qextra_plugin/` (общая библиотека-плагин, ~20 строк склейки) и
# `lito.lock` рядом с ней: в нём закреплены ревизия QExtra и всех его
# зависимостей — срез `lito.lock` выпуска waywallen, с которым приехала
# вендорная разметка. Сборка идёт с `--locked`: разошёлся lock — отказ, а
# не молчаливая сборка с головы.
#
# ⚠️ Собирается ВНЕ дерева, в каталоге с путём из латиницы
# (`~/.cache/rexon-qextra-build`): lito падает на нелатинских путях, а дерево
# может лежать в `~/Загрузки`. В `/tmp` не собирать — каталог сборки весит
# ~740 МБ, а `/tmp` часто живёт в памяти.
#
# ⚠️ Плагин зовёт частный API Qt (`QRhi`) и годится только той младшей
# версии Qt, с которой собран. Рядом с ним пишется отметка `QT_VERSION`;
# Rexon сверяет её с Qt процесса (`qml_modules::choose_qextra`) и при
# расхождении берёт QML-подмену из `qml_ui/QExtra/`.
#
# Требования: lito >= 0.8.7 (переменная LITO либо PATH), clang >= 22, lld,
# cmake, ninja, qt6-base, qt6-declarative, qt6-grpc, ffmpeg, libva, curl,
# glslang, vulkan-headers, libpulse.
#
# Переменные (необязательно): LITO — путь к lito; QEXTRA_BUILD — каталог
# сборки; QEXTRA_JOBS — число потоков (4).
# Для сборки пакета (в ней нет сети и флаги задаёт makepkg):
#   QEXTRA_SOURCE_BUNDLE — каталог набора исходников lito (внутри `v1/git/…`
#     и `v1/registry/source/…`, раскладка `lito lock export`): сборка идёт
#     с `--frozen` и в сеть не ходит вовсе;
#   QEXTRA_ENV_FLAGS=1 — добавить CFLAGS, CXXFLAGS и LDFLAGS окружения
#     (`lito --use-env-flags`), в том числе `-ffile-prefix-map`; ключи,
#     которые держит профиль release (-O*, -g*, LTO, strip), и
#     _FORTIFY_SOURCE сценарий срезает сам — годятся флаги любого makepkg.conf.
#
# Тяжёлая: первая сборка ~2–3 минуты на 4 потоках. На занятой машине —
#   systemd-run --user --scope -p MemoryMax=10G nice tools/build_qml_qextra.sh
set -euo pipefail

# 0.8.7: манифест wavsen 7564374 (срез waywallen 0.5.0) пишет у зависимости
# поле `condition`, и 0.8.5 отвергает его «unknown field: condition».
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

# Обёртка копируется в каталог сборки заново, каталог `build/` в нём
# остаётся: повторная сборка без изменений — секунды.
mkdir -p "$WORK/wrapper"
for f in lito.toml lito.lock build.lua plugin.hpp plugin.cpp plugin.cppm; do
    cmp -s "$SRC/$f" "$WORK/wrapper/$f" || cp "$SRC/$f" "$WORK/wrapper/$f"
done
LITO_GLOBAL=()

# Флаги окружения доходят до lito не целиком. Срезается то, что `--profile
# release` держит за собой: оптимизация (-O3), отладочная информация (нет),
# LTO (нет), а на линковке ещё и strip (нет). Любой такой ключ из окружения,
# расходящийся с профилем, lito отвергает ДО сборки («compiler option '-O2'
# from CXXFLAGS sets optimization to '-O2', but the selected profile 'release'
# fixes it to '-O3'»): с CFLAGS Arch (-O2) пакет не собирался вовсе, а на
# CachyOS проходил только потому, что там -O3. Защитные ключи
# (-fstack-clash-protection, -fcf-protection, -D_GLIBCXX_ASSERTIONS…) и карта
# путей остаются. `-Wl,-O1` — ключ компоновщика, не профиля, и тоже остаётся.
#
# ⚠️ _FORTIFY_SOURCE снимается тоже: с ним glibc объявляет open/openat
# встроенными обёртками с внутренним связыванием (bits/fcntl2.h), а модуль
# C++ такое имя экспортировать не может — rstd падает на `using ::openat`
# («with internal linkage cannot be exported»). Так же поступает и пакет
# самого lito в AUR.
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

# Версия Qt, с которой плагин собран, — из него самого: байты «старшая,
# младшая» за меткой `qt-project!` в заметке `.note.qt.metadata` (ту же
# метку читает ядро, `qml_modules::plugin_qt_version`). Полная версия
# системного Qt берётся, только если её старшая и младшая с ними совпали.
at="$(LC_ALL=C grep -a -b -o 'qt-project!' "$SO" | head -1 | cut -d: -f1 || true)"
if [ -z "$at" ]; then
    echo "в плагине нет отметки Qt (.note.qt.metadata): $SO" >&2
    exit 1
fi
read -r qt_major qt_minor < <(od -A n -t u1 -j $((at + 13)) -N 2 "$SO")
QT_BUILT="$qt_major.$qt_minor"
QT_FULL="$(pkg-config --modversion Qt6Core 2>/dev/null || true)"
case "$QT_FULL" in "$QT_BUILT".*) QT_BUILT="$QT_FULL" ;; esac

# Сначала собрать во временном каталоге рядом, потом подменить разом: Rexon
# не должен увидеть плагин без отметки или отметку от прежнего плагина.
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
