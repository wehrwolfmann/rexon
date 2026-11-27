// The rexon-qt-filter crate — the filter editor model of the Wallpaper
// Engine tab in Rust via cxx-qt (the first swallow of the GUI migration).
//
// Built into the cdylib librexon_qt_filter.so; tools/build_qml_control.sh
// puts it into the QML module directory next to waywallen.control, from where it is
// loaded by the wallpaper tab window (dlopen, rexon-gui we_env). The intermediate C++ copy of the model was removed
// on 2026-07-27 — this is the only implementation.

pub mod filter_rule_model;
