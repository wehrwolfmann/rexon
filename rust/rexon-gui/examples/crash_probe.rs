// Стенд отчёта о крахе Rust-окна: ставит перехватчик тем же вызовом, что
// и `rexon-main-window`, и паникует.
//
//     cargo build --release -p rexon-gui --example crash_probe
//     rust/target/release/examples/crash_probe            # главный поток: процесс умирает
//     rust/target/release/examples/crash_probe --worker   # рабочий поток: процесс живёт
//
// Окно о крахе стенд показывает тем же путём, что продукт: запускает сам
// себя с `--crash-dialog <файл>`, и первой строкой `main` его отрабатывает
// `install()`. Без окна — `REXON_CRASH_NO_DIALOG=1`.

fn main() {
    rexon_gui::crash_report::install();
    if std::env::args().any(|a| a == "--worker") {
        let joined = std::thread::Builder::new()
            .name("worker".into())
            .spawn(|| panic!("проверочная паника стенда: рабочий поток"))
            .expect("поток стенда не запустился")
            .join();
        println!("рабочий поток упал: {}; процесс жив", joined.is_err());
        // Дать окну о крахе подняться, пока жив родитель (как в продукте).
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    panic!("проверочная паника стенда: главный поток");
}
