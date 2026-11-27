//! "One password per launch" (variant "B"): the Rexon privilege session.
//!
//! The window starts the root helper with a single `pkexec` (action
//! `org.rexon.session`, `auth_admin` without caching) and then talks to it over
//! a `socketpair`: the helper performs only narrow operations from the core's
//! allow-lists and exits when the window is closed (including to the tray),
//! after 15 minutes of inactivity, on screen lock and on suspend. Marks such as
//! "B4" and "Z1" in this subsystem's modules are item numbers from its plan:
//! B are build steps, Z are prohibitions (in the original Russian plan they are
//! written «Б» and «З»).
//!
//! The subsystem is built piece by piece; until `run` is wired in, all calls go
//! the old way — through `rexon-*-helper` and plain `pkexec`.

pub mod aur;
pub mod client;
pub mod helper;
pub mod op;
pub mod proto;
pub mod revoke;
pub mod run;
pub mod unpack;
pub mod validate;
