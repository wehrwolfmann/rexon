//! Installing packages from the AUR with a helper (yay/paru): one for the
//! whole of Rexon.
//!
//! There are three decisions here, shared by EVERY place where Rexon calls
//! an AUR helper (appearance, adding to desktops, repairs from the check,
//! SteamOS; rule #12):
//!
//! 1. **The command** ([`install_argv`]). Rexon is launched from the
//!    application menu, so it has no terminal. Plain `sudo`, which the
//!    helper uses to install the built package, cannot ask for a password
//!    without a terminal and fails ("a terminal is required to read the
//!    password"). So privileges are requested through a graphical polkit
//!    window (`--sudo pkexec`), as the AeroShell rebuild already does
//!    ([`crate::aeroshell::rebuild_command`]). All of the helper's
//!    questions are answered in advance: showing diffs and cleaning build
//!    files is "no", build dependencies are removed after installation.
//!    Replacing conflicting packages (`--ask=4`) happens ONLY when the
//!    person has explicitly confirmed it ([`plan_install`]): silently
//!    removing a system package for the sake of a theme is exactly the
//!    trap rule #13 protects against.
//!
//! 2. **The failure reason** ([`failure_cause`] -> [`failure_text`]). The
//!    helper's output is parsed, and the person reads "package X conflicts
//!    with Y" or "the source failed to download" rather than "details in
//!    the log". The parser understands both English and Russian output
//!    (`LANG=ru_RU`): some places run the helper in the system language.
//!
//! 3. **Kits** ([`AurKit`]). A package that is really not a theme but a
//!    Plasma fork (AeroThemePlasma) is installed as a kit in the order of
//!    its authors' instructions and only after a warning: what will be
//!    replaced, what will have to be rebuilt after Plasma updates.
//!
//! The model defect (live 03.10.2026): "Appearance -> Global themes -> AUR
//! -> aerothemeplasma-desktop-git -> Install" built five dependencies and
//! failed on the very first install of what was built (sudo without a
//! terminal), and the person read only "details in the yay log", while the
//! log was not saved anywhere.

use crate::i18n::{ntr, tr, trc, trf};

/// Helpers that understand `--sudo`/`--sudoflags`/`--removemake` and pass
/// `--ask` to pacman itself (verified 03-04.10.2026 on yay 13.0.1 and paru
/// with a fake `--sudo` that records its command line: both call
/// `<sudo> pacman -U … --ask 4 …`).
/// The others (trizen, pikaur, aurman) are called as before: we have not
/// verified their flags and do not substitute them at random.
pub const FLAG_AWARE_HELPERS: &[&str] = &["yay", "paru"];

/// argv for installing packages from the AUR.
///
/// * `--needed`: what is already installed is not rebuilt;
/// * `--noconfirm`: not a single question into the void;
/// * yay: `--answerclean None --answerdiff None --answeredit None`: the
///   menus for cleaning/showing diffs/editing build files are answered
///   "none" (under `--noconfirm` yay skips them anyway, but prints empty
///   questions; an explicit answer does not depend on someone else's
///   `~/.config/yay/config.json`);
/// * paru: `--skipreview`: the same for paru (otherwise it waits for the
///   PKGBUILD to be reviewed in a pager and hangs forever);
/// * `--removemake`: remove build dependencies after installation,
///   otherwise every theme would leave compilers and cmake in the system;
/// * `--sudo pkexec --sudoflags ""`: privileges through a graphical polkit
///   window (there is no terminal, see the module header). Empty
///   `--sudoflags` are mandatory: the person's config may have sudo flags
///   pkexec does not know. In the Rexon window with the privilege session
///   helper, the session shim is used instead of pkexec ([`push_sudo`],
///   item B10);
/// * paru: `--nosudoloop`: paru "keeps sudo alive" by calling `<sudo> -v`,
///   and pkexec has no such option;
/// * `replace_conflicts` -> `--ask=4`: pacman inverts the default answer to
///   the question "X and Y are in conflict. Remove Y?" (bit 4 is a package
///   conflict) and installs the replacement. Only after the person's
///   explicit consent.
///   ⚠️ NOT `--useask`: in yay 13 it does not reach the `pacman -U` of the
///   built packages (verified with a fake sudo on 03.10.2026: the flag is
///   not in the line).
pub fn install_argv(helper: &str, packages: &[String], replace_conflicts: bool) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        helper.to_string(),
        "-S".to_string(),
        "--needed".to_string(),
        "--noconfirm".to_string(),
    ];
    let mut push = |items: &[&str]| argv.extend(items.iter().map(|s| s.to_string()));
    match helper {
        "yay" => push(&[
            "--answerclean",
            "None",
            "--answerdiff",
            "None",
            "--answeredit",
            "None",
        ]),
        "paru" => push(&["--skipreview", "--nosudoloop"]),
        _ => {}
    }
    if FLAG_AWARE_HELPERS.contains(&helper) {
        push(&["--removemake"]);
        push_sudo(&mut argv, helper, packages, replace_conflicts);
        if replace_conflicts {
            argv.push("--ask=4".to_string());
        }
    }
    argv.extend(packages.iter().cloned());
    argv
}

/// The builder's `--sudo`/`--sudoflags` (item B10). In the Rexon window
/// with the privilege session helper: the `rexon-sudo-shim` shim and the
/// new operation marker ([`crate::privilege::aur::sudo_for`]): the
/// builder's pacman calls go to the privilege session (one password), and
/// only what relates to building `packages` is let through. Otherwise
/// `pkexec` with empty flags, as before.
///
/// paru with the shim additionally gets `--nochroot --nolocalrepo`: in a
/// clean chroot and with a local repository paru calls through sudo not
/// pacman but other things (`mkarchroot`, `arch-nspawn`, `install -d`),
/// which the shim will not let through: the person's setting in
/// `paru.conf` would break the build.
fn push_sudo(argv: &mut Vec<String>, helper: &str, packages: &[String], replace: bool) {
    match crate::privilege::aur::sudo_for(packages, replace) {
        Some(sudo) => {
            if helper == "paru" {
                argv.extend(["--nochroot".to_string(), "--nolocalrepo".to_string()]);
            }
            argv.extend(["--sudo".to_string(), sudo.program, "--sudoflags".to_string(), sudo.flags]);
        }
        None => argv.extend(["--sudo", "pkexec", "--sudoflags", ""].iter().map(|s| s.to_string())),
    }
}

/// Build switches of AUR recipes (variables a PKGBUILD reads from the
/// environment: `: ${_build_x:=true}`): a package from the Rexon list ->
/// what to set for `makepkg` under the helper. The helper does not clean
/// the environment, and neither does `makepkg`.
///
/// * `vala-panel-appmenu` 25.04 (AUR, pkgbase for all of `-mate`, `-xfce`,
///   `-registrar`…) builds the Budgie module by default too, and that one
///   looks for `budgie-1.0`; budgie-desktop 10.10 provides only
///   `budgie-3.0`, so build() fails for everyone (live 08.11.2026:
///   "Dependency "budgie-1.0" not found"). Rexon installs only
///   `-mate`/`-registrar` from this recipe (`system.rs`) and does not need
///   the Budgie module.
pub const BUILD_ENV: &[(&str, &[(&str, &str)])] = &[
    ("vala-panel-appmenu-mate", &[("_build_budgie", "false")]),
    ("vala-panel-appmenu-registrar", &[("_build_budgie", "false")]),
    ("vala-panel-appmenu-xfce", &[("_build_budgie", "false")]),
];

/// Environment for an AUR helper command from [`install_argv`]: the build
/// switches ([`BUILD_ENV`]) of the packages it installs. Another command
/// gets an empty one.
pub fn build_env(argv: &[String]) -> Vec<(String, String)> {
    let helper = argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)).unwrap_or("");
    if !FLAG_AWARE_HELPERS.contains(&helper) {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for p in argv_packages(argv) {
        for (_, vars) in BUILD_ENV.iter().filter(|(name, _)| *name == p) {
            for (k, v) in vars.iter() {
                if !out.iter().any(|(ok, _)| ok == k) {
                    out.push((k.to_string(), v.to_string()));
                }
            }
        }
    }
    out
}

/// Flags of [`install_argv`] followed by a value, so that command-line
/// parsing does not take `pkexec` or `None` for a package name.
pub const VALUE_FLAGS: &[&str] =
    &["--answerclean", "--answerdiff", "--answeredit", "--sudo", "--sudoflags"];

/// Packages from the [`install_argv`] command line (everything after `-S`
/// except flags and their values).
pub fn argv_packages(argv: &[String]) -> Vec<String> {
    let Some(pos) = argv.iter().position(|a| a == "-S") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut skip_value = false;
    for a in &argv[pos + 1..] {
        if skip_value {
            skip_value = false;
        } else if a.starts_with('-') {
            skip_value = VALUE_FLAGS.contains(&a.as_str());
        } else {
            out.push(a.clone());
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════
// Known breakages of AUR recipes: Rexon builds such a package itself
// ════════════════════════════════════════════════════════════════════════
//
// Some AUR recipes are broken in a way their maintainer has not fixed yet,
// and the person cannot fix it without editing a PKGBUILD by hand. The model
// case (live 27.11.2026, MATE): `mate-menu` 26.10.1-2 runs
// `python setup.py install --root="$pkgdir"`, and upstream's `setup.py`
// compiles the GSettings schemas INTO the package since September 2026, so
// the built package carries `/usr/share/glib-2.0/schemas/gschemas.compiled`
// and pacman refuses it ("exists in filesystem"). The file is generated on
// every machine by glib's pacman hook and must never ship in a package.
// Another kind (vala-panel-appmenu 25.04): the recipe turns on every panel
// module by default, so even the MATE applet drags in budgie-desktop,
// xfce4-panel and the AUR vala-panel and fails on the Budgie module.
//
// Why Rexon builds the package itself instead of steering yay/paru: neither
// helper has a dependable way to edit a PKGBUILD without a person. The edit
// menu (`--editmenu`, `--fm`) is skipped under `--noconfirm`, and the clone
// in the helper's cache is reset to the AUR state before every build. So
// for a package from [`RECIPE_FIXES`] Rexon:
//
// 1. clones the recipe from the AUR into its own cache
//    ([`recipe_build_dir`]);
// 2. applies the fix ONLY if the recipe is the one it was written for
//    ([`patch_recipe`]): the line to fix is present exactly once, inside the
//    right function, and nothing in the recipe deals with the problem yet.
//    If the maintainer has fixed it (or rewritten the recipe), the fix is
//    not applied and the package goes the usual helper way;
// 3. installs the missing build and run dependencies from the official
//    repositories (privilege session: `PrivOp::AurDeps`);
// 4. builds with `makepkg` as the user (never as root);
// 5. checks the built package: the files the fix removes must not be in it
//    ([`forbidden_in_package`]);
// 6. installs it through the same root path as the helpers' built packages
//    (`PrivOp::AurInstallBuilt`: the root helper checks the `.PKGINFO` of its
//    own copy against the requested name), then removes the build-only
//    dependencies it installed (`PrivOp::AurDropMake`, orphans only).
//
// It happens in the shared launch point ([`run_with_recipe_fixes`], called by
// `utils::run`), so every place that calls an AUR helper gets it (rule #12).

/// One edit of a PKGBUILD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeEdit {
    /// Insert `line` right after `anchor` (the broken line, as the recipe has
    /// it, whitespace around it aside) inside function `function`, with the
    /// anchor's indentation. Already done: the recipe mentions `done_marker`.
    InsertAfter { function: &'static str, anchor: &'static str, line: &'static str, done_marker: &'static str },
    /// Replace the line `from` (whitespace around it aside) with `to`. Already
    /// done: `to` is there and `from` is not.
    Replace { from: &'static str, to: &'static str },
}

/// A known breakage of an AUR recipe and its fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeFix {
    /// The package (`pkgname`) the fix is for.
    pub package: &'static str,
    /// The AUR repository (`pkgbase`) it is built from.
    pub pkgbase: &'static str,
    /// The edits, all or nothing.
    pub edits: &'static [RecipeEdit],
    /// From this `pkgver` on, upstream itself has fixed the problem: the fix
    /// is not applied (`None` — no fixed release yet).
    pub fixed_upstream_in: Option<&'static str>,
    /// Paths (inside the package, without a leading `/`) that must not be in
    /// the built package — the check that the fix worked.
    pub must_not_ship: &'static [&'static str],
}

/// mate-menu: `setup.py` compiles the GSettings schemas into the package (see
/// the section header). Upstream pull request ubuntu-mate/mate-menu#121;
/// once a release with it is out, `fixed_upstream_in` gets its version.
const MATE_MENU_EDITS: &[RecipeEdit] = &[RecipeEdit::InsertAfter {
    function: "package",
    anchor: r#"python setup.py install --root="$pkgdir" --optimize=1"#,
    line: r#"rm -f "$pkgdir/usr/share/glib-2.0/schemas/gschemas.compiled""#,
    done_marker: "gschemas.compiled",
}];

/// vala-panel-appmenu 25.04: every panel module is on by default, so the
/// recipe's `depends` (and the `.SRCINFO` the helpers resolve) drag in
/// budgie-desktop, xfce4-panel with xfconf and the AUR vala-panel even for
/// the MATE applet, and the Budgie module does not build against
/// budgie-desktop 10.10 at all (`budgie-1.0` not found, live 08.11.2026).
/// Each package is built with only its own module.
macro_rules! appmenu_switch {
    ($name:literal, false) => {
        RecipeEdit::Replace { from: concat!(": ${_build_", $name, ":=true}"), to: concat!(": ${_build_", $name, ":=false}") }
    };
}

const APPMENU_MATE_EDITS: &[RecipeEdit] =
    &[appmenu_switch!("xfce", false), appmenu_switch!("vala", false), appmenu_switch!("budgie", false)];
