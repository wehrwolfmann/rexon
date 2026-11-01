//! `rexon-login-rescue` — понятный текст на консоли, когда экран входа
//! окончательно упал, и отчёт `rexon doctor`. Без Qt намеренно: нужен
//! именно тогда, когда Qt мог сломаться. Решения — в
//! [`rexon_core::login_rescue`].
//!
//! ```text
//! rexon-login-rescue --boot          # из службы rexon-login-rescue (root)
//! rexon-login-rescue --watch-greeter # из службы rexon-greeter-watch (root)
//! rexon-login-rescue --doctor        # отчёт для человека, ничего не меняет
//! rexon-login-rescue doctor          # то же (так зовёт лаунчер `rexon doctor`)
//! rexon-login-rescue --list-files    # файлы юнитов пакета (по пути в строке)
//! rexon-login-rescue --file <путь>   # текст одного из них
//! ```
//! Коды возврата: 0 — сделано; 1 — не записался текст на консоль; 2 —
//! неверные аргументы.

use rexon_core::login_rescue as lr;

fn files() -> Vec<(String, String)> {
    lr::unit_files(lr::UNIT_DIR_PACKAGE, lr::RESCUE_BIN_PACKAGE)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = |line: &str| println!("{line}");
    let rc = match args.as_slice() {
        [a] if a == lr::BOOT_FLAG => lr::boot_live(&mut out),
        [a] if a == lr::WATCH_FLAG => lr::watch_live(&mut out),
        _ if lr::is_doctor_args(&args) => lr::doctor_live(&mut out),
        [a] if a == "--list-files" => {
            for (path, _) in files() {
                println!("{path}");
            }
            0
        }
        [a, path] if a == "--file" => match files().into_iter().find(|(p, _)| p == path) {
            Some((_, text)) => {
                print!("{text}");
                0
            }
            None => {
                eprintln!("rexon-login-rescue: unknown file: {path}");
                2
            }
        },
        _ => {
            eprintln!("usage: rexon-login-rescue --boot | --watch-greeter | --doctor | --list-files | --file <path>");
            2
        }
    };
    std::process::exit(rc);
}
