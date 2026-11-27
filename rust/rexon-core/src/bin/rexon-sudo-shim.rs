//! `rexon-sudo-shim` is the "sudo" for AUR builders (paru, yay) inside the Rexon
//! window. The window puts it into `--sudo` and the operation label into
//! `--sudoflags`; the shim executes nothing itself, it passes the pacman
//! command line to the window, which decides on it and executes it through the
//! privilege session. All logic lives in [`rexon_core::privilege::aur`].
//!
//! ```text
//! rexon-sudo-shim --rexon-op=<window pid>.<token> pacman …
//! ```

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(rexon_core::privilege::aur::shim_main(&args));
}
