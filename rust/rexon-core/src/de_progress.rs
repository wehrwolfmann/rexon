//! A port of `rexon/qt_main/de_progress.py`: showing the progress of desktop
//! environment operations (installing, switching, removing).
//!
//! The dialog itself stays in Qt: the bar, the title, the current package
//! line, the log and the close button. Here lives everything that decides
//! WHAT to put in them: stage names, parsing the completed fraction, where a
//! message goes and with which mark, and what the dialog becomes on
//! completion. Exactly this will also be needed by the widget tab and the
//! future QML, so there is a single decision engine (rule #12).
//!
//! Labels go through the translation catalog (`crate::i18n`): the source
//! language is English, Russian lives in `po/ru.po`.

use crate::i18n::{n_, tr, trf};

/// Stage titles. A stage that is not listed here does NOT change the title —
/// that is how `warning` and any unknown stage behave.
///
/// This is the first line of the progress window and the customer reads it,
/// so it is plain language without jargon ("defaults", "lock", "MIME",
/// "Display manager", "backup"). Technology names (KDE Plasma, AUR) stay in
/// Latin script, as everywhere in the product. The old titles were rewritten
/// on 13.09.2026; the Python path takes them from here too, through the bridge.
///
/// The second column is the catalog `msgid`; it is translated in [`stage_title`].
pub const STAGE_TITLES: &[(&str, &str)] = &[
    ("snapshot", n_("Remembering your default apps…")),
    ("plasma_backup", n_("Saving a copy of the KDE Plasma panel…")),
    ("config_backup", n_("Saving a copy of your desktop settings…")),
    ("wait_lock", n_("Waiting for another package installation to finish…")),
    ("install", n_("Installing packages…")),
    ("done_main", n_("Main packages installed")),
    ("aur", n_("Installing packages from the AUR…")),
    ("aur_done", n_("AUR packages installed")),
    ("aur_warn", n_("Some AUR packages failed to install")),
    ("post_install", n_("Finishing setup…")),
    ("dm_enable", n_("Setting up the login screen…")),
    ("switcher", n_("Setting up default apps…")),
    ("restore", n_("Restoring your default apps…")),
    ("mime", n_("Choosing which apps open your files…")),
    ("remove", n_("Removing packages…")),
    ("done", n_("Finished")),
    ("error", n_("Error")),
];

/// The stage title in the interface language, if the stage has one.
pub fn stage_title(stage: &str) -> Option<String> {
    STAGE_TITLES
        .iter()
        .find(|(k, _)| *k == stage)
        .map(|(_, v)| tr(v))
}

/// The largest bar value: the completed fraction is spread over 0..1000.
pub const BAR_MAX: i32 = 1000;

/// The dialog state before the first message.
pub fn initial_status() -> String {
    tr("Preparing…")
}
/// The close button label while the operation is running.
pub fn close_text_busy() -> String {
    tr("Please wait…")
}
/// The close button label when everything is over.
pub fn close_text_done() -> String {
    tr("Close")
}
/// The title on successful completion.
pub fn status_done() -> String {
    tr("Finished ✅")
}
/// The title on failure.
pub fn status_failed() -> String {
    tr("Failed ❌")
}
/// The summary in the log when there is nothing substantial to say.
pub fn log_done_plain() -> String {
    format!("\n{}", tr("✅ Done"))
}
/// A success message that counts as empty.
pub const MSG_OK: &str = "ok";

/// The stage whose message goes to the log WITHOUT a mark and without trimming
/// whitespace at the ends: raw package manager output.
pub const STAGE_RAW: &str = "raw";
/// The stage whose message is shown as the current package line.
pub const STAGE_PKG_INSTALL: &str = "pkg_install";
/// Stages whose message also goes to the current package line, but only when
/// it is non-empty.
pub const STAGES_CURRENT_IF_SET: &[&str] = &["download", "check"];
/// The stage whose message is marked as an error in the log.
pub const STAGE_ERROR: &str = "error";
/// The stage whose message is marked as a warning in the log.
pub const STAGE_WARNING: &str = "warning";

/// The mark before an error message in the log.
pub const PREFIX_ERROR: &str = "❌ ";
/// The mark before a warning in the log.
pub const PREFIX_WARNING: &str = "⚠️ ";