const APPMENU_XFCE_EDITS: &[RecipeEdit] =
    &[appmenu_switch!("mate", false), appmenu_switch!("vala", false), appmenu_switch!("budgie", false)];
const APPMENU_REGISTRAR_EDITS: &[RecipeEdit] = &[
    appmenu_switch!("mate", false),
    appmenu_switch!("xfce", false),
    appmenu_switch!("vala", false),
    appmenu_switch!("budgie", false),
];

/// The table of known breakages (checked against the AUR recipes of
/// 27.11.2026; see the section header).
pub const RECIPE_FIXES: &[RecipeFix] = &[
    RecipeFix {
        package: "mate-menu",
        pkgbase: "mate-menu",
        edits: MATE_MENU_EDITS,
        fixed_upstream_in: None,
        must_not_ship: &["usr/share/glib-2.0/schemas/gschemas.compiled"],
    },
    RecipeFix {
        package: "vala-panel-appmenu-mate",
        pkgbase: "vala-panel-appmenu",
        edits: APPMENU_MATE_EDITS,
        fixed_upstream_in: None,
        must_not_ship: &[],
    },
    RecipeFix {
        package: "vala-panel-appmenu-xfce",
        pkgbase: "vala-panel-appmenu",
        edits: APPMENU_XFCE_EDITS,
        fixed_upstream_in: None,
        must_not_ship: &[],
    },
    RecipeFix {
        package: "vala-panel-appmenu-registrar",
        pkgbase: "vala-panel-appmenu",
        edits: APPMENU_REGISTRAR_EDITS,
        fixed_upstream_in: None,
        must_not_ship: &[],
    },
];

/// The known breakage of `package`, if there is one.
pub fn recipe_fix_for(package: &str) -> Option<&'static RecipeFix> {
    RECIPE_FIXES.iter().find(|f| f.package == package)
}

/// What [`patch_recipe`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipePatch {
    /// The recipe is the expected one: the fixed text.
    Patched(String),
    /// The recipe (or upstream) already deals with the problem: build it as
    /// it is.
    AlreadyFixed,
    /// The recipe is not the one the fix was written for (the reason is for
    /// the log): build it as it is.
    Unexpected(&'static str),
}

/// The lines of function `name` in `pkgbuild`: (first body line, line after
/// the body). A PKGBUILD function opens with `name() {` (the brace may be on
/// the next line) and closes with a `}` at the start of a line.
fn function_body(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let head = lines.iter().position(|l| {
        let l = l.trim_start();
        l.strip_prefix(name).map(str::trim_start).is_some_and(|r| r.starts_with("()"))
    })?;
    let mut start = head + 1;
    if !lines[head].contains('{') {
        if lines.get(start).map(|l| l.trim()) != Some("{") {
            return None;
        }
        start += 1;
    }
    let end = (start..lines.len()).find(|&i| lines[i].starts_with('}'))?;
    Some((start, end))
}

/// `pkgver=` of a PKGBUILD (a plain value only).
fn recipe_pkgver(pkgbuild: &str) -> Option<String> {
    pkgbuild.lines().find_map(|l| l.trim().strip_prefix("pkgver=")).map(|v| v.trim_matches(['"', '\'']).to_string())
}

/// `a >= b` for plain dotted versions (`26.10.1`), segment by segment as
/// numbers where both are numbers.
fn version_at_least(a: &str, b: &str) -> bool {
    let seg = |v: &str| -> Vec<String> { v.split(['.', '-', '_', '+']).map(str::to_string).collect() };
    let (a, b) = (seg(a), seg(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).map(String::as_str).unwrap_or("0"), b.get(i).map(String::as_str).unwrap_or("0"));
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
    }
    true
}

/// Apply `fix` to the text of a PKGBUILD — only if the recipe is the one the
/// fix was written for (see the section header): every edit finds its line
/// exactly once, in its place. Nothing to do because everything is already
/// done (or upstream fixed it in this version) — [`RecipePatch::AlreadyFixed`].
pub fn patch_recipe(fix: &RecipeFix, pkgbuild: &str) -> RecipePatch {
    if let (Some(fixed_in), Some(ver)) = (fix.fixed_upstream_in, recipe_pkgver(pkgbuild)) {
        if version_at_least(&ver, fixed_in) {
            return RecipePatch::AlreadyFixed;
        }
    }
    let mut lines: Vec<String> = pkgbuild.lines().map(str::to_string).collect();
    let mut changed = false;
    for edit in fix.edits {
        let hits = |lines: &[String], text: &str| -> Vec<usize> {
            (0..lines.len()).filter(|&i| lines[i].trim() == text).collect()
        };
        match *edit {
            RecipeEdit::InsertAfter { function, anchor, line, done_marker } => {
                if pkgbuild.contains(done_marker) {
                    continue;
                }
                let found = hits(&lines, anchor);
                let [at] = found[..] else {
                    return RecipePatch::Unexpected(if found.is_empty() {
                        "the line to fix is not in the recipe"
                    } else {
                        "the line to fix is in the recipe more than once"
                    });
                };
                let Some((start, end)) = function_body(&lines, function) else {
                    return RecipePatch::Unexpected("the recipe has no such function");
                };
                if !(start..end).contains(&at) {
                    return RecipePatch::Unexpected("the line to fix is outside its function");
                }
                let indent: String = lines[at].chars().take_while(|c| c.is_whitespace()).collect();
                lines.insert(at + 1, format!("{indent}{line}"));
                changed = true;
            }
            RecipeEdit::Replace { from, to } => {
                let found = hits(&lines, from);
                if found.is_empty() && !hits(&lines, to).is_empty() {
                    continue;
                }
                let [at] = found[..] else {
                    return RecipePatch::Unexpected(if found.is_empty() {
                        "the line to fix is not in the recipe"
                    } else {
                        "the line to fix is in the recipe more than once"
                    });
                };
                if from != to {
                    let indent: String = lines[at].chars().take_while(|c| c.is_whitespace()).collect();
                    lines[at] = format!("{indent}{to}");
                    changed = true;
                }
            }
        }
    }
    if !changed {
        return RecipePatch::AlreadyFixed;
    }
    let mut text = lines.join("\n");
    if pkgbuild.ends_with('\n') {
        text.push('\n');
    }
    RecipePatch::Patched(text)
}

