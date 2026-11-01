// Сборка cxx-qt-моста модели фильтров + обобщённого C++-шима мета-рефлексии
// гаджетов. Регистрационный шим (cpp/rexon_register.cpp) подключается после
// того, как известно имя сгенерированного заголовка cxx-qt.
use cxx_qt_build::CxxQtBuilder;

fn main() {
    // Собирать против того же Qt6, что использует PySide6 (ABI обязан
    // совпадать). Если QMAKE не задан — указать на qmake6 (в системе есть и
    // qmake от Qt5, cxx-qt по умолчанию может взять его).
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