/// What to change in the dialog for a single progress message.
///
/// `None` means "leave alone": for the bar it is a fraction that cannot be
/// shown, for the others it means this stage does not concern them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgressUpdate {
    /// The new bar value, 0..=[`BAR_MAX`].
    pub bar: Option<i32>,
    /// The new title.
    pub status: Option<String>,
    /// The new current package line; an empty string means "clear".
    pub current: Option<String>,
    /// A line appended to the log as a separate paragraph.
    pub log_line: Option<String>,
}

/// The bar value for a completed fraction.
///
/// The fraction is truncated toward zero, as the integer conversion in the
/// original does. A value outside 0..=[`BAR_MAX`] does not move the bar — Qt
/// silently discarded it too — so we return `None`.
///
/// Warning — FIXED (rule #16). An infinite fraction crashed the dialog: the
/// original caught only `TypeError`/`ValueError`, while converting infinity
/// to an integer raises `OverflowError`. Had such a value come from a
/// "done/total" computation with a zero denominator, the installation display
/// would have been cut off in the middle of the work. Now infinity and "not a
/// number" behave the same: the bar does not move.
///
/// The finiteness check is kept explicit, even though by itself it does not
/// affect the result: integer conversion in Rust does not overflow but
/// saturates at the type's limit, and such a value does not get into the bar
/// anyway. That is exactly why mutation testing cannot catch this line — it
/// is here as an explanation, not as protection.
pub fn bar_value(fraction: Option<f64>) -> Option<i32> {
    let f = fraction?;
    if !f.is_finite() {
        return None;
    }
    let value = (f * 1000.0) as i32;
    (0..=BAR_MAX).contains(&value).then_some(value)
}

/// A message that already carries a mark goes to the log as is.
fn return_log(message: &str) -> Option<String> {
    Some(message.to_string())
}

/// Parses a progress message.
pub fn on_progress(stage: &str, message: &str, fraction: Option<f64>) -> ProgressUpdate {
    let mut update = ProgressUpdate {
        bar: bar_value(fraction),
        status: stage_title(stage),
        ..Default::default()
    };

    if stage == STAGE_PKG_INSTALL {
        // An empty message here WIPES the current package line — the original set it
        // without checking.
        update.current = Some(message.to_string());
    } else if STAGES_CURRENT_IF_SET.contains(&stage) && !message.is_empty() {
        update.current = Some(message.to_string());
    }

    update.log_line = if stage == STAGE_RAW {
        let text = message.trim();
        (!text.is_empty()).then(|| text.to_string())
    } else if message.is_empty() {
        None
    } else {
        let prefix = match stage {
            STAGE_ERROR => PREFIX_ERROR,
            STAGE_WARNING => PREFIX_WARNING,
            _ => "",
        };
        // Warning: the mark is added ONCE. Parsing the package manager output
        // (`system_plan::parse_pacman_line`) already builds the message with a
        // cross/exclamation mark, and blind concatenation produced
        // "❌ ❌ failed to commit transaction" in the log — a double cross where
        // there is only one problem. Caught by showing the whole log while
        // analyzing the Budgie and LXQt installations.
        if !prefix.is_empty() && message.starts_with(prefix.trim_end()) {
            return_log(message)
        } else {
            Some(format!("{prefix}{message}"))
        }
    };

    update
}

/// What the dialog becomes when the operation has finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneUpdate {
    /// The new bar value; on failure it stays as it was.
    pub bar: Option<i32>,
    pub status: String,
    /// The new current package line; on completion it is always cleared.
    pub current: Option<String>,
    /// A line for the log; on failure nothing is appended — the error text has
    /// already been shown by the `error` stage, which the worker ensures
    /// (see [`worker_error_emit`]).
    pub log_line: Option<String>,
    pub close_text: String,
    pub close_enabled: bool,
}

/// Parses the completion of an operation.
pub fn on_done(ok: bool, msg: &str) -> DoneUpdate {
    if ok {
        let log_line = if !msg.is_empty() && msg != MSG_OK {
            format!("\n✅ {msg}")
        } else {
            log_done_plain()
        };
        DoneUpdate {
            bar: Some(BAR_MAX),
            status: status_done(),
            // The current step line under the summary is cleared on success too:
            // otherwise the last raw package manager line ("looking for conflicting
            // packages...", live River installation 24.10.2026) kept hanging under
            // "Finished ✅". The summary tells the status.
            current: Some(String::new()),
            log_line: Some(log_line),
            close_text: close_text_done(),
            close_enabled: true,
        }
    } else {
        DoneUpdate {
            bar: None,
            status: status_failed(),
            current: Some(String::new()),
            log_line: None,
            close_text: close_text_done(),
            close_enabled: true,
        }
    }
}