/// The edits for the build log.
pub fn recipe_edits_text(fix: &RecipeFix) -> String {
    fix.edits
        .iter()
        .map(|e| match e {
            RecipeEdit::InsertAfter { line, .. } => format!("+ {line}"),
            RecipeEdit::Replace { from, to } => format!("{from} -> {to}"),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Which of `fix.must_not_ship` are in the package listing `listing`
/// (`bsdtar -tf` output: one path per line, possibly with `./`).
pub fn forbidden_in_package(fix: &RecipeFix, listing: &str) -> Vec<String> {
    let files: Vec<&str> = listing.lines().map(|l| l.trim().trim_start_matches("./")).collect();
    fix.must_not_ship.iter().filter(|p| files.contains(p)).map(|p| p.to_string()).collect()
}

/// AUR helpers whose `-S` command line [`split_recipe_fixes`] understands.
const SPLIT_HELPERS: &[&str] = &["yay", "paru", "trizen", "pikaur", "aurman"];

/// An AUR helper install command split for [`run_with_recipe_fixes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixSplit {
    /// The packages of the command Rexon builds itself.
    pub fixes: Vec<&'static RecipeFix>,
    /// The person agreed to replace a conflicting package (`--ask=4`).
    pub replace: bool,
}

/// Which packages of the helper command `argv` have a known breakage and are
/// not installed yet (`installed`). `None` — nothing to do, the command runs
/// as it is.
pub fn split_recipe_fixes(argv: &[String], installed: &dyn Fn(&str) -> bool) -> Option<FixSplit> {
    let helper = argv.first().map(|a| a.rsplit('/').next().unwrap_or(a))?;
    if !SPLIT_HELPERS.contains(&helper) {
        return None;
    }
    let fixes: Vec<&'static RecipeFix> = argv_packages(argv)
        .iter()
        .filter_map(|p| recipe_fix_for(p))
        .filter(|f| !installed(f.package))
        .collect();
    if fixes.is_empty() {
        return None;
    }
    Some(FixSplit { fixes, replace: argv.iter().any(|a| a == "--ask=4") })
}

/// The helper command `argv` without the packages `drop`. `None` — no
/// package is left, there is nothing to run.
pub fn argv_without(argv: &[String], drop: &[&str]) -> Option<Vec<String>> {
    let Some(pos) = argv.iter().position(|a| a == "-S") else {
        return Some(argv.to_vec());
    };
    let mut out: Vec<String> = argv[..=pos].to_vec();
    let mut skip_value = false;
    let mut left = 0;
    for a in &argv[pos + 1..] {
        if skip_value {
            skip_value = false;
        } else if a.starts_with('-') {
            skip_value = VALUE_FLAGS.contains(&a.as_str());
        } else if drop.contains(&a.as_str()) {
            continue;
        } else {
            left += 1;
        }
        out.push(a.clone());
    }
    (left > 0).then_some(out)
}

/// Where Rexon builds a package with a fixed recipe:
/// `~/.cache/rexon/aur/build/<pkgbase>`.
pub fn recipe_build_dir(home: &str, pkgbase: &str) -> String {
    format!("{}/aur/build/{pkgbase}", crate::appearance_cache::cache_dir(home.trim_end_matches('/')))
}

/// The AUR address of a recipe.
pub fn aur_git_url(pkgbase: &str) -> String {
    format!("https://aur.archlinux.org/{pkgbase}.git")
}

/// How Rexon's own build of a package went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixedBuild {
    /// Built with the fix and installed.
    Installed,
    /// The fix does not apply (the recipe changed): the helper builds it.
    NotApplied,
    /// Failed: the reason for the person.
    Failed(String),
}

/// A command for [`run_with_recipe_fixes`]: captured output, the C locale.
fn fix_cmd(argv: Vec<String>, env: Vec<(String, String)>, timeout: u64) -> crate::utils::RunReport {
    crate::utils::run_direct(
        &crate::utils::RunSpec {
            argv,
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(timeout),
            lang_c: true,
            env,
            stderr_to_stdout: true,
        },
        None,
    )
}

/// The captured output of a command, for the build log.
fn report_text(r: &crate::utils::RunReport) -> String {
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A root operation for Rexon's own build: the privilege session, and
/// without its helper — the old path through `pkexec` with `fallback`.
fn fix_root(op: &crate::privilege::op::PrivOp, files: &[String], fallback: Vec<String>, say: &mut dyn FnMut(&str)) -> bool {
    match crate::privilege::run::run_live(op, files, None, &mut |l: &str| say(l)) {
        Some(out) => out.rc() == Some(0),
        None => {
            let mut argv = strings(&["pkexec"]);
            argv.extend(fallback);
            let r = fix_cmd(argv, Vec::new(), 3600);
            say(&String::from_utf8_lossy(&r.stdout));
            r.returncode == Some(0)
        }
    }
}

/// What Rexon's own build installs for `fix` and what it needs from outside:
/// the package itself plus the packages of the same recipe it depends on (a
/// split recipe: `vala-panel-appmenu-mate` needs `appmenu-glib-translator`),
/// and the dependencies of all of them that the recipe does not build itself.
/// `None` — the recipe (`.SRCINFO` → `pkgs`) has no such package.
pub fn fixed_build_targets(fix: &RecipeFix, pkgs: &[crate::privilege::aur::SrcPkg]) -> Option<(Vec<String>, Vec<String>)> {
    pkgs.iter().find(|p| p.name == fix.package)?;
    let mut targets = vec![fix.package.to_string()];
    targets.extend(crate::privilege::aur::aur_dep_closure(&targets, pkgs));
    let own = |n: &str| pkgs.iter().any(|p| p.name == n || p.provides.iter().any(|x| x == n));
    let mut needs: Vec<String> = Vec::new();
    for p in pkgs.iter().filter(|p| targets.contains(&p.name)) {
        for n in p.needs.iter().filter(|n| !n.is_empty() && !own(n)) {
            if !needs.contains(n) {
                needs.push(n.clone());
            }
        }
    }
    Some((targets, needs))
}

/// Whether package `name` is installed (`pacman -Q`).
pub fn live_installed(name: &str) -> bool {
    fix_cmd(strings(&["pacman", "-Q", "--", name]), Vec::new(), 30).returncode == Some(0)
}

/// Build `fix.package` with the fixed recipe and install it (see the section
/// header). `say` gets the build log line by line.
pub fn live_build_fixed(fix: &RecipeFix, replace: bool, say: &mut dyn FnMut(&str)) -> FixedBuild {
    use crate::privilege::op::{LocalFile, PrivOp};
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = recipe_build_dir(&home, fix.pkgbase);
    // A package the official repositories have is installed from there by the
    // helper itself; Rexon does not build over it.
    if fix_cmd(strings(&["pacman", "-Si", "--", fix.package]), Vec::new(), 60).returncode == Some(0) {
        return FixedBuild::NotApplied;
    }
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(parent) = std::path::Path::new(&dir).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    say(&format!("==> Rexon: fetching the AUR recipe of {}", fix.pkgbase));
    let clone = fix_cmd(strings(&["git", "clone", "--depth", "1", &aur_git_url(fix.pkgbase), &dir]), Vec::new(), 300);
    say(&report_text(&clone));
    if clone.returncode != Some(0) {
        return FixedBuild::Failed(tr("the recipe couldn't be downloaded from the AUR"));
    }
    let pkgbuild_path = format!("{dir}/PKGBUILD");
    let Ok(pkgbuild) = std::fs::read_to_string(&pkgbuild_path) else {
        return FixedBuild::Failed(tr("the recipe couldn't be downloaded from the AUR"));
    };
    let patched = match patch_recipe(fix, &pkgbuild) {
        RecipePatch::Patched(t) => t,
        RecipePatch::AlreadyFixed => {
            say("==> Rexon: the recipe already deals with the problem, building it as it is");
            crate::log_info!("aur", "recipe fix for {}: not needed any more", fix.package);
            return FixedBuild::NotApplied;
        }
        RecipePatch::Unexpected(why) => {
            say(&format!("==> Rexon: the recipe changed ({why}), building it as it is"));
            crate::log_info!("aur", "recipe fix for {}: not applied: {why}", fix.package);
            return FixedBuild::NotApplied;
        }
    };
    if std::fs::write(&pkgbuild_path, patched).is_err() {
        return FixedBuild::Failed(tr("the recipe couldn't be downloaded from the AUR"));
    }
    say(&format!("==> Rexon: fixed the recipe: {}", recipe_edits_text(fix)));
    crate::log_info!("aur", "recipe fix for {}: applied in {dir}", fix.package);

    // The recipe's switches decide the package list and the dependencies:
    // `.SRCINFO` is made anew from the fixed recipe.
    let srcinfo_cmd = fix_cmd(
        strings(&["sh", "-c", "cd \"$1\" && makepkg --printsrcinfo > .SRCINFO", "sh", &dir]),
        Vec::new(),
        120,
    );
    if srcinfo_cmd.returncode != Some(0) {
        say(&report_text(&srcinfo_cmd));
        return FixedBuild::Failed(tr("the package didn't build"));
    }
    // Dependencies (build and run) that are missing: only from the official
    // repositories. One not found there — let the helper deal with it.
    let srcinfo = std::fs::read_to_string(format!("{dir}/.SRCINFO")).unwrap_or_default();
    let pkgs = crate::privilege::aur::parse_srcinfo_pkgs(&srcinfo);
    let Some((targets, wanted)) = fixed_build_targets(fix, &pkgs) else {
        say("==> Rexon: the recipe has no such package, building it as it is");
        return FixedBuild::NotApplied;
    };
    let mut missing: Vec<String> = Vec::new();
    if !wanted.is_empty() {
        let mut argv = strings(&["pacman", "-T", "--"]);
        argv.extend(wanted.iter().cloned());
        let r = fix_cmd(argv, Vec::new(), 60);
        missing = String::from_utf8_lossy(&r.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    }
    let mut installed_deps: Vec<String> = Vec::new();
    if !missing.is_empty() {
        let mut argv = strings(&["pacman", "-Sp", "--print-format", "%n", "--"]);
        argv.extend(missing.iter().cloned());
        let r = fix_cmd(argv, Vec::new(), 120);
        if r.returncode != Some(0) {
            say("==> Rexon: a dependency is not in the official repositories, building it the usual way");
            return FixedBuild::NotApplied;
        }
        installed_deps = String::from_utf8_lossy(&r.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        say(&format!("==> Rexon: installing dependencies: {}", installed_deps.join(" ")));
        let mut fallback = strings(&["pacman", "-S", "--needed", "--noconfirm", "--asdeps", "--"]);
        fallback.extend(installed_deps.iter().cloned());
        if !fix_root(&PrivOp::AurDeps { names: installed_deps.clone() }, &[], fallback, say) {
            return FixedBuild::Failed(tr("the packages needed to build it couldn't be installed"));
        }
    }

    // The build: as the user, the output stays in Rexon's directory whatever
    // the person's makepkg.conf says.
    say(&format!("==> Rexon: building {}", fix.package));
    let env: Vec<(String, String)> =
        ["PKGDEST", "SRCDEST", "BUILDDIR", "LOGDEST"].iter().map(|k| (k.to_string(), dir.clone())).collect();
    let build = fix_cmd(
        strings(&["sh", "-c", "cd \"$1\" && exec makepkg -f --noconfirm --cleanbuild", "sh", &dir]),
        env,
        3600,
    );
    say(&report_text(&build));
    let files: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| !n.ends_with(".sig"))
                .filter(|n| crate::privilege::helper::pkgname_from_file(n).is_some_and(|p| targets.contains(&p)))
                .map(|n| format!("{dir}/{n}"))
                .collect()
        })
        .unwrap_or_default();
    if build.returncode != Some(0) || files.len() != targets.len() {
        return FixedBuild::Failed(tr("the package didn't build"));
    }
    for file in &files {
        let listing = fix_cmd(strings(&["bsdtar", "-tf", file]), Vec::new(), 120);
        let left = forbidden_in_package(fix, &String::from_utf8_lossy(&listing.stdout));
        if listing.returncode != Some(0) || !left.is_empty() {
            say(&format!("==> Rexon: the fix didn't work, {file} still has: {}", left.join(" ")));
            return FixedBuild::Failed(tr("the package didn't build"));
        }
    }

    say(&format!("==> Rexon: installing {}", files.join(" ")));
    let op = PrivOp::AurInstallBuilt {
        packages: files.iter().enumerate().map(|(i, f)| LocalFile { slot: i as u8, label: f.clone() }).collect(),
        allowed: targets.clone(),
        replace,
    };
    let mut fallback = strings(&["pacman", "-U", "--noconfirm"]);
    if replace {
        fallback.push("--ask=4".to_string());
    }
    fallback.push("--".to_string());
    fallback.extend(files.iter().cloned());
    fix_root(&op, &files, fallback, say);
    let ok = live_installed(fix.package);

    // Build-only dependencies this build installed: removed again (the
    // helper removes only orphans), as `--removemake` does for the helpers.
    let srcinfo_make: Vec<String> = srcinfo
        .lines()
        .filter_map(|l| l.trim().strip_prefix("makedepends = "))
        .map(|d| d.split(['<', '>', '=']).next().unwrap_or("").trim().to_string())
        .collect();
    let drop: Vec<String> = installed_deps.iter().filter(|d| srcinfo_make.contains(d)).cloned().collect();
    if !drop.is_empty() {
        let op = PrivOp::AurDropMake { names: drop };
        let _ = crate::privilege::run::run_live(&op, &[], None, &mut |l: &str| say(l));
    }
    if ok {
        FixedBuild::Installed
    } else {
        FixedBuild::Failed(tr("the built package couldn't be installed"))
    }
}

/// The shared launch point's hook (`utils::run`): an AUR helper command that
/// installs a package from [`RECIPE_FIXES`] — that package is built by Rexon
/// itself ([`live_build_fixed`]), the rest of the command runs as usual. The
/// build log goes to the same place as the command's output (the log file
/// or the captured output), so the callers read the reason as before.
/// `None` — not such a command.
pub fn run_with_recipe_fixes(spec: &crate::utils::RunSpec, path_env: Option<&str>) -> Option<crate::utils::RunReport> {
    use crate::utils::{RunOutcome, RunOutput, RunReport, RunSpec};
    if std::env::var("REXON_AUR_RECIPE_FIXES").is_ok_and(|v| v == "0") {
        return None;
    }
    let split = split_recipe_fixes(&spec.argv, &live_installed)?;
    let mut transcript = String::new();
    let mut handled: Vec<&str> = Vec::new();
    let mut failed = false;
    for fix in &split.fixes {
        crate::log_info!("aur", "building {} with a fixed recipe", fix.package);
        let mut say = |l: &str| {
            if !l.is_empty() {
                transcript.push_str(l);
                transcript.push('\n');
            }
        };
        match live_build_fixed(fix, split.replace, &mut say) {
            FixedBuild::Installed => handled.push(fix.package),
            FixedBuild::NotApplied => {}
            FixedBuild::Failed(why) => {
                crate::log_warn!("aur", "{} with a fixed recipe: {why}", fix.package);
                transcript.push_str(&format!("==> ERROR: {}: {why}\n", fix.package));
                handled.push(fix.package);
                failed = true;
            }
        }
    }
    let mut report = match argv_without(&spec.argv, &handled) {
        Some(argv) => crate::utils::run_direct(&RunSpec { argv, ..spec.clone() }, path_env),
        None => {
            let mut r = RunReport { timeout_secs: spec.timeout_secs.unwrap_or(3600), ..RunReport::default() };
            r.outcome = if failed { RunOutcome::Failed } else { RunOutcome::Ok };
            r.returncode = Some(if failed { 1 } else { 0 });
            if let RunOutput::File(path) = &spec.output {
                // The command did not run: the log starts afresh, as it would.
                let _ = std::fs::write(path, "");
            }
            r
        }
    };
    if failed && report.returncode == Some(0) {
        report.outcome = RunOutcome::Failed;
        report.returncode = Some(1);
    }
    match &spec.output {
        RunOutput::File(path) => {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(transcript.as_bytes());
            }
        }
        RunOutput::Capture | RunOutput::CaptureBytes => {
            let mut out = transcript.into_bytes();
            out.extend_from_slice(&report.stdout);
            report.stdout = out;
        }
        RunOutput::Inherit => {}
    }
    Some(report)
}

// ════════════════════════════════════════════════════════════════════════
// Kits: a fork package is installed together with its parts and a question
// ════════════════════════════════════════════════════════════════════════

/// An AUR package kit that is installed in one go and only after a
/// warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AurKit {
    /// The kit name for the person.
    pub title: &'static str,
    /// Packages by which the kit is recognised (the "Install" button on them
    /// installs the whole kit).
    pub triggers: &'static [&'static str],
    /// Contents in the order of the authors' instructions (AeroThemePlasma
    /// INSTALL.md, the "Arch Linux" section): first the libplasma fork, then
    /// the parts of the AeroShell shell, then sounds and icons, and the shell
    /// itself last. The helper builds the order from dependencies by itself;
    /// the explicit list is needed so that the libplasma fork is provided by
    /// EXACTLY this package rather than the first provider that comes along,
    /// and so that the kit is the same as the authors'.
    pub packages: &'static [&'static str],
    /// What the kit REPLACES in the system: (system package, fork package).
    pub replaces: &'static [(&'static str, &'static str)],
    /// Whether the kit installs a separate session on the login screen (session name).
    pub session: Option<&'static str>,
}

/// AeroThemePlasma: a "like Windows 7" shell on top of KDE Plasma.
///
/// ⚠️ This is a Plasma fork, not a theme: `aeroshell-libplasma-git`
/// replaces the system `libplasma` (Conflicts: libplasma), the KWin
/// effects are built against the headers of the installed Plasma and need
/// a rebuild after it is updated ([`crate::aeroshell`] does that).
pub const AEROTHEMEPLASMA: AurKit = AurKit {
    title: "AeroThemePlasma",
    triggers: &["aerothemeplasma-desktop-git"],
    packages: &[
        "aeroshell-libplasma-git",
        "aeroshell-workspace-git",
        "aeroshell-kwin-components-git",
        "aeroshell-smod-git",
        "aerothemeplasma-sounds-git",
        "aerothemeplasma-icons-git",
        "aerothemeplasma-desktop-git",
    ],
    replaces: &[("libplasma", "aeroshell-libplasma-git")],
    session: Some("AeroThemePlasma (Wayland)"),
};

/// AeroThemePlasma for an X11 session: the same kit plus the parts for
/// `kwin-x11` (their dependencies per AUR RPC 04.10.2026: desktop-x11 pulls
/// in desktop, kwin-components-x11 and smodglow-x11).
pub const AEROTHEMEPLASMA_X11: AurKit = AurKit {
    title: "AeroThemePlasma (X11)",
    triggers: &["aerothemeplasma-desktop-x11-git"],
    packages: &[
        "aeroshell-libplasma-git",
        "aeroshell-workspace-git",
        "aeroshell-kwin-components-git",
        "aeroshell-smod-git",
        "aerothemeplasma-sounds-git",
        "aerothemeplasma-icons-git",
        "aerothemeplasma-desktop-git",
        "aeroshell-kwin-components-x11-git",
        "aeroshell-smodglow-x11-git",
        "aerothemeplasma-desktop-x11-git",
    ],
    replaces: &[("libplasma", "aeroshell-libplasma-git")],
    session: Some("AeroThemePlasma (X11)"),
};

/// All kits. A new one goes here (rule #12: one table).
pub const KITS: &[AurKit] = &[AEROTHEMEPLASMA, AEROTHEMEPLASMA_X11];

/// The kit that the "Install" button on package `name` belongs to.
pub fn kit_for(name: &str) -> Option<&'static AurKit> {
    KITS.iter().find(|k| k.triggers.contains(&name))
}

/// The question before installation: a title and a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallQuestion {
    pub title: String,
    pub text: String,
}

/// What to install and how for the "Install" button on an AUR package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Packages in installation order.
    pub packages: Vec<String>,
    /// Allow pacman to replace a conflicting system package.
    pub replace_conflicts: bool,
    /// Ask the person before starting (None means install right away).
    pub question: Option<InstallQuestion>,
    /// The kit, if this is one.
    pub kit: Option<&'static AurKit>,
}

/// Installation plan for package `name`. `installed` tells whether the
/// package is in the system.
///
/// A plain package is installed alone, without a question and without the
/// right to replace system packages. A kit package means the whole kit
/// (parts already installed are skipped: the helper with `--needed` would
/// not touch them, but we do not pull them into the list either, so the
/// question tells the truth), with a question; replacing a system package
/// is allowed only if it is really about to happen.
pub fn plan_install(name: &str, installed: &dyn Fn(&str) -> bool) -> InstallPlan {
    let Some(kit) = kit_for(name) else {
        return InstallPlan {
            packages: vec![name.to_string()],
            replace_conflicts: false,
            question: None,
            kit: None,
        };
    };
    let packages: Vec<String> =
        kit.packages.iter().filter(|p| !installed(p) || **p == name).map(|p| p.to_string()).collect();
    let replacing: Vec<&(&str, &str)> =
        kit.replaces.iter().filter(|(sys, fork)| !installed(fork) && installed(sys)).collect();
    InstallPlan {
        replace_conflicts: !replacing.is_empty(),
        question: Some(kit_question(kit, packages.len(), &replacing)),
        packages,
        kit: Some(kit),
    }
}

