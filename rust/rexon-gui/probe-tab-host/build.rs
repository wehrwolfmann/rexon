// Build of the frame mock-up. Qt is found through pkg-config — the same
// Qt 6 as for PySide6 and the product crates: the ABI must match,
// otherwise the mock-up would test something other than what ships in the
// product.
//
// moc is deliberately not called: the mock-up has no Q_OBJECT (see
// Cargo.toml).

fn main() {
    println!("cargo:rerun-if-changed=cpp/tab_host_probe.cpp");

    let mut build = cc::Build::new();
    build.cpp(true).std("c++17").pic(true).file("cpp/tab_host_probe.cpp");

    // Order matters: QuickWidgets pulls in Quick/Qml, Widgets pulls in
    // Gui/Core.
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
