//! `rexon-priv-helper` is the root helper of the privilege session ("one
//! password per launch"). It is started only by the Rexon window via `pkexec`;
//! the channel is the window's socket on fd 0/1. All logic lives in
//! [`rexon_core::privilege::helper`].
//!
//! ```text
//! pkexec rexon-priv-helper --session=<logind session number> [--lang=<code>]
//! ```
//! Exit codes are the `EXIT_*` constants in `helper.rs` (126/127 are not used:
//! those are pkexec's own codes).
//!
//! `rexon-priv-helper --build-id` needs neither root nor the channel: it prints
//! the fingerprint of its build and exits. The window asks for it BEFORE
//! `pkexec`, so that it does not ask for the password in vain for a helper
//! from a different build.

fn main() {
    if std::env::args().nth(1).as_deref() == Some(rexon_core::privilege::client::BUILD_ID_FLAG) {
        println!("{}", rexon_core::privilege::proto::build_id());
        return;
    }
    std::process::exit(rexon_core::privilege::helper::main_live());
}
