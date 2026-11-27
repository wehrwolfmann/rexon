#!/usr/bin/env bash
# Build of the waywallen.control QML module — Qt Protobuf value types (the enums
# WallpaperSortKey/SortDirection, the value types wallpaperSortRule/
# WallpaperFilterRule/… ), which the vendored waywallen QML requires as
# real value types (PySide6 cannot provide them). Generated from
# our proto (data/qt_wallpaper_engine/proto/*.proto) through qt_add_protobuf.
#
# The second target is librexon_qt_filter.so: the filter editor model, the crate
# rust/rexon-qt-filter (cxx-qt). It cannot be in Python — the delegates
# write Qt Protobuf gadgets into the model, and marshalling them into a Python setData
# segfaults PySide6. The window loads the .so through dlopen (rexon-gui, we_env.rs)
# and calls rexon_register_filter_model(). The former C++ copy of the model was removed.
#
# Requirements: cmake, ninja, g++, qt6-base, qt6-declarative, qt6-grpc
# (Qt6 Protobuf/ProtobufQuick), protoc, cargo. Build against the same
# Qt that PySide6 uses.
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
# The protocol mark: SHA-256 of the concatenation of the very .proto files the module
# was generated from. Rexon compares it with its own (`qml_modules::control_proto_stamp`) and
# does not let a build from an older protocol shadow a fresh one from another directory.
# The file order and the mark name are as in the core (`CONTROL_STAMP_REL`).
cat "$WORK/proto/control.proto" "$WORK/proto/filter.proto" | sha256sum \
    | cut -d' ' -f1 > "$DEST/waywallen/control/proto.sha256"

# The filter editor model is the crate rust/rexon-qt-filter (cxx-qt). We put it
# next to the module: the package has no rust/target tree, and without this copy
# the Rust path would be available only in the developer's repository. It links
# only against the system Qt (checked with ldd) — no rpath needed.
echo "== rexon-qt-filter (Rust) =="
if command -v cargo >/dev/null 2>&1; then
    # The package is named with -p, and it must not be removed: rust/Cargo.toml has
    # `default-members = ["rexon-core"]`, so a bare `cargo build --release`
    # would build only the core, report «Finished» and leave the .so stale.
    # The `cargo build-app` alias does not fit here: --manifest-path does not change
    # the current directory, and rust/.cargo/config.toml is read exactly by it.
    # rexon-gui is not needed here: it no longer has its own .so, its types are embedded
    # in rexon-main-window. The former copy of librexon_gui.so is removed from the directory
    # by the `rm -rf` above.
    cargo build --release --manifest-path "$HERE/rust/Cargo.toml" \
        -p rexon-qt-filter
    cp "$HERE/rust/target/release/librexon_qt_filter.so" \
       "$DEST/waywallen/control/"
else
    echo "!! cargo не найден — модель фильтров не собрана (редактор" \
         "фильтров во вкладке будет пуст)"
fi
echo "== готово: $DEST/waywallen/control =="
