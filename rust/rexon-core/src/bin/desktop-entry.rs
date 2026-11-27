//! `desktop-entry` prints Rexon's desktop entries using the core generator.
//!
//! Why
//! ---
//! The package (`packaging/aur/PKGBUILD`) used to ship static copies of
//! `rexon.desktop` and `rexon-recover.desktop`, checked against the generator
//! by hand. The copy fell behind at the very first change to the generator:
//! the package's entry stayed on the Python entry point after the main entry
//! point had moved to the Rust window. Now the package prints the entries with
//! this program at build time, so there is no second copy of their contents.
//!
//! The entry point is given EXPLICITLY rather than picked from the builder's
//! machine: in the package the window binary lives at the fixed path
//! [`PACKAGE_MAIN_WINDOW`], while the live choice
//! ([`rexon_core::installer::launch_entry_live`]) would look at the PATH and
//! the trees of whoever is building.
//!
//! ```text
//! desktop-entry --package              # the package's rexon.desktop
//! desktop-entry --package --recover    # the package's rexon-recover.desktop
//! desktop-entry --main-window PATH     # entry for the Rust window at PATH
//! desktop-entry --python               # entry for Python (the rexon launcher)
//! desktop-entry --main-window PATH --autostart   # the tray autostart entry
//! ```
//! The autostart entry is printed by the same core functions
//! ([`rexon_core::tray::autostart_exec`], [`rexon_core::tray::autostart_desktop_content`])
//! that the "Autostart" checkbox in the tray uses to write it: the desktop
//! entry and the autostart entry must lead to the same entry point, otherwise
//! the desktop entry would bring up a window owned by someone else.
//! Exit codes: 0 — entry printed; 2 — invalid arguments.

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
        // The Python launcher is on the PATH under the same name as in the entry.
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
