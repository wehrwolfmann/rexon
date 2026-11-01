// Сборка макета каркаса. Qt ищется через pkg-config — тот же Qt 6, что
// у PySide6 и продуктовых крейтов: ABI обязан совпадать, иначе макет
// проверит не то, что поедет в продукт.
//
// moc не зовётся намеренно: в макете нет Q_OBJECT (см. Cargo.toml).

fn main() {
    println!("cargo:rerun-if-changed=cpp/tab_host_probe.cpp");

    let mut build = cc::Build::new();
    build.cpp(true).std("c++17").pic(true).file("cpp/tab_host_probe.cpp");

    // Порядок важен: QuickWidgets тянет Quick/Qml, Widgets тянет Gui/Core.
    for module in ["Qt6QuickWidgets", "Qt6Widgets", "Qt6Quick", "Qt6Qml", "Qt6Gui", "Qt6Core"] {
        let lib = pkg_config::Config::new()
            .cargo_metadata(true)
            .probe(module)
            .unwrap_or_else(|e| panic!("не найден {module} через pkg-config: {e}"));
        for path in &lib.include_paths {
            build.include(path);
        }
        for (name, value) in &lib.defines {
            build.define(name, value.as_deref());
        }
    }

    build.compile("rexon_tab_host_probe");
}
