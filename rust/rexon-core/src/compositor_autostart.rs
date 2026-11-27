//! Autostart at login where the desktop does NOT execute XDG autostart.
//!
//! `~/.config/autostart/*.desktop` records are executed by full desktops (KDE,
//! GNOME, Cinnamon, XFCE, MATE, Budgie, LXQt, COSMIC) and by systemd sessions
//! that bring up `xdg-desktop-autostart.target`. Bare compositors
//! do not read them at all, and the "Start at login" switch was lit, while after
//! login nothing started: neither the tray, nor the splash, nor the wallpaper engine.
//!
//! Who executes what (checked against the sources and man pages, 02.10.2026):
//!
//! | compositor | XDG autostart                                  | native way                            |
//! |------------|------------------------------------------------|---------------------------------------|
//! | sway       | no (only with sway-systemd / its own lines)    | `exec …` in `~/.config/sway/config`   |
//! | Hyprland   | no; yes only under uwsm                        | `hl.on("hyprland.start", …)` in `hyprland.lua`, `exec-once =` in `hyprland.conf` |
//! | niri       | yes via `niri-session` (systemd), otherwise no | `spawn-at-startup` in `config.kdl`    |
//! | river      | no                                             | the executable `~/.config/river/init` |
//! | labwc      | no                                             | the script `~/.config/labwc/autostart` |
//! | Wayfire    | no                                             | the `[autostart]` section in `wayfire.ini` |
//!
//! So in these six Rexon writes ITS OWN line into the native config — as a managed
//! block between markers — and hides the `.desktop` record from them with the key
//! `NotShowIn=` ([`with_not_show_in`]). The result is exactly one launch path per
//! desktop: where the compositor executes both XDG and its own line (niri from
//! `niri-session`, Hyprland under uwsm), the `.desktop` is hidden from it, and the program
//! does not start twice. `NotShowIn=` gets only the compositor into whose
//! config the line REALLY went: otherwise under uwsm we would hide a record that
//! had no replacement left.
//!
//! The block is reversible bit for bit: the start marker remembers whether Rexon added a
//! newline before the block (` +nl`) and whether it created the file itself (` +new`); removal
//! returns the file to its previous bytes, and deletes a created file.
//!
//! ⚠️ Missing files are created only where it breaks nothing:
//! * sway — our own `config` with an `include` of the system one (without it our own file
//!   would disable the whole system config, keybindings included);
//! * labwc — our own `autostart` that first executes the system one;
//! * Wayfire — our own `wayfire.ini` (the defaults are read from
//!   `defaults.ini` anyway).
//!
//! Hyprland and niri put a default config themselves on first login — if
//! we got ahead of them with a one-line file, the person would be left without keybindings.
//! river without `init` is useless by itself. There we do not create a file and honestly
//! say the line did not go in.
//!
//! A shared helper for other autostart lines (for example, a shell that
//! the "Desktops" tab installs): [`ensure_block`] / [`remove_block`] per
//! file, [`resolve_target`] — which file is the compositor's main one,
//! [`launch_line`] — the launch line in its syntax.

use std::path::Path;

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Compositors and syntaxes
// ============================================================================

/// A compositor that does not execute XDG autostart records itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compositor {
    Sway,
    Hyprland,
    Niri,
    River,
    Labwc,
    Wayfire,
}

impl Compositor {
    /// All of them, in order.
    pub const ALL: [Compositor; 6] = [
        Compositor::Sway,
        Compositor::Hyprland,
        Compositor::Niri,
        Compositor::River,
        Compositor::Labwc,
        Compositor::Wayfire,
    ];

    /// Name for the person (program names are not translated).
    pub fn display_name(self) -> &'static str {
        match self {
            Compositor::Sway => "Sway",
            Compositor::Hyprland => "Hyprland",
            Compositor::Niri => "niri",
            Compositor::River => "river",
            Compositor::Labwc => "labwc",
            Compositor::Wayfire => "Wayfire",
        }
    }

    /// Names in `XDG_CURRENT_DESKTOP` (`NotShowIn=` gets the same ones):
    /// `DesktopNames=` of the session files plus what the compositor sets itself.
    pub fn desktop_names(self) -> &'static [&'static str] {
        match self {
            Compositor::Sway => &["sway"],
            Compositor::Hyprland => &["Hyprland"],
            Compositor::Niri => &["niri"],
            Compositor::River => &["river"],
            Compositor::Labwc => &["labwc"],
            Compositor::Wayfire => &["Wayfire", "wayfire"],
        }
    }

    /// Binaries in `PATH` that show the compositor is installed.
    pub fn binaries(self) -> &'static [&'static str] {
        match self {
            Compositor::Sway => &["sway"],
            Compositor::Hyprland => &["Hyprland", "hyprland"],
            Compositor::Niri => &["niri"],
            Compositor::River => &["river"],
            Compositor::Labwc => &["labwc"],
            Compositor::Wayfire => &["wayfire"],
        }
    }

    /// A variable the compositor sets for its children — a sign that
    /// the session is currently its own. river has none (only `XDG_CURRENT_DESKTOP`).
    fn session_marker(self) -> Option<&'static str> {
        match self {
            Compositor::Sway => Some("SWAYSOCK"),
            Compositor::Hyprland => Some("HYPRLAND_INSTANCE_SIGNATURE"),
            Compositor::Niri => Some("NIRI_SOCKET"),
            Compositor::River => None,
            Compositor::Labwc => Some("LABWC_PID"),
            Compositor::Wayfire => Some("WAYFIRE_CONFIG_FILE"),
        }
    }
}

/// The syntax of the file the block goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// `~/.config/sway/config`: `exec <shell command>`.
    Sway,
    /// `hyprland.conf` (hyprlang): `exec-once = <command>`.
    HyprConf,
    /// `hyprland.lua`: `hl.on("hyprland.start", …)`.
    HyprLua,
    /// `config.kdl`: `spawn-at-startup "argv0" "argv1" …`.
    NiriKdl,
    /// A shell script (river `init`, labwc `autostart`): `<command> &`.
    Shell,
    /// `wayfire.ini`: a key in the `[autostart]` section.
    WayfireIni,
}

impl Syntax {
    /// The start of a comment line.
    pub fn comment(self) -> &'static str {
        match self {
            Syntax::HyprLua => "--",
            Syntax::NiriKdl => "//",
            _ => "#",
        }
    }
}

// ============================================================================
// The launch line
// ============================================================================

/// Why the line cannot be written into this config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no main settings file, and we must not create it ourselves.
    ConfigMissing,
    /// The file is structured so that it cannot be safely appended to.
    Unsupported,
    /// The command has characters this syntax cannot hold.
    BadCharacters,
    /// The autostart module is disabled in the config (Wayfire).
    PluginOff,
    /// Writing to disk failed.
    Io(String),
}

// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config: the config file doesn't exist yet.
const R_MISSING: &str = n_("its settings file wasn't found");
// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config.
const R_UNSUPPORTED: &str = n_("its settings file is arranged in a way Rexon can't safely add to");
// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config.
const R_CHARS: &str = n_("the launch command has characters this settings file can't hold");
// TRANSLATORS: Reason Rexon couldn't add its startup line to the Wayfire config: its autostart plugin is disabled.
const R_PLUGIN: &str = n_("startup programs are turned off in its settings");
// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config; %1 is the system error.
const R_IO: &str = n_("its settings file couldn't be changed (%1)");
const MSG_NOT_COVERED: &str =
    // TRANSLATORS: %1 is a window manager name (Sway, Hyprland…), %2 is the reason.
    n_("%1 doesn't start programs from the common startup list, and Rexon couldn't add itself to its settings: %2.");
// TRANSLATORS: Manual fallback; %1 is a settings file path, %2 is a line to paste.
const MSG_MANUAL: &str = n_("Add this line to %1 yourself: %2");

impl Refusal {
    /// The reason in the person's words.
    pub fn text(&self) -> String {
        match self {
            Refusal::ConfigMissing => tr(R_MISSING),
            Refusal::Unsupported => tr(R_UNSUPPORTED),
            Refusal::BadCharacters => tr(R_CHARS),
            Refusal::PluginOff => tr(R_PLUGIN),
            Refusal::Io(e) => trf(R_IO, &[e]),
        }
    }
}

/// A character that needs no quotes in the shell. Non-ASCII (Cyrillic in paths)
/// the shell keeps as part of a word.
fn shell_safe(c: char) -> bool {
    c.is_alphanumeric() || "_@%+=:,./-".contains(c)
}

/// One word for `sh -c`: as is or in single quotes.
pub fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(shell_safe) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// argv → a shell command line.
pub fn shell_command(argv: &[String]) -> String {
    argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ")
}

/// A KDL string in quotes.
fn kdl_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The Wayfire key for our record: `rexon_<id>` (no hyphen needed in the key).
fn wayfire_key(id: &str) -> String {
    format!("rexon_{}", id.replace('-', "_"))
}

/// The block body lines: launching `argv` in the syntax `syntax`.
///
/// `id` is needed only by Wayfire — the section keys must differ.
pub fn launch_line(syntax: Syntax, id: &str, argv: &[String]) -> Result<Vec<String>, Refusal> {
    if argv.is_empty() || argv.iter().any(|a| a.contains('\n') || a.contains('\r') || a.contains('\0')) {
        return Err(Refusal::BadCharacters);
    }
    let sh = shell_command(argv);
    Ok(match syntax {
        Syntax::Sway => {
            // sway expands `$name` and removes `\` escaping even before
            // the shell (`unescape_string` for all `exec` arguments).
            if sh.contains('$') || sh.contains('\\') {
                return Err(Refusal::BadCharacters);
            }
            // sway treats `,` outside quotes as a command boundary (the shell quotes
            // `;` anyway) — a word with a comma goes in quotes.
            let words: Vec<String> = argv
                .iter()
                .map(|a| if a.contains(',') { format!("'{}'", a.replace('\'', "'\\''")) } else { shell_quote(a) })
                .collect();
            let sh = words.join(" ");
            // sway unquotes a single quoted word itself, and
            // the shell would split the path by spaces: we wrap it once more.
            if words.len() == 1 && sh.starts_with('\'') {
                if sh.contains('"') {
                    return Err(Refusal::BadCharacters);
                }
                return Ok(vec![format!("exec \"{sh}\"")]);
            }
            vec![format!("exec {sh}")]
        }
        Syntax::HyprConf => {
            // hyprlang: `#` is a comment, `$` is a variable.
            if sh.contains('#') || sh.contains('$') {
                return Err(Refusal::BadCharacters);
            }
            vec![format!("exec-once = {sh}")]
        }
        Syntax::HyprLua => {
            let lua = sh.replace('\\', "\\\\").replace('"', "\\\"");
            vec![format!("hl.on(\"hyprland.start\", function() hl.exec_cmd(\"{lua}\") end)")]
        }
        Syntax::NiriKdl => {
            let args: Vec<String> = argv.iter().map(|a| kdl_string(a)).collect();
            vec![format!("spawn-at-startup {}", args.join(" "))]
        }
        Syntax::Shell => vec![format!("{sh} &")],
        Syntax::WayfireIni => {
            // wf-config: `#` starts a comment, `\` is escaping.
            if sh.contains('#') || sh.contains('\\') {
                return Err(Refusal::BadCharacters);
            }
            vec![format!("{} = {sh}", wayfire_key(id))]
        }
    })
}

