//! `rexon-login-rescue` shows understandable text on the console when the login
//! screen has failed for good, and provides the `rexon doctor` report. It is
//! intentionally free of Qt: it is needed precisely when Qt may be broken. The
//! decisions live in [`rexon_core::login_rescue`].
//!
//! ```text
//! rexon-login-rescue --boot          # from the rexon-login-rescue service (root)
//! rexon-login-rescue --watch-greeter # from the rexon-greeter-watch service (root)
//! rexon-login-rescue --doctor        # report for a human, changes nothing
//! rexon-login-rescue doctor          # the same (this is how the `rexon doctor` launcher calls it)
//! rexon-login-rescue --list-files    # the package's unit files (one path per line)
//! rexon-login-rescue --file <path>   # the text of one of them
//! ```
//! Exit codes: 0 — done; 1 — the text could not be written to the console;
//! 2 — invalid arguments.

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