/// Warning before a fork kit.
fn kit_question(kit: &AurKit, count: usize, replacing: &[&(&str, &str)]) -> InstallQuestion {
    let mut text = trf(
        // TRANSLATORS: %1 is a theme name (AeroThemePlasma), %2 a number of packages.
        "%1 isn't an ordinary theme: it is a modified KDE Plasma (a fork). Rexon will build \
         it from source (packages: %2) — this takes 10 to 30 minutes, and the system will ask \
         for the administrator password a few times.",
        &[&kit.title, &count],
    );
    for (sys, fork) in replacing {
        text.push_str("\n\n• ");
        text.push_str(&trf(
            "The system package %1 will be replaced by %2.",
            &[sys, fork],
        ));
    }
    text.push_str("\n• ");
    text.push_str(&tr(
        "After every KDE Plasma update it has to be rebuilt — Rexon checks this at start-up \
         and offers the rebuild itself.",
    ));
    if let Some(session) = kit.session {
        text.push_str("\n• ");
        text.push_str(&trf(
            "To use it, log out and pick the “%1” session on the login screen.",
            &[&session],
        ));
    }
    text.push_str("\n\n");
    text.push_str(&tr("Continue?"));
    InstallQuestion {
        title: trf("Install %1", &[&kit.title]),
        text,
    }
}

/// Result of a successful kit installation: where to go next.
pub fn kit_done_message(kit: &AurKit) -> String {
    match kit.session {
        Some(session) => trf(
            "“%1” is installed. Log out and pick the “%2” session on the login screen.",
            &[&kit.title, &session],
        ),
        None => trf("“%1” is installed", &[&kit.title]),
    }
}

// ════════════════════════════════════════════════════════════════════════
// Parsing the helper's output: the real reason for a failure
// ════════════════════════════════════════════════════════════════════════

/// Why the AUR helper did not install the package.
///
/// Privilege failures differ by WHAT the person should do next (a live
/// case on 05.10.2026: three installations in a row -> three password
/// windows; the KDE agent refused the second and third with "Another
/// client is already authenticating", pkexec answered "Not authorized",
/// and Rexon wrote "password not accepted": untrue, nobody had entered a
/// password):
/// * [`AurFailure::WrongPassword`]: a password was entered but is wrong;
/// * [`AurFailure::PasswordDismissed`]: the password window was closed
///   (pkexec 126);
/// * [`AurFailure::NotAuthorized`]: privileges were not granted, and the
///   system did not say why (pkexec 127): the window was closed, timed out
///   or did not open;
/// * [`AurFailure::PasswordWindowBusy`]: the window did not open because
///   another one was already open (refined from the agent's journal,
///   [`refine_with_agent_journal`]);
/// * [`AurFailure::NoPasswordAgent`]: there is nowhere for a password
///   window to come from at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AurFailure {
    /// sudo without a terminal could not ask for the password.
    NoTerminalForPassword,
    /// The password window was closed / "Cancel" was pressed.
    PasswordDismissed,
    /// Privileges were not granted; the system did not name the reason.
    NotAuthorized,
    /// The password was entered incorrectly.
    WrongPassword,
    /// The password window did not open: another one was already open.
    PasswordWindowBusy,
    /// No polkit agent: there is nowhere for a password window to come from.
    NoPasswordAgent,
    /// The built packages were not installed, and pacman named no reason: the
    /// privilege escalation command returned an error silently.
    PrivilegeFailed,
    /// Package `new` conflicts with the installed `old`.
    Conflict { new: String, old: String },
    /// The file already exists in the system (owned by someone else or by nobody).
    FileExists { path: String },
    /// Installing or removing would break someone else's dependency.
    BreaksDependency { line: String },
    /// The source failed to download.
    Download { what: String },
    /// Unknown PGP key for the sources.
    PgpKey,
    /// The checksum of the download did not match.
    Checksum,
    /// The package build failed.
    Build { package: String },
    /// A dependency was found neither in the repositories nor in the AUR.
    MissingDependency { name: String },
    /// The build of `package` lacked the dependencies `names` that should have
    /// been installed by then (makepkg "Missing dependencies"): another
    /// installation was installing them, or they failed to install themselves.
    DependencyNotInstalled { package: String, names: Vec<String> },
    /// The package database is held by another installation.
    DatabaseLocked,
    /// The disk ran out of space.
    DiskFull,
}

/// Remove colour escape sequences (`\x1b[1m` etc.) and progress line
/// redraws (`\r`).
pub fn clean_line(raw: &str) -> String {
    let raw = raw.rsplit('\r').next().unwrap_or(raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Meaningful output lines (without empty ones and progress noise).
fn lines(log: &str) -> Vec<String> {
    log.lines().map(clean_line).filter(|l| !l.is_empty()).collect()
}

/// The text between the first pair of single quotes (`'…'`) or None.
fn quoted(line: &str) -> Option<String> {
    let start = line.find('\'')? + 1;
    let end = line[start..].find('\'')? + start;
    Some(line[start..end].to_string())
}

/// Package name without the version: `libplasma-6.7.4-1` -> `libplasma`.
fn strip_version(spec: &str) -> String {
    let parts: Vec<&str> = spec.split('-').collect();
    // A pacman version is the last two pieces "ver-rel", where ver starts
    // with a digit (or with an epoch "N:").
    if parts.len() >= 3 {
        let ver = parts[parts.len() - 2];
        if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return parts[..parts.len() - 2].join("-");
        }
    }
    spec.to_string()
}

/// Find the real reason for the failure in the helper's output.
///
/// Reasons are checked from root cause to consequence: "no password" beats
/// "failed to install the layer", a conflict beats "failed to prepare the
/// transaction". None means nothing recognisable (then the tail is shown).
pub fn failure_cause(log: &str) -> Option<AurFailure> {
    let lines = lines(log);
    let has = |needles: &[&str]| lines.iter().any(|l| needles.iter().any(|n| l.contains(n)));
    let find = |needles: &[&str]| {
        lines.iter().rev().find(|l| needles.iter().any(|n| l.contains(n))).cloned()
    };

    // ── Privileges ──
    if has(&["a terminal is required to read the password", "для ввода пароля требуется терминал", "no askpass program specified"]) {
        return Some(AurFailure::NoTerminalForPassword);
    }
    if has(&["No authentication agent found", "Агент аутентификации не найден"]) {
        return Some(AurFailure::NoPasswordAgent);
    }
    if has(&["Request dismissed", "Запрос отклонён", "Запрос отклонен"]) {
        return Some(AurFailure::PasswordDismissed);
    }
    // A wrong password is reported by the text polkit agent (pkttyagent:
    // "AUTHENTICATION FAILED", the PAM helper "Authentication failure") and
    // by sudo ("incorrect password attempt"). The graphical agent does not
    // write to pkexec's output itself; its case is refined by the agent's
    // journal.
    if has(&[
        "AUTHENTICATION FAILED",
        "ОШИБКА АУТЕНТИФИКАЦИИ",
        "pam_authenticate failed",
        "Authentication failure",
        "incorrect password attempt",
        "неверные попытки ввода пароля",
        "Sorry, try again",
    ]) {
        return Some(AurFailure::WrongPassword);
    }
    if has(&["Not authorized", "Не авторизован"]) {
        return Some(AurFailure::NotAuthorized);
    }
    // No pkexec words, but its exit code: 126 means the window was closed,
    // 127 means privileges were not granted ("error installing: […] - exit
    // status 127").
    if let Some(line) = find(&["error installing:", "ошибка установки:"]) {
        if line.ends_with("exit status 126") {
            return Some(AurFailure::PasswordDismissed);
        }
        if line.ends_with("exit status 127") {
            return Some(AurFailure::NotAuthorized);
        }
    }

    // ── Package conflicts: ":: a-1-1 and b-2-1 are in conflict" ──
    // (this pacman line is not translated in the Russian catalog: always English).
    if let Some(line) = find(&[" are in conflict"]) {
        let body = line.trim_start_matches(':').trim();
        let body = body.split(" are in conflict").next().unwrap_or("");
        if let Some((a, b)) = body.split_once(" and ") {
            return Some(AurFailure::Conflict { new: strip_version(a.trim()), old: strip_version(b.trim()) });
        }
    }
    if let Some(line) = find(&[" exists in filesystem", " существует в файловой системе"]) {
        // «pkg: /usr/share/x exists in filesystem (owned by y)»
        let path = line
            .split_whitespace()
            .find(|w| w.trim_matches('\'').starts_with('/'))
            .map(|w| w.trim_matches('\'').to_string())
            .unwrap_or_default();
        return Some(AurFailure::FileExists { path });
    }
    if let Some(line) = find(&["breaks dependency", "нарушает зависимость"]) {
        return Some(AurFailure::BreaksDependency { line: line.trim_start_matches(':').trim().to_string() });
    }

    // ── Sources ──
    if has(&["One or more PGP signatures could not be verified", "PGP-подписей не могут быть проверены", "unknown public key"]) {
        return Some(AurFailure::PgpKey);
    }
    if has(&["did not pass the validity check", "не прошли проверку целостности"]) {
        return Some(AurFailure::Checksum);
    }
    if let Some(line) = find(&[
        "Failure while downloading",
        "Ошибка при загрузке",
        "error downloading sources",
        "ошибка загрузки исходников",
        "Could not resolve host",
        "unable to access '",
        "failed retrieving file",
    ]) {
        let what = quoted(&line).unwrap_or_else(|| {
            line.rsplit(' ').next().unwrap_or("").trim_end_matches(['.', ':']).to_string()
        });
        return Some(AurFailure::Download { what });
    }

    // ── Dependencies ──
    // makepkg without `-s` (as the helper calls it): "==> Missing
    // dependencies:", followed by lines "  -> name", then "==> ERROR: Could
    // not resolve all dependencies." This is what the race of 05.10 looked
    // like: a neighbouring installation was installing the dependency and did
    // not make it in time.
    if let Some(pos) = lines.iter().rposition(|l| {
        l.contains("Missing dependencies:") || l.contains("Отсутствуют зависимости:")
    }) {
        let names: Vec<String> = lines[pos + 1..]
            .iter()
            .take_while(|l| l.starts_with("->"))
            .map(|l| l.trim_start_matches("->").trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let package = lines[..pos]
            .iter()
            .rev()
            .find(|l| l.contains("==> Making package:") || l.contains("==> Создание пакета:"))
            .and_then(|l| l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from))
            .unwrap_or_default();
        return Some(AurFailure::DependencyNotInstalled { package, names });
    }
    if has(&["Could not resolve all dependencies", "Не удалось разрешить все зависимости"]) {
        return Some(AurFailure::DependencyNotInstalled { package: String::new(), names: Vec::new() });
    }
    if let Some(line) = find(&["unable to satisfy dependency", "не удается удовлетворить зависимость", "target not found:", "не найдена цель:", "No AUR package found for", "Не найден пакет AUR для"]) {
        let name = quoted(&line).unwrap_or_else(|| {
            line.rsplit([' ', ':']).find(|w| !w.is_empty()).unwrap_or("").to_string()
        });
        return Some(AurFailure::MissingDependency { name });
    }

    // ── Build ──
    if has(&["A failure occurred in", "Произошел сбой в"]) || has(&["error making:", "ошибка сборки:"]) {
        // The name comes from the last "==> Making package: name version"
        // (makepkg, the format is stable); the fallback is the first word after
        // "error making:".
        let package = find(&["==> Making package:", "==> Создание пакета:"])
            .and_then(|l| {
                l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from)
            })
            .or_else(|| {
                find(&["error making:", "ошибка сборки:"]).and_then(|l| {
                    l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from)
                })
            })
            .unwrap_or_default();
        return Some(AurFailure::Build { package });
    }

    // ── Other ──
    if has(&["unable to lock database", "не удалось заблокировать базу"]) {
        return Some(AurFailure::DatabaseLocked);
    }
    if has(&["not enough free disk space", "No space left on device", "недостаточно места на диске"]) {
        return Some(AurFailure::DiskFull);
    }
    // What was built did not install, and pacman itself complained about
    // nothing, so it was not given privileges: the privilege escalation
    // command returned an error silently (a fake `--sudo /bin/false` refusal
    // looks the same). yay writes "error installing: [packages] - exit status
    // N".
    if has(&["error installing:", "ошибка установки:"]) {
        return Some(AurFailure::PrivilegeFailed);
    }
    None
}