/// Whether the executor should announce the error itself when the operation
/// has finished.
///
/// The original's rule: if the operation failed, nobody announced an error
/// along the way, and there is something to say, announce it ourselves so
/// that the text is sure to reach the log through the single path. Returns
/// the message for the [`STAGE_ERROR`] stage.
pub fn worker_error_emit(ok: bool, error_seen: bool, msg: &str) -> Option<String> {
    (!ok && !error_seen && !msg.is_empty()).then(|| msg.to_string())
}

// ── Confirming a desktop environment installation ──────────────────────────
//
// `confirm_de_install` is the ONLY place where the tabs ask "shall we
// install?". The decision "may it be installed" itself lives in
// [`crate::distro_compat`]; what has been ported here is what was done with
// that decision afterwards and lay as a second copy in the Qt shim: which of
// the three dialogs to show and what to write in it.

/// The title of the dialog that refuses the installation.
pub fn confirm_blocked_title() -> String {
    tr("Can't Install")
}
/// The label of the only button in the refusal dialog. Not "OK": the person
/// is being told news, not asked for consent (rule #13).
pub fn confirm_blocked_button() -> String {
    tr("Got It")
}
/// The note added when compatibility could not be checked at all.
///
/// Staying silent is not allowed here: "unknown" is not "allowed". We do not
/// forbid the installation (otherwise Rexon would be useless on an unknown
/// distribution), but we honestly warn that there will be no warnings.
pub fn confirm_unknown_note() -> String {
    tr("Couldn't check whether your distribution is compatible: the Rexon core \
        isn't available. You can still install, but you won't be warned about \
        your distribution's quirks.")
}

/// The check outcome, as returned by `distro_compat.explain_de_install`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallVerdict {
    /// Whether the installation is allowed.
    pub allowed: bool,
    /// `"ok"` / `"warning"` / `"blocked"`.
    pub outcome: String,
    /// The reason title in the person's language.
    pub title: String,
    /// The explanation: what is in the way and what to do about it. Line breaks are `\n`.
    pub explanation: String,
}

/// Which dialog to show before installing a desktop environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPlan {
    /// A refusal: a dialog WITHOUT an "install" button. Offering to press a button
    /// that is known to lead to an error is the same as not warning at all.
    Blocked {
        window_title: String,
        /// The dialog's heading text (simple HTML).
        text: String,
        /// The detailed explanation (plain text, `\n` line breaks).
        informative: String,
        button: String,
    },
    /// The question "shall we install?". The warning (if any) is included IN THE
    /// SAME dialog rather than in a second dialog right after it: chains of
    /// dialogs are inconvenient in Big Picture mode (architectural rule #16).
    Ask {
        /// The question body, simple HTML (`<b>`, `<br>`).
        body: String,
    },
}

/// What to show before installation: a port of the `confirm_de_install` branching.
///
/// `verdict` = `None` means "the check failed": the question is asked, but
/// with the note [`confirm_unknown_note`]. `question` is the tab's question
/// text (simple HTML).
pub fn confirm_install_plan(verdict: Option<&InstallVerdict>, question: &str) -> ConfirmPlan {
    match verdict {
        Some(v) if !v.allowed => ConfirmPlan::Blocked {
            window_title: confirm_blocked_title(),
            text: format!("<b>{}</b>", v.title),
            informative: v.explanation.clone(),
            button: confirm_blocked_button(),
        },
        None => ConfirmPlan::Ask {
            body: format!("{}<br><br>{question}", confirm_unknown_note()),
        },
        Some(v) if v.outcome == "warning" => ConfirmPlan::Ask {
            body: format!(
                "<b>{}</b><br><br>{}<br><br>{question}",
                v.title,
                v.explanation.replace('\n', "<br>")
            ),
        },
        Some(_) => ConfirmPlan::Ask {
            body: question.to_string(),
        },
    }
}

// ── AeroShell appearance has fallen behind the updated system ──────────────
//
// Appearance packages are built for the desktop version that was installed
// at build time. When the system is updated, the appearance stops working
// SILENTLY: the taskbar, the menu, button highlighting and transparency
// disappear, and in a severe case the lock screen does not open and the
// person sees a black screen.
//
// The offer to repair is shown by two places — the main window at startup
// and a button in the settings — so the texts and the state parsing live in
// one place (rule #12).

