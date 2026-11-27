//! The Conky widget log: what conky wrote at startup and while running.
//!
//! WHY. A widget that "failed to start" used to leave the person with a single
//! phrase, "the settings file may be corrupted": conky's output went nowhere,
//! and the cause could only be found from a terminal. Now the "Start" button
//! sends conky's output to a file ([`log_path`]), the widget card shows its last
//! lines ([`tail`]), and the failure message names the cause in conky's own
//! words ([`failure_reason`]).
//!
//! Why a file and not a pipe: conky with `-d` goes into the background and
//! keeps the inherited output for its whole life — with a pipe the button would
//! wait for it to exit. The file is opened for appending ([`open_for_start`]):
//! Rexon may truncate a log that has grown too large ([`trim_if_large`]), and
//! conky keeps writing at its new end rather than into a hole at the old
//! position.
//!
//! A conky 1.2x line: `[2026-10-19 00:21:46.186] [warn ] [src/conky.cc:2059]
//! syntax error (…) while reading config file 'bad.conf'`; older releases, and
//! the last line before exit, use `conky: …`. Measured on conky 1.25.2 under
//! Xvfb, 19.10.2026.

use std::io::Write;

use crate::de_apply::Ctx;

/// How many of the last log lines to show in the card.
pub const TAIL_LINES: usize = 12;

/// A log larger than this is truncated to its last [`KEEP_BYTES`] bytes: a
/// widget with an error in `${exec}` writes it on every update.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
pub const KEEP_BYTES: usize = 64 * 1024;

/// The log directory: `$XDG_STATE_HOME/rexon/conky` (by default
/// `~/.local/state/rexon/conky`).
pub fn log_dir(ctx: &Ctx, xdg_state_home: Option<&str>) -> String {
    let base = match xdg_state_home.filter(|v| !v.is_empty()) {
        Some(v) => v.trim_end_matches('/').to_string(),
        None => format!("{}/.local/state", ctx.home.trim_end_matches('/')),
    };
    format!("{base}/rexon/conky")
}

/// The log file name for the settings file `conf`: the whole path, with `%`
/// and `/` encoded — the parts of one widget get their own logs, and two
/// widgets with the same name in different places do not share one file.
pub fn log_name(conf: &str) -> String {
    let mut out = String::new();
    for c in conf.trim_start_matches('/').chars() {
        match c {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            c => out.push(c),
        }
    }
    format!("{out}.log")
}

/// The startup log of the settings file `conf`.
pub fn log_path(ctx: &Ctx, xdg_state_home: Option<&str>, conf: &str) -> String {
    format!("{}/{}", log_dir(ctx, xdg_state_home), log_name(conf))
}

/// The live log path (`$XDG_STATE_HOME` from the environment).
pub fn live_log_path(ctx: &Ctx, conf: &str) -> String {
    log_path(ctx, std::env::var("XDG_STATE_HOME").ok().as_deref(), conf)
}

/// Opens the log for a new start: the previous text is erased (the log is
/// about this start), and the file is opened for appending. `None` means the
/// directory or the file could not be created (then conky starts without a
/// log, as before).
pub fn open_for_start(path: &str) -> Option<std::fs::File> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    std::fs::File::create(path).ok()?;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).ok()?;
    let _ = file.flush();
    Some(file)
}

/// Truncates the log if it has grown too large, keeping the last
/// [`KEEP_BYTES`]. Returns `true` if it was truncated.
pub fn trim_if_large(path: &str) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if meta.len() <= MAX_LOG_BYTES {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else { return false };
    let mut start = bytes.len().saturating_sub(KEEP_BYTES);
    // From the start of a line, not from the middle.
    if let Some(nl) = bytes[start..].iter().position(|b| *b == b'\n') {
        start += nl + 1;
    }
    let keep = bytes[start..].to_vec();
    let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) else { return false };
    if file.set_len(0).is_err() {
        return false;
    }
    drop(file);
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(&keep))
        .is_ok()
}

/// The level of a conky 1.2x line (`info`, `warn`, `error`, `critical`…);
/// `None` means a line without a tag (the old format, a message continuation).
pub fn level(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (_, rest) = rest.split_once("] [")?;
    let (lvl, _) = rest.split_once(']')?;
    Some(lvl.trim())
}

/// The line text without the timestamp, the level and the conky source
/// location, and without the `conky: ` prefix of the old format.
pub fn clean_line(line: &str) -> String {
    let mut s = line.trim();
    if level(s).is_some() {
        // `[time] [level] [src/file.cc:line] text` — three bracket groups.
        for _ in 0..3 {
            match s.strip_prefix('[').and_then(|r| r.split_once(']')) {
                Some((_, rest)) => s = rest.trim_start(),
                None => break,
            }
        }
    }
    s.strip_prefix("conky: ").unwrap_or(s).trim().to_string()
}

