//! `rexon-sudo-shim` — «sudo» сборщиков AUR (paru, yay) в окне Rexon.
//! Окно ставит его в `--sudo`, метку операции — в `--sudoflags`; шим ничего
//! не исполняет сам, а передаёт строку pacman окну, и то решает и исполняет
//! её через сеанс прав. Вся логика — в [`rexon_core::privilege::aur`].
//!
//! ```text
//! rexon-sudo-shim --rexon-op=<pid окна>.<токен> pacman …
//! ```

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(rexon_core::privilege::aur::shim_main(&args));
}