// ============================================================================
// The managed block (pure functions over text)
// ============================================================================

const FLAG_NL: &str = " +nl";
const FLAG_NEW: &str = " +new";

fn begin_prefix(syntax: Syntax, id: &str) -> String {
    format!("{} >>> rexon:{id}", syntax.comment())
}

fn begin_line(syntax: Syntax, id: &str, added_nl: bool, new_file: bool) -> String {
    let mut s = begin_prefix(syntax, id);
    if added_nl {
        s.push_str(FLAG_NL);
    }
    if new_file {
        s.push_str(FLAG_NEW);
    }
    s.push_str(" >>> managed by Rexon, edits are overwritten");
    s
}

fn end_line(syntax: Syntax, id: &str) -> String {
    format!("{} <<< rexon:{id} <<<", syntax.comment())
}

/// A found block: the byte boundaries of the start and end lines (the end — after
/// the end marker's newline, if there is one) and the marker flags.
struct Found {
    start: usize,
    /// The start of the body (right after the start marker line).
    body_start: usize,
    /// The start of the end marker line.
    end_line_start: usize,
    end: usize,
    body: String,
    added_nl: bool,
    new_file: bool,
}

/// The start and end of each line (with its `\n`).
fn line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for piece in text.split_inclusive('\n') {
        out.push((pos, pos + piece.len()));
        pos += piece.len();
    }
    out
}

fn find_block(text: &str, syntax: Syntax, id: &str) -> Option<Found> {
    let prefix = begin_prefix(syntax, id);
    let end_marker = end_line(syntax, id);
    let spans = line_spans(text);
    let mut i = 0;
    while i < spans.len() {
        let (s, e) = spans[i];
        let line = text[s..e].trim_end_matches(['\n', '\r']);
        // `rexon:tray` must not catch `rexon:tray-extra`.
        if let Some(rest) = line.strip_prefix(&prefix) {
            if rest.starts_with(' ') {
                let flags = rest.split(" >>>").next().unwrap_or("");
                let added_nl = flags.contains(FLAG_NL.trim());
                let new_file = flags.contains(FLAG_NEW.trim());
                for &(bs, be) in spans.iter().skip(i + 1) {
                    let l = text[bs..be].trim_end_matches(['\n', '\r']);
                    if l == end_marker {
                        let body_start = spans[i].1;
                        return Some(Found {
                            start: s,
                            body_start,
                            end_line_start: bs,
                            end: be,
                            body: text[body_start..bs].to_string(),
                            added_nl,
                            new_file,
                        });
                    }
                }
                return None; // a start marker without an end — we do not touch it
            }
        }
        i += 1;
    }
    None
}

/// Whether the text has the block `id`.
pub fn has_block(text: &str, syntax: Syntax, id: &str) -> bool {
    find_block(text, syntax, id).is_some()
}