/// Why conky failed to start, in conky's own words. Order of importance:
/// 1. a syntax error in the settings file (after it conky tries the old
///    syntax and fails on that attempt too — that last line does not tell
///    the cause);
/// 2. the last line of level `critical` / `error`;
/// 3. the last old-format line `conky: …`;
/// 4. the last warning.
///
/// `None` means conky wrote nothing useful.
pub fn failure_reason(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let pick = |pred: &dyn Fn(&str) -> bool, last: bool| {
        let mut it: Box<dyn Iterator<Item = &&str>> =
            if last { Box::new(lines.iter().rev()) } else { Box::new(lines.iter()) };
        it.find(|l| pred(l)).map(|l| clean_line(l)).filter(|s| !s.is_empty())
    };
    pick(&|l| l.contains("syntax error"), false)
        .or_else(|| pick(&|l| matches!(level(l), Some("critical" | "error" | "err")), true))
        .or_else(|| pick(&|l| l.trim_start().starts_with("conky: "), true))
        .or_else(|| pick(&|l| matches!(level(l), Some("warn" | "warning")), true))
}

/// The last `n` log lines for the card, cleaned of the service tags.
pub fn tail(text: &str, n: usize) -> String {
    let lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(clean_line)
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// The widget log for the card: the freshest one (the last start) among the
/// logs of its parts (`confs`). Returns `(log file, last lines)`; `None` means
/// the widget has not been started from Rexon yet.
pub fn widget_log(ctx: &Ctx, confs: &[String]) -> Option<(String, String)> {
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for conf in confs {
        let path = live_log_path(ctx, conf);
        let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else { continue };
        if best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, path));
        }
    }
    let (_, path) = best?;
    trim_if_large(&path);
    let text = std::fs::read(&path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())?;
    Some((path, tail(&text, TAIL_LINES)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Output of conky 1.25.2 on a broken file (Xvfb stand, 19.10.2026).
    const BROKEN: &str = "\
[2026-10-19 00:21:46.185] [info ] [src/system-details.cc:177] 'unknown' X11 session running
[2026-10-19 00:21:46.186] [warn ] [src/conky.cc:2059] syntax error (bad.conf:2: '}' expected (to close '{' at line 1) near '=') while reading config file 'bad.conf'
[2026-10-19 00:21:46.186] [info ] [src/conky.cc:2061] assuming old config syntax and attempting conversion
conky: [string \"...\"]:148: attempt to index a nil value (local 'settings')
";

    const MISSING_LUA: &str = "\
[2026-10-19 00:21:46.199] [info ] [src/system-details.cc:177] 'unknown' X11 session running
[2026-10-19 00:21:46.200] [error] [src/lua/llua.cc:339] lua script './missing.lua' not found
[2026-10-19 00:21:46.201] [warn ] [src/output/display-x11.cc:1061] font index 0 out of range (0 loaded)
";

    #[test]
    fn reason_names_the_real_cause() {
        assert_eq!(
            failure_reason(BROKEN).as_deref(),
            Some("syntax error (bad.conf:2: '}' expected (to close '{' at line 1) near '=') while reading config file 'bad.conf'")
        );
        assert_eq!(failure_reason(MISSING_LUA).as_deref(), Some("lua script './missing.lua' not found"));
        // The old conky format (1.10–1.19).
        assert_eq!(failure_reason("conky: can't open display: \n").as_deref(), Some("can't open display:"));
        assert_eq!(failure_reason("[2026-10-19 00:00:00.000] [info ] [src/a.cc:1] ok\n"), None);
        assert_eq!(failure_reason(""), None);
    }

    #[test]
    fn lines_lose_service_marks() {
        assert_eq!(level("[2026-10-19 00:21:46.186] [warn ] [src/conky.cc:2059] x"), Some("warn"));
        assert_eq!(clean_line("[2026-10-19 00:21:46.186] [warn ] [src/conky.cc:2059] x y"), "x y");
        assert_eq!(clean_line("conky: z"), "z");
        assert_eq!(clean_line("plain"), "plain");
        assert_eq!(tail(MISSING_LUA, 2), "lua script './missing.lua' not found\nfont index 0 out of range (0 loaded)");
    }

    #[test]
    fn log_names_are_unique_per_config() {
        let c = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        assert_eq!(log_path(&c, None, "/h/.conky/A/conky.conf"), "/h/.local/state/rexon/conky/h%2F.conky%2FA%2Fconky.conf.log");
        assert_eq!(log_path(&c, Some("/s/"), "/x/100%/c"), "/s/rexon/conky/x%2F100%25%2Fc.log");
        assert_ne!(log_name("/a/b_c"), log_name("/a_b/c"));
    }

    /// The log is opened for appending: after truncation, writes go to the new
    /// end rather than into a hole at the old position.
    #[test]
    fn trimmed_log_keeps_the_tail_and_appends_at_end() {
        let dir = std::env::temp_dir().join(format!("rexon-conky-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("w.log").to_string_lossy().into_owned();
        let mut f = open_for_start(&path).expect("журнал открывается");
        let line = "x".repeat(99) + "\n";
        for _ in 0..(MAX_LOG_BYTES as usize / 100 + 10) {
            f.write_all(line.as_bytes()).unwrap();
        }
        assert!(trim_if_large(&path));
        f.write_all(b"after\n").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.len() <= KEEP_BYTES + 10, "{}", text.len());
        assert!(!text.contains('\0'), "дыра от записи по старому смещению");
        assert!(text.ends_with("after\n") && text.starts_with('x'));
        // A new start erases the previous log.
        drop(f);
        drop(open_for_start(&path));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
