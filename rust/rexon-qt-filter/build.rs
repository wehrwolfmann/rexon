// Build of the filter model cxx-qt bridge + the generic C++ shim for gadget
// meta-reflection. The registration shim (cpp/rexon_register.cpp) is added after
// the name of the generated cxx-qt header is known.
use cxx_qt_build::CxxQtBuilder;

fn main() {
    // Build against the same Qt6 that PySide6 uses (the ABI must
    // match). If QMAKE is not set — point to qmake6 (the system also has
    // qmake from Qt5, and cxx-qt may pick it by default).
    if std::env::var_os("QMAKE").is_none() {
        for cand in ["/usr/bin/qmake6", "/usr/lib/qt6/bin/qmake"] {
            if std::path::Path::new(cand).exists() {
                std::env::set_var("QMAKE", cand);
                break;
            }
        }
    }

    CxxQtBuilder::new()
        .qt_module("Qml")
        .qt_module("Quick")
        .file("src/filter_rule_model.rs")
        .include_dir("cpp")
        .cpp_files(["cpp/rexon_gadget_shim.cpp", "cpp/rexon_register.cpp"])
        .build();
}
