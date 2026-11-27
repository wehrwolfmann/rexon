#!/usr/bin/env bash
# Build of the Qcm.Material QML plugin (QmlMaterial, MIT) for the QML interface
# of the WE tab. The commit pin TRAVELS WITH THE VENDOR: it is written by
# scripts/sync-waywallen-qml.py from the waywallen lito.lock at the tag from
# ui/UPSTREAM_TAG and put next to it as the file ui/QML_MATERIAL_PIN.
#
# ⚠️ Why not a literal: once diverged, the pin gives the nastiest class of
# defects — the vendored QML is new, the library under it is old, and the mismatch
# is visible only by eye on a live page. A caught case (07.09.2026):
# the pin here lagged behind the vendor by five releases, and the title case of
# Russian labels fixed by hypengw (043c09e, merged 27.08) did not reach our
# build — four patches on top of the vendor were kept instead.
#
# Requirements: cmake, ninja, g++, qt6-{base,declarative,shadertools,svg}
# (the Qt version == the Qt version of PySide6 — the plugin uses QuickPrivate).
#
# IMPORTANT: the Material Symbols fonts lie in the repo as git-lfs pointers;
# the script downloads the real ones from media.githubusercontent.com.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PIN_FILE="$HERE/../data/qt_wallpaper_engine/qml_ui/waywallen/ui/QML_MATERIAL_PIN"
if [ -z "${QML_MATERIAL_PIN:-}" ] && [ ! -r "$PIN_FILE" ]; then
    echo "нет $PIN_FILE — прогоните scripts/sync-waywallen-qml.py" >&2
    exit 2
fi
PIN="${QML_MATERIAL_PIN:-$(tr -d '[:space:]' < "$PIN_FILE")}"
REPO="https://github.com/hypengw/QmlMaterial.git"
WORK="${QML_MATERIAL_SRC:-$HOME/.cache/rexon/qml_material-src}"
DEST="${1:-$HOME/.local/share/rexon/qml_modules}"

echo "== QmlMaterial @$PIN → $DEST =="

if [ ! -d "$WORK/.git" ]; then
    git clone --no-checkout "$REPO" "$WORK"
fi
git -C "$WORK" fetch origin "$PIN" 2>/dev/null || git -C "$WORK" fetch origin
# `-f`: in the clone cache the fonts below are already replaced with the real ones (an edit
# of tracked files), and without it switching to a NEW pin refuses with
# «Сделайте коммит или спрячьте ваши изменения» (caught 24.10.2026 when
# switching 98b8acd → f1464c5). The fonts for the new pin are downloaded again.
#
# ⛔ Only if the tree is NOT on the pin. `checkout -f` on the same commit also
# resets edits of tracked files — while the package (PKGBUILD, prepare())
# puts the real fonts into the tree in advance, to build without network.
# The reset brought back the git-lfs pointers, downloading them again without network
# was impossible, and 131-byte pointers silently went into the package instead of fonts:
# all icons of the wallpaper tab were empty squares (VM, 30.10.2026, r1893).
# A fresh `clone --no-checkout` has HEAD on the remote's default branch and an
# empty tree: when that branch tip IS the pin, the commit check alone skipped the
# checkout and cmake found no CMakeLists.txt (27.11.2026, pin 712fb28). Only an
# EMPTY tree counts: a prepared one must keep its real fonts (see above).
if [ -z "$(ls -A "$WORK" | grep -vx '.git')" ] || \
   [ "$(git -C "$WORK" rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null || true)" != \
     "$(git -C "$WORK" rev-parse -q --verify "$PIN^{commit}")" ]; then
    git -C "$WORK" checkout -q -f "$PIN"
fi

# git-lfs pointers → real fonts
is_lfs_pointer() { head -1 "$1" | grep -q '^version https://git-lfs'; }
for f in "$WORK"/assets/MaterialSymbolsRounded.*; do
    if is_lfs_pointer "$f"; then
        name="$(basename "$f")"
        echo "  lfs: $name"
        curl -sfL "https://media.githubusercontent.com/media/hypengw/QmlMaterial/$PIN/assets/$name" \
            -o "$f.dl" && mv "$f.dl" "$f" || rm -f "$f.dl"
    fi
done
# A pointer instead of a font — the build fails rather than building the module without
# icons.
for f in "$WORK"/assets/MaterialSymbolsRounded.*; do
    if is_lfs_pointer "$f"; then
        echo "ОШИБКА: $(basename "$f") — указатель git-lfs, а не шрифт (нет сети?)." >&2
        echo "Положите настоящий файл в $WORK/assets/ или повторите со связью." >&2
        exit 1
    fi
done

cmake -S "$WORK" -B "$WORK/build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_COMPILER=g++ \
    -DQM_BUILD_EXAMPLE=OFF -DQM_BUILD_TESTS=OFF \
    -DQML_MATERIAL_BUILD_TYPE=SHARED \
    -DQT_NO_PRIVATE_MODULE_WARNING=ON \
    -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
    -DCMAKE_INSTALL_RPATH='$ORIGIN'
cmake --build "$WORK/build"

mkdir -p "$DEST"
rm -rf "$DEST/Qcm"
cp -r "$WORK/build/qml_modules/Qcm" "$DEST/"
# The plugins link against the module's shared libraries — we put them next to them
# (rpath $ORIGIN). Since the waywallen v0.2.6 build there are TWO: Qcm.Material
# gained the Layouts submodule with its own libqml_material_layouts.so; without it
# the QML engine does not load Qcm.Material at all («Failed to load dependent
# import Qcm.Material.Layouts»). We copy everything that was built in the root.
for lib in "$WORK"/build/libqml_material*.so; do
    cp "$lib" "$DEST/Qcm/Material/"
done
# submodules (Layouts) have their own directory with a plugin — the shared library is needed
# there too; directories without a plugin (assets, qml) are left alone. We put not a copy but
# a relative link to the library one level up: a second copy of
# libqml_material.so is +20.8 MiB in the package (6–7 MiB compressed), and
# the loader keeps one library per SONAME anyway, so the copy
# gave nothing. The plugin's rpath $ORIGIN finds the link like a regular file.
for plug in "$DEST"/Qcm/Material/*/*plugin.so; do
    [ -e "$plug" ] || continue
    for lib in "$WORK"/build/libqml_material*.so; do
        ln -sfn "../$(basename "$lib")" "$(dirname "$plug")/$(basename "$lib")"
    done
done
# A mark of the built revision next to the module. ⚠️ Without it «the pin moved with
# the vendor, but the module stayed yesterday's» cannot be seen at all: the module does not
# name its revision, and the mismatch surfaced only by eye on a live
# page (the caught case of 07.09.2026 — a lag of five releases).
printf '%s\n' "$PIN" > "$DEST/Qcm/QML_MATERIAL_PIN"
echo "== готово: $DEST/Qcm/Material (редакция $PIN) =="