/// Where to insert a new block in an existing file.
fn insertion_point(text: &str, syntax: Syntax) -> Result<Option<usize>, Refusal> {
    match syntax {
        Syntax::Shell => {
            // `exec` at the start of a line replaces the shell — nothing after it
            // will run. We go before the first such line.
            for (s, e) in line_spans(text) {
                let l = &text[s..e];
                if l.starts_with("exec ") || l.starts_with("exec\t") || l.trim_end() == "exec" {
                    return Ok(Some(s));
                }
            }
            Ok(None)
        }
        Syntax::HyprLua => {
            // After a top-level `return` Lua accepts no statements.
            if line_spans(text).iter().any(|&(s, e)| {
                let l = &text[s..e];
                l.starts_with("return") && l[6..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_')
            }) {
                return Err(Refusal::Unsupported);
            }
            Ok(None)
        }
        Syntax::WayfireIni => {
            // Inside an already existing `[autostart]` section — right after
            // the header (wf-config is not obliged to merge a second such section).
            // If the header was created by our own block, we go after ITS end marker:
            // otherwise the new block would end up inside the old one and would go away together
            // with it on removal.
            let mut open_end: Option<String> = None;
            let mut header_inside = false;
            for (s, e) in line_spans(text) {
                let l = text[s..e].trim_end_matches(['\n', '\r']);
                let at_end = if text[s..e].ends_with('\n') { Some(e) } else { None };
                if let Some(end) = &open_end {
                    if l == end {
                        if header_inside {
                            return Ok(at_end);
                        }
                        open_end = None;
                    } else if l.trim() == "[autostart]" {
                        header_inside = true;
                    }
                    continue;
                }
                if l.trim() == "[autostart]" {
                    return Ok(at_end); // the header is the last line — to the end
                }
                if let Some(id) = our_block_id(l, syntax) {
                    open_end = Some(end_line(syntax, id));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// The start marker of our block (any) — its `id`.
fn our_block_id(line: &str, syntax: Syntax) -> Option<&str> {
    let head = format!("{} >>> rexon:", syntax.comment());
    let rest = line.strip_prefix(&head)?;
    rest.find(' ').map(|i| &rest[..i])
}

/// A line is a preamble the block carries for the whole file, not for itself:
/// sway — the `include` of the system config in a file we created, labwc —
/// executing the system `autostart`, Wayfire — the `[autostart]` header.
/// While the file has our blocks, it must stay in it.
fn preamble_like(syntax: Syntax, line: &str) -> bool {
    match syntax {
        Syntax::Sway => line.starts_with("include "),
        Syntax::Shell => line.starts_with(". "),
        Syntax::WayfireIni => line.trim() == "[autostart]",
        _ => false,
    }
}

/// Whether `wayfire.ini` has an `[autostart]` section.
fn wayfire_has_section(text: &str) -> bool {
    text.lines().any(|l| l.trim() == "[autostart]")
}

/// The result of a text edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEdit {
    /// Nothing needs changing.
    Unchanged,
    /// The new file text.
    Write(String),
    /// The file was created by us and is now empty — delete it.
    Delete,
}

/// Install/update the block `id` with the body `body` (lines without `\n`).
///
/// `existing` — the current text or `None` if there is no file (then the file
/// is created, and the marker remembers it). `preamble` — lines that go into the block
/// BEFORE the body only when the file is created (sway: an `include` of the system one).
pub fn upsert_text(
    existing: Option<&str>,
    syntax: Syntax,
    id: &str,
    body: &[String],
    preamble: &[String],
) -> Result<TextEdit, Refusal> {
    let mut body_text = String::new();
    let new_file = existing.is_none();
    if new_file {
        for l in preamble {
            body_text.push_str(l);
            body_text.push('\n');
        }
    }
    let wf_new_section = syntax == Syntax::WayfireIni
        && !existing.is_some_and(wayfire_has_section);
    if wf_new_section {
        body_text.push_str("[autostart]\n");
    }
    for l in body {
        body_text.push_str(l);
        body_text.push('\n');
    }

    let text = existing.unwrap_or("");
    if let Some(found) = find_block(text, syntax, id) {
        // An update: the preamble and section header of the old block are its own —
        // only the body lines change, the flags and the block's surroundings stay.
        let old_body = &found.body;
        let mut kept = String::new();
        for l in old_body.split_inclusive('\n') {
            let t = l.trim_end_matches(['\n', '\r']);
            let is_body = body_like(syntax, t);
            if !is_body {
                kept.push_str(l);
            }
        }
        let mut new_body = kept;
        for l in body {
            new_body.push_str(l);
            new_body.push('\n');
        }
        if &new_body == old_body {
            return Ok(TextEdit::Unchanged);
        }
        let mut out = String::with_capacity(text.len() + new_body.len());
        out.push_str(&text[..found.body_start]);
        out.push_str(&new_body);
        out.push_str(&text[found.end_line_start..]);
        return Ok(TextEdit::Write(out));
    }

    let at = if new_file { None } else { insertion_point(text, syntax)? };
    let added_nl = at.is_none() && !text.is_empty() && !text.ends_with('\n');
    let mut block = begin_line(syntax, id, added_nl, new_file);
    block.push('\n');
    block.push_str(&body_text);
    block.push_str(&end_line(syntax, id));
    block.push('\n');

    let out = match at {
        Some(pos) => format!("{}{}{}", &text[..pos], block, &text[pos..]),
        None => {
            let mut s = text.to_string();
            if added_nl {
                s.push('\n');
            }
            s.push_str(&block);
            s
        }
    };
    Ok(TextEdit::Write(out))
}

/// Is the line our launch body (rather than a preamble/section header)?
fn body_like(syntax: Syntax, line: &str) -> bool {
    match syntax {
        Syntax::Sway => line.starts_with("exec "),
        Syntax::HyprConf => line.starts_with("exec-once"),
        Syntax::HyprLua => line.starts_with("hl.on("),
        Syntax::NiriKdl => line.starts_with("spawn-at-startup"),
        Syntax::Shell => line.ends_with(" &"),
        Syntax::WayfireIni => line.starts_with("rexon_"),
    }
}

/// Remove the block `id`: the text becomes byte for byte as before.
pub fn remove_text(existing: &str, syntax: Syntax, id: &str) -> TextEdit {
    let Some(found) = find_block(existing, syntax, id) else {
        return TextEdit::Unchanged;
    };
    let mut start = found.start;
    let mut tail = existing[found.end..].to_string();
    let our_nl = found.added_nl && start > 0 && existing.as_bytes()[start - 1] == b'\n';
    // We created the file, and there is nothing in it before the block.
    let our_file = found.new_file && start == 0;
    if tail.is_empty() {
        if our_nl {
            start -= 1;
        }
    } else if our_nl || our_file {
        // Is there another block of ours after this one (it was installed later — the file already
        // ended with our `\n`)? If we removed the newline, the next block's marker
        // would stick to the person's last line, and a file we created
        // would remain empty instead of being deleted after all blocks were removed.
        // The newline and the file pass to the next block together with
        // the flags: its removal will return the file to its previous bytes.
        // If foreign lines follow the block (the person added them after us), the newline
        // before them is needed, and it stays.
        if let Some(next) = inherit_flags(&tail, syntax, our_nl, our_file) {
            tail = next;
        }
    }
    // The block's preamble (sway: the `include` of the system config) belongs to
    // the file, not to the block: removing it while something remains after the block means
    // disabling the system config together with the keybindings. The next block of ours
    // takes it over (and will go away with it last); foreign lines get
    // it as ordinary lines before them.
    let preamble: String = found
        .body
        .split_inclusive('\n')
        .filter(|l| preamble_like(syntax, l.trim_end_matches(['\n', '\r'])))
        .collect();
    if !preamble.is_empty() && !tail.is_empty() {
        tail = match our_block_id(tail.lines().next().unwrap_or(""), syntax) {
            Some(_) => {
                let first = tail.find('\n').map_or(tail.len(), |i| i + 1);
                let rest = &tail[first..];
                let missing: String = preamble
                    .split_inclusive('\n')
                    .filter(|l| !rest.split_inclusive('\n').any(|r| r.trim_end() == l.trim_end()))
                    .collect();
                let sep = if first == tail.len() && !tail.ends_with('\n') { "\n" } else { "" };
                format!("{}{sep}{missing}{rest}", &tail[..first])
            }
            None => format!("{preamble}{tail}"),
        };
    }
    let out = format!("{}{}", &existing[..start], tail);
    if found.new_file && out.is_empty() {
        return TextEdit::Delete;
    }
    TextEdit::Write(out)
}

/// If `tail` starts with our block's start marker — return `tail` where
/// this marker gets the `add_nl`/`add_new` flags (inherited from the block being
/// removed); otherwise `None`.
fn inherit_flags(tail: &str, syntax: Syntax, add_nl: bool, add_new: bool) -> Option<String> {
    let line_end = tail.find('\n').unwrap_or(tail.len());
    let line = tail[..line_end].trim_end_matches('\r');
    let head = format!("{} >>> rexon:", syntax.comment());
    let rest = line.strip_prefix(&head)?;
    let id = &rest[..rest.find(' ')?];
    let flags = rest[id.len()..].split(" >>>").next().unwrap_or("");
    let nl = flags.contains(FLAG_NL.trim()) || add_nl;
    let new = flags.contains(FLAG_NEW.trim()) || add_new;
    Some(format!("{}{}", begin_line(syntax, id, nl, new), &tail[line.len()..]))
}

// ============================================================================
// Environment and choosing the file
// ============================================================================

/// What is known about the machine and the session. Live — [`SessionEnv::live`]; checks
/// assemble their own with a temporary home directory.
#[derive(Debug, Clone, Default)]
pub struct SessionEnv {
    pub home: String,
    /// `$XDG_CONFIG_HOME` or `~/.config`.
    pub config_home: String,
    /// `$XDG_CONFIG_DIRS` or `/etc/xdg`.
    pub config_dirs: Vec<String>,
    /// The system config directory (`/etc`): sway and niri look there.
    pub sysconf: String,
    /// Components of `XDG_CURRENT_DESKTOP`.
    pub current_desktop: Vec<String>,
    /// Variables read here (session markers and config path
    /// overrides); empty ones are not stored.
    pub vars: std::collections::BTreeMap<String, String>,
    /// Which compositors are installed (the binary in `PATH`).
    pub installed: Vec<Compositor>,
}

/// Variables [`SessionEnv::live`] takes from the process.
const LIVE_VARS: &[&str] = &[
    "SWAYSOCK",
    "HYPRLAND_INSTANCE_SIGNATURE",
    "HYPRLAND_CONFIG",
    "NIRI_SOCKET",
    "NIRI_CONFIG",
    "LABWC_PID",
    "WAYFIRE_CONFIG_FILE",
];

impl SessionEnv {
    /// The live process environment.
    pub fn live() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = get("HOME").unwrap_or_default();
        let config_home = crate::utils::config_home(get("XDG_CONFIG_HOME").as_deref(), &home);
        let config_dirs = get("XDG_CONFIG_DIRS")
            .map(|v| v.split(':').filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_else(|| vec!["/etc/xdg".to_string()]);
        let current_desktop = get("XDG_CURRENT_DESKTOP")
            .map(|v| v.split(':').filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        let vars = LIVE_VARS
            .iter()
            .filter_map(|k| get(k).map(|v| (k.to_string(), v)))
            .collect();
        let path = std::env::var("PATH").ok();
        let installed = Compositor::ALL
            .into_iter()
            .filter(|c| c.binaries().iter().any(|b| crate::utils::which(b, path.as_deref()).is_some()))
            .collect();
        SessionEnv {
            home,
            config_home,
            config_dirs,
            sysconf: "/etc".to_string(),
            current_desktop,
            vars,
            installed,
        }
    }

    fn var(&self, k: &str) -> Option<&str> {
        self.vars.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// Full desktops that execute XDG records themselves (with their own session
/// manager or via systemd). Case-insensitive comparison.
const XDG_DESKTOPS: &[&str] = &[
    "KDE", "GNOME", "X-Cinnamon", "Cinnamon", "XFCE", "MATE", "Budgie", "LXQt", "LXDE",
    "COSMIC", "Pantheon", "Unity", "Deepin", "DDE", "Enlightenment", "Lumina", "UKUI",
];

/// Whose session is running now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentSession {
    /// A full desktop — XDG records are executed.
    FullDesktop,
    /// One of the six compositors.
    Compositor(Compositor),
    /// Not recognized — we judge only by the presence of the `.desktop`, as before.
    Unknown,
}

/// Recognize the session. A full desktop matters more than compositor markers: Budgie
/// and Xfce on Wayland run on top of labwc, LXQt — on top of any of the six, and
/// their autostart is executed by their own session manager.
pub fn current_session(env: &SessionEnv) -> CurrentSession {
    let has_name = |n: &str| env.current_desktop.iter().any(|d| d.eq_ignore_ascii_case(n));
    if XDG_DESKTOPS.iter().any(|n| has_name(n)) {
        return CurrentSession::FullDesktop;
    }
    for c in Compositor::ALL {
        if c.session_marker().is_some_and(|m| env.var(m).is_some())
            || c.desktop_names().iter().any(|n| has_name(n))
        {
            return CurrentSession::Compositor(c);
        }
    }
    CurrentSession::Unknown
}

/// Which file is the compositor's main one and how to edit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: String,
    pub syntax: Syntax,
    /// There is no file. `Some(preamble)` — it may be created with these lines
    /// at the front; `None` for a nonexistent file — it must not be created.
    pub create_with: Option<Vec<String>>,
    pub exists: bool,
}

fn is_file(p: &str) -> bool {
    Path::new(p).is_file()
}

fn join(dir: &str, rel: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), rel)
}

/// The compositor's main file — in the same lookup order as the compositor's own.
pub fn resolve_target(c: Compositor, env: &SessionEnv) -> Target {
    let existing = |path: String, syntax: Syntax| Target { path, syntax, create_with: None, exists: true };
    let missing = |path: String, syntax: Syntax, create_with: Option<Vec<String>>| Target {
        path,
        syntax,
        create_with,
        exists: false,
    };
    match c {
        Compositor::Sway => {
            // sway/config.c `get_config_path`.
            let cands = [
                join(&env.home, ".sway/config"),
                join(&env.config_home, "sway/config"),
                join(&env.home, ".i3/config"),
                join(&env.config_home, "i3/config"),
            ];
            if let Some(p) = cands.iter().find(|p| is_file(p)) {
                return existing(p.clone(), Syntax::Sway);
            }
            let system = [join(&env.sysconf, "sway/config"), join(&env.sysconf, "i3/config")]
                .into_iter()
                .find(|p| is_file(p));
            let preamble = system.map(|s| vec![format!("include {}", crate::themes::sway_path(&s))]);
            missing(join(&env.config_home, "sway/config"), Syntax::Sway, preamble)
        }
        Compositor::Hyprland => {
            if let Some(p) = env.var("HYPRLAND_CONFIG") {
                let syntax = if p.ends_with(".lua") { Syntax::HyprLua } else { Syntax::HyprConf };
                return if is_file(p) { existing(p.to_string(), syntax) } else { missing(p.to_string(), syntax, None) };
            }
            // 0.55+: first `hyprland.lua`, without it — the old `hyprland.conf`.
            let lua = join(&env.config_home, "hypr/hyprland.lua");
            let conf = join(&env.config_home, "hypr/hyprland.conf");
            if is_file(&lua) {
                existing(lua, Syntax::HyprLua)
            } else if is_file(&conf) {
                existing(conf, Syntax::HyprConf)
            } else {
                missing(lua, Syntax::HyprLua, None)
            }
        }
        Compositor::Niri => {
            let p = env
                .var("NIRI_CONFIG")
                .map(str::to_string)
                .unwrap_or_else(|| join(&env.config_home, "niri/config.kdl"));
            if is_file(&p) { existing(p, Syntax::NiriKdl) } else { missing(p, Syntax::NiriKdl, None) }
        }
        Compositor::River => {
            let p = join(&env.config_home, "river/init");
            if is_file(&p) { existing(p, Syntax::Shell) } else { missing(p, Syntax::Shell, None) }
        }
        Compositor::Labwc => {
            let p = join(&env.config_home, "labwc/autostart");
            if is_file(&p) {
                return existing(p, Syntax::Shell);
            }
            // labwc reads the FIRST `autostart` found: our own file would shadow
            // the system one, so the created one first executes it.
            let system = env.config_dirs.iter().map(|d| join(d, "labwc/autostart")).find(|p| is_file(p));
            let preamble = match system {
                Some(s) => vec![format!(". {}", shell_quote(&s))],
                None => Vec::new(),
            };
            missing(p, Syntax::Shell, Some(preamble))
        }
        Compositor::Wayfire => {
            let p = match env.var("WAYFIRE_CONFIG_FILE") {
                Some(p) => p.to_string(),
                None => {
                    let vendored = join(&env.config_home, "wayfire/wayfire.ini");
                    if is_file(&vendored) { vendored } else { join(&env.config_home, "wayfire.ini") }
                }
            };
            if is_file(&p) { existing(p, Syntax::WayfireIni) } else { missing(p, Syntax::WayfireIni, Some(Vec::new())) }
        }
    }
}

/// All files where our block MAY be for the compositor (for removal).
fn all_candidates(c: Compositor, env: &SessionEnv) -> Vec<(String, Syntax)> {
    let mut out = vec![{
        let t = resolve_target(c, env);
        (t.path, t.syntax)
    }];
    let extra: Vec<(String, Syntax)> = match c {
        Compositor::Sway => vec![
            (join(&env.home, ".sway/config"), Syntax::Sway),
            (join(&env.config_home, "sway/config"), Syntax::Sway),
            (join(&env.home, ".i3/config"), Syntax::Sway),
            (join(&env.config_home, "i3/config"), Syntax::Sway),
        ],
        Compositor::Hyprland => vec![
            (join(&env.config_home, "hypr/hyprland.lua"), Syntax::HyprLua),
            (join(&env.config_home, "hypr/hyprland.conf"), Syntax::HyprConf),
        ],
        Compositor::Niri => vec![(join(&env.config_home, "niri/config.kdl"), Syntax::NiriKdl)],
        Compositor::River => vec![],
        Compositor::Labwc => vec![],
        Compositor::Wayfire => vec![
            (join(&env.config_home, "wayfire/wayfire.ini"), Syntax::WayfireIni),
            (join(&env.config_home, "wayfire.ini"), Syntax::WayfireIni),
        ],
    };
    for e in extra {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// Checks of the file itself before editing: a format that cannot be appended to.
fn precheck(c: Compositor, target: &Target, text: Option<&str>) -> Result<(), Refusal> {
    match (c, text) {
        (Compositor::River, Some(t)) => {
            // init is executed as a program: only a shell script can be appended to,
            // and it must remain executable.
            let first = t.lines().next().unwrap_or("");
            let shell = first.starts_with("#!")
                && ["sh", "bash", "dash", "zsh", "ksh", "mksh", "ash"].iter().any(|s| {
                    first.split(|ch: char| ch == '/' || ch.is_whitespace()).any(|w| w == *s)
                });
            if !shell {
                return Err(Refusal::Unsupported);
            }
            Ok(())
        }
        (Compositor::Wayfire, Some(t)) => {
            // The `autostart` module must be in `[core] plugins` if the list
            // is set (without the key the default applies — with it).
            let mut in_core = false;
            let mut lines = t.lines().peekable();
            while let Some(l) = lines.next() {
                let s = l.trim();
                if s.starts_with('[') {
                    in_core = s == "[core]";
                    continue;
                }
                if in_core && s.starts_with("plugins") && s[7..].trim_start().starts_with('=') {
                    let mut value = s.split_once('=').map(|x| x.1).unwrap_or("").to_string();
                    while value.trim_end().ends_with('\\') {
                        match lines.next() {
                            Some(n) => {
                                value.push(' ');
                                value.push_str(n);
                            }
                            None => break,
                        }
                    }
                    if !value.split(|ch: char| ch.is_whitespace() || ch == '\\').any(|w| w == "autostart") {
                        return Err(Refusal::PluginOff);
                    }
                }
            }
            Ok(())
        }
        _ => {
            let _ = target;
            Ok(())
        }
    }
}

// ============================================================================
// File helpers (a shared API for other autostart lines)
// ============================================================================

/// What happened to the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    Written,
    Created,
    Removed,
    Deleted,
}

fn write_file(path: &str, text: &str) -> Result<(), Refusal> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| Refusal::Io(e.to_string()))?;
    }
    crate::utils::atomic_write(path, text).map_err(|e| Refusal::Io(e.to_string()))
}

/// Install/update the managed block `id` in the file `path`.
///
/// `body` — the body lines (ready, in the file's syntax: see [`launch_line`]).
/// `create_with` — `Some(preamble)` if the file may be missing and may be
/// created; `None` — a missing file is the refusal [`Refusal::ConfigMissing`].
/// A repeat with the same body does not touch the file.
pub fn ensure_block(
    path: &str,
    syntax: Syntax,
    id: &str,
    body: &[String],
    create_with: Option<&[String]>,
) -> Result<Change, Refusal> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Refusal::Io(e.to_string())),
    };
    if existing.is_none() && create_with.is_none() {
        return Err(Refusal::ConfigMissing);
    }
    let preamble = create_with.unwrap_or(&[]);
    match upsert_text(existing.as_deref(), syntax, id, body, preamble)? {
        TextEdit::Unchanged => Ok(Change::Unchanged),
        TextEdit::Write(t) => {
            write_file(path, &t)?;
            Ok(if existing.is_none() { Change::Created } else { Change::Written })
        }
        TextEdit::Delete => Ok(Change::Unchanged),
    }
}