/// The failure reason in the person's words.
pub fn failure_text(cause: &AurFailure) -> String {
    match cause {
        AurFailure::NoTerminalForPassword => tr(
            "the system couldn't ask for the administrator password (sudo needs a terminal)",
        ),
        AurFailure::PasswordDismissed => tr("the password window was closed"),
        AurFailure::NotAuthorized => tr(
            "the system didn't grant administrator rights: the password window was closed, \
             didn't wait for the password or didn't open",
        ),
        AurFailure::WrongPassword => tr("the administrator password was wrong"),
        AurFailure::PasswordWindowBusy => tr(
            "the password window didn't open: another password window was already open",
        ),
        AurFailure::NoPasswordAgent => tr(
            "no password dialog is running (polkit agent), so the system couldn't ask for \
             the administrator password",
        ),
        AurFailure::PrivilegeFailed => tr(
            "the packages were built, but installing them needs administrator rights, and \
             they weren't granted",
        ),
        AurFailure::Conflict { new, old } => trf(
            "%1 conflicts with the installed %2 — it has to replace it",
            &[new, old],
        ),
        AurFailure::FileExists { path } if !path.is_empty() => trf(
            "the file %1 is already on the system and belongs to something else",
            &[path],
        ),
        AurFailure::FileExists { .. } => tr("some of its files are already on the system and belong to something else"),
        AurFailure::BreaksDependency { line } => trf("it would break another package: %1", &[line]),
        AurFailure::Download { what } if !what.is_empty() => trf(
            "the source code couldn't be downloaded (%1) — check the internet connection",
            &[what],
        ),
        AurFailure::Download { .. } => {
            tr("the source code couldn't be downloaded — check the internet connection")
        }
        AurFailure::PgpKey => tr(
            "the signature of the source code couldn't be checked: the author's PGP key is unknown",
        ),
        AurFailure::Checksum => tr("the downloaded source code failed the integrity check"),
        AurFailure::Build { package } if !package.is_empty() => {
            trf("building %1 from source failed", &[package])
        }
        AurFailure::Build { .. } => tr("building from source failed"),
        AurFailure::MissingDependency { name } if !name.is_empty() => trf(
            "the dependency %1 wasn't found in the repositories or the AUR",
            &[name],
        ),
        AurFailure::MissingDependency { .. } => {
            tr("a dependency wasn't found in the repositories or the AUR")
        }
        AurFailure::DependencyNotInstalled { package, names } => {
            let names = names.join(", ");
            match (package.is_empty(), names.is_empty()) {
                (false, false) => trf(
                    "%1 couldn't be built: its dependencies %2 weren't installed yet",
                    &[package, &names],
                ),
                (true, false) => trf("the dependencies %1 weren't installed yet", &[&names]),
                _ => tr("some dependencies weren't installed yet"),
            }
        }
        AurFailure::DatabaseLocked => tr("the package database is busy with another installation"),
        AurFailure::DiskFull => tr("there isn't enough free disk space"),
    }
}

/// The last `n` meaningful output lines, for the installation progress window.
pub fn tail_lines(log: &str, n: usize) -> Vec<String> {
    let all = lines(log);
    let from = all.len().saturating_sub(n);
    all[from..].to_vec()
}

/// How many tail lines to show on failure.
pub const FAILURE_TAIL_LINES: usize = 8;

/// The failure result for the person: what did not install, why, and where
/// the full log is.
///
/// `log_path` is where the helper's full output was saved (empty means not
/// saved). ⛔ No "run this in a terminal" advice (Wehrwolfmann 27.11.2026):
/// the person came from Windows and has no terminal; what can be done is a
/// button next to the message ("Retry", "Open log").
pub fn failure_message(name: &str, log: &str, log_path: &str) -> String {
    failure_message_for(name, log, log_path, failure_cause(log).as_ref())
}

/// The same as [`failure_message`], but with a reason already found (and
/// possibly refined from the agent's journal, [`refine_with_agent_journal`]).
pub fn failure_message_for(name: &str, log: &str, log_path: &str, cause: Option<&AurFailure>) -> String {
    let mut msg = match cause {
        Some(c) => trf("Couldn't install “%1”: %2.", &[&name, &failure_text(c)]),
        None => match tail_lines(log, 1).first() {
            Some(last) => trf("Couldn't install “%1”. The installer's last words: %2", &[&name, last]),
            None => trf("Couldn't install “%1”.", &[&name]),
        },
    };
    if let Some(advice) = cause.and_then(failure_advice) {
        msg.push(' ');
        msg.push_str(&advice);
    }
    if !log_path.is_empty() {
        msg.push('\n');
        msg.push_str(&trf("Full log: %1", &[&log_path]));
    }
    msg
}

/// What the person should do after the failure: one phrase for the reason
/// (None means there is nothing to say beyond the reason itself).
pub fn failure_advice(cause: &AurFailure) -> Option<String> {
    Some(match cause {
        AurFailure::PasswordDismissed | AurFailure::NotAuthorized => {
            tr("Press “Retry” and enter your password in the window that opens.")
        }
        AurFailure::WrongPassword => {
            tr("Press “Retry” and enter the password of your user account again.")
        }
        AurFailure::PasswordWindowBusy => tr(
            "Finish or close the other password window (another program asked for it), \
             then press “Retry”.",
        ),
        AurFailure::NoPasswordAgent => tr(
            "Log out and back in; if that doesn't help, install the password dialog \
             (polkit agent) for your desktop.",
        ),
        AurFailure::NoTerminalForPassword | AurFailure::PrivilegeFailed => tr("Press “Retry”."),
        AurFailure::DependencyNotInstalled { .. } => tr(
            "Press “Retry”: Rexon installs one package at a time, so the dependencies \
             will be in place.",
        ),
        AurFailure::MissingDependency { .. } => tr(
            "The package may be outdated in the AUR — check its page for comments.",
        ),
        AurFailure::Build { .. }
        | AurFailure::Conflict { .. }
        | AurFailure::FileExists { .. }
        | AurFailure::BreaksDependency { .. } => tr(
            "This is usually a problem of the package itself — check its page in the AUR \
             for comments, or try again later.",
        ),
        AurFailure::PgpKey => tr(
            "The key the package is signed with isn't known on this computer — check the \
             package's page in the AUR for comments, or try again later.",
        ),
        AurFailure::Download { .. } => tr("Check the internet connection and press “Retry”."),
        AurFailure::Checksum => tr("Press “Retry” — the download may have been damaged."),
        AurFailure::DatabaseLocked => {
            tr("Wait until the other installation finishes and press “Retry”.")
        }
        AurFailure::DiskFull => tr("Free up some disk space and press “Retry”."),
    })
}

/// A short reason for the tile label (without the package name and
/// advice). Lower case: on the tile it follows "Error:", and the first
/// word of a reason is sometimes a package name, which capitalisation
/// would distort.
pub fn failure_short(cause: Option<&AurFailure>) -> String {
    match cause {
        Some(c) => failure_text(c),
        None => tr("The installation failed — see the log"),
    }
}

/// Refine the "privileges not granted" reason from the system journal for
/// the duration of the installation (`journalctl -o cat --since …`).
///
/// pkexec says the same "Not authorized" both when the password window was
/// closed and when the polkit agent did not even open it because it was
/// already showing another. The graphical agent writes the real reason
/// only to its own journal: the KDE agent writes "Another client is
/// already authenticating, please try again later.", and a wrong password
/// shows up as the PAM "authentication failure" entry of the `polkit-1`
/// service (measured 05.10.2026, KDE Plasma 6.7).
pub fn refine_with_agent_journal(cause: Option<AurFailure>, journal: &str) -> Option<AurFailure> {
    match cause {
        Some(AurFailure::NotAuthorized) | Some(AurFailure::PrivilegeFailed) => {
            if journal.contains("Another client is already authenticating") {
                Some(AurFailure::PasswordWindowBusy)
            } else if journal.lines().any(|l| {
                l.contains("polkit-1") && l.contains("authentication failure")
            }) {
                Some(AurFailure::WrongPassword)
            } else {
                cause
            }
        }
        other => other,
    }
}

// ════════════════════════════════════════════════════════════════════════
// Installation stages: what the helper is doing right now
// ════════════════════════════════════════════════════════════════════════

/// Where an AUR package installation is now, for the tile label and the top line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AurStage {
    /// Waiting for its turn: in Rexon AUR installations run strictly one at a time.
    Queued,
    /// Waiting for another installation to release the package database.
    WaitingLock,
    /// Downloading build files and sources (and packages from the repositories).
    Downloading,
    /// Building (makepkg).
    Building,
    /// Installing what was built (pacman).
    Installing,
    /// Installed.
    Done,
    /// Failed to install.
    Failed,
}

impl AurStage {
    /// Name for the exchange with the page.
    pub fn as_str(self) -> &'static str {
        match self {
            AurStage::Queued => "queued",
            AurStage::WaitingLock => "waiting",
            AurStage::Downloading => "downloading",
            AurStage::Building => "building",
            AurStage::Installing => "installing",
            AurStage::Done => "done",
            AurStage::Failed => "failed",
        }
    }

    /// Back from [`AurStage::as_str`].
    pub fn parse(s: &str) -> Option<AurStage> {
        [
            AurStage::Queued,
            AurStage::WaitingLock,
            AurStage::Downloading,
            AurStage::Building,
            AurStage::Installing,
            AurStage::Done,
            AurStage::Failed,
        ]
        .into_iter()
        .find(|st| st.as_str() == s)
    }

    /// Still running (queued or in progress).
    pub fn active(self) -> bool {
        !matches!(self, AurStage::Done | AurStage::Failed)
    }

    /// Label on the tile button.
    pub fn label(self) -> String {
        match self {
            AurStage::Queued => trc("AUR install stage", "In queue"),
            AurStage::WaitingLock => trc("AUR install stage", "Waiting…"),
            AurStage::Downloading => trc("AUR install stage", "Downloading…"),
            AurStage::Building => trc("AUR install stage", "Building…"),
            AurStage::Installing => trc("AUR install stage", "Installing…"),
            AurStage::Done => trc("AUR install stage", "Done"),
            AurStage::Failed => trc("AUR install stage", "Error"),
        }
    }

    /// The same stage in lower case, for the "package: building…" line.
    fn status_word(self) -> String {
        match self {
            AurStage::Queued => trc("AUR install status", "in queue"),
            AurStage::WaitingLock => {
                trc("AUR install status", "waiting for another installation to finish…")
            }
            AurStage::Downloading => trc("AUR install status", "downloading…"),
            AurStage::Building => trc("AUR install status", "building…"),
            AurStage::Installing => trc("AUR install status", "installing…"),
            AurStage::Done => trc("AUR install status", "installed"),
            AurStage::Failed => trc("AUR install status", "failed"),
        }
    }
}

/// What a line of the helper's output says about the installation progress.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineStage {
    /// The new stage (None means the stage is unchanged).
    pub stage: Option<AurStage>,
    /// The package the helper took up (None means unchanged).
    pub package: Option<String>,
}

/// Stage by a line of the helper's output (yay/paru -> makepkg -> pacman)
/// and the package name, if the line names it. None means the line says
/// nothing about progress.
///
/// makepkg and pacman lines are partly translated in the Russian catalog,
/// so both forms are recognised; Rexon runs the helper under `LANG=C`.
pub fn stage_of_line(raw: &str) -> Option<LineStage> {
    let line = clean_line(raw);
    let after_colon = |l: &str| {
        l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from)
    };
    // "==> Making package: name version (date)": makepkg took up the next
    // package. The line does not change the stage: yay calls makepkg on one
    // package up to three times (download, build, package), and the
    // following lines tell which it is now.
    if line.contains("==> Making package:") || line.contains("==> Создание пакета:") {
        return Some(LineStage { stage: None, package: after_colon(&line) });
    }
    const DOWNLOADING: &[&str] = &[
        "==> Retrieving sources",
        "==> Получение исходных",
        "Downloaded PKGBUILD",
        "Downloading PKGBUILDs",
        ":: Retrieving packages",
        ":: Получение пакетов",
        "-> Downloading",
        "-> Cloning",
        "-> Updating",
    ];
    const BUILDING: &[&str] = &[
        "==> Extracting sources",
        "==> Извлечение исходных",
        "==> Starting prepare()",
        "==> Starting build()",
        "==> Starting check()",
        "==> Starting package()",
        "==> Entering fakeroot",
        "==> Запуск prepare()",
        "==> Запуск build()",
        "==> Запуск package()",
    ];
    const INSTALLING: &[&str] = &[
        "loading packages...",
        "загрузка пакетов...",
        "resolving dependencies...",
        "разрешение зависимостей...",
        "checking keyring...",
        "проверка связки ключей...",
        ":: Processing package changes",
        ":: Обработка изменений пакета",
        ":: Running pre-transaction hooks",
        ":: Запуск хуков перед транзакцией",
    ];
    let stage = |st| Some(LineStage { stage: Some(st), package: None });
    if DOWNLOADING.iter().any(|m| line.contains(m)) {
        return stage(AurStage::Downloading);
    }
    if BUILDING.iter().any(|m| line.contains(m)) {
        return stage(AurStage::Building);
    }
    if INSTALLING.iter().any(|m| line.contains(m)) {
        return stage(AurStage::Installing);
    }
    // "installing name...": pacman is installing a specific package.
    if let Some(rest) = line.strip_prefix("installing ").or_else(|| line.strip_prefix("установка ")) {
        let name = rest.trim_end_matches("...").trim().to_string();
        return Some(LineStage {
            stage: Some(AurStage::Installing),
            package: (!name.is_empty()).then_some(name),
        });
    }
    None
}

/// The status line at the top: "aerothemeplasma-desktop-git: building…".
///
/// `current` is the package the helper is working on now (for a kit, its
/// part), `queued` is how many installations wait after this one.
pub fn stage_status_line(name: &str, current: &str, stage: AurStage, queued: usize) -> String {
    let who = if current.is_empty() || current == name {
        name.to_string()
    } else {
        // TRANSLATORS: %1 is the package being built right now, %2 the
        // package (or theme kit) the person asked to install.
        trf("%1 (for %2)", &[&current, &name])
    };
    let mut line = format!("{who}: {}", stage.status_word());
    if queued > 0 {
        line.push(' ');
        line.push_str(&ntr("(%n more in queue)", "(%n more in queue)", queued as u64));
    }
    line
}

/// The tile of an AUR package that has an installation in this session's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardJobState {
    /// Button label.
    pub button_text: String,
    /// The button can be pressed ("Retry" can, "Building…" cannot).
    pub button_enabled: bool,
    /// Button look for `appearance_page::card_button_colors`: "true" is a
    /// status outline, "false" a filled action button.
    pub button_state: &'static str,
    /// Button tooltip.
    pub button_tooltip: String,
    /// The line under the buttons (failure reason); empty means no line.
    pub note: String,
    /// Tooltip for the line: the full result with advice and the log path.
    pub note_tooltip: String,
    /// Label of the "Log" link (empty means no link).
    pub log_text: String,
}

