//! `desktop-entry` — печать ярлыков Rexon генератором ядра.
//!
//! Зачем
//! -----
//! Пакет (`packaging/aur/PKGBUILD`) раньше вёз статичные копии
//! `rexon.desktop` и `rexon-recover.desktop`, сверенные с генератором руками.
//! Копия отстала при первой же правке генератора: ярлык пакета так и остался
//! на Python-входе, когда основной вход ушёл на Rust-окно. Теперь пакет
//! печатает ярлыки при сборке этой программой — второй копии содержимого нет.
//!
//! Вход задаётся ЯВНО, а не выбирается по машине сборщика: у пакета бинарь
//! окна лежит по постоянному пути [`PACKAGE_MAIN_WINDOW`], а живой выбор
//! ([`rexon_core::installer::launch_entry_live`]) смотрел бы на PATH и
//! деревья того, кто собирает.
//!
//! ```text
//! desktop-entry --package              # rexon.desktop пакета
//! desktop-entry --package --recover    # rexon-recover.desktop пакета
//! desktop-entry --main-window ПУТЬ     # вход на Rust-окно по пути
//! desktop-entry --python               # вход на Python (лаунчер rexon)
//! desktop-entry --main-window ПУТЬ --autostart   # запись автозапуска трея
//! ```
//! Запись автозапуска печатается теми же функциями ядра
//! ([`rexon_core::tray::autostart_exec`], [`rexon_core::tray::autostart_desktop_content`]),
//! которыми её пишет галочка «Автозапуск» в трее: ярлык и автозапуск обязаны
//! вести на один и тот же вход, иначе ярлык поднимет окно другого хозяина.
//! Коды возврата: 0 — ярлык напечатан; 2 — неверные аргументы.

use rexon_core::installer::{
    desktop_content_for, recover_desktop_content_for, LaunchEntry, PACKAGE_MAIN_WINDOW,
    PYTHON_ENTRY_CMD,
};
use rexon_core::tray::{autostart_desktop_content, autostart_exec};

const USAGE: &str =
    "использование: desktop-entry (--package | --main-window ПУТЬ | --python) [--recover | --autostart]";

fn main() {
    let mut entry: Option<LaunchEntry> = None;
    let mut recover = false;
    let mut autostart = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let next = match arg.as_str() {
            "--recover" => {
                recover = true;
                continue;
            }
            "--autostart" => {
                autostart = true;
                continue;
            }
            "--package" => LaunchEntry::MainWindow(PACKAGE_MAIN_WINDOW.to_string()),
            "--python" => LaunchEntry::Python,
            "--main-window" => match args.next() {
                Some(path) if path.starts_with('/') => LaunchEntry::MainWindow(path),
                _ => fail("после --main-window нужен абсолютный путь к бинарю окна"),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => fail(&format!("неизвестный аргумент: {other}")),
        };
        if entry.replace(next).is_some() {
            fail("вход указывается один раз");
        }
    }
    let Some(entry) = entry else { fail("не указан вход") };
    if recover && autostart {
        fail("--recover и --autostart взаимоисключают друг друга");
    }
    let text = if autostart {
        // Python-лаунчер лежит в PATH под тем же именем, что стоит в ярлыке.
        match autostart_exec(&entry, Some(PYTHON_ENTRY_CMD)) {
            Some(exec) => autostart_desktop_content(&exec),
            None => fail(rexon_core::app::MSG_NOTHING_TO_LAUNCH),
        }
    } else if recover {
        recover_desktop_content_for(&entry)
    } else {
        desktop_content_for(&entry)
    };
    print!("{text}");
}

fn fail(msg: &str) -> ! {
    eprintln!("desktop-entry: {msg}\n{USAGE}");
    std::process::exit(2);
}
