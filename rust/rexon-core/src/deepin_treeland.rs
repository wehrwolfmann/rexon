//! "Deepin on Treeland (experimental)" is the ONLY Deepin that Rexon installs
//! (Wehrwolfmann's decision, 26.10.2026), on Arch and derivatives.
//!
//! WHY. The regular Deepin set pulls in an X11 session: `deepin-session` hard
//! depends on `deepin-kwin`, a KWin fork that declares
//! `Conflicts With: kwin kwin-x11` (hundreds of shared files) and cannot be
//! installed next to KDE Plasma. But `deepin-kwin` is needed ONLY by the X11
//! session: it is started only by `dde-session@x11.service` (condition
//! `XDG_SESSION_TYPE=x11`). In a Treeland session (Deepin's Wayland
//! compositor) it is not started at all. So Rexon installs Deepin in a single
//! way, with or without Plasma on the machine: it builds `deepin-session` from
//! the official Arch recipe under its own name [`LOCAL_PACKAGE`] — with
//! `deepin-kwin` among the optional dependencies and without the X11 entry on
//! the login screen — and installs the Deepin set without `deepin-kwin`.
//! Rexon never installs the regular Deepin (X11) under any circumstances.
//! Deepin's own login screen (`ddm`) is installed, because `treeland` cannot
//! be installed without it, but it is NOT enabled (the login screen must not
//! be changed silently: it is shared by all desktops).
//!
//! Warning — LOGIN ONLY THROUGH DDM. The earlier assumption "the 'Treeland
//! User' entry (`treeland-user.desktop`) works under any login screen (SDDM,
//! Plasma Login Manager)" is WRONG since Treeland 0.11: in user mode Treeland
//! creates the person's session only on DDM messages or for the `ddm`
//! service's session, and under another login screen only a bare background
//! without a panel is visible (dde-shell waits for a socket that never
//! appears; linuxdeepin/treeland#1656, 15.11.2026). That is why the texts
//! below say honestly: to log into Deepin, DDM must become the default login
//! screen (the "Login manager" section,
//! [`crate::system_plan::DESKTOP_DM_RULES`]); a warning is shown there too if
//! Deepin is installed but the login screen is a different one.
//!
//! THE REGULAR DEEPIN IS ALREADY INSTALLED (`deepin-session` from the
//! repository). Rexon does not remove it by itself, and "Repair" does not
//! reinstall it (a refusal before the password, [`install_verdict`]); the
//! card offers to switch to the Treeland variant ([`Offer::Install`]): the
//! same build, installed by replacing the installed `deepin-session`
//! (`pacman -U --ask=4`), with `deepin-kwin` removed by the same command.
//!
//! UPDATES — the decision and why. A package under the Arch name
//! (`deepin-session` of the same version) would be "updated" by the very
//! first Arch update with a new version back to the Arch recipe, which would
//! again require `deepin-kwin`: pacman would ask "remove kwin?" (the default
//! answer "no" aborts the WHOLE system update, the answer "yes" wipes KDE
//! Plasma). `IgnorePkg` in `pacman.conf` avoids this, but edits someone
//! else's system file and leaves the person with an eternal "ignoring package
//! upgrade". A pacman hook cannot rebuild a package inside a transaction (the
//! database lock, building not as root, and pacman finds the conflict before
//! the hooks run). Therefore:
//!
//! * our own name [`LOCAL_PACKAGE`] with `provides=deepin-session=<version>`
//!   and `conflicts=deepin-session`: `-Syu` never touches it (no such name
//!   exists in the repositories), and `treeland` and `deepin-session-shell`,
//!   which need `deepin-session`, are satisfied by `provides`;
//! * the version is exactly the one in the repository (`pkgrel` is taken from
//!   it: CachyOS rebuilds with `1.1`);
//! * the build does not update itself, and two places say so: a pacman hook
//!   inside the package itself (after every transaction it compares its
//!   version with `deepin-session` in the repository and prints that it is
//!   time to rebuild) and the Deepin card in Rexon (the "Update build"
//!   button). Until it is rebuilt, the Treeland session runs on the previous
//!   build; Plasma is not affected in any way.
//!
//! Removal is the regular Deepin removal: our package stays in the `deepin`
//! group, and `pacman -Qg deepin` returns it together with the others; the
//! hook goes away with the package, `pacman.conf` is not touched.
//!
//! Only Arch and only a mutable system, as with any rebuild
//! ([`crate::repo_rebuild::rebuild_ctx_live`]).

use std::sync::Arc;

use crate::i18n::{tr, trf};
use crate::repo_rebuild::{self, RecipeEdit};
use crate::system::PkgFamily;
use crate::system_de_exec::{CancelToken, DeOpOutcome, DeOpSink};
use crate::system_plan::{self, Cmd, InstallPreview, PreviewAnswer};

/// A command with output under LANG=C (it is parsed).
fn cmd(argv: Vec<String>, timeout_secs: Option<u64>) -> Cmd {
    Cmd { argv, timeout_secs, lang_c: true }
}

/// The desktop in the core database that the variant belongs to.
pub const DE_ID: &str = "deepin";
/// The Deepin group in the Arch repositories.
pub const GROUP: &str = "deepin";
/// The Arch package that is rebuilt.
pub const SESSION_PACKAGE: &str = "deepin-session";
/// The name it is installed under.
pub const LOCAL_PACKAGE: &str = "deepin-session-treeland";
/// The window manager of the X11 session, the one that conflicts with KWin.
/// Rexon never installs it.
pub const X11_WM_PACKAGE: &str = "deepin-kwin";
/// The "Deepin" (X11) entry on the login screen: it does not work without `deepin-kwin`.
const X11_SESSION_FILE: &str = "usr/share/xsessions/deepin.desktop";
const HOOK_FILE: &str = "usr/share/libalpm/hooks/90-deepin-session-treeland.hook";
const CHECK_FILE: &str = "usr/share/libalpm/scripts/deepin-session-treeland-check";