/// Remove the block `id` from the file `path`; a file we created that became empty
/// is deleted. No file or no block — [`Change::Unchanged`].
pub fn remove_block(path: &str, syntax: Syntax, id: &str) -> Result<Change, Refusal> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Change::Unchanged),
        Err(e) => return Err(Refusal::Io(e.to_string())),
    };
    match remove_text(&text, syntax, id) {
        TextEdit::Unchanged => Ok(Change::Unchanged),
        TextEdit::Write(t) => {
            write_file(path, &t)?;
            Ok(Change::Removed)
        }
        TextEdit::Delete => {
            std::fs::remove_file(path).map_err(|e| Refusal::Io(e.to_string()))?;
            Ok(Change::Deleted)
        }
    }
}

// ============================================================================
// Autostart lines of Rexon programs
// ============================================================================

/// The result for one compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub compositor: Compositor,
    pub path: String,
    /// `Ok` — the block is in place (written or already there).
    pub result: Result<Change, Refusal>,
    /// A ready line for manual insertion (if it could be composed).
    pub manual_line: Option<String>,
}

/// The result of installing the lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub outcomes: Vec<Outcome>,
}

impl SyncReport {
    /// Compositors in whose configs the block is.
    pub fn covered(&self) -> Vec<Compositor> {
        self.outcomes.iter().filter(|o| o.result.is_ok()).map(|o| o.compositor).collect()
    }

    /// Names for `NotShowIn=`.
    pub fn not_show_in(&self) -> Vec<&'static str> {
        self.covered().into_iter().flat_map(|c| c.desktop_names().iter().copied()).collect()
    }

    /// The refusal for the current session's compositor — a sentence for the person, or
    /// `None` if everything will work in this session.
    pub fn current_refusal(&self, env: &SessionEnv) -> Option<String> {
        let CurrentSession::Compositor(c) = current_session(env) else {
            return None;
        };
        let o = self.outcomes.iter().find(|o| o.compositor == c)?;
        let Err(r) = &o.result else {
            return None;
        };
        let mut msg = trf(MSG_NOT_COVERED, &[&c.display_name(), &r.text()]);
        if let Some(line) = &o.manual_line {
            msg.push(' ');
            msg.push_str(&trf(MSG_MANUAL, &[&o.path, line]));
        }
        Some(msg)
    }
}

/// For which compositors to write the line: those installed on the machine plus the one whose
/// session is running now.
fn targets_for(env: &SessionEnv) -> Vec<Compositor> {
    let mut out: Vec<Compositor> = env.installed.clone();
    if let CurrentSession::Compositor(c) = current_session(env) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Which compositor configs [`sync_command_in`] edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// All compositors installed on the machine and the one whose session is running (lines
    /// of Rexon programs: the tray and others).
    Installed,
    /// Only the CURRENT session's compositor; in a full desktop and in an
    /// unrecognized session — none. In the configs of the other compositors
    /// only a line that is already there (from earlier
    /// enablings) is refreshed — no new one appears. This is how the lines of Conky
    /// widgets are written (Wehrwolfmann's decision of 23.10.2026).
    Current,
    /// Not a single new line: only refresh existing ones (a widget the
    /// current session does not need — "only on another desktop").
    Present,
}

/// Install the launch line `argv` under the marker `id` into all applicable
/// compositor configs (`None` — remove from everywhere).
pub fn sync_command(env: &SessionEnv, id: &str, argv: Option<&[String]>) -> SyncReport {
    sync_command_in(env, id, argv, Scope::Installed)
}

/// [`sync_command`] with a choice of which configs to touch ([`Scope`]). `None`
/// removes the line from everywhere with any `scope`.
pub fn sync_command_in(env: &SessionEnv, id: &str, argv: Option<&[String]>, scope: Scope) -> SyncReport {
    let mut report = SyncReport::default();
    let Some(argv) = argv else {
        remove_everywhere(env, id);
        return report;
    };
    match scope {
        Scope::Installed => {
            let wanted = targets_for(env);
            for c in Compositor::ALL {
                if !wanted.contains(&c) {
                    // Not installed and not current — the block must not be there.
                    let target = resolve_target(c, env);
                    let _ = remove_block(&target.path, target.syntax, id);
                    continue;
                }
                report.outcomes.push(sync_one(env, c, id, argv));
            }
        }
        Scope::Current | Scope::Present => {
            let current = match (scope, current_session(env)) {
                (Scope::Current, CurrentSession::Compositor(c)) => Some(c),
                _ => None,
            };
            let present = compositors_with_block(env, id);
            for c in Compositor::ALL {
                if Some(c) == current || present.contains(&c) {
                    report.outcomes.push(sync_one(env, c, id, argv));
                }
            }
        }
    }
    report
}

/// Install the line `argv` under the marker `id` into the config of ONE compositor
/// `c` (and remove it from its other candidate files).
fn sync_one(env: &SessionEnv, c: Compositor, id: &str, argv: &[String]) -> Outcome {
    let target = resolve_target(c, env);
    let line = launch_line(target.syntax, id, argv);
    let manual_line = line.as_ref().ok().map(|l| l.join("\n"));
    let result = (|| {
        let body = line.clone()?;
        let text = std::fs::read_to_string(&target.path).ok();
        precheck(c, &target, text.as_deref())?;
        ensure_block(&target.path, target.syntax, id, &body, target.create_with.as_deref())
    })();
    // A line that did not go into the main file may have remained in another
    // candidate (the Hyprland format changed) — do not keep two.
    for (p, s) in all_candidates(c, env) {
        if p != target.path {
            let _ = remove_block(&p, s, id);
        }
    }
    Outcome { compositor: c, path: target.path, result, manual_line }
}

/// Compositors in whose configs (any of the candidate files) the block `id` is.
pub fn compositors_with_block(env: &SessionEnv, id: &str) -> Vec<Compositor> {
    Compositor::ALL
        .into_iter()
        .filter(|&c| {
            all_candidates(c, env)
                .iter()
                .any(|(p, s)| std::fs::read_to_string(p).is_ok_and(|t| has_block(&t, *s, id)))
        })
        .collect()
}

/// Remove the block `id` from all candidate files of the compositor `c`.
pub fn remove_from(env: &SessionEnv, c: Compositor, id: &str) {
    for (p, s) in all_candidates(c, env) {
        let _ = remove_block(&p, s, id);
    }
}

/// Remove the block `id` from all possible configs of all six compositors.
pub fn remove_everywhere(env: &SessionEnv, id: &str) -> Vec<(String, Result<Change, Refusal>)> {
    let mut out = Vec::new();
    for c in Compositor::ALL {
        for (p, s) in all_candidates(c, env) {
            let r = remove_block(&p, s, id);
            if r != Ok(Change::Unchanged) {
                out.push((p, r));
            }
        }
    }
    out
}

/// Whether the autostart `id` will work in the CURRENT session if the `.desktop` record
/// (or the unit) is in place. Full desktops and unrecognized sessions — yes;
/// for a compositor — if our block is in its main config, or
/// `session_runs_it()` says this session will execute the record itself after all
/// (for a `.desktop` — `xdg-desktop-autostart.target` is up: uwsm,
/// `niri-session`; for a unit — `graphical-session.target`), and the record is not
/// hidden from it by `NotShowIn=`. `desktop_text` — `None` for a unit.
pub fn works_in_current_session(
    env: &SessionEnv,
    id: &str,
    desktop_text: Option<&str>,
    session_runs_it: impl FnOnce() -> bool,
) -> bool {
    let CurrentSession::Compositor(c) = current_session(env) else {
        return true;
    };
    let target = resolve_target(c, env);
    if let Ok(text) = std::fs::read_to_string(&target.path) {
        if has_block(&text, target.syntax, id) {
            return true;
        }
    }
    let hidden = desktop_text
        .map(|t| not_show_in_of(t).iter().any(|n| c.desktop_names().contains(&n.as_str())))
        .unwrap_or(false);
    !hidden && session_runs_it()
}

