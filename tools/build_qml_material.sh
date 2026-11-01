#!/usr/bin/env bash
# Сборка QML-плагина Qcm.Material (QmlMaterial, MIT) для QML-интерфейса
# вкладки WE. Пин коммита ЕДЕТ ВМЕСТЕ С ВЕНДОРОМ: его выписывает
# scripts/sync-waywallen-qml.py из lito.lock waywallen на теге из
# ui/UPSTREAM_TAG и кладёт рядом файлом ui/QML_MATERIAL_PIN.
#
# ⚠️ Почему не литерал: разъехавшись, пин даёт самый неприятный класс
# дефектов — вендорный QML новый, библиотека под ним старая, и расхождение
# видно только глазами на живой странице. Пойманный случай (07.09.2026):
# пин тут отставал от вендора на пять выпусков, и починенный у hypengw
# заголовочный регистр русских подписей (043c09e, влит 27.08) до нашей
# сборки не доезжал — вместо него держались четыре патча поверх вендора.
#
# Требования: cmake, ninja, g++, qt6-{base,declarative,shadertools,svg}
# (версия Qt == версии Qt у PySide6 — плагин использует QuickPrivate).
#
# ВАЖНО: шрифты Material Symbols в репо лежат как git-lfs pointers;
# скрипт скачивает настоящие с media.githubusercontent.com.
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
# `-f`: в кэше клона шрифты ниже уже заменены настоящими (это правка
# отслеживаемых файлов), и без него переход на НОВЫЙ пин отказывает
# «Сделайте коммит или спрячьте ваши изменения» (пойман 24.10.2026 на
# переходе 98b8acd → f1464c5). Шрифты под новый пин скачиваются заново.
#
# ⛔ Только если дерево НЕ на пине. `checkout -f` на том же коммите тоже
# сбрасывает правки отслеживаемых файлов — а пакет (PKGBUILD, prepare())
# кладёт настоящие шрифты в дерево заранее, чтобы собираться без сети.
# Сброс возвращал указатели git-lfs, скачать их заново без сети не
# выходило, и в пакет молча уезжали 131-байтные указатели вместо шрифтов:
# все значки вкладки обоев — пустые квадраты (ВМ, 30.10.2026, r1893).
if [ "$(git -C "$WORK" rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null || true)" != \
     "$(git -C "$WORK" rev-parse -q --verify "$PIN^{commit}")" ]; then
    git -C "$WORK" checkout -q -f "$PIN"
fi

# git-lfs pointers → настоящие шрифты
is_lfs_pointer() { head -1 "$1" | grep -q '^version https://git-lfs'; }
for f in "$WORK"/assets/MaterialSymbolsRounded.*; do
    if is_lfs_pointer "$f"; then
        name="$(basename "$f")"
        echo "  lfs: $name"
        curl -sfL "https://media.githubusercontent.com/media/hypengw/QmlMaterial/$PIN/assets/$name" \
            -o "$f.dl" && mv "$f.dl" "$f" || rm -f "$f.dl"
    fi
done
# Указатель вместо шрифта — сборка проваливается, а не собирает модуль без
# значков.
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
# Плагины линкуются с общими библиотеками модуля — кладём их рядом
# (rpath $ORIGIN). Со сборки waywallen v0.2.6 их ДВЕ: у Qcm.Material
# появился подмодуль Layouts со своей libqml_material_layouts.so; без неё
# QML-движок не грузит весь Qcm.Material («Failed to load dependent
# import Qcm.Material.Layouts»). Копируем всё, что собралось в корне.
for lib in "$WORK"/build/libqml_material*.so; do
    cp "$lib" "$DEST/Qcm/Material/"
done
# у подмодулей (Layouts) свой каталог с плагином — общая библиотека нужна
# и там; каталоги без плагина (assets, qml) не трогаем
for plug in "$DEST"/Qcm/Material/*/*plugin.so; do
    [ -e "$plug" ] || continue
    cp "$WORK"/build/libqml_material*.so "$(dirname "$plug")/"
done
# Отметка собранной редакции рядом с модулем. ⚠️ Без неё «пин уехал с
# вендором, а модуль остался вчерашний» никак не увидеть: сам модуль своей
# ревизии не называет, и расхождение вылезало только глазами на живой
# странице (пойманный случай 07.09.2026 — отставание на пять выпусков).
printf '%s\n' "$PIN" > "$DEST/Qcm/QML_MATERIAL_PIN"
echo "== готово: $DEST/Qcm/Material (редакция $PIN) =="