/// What to show on the tile for an installation stage. `short` is the
/// short failure reason, `message` the full result.
pub fn card_job_state(stage: AurStage, short: &str, message: &str) -> CardJobState {
    match stage {
        AurStage::Failed => CardJobState {
            button_text: trc("AUR install", "Retry"),
            button_enabled: true,
            button_state: "false",
            button_tooltip: message.to_string(),
            note: if short.is_empty() {
                stage.label()
            } else {
                format!("{}: {short}", stage.label())
            },
            note_tooltip: message.to_string(),
            log_text: trc("AUR install", "Log"),
        },
        AurStage::Done => CardJobState {
            button_text: format!("✅ {}", stage.label()),
            button_enabled: false,
            button_state: "true",
            button_tooltip: message.to_string(),
            note: String::new(),
            note_tooltip: String::new(),
            log_text: String::new(),
        },
        _ => CardJobState {
            button_text: stage.label(),
            button_enabled: false,
            button_state: "true",
            button_tooltip: if stage == AurStage::Queued {
                tr("Rexon installs AUR packages one at a time — this one starts when the previous one finishes.")
            } else {
                String::new()
            },
            note: String::new(),
            note_tooltip: String::new(),
            log_text: String::new(),
        },
    }
}

// ════════════════════════════════════════════════════════════════════════
// The queue: AUR installations strictly one at a time for the whole app
// ════════════════════════════════════════════════════════════════════════
//
// A live case on 05.10.2026: three "Install" clicks in a row started
// three helpers at once. There were three password windows, the polkit
// agent rejected two of them, and one kit's build ran before a
// neighbouring process had installed its dependency. Hence one queue,
// with one installation at a time.
//
// ⚠️ Clicks in a row are NOT merged into one helper call. That would not
// reduce the passwords to one: yay asks for privileges on every
// `pacman -U` of a build layer and once more on `pacman -D --asexplicit`
// (visible in the 05.10 log: five windows for one kit). And a failure of
// one package in a combined call would cut off all the others too, and
// the stage on the tiles could not be told apart.

/// One installation in the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AurJob {
    /// The package on which "Install" was pressed.
    pub name: String,
    /// What to install (for a kit, the whole kit).
    pub plan: InstallPlan,
    /// With which helper.
    pub helper: String,
    pub stage: AurStage,
    /// The package the helper is working on now.
    pub current: String,
    /// The result for the person (after the end).
    pub message: String,
    /// Short failure reason, for the tile label.
    pub short: String,
    /// The helper's full log.
    pub log_path: String,
}

/// The AUR installation queue. Pure state: launching and threads belong to
/// the caller; here only the decisions "what next" and "should the
/// executor be woken".
#[derive(Debug, Default, Clone)]
pub struct AurQueue {
    jobs: Vec<AurJob>,
    /// The executor is running (the thread is alive and will pick up the next one itself).
    worker_busy: bool,
}

/// Answer of [`AurQueue::enqueue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enqueued {
    /// The installation was queued (false means it is already there or running).
    pub added: bool,
    /// There is no executor: it must be started.
    pub start_worker: bool,
}

impl AurQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an installation. A package that is already waiting or being
    /// installed is not queued a second time; a finished one (including with
    /// an error, "Retry") is queued again at the end.
    pub fn enqueue(&mut self, name: &str, plan: InstallPlan, helper: &str, log_path: &str) -> Enqueued {
        if self.jobs.iter().any(|j| j.name == name && j.stage.active()) {
            return Enqueued { added: false, start_worker: false };
        }
        self.jobs.retain(|j| j.name != name);
        self.jobs.push(AurJob {
            name: name.to_string(),
            plan,
            helper: helper.to_string(),
            stage: AurStage::Queued,
            current: String::new(),
            message: String::new(),
            short: String::new(),
            log_path: log_path.to_string(),
        });
        let start_worker = !self.worker_busy;
        self.worker_busy = true;
        Enqueued { added: true, start_worker }
    }

    /// The executor takes the next one. None means the queue is empty, and
    /// the executor must exit (the flag is cleared right here, under the same
    /// lock; otherwise an installation queued between "empty" and the exit
    /// would wait forever).
    pub fn take_next(&mut self) -> Option<AurJob> {
        match self.jobs.iter_mut().find(|j| j.stage == AurStage::Queued) {
            Some(job) => {
                job.stage = AurStage::WaitingLock;
                Some(job.clone())
            }
            None => {
                self.worker_busy = false;
                None
            }
        }
    }

    /// A new stage of the running installation. true means something changed.
    pub fn set_stage(&mut self, name: &str, stage: Option<AurStage>, current: Option<&str>) -> bool {
        let Some(job) = self.jobs.iter_mut().find(|j| j.name == name && j.stage.active()) else {
            return false;
        };
        let mut changed = false;
        if let Some(stage) = stage.filter(|st| st.active() && *st != job.stage) {
            job.stage = stage;
            changed = true;
        }
        if let Some(cur) = current {
            if job.current != cur {
                job.current = cur.to_string();
                changed = true;
            }
        }
        changed
    }

    /// The installation has ended.
    pub fn finish(&mut self, name: &str, ok: bool, message: &str, short: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.name == name && j.stage.active()) {
            job.stage = if ok { AurStage::Done } else { AurStage::Failed };
            job.current.clear();
            job.message = message.to_string();
            job.short = short.to_string();
        }
    }

    /// All installations: running, waiting and finished during the session.
    pub fn jobs(&self) -> &[AurJob] {
        &self.jobs
    }

    /// The status line at the top: the running installation with the package
    /// name; empty means nothing is running.
    pub fn status_line(&self) -> String {
        let Some(job) = self.jobs.iter().find(|j| j.stage.active() && j.stage != AurStage::Queued) else {
            return String::new();
        };
        let queued = self.jobs.iter().filter(|j| j.stage == AurStage::Queued).count();
        stage_status_line(&job.name, &job.current, job.stage, queued)
    }

    /// State for the page: `{"jobs": [...], "status": "..."}`.
    ///
    /// For a running kit installation, `packages` holds the whole kit: the
    /// tile of any of its parts shows the same stage.
    pub fn snapshot_json(&self) -> String {
        let jobs: Vec<serde_json::Value> = self
            .jobs
            .iter()
            .map(|j| {
                serde_json::json!({
                    "name": j.name,
                    "packages": j.plan.packages,
                    "stage": j.stage.as_str(),
                    "label": j.stage.label(),
                    "current": j.current,
                    "message": j.message,
                    "short": j.short,
                    "logPath": j.log_path,
                })
            })
            .collect();
        serde_json::json!({ "jobs": jobs, "status": self.status_line() }).to_string()
    }
}

/// Where to save the helper's full output: `aur/<package>.log` in the
/// "Appearance" cache (`~/.cache/rexon`, [`crate::appearance_cache::cache_dir`]).
pub fn log_path(home: &str, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.+@".contains(c) { c } else { '_' })
        .collect();
    format!("{}/aur/{safe}.log", crate::appearance_cache::cache_dir(home.trim_end_matches('/')))
}

// ════════════════════════════════════════════════════════════════════════
// Build progress in the window: what of the helper's output to show
// ════════════════════════════════════════════════════════════════════════