/// A live check: whether `xdg-desktop-autostart.target` is up in this session.
pub fn xdg_autostart_target_active() -> bool {
    user_unit_active("xdg-desktop-autostart.target")
}

/// A live check: whether `graphical-session.target` is up (units
/// with `WantedBy=graphical-session.target` wait for it).
pub fn graphical_session_target_active() -> bool {
    user_unit_active("graphical-session.target")
}

fn user_unit_active(unit: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", unit])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ============================================================================
// .desktop: NotShowIn and parsing Exec
// ============================================================================

/// The record's `NotShowIn=` values.
fn not_show_in_of(desktop: &str) -> Vec<String> {
    desktop
        .lines()
        .find_map(|l| l.strip_prefix("NotShowIn="))
        .map(|v| v.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// The record `desktop` with the key `NotShowIn=<names>` (an empty list — without
/// the key). The previous key is replaced; a new one goes right after `Type=`.
pub fn with_not_show_in(desktop: &str, names: &[&str]) -> String {
    let mut out = String::with_capacity(desktop.len() + 64);
    let mut placed = names.is_empty();
    for line in desktop.split_inclusive('\n') {
        if line.starts_with("NotShowIn=") {
            continue;
        }
        out.push_str(line);
        if !placed && line.starts_with("Type=") {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("NotShowIn={};\n", names.join(";")));
            placed = true;
        }
    }
    if !placed {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("NotShowIn={};\n", names.join(";")));
    }
    out
}

/// argv from the `Exec=` value per the Desktop Entry specification: first
/// unescaping the key value (`\s \n \t \r \\`), then parsing
/// quotes (`\"`, `` \` ``, `\$`, `\\` inside) and `%%`; field codes (`%f` …)
/// are dropped. `None` — the line is broken (an unclosed quote).
pub fn exec_argv(exec_value: &str) -> Option<Vec<String>> {
    let mut value = String::with_capacity(exec_value.len());
    let mut chars = exec_value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => value.push(' '),
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('\\') => value.push('\\'),
                Some(o) => {
                    value.push('\\');
                    value.push(o);
                }
                None => value.push('\\'),
            }
        } else {
            value.push(c);
        }
    }
    let mut argv = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut it = value.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    argv.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => match it.next()? {
                            e @ ('"' | '`' | '$' | '\\') => cur.push(e),
                            o => {
                                cur.push('\\');
                                cur.push(o);
                            }
                        },
                        o => cur.push(o),
                    }
                }
            }
            '%' => match it.next() {
                Some('%') => {
                    in_word = true;
                    cur.push('%');
                }
                Some(_) | None => {} // a field code — not an argument
            },
            o => {
                in_word = true;
                cur.push(o);
            }
        }
    }
    if in_word {
        argv.push(cur);
    }
    Some(argv)
}

/// argv from `Exec=` of a `[Desktop Entry]` record.
pub fn desktop_argv(desktop: &str) -> Option<Vec<String>> {
    let mut in_main = false;
    for line in desktop.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_main = t == "[Desktop Entry]";
            continue;
        }
        if in_main {
            if let Some(v) = t.strip_prefix("Exec=") {
                return exec_argv(v).filter(|a| !a.is_empty());
            }
        }
    }
    None
}

/// The result of installing an XDG record together with the compositor lines.
#[derive(Debug, Clone)]
pub struct DesktopMirror {
    /// The final `.desktop` text (with `NotShowIn=` for the covered ones).
    pub desktop: String,
    pub report: SyncReport,
}

/// Compositor lines for the XDG record `desktop` under the marker `id`. Call
/// BEFORE writing the `.desktop` and write [`DesktopMirror::desktop`] to disk.
pub fn mirror_desktop(env: &SessionEnv, id: &str, desktop: &str) -> DesktopMirror {
    let Some(argv) = desktop_argv(desktop) else {
        return DesktopMirror { desktop: desktop.to_string(), report: SyncReport::default() };
    };
    let report = sync_command(env, id, Some(&argv));
    let names = report.not_show_in();
    DesktopMirror { desktop: with_not_show_in(desktop, &names), report }
}

// ============================================================================
// Rexon program markers and the live door for autostart writers
// ============================================================================

/// The Rexon tray (`~/.config/autostart/rexon.desktop`).
pub const ID_TRAY: &str = "tray";
/// The login splash (`rexon-session-splash.desktop`).
pub const ID_SPLASH: &str = "session-splash";
/// Applying default applications (`rexon-appdefaults.desktop`).
pub const ID_APPDEFAULTS: &str = "appdefaults";
/// The wallpaper engine (a systemd unit or `rexon-wallpaper-engine.desktop`).
pub const ID_ENGINE: &str = "wallpaper-engine";

/// All Rexon block markers — for removal when Rexon is uninstalled: program launch
/// lines and session variables ([`ID_SESSION_ENV`]).
pub const REXON_IDS: &[&str] = &[ID_TRAY, ID_SPLASH, ID_APPDEFAULTS, ID_ENGINE, ID_SESSION_ENV];

/// [`mirror_desktop`] on the live machine. Returns the final record text
/// and the refusal for the current session (`None` — it will work here).
pub fn mirror_desktop_live(id: &str, desktop: &str) -> (String, Option<String>) {
    let env = SessionEnv::live();
    let m = mirror_desktop(&env, id, desktop);
    let refusal = m.report.current_refusal(&env);
    (m.desktop, refusal)
}

/// Remove the `id` lines from the configs of all compositors on the live machine.
pub fn remove_live(id: &str) {
    remove_everywhere(&SessionEnv::live(), id);
}

/// [`works_in_current_session`] on the live machine.
pub fn works_live(id: &str, desktop_text: Option<&str>) -> bool {
    works_in_current_session(&SessionEnv::live(), id, desktop_text, xdg_autostart_target_active)
}

/// Refresh the compositor lines from the existing XDG records of Rexon programs.
///
/// The lines are written when the switch is turned on — for the compositors that
/// were installed at that moment. A compositor installed later (or a changed path
/// to Rexon) would otherwise be left without a line until the next click. Called at
/// tray start: unchanged records are not touched by a single byte. The engine unit
/// is not looked at here (that needs `systemctl`) — its lines are installed by
/// the engine switch.
pub fn resync_rexon_live() {
    let env = SessionEnv::live();
    for (id, path) in rexon_autostart_entries(&env) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let m = mirror_desktop(&env, id, &text);
        if m.desktop != text {
            let _ = crate::utils::atomic_write(&path, &m.desktop);
        }
    }
}

/// XDG autostart records of Rexon programs: marker and path. ONE list for
/// refreshing compositor lines and for fixing outdated paths.
pub fn rexon_autostart_entries(env: &SessionEnv) -> Vec<(&'static str, String)> {
    let cfg = &env.config_home;
    vec![
        (ID_TRAY, crate::tray::autostart_path(Some(cfg), &env.home)),
        (ID_SPLASH, crate::session_splash::autostart_path(Some(cfg), &env.home)),
        (ID_APPDEFAULTS, format!("{cfg}/autostart/rexon-appdefaults.desktop")),
        (ID_ENGINE, crate::we_client::autostart_desktop_path(Some(cfg), &env.home)),
    ]
}

/// Fixing a record whose program vanished. `Some((old program, new
/// text))` — the record is outdated and there is something to replace it with; `None` — nothing to fix.
///
/// A live case on 08.10.2026: the owner's `rexon.desktop` and `rexon-appdefaults.desktop`
/// called `/usr/local/bin/rexon` — the path of a long-removed developer
/// install. systemd-xdg-autostart-generator silently skips such records
/// ("executable specified in Exec= does not exist"), and Rexon
/// did not start at login, while the autostart checkbox in the window was lit.
///
/// ONLY the program changes (`argv[0]`); the record's flags (`--tray`,
/// `--apply-defaults`…) and other lines stay as they were.
pub fn repaired_exec(
    desktop: &str,
    new_program: &str,
    program_exists: &dyn Fn(&str) -> bool,
) -> Option<(String, String)> {
    let argv = desktop_argv(desktop)?;
    let old = argv.first()?.clone();
    if program_exists(&old) {
        return None;
    }
    let new_argv = desktop_argv(&format!("[Desktop Entry]\nExec={new_program}\n"))?;
    if new_argv.first() == Some(&old) || !program_exists(new_argv.first()?) {
        return None;
    }
    let mut exec = new_program.to_string();
    for a in &argv[1..] {
        exec.push(' ');
        exec.push_str(&crate::installer::desktop_exec_arg(a));
    }
    let mut in_main = false;
    let mut done = false;
    let mut out = String::with_capacity(desktop.len() + exec.len());
    for line in desktop.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            in_main = t == "[Desktop Entry]";
        }
        if in_main && !done && t.starts_with("Exec=") {
            out.push_str(&format!("Exec={exec}\n"));
            done = true;
            continue;
        }
        if in_main && t.starts_with("TryExec=") {
            // A TryExec with the old path would hide the record just like Exec.
            out.push_str(&format!("TryExec={}\n", new_argv[0]));
            continue;
        }
        out.push_str(line);
    }
    Some((old, out))
}

/// Whether a program exists: an absolute path — an executable file, a name — in `PATH`.
pub fn program_exists_live(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if program.contains('/') {
        return std::fs::metadata(program)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    crate::utils::which(program, std::env::var("PATH").ok().as_deref()).is_some()
}

/// Fix outdated Rexon autostart records on the live machine.
/// `program_for(marker)` — what to launch with now (in `Exec=` format); `None`
/// — nothing to replace with, the record stays and this is written to the log.
pub fn repair_stale_rexon_live(program_for: &dyn Fn(&str) -> Option<String>) {
    let env = SessionEnv::live();
    for (id, path) in rexon_autostart_entries(&env) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(argv) = desktop_argv(&text) else {
            continue;
        };
        if program_exists_live(&argv[0]) {
            continue;
        }
        let Some(program) = program_for(id) else {
            crate::log_warn!(
                "autostart",
                "{path}: программы {} нет, а заменить нечем — запись оставлена как есть",
                argv[0]
            );
            continue;
        };
        match repaired_exec(&text, &program, &program_exists_live) {
            Some((old, fixed)) => match crate::utils::atomic_write(&path, &fixed) {
                Ok(()) => crate::log_info!(
                    "autostart",
                    "{path}: программы {old} нет — запись исправлена на {program}"
                ),
                Err(e) => crate::log_warn!("autostart", "{path}: исправить не удалось: {e}"),
            },
            None => crate::log_warn!(
                "autostart",
                "{path}: программы {} нет, замена {program} тоже не годится — запись оставлена",
                argv[0]
            ),
        }
    }
}