// ── Editing the recipe ──────────────────────────────────────────────────

/// A string for `printf` inside `sh` double quotes.
fn dq(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '"' | '$' | '`' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The pacman hook inside the package itself: after every installation and
/// update it checks whether the repository has overtaken our build.
pub fn hook_text() -> String {
    format!(
        "[Trigger]\n\
         Operation = Install\n\
         Operation = Upgrade\n\
         Type = Package\n\
         Target = *\n\
         \n\
         [Action]\n\
         Description = Checking the local {LOCAL_PACKAGE} build against {SESSION_PACKAGE} in the repositories...\n\
         When = PostTransaction\n\
         Exec = /{CHECK_FILE}\n"
    )
}

/// The check itself. The texts are in Rexon's language at build time (they
/// are read by whoever built it).
pub fn check_script() -> String {
    let line1 = dq(&trf(
        "Rexon: the repositories have %1 %2, and your Treeland build of it (without deepin-kwin) is still %3.",
        &[&SESSION_PACKAGE, &"%REPO%", &"%LOCAL%"],
    ))
    .replace("%REPO%", "${repo_ver}")
    .replace("%LOCAL%", "${local_ver}");
    let line2 = dq(&tr(
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        "The Treeland session keeps working on the old build. To update it, open Rexon → Desktops → Deepin → “Update the Treeland build”.",
    ));
    format!(
        "#!/bin/sh\n\
         # Rexon: {LOCAL_PACKAGE} is {SESSION_PACKAGE} built from the Arch recipe without {X11_WM_PACKAGE}\n\
         # (Deepin on Treeland). It is never upgraded by pacman -Syu,\n\
         # so this hook says when the repositories move ahead of it.\n\
         local_ver=$(pacman -Q {LOCAL_PACKAGE} 2>/dev/null) || exit 0\n\
         local_ver=${{local_ver#* }}\n\
         repo_ver=$(LC_ALL=C pacman -Si {SESSION_PACKAGE} 2>/dev/null | sed -n 's/^Version *: *//p' | head -n 1)\n\
         [ -n \"$repo_ver\" ] || exit 0\n\
         [ \"$(vercmp \"$repo_ver\" \"$local_ver\")\" -gt 0 ] || exit 0\n\
         printf '%s\\n' \"{line1}\" \"{line2}\"\n\
         exit 0\n"
    )
}

/// What changes in the `deepin-session` recipe.
pub fn recipe_edit() -> RecipeEdit {
    RecipeEdit {
        pkgname: LOCAL_PACKAGE.to_string(),
        pkgdesc_note: format!("Rexon build without {X11_WM_PACKAGE}, for the Treeland session"),
        to_optdepends: vec![(
            X11_WM_PACKAGE.to_string(),
            "X11 session (conflicts with kwin from KDE Plasma)".to_string(),
        )],
        drop_files: vec![X11_SESSION_FILE.to_string()],
        add_files: vec![
            (HOOK_FILE.to_string(), "0644".to_string(), hook_text()),
            (CHECK_FILE.to_string(), "0755".to_string(), check_script()),
        ],
        ..RecipeEdit::default()
    }
}

/// The edit for switching from the regular Deepin: the same build is
/// installed by replacing the installed `deepin-session`, and `deepin-kwin`
/// (if installed) is removed by the same root command — there is no X11
/// session after the switch.
pub fn migration_edit(x11_wm_installed: bool) -> RecipeEdit {
    RecipeEdit {
        replace_conflicts: true,
        remove_after: if x11_wm_installed { vec![X11_WM_PACKAGE.to_string()] } else { Vec::new() },
        ..recipe_edit()
    }
}

// ── The set and the dry run ─────────────────────────────────────────────

/// What is installed: members of the Deepin group without `deepin-session`
/// (our own build replaces it) and without `deepin-kwin` (it is not in the
/// group, but it might appear).
pub fn variant_targets(group_members: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in group_members {
        if m != SESSION_PACKAGE && m != X11_WM_PACKAGE && m != LOCAL_PACKAGE && !out.contains(m) {
            out.push(m.clone());
        }
    }
    out
}

/// `deepin-kwin` is never installed, neither as a set member nor as a
/// dependency: a dry run whose plan contains it counts as not installable (it
/// is in [`InstallPreview::blocked`], and the installation refuses before the
/// password).
pub fn forbid_x11_wm(p: &mut InstallPreview) {
    let pulled = p.new_packages.iter().chain(&p.targets).any(|n| n == X11_WM_PACKAGE);
    if pulled && !p.blocked.iter().any(|n| n == X11_WM_PACKAGE) {
        p.blocked.push(X11_WM_PACKAGE.to_string());
    }
}

/// Members of the Deepin group in the repositories (`pacman -Sg deepin`).
fn group_members(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<Vec<String>> {
    let cmd = system_plan::sync_groups_cmd(family, &[GROUP.to_string()])?;
    let (_, stdout, _) = run(&cmd)?;
    system_plan::parse_sync_groups(&stdout)
        .into_iter()
        .find(|(g, _)| g == GROUP)
        .map(|(_, m)| m)
        .filter(|m| !m.is_empty())
}

/// The `Version` field of the first block of `pacman -Si`/`-Qi`, i.e. of the
/// repository pacman would install the package from.
fn first_version(stdout: &str) -> Option<String> {
    repo_rebuild::si_field(stdout, "Version").into_iter().next().filter(|v| !v.is_empty())
}

/// The version of `deepin-session` in the repositories.
pub fn repo_session_version(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<String> {
    let cmd = cmd(vec!["pacman".into(), "-Si".into(), SESSION_PACKAGE.into()], Some(60));
    match run(&cmd)? {
        (0, stdout, _) => first_version(&stdout),
        _ => None,
    }
}

/// The version of the installed package `name` according to `pacman -Q`, if
/// it is exactly that package that is installed (and not another one that
/// `provides` it).
fn installed_version(name: &str, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<String> {
    let cmd = cmd(vec!["pacman".into(), "-Q".into(), name.into()], Some(30));
    match run(&cmd)? {
        (0, stdout, _) => {
            let mut parts = stdout.split_whitespace();
            (parts.next() == Some(name)).then(|| parts.next().map(str::to_string)).flatten()
        }
        _ => None,
    }
}

/// The version of our own build, if it is installed.
pub fn local_version(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<String> {
    installed_version(LOCAL_PACKAGE, run)
}

/// The regular Deepin is installed: `deepin-session` from the repository, not our own build.
pub fn ordinary_installed(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> bool {
    installed_version(SESSION_PACKAGE, run).is_some()
}

/// Whether `deepin-kwin` is installed (a leftover of the regular Deepin).
fn x11_wm_installed(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> bool {
    installed_version(X11_WM_PACKAGE, run).is_some()
}

/// `a` is newer than `b`, according to pacman's `vercmp` (there is no point
/// in reimplementing Arch version comparison). No answer means "not newer":
/// a false alarm is worse.
pub fn newer(a: &str, b: &str, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> bool {
    let cmd = cmd(vec!["vercmp".into(), a.into(), b.into()], Some(10));
    matches!(run(&cmd), Some((0, out, _)) if out.trim().parse::<i32>().is_ok_and(|n| n > 0))
}

/// A `-Sp` dry run of the same set, but as if `deepin-session` were already
/// installed: `--assume-installed` satisfies the dependency of `treeland` and
/// `deepin-session-shell` on it, and pacman does not pull in the recipe's
/// `deepin-session` with its `deepin-kwin`.
fn assume_session(cmd: &Cmd, version: &str) -> Cmd {
    let mut argv = cmd.argv.clone();
    if argv.get(1).map(String::as_str) == Some("-Sp") {
        argv.insert(2, format!("{SESSION_PACKAGE}={version}"));
        argv.insert(2, "--assume-installed".to_string());
    }
    Cmd { argv, timeout_secs: cmd.timeout_secs, lang_c: cmd.lang_c }
}

/// The variant's set and a dry run of its installation (with the same
/// [`system_plan::install_preview`] as any desktop, with conflict checking).
/// `assume` is the version of `deepin-session` to treat as installed (our
/// own build does not exist yet); `None` means our build is already installed
/// and provides it itself.
pub fn variant_preview(
    family: PkgFamily,
    assume: Option<&str>,
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Option<(Vec<String>, InstallPreview)> {
    if family != PkgFamily::Arch {
        return None;
    }
    let targets = variant_targets(&group_members(family, run)?);
    let wrapped = |cmd: &Cmd| match assume {
        Some(v) => run(&assume_session(cmd, v)),
        None => run(cmd),
    };
    let mut preview = system_plan::install_preview(family, &targets, &wrapped)?;
    forbid_x11_wm(&mut preview);
    Some((targets, preview))
}

/// What to offer on the Deepin card with a button under the warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// The regular Deepin (X11) is installed: switch to the Treeland variant —
    /// our own build replacing the installed `deepin-session` ([`install_live`]).
    Install,
    /// The variant is installed, and the repository's `deepin-session` is newer than our build.
    Update { local: String, repo: String },
}

/// The state of the Deepin card: the dry run to show instead of the regular
/// one (the regular set pulls in `deepin-session` with `deepin-kwin`, and
/// Rexon does not install it), and what to offer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardState {
    pub preview: Option<InstallPreview>,
    pub offer: Option<Offer>,
}

/// The decision for the card. `normal` is the dry run of the desktop's
/// regular set: it is used only if the variant's dry run could not be made,
/// and then with `deepin-kwin` forbidden ([`forbid_x11_wm`]) — the card must
/// not promise the regular Deepin.
pub fn card_state(
    de_id: &str,
    family: PkgFamily,
    normal: Option<&InstallPreview>,
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> CardState {
    if de_id != DE_ID || family != PkgFamily::Arch {
        return CardState::default();
    }
    if ordinary_installed(run) {
        return CardState { preview: None, offer: Some(Offer::Install) };
    }
    if let Some(local) = local_version(run) {
        let preview = variant_preview(family, None, run).map(|(_, p)| p);
        let offer = repo_session_version(run)
            .filter(|repo| newer(repo, &local, run))
            .map(|repo| Offer::Update { local, repo });
        return CardState { preview, offer };
    }
    let preview = repo_session_version(run)
        .and_then(|repo| variant_preview(family, Some(&repo), run))
        .map(|(_, p)| p)
        .or_else(|| {
            normal.cloned().map(|mut p| {
                forbid_x11_wm(&mut p);
                p
            })
        });
    CardState { preview, offer: None }
}

/// The set for installing Deepin when our own build is already installed:
/// group members without `deepin-session`, otherwise pacman would pull it in
/// along with `deepin-kwin`. Without our own build it is a refusal: Rexon does
/// not install the group as a whole.
pub fn install_set(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Result<Vec<String>, String> {
    if family != PkgFamily::Arch || local_version(run).is_none() {
        return Err(format!("{}\n\n{}", only_treeland_title(), not_built_text()));
    }
    group_members(family, run)
        .map(|m| variant_targets(&m))
        .ok_or_else(|| format!("{}\n\n{}", only_treeland_title(), group_unknown_text()))
}

/// The verdict before installation and "Repair" (after the distribution check):
/// `None` means there is nothing to say (our build is installed, "Repair"
/// reinstalls its set); `Some((false, …))` means the regular Deepin is
/// installed, a refusal before the password; `Some((true, …))` means the
/// "experimental" warning in the same question.
pub fn install_verdict(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<(bool, String, String)> {
    if family != PkgFamily::Arch {
        return None;
    }
    if ordinary_installed(run) {
        return Some((false, only_treeland_title(), ordinary_installed_text()));
    }
    if local_version(run).is_some() {
        return None;
    }
    Some((true, install_warning_title(), install_warning()))
}

// ── Execution ───────────────────────────────────────────────────────────

fn live_run(cmd: &Cmd) -> PreviewAnswer {
    let r = crate::system_exec::SystemIo::run(
        &crate::system_de_exec::LiveDeOpsIo::new(),
        &cmd.argv,
        cmd.timeout_secs,
        cmd.lang_c,
    );
    r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
}

/// [`card_state`] on the live machine (not on an immutable system).
pub fn card_state_live(de_id: &str, family: PkgFamily, normal: Option<&InstallPreview>) -> CardState {
    if de_id != DE_ID || repo_rebuild::rebuild_ctx_live().is_err() {
        return CardState::default();
    }
    card_state(de_id, family, normal, &live_run)
}

/// Our own build of `deepin-session` (if it does not exist yet or must
/// replace the regular one), then the installation of the Deepin desktop with
/// the variant's set ([`install_set`]) — with the same checks, conflict dry
/// run and login screen protection. `migrate` is the consent to replace the
/// regular Deepin; without it, an installed regular Deepin means a refusal.
fn install_variant(sink: Arc<dyn DeOpSink>, cancel: CancelToken, current_de: &str, migrate: bool) -> DeOpOutcome {
    let ctx = match repo_rebuild::rebuild_ctx_live() {
        Ok(ctx) => ctx,
        Err(message) => return repo_rebuild::refuse(&sink, message),
    };
    let ordinary = ordinary_installed(&live_run);
    if ordinary && !migrate {
        return repo_rebuild::refuse(&sink, format!("{}\n\n{}", only_treeland_title(), ordinary_installed_text()));
    }
    if ordinary || local_version(&live_run).is_none() {
        let edit = if ordinary { migration_edit(x11_wm_installed(&live_run)) } else { recipe_edit() };
        let built = repo_rebuild::rebuild_edited_with(
            &repo_rebuild::LiveRebuildIo::new(),
            sink.clone(),
            &cancel,
            &ctx,
            SESSION_PACKAGE,
            &edit,
            build_done_message(),
        );
        if !built.ok {
            return built;
        }
        sink.progress("rebuild", &built.message, None);
    }
    let mut outcome = crate::system_de_exec::install_de_packages_live(sink, cancel, DE_ID, current_de);
    if outcome.ok {
        outcome.message = format!("{}\n\n{}", outcome.message, install_result_text()).trim().to_string();
    }
    outcome
}

/// Installing and "Repair" of Deepin from the card: the
/// [`crate::system_de_exec::install_de_live`] path for Deepin.
pub fn install_from_card_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, current_de: &str) -> DeOpOutcome {
    install_variant(sink, cancel, current_de, false)
}

/// Switching from the regular Deepin to the Treeland variant ([`Offer::Install`]).
pub fn install_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, current_de: &str) -> DeOpOutcome {
    install_variant(sink, cancel, current_de, true)
}

/// Updating our own build to the repository version.
pub fn update_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken) -> DeOpOutcome {
    let ctx = match repo_rebuild::rebuild_ctx_live() {
        Ok(ctx) => ctx,
        Err(message) => return repo_rebuild::refuse(&sink, message),
    };
    repo_rebuild::rebuild_edited_with(
        &repo_rebuild::LiveRebuildIo::new(),
        sink,
        &cancel,
        &ctx,
        SESSION_PACKAGE,
        &recipe_edit(),
        update_done_message(),
    )
}

// ── Texts for humans ────────────────────────────────────────────────────

/// The main refusal sentence for any Deepin not on Treeland.
pub fn only_treeland_title() -> String {
    tr("Rexon supports Deepin only on Treeland")
}

/// The refusal of "Repair" and of installing over the regular Deepin.
fn ordinary_installed_text() -> String {
    tr("The usual Deepin with the X11 session is installed here. Rexon doesn't repair or reinstall it. \
        To move to Deepin on Treeland, press “Switch to Deepin on Treeland” on the Deepin card.")
}

fn not_built_text() -> String {
    tr("The Treeland build of deepin-session isn't installed, and the usual Deepin set with deepin-kwin \
        is never installed by Rexon. Install Deepin with the “Install” button on its card.")
}

fn group_unknown_text() -> String {
    tr("The list of Deepin packages couldn't be read from the repositories. Check the internet connection and try again.")
}

/// The warning title in the question before installation.
pub fn install_warning_title() -> String {
    tr("Deepin on Treeland (experimental)")
}

/// The honest warning in the question before installation.
pub fn install_warning() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("Deepin is installed on Treeland, its own Wayland session.\n\n\
        • This is experimental: Treeland is still unstable, some Deepin settings and applets \
        may not work, and the session may crash.\n\
        • There is no X11 session of Deepin: its window manager (deepin-kwin) conflicts with \
        KWin of KDE Plasma and is never installed.\n\
        • deepin-session is built on your computer from the official Arch recipe — the same \
        sources, checksums and signatures, but without deepin-kwin. It is installed as \
        deepin-session-treeland.\n\
        • System updates don't touch this build. When Arch releases a new deepin-session, \
        the update log and the Deepin card in Rexon say so, and the build is redone with one button.\n\
        • Deepin's own login screen (DDM) is installed because Treeland needs it, but it isn't \
        turned on: you keep your login screen. Deepin starts only from DDM — under another login \
        screen the “Treeland User” entry shows just the wallpaper, without the panel. To use Deepin, \
        make DDM the default in Appearance → Login Screen → Login manager and restart.\n\n\
        The password is asked once. If the build takes longer than a few minutes, it may be asked \
        once more before installing. Removing Deepin later removes all of it.")
}

/// The card button when the regular Deepin is installed.
pub fn install_button() -> String {
    tr("🧪 Switch to Deepin on Treeland (experimental)")
}

/// The explanation under the switch button.
pub fn install_hint() -> String {
    tr("This is the usual Deepin with the X11 session. Rexon supports Deepin only on Treeland, \
        its Wayland session: Rexon builds its own deepin-session without deepin-kwin, puts it in place \
        of the installed one and removes the X11 session. Treeland is unstable. Deepin on Treeland \
        starts only from its own login screen, DDM.")
}

/// The title of the switch dialog.
pub fn install_title() -> String {
    tr("Switching to Deepin on Treeland (experimental)")
}

/// The question before switching.
pub fn install_confirm_text() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("The usual Deepin will be moved to Treeland, its own Wayland session.\n\n\
        • This is experimental: Treeland is still unstable, some Deepin settings and applets \
        may not work, and the session may crash.\n\
        • The X11 session of Deepin goes away: its window manager deepin-kwin is removed, \
        and the “Deepin” entry disappears from the login screen.\n\
        • deepin-session is built on your computer from the official Arch recipe — the same \
        sources, checksums and signatures, but without deepin-kwin — and replaces the installed one \
        as deepin-session-treeland. The rest of Deepin and your settings stay.\n\
        • System updates don't touch this build. When Arch releases a new deepin-session, \
        the update log and the Deepin card in Rexon say so, and the build is redone with one button.\n\
        • Deepin on Treeland starts only from its own login screen, DDM: under another one the \
        “Treeland User” entry shows just the wallpaper. To use Deepin, make DDM the default in \
        Appearance → Login Screen → Login manager and restart.\n\n\
        The password is asked once. If the build takes longer than a few minutes, it may be asked \
        once more before installing.")
}

/// The note added to the result of a successful installation: how to log in.
/// Under another login screen Treeland 0.11 does not bring up a session
/// (treeland#1656) — say so honestly instead of leaving the person in front of
/// a bare background.
pub fn install_result_text() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("Deepin starts only from its own login screen, DDM. To try it, make DDM the default in \
        Appearance → Login Screen → Login manager, restart and choose Treeland there.")
}

/// The card button when our own build has fallen behind the repository.
pub fn update_button() -> String {
    tr("🛠 Update the Treeland build")
}

/// The explanation under the update button.
pub fn update_hint(local: &str, repo: &str) -> String {
    trf(
        "The repositories have deepin-session %1, your Treeland build is %2. System updates \
         don't touch it; until you rebuild it, the Treeland session keeps working on the old build.",
        &[&repo, &local],
    )
}

/// The title of the update dialog.
pub fn update_title() -> String {
    tr("Updating the Treeland build of deepin-session")
}

/// The question before updating.
pub fn update_confirm_text(local: &str, repo: &str) -> String {
    trf(
        "deepin-session %1 will be built from the official Arch recipe without deepin-kwin and \
         will replace your build %2. The password is asked once. If the build takes longer than \
         a few minutes, it may be asked once more before installing.",
        &[&repo, &local],
    )
}

fn build_done_message() -> String {
    tr("deepin-session was built without deepin-kwin and installed as deepin-session-treeland. \
        Now the rest of Deepin is installed.")
}

fn update_done_message() -> String {
    tr("The Treeland build of deepin-session was updated. The new build is used from the next login to Treeland.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_plan::PkgConflict;
    use std::cell::RefCell;

    /// `LC_ALL=C pacman -Sg deepin` (CachyOS, 20.10.2026): the beginning and the
    /// members that matter for the variant; one and the same member from two
    /// repositories.
    const SG_DEEPIN: &str = "deepin ddm\ndeepin deepin-daemon\n\
deepin deepin-session\ndeepin deepin-session-shell\ndeepin deepin-shell\ndeepin treeland\n\
deepin ddm\ndeepin deepin-session\n";

    /// `LC_ALL=C pacman -Si deepin-session`: a cachyos-extra-znver4 block, then extra.
    const SI_SESSION: &str = "Repository      : cachyos-extra-znver4\n\
Name            : deepin-session\nVersion         : 2.0.33-1.1\n\
Depends On      : bash  deepin-kwin  deepin-shell\n\n\
Repository      : extra\nName            : deepin-session\nVersion         : 2.0.33-1\n";

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// What is installed on the machine.
    #[derive(Clone, Copy)]
    struct Machine {
        /// KDE Plasma (kwin) is installed.
        plasma: bool,
        /// The version of our own build, if installed.
        local: Option<&'static str>,
        /// The regular Deepin is installed (`deepin-session` and `deepin-kwin` from the repository).
        ordinary: bool,
        /// The dry run of the variant's set pulls in `deepin-kwin` (in case another
        /// group member ever requires it).
        variant_pulls_kwin: bool,
    }

    const FRESH: Machine = Machine { plasma: false, local: None, ordinary: false, variant_pulls_kwin: false };

    /// pacman answers for machine `m`; every `-Sp` dry run is recorded in `seen`.
    fn machine(m: Machine, seen: &RefCell<Vec<Vec<String>>>) -> impl Fn(&Cmd) -> PreviewAnswer + '_ {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
            let missing = |n: &str| Some((1, String::new(), format!("error: package '{n}' was not found\n")));
            match a.as_slice() {
                ["pacman", "-Q", LOCAL_PACKAGE] => match m.local {
                    Some(v) => Some((0, format!("{LOCAL_PACKAGE} {v}\n"), String::new())),
                    None => missing(LOCAL_PACKAGE),
                },
                // pacman -Q also answers for a package that `provides`: our own build
                // answers under its own name.
                ["pacman", "-Q", SESSION_PACKAGE] => match (m.ordinary, m.local) {
                    (true, _) => Some((0, "deepin-session 2.0.32-1\n".into(), String::new())),
                    (false, Some(v)) => Some((0, format!("{LOCAL_PACKAGE} {v}\n"), String::new())),
                    (false, None) => missing(SESSION_PACKAGE),
                },
                ["pacman", "-Q", X11_WM_PACKAGE] if m.ordinary => {
                    Some((0, "deepin-kwin 6.0.8-12\n".into(), String::new()))
                }
                ["pacman", "-Q", X11_WM_PACKAGE] => missing(X11_WM_PACKAGE),
                ["pacman", "-Si", SESSION_PACKAGE] => Some((0, SI_SESSION.into(), String::new())),
                ["pacman", "-Sg", GROUP] => Some((0, SG_DEEPIN.into(), String::new())),
                // The variant's set consists of packages, not groups.
                ["pacman", "-Sg", ..] => Some((1, String::new(), "error: target not found\n".into())),
                ["pacman", "-Sp", rest @ ..] => {
                    seen.borrow_mut().push(rest.iter().map(|s| s.to_string()).collect());
                    assert!(!rest.contains(&SESSION_PACKAGE) && !rest.contains(&GROUP), "{a:?}");
                    assert!(!rest.contains(&X11_WM_PACKAGE), "{a:?}");
                    let mut plan = "ddm\ndeepin-daemon\ndeepin-session-shell\ndeepin-shell\ntreeland\nlightdm\n".to_string();
                    if m.variant_pulls_kwin {
                        plan.push_str("deepin-kwin\n");
                    }
                    Some((0, plan, String::new()))
                }
                ["pacman", "-Qq"] => {
                    let mut out = "glibc\n".to_string();
                    if m.plasma {
                        out.push_str("kwin\nplasma-workspace\n");
                    }
                    Some((0, out, String::new()))
                }
                ["pacman", "-Si", ..] => Some((0, "Name            : ddm\nConflicts With  : None\n".into(), String::new())),
                ["pacman", "-Qi"] => Some((0, "Name            : kwin\nConflicts With  : None\n".into(), String::new())),
                ["vercmp", x, y] => Some((0, if x > y { "1\n" } else if x == y { "0\n" } else { "-1\n" }.into(), String::new())),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    /// A dry run of the regular set on a machine with Plasma: it ran into `deepin-kwin`.
    fn kwin_blocked() -> InstallPreview {
        InstallPreview {
            conflicts: vec![PkgConflict { new_package: "deepin-kwin".into(), installed: "kwin".into(), ..Default::default() }],
            new_packages: names(&["deepin-kwin", "deepin-session"]),
            ..Default::default()
        }
    }

    /// A dry run of the regular set on a machine without Plasma: no conflict, but
    /// `deepin-kwin` is in the plan.
    fn plain_group() -> InstallPreview {
        InstallPreview { new_packages: names(&["deepin-kwin", "deepin-session"]), ..Default::default() }
    }

    #[test]
    fn variant_set_is_the_group_without_session_and_kwin() {
        let members = names(&["ddm", "deepin-daemon", "deepin-session", "deepin-kwin", "treeland", "ddm"]);
        assert_eq!(variant_targets(&members), names(&["ddm", "deepin-daemon", "treeland"]));
    }

    #[test]
    fn edit_moves_kwin_to_optdepends_and_drops_x11_session() {
        let e = recipe_edit();
        assert_eq!(e.pkgname, LOCAL_PACKAGE);
        assert_eq!(e.to_optdepends[0].0, X11_WM_PACKAGE);
        assert_eq!(e.drop_files, vec!["usr/share/xsessions/deepin.desktop"]);
        assert!(!e.replace_conflicts && e.remove_after.is_empty());
        let hook = &e.add_files[0];
        assert!(hook.0.starts_with("usr/share/libalpm/hooks/") && hook.0.ends_with(".hook"));
        assert!(hook.2.contains("When = PostTransaction"));
        assert!(hook.2.contains(&format!("Exec = /{CHECK_FILE}")));
        let script = &e.add_files[1];
        assert_eq!(script.0, CHECK_FILE);
        assert_eq!(script.1, "0755");
        assert!(script.2.starts_with("#!/bin/sh\n"));
        assert!(script.2.contains("vercmp \"$repo_ver\" \"$local_ver\""));
        // The substitutions are shell variables, not the text "%REPO%".
        assert!(script.2.contains("${repo_ver}") && script.2.contains("${local_ver}"));
        assert!(!script.2.contains("%REPO%"));
    }

    /// Switching from the regular Deepin: the same build, but replacing the
    /// installed `deepin-session` and removing `deepin-kwin` if it is installed.
    #[test]
    fn migration_replaces_the_session_and_removes_kwin() {
        let e = migration_edit(true);
        assert!(e.replace_conflicts);
        assert_eq!(e.remove_after, names(&[X11_WM_PACKAGE]));
        assert_eq!(RecipeEdit { replace_conflicts: false, remove_after: Vec::new(), ..e }, recipe_edit());
        assert!(migration_edit(false).remove_after.is_empty());
    }

    #[test]
    fn translated_text_cannot_break_out_of_the_printf_quotes() {
        assert_eq!(dq(r#"a "b" $c `d` \e"#), r#"a \"b\" \$c \`d\` \\e"#);
    }

    #[test]
    fn preview_assumes_session_only_in_the_dry_run() {
        let sp = cmd(names(&["pacman", "-Sp", "--needed", "treeland"]), None);
        assert_eq!(
            assume_session(&sp, "2.0.33-1.1").argv,
            names(&["pacman", "-Sp", "--assume-installed", "deepin-session=2.0.33-1.1", "--needed", "treeland"])
        );
        let si = cmd(names(&["pacman", "-Si", "treeland"]), None);
        assert_eq!(assume_session(&si, "1").argv, si.argv);
    }

    #[test]
    fn kwin_in_any_plan_makes_it_not_installable() {
        let mut p = plain_group();
        assert!(p.installable());
        forbid_x11_wm(&mut p);
        forbid_x11_wm(&mut p);
        assert!(!p.installable());
        assert_eq!(p.blocked, names(&[X11_WM_PACKAGE]));
        let mut clean = InstallPreview { new_packages: names(&["treeland"]), ..Default::default() };
        forbid_x11_wm(&mut clean);
        assert!(clean.installable());
    }

    /// The card of a Deepin that is not installed yet is the same with and
    /// without Plasma: a dry run of the variant's set (with `--assume-installed`),
    /// without `deepin-session` and `deepin-kwin`, and no second button.
    #[test]
    fn card_is_the_treeland_set_with_and_without_plasma() {
        let mut previews = Vec::new();
        let mut probes = Vec::new();
        for (plasma, normal) in [(true, kwin_blocked()), (false, plain_group())] {
            let seen = RefCell::new(Vec::new());
            let m = Machine { plasma, ..FRESH };
            let state = card_state(DE_ID, PkgFamily::Arch, Some(&normal), &machine(m, &seen));
            assert_eq!(state.offer, None);
            let p = state.preview.expect("проба набора варианта");
            assert!(p.installable(), "{p:?}");
            assert!(!p.new_packages.iter().any(|n| n == X11_WM_PACKAGE || n == SESSION_PACKAGE), "{p:?}");
            assert!(p.new_packages.iter().any(|n| n == "treeland"));
            let seen = seen.into_inner();
            assert!(seen.iter().all(|argv| argv.windows(2).any(|w| w == ["--assume-installed", "deepin-session=2.0.33-1.1"])));
            previews.push(p);
            probes.push(seen);
        }
        assert_eq!(previews[0], previews[1]);
        assert_eq!(probes[0], probes[1]);
    }

    #[test]
    fn variant_that_pulls_kwin_is_not_offered_as_installable() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { variant_pulls_kwin: true, ..FRESH };
        let p = card_state(DE_ID, PkgFamily::Arch, Some(&plain_group()), &machine(m, &seen)).preview.unwrap();
        assert!(!p.installable());
        assert!(p.blocked.iter().any(|n| n == X11_WM_PACKAGE));
    }

    #[test]
    fn other_desktops_and_families_get_nothing() {
        let seen = RefCell::new(Vec::new());
        let run = machine(FRESH, &seen);
        assert_eq!(card_state("pantheon", PkgFamily::Arch, Some(&kwin_blocked()), &run), CardState::default());
        assert_eq!(card_state(DE_ID, PkgFamily::Fedora, Some(&kwin_blocked()), &run), CardState::default());
        assert_eq!(install_verdict(PkgFamily::Fedora, &run), None);
        assert!(install_set(PkgFamily::Fedora, &run).is_err());
    }

    #[test]
    fn installed_build_replaces_the_preview_and_is_quiet_while_current() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { plasma: true, local: Some("2.0.33-1.1"), ..FRESH };
        let state = card_state(DE_ID, PkgFamily::Arch, Some(&kwin_blocked()), &machine(m, &seen));
        let preview = state.preview.expect("проба набора варианта");
        assert!(preview.installable(), "{preview:?}");
        assert!(!preview.targets.iter().any(|t| t == SESSION_PACKAGE));
        assert_eq!(state.offer, None);
        // Our own build is installed: `--assume-installed` is not needed.
        assert!(seen.borrow().iter().all(|argv| !argv.iter().any(|a| a == "--assume-installed")));
    }

    #[test]
    fn outdated_build_is_offered_an_update() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { local: Some("2.0.32-1"), ..FRESH };
        let state = card_state(DE_ID, PkgFamily::Arch, None, &machine(m, &seen));
        assert_eq!(state.offer, Some(Offer::Update { local: "2.0.32-1".into(), repo: "2.0.33-1.1".into() }));
    }

    /// The regular Deepin is installed: Rexon does not touch it, the card offers to switch.
    #[test]
    fn ordinary_deepin_is_offered_the_switch() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { ordinary: true, ..FRESH };
        let state = card_state(DE_ID, PkgFamily::Arch, Some(&InstallPreview::default()), &machine(m, &seen));
        assert_eq!(state, CardState { preview: None, offer: Some(Offer::Install) });
        assert!(x11_wm_installed(&machine(m, &seen)));
    }

    /// The question before installation: a fresh machine gets the "experimental"
    /// warning; the regular Deepin gets a refusal before the password ("Repair"
    /// does not reinstall it); our own build — nothing to say.
    #[test]
    fn verdict_warns_refuses_or_stays_quiet() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let seen = RefCell::new(Vec::new());
        let (allowed, title, text) = install_verdict(PkgFamily::Arch, &machine(FRESH, &seen)).unwrap();
        assert!(allowed);
        assert!(title.contains("experimental"), "{title}");
        assert!(text.contains("no X11 session") && text.contains("Treeland User"), "{text}");
        // Treeland 0.11 does not bring up a session without DDM (treeland#1656): the
        // texts do not promise login "from any login screen", they lead to DDM.
        assert!(text.contains("starts only from DDM") && text.contains("Login manager"), "{text}");
        for t in [install_confirm_text(), install_hint(), install_result_text()] {
            assert!(t.contains("DDM") && !t.contains("choose “Treeland User”"), "{t}");
        }
        let ordinary = Machine { ordinary: true, ..FRESH };
        let (allowed, title, _) = install_verdict(PkgFamily::Arch, &machine(ordinary, &seen)).unwrap();
        assert!(!allowed);
        assert_eq!(title, "Rexon supports Deepin only on Treeland");
        let built = Machine { local: Some("2.0.33-1.1"), ..FRESH };
        assert_eq!(install_verdict(PkgFamily::Arch, &machine(built, &seen)), None);
    }

    /// The installation set exists only with our own build; otherwise a refusal, not the group.
    #[test]
    fn install_set_needs_the_own_build() {
        let seen = RefCell::new(Vec::new());
        let built = Machine { local: Some("2.0.33-1.1"), ..FRESH };
        let t = install_set(PkgFamily::Arch, &machine(built, &seen)).unwrap();
        assert_eq!(t, names(&["ddm", "deepin-daemon", "deepin-session-shell", "deepin-shell", "treeland"]));
        let err = install_set(PkgFamily::Arch, &machine(FRESH, &seen)).unwrap_err();
        assert!(err.starts_with(&only_treeland_title()), "{err}");
    }

    /// A stand in a CONTAINER (archlinux with plasma-desktop and kwin, `pkexec` is
    /// a wrapper over sudo): the step from `REXON_DTL_STEP` is `card`, `install`,
    /// `switch`, `update`, `remove`. Do not run on a live machine: it installs
    /// packages.
    /// `cargo test -p rexon-core --lib deepin_treeland::tests::container_stand -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn container_stand() {
        struct Print;
        impl DeOpSink for Print {
            fn progress(&self, stage: &str, message: &str, _: Option<f64>) {
                eprintln!("[{stage}] {message}");
            }
            fn cancellable(&self, _: bool) {}
        }
        assert!(std::path::Path::new("/run/.containerenv").exists(), "только в контейнере podman");
        let family = PkgFamily::Arch;
        let step = std::env::var("REXON_DTL_STEP").unwrap_or_default();
        let sink: Arc<dyn DeOpSink> = Arc::new(Print);
        match step.as_str() {
            "card" => {
                let packages = crate::system::get_packages(DE_ID, family);
                let normal = system_plan::install_preview_live(family, &packages);
                let state = card_state_live(DE_ID, family, normal.as_ref());
                if let Some(p) = &state.preview {
                    eprintln!(
                        "проба варианта: ставится={} новых={} конфликты={:?} заблокировано={:?}",
                        p.installable(),
                        p.new_packages.len(),
                        p.conflicts,
                        p.blocked
                    );
                }
                eprintln!("предложение: {:?}", state.offer);
            }
            "install" => eprintln!("ИТОГ: {:?}", install_from_card_live(sink, CancelToken::new(), "kde")),
            "switch" => eprintln!("ИТОГ: {:?}", install_live(sink, CancelToken::new(), "kde")),
            "update" => eprintln!("ИТОГ: {:?}", update_live(sink, CancelToken::new())),
            "remove" => eprintln!(
                "ИТОГ: {:?}",
                crate::system_de_exec::remove_de_live(sink, CancelToken::new(), DE_ID, "kde")
            ),
            other => panic!("REXON_DTL_STEP: неизвестный шаг {other:?}"),
        }
    }

    #[test]
    fn versions_are_read_from_recorded_answers() {
        let seen = RefCell::new(Vec::new());
        let built = Machine { local: Some("2.0.33-1.1"), ..FRESH };
        assert_eq!(repo_session_version(&machine(built, &seen)).as_deref(), Some("2.0.33-1.1"));
        assert_eq!(local_version(&machine(built, &seen)).as_deref(), Some("2.0.33-1.1"));
        // Our own build `provides` deepin-session, but does not count as the regular Deepin.
        assert!(!ordinary_installed(&machine(built, &seen)));
        assert_eq!(local_version(&machine(FRESH, &seen)), None);
    }
}