/// The title of the dialog offering to repair the appearance.
pub fn aeroshell_title() -> String {
    tr("Desktop Appearance Stopped Working")
}
/// The main text of the offer (simple HTML).
pub fn aeroshell_text() -> String {
    tr("<b>After a system update, your desktop appearance no longer fits the \
        new version of the desktop.</b>")
}
/// The title of the same dialog when the appearance still works but was built
/// for the previous system version (`aeroshell::Report::at_risk_only`).
pub fn aeroshell_risk_title() -> String {
    tr("Desktop Appearance Needs Rebuilding")
}
/// The main text of the dialog when there is only a risk (simple HTML).
pub fn aeroshell_risk_text() -> String {
    tr("<b>After a system update, your desktop appearance is still built for \
        the previous version of the system. It works for now, but it can fail \
        at any moment.</b>")
}
/// The detailed explanation: what the danger is and what Rexon will do.
pub fn aeroshell_informative() -> String {
    tr("Because of this, the taskbar, the Start menu, button highlights and \
        window transparency may disappear, and the lock screen may stop \
        opening.<br><br>Rexon will rebuild the appearance for the new version. \
        This takes a few minutes; you'll see the progress in a window, and at \
        the end you'll need to log out and log back in.")
}
/// The label of the consent button.
pub fn aeroshell_fix_button() -> String {
    tr("🛠️ Repair")
}
/// The label of the same button when there is only a risk: the dialog says
/// "needs rebuilding", and the button names the same action in the same words.
pub fn aeroshell_rebuild_button() -> String {
    tr("🛠️ Rebuild")
}
/// The label of the decline button. Never start the repair automatically:
/// the build takes minutes and loads the machine, which is unacceptable in
/// the middle of work.
pub fn aeroshell_skip_button() -> String {
    tr("Not Now")
}
/// The title of the repair progress dialog.
pub fn aeroshell_run_title() -> String {
    tr("Repairing Desktop Appearance")
}
/// The title of the success dialog.
pub fn aeroshell_ok_title() -> String {
    tr("Desktop Appearance Repaired")
}
/// The text of the success dialog.
pub fn aeroshell_ok_text() -> String {
    tr("The appearance has been rebuilt for the current system version.\
        <br><br><b>Log out and log back in</b> to get the panel, menu and \
        effects back.")
}
/// The title of the failure dialog.
pub fn aeroshell_fail_title() -> String {
    tr("Couldn't Repair")
}
/// The note added to the builder output on failure: the main thing is that the system is intact.
pub fn aeroshell_fail_note() -> String {
    tr("Your appearance was left as it was — your system is fine. Try again later.")
}

/// The state of a single appearance package in the comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AeroshellPackage {
    pub package: String,
    /// The desktop version the package was built for (empty means not specified).
    pub built_for: String,
    /// The version installed now (empty means it could not be determined).
    pub installed: String,
    /// Whether the package has fallen behind.
    pub stale: bool,
}

/// Comparison details for the dialog's expandable block: a port of
/// `_aeroshell_details`.
///
/// Package names and versions mean nothing to a customer, so they are not in
/// the main text but under "Details".
pub fn aeroshell_details(packages: &[AeroshellPackage]) -> String {
    packages
        .iter()
        .map(|item| {
            if !item.built_for.is_empty() && !item.installed.is_empty() {
                let state = if item.stale { tr("outdated") } else { tr("fine") };
                trf(
                    // TRANSLATORS: %1 is a package name, %2 and %3 are desktop
                    // versions, %4 is “outdated” or “fine”.
                    "%1: built for %2, installed %3 — %4",
                    &[&item.package, &item.built_for, &item.installed, &state],
                )
            } else {
                trf("%1: build version not specified", &[&item.package])
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// The result of repairing the appearance: the dialog title and text (simple HTML).
///
/// `escaped_output` is the builder output ALREADY escaped by the shim: it is
/// the raw text of a foreign program, and without escaping it breaks the
/// dialog markup.
pub fn aeroshell_result(ok: bool, escaped_output: &str) -> (String, String) {
    if ok {
        (aeroshell_ok_title(), aeroshell_ok_text())
    } else {
        (
            aeroshell_fail_title(),
            format!("{escaped_output}<br><br>{}", aeroshell_fail_note()),
        )
    }
}
