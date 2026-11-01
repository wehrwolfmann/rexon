//! `rexon-priv-helper` — root-помощник сеанса прав («один пароль на запуск»).
//! Запускается только окном Rexon через `pkexec`, канал — сокет окна на fd 0/1.
//! Вся логика — в [`rexon_core::privilege::helper`].
//!
//! ```text
//! pkexec rexon-priv-helper --session=<номер сеанса logind> [--lang=<код>]
//! ```
//! Коды возврата — `EXIT_*` в `helper.rs` (126/127 не используются: это коды
//! самого pkexec).
//!
//! `rexon-priv-helper --build-id` — без root и без канала: печатает отпечаток
//! своей сборки и выходит. Окно спрашивает его ДО `pkexec`, чтобы с
//! помощником чужой сборки не спрашивать пароль впустую.

fn main() {
    if std::env::args().nth(1).as_deref() == Some(rexon_core::privilege::client::BUILD_ID_FLAG) {
        println!("{}", rexon_core::privilege::proto::build_id());
        return;
    }
    std::process::exit(rexon_core::privilege::helper::main_live());
}