/// Remove ALL Rexon blocks (uninstalling Rexon): launch lines from compositor
/// configs and session variables — also from `~/.xprofile` and
/// `labwc/environment`, which are not in the compositor config list.
pub fn remove_all_rexon(env: &SessionEnv) -> Vec<(String, Result<Change, Refusal>)> {
    let mut out = Vec::new();
    for id in REXON_IDS {
        out.extend(remove_everywhere(env, id));
    }
    for (p, syntax) in session_env_candidates(env) {
        let r = remove_block(&p, syntax, ID_SESSION_ENV);
        if r != Ok(Change::Unchanged) && !out.iter().any(|(q, _)| *q == p) {
            out.push((p, r));
        }
    }
    out
}

/// [`remove_all_rexon`] on the live machine.
pub fn remove_all_rexon_live() {
    remove_all_rexon(&SessionEnv::live());
}

// ============================================================================
// Session environment variables where ~/.config/environment.d is not read
// ============================================================================
//
// Rexon's variables (the Qt style — `QT_QPA_PLATFORMTHEME`/`QT_STYLE_OVERRIDE`,
// the cursor — `XCURSOR_*`, the terminal — `TERMINAL`) live in
// `~/.config/environment.d/rexon-*.conf`. This directory is read only by a session
// that takes its environment from systemd: Plasma, GNOME, COSMIC, niri via
// `niri-session`, Hyprland under uwsm. The others do not see it (desktop check
// of 02.10.2026, D1; checked against the launch scripts on the machine):
//
// | session                            | what it reads                          | what Rexon does                      |
// |------------------------------------|----------------------------------------|--------------------------------------|
// | Cinnamon, MATE, XFCE, Budgie on X11| `~/.xprofile` (Xsession of SDDM, LightDM, GDM) | a block in `~/.xprofile` only for these desktops (`case` on `XDG_CURRENT_DESKTOP`) |
// | XFCE on Wayland (`startxfce4 --wayland`) | `~/.config/xfce4/labwc/environment` | a `KEY=value` block in this file     |
// | Budgie on labwc (`startbudgielabwc`)| `~/.config/budgie-desktop/labwc/environment` | the same                       |
// | labwc by itself                    | `~/.config/labwc/environment`          | the same                             |
// | Hyprland without uwsm              | `env =` in its config                  | an `env =` / `hl.env(…)` block in the main config (the cursor is already in its own block) |
// | sway (`Exec=sway`), river, Wayfire | nothing: their configs have no variables | does NOT take them — an honest caveat in the matrix (`de_customization::condition_note_id`) |
//
// There is one source of truth — the `environment.d` files: these blocks mirror them and
// are rebuilt after every write there ([`sync_session_env`]).
//
// Variables that must not reach Plasma live in Rexon's own directory instead
// ([`crate::themes::DESKTOP_BOUND_ENV_DIR`]); they are mirrored into the same
// blocks, and GNOME and niri get them through a drop-in of their shell's unit
// ([`session_unit_targets`]).
//
// ⚠️ The labwc environment files of XFCE and Budgie are copied by their launch scripts from
// a system template only if the file does not exist yet. If Rexon created the file from its single
// block, the template (the keyboard layout) would no longer arrive, so
// a missing file is first seeded with the template.

/// The marker of the session variables block.
pub const ID_SESSION_ENV: &str = "session-env";

/// X11 desktops that do not read `environment.d` (`XDG_CURRENT_DESKTOP`).
const X11_ENV_DESKTOPS: &str = "*X-Cinnamon*|*MATE*|*XFCE*|*Budgie*";
/// Their session binaries: if at least one exists, `~/.xprofile` is needed.
const X11_ENV_SESSION_BINS: &[&str] = &["cinnamon-session", "mate-session", "xfce4-session", "budgie-desktop"];

/// Variables from `~/.config/environment.d/rexon-*.conf` (files by name,
/// inside — in order; a repeated key — the last one wins).
///
/// Also read: Rexon's desktop-bound directory
/// ([`crate::themes::DESKTOP_BOUND_ENV_DIR`]) — variables that must not go
/// into `environment.d` because it would carry them into Plasma too. Its files
/// come FIRST, so a value in `environment.d` (the newer, shared choice) wins.
pub fn rexon_environment_vars(config_home: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for rel in [crate::themes::DESKTOP_BOUND_ENV_DIR, "environment.d"] {
        for (k, v) in rexon_env_dir_vars(&join(config_home, rel)) {
            out.retain(|(ek, _)| *ek != k);
            out.push((k, v));
        }
    }
    out
}

/// The variables of the `rexon-*.conf` files of one directory (files by name,
/// a repeated key — the last one wins).
fn rexon_env_dir_vars(dir: &str) -> Vec<(String, String)> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("rexon-") && n.ends_with(".conf"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out: Vec<(String, String)> = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(join(dir, &f)) else { continue };
        for (k, v) in parse_environment_d(&text) {
            out.retain(|(ek, _)| *ek != k);
            out.push((k, v));
        }
    }
    out
}

/// The variables that live ONLY in Rexon's desktop-bound directory
/// ([`crate::themes::DESKTOP_BOUND_ENV_DIR`]), not in `environment.d`. A
/// systemd session does not get these from its manager, so
/// [`session_unit_targets`] hands them to its shell's unit.
pub fn desktop_bound_vars(config_home: &str) -> Vec<(String, String)> {
    let shared = rexon_env_dir_vars(&join(config_home, "environment.d"));
    rexon_env_dir_vars(&join(config_home, crate::themes::DESKTOP_BOUND_ENV_DIR))
        .into_iter()
        .filter(|(k, _)| !shared.iter().any(|(s, _)| s == k))
        .collect()
}

// ── systemd sessions: the variable goes to the shell's own unit ─────────────
//
// GNOME and niri (through `niri-session`) take their environment from the
// systemd user manager, which reads `~/.config/environment.d`. A variable kept
// out of it (because Plasma imports that environment too) must reach them some
// other way that stays inside their session. Both run their shell or compositor
// as ONE user unit, and the programs started from its launcher, keybindings
// and app grid are its children and inherit its environment. So `Environment=`
// in a drop-in of exactly that unit:
//   * GNOME: `org.gnome.Shell@wayland.service` / `@x11.service` — a drop-in of the
//     template (`org.gnome.Shell@.service.d/`) covers both instances;
//   * niri: `niri.service` (started by `niri-session` with
//     `systemctl --user --wait start niri.service`).
// The manager's environment (`systemctl --user show-environment`, which
// `startplasma` imports) stays untouched, and no other unit sees the variable.
// Bench of 25.11.2026: an isolated `systemd --user` (bwrap in a delegated
// scope) with these two drop-ins and a Plasma-like unit — the variable was in
// the GNOME and niri units and in their children, NOT in the Plasma unit and
// NOT in `show-environment`.
// ⚠️ What this does not reach: programs started by D-Bus activation or by the
// xdg-autostart generator (they are children of the manager) — e.g. a Qt
// program started from GNOME's terminal, which is D-Bus activated.
//
// COSMIC has no such unit: cosmic-session is started by `start-cosmic` from
// the login screen, and `start-cosmic` sets `QT_QPA_PLATFORMTHEME` itself
// (`cosmic` with CuteCosmic, otherwise `qt5ct` — a key the qt6ct plugin also
// answers to), and cosmic-session takes from the manager only what is still
// unset. So the variable from `environment.d` never won there anyway, and the
// style chosen in the qt6ct config still applies.

/// Units whose drop-in carries the desktop-bound variables, and the program
/// that says the desktop is installed.
pub const SESSION_UNITS: &[(&str, &str)] = &[("org.gnome.Shell@.service", "gnome-shell"), ("niri.service", "niri")];

/// Name of Rexon's drop-in file in `<unit>.d/`.
pub const SESSION_UNIT_DROPIN: &str = "rexon-session-env.conf";

/// Where Rexon's drop-in for `unit` lies.
pub fn session_unit_dropin_path(config_home: &str, unit: &str) -> String {
    join(config_home, &format!("systemd/user/{unit}.d/{SESSION_UNIT_DROPIN}"))
}

/// `Environment="K=V"` with systemd's quoting: `\` and `"` escaped, `%` doubled
/// (it starts a specifier otherwise).
fn systemd_environment_line(k: &str, v: &str) -> String {
    let v = v.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
    format!("Environment=\"{k}={v}\"")
}

/// The drop-ins this machine needs: one per installed desktop of
/// [`SESSION_UNITS`], with the [`desktop_bound_vars`]. No such variables —
/// none (existing ones are then removed by [`sync_session_env`]).
pub fn session_unit_targets(env: &SessionEnv, bound: &[(String, String)], has_bin: &dyn Fn(&str) -> bool) -> Vec<EnvTarget> {
    if bound.is_empty() {
        return Vec::new();
    }
    let mut body = vec!["[Service]".to_string()];
    body.extend(bound.iter().map(|(k, v)| systemd_environment_line(k, v)));
    SESSION_UNITS
        .iter()
        .filter(|(_, bin)| has_bin(bin))
        .map(|(unit, _)| EnvTarget {
            path: session_unit_dropin_path(&env.config_home, unit),
            syntax: Syntax::Shell,
            body: body.clone(),
            seed: None,
            create: true,
        })
        .collect()
}

/// Whether `path` is one of Rexon's unit drop-ins: a change there takes effect
/// only after `systemctl --user daemon-reload`.
pub fn is_session_unit_dropin(path: &str) -> bool {
    path.contains("/systemd/user/") && path.ends_with(&format!(".d/{SESSION_UNIT_DROPIN}"))
}

/// Parsing an `environment.d` file: `KEY=value` lines, `#` comments.
/// The key is only `[A-Za-z0-9_]`, the value without newlines.
pub fn parse_environment_d(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some((k.to_string(), v.trim().trim_matches('"').to_string()))
        })
        .collect()
}

/// Where and what to put as the variables block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvTarget {
    pub path: String,
    pub syntax: Syntax,
    pub body: Vec<String>,
    /// No file — first copy this template (labwc of XFCE and Budgie).
    pub seed: Option<String>,
    /// No file (and no template) — whether it may be created.
    pub create: bool,
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The block body for Hyprland: `XCURSOR_*`/`HYPRCURSOR_*` are already set by the cursor's own block
/// — we do not write them a second time.
fn hypr_env_body(syntax: Syntax, vars: &[(String, String)]) -> Vec<String> {
    vars.iter()
        .filter(|(k, _)| !k.starts_with("XCURSOR_") && !k.starts_with("HYPRCURSOR_"))
        .map(|(k, v)| match syntax {
            Syntax::HyprLua => format!("hl.env({}, {})", lua_string(k), lua_string(v)),
            _ => format!("env = {k},{v}"),
        })
        .collect()
}