/// Strip terminal colour: `ESC[1m`, `ESC[0;32m` and the same code without
/// ESC (`[1m … [0m`: that is how it arrives when the ESC was eaten on the
/// way).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let after_esc = rest.strip_prefix('\u{1b}');
        let body = after_esc.unwrap_or(rest);
        if let Some(seq) = body.strip_prefix('[') {
            let n = seq.chars().take_while(|c| c.is_ascii_digit() || *c == ';').count();
            if seq[n..].starts_with('m') && (n > 0 || after_esc.is_some()) {
                rest = &seq[n + 1..];
                continue;
            }
        }
        if let Some(b) = after_esc {
            // Other ESC sequences (`ESC[K`, `ESC(B`) run up to a letter.
            let skip = b.char_indices().find(|(_, c)| c.is_ascii_alphabetic()).map_or(b.len(), |(i, _)| i + 1);
            rest = &b[skip..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A helper question Rexon answered in advance (`--answerclean`,
/// `--answerdiff`, `--noconfirm`): "Packages to cleanBuild?", the menu
/// "[N]one [A]ll … (1 2 3, 1-3, ^4)", the answer echo "None", "[Y/n]".
/// There is nothing for the person to read here: they give no answer.
fn is_answered_question(body: &str) -> bool {
    let b = body.trim();
    b.ends_with('?')
        || b.contains("^4")
        || ["[Y/n]", "[y/N]", "[Д/н]", "[д/Н]", "[J/n]", "[j/N]"].iter().any(|q| b.contains(q))
        || matches!(b, "None" | "none" | "All" | "N" | "A")
}

/// A line of the AUR helper's output -> a line of the progress window (or
/// `None` to hide it).
///
/// Terminal colour is stripped; questions answered in advance are hidden;
/// pacman lines the builder passes on from installing dependencies and the
/// built packages go through the same parser as installation from the
/// repositories ([`crate::system_plan::parse_pacman_line`]): in Russian or
/// not at all. Of the rest, only `makepkg` stages (`==>`), helper messages
/// (`::`) and transitions (` -> `) are shown; everything else is compiler
/// noise.
pub fn progress_line(raw: &str) -> Option<String> {
    let clean = strip_ansi(raw);
    let line = clean.trim();
    if line.is_empty() {
        return None;
    }
    let body = line.trim_start_matches("==>").trim_start_matches("::").trim();
    if body.is_empty() || is_answered_question(body) {
        return None;
    }
    // `-> librda-0.0.5-1 is up to date -- skipping` (yay about an installed package).
    if let Some(stem) = body.trim_start_matches("->").trim().strip_suffix(" is up to date -- skipping") {
        let mut parts = stem.rsplitn(3, '-');
        let name = match (parts.next(), parts.next(), parts.next()) {
            (Some(_), Some(_), Some(n)) if !n.is_empty() => n,
            _ => stem,
        };
        return Some(format!("    {}", trf("✅ already installed: %1", &[&name])));
    }
    if let Some(p) = crate::system_plan::parse_pacman_line(line) {
        // An empty parse message means "nothing to show" (a pacman question, a
        // service header); a "📌 …" section is someone else's header, shown as
        // is below.
        if p.stage == "nothing" {
            return None;
        }
        if p.stage != "section" {
            return (!p.message.is_empty()).then(|| format!("    {}", p.message));
        }
    }
    if !crate::system_plan::AUR_PROGRESS_MARKERS.iter().any(|m| line.contains(m)) {
        return None;
    }
    Some(format!("    {line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn yay_argv_asks_password_graphically_and_answers_everything() {
        let argv = install_argv("yay", &s(&["foo"]), false);
        assert_eq!(
            argv,
            s(&[
                "yay", "-S", "--needed", "--noconfirm", "--answerclean", "None", "--answerdiff",
                "None", "--answeredit", "None", "--removemake", "--sudo", "pkexec", "--sudoflags",
                "", "foo"
            ])
        );
        assert!(!argv.iter().any(|a| a.starts_with("--ask")));
    }

    #[test]
    fn paru_argv_skips_review_and_sudoloop() {
        let argv = install_argv("paru", &s(&["a", "b"]), true);
        assert_eq!(
            argv,
            s(&[
                "paru", "-S", "--needed", "--noconfirm", "--skipreview", "--nosudoloop",
                "--removemake", "--sudo", "pkexec", "--sudoflags", "", "--ask=4", "a", "b"
            ])
        );
    }

    #[test]
    fn argv_packages_skips_flag_values() {
        let argv = install_argv("yay", &s(&["a", "b"]), true);
        assert_eq!(argv_packages(&argv), s(&["a", "b"]));
        let argv = install_argv("paru", &s(&["c"]), false);
        assert_eq!(argv_packages(&argv), s(&["c"]));
    }

    #[test]
    fn unknown_helper_keeps_plain_argv() {
        assert_eq!(
            install_argv("pikaur", &s(&["x"]), true),
            s(&["pikaur", "-S", "--needed", "--noconfirm", "x"])
        );
    }

    #[test]
    fn plain_package_has_no_question_and_no_replacement() {
        let plan = plan_install("bibata-cursor-theme", &|_| false);
        assert_eq!(plan.packages, s(&["bibata-cursor-theme"]));
        assert!(plan.question.is_none() && !plan.replace_conflicts && plan.kit.is_none());
    }

    #[test]
    fn aero_kit_in_authors_order_with_warning() {
        // Clean Plasma: libplasma is the system one, no fork.
        let plan = plan_install("aerothemeplasma-desktop-git", &|p| p == "libplasma");
        assert_eq!(plan.packages, AEROTHEMEPLASMA.packages.iter().map(|p| p.to_string()).collect::<Vec<_>>());
        assert_eq!(plan.packages.first().unwrap(), "aeroshell-libplasma-git");
        assert_eq!(plan.packages.last().unwrap(), "aerothemeplasma-desktop-git");
        assert!(plan.replace_conflicts);
        let q = plan.question.unwrap();
        assert!(q.text.contains("libplasma") && q.text.contains("aeroshell-libplasma-git"), "{}", q.text);
        assert!(q.text.contains("AeroThemePlasma"));
    }

    #[test]
    fn aero_kit_on_machine_with_fork_already_installed() {
        // As on the owner's machine on 03.10: the libplasma fork is already installed, so no replacement.
        let plan = plan_install("aerothemeplasma-desktop-git", &|p| p == "aeroshell-libplasma-git");
        assert!(!plan.packages.contains(&"aeroshell-libplasma-git".to_string()));
        assert_eq!(plan.packages.len(), 6);
        assert!(!plan.replace_conflicts);
        assert!(!plan.question.unwrap().text.contains("will be replaced"));
    }

    #[test]
    fn x11_variant_is_a_kit_too() {
        let plan = plan_install("aerothemeplasma-desktop-x11-git", &|p| p == "libplasma");
        assert_eq!(plan.packages.last().unwrap(), "aerothemeplasma-desktop-x11-git");
        assert!(plan.packages.contains(&"aerothemeplasma-desktop-git".to_string()));
        assert!(plan.replace_conflicts && plan.question.is_some());
    }

    #[test]
    fn icons_alone_are_not_a_kit() {
        assert!(kit_for("aerothemeplasma-icons-git").is_none());
        assert!(kit_for("aerothemeplasma-sounds-git").is_none());
    }

    #[test]
    fn cause_sudo_without_terminal() {
        let log = "==> Finished making: aeroshell-smod-git\n\
                   sudo: a terminal is required to read the password; either use the -S option to read from standard input or configure an askpass helper\n\
                   sudo: a password is required\n -> error installing: [/usr/bin/sudo pacman -U ...] - exit status 1\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NoTerminalForPassword));
    }

    #[test]
    fn cause_sudo_without_terminal_russian() {
        let log = "sudo: для ввода пароля требуется терминал; добавьте параметр -S\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NoTerminalForPassword));
    }

    /// The tail of a real dry run on 03.10.2026 (`yay -S --sudo /bin/false
    /// aerothemeplasma-desktop-git`): five dependencies built, the layer
    /// installation refused: exactly what the owner had.
    #[test]
    fn cause_dry_run_install_layer_refused() {
        let log = "==> Sources are ready.\n -> error installing: [/b/aeroshell-smod-git/aeroshell-smod-git-6.7.4_66.rf58780d-1-x86_64.pkg.tar.zst /b/aerothemeplasma-icons-git/aerothemeplasma-icons-git-13.r0100536-1-any.pkg.tar.zst] - exit status 1\n\
                   error installing: [/b/x.pkg.tar.zst] - exit status 1\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PrivilegeFailed));
    }

    #[test]
    fn cause_pkexec_dismissed() {
        let log = "Error executing command as another user: Request dismissed\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PasswordDismissed));
    }

    #[test]
    fn cause_conflict_names_both_packages() {
        let log = "\x1b[1;33m:: \x1b[0maeroshell-libplasma-git-6.7.4_18049.rda45ff7-1 and libplasma-6.7.4-1 are in conflict\n\
                   error: failed to prepare transaction (conflicting dependencies)\n";
        assert_eq!(
            failure_cause(log),
            Some(AurFailure::Conflict { new: "aeroshell-libplasma-git".into(), old: "libplasma".into() })
        );
        let msg = failure_message("aerothemeplasma-desktop-git", log, "/h/.cache/rexon/aur/x.log");
        assert!(msg.contains("aeroshell-libplasma-git conflicts with the installed libplasma"), "{msg}");
        // No "run this in a terminal" advice (27.11.2026): the window offers buttons.
        assert!(!msg.contains("terminal") && !msg.contains("yay -S"), "{msg}");
        assert!(msg.contains("/h/.cache/rexon/aur/x.log"), "{msg}");
    }

    #[test]
    fn cause_download() {
        let log = "==> ERROR: Failure while downloading foo-git git repo\nAborting...\n";
        assert!(matches!(failure_cause(log), Some(AurFailure::Download { .. })));
        let log = "fatal: unable to access 'https://gitgud.io/x.git/': Could not resolve host: gitgud.io\n";
        assert_eq!(failure_cause(log), Some(AurFailure::Download { what: "https://gitgud.io/x.git/".into() }));
    }

    #[test]
    fn cause_build_failure_names_package() {
        let log = "==> Making package: aeroshell-smod-git 6.7.4-1 (Sat)\nmake: *** [all] Error 2\n\
                   ==> ERROR: A failure occurred in build().\n    Aborting...\n -> error making: aeroshell-smod-git\n";
        assert_eq!(failure_cause(log), Some(AurFailure::Build { package: "aeroshell-smod-git".into() }));
    }

    #[test]
    fn cause_missing_dependency() {
        let log = "error: target not found: qt5-foo\n";
        assert_eq!(failure_cause(log), Some(AurFailure::MissingDependency { name: "qt5-foo".into() }));
    }

    #[test]
    fn unknown_failure_quotes_last_line() {
        let msg = failure_message("x", "something odd\nweird end\n", "");
        assert!(msg.contains("weird end"), "{msg}");
        assert!(!msg.contains("terminal") && !msg.contains("yay -S"), "{msg}");
    }

    #[test]
    fn clean_line_strips_colors_and_progress() {
        assert_eq!(clean_line("\x1b[1m(1/6) Downloaded\x1b[0m"), "(1/6) Downloaded");
        assert_eq!(clean_line("Updating 1%\rUpdating 100%, done."), "Updating 100%, done.");
    }

    /// The tail of the owner's log on 05.10.2026 (aerothemeplasma-icons-git):
    /// the KDE agent was already showing a window to another installation,
    /// pkexec answered "Not authorized" with code 127. Nobody had entered a
    /// password, so "password not accepted" was untrue.
    const OWNER_ICONS_TAIL: &str = "==> Sources are ready.\n -> /var/lib/pacman/db.lck is present.\n\
         -> There may be another Pacman instance running. Waiting...\n\
        Error executing command as another user: Not authorized\n\nThis incident has been reported.\n\
         -> error installing: [/home/user/.cache/yay/aerothemeplasma-icons-git/aerothemeplasma-icons-git-13.r0100536-1-any.pkg.tar.zst] - exit status 127\n";

    /// The same refusal in the system journal for the duration of the installation (journalctl -o cat).
    const OWNER_AGENT_JOURNAL: &str = "Initiating authentication\n\
        Another client is already authenticating, please try again later.\n\
        Operator of unix-session:2 FAILED to authenticate to gain authorization for action org.freedesktop.policykit.exec\n";

    #[test]
    fn owner_case_not_authorized_is_not_wrong_password() {
        let cause = failure_cause(OWNER_ICONS_TAIL);
        assert_eq!(cause, Some(AurFailure::NotAuthorized));
        assert!(!failure_text(&AurFailure::NotAuthorized).contains("not accepted"));
        // From the agent's journal, the exact reason: the window was busy with another one.
        let refined = refine_with_agent_journal(cause, OWNER_AGENT_JOURNAL);
        assert_eq!(refined, Some(AurFailure::PasswordWindowBusy));
        let msg = failure_message_for("aerothemeplasma-icons-git", OWNER_ICONS_TAIL, "/l", refined.as_ref());
        assert!(msg.contains("another password window was already open"), "{msg}");
        assert!(msg.contains("“Retry”"), "{msg}");
    }

    #[test]
    fn journal_refines_wrong_password_only_for_unclear_refusals() {
        let journal = "pam_unix(polkit-1:auth): authentication failure; logname= uid=1000\n";
        assert_eq!(refine_with_agent_journal(Some(AurFailure::NotAuthorized), journal), Some(AurFailure::WrongPassword));
        // A clear reason is not overridden by the journal.
        assert_eq!(
            refine_with_agent_journal(Some(AurFailure::PasswordDismissed), OWNER_AGENT_JOURNAL),
            Some(AurFailure::PasswordDismissed)
        );
        // An empty journal: the reason stays the same.
        assert_eq!(refine_with_agent_journal(Some(AurFailure::NotAuthorized), ""), Some(AurFailure::NotAuthorized));
    }

    #[test]
    fn pkexec_exit_codes_without_words() {
        let log = " -> error installing: [/b/x.pkg.tar.zst] - exit status 126\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PasswordDismissed));
        let log = " -> error installing: [/b/x.pkg.tar.zst] - exit status 127\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NotAuthorized));
        // A different code: privileges were refused silently, as before.
        let log = " -> error installing: [/b/x.pkg.tar.zst] - exit status 1\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PrivilegeFailed));
    }

    #[test]
    fn wrong_password_from_text_agent() {
        let log = "polkit-agent-helper-1: pam_authenticate failed: Authentication failure\n==== AUTHENTICATION FAILED ====\nError executing command as another user: Not authorized\n";
        assert_eq!(failure_cause(log), Some(AurFailure::WrongPassword));
    }

    #[test]
    fn no_polkit_agent() {
        let log = "Error executing command as another user: No authentication agent found.\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NoPasswordAgent));
        assert!(failure_advice(&AurFailure::NoPasswordAgent).is_some());
    }

    /// The race of 05.10: part of the kit was built before a neighbouring
    /// process installed the libplasma fork.
    #[test]
    fn missing_dependencies_race() {
        let log = "==> Making package: aeroshell-workspace-git 6.7.5_22.rad919f4-1 (Mon Oct  5 02:10:01 2026)\n\
                   ==> Checking runtime dependencies...\n==> Missing dependencies:\n  -> aeroshell-libplasma-git\n\
                   ==> Checking buildtime dependencies...\n==> ERROR: Could not resolve all dependencies.\n\
                    -> error making: aeroshell-workspace-git-exit status 8\n";
        let cause = failure_cause(log);
        assert_eq!(
            cause,
            Some(AurFailure::DependencyNotInstalled {
                package: "aeroshell-workspace-git".into(),
                names: vec!["aeroshell-libplasma-git".into()]
            })
        );
        let text = failure_text(cause.as_ref().unwrap());
        assert!(text.contains("aeroshell-workspace-git") && text.contains("aeroshell-libplasma-git"), "{text}");
        assert!(failure_advice(cause.as_ref().unwrap()).unwrap().contains("one package at a time"));
    }

    // ── Known breakages of AUR recipes ──────────────────────────────────

    /// The AUR recipe of mate-menu 26.10.1-2 as it was on 27.11.2026.
    const MATE_MENU_PKGBUILD: &str = "pkgname=mate-menu\npkgver=26.10.1\npkgrel=2\n\
        install=$pkgname.install\n\npackage() {\n\tcd $pkgname-$pkgver\n\
        \tpython setup.py install --root=\"$pkgdir\" --optimize=1\n}\n";

    fn mate_menu_fix() -> &'static RecipeFix {
        recipe_fix_for("mate-menu").expect("mate-menu is in the table")
    }

    const SCHEMA_RM: &str = "\trm -f \"$pkgdir/usr/share/glib-2.0/schemas/gschemas.compiled\"\n";

    #[test]
    fn mate_menu_recipe_gets_the_schema_cache_removed() {
        let RecipePatch::Patched(text) = patch_recipe(mate_menu_fix(), MATE_MENU_PKGBUILD) else {
            panic!("the 26.10.1-2 recipe must be patched");
        };
        assert!(text.contains(&format!("--optimize=1\n{SCHEMA_RM}}}\n")), "{text}");
        // Everything else stays as it was.
        assert_eq!(text.replace(SCHEMA_RM, ""), MATE_MENU_PKGBUILD);
    }

    #[test]
    fn recipe_fix_is_skipped_once_the_maintainer_fixed_it() {
        let fixed = MATE_MENU_PKGBUILD.replace(
            "--optimize=1\n",
            "--optimize=1\n\trm \"$pkgdir\"/usr/share/glib-2.0/schemas/gschemas.compiled\n",
        );
        assert_eq!(patch_recipe(mate_menu_fix(), &fixed), RecipePatch::AlreadyFixed);
    }

    #[test]
    fn recipe_fix_is_skipped_for_a_rewritten_recipe() {
        // The maintainer moved to the PEP 517 installer: the line is gone.
        let rewritten = MATE_MENU_PKGBUILD.replace(
            "python setup.py install --root=\"$pkgdir\" --optimize=1",
            "python -m installer --destdir=\"$pkgdir\" dist/*.whl",
        );
        assert!(matches!(patch_recipe(mate_menu_fix(), &rewritten), RecipePatch::Unexpected(_)));
        // The line in a different function is not the broken one.
        let moved = MATE_MENU_PKGBUILD.replace("package() {", "build() {");
        assert!(matches!(patch_recipe(mate_menu_fix(), &moved), RecipePatch::Unexpected(_)));
        // Twice — not the recipe the fix was written for.
        let twice = MATE_MENU_PKGBUILD.replace(
            "--optimize=1\n",
            "--optimize=1\n\tpython setup.py install --root=\"$pkgdir\" --optimize=1\n",
        );
        assert!(matches!(patch_recipe(mate_menu_fix(), &twice), RecipePatch::Unexpected(_)));
    }

    #[test]
    fn recipe_fix_accepts_a_brace_on_the_next_line() {
        let recipe = MATE_MENU_PKGBUILD.replace("package() {\n", "package()\n{\n");
        assert!(matches!(patch_recipe(mate_menu_fix(), &recipe), RecipePatch::Patched(_)));
    }

    #[test]
    fn built_package_is_checked_for_the_removed_file() {
        let listing = ".PKGINFO\n.MTREE\nusr/bin/mate-menu\nusr/share/glib-2.0/schemas/\n\
                       usr/share/glib-2.0/schemas/org.mate.mate-menu.gschema.xml\n";
        assert!(forbidden_in_package(mate_menu_fix(), listing).is_empty());
        let broken = format!("{listing}./usr/share/glib-2.0/schemas/gschemas.compiled\n");
        assert_eq!(forbidden_in_package(mate_menu_fix(), &broken), vec!["usr/share/glib-2.0/schemas/gschemas.compiled"]);
    }

    #[test]
    fn helper_command_is_split_around_the_fixed_package() {
        let argv = install_argv("yay", &s(&["mate-menu", "mate-indicator-applet"]), false);
        let split = split_recipe_fixes(&argv, &|_| false).expect("mate-menu has a fix");
        assert_eq!(split.fixes.iter().map(|f| f.package).collect::<Vec<_>>(), vec!["mate-menu"]);
        assert!(!split.replace);
        let rest = argv_without(&argv, &["mate-menu"]).expect("one package is left");
        assert_eq!(argv_packages(&rest), s(&["mate-indicator-applet"]));
        // The flags and their values are kept as they were.
        assert_eq!(rest.len(), argv.len() - 1);
        assert_eq!(argv_without(&install_argv("yay", &s(&["mate-menu"]), false), &["mate-menu"]), None);
        // Installed already — the helper's `--needed` deals with it.
        assert_eq!(split_recipe_fixes(&argv, &|p| p == "mate-menu"), None);
        // Not an AUR helper, or no package with a fix.
        assert_eq!(split_recipe_fixes(&s(&["pacman", "-S", "mate-menu"]), &|_| false), None);
        assert_eq!(split_recipe_fixes(&install_argv("paru", &s(&["mate-tweak"]), false), &|_| false), None);
        let paru = install_argv("paru", &s(&["mate-menu"]), true);
        assert!(split_recipe_fixes(&paru, &|_| false).is_some_and(|f| f.replace));
    }

    /// The switches of the AUR recipe vala-panel-appmenu 25.04 (27.11.2026).
    const APPMENU_SWITCHES: &str = "## options\n: ${_build_mate:=true}\n: ${_build_xfce:=true}\n\
        : ${_build_vala:=true}\n: ${_build_budgie:=true}\n\n: ${_build_registrar:=true}\n\
        : ${_build_translator:=true}\n\n_pkgname=\"vala-panel-appmenu\"\npkgver=25.04\n";

    #[test]
    fn appmenu_recipe_builds_only_the_mate_module() {
        let fix = recipe_fix_for("vala-panel-appmenu-mate").unwrap();
        assert_eq!(fix.pkgbase, "vala-panel-appmenu");
        let RecipePatch::Patched(text) = patch_recipe(fix, APPMENU_SWITCHES) else {
            panic!("the 25.04 recipe must be patched");
        };
        for kept in [": ${_build_mate:=true}", ": ${_build_registrar:=true}", ": ${_build_translator:=true}"] {
            assert!(text.contains(kept), "{text}");
        }
        for off in [": ${_build_xfce:=false}", ": ${_build_vala:=false}", ": ${_build_budgie:=false}"] {
            assert!(text.contains(off), "{text}");
        }
        // Patched once — nothing more to do; the maintainer turning a module
        // off by himself is fine too.
        assert_eq!(patch_recipe(fix, &text), RecipePatch::AlreadyFixed);
        // A recipe without the switches is not the one the fix was written for.
        let other = APPMENU_SWITCHES.replace(": ${_build_vala:=true}\n", "");
        assert!(matches!(patch_recipe(fix, &other), RecipePatch::Unexpected(_)));
        // The XFCE and registrar packages keep their own module only.
        let RecipePatch::Patched(xfce) = patch_recipe(recipe_fix_for("vala-panel-appmenu-xfce").unwrap(), APPMENU_SWITCHES) else {
            panic!("xfce");
        };
        assert!(xfce.contains(": ${_build_xfce:=true}") && xfce.contains(": ${_build_mate:=false}"), "{xfce}");
        let RecipePatch::Patched(reg) = patch_recipe(recipe_fix_for("vala-panel-appmenu-registrar").unwrap(), APPMENU_SWITCHES) else {
            panic!("registrar");
        };
        assert!(reg.contains(": ${_build_registrar:=true}") && !reg.contains("_build_mate:=true"), "{reg}");
    }

    #[test]
    fn split_recipe_installs_its_own_dependencies_together() {
        // `.SRCINFO` of the fixed appmenu recipe (abridged).
        let srcinfo = "pkgbase = vala-panel-appmenu\n\tmakedepends = meson\n\tmakedepends = vala\n\n\
                       pkgname = vala-panel-appmenu\n\tdepends = vala-panel-appmenu-locale\n\n\
                       pkgname = appmenu-glib-translator\n\tdepends = dconf\n\tdepends = gdk-pixbuf2\n\n\
                       pkgname = vala-panel-appmenu-mate\n\tdepends = appmenu-glib-translator\n\tdepends = mate-panel\n";
        let pkgs = crate::privilege::aur::parse_srcinfo_pkgs(srcinfo);
        let fix = recipe_fix_for("vala-panel-appmenu-mate").unwrap();
        let (targets, needs) = fixed_build_targets(fix, &pkgs).unwrap();
        assert_eq!(targets, s(&["vala-panel-appmenu-mate", "appmenu-glib-translator"]));
        for n in ["mate-panel", "dconf", "gdk-pixbuf2", "meson", "vala"] {
            assert!(needs.contains(&n.to_string()), "{needs:?}");
        }
        assert!(!needs.iter().any(|n| n == "appmenu-glib-translator"), "{needs:?}");
        // The recipe lost the package — nothing for Rexon to build.
        assert_eq!(fixed_build_targets(recipe_fix_for("mate-menu").unwrap(), &pkgs), None);
    }

    #[test]
    fn recipe_fix_switches_off_from_the_fixed_upstream_release() {
        let fix = RecipeFix { fixed_upstream_in: Some("26.10.2"), ..*mate_menu_fix() };
        assert!(matches!(patch_recipe(&fix, MATE_MENU_PKGBUILD), RecipePatch::Patched(_)));
        let newer = MATE_MENU_PKGBUILD.replace("pkgver=26.10.1", "pkgver=26.10.2");
        assert_eq!(patch_recipe(&fix, &newer), RecipePatch::AlreadyFixed);
        let much_newer = MATE_MENU_PKGBUILD.replace("pkgver=26.10.1", "pkgver=27.04");
        assert_eq!(patch_recipe(&fix, &much_newer), RecipePatch::AlreadyFixed);
        assert!(version_at_least("26.10.10", "26.10.2") && !version_at_least("26.9", "26.10"));
    }

    /// A live check on demand: `REXON_TEST_PATCH_PKGBUILD=<path>` patches that
    /// PKGBUILD in place with the fix of `REXON_TEST_PATCH_PACKAGE` (mate-menu
    /// by default) — used to build the real package by hand with makepkg.
    #[test]
    #[ignore]
    fn live_patch_pkgbuild_file() {
        let path = std::env::var("REXON_TEST_PATCH_PKGBUILD").expect("set REXON_TEST_PATCH_PKGBUILD");
        let text = std::fs::read_to_string(&path).unwrap();
        let package = std::env::var("REXON_TEST_PATCH_PACKAGE").unwrap_or_else(|_| "mate-menu".into());
        let RecipePatch::Patched(fixed) = patch_recipe(recipe_fix_for(&package).unwrap(), &text) else {
            panic!("the live recipe was not patched");
        };
        std::fs::write(&path, fixed).unwrap();
    }

    #[test]
    fn conflict_and_build_keep_their_causes() {
        let log = ":: a-1-1 and b-2-1 are in conflict\n";
        assert!(matches!(failure_cause(log), Some(AurFailure::Conflict { .. })));
        // Every reason has advice without a terminal (27.11.2026).
        let advice = failure_advice(&AurFailure::Conflict { new: "a".into(), old: "b".into() }).unwrap();
        assert!(!advice.contains("terminal"), "{advice}");
        let short = failure_short(Some(&AurFailure::PasswordDismissed));
        assert_eq!(short, "the password window was closed");
        assert_eq!(failure_short(None), "The installation failed — see the log");
    }

    #[test]
    fn stages_from_real_output() {
        let st = |l: &str| stage_of_line(l).and_then(|x| x.stage);
        let pkg = |l: &str| stage_of_line(l).and_then(|x| x.package);
        assert_eq!(pkg("==> Making package: aerothemeplasma-icons-git 8.rb8d5ce1-1 (Mon Oct  5 02:08:52 2026)").as_deref(), Some("aerothemeplasma-icons-git"));
        assert_eq!(st("==> Making package: x 1-1"), None);
        assert_eq!(st("==> Retrieving sources..."), Some(AurStage::Downloading));
        assert_eq!(st("  -> Updating aerothemeplasma-icons-git git repo..."), Some(AurStage::Downloading));
        assert_eq!(st("\x1b[1m:: (1/3) Downloaded PKGBUILD: x\x1b[0m"), Some(AurStage::Downloading));
        assert_eq!(st("==> Extracting sources..."), Some(AurStage::Building));
        assert_eq!(st("==> Starting build()..."), Some(AurStage::Building));
        assert_eq!(st("loading packages..."), Some(AurStage::Installing));
        assert_eq!(st(":: Running pre-transaction hooks..."), Some(AurStage::Installing));
        assert_eq!(st(":: Processing package changes..."), Some(AurStage::Installing));
        assert_eq!(pkg("installing aerothemeplasma-sounds-git...").as_deref(), Some("aerothemeplasma-sounds-git"));
        assert_eq!(stage_of_line("Updating files:  43% (3522/8167)"), None);
        assert_eq!(stage_of_line("cc1plus: warning: x"), None);
    }

    #[test]
    fn status_line_names_package() {
        let line = stage_status_line("aerothemeplasma-desktop-git", "", AurStage::Building, 0);
        assert_eq!(line, "aerothemeplasma-desktop-git: building…");
        let line = stage_status_line("aerothemeplasma-desktop-git", "aeroshell-workspace-git", AurStage::Installing, 2);
        assert_eq!(line, "aeroshell-workspace-git (for aerothemeplasma-desktop-git): installing… (2 more in queue)");
    }

    fn plain(name: &str) -> InstallPlan {
        plan_install(name, &|_| false)
    }

    /// Three "Install" clicks in a row: one executor, the others wait.
    #[test]
    fn queue_runs_one_at_a_time() {
        let mut q = AurQueue::new();
        let a = q.enqueue("a", plain("a"), "yay", "/la");
        assert_eq!(a, Enqueued { added: true, start_worker: true });
        let b = q.enqueue("b", plain("b"), "yay", "/lb");
        assert_eq!(b, Enqueued { added: true, start_worker: false });
        // Pressing again on a waiting package adds nothing.
        assert!(!q.enqueue("b", plain("b"), "yay", "/lb").added);
        let first = q.take_next().unwrap();
        assert_eq!(first.name, "a");
        assert_eq!(q.jobs()[1].stage, AurStage::Queued);
        assert!(q.set_stage("a", Some(AurStage::Building), Some("a")));
        assert!(!q.set_stage("a", Some(AurStage::Building), Some("a")));
        assert_eq!(q.status_line(), "a: building… (1 more in queue)");
        q.finish("a", false, "msg", "short");
        assert_eq!(q.jobs()[0].stage, AurStage::Failed);
        // A click while the executor is working does not start it a second time.
        let c = q.enqueue("c", plain("c"), "yay", "/lc");
        assert!(c.added && !c.start_worker);
        assert_eq!(q.take_next().unwrap().name, "b");
        q.finish("b", true, "ok", "");
        assert_eq!(q.take_next().unwrap().name, "c");
        q.finish("c", true, "ok", "");
        assert!(q.take_next().is_none());
        // The executor has exited: the next click must start it.
        let again = q.enqueue("a", plain("a"), "yay", "/la");
        assert_eq!(again, Enqueued { added: true, start_worker: true });
        assert_eq!(q.jobs().iter().filter(|j| j.name == "a").count(), 1);
        let snap: serde_json::Value = serde_json::from_str(&q.snapshot_json()).unwrap();
        assert_eq!(snap["jobs"].as_array().unwrap().len(), 3);
        assert_eq!(snap["jobs"][2]["stage"], "queued");
    }

    #[test]
    fn log_path_is_in_cache() {
        assert_eq!(log_path("/home/a/", "aero-git"), "/home/a/.cache/rexon/aur/aero-git.log");
        assert_eq!(log_path("/home/a", "../x"), "/home/a/.cache/rexon/aur/.._x.log");
    }

    #[test]
    fn build_progress_without_colour_and_prompts() {
        // Live 08.11.2026, MATE "Check / Restore" (yay 13.1.0, LANG=ru).
        assert_eq!(strip_ansi("\u{1b}[1;34m::\u{1b}[0m\u{1b}[1m Проверка\u{1b}[0m"), ":: Проверка");
        assert_eq!(strip_ansi("[1m==> [0mСборка"), "==> Сборка");
        assert_eq!(strip_ansi("массив[1] и [x]"), "массив[1] и [x]");
        for hidden in [
            "\u{1b}[1;32m==>\u{1b}[0m Пакеты для чистой сборки?",
            "==> Показать изменения?",
            "==> [Н]ет [В]се [О]тменить [У]становленные [Н]еустановленные или (1 2 3, 1-3, ^4)",
            "==> None",
            ":: Proceed with installation? [Y/n]",
            "there is nothing to do",
            ":: Running pre-transaction hooks...",
            "",
        ] {
            assert_eq!(progress_line(hidden), None, "{hidden}");
        }
        assert_eq!(progress_line("==> Сборка пакета librda"), Some("    ==> Сборка пакета librda".into()));
        assert_eq!(progress_line(":: Retrieving packages..."), Some(format!("    {}", tr("⬇️  Downloading packages…"))));
        assert_eq!(progress_line("gcc -O2 -c x.c"), None);
        assert_eq!(
            progress_line(" -> librda-0.0.5-1 is up to date -- skipping"),
            Some(format!("    {}", trf("✅ already installed: %1", &[&"librda"])))
        );
    }

    #[test]
    fn vala_panel_appmenu_build_options() {
        let argv = install_argv("yay", &s(&["mate-tweak", "vala-panel-appmenu-mate", "vala-panel-appmenu-registrar"]), false);
        assert_eq!(build_env(&argv), vec![("_build_budgie".to_string(), "false".to_string())]);
        assert!(build_env(&install_argv("yay", &s(&["mate-tweak"]), false)).is_empty());
        assert!(build_env(&s(&["pacman", "-S", "vala-panel-appmenu-mate"])).is_empty());
    }
}
