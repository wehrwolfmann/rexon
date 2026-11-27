// Test bench for the Rust window crash report: installs the handler with
// the same call as `rexon-main-window`, and panics.
//
//     cargo build --release -p rexon-gui --example crash_probe
//     rust/target/release/examples/crash_probe            # main thread: the process dies
//     rust/target/release/examples/crash_probe --worker   # worker thread: the process lives on
//
// The bench shows the crash window the same way as the product: it starts
// itself with `--crash-dialog <file>`, and the first line of `main`,
// `install()`, handles it. Without the window — `REXON_CRASH_NO_DIALOG=1`.

fn main() {
    rexon_gui::crash_report::install();
    if std::env::args().any(|a| a == "--worker") {
        let joined = std::thread::Builder::new()
            .name("worker".into())
            .spawn(|| panic!("проверочная паника стенда: рабочий поток"))
            .expect("поток стенда не запустился")
            .join();
        println!("рабочий поток упал: {}; процесс жив", joined.is_err());
        // Let the crash window come up while the parent is alive (as in the
        // product).
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    panic!("проверочная паника стенда: главный поток");
}