/// Where this machine needs Rexon's variables besides `environment.d`.
/// `has_bin` — whether a program is in `PATH`; `is_file` — whether a file exists.
pub fn session_env_targets(
    env: &SessionEnv,
    vars: &[(String, String)],
    has_bin: &dyn Fn(&str) -> bool,
    is_file: &dyn Fn(&str) -> bool,
) -> Vec<EnvTarget> {
    let mut out = Vec::new();
    if vars.is_empty() {
        return out;
    }
    // X11 sessions through a login screen: `~/.xprofile`, only for its own desktops
    // (Plasma and GNOME on X11 read it too — they do not need the variables).
    if X11_ENV_SESSION_BINS.iter().any(|b| has_bin(b)) {
        let mut body = vec![format!("case \"$XDG_CURRENT_DESKTOP\" in"), format!("    {X11_ENV_DESKTOPS})")];
        body.extend(vars.iter().map(|(k, v)| format!("        export {k}={}", shell_quote(v))));
        body.push("        ;;".into());
        body.push("esac".into());
        out.push(EnvTarget {
            path: join(&env.home, ".xprofile"),
            syntax: Syntax::Shell,
            body,
            seed: None,
            create: true,
        });
    }
    // labwc sessions of XFCE and Budgie: each has its own environment file.
    for (rel, sample) in [
        ("xfce4/labwc/environment", "/usr/share/xfce4/labwc/labwc-environment"),
        ("budgie-desktop/labwc/environment", "/usr/share/budgie-desktop/labwc/environment"),
    ] {
        if is_file(sample) {
            out.push(EnvTarget {
                path: join(&env.config_home, rel),
                syntax: Syntax::Shell,
                body: vars.iter().map(|(k, v)| format!("{k}={v}")).collect(),
                seed: Some(sample.to_string()),
                create: true,
            });
        }
    }
    // labwc by itself: `~/.config/labwc/environment` (XFCE and Budgie have
    // their own labwc settings directory — above).
    if env.installed.contains(&Compositor::Labwc) {
        out.push(EnvTarget {
            path: join(&env.config_home, "labwc/environment"),
            syntax: Syntax::Shell,
            body: vars.iter().map(|(k, v)| format!("{k}={v}")).collect(),
            seed: None,
            create: true,
        });
    }
    // Hyprland without uwsm: `env =` in the main config (under uwsm it repeats what
    // it already took from environment.d, no harm done).
    if env.installed.contains(&Compositor::Hyprland) {
        let t = resolve_target(Compositor::Hyprland, env);
        let body = hypr_env_body(t.syntax, vars);
        if !body.is_empty() {
            out.push(EnvTarget { path: t.path, syntax: t.syntax, body, seed: None, create: false });
        }
    }
    out
}

/// All files where the variables block may be (for removal).
fn session_env_candidates(env: &SessionEnv) -> Vec<(String, Syntax)> {
    let mut out = vec![
        (join(&env.home, ".xprofile"), Syntax::Shell),
        (join(&env.config_home, "xfce4/labwc/environment"), Syntax::Shell),
        (join(&env.config_home, "budgie-desktop/labwc/environment"), Syntax::Shell),
        (join(&env.config_home, "labwc/environment"), Syntax::Shell),
    ];
    out.extend(all_candidates(Compositor::Hyprland, env));
    out.extend(SESSION_UNITS.iter().map(|(unit, _)| (session_unit_dropin_path(&env.config_home, unit), Syntax::Shell)));
    out
}

/// Every target: the desktop-bound places of [`session_env_targets`] plus the
/// unit drop-ins of [`session_unit_targets`].
fn all_session_env_targets(env: &SessionEnv, has_bin: &dyn Fn(&str) -> bool) -> Vec<EnvTarget> {
    let vars = rexon_environment_vars(&env.config_home);
    let mut targets = session_env_targets(env, &vars, has_bin, &|p| is_file(p));
    targets.extend(session_unit_targets(env, &desktop_bound_vars(&env.config_home), has_bin));
    targets
}

/// Install the block `id` with the body `body`, replacing the previous one ENTIRELY (a regular
/// block edit keeps lines that do not look like a launch line — for
/// variables these would be repeats).
fn replace_block(path: &str, syntax: Syntax, id: &str, body: &[String], create: bool) -> Result<Change, Refusal> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Refusal::Io(e.to_string())),
    };
    let base = match existing.as_deref() {
        Some(t) => match remove_text(t, syntax, id) {
            TextEdit::Unchanged => Some(t.to_string()),
            TextEdit::Write(x) => Some(x),
            TextEdit::Delete => None,
        },
        None => None,
    };
    if base.is_none() && !create {
        return Err(Refusal::ConfigMissing);
    }
    match upsert_text(base.as_deref(), syntax, id, body, &[])? {
        TextEdit::Write(t) if existing.as_deref() != Some(t.as_str()) => {
            write_file(path, &t)?;
            Ok(if existing.is_none() { Change::Created } else { Change::Written })
        }
        _ => Ok(Change::Unchanged),
    }
}

/// Mirror Rexon's variables from `environment.d` to where sessions
/// without systemd read them (see the table above). No variables — the blocks are removed.
/// The result for each file where something was done.
pub fn sync_session_env(env: &SessionEnv, has_bin: &dyn Fn(&str) -> bool) -> Vec<(String, Result<Change, Refusal>)> {
    let targets = all_session_env_targets(env, has_bin);
    let mut out = Vec::new();
    for t in &targets {
        if !is_file(&t.path) {
            if let Some(sample) = &t.seed {
                if let Ok(text) = std::fs::read_to_string(sample) {
                    if let Err(e) = write_file(&t.path, &text) {
                        out.push((t.path.clone(), Err(e)));
                        continue;
                    }
                }
            }
        }
        out.push((t.path.clone(), replace_block(&t.path, t.syntax, ID_SESSION_ENV, &t.body, t.create)));
    }
    for (p, syntax) in session_env_candidates(env) {
        if targets.iter().any(|t| t.path == p) {
            continue;
        }
        let r = remove_block(&p, syntax, ID_SESSION_ENV);
        if r != Ok(Change::Unchanged) {
            out.push((p, r));
        }
    }
    out
}

/// [`sync_session_env`] on the live machine.
pub fn sync_session_env_live() -> Vec<(String, Result<Change, Refusal>)> {
    let path = std::env::var("PATH").ok();
    let out = sync_session_env(&SessionEnv::live(), &|b| crate::utils::which(b, path.as_deref()).is_some());
    if needs_daemon_reload(&out) {
        // A running user manager keeps the unit files it has loaded: without the
        // reload a drop-in written now would be missed if this manager outlives
        // the session (lingering) and starts the desktop's unit again.
        let spec = crate::utils::RunSpec {
            argv: vec!["systemctl".into(), "--user".into(), "daemon-reload".into()],
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(30),
            ..crate::utils::RunSpec::default()
        };
        let _ = crate::utils::run(&spec, path.as_deref());
    }
    out
}

/// Whether [`sync_session_env`] changed one of the unit drop-ins.
pub fn needs_daemon_reload(results: &[(String, Result<Change, Refusal>)]) -> bool {
    results.iter().any(|(p, r)| is_session_unit_dropin(p) && matches!(r, Ok(c) if *c != Change::Unchanged))
}

