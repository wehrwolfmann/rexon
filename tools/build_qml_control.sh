#!/usr/bin/env bash
# Сборка QML-модуля waywallen.control — Qt Protobuf value-типы (enum'ы
# WallpaperSortKey/SortDirection, value-типы wallpaperSortRule/
# WallpaperFilterRule/… ), которые вендоренный QML waywallen требует как
# настоящие value-типы (PySide6 их предоставить не может). Генерится из
# наших proto (data/qt_wallpaper_engine/proto/*.proto) через qt_add_protobuf.
#
# Вторая цель — librexon_qt_filter.so: модель редактора фильтров, крейт
# rust/rexon-qt-filter (cxx-qt). В Python её быть не может — делегаты
# пишут в модель Qt-Protobuf-гаджеты, а их маршалинг в Python-setData
# сегфолтит PySide6. Окно подгружает .so через dlopen (rexon-gui, we_env.rs)
# и зовёт rexon_register_filter_model(). Прежняя C++-копия модели удалена.
#
# Требования: cmake, ninja, g++, qt6-base, qt6-declarative, qt6-grpc
# (Qt6 Protobuf/ProtobufQuick), protoc, cargo. Собирать против того же
# Qt, что использует PySide6.
set -euo pipefail
DEST="${1:-$HOME/.local/share/rexon/qml_modules}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
PROTO="$HERE/data/qt_wallpaper_engine/proto"
WORK="${QML_CONTROL_BUILD:-$HOME/.cache/rexon/qml_control-build}"
echo "== waywallen.control из $PROTO → $DEST =="
rm -rf "$WORK"; mkdir -p "$WORK/proto"
cp "$PROTO/control.proto" "$PROTO/filter.proto" "$WORK/proto/"
cat > "$WORK/CMakeLists.txt" <<'CM'
cmake_minimum_required(VERSION 3.20)
project(waywallen_control LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(BUILD_SHARED_LIBS ON)
find_package(Qt6 REQUIRED COMPONENTS Core Qml Quick Protobuf ProtobufQuick)
qt_standard_project_setup(REQUIRES 6.5)
set(PROTO_DIR "${CMAKE_CURRENT_SOURCE_DIR}/proto")
qt_add_protobuf(waywallen-control QML QML_URI waywallen.control
    PROTO_FILES "${PROTO_DIR}/control.proto" "${PROTO_DIR}/filter.proto"
    PROTO_INCLUDES "${PROTO_DIR}")
CM
cmake -S "$WORK" -B "$WORK/build" -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_COMPILER=g++ -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
    -DCMAKE_INSTALL_RPATH='$ORIGIN'
cmake --build "$WORK/build"
mkdir -p "$DEST/waywallen"
rm -rf "$DEST/waywallen/control"
cp -r "$WORK/build/waywallen/control" "$DEST/waywallen/control"
cp "$WORK/build/libwaywallen-control.so" "$DEST/waywallen/control/"
# Отметка протокола: SHA-256 склейки тех самых .proto, из которых модуль
# порождён. Rexon сверяет её со своей (`qml_modules::control_proto_stamp`) и
# не даёт сборке из прежнего протокола заслонить свежую из другого каталога.
# Порядок файлов и имя отметки — как в ядре (`CONTROL_STAMP_REL`).
cat "$WORK/proto/control.proto" "$WORK/proto/filter.proto" | sha256sum \
    | cut -d' ' -f1 > "$DEST/waywallen/control/proto.sha256"

# Модель редактора фильтров — крейт rust/rexon-qt-filter (cxx-qt). Кладём
# рядом с модулем: в пакете дерева rust/target нет, и без этой копии
# Rust-путь был бы доступен только в репозитории разработчика. Линкуется
# лишь с системным Qt (проверено ldd) — rpath не нужен.
echo "== rexon-qt-filter (Rust) =="
if command -v cargo >/dev/null 2>&1; then
    # Пакет назван через -p, и убирать его нельзя: в rust/Cargo.toml стоит
    # `default-members = ["rexon-core"]`, поэтому голый `cargo build --release`
    # собрал бы одно ядро, отрапортовал «Finished» и оставил .so старой.
    # Псевдоним `cargo build-app` тут не годится: --manifest-path не меняет
    # текущий каталог, а rust/.cargo/config.toml читается именно по нему.
    # rexon-gui здесь не нужен: своей .so у него больше нет, его типы вшиты
    # в rexon-main-window. Прежнюю копию librexon_gui.so из каталога
    # снимает `rm -rf` выше.
    cargo build --release --manifest-path "$HERE/rust/Cargo.toml" \
        -p rexon-qt-filter
    cp "$HERE/rust/target/release/librexon_qt_filter.so" \
       "$DEST/waywallen/control/"
else
    echo "!! cargo не найден — модель фильтров не собрана (редактор" \
         "фильтров во вкладке будет пуст)"
fi
echo "== готово: $DEST/waywallen/control =="