/// Paths [`sync_session_env_live`] may touch (captured before
/// writing — for the "Apply" rollback): the targets and all candidates where the variables
/// block is removed.
pub fn session_env_paths_live() -> Vec<String> {
    let env = SessionEnv::live();
    let path = std::env::var("PATH").ok();
    let has = |b: &str| crate::utils::which(b, path.as_deref()).is_some();
    let mut out: Vec<String> = all_session_env_targets(&env, &has).into_iter().map(|t| t.path).collect();
    for (p, _) in session_env_candidates(&env) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Whether writing at this path concerns session variables (`environment.d`
/// or Rexon's desktop-bound directory — both are mirrored).
pub fn is_environment_d_path(path: &str) -> bool {
    path.contains("/environment.d/") || path.contains(&format!("/{}/", crate::themes::DESKTOP_BOUND_ENV_DIR))
}

#[cfg(test)]
mod tests {
    #[test]
    fn outdated_autostart_entry_path_replaced_with_current() {
        let old = "[Desktop Entry]\nType=Application\nNotShowIn=sway;labwc;\nName=Rexon\n\
                   Exec=/usr/local/bin/rexon --tray\nIcon=rexon\n";
        let have = |p: &str| p == "/usr/lib/rexon/rexon-main-window";
        let (was, fixed) =
            super::repaired_exec(old, "/usr/lib/rexon/rexon-main-window", &have).expect("чинится");
        assert_eq!(was, "/usr/local/bin/rexon");
        assert!(fixed.contains("\nExec=/usr/lib/rexon/rexon-main-window --tray\n"), "{fixed}");
        assert!(fixed.contains("NotShowIn=sway;labwc;"), "прочие строки не трогаются");
        // A working record is not touched.
        assert!(super::repaired_exec(&fixed, "/x/other", &|_| true).is_none());
        // A replacement that does not exist either is not written.
        assert!(super::repaired_exec(old, "/nope/rexon", &|_| false).is_none());
        // A path with a space — in quotes, the flags are preserved.
        let ad = "[Desktop Entry]\nExec=/usr/local/bin/rexon --apply-defaults\nTryExec=/usr/local/bin/rexon\n";
        let (_, f) = super::repaired_exec(ad, "\"/opt/My Rexon/rexon-main-window\"", &|p| p.starts_with("/opt/")).unwrap();
        assert!(f.contains("Exec=\"/opt/My Rexon/rexon-main-window\" --apply-defaults\n"), "{f}");
        assert!(f.contains("TryExec=/opt/My Rexon/rexon-main-window\n"), "{f}");
    }

    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn exec_roundtrip_with_desktop_quoting() {
        let path = "/home/u/My Apps/rexon \"x\" $y";
        let exec = format!("{} --tray", crate::installer::desktop_exec_arg(path));
        // Is the key escaped once more in the file? desktop_exec_arg already returns
        // the key value — parsing must return the path as it was.
        assert_eq!(exec_argv(&exec), Some(s(&[path, "--tray"])));
        assert_eq!(exec_argv("rexon %U --x 100%%"), Some(s(&["rexon", "--x", "100%"])));
        assert_eq!(exec_argv("\"unterminated"), None);
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("/usr/bin/rexon"), "/usr/bin/rexon");
        assert_eq!(shell_quote("/home/а/Загрузки/x"), "/home/а/Загрузки/x");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn not_show_in_placed_and_replaced() {
        let d = "[Desktop Entry]\nType=Application\nName=X\nExec=x\n";
        let one = with_not_show_in(d, &["sway", "niri"]);
        assert_eq!(one, "[Desktop Entry]\nType=Application\nNotShowIn=sway;niri;\nName=X\nExec=x\n");
        assert_eq!(with_not_show_in(&one, &[]), d);
        assert_eq!(with_not_show_in(&one, &["niri"]), with_not_show_in(d, &["niri"]));
    }

    #[test]
    fn hyprlua_return_refused() {
        let r = upsert_text(Some("hl.config({})\nreturn cfg\n"), Syntax::HyprLua, "t", &s(&["x"]), &[]);
        assert_eq!(r, Err(Refusal::Unsupported));
        // `returns_x = 1` is not `return`.
        assert!(upsert_text(Some("returns_x = 1\n"), Syntax::HyprLua, "t", &s(&["x"]), &[]).is_ok());
    }

    /// D1 (desktop check of 02.10.2026): Rexon's variables reach
    /// sessions that do not read `environment.d` — and only them.
    #[test]
    fn session_env_reaches_sessions_without_systemd() {
        let dir = std::env::temp_dir().join(format!("rexon-senv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.to_string_lossy().into_owned();
        let cfg = format!("{home}/.config");
        std::fs::create_dir_all(format!("{cfg}/environment.d")).unwrap();
        std::fs::create_dir_all(format!("{cfg}/hypr")).unwrap();
        std::fs::write(format!("{cfg}/environment.d/rexon-qt-style.conf"), "QT_QPA_PLATFORMTHEME=qt6ct\n").unwrap();
        std::fs::write(format!("{cfg}/environment.d/rexon-cursor.conf"), "XCURSOR_THEME=Bibata\nXCURSOR_SIZE=32\n").unwrap();
        std::fs::write(format!("{cfg}/environment.d/other.conf"), "FOO=bar\n").unwrap();
        std::fs::write(format!("{cfg}/hypr/hyprland.conf"), "monitor=,preferred,auto,1\n").unwrap();
        let sample = format!("{home}/sample-env");
        std::fs::write(&sample, "XKB_DEFAULT_LAYOUT=us\n").unwrap();
        let env = SessionEnv {
            home: home.clone(),
            config_home: cfg.clone(),
            sysconf: "/nonexistent".into(),
            installed: vec![Compositor::Hyprland],
            ..Default::default()
        };
        assert_eq!(
            rexon_environment_vars(&cfg),
            [
                ("XCURSOR_THEME".to_string(), "Bibata".to_string()),
                ("XCURSOR_SIZE".to_string(), "32".to_string()),
                ("QT_QPA_PLATFORMTHEME".to_string(), "qt6ct".to_string()),
            ]
        );
        let vars = rexon_environment_vars(&cfg);
        let only_xfce = |b: &str| b == "xfce4-session";
        let targets = session_env_targets(&env, &vars, &only_xfce, &|p| p == "/usr/share/xfce4/labwc/labwc-environment");
        let paths: Vec<&str> = targets.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(
            paths,
            [format!("{home}/.xprofile").as_str(), format!("{cfg}/xfce4/labwc/environment").as_str(), format!("{cfg}/hypr/hyprland.conf").as_str()]
        );
        assert_eq!(
            targets[0].body,
            s(&[
                "case \"$XDG_CURRENT_DESKTOP\" in",
                "    *X-Cinnamon*|*MATE*|*XFCE*|*Budgie*)",
                "        export XCURSOR_THEME=Bibata",
                "        export XCURSOR_SIZE=32",
                "        export QT_QPA_PLATFORMTHEME=qt6ct",
                "        ;;",
                "esac",
            ])
        );
        assert_eq!(targets[1].body, s(&["XCURSOR_THEME=Bibata", "XCURSOR_SIZE=32", "QT_QPA_PLATFORMTHEME=qt6ct"]));
        // Hyprland's cursor is in its own block; here only the rest.
        assert_eq!(targets[2].body, s(&["env = QT_QPA_PLATFORMTHEME,qt6ct"]));
        // Not a single desktop without environment.d — nowhere and no need.
        assert!(session_env_targets(&SessionEnv { installed: vec![], ..env.clone() }, &vars, &|_| false, &|_| false).is_empty());

        // Writing to disk: the .xprofile file is created, the Hyprland config is appended to.
        let r = sync_session_env(&env, &|b| b == "mate-session");
        assert!(r.iter().all(|(_, x)| x.is_ok()), "{r:?}");
        let xp = std::fs::read_to_string(format!("{home}/.xprofile")).unwrap();
        assert!(xp.contains("export QT_QPA_PLATFORMTHEME=qt6ct\n"), "{xp}");
        let hy = std::fs::read_to_string(format!("{cfg}/hypr/hyprland.conf")).unwrap();
        assert!(hy.starts_with("monitor=,preferred,auto,1\n") && hy.contains("\nenv = QT_QPA_PLATFORMTHEME,qt6ct\n"), "{hy}");
        // A repeat — not a byte; a value change — without repeated lines.
        let again = sync_session_env(&env, &|b| b == "mate-session");
        assert!(again.iter().all(|(_, x)| *x == Ok(Change::Unchanged)), "{again:?}");
        std::fs::write(format!("{cfg}/environment.d/rexon-qt-style.conf"), "QT_STYLE_OVERRIDE=kvantum\n").unwrap();
        sync_session_env(&env, &|b| b == "mate-session");
        let hy = std::fs::read_to_string(format!("{cfg}/hypr/hyprland.conf")).unwrap();
        assert!(!hy.contains("QT_QPA_PLATFORMTHEME") && hy.matches("QT_STYLE_OVERRIDE").count() == 1, "{hy}");
        // No variables left — the blocks are removed, the created .xprofile is deleted,
        // the Hyprland config is back byte for byte.
        for f in ["rexon-qt-style.conf", "rexon-cursor.conf"] {
            std::fs::remove_file(format!("{cfg}/environment.d/{f}")).unwrap();
        }
        sync_session_env(&env, &|b| b == "mate-session");
        assert!(!std::path::Path::new(&format!("{home}/.xprofile")).exists());
        assert_eq!(std::fs::read_to_string(format!("{cfg}/hypr/hyprland.conf")).unwrap(), "monitor=,preferred,auto,1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Variables kept out of `environment.d` (Rexon's desktop-bound directory)
    /// still reach the desktop-bound places; a shared value wins over a stale one.
    #[test]
    fn desktop_bound_variables_are_mirrored() {
        let dir = std::env::temp_dir().join(format!("rexon-senv-bound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = dir.to_string_lossy().into_owned();
        std::fs::create_dir_all(format!("{cfg}/rexon/session-env")).unwrap();
        std::fs::create_dir_all(format!("{cfg}/environment.d")).unwrap();
        std::fs::write(format!("{cfg}/rexon/session-env/rexon-qt-style.conf"), "QT_QPA_PLATFORMTHEME=qt6ct\nA=old\n").unwrap();
        std::fs::write(format!("{cfg}/environment.d/rexon-x.conf"), "A=new\n").unwrap();
        assert_eq!(
            rexon_environment_vars(&cfg),
            [("QT_QPA_PLATFORMTHEME".to_string(), "qt6ct".to_string()), ("A".to_string(), "new".to_string())]
        );
        assert!(is_environment_d_path(&format!("{cfg}/rexon/session-env/rexon-qt-style.conf")));
        assert!(!is_environment_d_path(&format!("{cfg}/rexon/other.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GNOME and niri read only the systemd environment: a variable kept out of
    /// `environment.d` reaches them through a drop-in of their shell's unit —
    /// only while it is desktop-bound, only for an installed desktop, and the
    /// drop-in goes away with the variable.
    #[test]
    fn desktop_bound_variables_reach_gnome_and_niri_units() {
        let dir = std::env::temp_dir().join(format!("rexon-senv-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.to_string_lossy().into_owned();
        let cfg = format!("{home}/.config");
        std::fs::create_dir_all(format!("{cfg}/rexon/session-env")).unwrap();
        std::fs::create_dir_all(format!("{cfg}/environment.d")).unwrap();
        std::fs::write(format!("{cfg}/rexon/session-env/rexon-qt-style.conf"), "QT_QPA_PLATFORMTHEME=qt6ct\nX=a%b\\c \"q\" d\n").unwrap();
        // A shared variable is already in the manager's environment — not repeated.
        std::fs::write(format!("{cfg}/environment.d/rexon-cursor.conf"), "XCURSOR_THEME=Bibata\n").unwrap();
        let env = SessionEnv { home: home.clone(), config_home: cfg.clone(), sysconf: "/nonexistent".into(), ..Default::default() };
        let gnome = session_unit_dropin_path(&cfg, "org.gnome.Shell@.service");
        let niri = session_unit_dropin_path(&cfg, "niri.service");
        assert_eq!(gnome, format!("{cfg}/systemd/user/org.gnome.Shell@.service.d/rexon-session-env.conf"));
        assert!(is_session_unit_dropin(&gnome) && is_session_unit_dropin(&niri));
        assert!(!is_session_unit_dropin(&format!("{home}/.xprofile")));

        let bound = desktop_bound_vars(&cfg);
        assert_eq!(
            bound,
            [("QT_QPA_PLATFORMTHEME".to_string(), "qt6ct".to_string()), ("X".to_string(), "a%b\\c \"q\" d".to_string())]
        );
        let targets = session_unit_targets(&env, &bound, &|b| b == "gnome-shell");
        assert_eq!(targets.iter().map(|t| t.path.as_str()).collect::<Vec<_>>(), [gnome.as_str()], "niri is not installed");
        assert_eq!(
            targets[0].body,
            s(&["[Service]", "Environment=\"QT_QPA_PLATFORMTHEME=qt6ct\"", "Environment=\"X=a%%b\\\\c \\\"q\\\" d\""])
        );
        assert!(session_unit_targets(&env, &[], &|_| true).is_empty());

        // On disk: both drop-ins, a valid unit fragment; a repeat changes nothing.
        let both = |b: &str| b == "gnome-shell" || b == "niri";
        let r = sync_session_env(&env, &both);
        assert!(needs_daemon_reload(&r), "{r:?}");
        for p in [&gnome, &niri] {
            let text = std::fs::read_to_string(p).unwrap();
            assert!(text.contains("\n[Service]\nEnvironment=\"QT_QPA_PLATFORMTHEME=qt6ct\"\n"), "{text}");
            assert!(text.lines().all(|l| l.is_empty() || l.starts_with('#') || l.starts_with('[') || l.starts_with("Environment=")), "{text}");
        }
        let again = sync_session_env(&env, &both);
        assert!(!needs_daemon_reload(&again), "{again:?}");

        // The variable moved to the shared file (no own-Qt desktop any more) —
        // the manager carries it, the drop-ins are deleted.
        std::fs::remove_file(format!("{cfg}/rexon/session-env/rexon-qt-style.conf")).unwrap();
        std::fs::write(format!("{cfg}/environment.d/rexon-qt-style.conf"), "QT_QPA_PLATFORMTHEME=qt6ct\n").unwrap();
        assert!(desktop_bound_vars(&cfg).is_empty());
        let r = sync_session_env(&env, &both);
        assert!(needs_daemon_reload(&r), "{r:?}");
        assert!(!std::path::Path::new(&gnome).exists() && !std::path::Path::new(&niri).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hyprland in Lua: `hl.env("K", "v")`.
    #[test]
    fn session_env_hyprland_lua_syntax() {
        let vars = vec![("QT_QPA_PLATFORMTHEME".to_string(), "qt6ct".to_string())];
        assert_eq!(hypr_env_body(Syntax::HyprLua, &vars), s(&["hl.env(\"QT_QPA_PLATFORMTHEME\", \"qt6ct\")"]));
    }
}
