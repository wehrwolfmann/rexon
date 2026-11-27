//! Port of the PURE (non-GUI) logic of `rexon/installer.py` — system/user
//! install and uninstall of Rexon.
//!
//! Like `system_plan.rs`, the module executes NOTHING: it builds TEXTS (bash
//! install/uninstall scripts, privileged helper bodies, polkit XML, .desktop,
//! launchers) and command PLANS (`Cmd`/`PlanStep` from `system_plan`) — the
//! same argv/timeout/scripts live Python passes to `subprocess.run`. Real
//! execution (pkexec, copytree, FS checks, tempfile.mkstemp) is the
//! executor's job (for now live Python via the bridge, later rexon-gui). Temp
//! file paths are passed as parameters, so the texts are deterministic and
//! comparable byte for byte (see `installer_parity.rs`).
//!
//! Ported (pure logic):
//!   * the constants SYSTEM_INSTALL_ACTION / SYSTEM_HELPER_PATH / SYSTEM_POLICY_PATH
//!     + class-level INSTALL_DIR / LIB_DIR / DESKTOP_DIR;
//!   * the texts `system_helper_script` / `system_policy_xml` (text for text),
//!     `desktop_content` / `recover_desktop_content`, the launchers of the
//!     system and home folder installs;
//!   * `detect_default_mode` (Steam env → portable), `install_paths` (paths by
//!     system/user/portable mode, XDG_DATA_HOME/HOME), the `*_installed` predicates;
//!   * runtime dependencies per `PkgFamily` in two tiers (both with a DEBIAN
//!     fallback): required `dep_packages`/`dep_install_cmd` — strictly,
//!     recommended `optional_dep_packages`/`optional_dep_install_cmd` — on a
//!     separate line with `|| true`; the install commands THEMSELVES are not
//!     here, the `system_plan::install_cmd` engine provides them, here is only
//!     the form of the bash line;
//!   * generation of the system-mode install script (icon block, helper block,
//!     dep branch) + plan (WriteFile → pkexec → RemoveFiles) + cleanup list;
//!   * uninstall: the `distro_pkg_owner` / `distro_pkg_remove` tables,
//!     `parse_owner_output`, candidate binaries, pkg-remove via the manager,
//!     the system uninstall script + plan, `installed_files`, the list of user
//!     removal paths (`uninstall_user_paths`, including per-user icons) and the
//!     summary message (`uninstall_summary`).
//!
//! NOT ported (side effects / GUI — executed when hooked to live Python): the
//! real pkexec/subprocess/copytree/mkstemp/rmtree, the FS checks
//! `check_if_installed`/`_detect_distro_package` (path existence and the
//! package manager owner — passed as a parameter/closure), user-install and
//! portable-install as sequences of FS operations (except their pure texts —
//! launcher/desktop), the CLI dialog `run_installer_dialog`, the whole PySide6
//! class `QtInstallerWindow`.
//!
//! ⚠ Rule #16 (2026-07-21): fixed a bug of the original — in
//! `install_to_system` the `cleanup_paths` list did NOT include
//! `tmp_stage_helper` / `tmp_stage_policy` (a leak of two temporary files on
//! every install). In the port `install_cleanup_paths` adds them. See the note
//! at the function.

use serde::Serialize;

use crate::system::PkgFamily;
use crate::system_plan::{
    Cmd, PlanStep, RunStepId, DE_HELPER_PATH, DE_POLICY_PATH, PKG_HELPER_PATH,
    PKG_POLICY_PATH, REXON_POLKIT_RULES_PATH, STAGE_HELPER_PATH,
    STAGE_POLICY_PATH,
};

// ============================================================================
// Constants (port of module-level + class-level RexonInstaller)
// ============================================================================

pub const SYSTEM_INSTALL_ACTION: &str = "org.rexon.system-install";
/// Name of the helper for login screen (SDDM) and boot screen (Plymouth) themes.
pub const SYSTEM_HELPER_NAME: &str = "rexon-system-helper";
/// Helper path of the MANUAL install: the installer puts the file there and
/// writes the same path into its policy's `exec.path`. Where to look at run
/// time is answered by [`system_helper_path`] — the package keeps the helper in `/usr/lib`.
pub const SYSTEM_HELPER_PATH: &str = "/usr/local/lib/rexon-system-helper";
pub const SYSTEM_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.system-install.policy";

/// Where the login and boot screen theme helper lies on this machine.
///
/// ⛔ The defect this closes (28.09.2026): the path was nailed to
/// `/usr/local/lib`, and the package didn't install the helper at all — for a
/// package user SDDM and Plymouth themes from «Appearance» never installed
/// ("the system part is not installed"). The selection rule is the same as for
/// the other helpers ([`crate::system_plan::pick_helper_path`]): the package
/// before the manual install.
pub fn system_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| {
        crate::system_plan::pick_helper_path(SYSTEM_HELPER_NAME, &|p| {
            crate::system_plan::is_exec_file(p)
        })
    })
}

/// Class-level paths of system mode (for the uninstaller / backward compat).
pub const INSTALL_DIR: &str = "/usr/local/bin";
pub const LIB_DIR: &str = "/usr/local/lib/rexon";
pub const DESKTOP_DIR: &str = "/usr/share/applications";

pub const EXT_ID: &str = "rexon-wallpaper@rexon.app";
/// The old name of the wallpaper extension: the domain was inherited from
/// another project (the Razer Axon port), it has nothing to do with Rexon. The
/// directory with the old name is wiped both on install and on uninstall —
/// otherwise the shell will see two identical extensions and draw the wallpaper twice.
pub const EXT_ID_LEGACY: &str = "rexon-wallpaper@openaxon.io";

/// Timeout of the system install root script: it installs missing components
/// inside (600 s — headroom for installs under a password).
/// ⭐ Fix of 15.09.2026: it was 300 s.
pub const SYSTEM_INSTALL_TIMEOUT_SECS: u64 = 600;

// ============================================================================
// Texts (text for text with Python)
// ============================================================================

/// Splash modules that plymouth ships itself (a theme's `ModuleName=`).
/// plymouth turns the module name into the path `<modules dir>/<name>.so`
/// WITHOUT checking (`ply_boot_splash_load`), and `ModuleName=../../../share/plymouth/
/// themes/<theme>/evil` would load a `.so` from the theme itself — code as
/// root, on every boot and shutdown, and in the initramfs too. So a theme is
/// accepted only on a stock module, and a `.so` inside a theme is a refusal.
pub const PLYMOUTH_SPLASH_MODULES: &[&str] = &[
    "details", "fade-throbber", "script", "space-flares", "text", "tribar", "two-step",
];

/// The body of `rexon-system-helper` — privileged install of SDDM and
/// Plymouth themes.
///
/// ⛔ 28.09.2026 (the "one password" plan, item B0c) three things were closed,
/// and none of them may be weakened:
///   1. **root code from a Plymouth theme.** `ModuleName` — only from
///      [`PLYMOUTH_SPLASH_MODULES`] (every occurrence of the word in every
///      `.plymouth`, strictly line by line: plymouth's `fscanf` parsing
///      tolerates a newline between the key and `=`, and a lax check would see
///      something other than what it reads), a `.so` in the theme — refusal;
///   2. **swapping the directory between check and copy.** Previously:
///      `realpath`, then `cp` by path — a user directory, and any part of the
///      path could be swapped for a link in time (root copying a foreign
///      directory into the world-readable `/usr/share`). Now the helper first
///      ENTERS the directory (`cd -P`), checks the location and owner of what
///      it entered, copies `.` into its own 0700 directory and checks the COPY —
///      the user won't touch it;
///   3. **links and special files** in a theme — refusal (a theme is ordinary
///      files and directories); writing to the destination uses
///      `--remove-destination` so as not to write through a link left in an
///      older version of the theme.
///
/// The same rules are checked before the password by [`system_theme_problem`] —
/// so the person sees the reason right away, not after the password dialog.
pub fn system_helper_script() -> String {
    let text = SYSTEM_HELPER_SCRIPT_TMPL
        .replace("__PLY_MODULES__", &PLYMOUTH_SPLASH_MODULES.join("|"));
    // The name in messages is the old "rexon-helper" (the helper has always
    // signed that way; logs and checks know it).
    crate::system_l10n::localize_script("rexon-helper", &text)
}

const SYSTEM_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon system helper — privileged install and selection of SDDM/Plymouth themes.
# Run EXCLUSIVELY through pkexec (polkit action org.rexon.system-install,
# auth_admin_keep: a confirmed password is kept for a few minutes for the
# Rexon window). Accepts only strictly validated arguments. Does NOT run
# arbitrary code and does not let it into the system as part of a theme.
set -euo pipefail
export LC_ALL=C LANG=C
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
exec </dev/null
umask 022

__L10N__

[ "$#" -eq 4 ] || die sys_args

category="$1"
theme_id="$2"
staged="$3"
family="$4"

# 1) category — a strict allowlist. `apply-<kind>` — the «Apply» mode: the theme
#    is already in /usr/share/<kind>/themes and only needs to be selected (decision
#    of 04.10.2026: installing does not enable the theme, the button does). `remove-<kind>`
#    — the «Remove» mode: remove the theme directory (not the active one, not from a package).
mode=install
case "$category" in
    sddm|plymouth) : ;;
    apply-sddm|apply-plymouth) mode=apply; category="${category#apply-}" ;;
    remove-sddm|remove-plymouth) mode=remove; category="${category#remove-}" ;;
    *) die bad_category "$category" ;;
esac

# 2) theme_id — only safe characters; no / .. and no leading '-' and '.'
#    ('-x' would become a flag of plymouth-set-default-theme, '.' — the themes root).
case "$theme_id" in
    ""|-*|.*|*/*|*..*) die bad_theme_name "$theme_id" ;;
esac
if ! printf '%s' "$theme_id" | grep -Eq '^[A-Za-z0-9._-]+$'; then
    die bad_theme_chars "$theme_id"
fi

# 3) family — accepted for call compatibility; the Plymouth activation
#    method is chosen NOT by it but by the actual presence of tools.
case "$family" in
    arch|debian) : ;;
    *) die bad_family "$family" ;;
esac

# Rebuilding the kernel image after a theme change — with the first thing available. The order is
# as in the session rights helper: first plymouth's own script (it is also called by
# `plymouth-set-default-theme -R`; the distribution sets it up for its own
# bootloader — on CachyOS with Limine that is limine-mkinitcpio, while a bare
# `mkinitcpio -P` does not update the Limine image). Nothing to use — code 3.
rebuild_initramfs() {
    local p
    for p in /usr/lib/plymouth/plymouth-update-initrd \
             /usr/libexec/plymouth/plymouth-update-initrd; do
        if [ -x "$p" ]; then "$p"; return; fi
    done
    if command -v update-initramfs >/dev/null 2>&1; then
        update-initramfs -u
    elif command -v limine-mkinitcpio >/dev/null 2>&1; then
        limine-mkinitcpio
    elif command -v mkinitcpio >/dev/null 2>&1; then
        mkinitcpio -P
    elif command -v limine-dracut >/dev/null 2>&1; then
        limine-dracut
    elif command -v dracut >/dev/null 2>&1; then
        dracut --regenerate-all --force
    elif [ -x /usr/lib/booster/regenerate_images ]; then
        /usr/lib/booster/regenerate_images
    else
        return 3
    fi
}

# Whether the file has `[Theme] Current=` (not in a comment, exactly in the Theme group).
sddm_has_current() {
    awk 'BEGIN { r = 1 }
         { s = $0; sub(/^[ \t]+/, "", s) }
         s ~ /^\[/ { g = s; sub(/^\[[ \t]*/, "", g); sub(/[ \t]*\][ \t\r]*$/, "", g); next }
         g == "Theme" && s ~ /^Current[ \t]*=/ { r = 0 }
         END { exit r }' "$1"
}

# The SDDM login screen theme. SDDM reads /etc/sddm.conf LAST, after all
# sddm.conf.d/*.conf, and its `Current=` silently overrode our own file (so on
# CachyOS: Current=breeze). So the value is changed in all files of /etc
# where the key already exists (the KDE settings will show this theme too), and our own file
# is written only if the key is nowhere in /etc. Two phases: first edited copies
# next to each file (if even one fails, or a file is a link, the
# copies are removed and the system is untouched), then mv over. Other lines
# stay as they were.
sddm_set_theme() {
    local f tmp files=() tmps=()
    for f in /etc/sddm.conf.d/*.conf /etc/sddm.conf; do
        [ -f "$f" ] || continue
        sddm_has_current "$f" || continue
        [ ! -L "$f" ] || die theme_set_elsewhere "$theme_id"
        files+=("$f")
    done
    for f in "${files[@]}"; do
        if ! tmp="$(mktemp "$f.rexon.XXXXXXXX")" \
            || ! awk -v t="$theme_id" '
                { s = $0; sub(/^[ \t]+/, "", s) }
                s ~ /^\[/ { g = s; sub(/^\[[ \t]*/, "", g); sub(/[ \t]*\][ \t\r]*$/, "", g); print; next }
                g == "Theme" && match($0, /^[ \t]*Current[ \t]*=/) { print substr($0, 1, RLENGTH) t; next }
                { print }' "$f" >"$tmp" \
            || ! chmod --reference="$f" -- "$tmp"; then
            rm -f -- "${tmps[@]}" "${tmp:-}"
            exit 1
        fi
        tmps+=("$tmp")
    done
    local i
    for i in "${!files[@]}"; do
        mv -f -- "${tmps[$i]}" "${files[$i]}"
    done
    if [ "${#files[@]}" = 0 ]; then
        install -d /etc/sddm.conf.d
        cat > /etc/sddm.conf.d/zz-rexon-theme.conf <<REXON_EOF
[Theme]
Current=$theme_id
REXON_EOF
    fi
}

# Selecting a theme that already lies in /usr/share/<kind>/themes/<name>.
activate() {
    local dest="/usr/share/$category/themes/$theme_id" prev
    if [ "$category" = "sddm" ]; then
        sddm_set_theme
        return 0
    fi
    # The activation method is chosen by FACT, not by family: on Ubuntu
    # the plymouth-set-default-theme binary is cut out of the package and only
    # alternatives remain, while on Debian it is the other way round — alternatives are not
    # used for plymouth (the theme there is a regular file, update-alternatives fails
    # on it), but the binary works and calls update-initramfs itself.
    # On Debian and openSUSE the binary lives in /usr/sbin (it is already in PATH above).
    # The theme is selected without -R, the image is rebuilt right after: if that fails,
    # the previous theme goes back into the settings (the image is the previous one anyway).
    if command -v plymouth-set-default-theme >/dev/null 2>&1; then
        prev="$(plymouth-set-default-theme 2>/dev/null || true)"
        plymouth-set-default-theme "$theme_id"
        if ! rebuild_initramfs; then
            case "$prev" in
                ""|"$theme_id"|-*|.*|*/*) : ;;
                *) plymouth-set-default-theme "$prev" || true ;;
            esac
            die initramfs_failed
        fi
    elif command -v update-alternatives >/dev/null 2>&1; then
        pl="$dest/$theme_id.plymouth"
        [ -f "$pl" ] || die no_plymouth_file "$theme_id.plymouth"
        prev="$(readlink -f /usr/share/plymouth/themes/default.plymouth 2>/dev/null || true)"
        update-alternatives --install \
            /usr/share/plymouth/themes/default.plymouth \
            default.plymouth "$pl" 100
        update-alternatives --set default.plymouth "$pl"
        if ! rebuild_initramfs; then
            if [ -n "$prev" ] && [ "$prev" != "$pl" ] && [ -f "$prev" ]; then
                update-alternatives --set default.plymouth "$prev" || true
            fi
            die initramfs_failed
        fi
    else
        die no_activation
    fi
}

# A `key = value` line in a settings file (the key case-insensitive,
# spaces at the edges do not count). Parsing — with bash means, without external programs.
conf_has() {
    local line k v
    while IFS= read -r line || [ -n "$line" ]; do
        line="${line%$'\r'}"
        case "$line" in *=*) : ;; *) continue ;; esac
        k="${line%%=*}"; v="${line#*=}"
        k="${k#"${k%%[![:space:]]*}"}"; k="${k%"${k##*[![:space:]]}"}"
        v="${v#"${v%%[![:space:]]*}"}"; v="${v%"${v##*[![:space:]]}"}"
        [ "${k,,}" = "${2,,}" ] && [ "$v" = "$3" ] && return 0
    done < "$1"
    return 1
}

# Whether the theme is selected now. With a margin: any mention in any source, without
# parsing groups and order — an extra refusal is better than a removed active theme
# (a login screen without a theme, a splash without files on the next image build).
theme_is_active() {
    local f link
    if [ "$category" = "sddm" ]; then
        for f in /usr/lib/sddm/sddm.conf.d/*.conf /etc/sddm.conf.d/*.conf /etc/sddm.conf; do
            [ -f "$f" ] && conf_has "$f" Current "$theme_id" && return 0
        done
        return 1
    fi
    for f in /etc/plymouth/plymouthd.conf /usr/share/plymouth/plymouthd.defaults \
             /usr/share/plymouth/plymouthd.conf; do
        [ -f "$f" ] && conf_has "$f" Theme "$theme_id" && return 0
    done
    link="$(readlink -f /usr/share/plymouth/themes/default.plymouth 2>/dev/null || true)"
    case "$link" in "$dest"/*) return 0 ;; esac
    return 1
}

# The directory belongs to a system package — the package manager removes it, not us.
theme_from_package() {
    if command -v pacman >/dev/null 2>&1 && pacman -Qoq -- "$dest" >/dev/null 2>&1; then
        return 0
    fi
    if command -v dpkg-query >/dev/null 2>&1 && dpkg-query -S -- "$dest" >/dev/null 2>&1; then
        return 0
    fi
    if command -v rpm >/dev/null 2>&1 && rpm -qf -- "$dest" >/dev/null 2>&1; then
        return 0
    fi
    return 1
}

[ -n "${PKEXEC_UID:-}" ] || die not_via_pkexec

# The «Remove» mode: only the directory /usr/share/<kind>/themes/<name> — a regular
# directory, not a link (the name is already without / and ..), not the currently selected theme and not
# files of a system package. rm does not dereference links INSIDE the theme.
if [ "$mode" = "remove" ]; then
    [ "$staged" = "-" ] || die sys_args
    dest="/usr/share/$category/themes/$theme_id"
    [ -d "$dest" ] && [ ! -L "$dest" ] || die theme_not_installed "$theme_id"
    if theme_is_active; then die theme_active "$theme_id"; fi
    if theme_from_package; then die theme_from_package "$theme_id"; fi
    rm -rf --one-file-system -- "$dest"
    exit 0
fi

# The «Apply» mode: we copy nothing, staged is a dash. The theme must already
# be in the system directory as a regular directory (not a link): only what root
# put there can be selected.
if [ "$mode" = "apply" ]; then
    [ "$staged" = "-" ] || die sys_args
    dest="/usr/share/$category/themes/$theme_id"
    [ -d "$dest" ] && [ ! -L "$dest" ] || die theme_not_installed "$theme_id"
    if [ "$category" = "plymouth" ]; then
        [ -f "$dest/$theme_id.plymouth" ] || die no_plymouth_file "$theme_id.plymouth"
    fi
    activate
    exit 0
fi

# 4) staged — the caller's directory under /tmp|/var/tmp (Rexon creates it with
#    mkdtemp). First we ENTER it: from then on we work with that very directory,
#    and replacing any part of the path with a link changes nothing anymore. The place and
#    the owner are checked at the entry, not on the path.
[ -d "$staged" ] || die staged_not_dir "$staged"
cd -P -- "$staged" 2>/dev/null || die staged_unreachable "$staged"
real_staged="$(pwd -P)"
case "$real_staged" in
    /tmp/?*|/var/tmp/?*) : ;;
    *) die staged_outside_tmp "$real_staged" ;;
esac
[ "$(stat -c %u .)" = "$PKEXEC_UID" ] || die staged_not_callers "$real_staged"

# 5) A copy into our own directory (0700, root) — it is EXACTLY that copy that is checked and installed.
work="$(mktemp -d /var/tmp/rexon-system-helper.XXXXXXXX)" || die no_work_dir
trap 'rm -rf -- "$work"' EXIT
src="$work/theme"
cp -R -P -- . "$src" || die theme_copy_failed

# 6) A theme — only regular files and directories.
odd="$(find "$src" -mindepth 1 ! -type f ! -type d -print -quit)"
[ -z "$odd" ] || die theme_odd "${odd#"$src"/}"

# 7) Plymouth: no executable code, no module outside the modules directory.
modline='^[[:space:]]*ModuleName[[:space:]]*=[[:space:]]*(__PLY_MODULES__)[[:space:]]*$'
check_plymouth_conf() {
    local f="$1" rel="${1#"$src"/}" total strict
    # NUL: plymouth reads the line up to it, the check would see something else.
    [ "$(tr -d '\000' <"$f" | wc -c)" = "$(wc -c <"$f")" ] \
        || die nul_byte "$rel"
    # Every occurrence of the word ModuleName (case-insensitive) must be
    # a whole line ModuleName=<stock module>.
    total="$({ grep -aoi 'modulename' "$f" || true; } | wc -l)"
    strict="$({ grep -aE "$modline" "$f" || true; } | wc -l)"
    [ "$total" = "$strict" ] \
        || die bad_module "$rel"
    printf '%s' "$strict"
}
if [ "$category" = "plymouth" ]; then
    so="$(find "$src" -type f \( -name '*.so' -o -name '*.so.*' \) -print -quit)"
    [ -z "$so" ] || die theme_has_lib "${so#"$src"/}"
    main="$src/$theme_id.plymouth"
    [ -f "$main" ] || die no_plymouth_file "$theme_id.plymouth"
    while IFS= read -r -d '' conf; do
        check_plymouth_conf "$conf" >/dev/null
    done < <(find "$src" -type f -iname '*.plymouth' -print0)
    [ "$(check_plymouth_conf "$main")" -ge 1 ] \
        || die no_stock_module "$theme_id.plymouth"
fi

# ── Install ──────────────────────────────────────────────────────────
# Copy only: the theme is selected by the `apply-<kind>` mode (the «Apply» button).
dest="/usr/share/$category/themes/$theme_id"
install -d "$dest"
cp -R -P -T --remove-destination -- "$src" "$dest"
chmod -R a+rX "$dest"
exit 0
"#;

/// Why a system theme won't be installed — the same check the root helper
/// does ([`system_helper_script`]), only BEFORE the password dialog.
///
/// By itself it protects nothing (it checks the user directory, and the
/// theme is installed anyway by the helper, which checks its own copy), but
/// the person learns the reason right away. It is also the only check on the
/// extensions fallback path (a raw `pkexec bash` when there is no helper).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemThemeProblem {
    /// A link, device, pipe etc. — a theme must be ordinary files and
    /// directories. Inside — the path relative to the theme root.
    OddFile(String),
    /// A boot theme carries its own executable code: a `.so` inside or a
    /// `ModuleName` not from [`PLYMOUTH_SPLASH_MODULES`] (including with `../`).
    BootSplashCode(String),
}

impl SystemThemeProblem {
    /// Text for the person.
    pub fn message(&self) -> String {
        use crate::i18n::trf;
        match self {
            // TRANSLATORS: %1 is a file inside the theme, relative to the theme folder.
            SystemThemeProblem::OddFile(f) => trf(
                "The theme can't be installed: it contains a link or a special file (%1). \
                 Rexon installs only themes made of ordinary files. Nothing was changed.",
                &[f],
            ),
            // TRANSLATORS: %1 is a file inside the theme, relative to the theme folder.
            SystemThemeProblem::BootSplashCode(f) => trf(
                "The boot splash theme can't be installed: it brings its own program code \
                 (%1), which would run with full system rights at every boot. Rexon installs \
                 only themes built on Plymouth's standard modules. Nothing was changed.",
                &[f],
            ),
        }
    }
}

/// Check a system theme directory (`kind` — `sddm` or `plymouth`).
/// `theme_id` — if known, `<theme_id>.plymouth` must name a stock module.
/// Links are not dereferenced.
pub fn system_theme_problem(
    kind: &str,
    dir: &std::path::Path,
    theme_id: Option<&str>,
) -> Option<SystemThemeProblem> {
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut stack = vec![(String::new(), dir.to_path_buf())];
    while let Some((rel_dir, abs_dir)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&abs_dir) else {
            return Some(SystemThemeProblem::OddFile(rel_dir));
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if rel_dir.is_empty() { name } else { format!("{rel_dir}/{name}") };
            let Ok(ft) = e.file_type() else {
                return Some(SystemThemeProblem::OddFile(rel));
            };
            if ft.is_dir() {
                stack.push((rel, e.path()));
            } else if ft.is_file() {
                files.push((rel, e.path()));
            } else {
                return Some(SystemThemeProblem::OddFile(rel));
            }
        }
    }
    if kind != "plymouth" {
        return None;
    }
    files.sort();
    for (rel, _) in &files {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        if base.ends_with(".so") || base.contains(".so.") {
            return Some(SystemThemeProblem::BootSplashCode(rel.clone()));
        }
    }
    for (rel, path) in &files {
        if !rel.to_ascii_lowercase().ends_with(".plymouth") {
            continue;
        }
        let stock = std::fs::read(path).ok().and_then(|b| plymouth_stock_module_lines(&b));
        let main = theme_id.map(|id| *rel == format!("{id}.plymouth")).unwrap_or(false);
        match stock {
            None => return Some(SystemThemeProblem::BootSplashCode(rel.clone())),
            Some(0) if main => return Some(SystemThemeProblem::BootSplashCode(rel.clone())),
            Some(_) => {}
        }
    }
    None
}

/// How many `ModuleName=<stock module>` lines a `.plymouth` has; `None` —
/// there is a NUL byte or an occurrence of the word `ModuleName`
/// (case-insensitive) outside such a line. The rule is the same as the helper's.
fn plymouth_stock_module_lines(bytes: &[u8]) -> Option<usize> {
    if bytes.contains(&0) {
        return None;
    }
    let lower = bytes.to_ascii_lowercase();
    let total = lower.windows(b"modulename".len()).filter(|w| *w == b"modulename").count();
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | b'\x0b' | b'\x0c');
    let strict = bytes
        .split(|&c| c == b'\n')
        .filter(|line| {
            let t = trim_bytes(line, is_space);
            let Some(rest) = t.strip_prefix(b"ModuleName") else { return false };
            let rest = trim_bytes(rest, is_space);
            let Some(value) = rest.strip_prefix(b"=") else { return false };
            let value = trim_bytes(value, is_space);
            PLYMOUTH_SPLASH_MODULES.iter().any(|m| m.as_bytes() == value)
        })
        .count();
    (total == strict).then_some(strict)
}

fn trim_bytes(b: &[u8], is_space: impl Fn(u8) -> bool) -> &[u8] {
    let start = b.iter().position(|&c| !is_space(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|&c| !is_space(c)).map(|i| i + 1).unwrap_or(start);
    &b[start..end.max(start)]
}

/// XML of the SYSTEM_INSTALL_ACTION polkit policy — port of `_system_policy_xml`
/// (the manual install path; the package prints [`system_policy_xml_for`]).
pub fn system_policy_xml() -> String {
    system_policy_xml_for(SYSTEM_HELPER_PATH)
}

/// XML of the same policy for a helper lying at the given path.
///
/// `exec.path` is a contract with polkit: if it doesn't match what is actually
/// run, pkexec won't pick our action (a password every time).
pub fn system_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{action}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">{helper}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        action = SYSTEM_INSTALL_ACTION,
        helper = helper_path,
        description = crate::system_l10n::polkit_elem("    ", "description", SYSTEM_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", POLICY_MESSAGE_SYSTEM_THEME),
    )
}

// ============================================================================
// Rexon entry point: the Rust window or the classic Python window
// ============================================================================
//
// OWNER'S DECISION (11.09.2026). The Rexon desktop entry leads to the Rust
// window `rexon-main-window`; the Python window remains a fallback item of the
// entry's menu "Rexon (classic window)".
//
// ⚠️ THE ENTRY IS CHOSEN BY ONE FUNCTION — [`launch_entry_live`], and it is
// called by BOTH writers: the desktop entry ([`desktop_content`]) and the
// autostart entry (`tray::autostart_exec`, written by `rexon-gui/src/tray.rs`
// for both the Python and the Rust host). The reason is strict: the tray is
// the whole process host, and the "one Rexon per session" lock is shared by
// both entries. A desktop entry for the Rust window with a Python tray in
// autostart would bring up the PYTHON window: the Rust binary knocks on the
// taken lock and exits. The two writers must not drift apart — so a second
// copy of the selection rule is forbidden.
//
// WHERE THE CHECK IS. At generation time, without a launch wrapper. The
// wrapper would live in the `rexon` launcher, and that is Python: then the
// Rust window wouldn't start at all without the interpreter, i.e. moving off
// Python would lose its point. If the binary later disappears, `TryExec` on
// the same path removes the item from the menu instead of an entry that
// silently does nothing; regeneration brings back the Python entry.
//
// The choice looks only at what is the same for any writer process: the
// [`ENTRY_ENV`] variable, PATH and fixed paths. The path of one's OWN
// executable is deliberately not included: from the Rust window it would find
// the binary, from Python — not, and the desktop entry and autostart written
// by different hosts would diverge.

/// What the product calls the classic Python window. One place for the whole
/// product: this name is referenced by the Rust window's answer about features
/// that so far exist only in the classic window (`rexon-gui/src/settings_actions.rs`).
///
/// ⚠️ On 16.09.2026 the item with this name was removed from the desktop entry
/// ([`desktop_content_for`]): there is no longer anything to open the classic
/// window from the menu with, and the answer that tells the person to "click
/// the desktop entry" must be rewritten together with the move off Python.
pub const CLASSIC_WINDOW_ENTRY: &str = "Rexon (классическое окно)"; // i18n-exempt: the removed item's name as old Russian desktop entries carried it

/// Fallback variable: `REXON_ENTRY=python` sends the main entry to Python.
pub const ENTRY_ENV: &str = "REXON_ENTRY";

/// The [`ENTRY_ENV`] value that selects the Python entry.
pub const ENTRY_PYTHON: &str = "python";

/// The product command in `PATH` — the `rexon` launcher, installed by the
/// installer (and the distribution package).
///
/// ⚠️ The name is kept, but since 16.09.2026 the launcher does NOT lead to
/// Python: both the package and the manual install print an `sh` launcher for
/// the Rust window binary ([`system_launcher`]). The value still serves as the
/// desktop entry's fallback `Exec=` ([`LaunchEntry::Python`]) — for when the
/// window binary wasn't found and there is no full path to write into the entry.
pub const PYTHON_ENTRY_CMD: &str = "rexon";

/// Interpreter for the FALLBACK to the Python entry when the [`PYTHON_ENTRY_CMD`]
/// launcher is not in `PATH` (developer tree): the entry hooks then call
/// `python3 -m rexon.app …`. One place for all of Rust: with the Rust entry
/// ([`LaunchEntry::MainWindow`]) the value is not used at all.
pub const PYTHON_FALLBACK_EXE: &str = "python3";

/// Where the package keeps the window binary (the Rexon data directory, not in PATH).
pub const PACKAGE_MAIN_WINDOW: &str = "/usr/lib/rexon/rexon-main-window";

/// The window binary inside the developer tree, relative to the tree root.
pub const DEV_TREE_MAIN_WINDOW_REL: &str = "rust/target/release/rexon-main-window";

/// The product data directory in the developer tree, relative to the tree root.
/// The layout inside is the same as the payload's `rexon/` package
/// (`qt_wallpaper_engine/qml_ui`, `kpackage`, `themes.json`, `assets`, the
/// GNOME and Cinnamon add-ons); until 02.10.2026 the data lay in `rexon/` mixed
/// with the Python code, now Python is an archive (`_archived/`).
pub const DEV_TREE_DATA_DIR: &str = "data";

/// The Rexon data directory of the distribution package — the parent of the
/// window binary ([`PACKAGE_MAIN_WINDOW`]). The `rexon/` package and the QML
/// module directory (`qml_modules::SYSTEM_MODULE_DIRS`) lie there too.
pub fn package_data_dir() -> &'static str {
    match PACKAGE_MAIN_WINDOW.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "/usr/lib/rexon",
    }
}

/// Where to look for the `rexon/` package — the product data: the vendored
/// interface of the wallpaper tab (`qt_wallpaper_engine/qml_ui`), the
/// embedded engine and the translation directories. In decreasing order of
/// precision, without repeats.
///
/// There are four layouts, and all are alive:
///   * developer tree — `<root>/data` ([`DEV_TREE_DATA_DIR`]; the root is
///     derived from the path of one's own binary, `cli::repo_root`);
///   * distribution package — `<package_data_dir>/rexon` (`/usr/lib/rexon/rexon`);
///   * system install — `<LIB_DIR>/rexon` (the layout is the same as the
///     package's: `installer_exec::InstallLayout`; the old Python installer put
///     the package straight into `LIB_DIR`, and this layout didn't find it);
///   * home folder install — `~/.local/lib/rexon/rexon` (the same layout;
///     empty `home` — no candidate).
///
/// There is deliberately no override variable of our own here: the Python side
/// has none either (it finds itself via `__file__`), and `REXON_PACKAGED`
/// speaks of the package MODE, not the path. Starting a second truth about
/// where the product lies is not allowed.
///
/// The right directory is the one where the vendored interface actually lies —
/// the executor decides that (it has the live file system).
pub fn package_data_dir_candidates(repo_root: &str, home: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let root = repo_root.trim_end_matches('/');
    if !root.is_empty() {
        out.push(format!("{root}/{DEV_TREE_DATA_DIR}"));
    }
    out.push(format!("{}/rexon", package_data_dir()));
    out.push(format!("{}/rexon", LIB_DIR.trim_end_matches('/')));
    // Home folder install — the same layout (`<~/.local/lib/rexon>/rexon`).
    if let Some(lib) = user_install_lib_dir(home) {
        out.push(format!("{lib}/rexon"));
    }
    out.dedup();
    out
}

/// The data directory of the home folder install (`~/.local/lib/rexon`), or
/// `None` with an unknown HOME: a relative path would look into the process's
/// current directory.
pub fn user_install_lib_dir(home: &str) -> Option<String> {
    let home = home.trim_end_matches('/');
    if !home.starts_with('/') {
        return None;
    }
    install_paths(InstallMode::User, home, None).lib_dir
}

/// The sign "this is the `rexon/` package": the path of its vendored interface.
/// Relative, from the package directory.
pub const PACKAGE_MARKER_REL: &str = "qt_wallpaper_engine/qml_ui/PageHost.qml";

/// Choose the Rexon data directory among the candidates: the first where the
/// vendored wallpaper tab interface lies. `None` — no layout nearby, and the
/// tab honestly opens without the vendored pages.
///
/// The live file system probe comes as a callback: the core doesn't go to the
/// disk itself, and the check is one for all three layouts (rule #12).
pub fn pick_package_data_dir(
    repo_root: &str,
    is_file: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    package_data_dir_candidates(repo_root, &home)
        .into_iter()
        .find(|dir| is_file(&format!("{dir}/{PACKAGE_MARKER_REL}")))
}

/// Which entry the desktop entry and autostart write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchEntry {
    /// The Rust window; inside — the full path to the binary.
    MainWindow(String),
    /// The classic Python window (the [`PYTHON_ENTRY_CMD`] launcher).
    Python,
}

impl LaunchEntry {
    /// The program for `Exec=` — already quoted if the path requires it.
    pub fn exec_program(&self) -> String {
        match self {
            LaunchEntry::MainWindow(path) => desktop_exec_arg(path),
            LaunchEntry::Python => PYTHON_ENTRY_CMD.to_string(),
        }
    }

    /// The `Exec=` line with arguments (the flags are ours, unquoted).
    pub fn exec(&self, args: &[&str]) -> String {
        let mut out = self.exec_program();
        for a in args {
            out.push(' ');
            out.push_str(a);
        }
        out
    }

    /// The `TryExec=` value: the path as is (it isn't a command line).
    pub fn try_exec(&self) -> String {
        match self {
            LaunchEntry::MainWindow(path) => path.replace('\\', "\\\\"),
            LaunchEntry::Python => PYTHON_ENTRY_CMD.to_string(),
        }
    }
}

/// A command line argument of `Exec=` per the Desktop Entry spec.
///
/// A path with a space or a reserved character is put in double quotes; inside
/// the quotes `"`, `` ` ``, `$` and `\` are escaped with a backslash, and the
/// backslash itself is doubled once more — the key value goes through string
/// unescaping before argument parsing. `%` — always `%%`.
pub fn desktop_exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let arg = arg.replace('%', "%%");
    if !arg.contains(RESERVED) {
        return arg;
    }
    let mut q = String::with_capacity(arg.len() + 2);
    q.push('"');
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' => q.push_str("\\\\"),
            '\\' => q.push_str("\\\\\\"),
            _ => {}
        }
        q.push(c);
    }
    q.push('"');
    q
}

/// Whether the fallback variable asked for the Python entry.
pub fn python_entry_forced(entry_env: Option<&str>) -> bool {
    entry_env.is_some_and(|v| v.trim().eq_ignore_ascii_case(ENTRY_PYTHON))
}

/// Where to look for the window binary — in order: PATH directories, the
/// package path, the system install, the home folder install, developer trees.
///
/// Relative and empty PATH elements are skipped: they look into the writer's
/// current directory, and two processes would get different answers.
/// `home` — the home folder (empty — no home install candidate).
/// `dev_roots` — roots of the trees the installed Rexon directory leads to
/// (for the developer `/usr/local/lib/rexon` is a symlink into the tree).
pub fn main_window_candidates(
    path_env: Option<&str>,
    home: &str,
    dev_roots: &[String],
) -> Vec<String> {
    let bin = crate::window_menu::MAIN_WINDOW_BIN;
    let mut out: Vec<String> = Vec::new();
    for dir in path_env.unwrap_or("").split(':') {
        if dir.starts_with('/') {
            out.push(format!("{}/{bin}", dir.trim_end_matches('/')));
        }
    }
    out.push(PACKAGE_MAIN_WINDOW.to_string());
    // The system install puts the window binary per the package layout
    // (`installer_exec::InstallLayout::main_window`).
    out.push(format!("{}/{bin}", LIB_DIR.trim_end_matches('/')));
    // ⭐ Fix of 15.09.2026: the home folder install puts the window in the same
    // place per the same layout, but the lookup didn't know it — after such an
    // install the desktop entry went to the classic window.
    if let Some(lib) = user_install_lib_dir(home) {
        out.push(format!("{lib}/{bin}"));
    }
    for root in dev_roots.iter().filter(|r| !r.is_empty()) {
        out.push(format!("{}/{DEV_TREE_MAIN_WINDOW_REL}", root.trim_end_matches('/')));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// The entry selection rule — ONE for the desktop entry and autostart.
///
/// `REXON_ENTRY=python` → Python. Otherwise — the first executable window
/// binary candidate; none — Python (not a broken entry). A path with a control
/// character can't be written into `.desktop` — such a candidate is skipped.
pub fn choose_launch_entry(
    entry_env: Option<&str>,
    candidates: &[String],
    is_exec: impl Fn(&str) -> bool,
) -> LaunchEntry {
    if python_entry_forced(entry_env) {
        return LaunchEntry::Python;
    }
    candidates
        .iter()
        .filter(|p| !p.chars().any(char::is_control))
        .find(|p| is_exec(p))
        .map(|p| LaunchEntry::MainWindow(p.clone()))
        .unwrap_or(LaunchEntry::Python)
}

/// The developer tree root, if the installed Rexon directory is a symlink into
/// the `rexon/` package inside the tree. A real directory (a user-install copy,
/// a system install without a symlink) gives no tree.
fn dev_root_behind(lib_dir: &str) -> Option<String> {
    let link = std::path::Path::new(lib_dir);
    if !std::fs::symlink_metadata(link).ok()?.file_type().is_symlink() {
        return None;
    }
    let real = std::fs::canonicalize(link).ok()?;
    Some(real.parent()?.to_string_lossy().into_owned())
}

fn is_executable_file(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The entry per the live machine: [`ENTRY_ENV`], PATH and installed directories.
pub fn launch_entry_live() -> LaunchEntry {
    let home = std::env::var("HOME").unwrap_or_default();
    let user_lib = install_paths(InstallMode::User, &home, None).lib_dir;
    let dev_roots: Vec<String> = [Some(LIB_DIR.to_string()), user_lib]
        .into_iter()
        .flatten()
        .filter_map(|d| dev_root_behind(&d))
        .collect();
    let path_env = std::env::var("PATH").ok();
    choose_launch_entry(
        std::env::var(ENTRY_ENV).ok().as_deref(),
        &main_window_candidates(path_env.as_deref(), &home, &dev_roots),
        is_executable_file,
    )
}

/// Open the Rexon window as a separate process with arguments `args` (a
/// notification button: «Open Wallpaper Engine» — `--section=wallpaper_engine`).
/// If Rexon is already running, the new copy only raises its window on the
/// right section (`single_instance`). `Err` — the reason for the log.
pub fn open_rexon_live(args: &[&str]) -> Result<(), String> {
    let program = match launch_entry_live() {
        LaunchEntry::MainWindow(path) => path,
        LaunchEntry::Python => return Err("nothing to open Rexon with".to_string()),
    };
    std::process::Command::new(&program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't start {program}: {e}"))
}

/// Content of `rexon.desktop` per the live machine — the entry is chosen by
/// [`launch_entry_live`]. The Python installer writes the desktop entry with this function.
pub fn desktop_content() -> String {
    desktop_content_for(&launch_entry_live())
}

// Desktop entry labels: all the catalog's languages go into the file at once
// (`Name[ru]=`), the desktop menu picks by the session locale itself ([`crate::system_l10n`]).
// TRANSLATORS: Menu entry of Rexon: GenericName shown under the name "Rexon".
const DESKTOP_GENERIC_NAME: &str = n_("Desktop Environment Manager");
// TRANSLATORS: Menu entry of Rexon: tooltip/description.
const DESKTOP_COMMENT: &str = n_("Universal desktop environment manager for Linux");
// TRANSLATORS: Search keywords of the menu entry: words separated by ";" and ending with ";". Translate or add words in your language; do not remove the separators.
const DESKTOP_KEYWORDS: &str = n_("desktop;environment;DE;settings;system;cinnamon;xfce;mate;gnome;kde;plasma;");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry.
const DESKTOP_ACTION_INSTALL: &str = n_("Install Desktop Environment");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry.
const DESKTOP_ACTION_BACKUPS: &str = n_("Manage Backups");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry.
const DESKTOP_ACTION_COMPARE: &str = n_("Compare Desktop Environments");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry (KDE only).
const DESKTOP_ACTION_RECOVER: &str = n_("🛟 Recover desktop appearance (emergency)");
// TRANSLATORS: Separate menu entry (KDE only) for the emergency appearance rollback.
const RECOVER_NAME: &str = n_("Rexon — Recover desktop appearance");
// TRANSLATORS: GenericName of the emergency rollback menu entry.
const RECOVER_GENERIC_NAME: &str = n_("Emergency appearance rollback");
// TRANSLATORS: Description of the emergency rollback menu entry.
const RECOVER_COMMENT: &str = n_("Restore the standard desktop look if an applied theme broke it or Rexon won't start");
// TRANSLATORS: Search keywords of the emergency rollback entry, separated by ";" and ending with ";".
const RECOVER_KEYWORDS: &str = n_("rexon;recover;kde;plasma;theme;emergency;");
// TRANSLATORS: Name of the system action in the password dialog (polkit): installing an SDDM or Plymouth theme.
const SYSTEM_POLICY_DESCRIPTION: &str = n_("Install a system theme (SDDM / Plymouth)");
// TRANSLATORS: Text of the system password dialog (polkit).
pub(crate) const POLICY_MESSAGE_SYSTEM_THEME: &str = n_("Authentication is required to install a system theme");

/// Content of `rexon.desktop` for the given entry — port of
/// `_get_desktop_content`.
///
/// Quick actions go to the same entry as the main launch: the Rust window
/// understands `--page=` and `--recover-kde` through the same door as Python
/// (`cli::start_section` / `cli::run_headless`).
///
/// ⭐ Fixed in the port: the keys `X-GNOME-UsesNotifications` and
/// `X-Desktop-File-Install-Version` came after the last `[Desktop Action]`,
/// i.e. belonged to the action group rather than `[Desktop Entry]` — GNOME
/// reads them only from the main group.
///
/// ⭐ Fix of 16.09.2026: the "Rexon (classic window)" item
/// (`Exec=rexon --classic`) was removed from the desktop entry. There is no
/// classic Python window either in the distribution package or in the manual
/// install — the `rexon` command in both leads to the same window, and the
/// item would open a copy of what the entry itself already opens.
///
/// ⭐ Fix of 26.09.2026: `OnlyShowIn=GNOME;KDE;XFCE;MATE;Cinnamon;` was
/// removed. The restriction came from the first snapshot without a rationale,
/// while the product itself installs Hyprland, sway, niri, LXQt, COSMIC too —
/// there was no menu item there at all, and Rexon could only be started from a
/// terminal. The main desktop entry needs no restriction in any environment.
/// `rexon-recover.desktop` keeps it ([`recover_desktop_content_for`]).
pub fn desktop_content_for(entry: &LaunchEntry) -> String {
    use crate::system_l10n::desktop_key as key;
    let actions = "InstallDE;Backup;Compare;Recover;";
    let page = |name: &str| entry.exec(&[&format!("{}{name}", crate::app::PAGE_FLAG)]);
    format!(
        "[Desktop Entry]\n\
         Version=1.5\n\
         Type=Application\n\
         Name=Rexon\n\
         {generic}\
         {comment}\
         {keywords}\
         \n\
         Exec={main}\n\
         TryExec={try_exec}\n\
         Terminal=false\n\
         StartupNotify=true\n\
         \n\
         Icon=rexon\n\
         \n\
         Categories=Qt;Graphics;\n\
         X-GNOME-UsesNotifications=true\n\
         X-Desktop-File-Install-Version=0.26\n\
         \n\
         Actions={actions}\n\
         \n\
         [Desktop Action InstallDE]\n\
         {install_name}\
         Exec={install}\n\
         \n\
         [Desktop Action Backup]\n\
         {backups_name}\
         Exec={backups}\n\
         \n\
         [Desktop Action Compare]\n\
         {compare_name}\
         Exec={compare}\n\
         \n\
         [Desktop Action Recover]\n\
         {recover_name}\
         Exec={recover}\n\
         Icon=emblem-important-symbolic\n",
        generic = key("GenericName", DESKTOP_GENERIC_NAME),
        comment = key("Comment", DESKTOP_COMMENT),
        keywords = key("Keywords", DESKTOP_KEYWORDS),
        install_name = key("Name", DESKTOP_ACTION_INSTALL),
        backups_name = key("Name", DESKTOP_ACTION_BACKUPS),
        compare_name = key("Name", DESKTOP_ACTION_COMPARE),
        recover_name = key("Name", DESKTOP_ACTION_RECOVER),
        main = entry.exec(&[]),
        try_exec = entry.try_exec(),
        install = page("install"),
        backups = page("backups"),
        compare = page("compare"),
        recover = entry.exec(&[crate::app::RECOVER_KDE_FLAG]),
    )
}

/// Content of `rexon-recover.desktop` per the live machine — the entry is the
/// same as the desktop entry's ([`launch_entry_live`]).
pub fn recover_desktop_content() -> String {
    recover_desktop_content_for(&launch_entry_live())
}

/// Content of `rexon-recover.desktop` for the given entry — port of
/// `_get_recover_desktop_content`.
///
/// `OnlyShowIn=KDE` is justified here: `--recover-kde` rolls back the Plasma
/// Global Theme, in other environments the item has nothing to restore.
///
/// The Rust entry is especially apt for the emergency rollback: the
/// `--recover-kde` route is entirely in the core and runs before Qt starts, it
/// doesn't need the interpreter.
pub fn recover_desktop_content_for(entry: &LaunchEntry) -> String {
    use crate::system_l10n::desktop_key as key;
    format!(
        "[Desktop Entry]\n\
         Version=1.0\n\
         Type=Application\n\
         {name}\
         {generic}\
         {comment}\
         {keywords}\
         \n\
         Exec={recover}\n\
         TryExec={try_exec}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         \n\
         Icon=emblem-important-symbolic\n\
         \n\
         Categories=Qt;Graphics;\n\
         OnlyShowIn=KDE;\n",
        name = key("Name", RECOVER_NAME),
        generic = key("GenericName", RECOVER_GENERIC_NAME),
        comment = key("Comment", RECOVER_COMMENT),
        keywords = key("Keywords", RECOVER_KEYWORDS),
        recover = entry.exec(&[crate::app::RECOVER_KDE_FLAG]),
        try_exec = entry.try_exec(),
    )
}

/// The `rexon` command — the manual install launcher. `lib_dir` — the Rexon
/// data directory of this install (`installer_exec::InstallLayout::lib_dir`);
/// the window binary lies in it.
///
/// ⭐ Fix of 16.09.2026 (rule #16): both launchers were Python and called
/// `from rexon.app import main` — the classic window the payload no longer
/// carries (commits 82e2113, b821779). After a manual install the `rexon`
/// command and the desktop entry would lead into the void. Now the launcher is
/// the same `sh` the distribution package prints, and it leads to the Rust
/// window binary.
///
/// ⚠️ The difference from the PACKAGE launcher is exactly one, and deliberate:
/// [`PACKAGED_ENV`] is NOT set here. A manual install is not a package: the
/// package marker would send [`detect_default_mode`] into portable mode, and
/// the person could no longer finish or redo the Rexon install with this same
/// wizard.
///
/// `note` — an explanatory line in the header: a person who opens
/// `/usr/local/bin/rexon` or `~/.local/bin/rexon` sees which install it is.
fn launcher_script(lib_dir: &str, note: &str) -> String {
    let bin = format!(
        "{}/{}",
        lib_dir.trim_end_matches('/'),
        crate::window_menu::MAIN_WINDOW_BIN
    );
    format!(
        "#!/bin/sh\n\
         # Rexon — launcher ({note}).\n\
         exec {bin} \"$@\"\n",
        bin = crate::ext_manager::shlex_quote(&bin),
    )
}

/// The system install launcher (`self.LIB_DIR` is fixed).
pub fn system_launcher(lib_dir: &str) -> String {
    launcher_script(lib_dir, "system install")
}

/// The home folder install launcher.
pub fn user_launcher(lib_dir: &str) -> String {
    launcher_script(lib_dir, "home folder install")
}

// ============================================================================
// Install mode and paths
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallMode {
    System,
    User,
    Portable,
}

impl InstallMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallMode::System => "system",
            InstallMode::User => "user",
            InstallMode::Portable => "portable",
        }
    }

    /// Port of the `mode` validation in `RexonInstaller.__init__` (None — ValueError).
    pub fn parse(s: &str) -> Option<InstallMode> {
        match s {
            "system" => Some(InstallMode::System),
            "user" => Some(InstallMode::User),
            "portable" => Some(InstallMode::Portable),
            _ => None,
        }
    }
}

/// Distributions with an immutable system part: `/usr` and `/usr/local` lie in
/// a read-only snapshot, and the update is atomic (A/B), so everything written
/// there disappears at the next system update.
///
/// SteamOS: the `usr-local.mount` unit was removed from the offload scheme
/// (commit `d953c8b` in `steamos-customizations`, Jan. 2023) — `/usr/local` is
/// no longer a separate partition in `$HOME` but part of the image. Only
/// `$HOME`, Flatpak and the offload directories persist.
const IMMUTABLE_SYSTEM_DISTROS: &[&str] = &["steamos", "holoiso", "bazzite"];

/// Whether this distribution's system part is read-only (by `ID` from os-release).
pub fn has_immutable_system(distro_id: &str) -> bool {
    let id = distro_id.trim().to_ascii_lowercase();
    IMMUTABLE_SYSTEM_DISTROS.contains(&id.as_str())
}

/// The variable set by the distribution package launcher
/// (`packaging/aur/PKGBUILD`, `/usr/bin/rexon`).
pub const PACKAGED_ENV: &str = "REXON_PACKAGED";

/// Port of `RexonInstaller.detect_default_mode`. Steam (Deck/Big Picture/client)
/// → portable (running without install). `get_env` returns the value of an
/// environment variable (None — not set), `distro_id` — `ID` from os-release
/// (an empty string if undetermined).
///
/// On SteamOS in desktop mode (Rexon started from the menu, not via Steam) the
/// system mode must NOT be chosen: `/usr/local` and `/usr/share` are part of
/// the read-only image, the install either fails or disappears at the next
/// SteamOS update. There we install into the home folder — it survives the
/// atomic update.
///
/// The distribution package ([`PACKAGED_ENV`] is set by its launcher) is
/// portable too: the package manager has already laid out the files, and
/// offering an install into `/usr/local` on top of the package would mean
/// showing the installer dialog on every launch. Previously this check was
/// inserted into PKGBUILD by sed; when mode selection moved into the core, sed
/// lost its line and the package build broke.
pub fn detect_default_mode(
    get_env: impl Fn(&str) -> Option<String>,
    distro_id: &str,
) -> InstallMode {
    if get_env(PACKAGED_ENV).is_some()
        || get_env("SteamGameId").is_some()
        || get_env("SteamAppId").is_some()
        || get_env("STEAM_COMPAT_DATA_PATH").is_some()
    {
        return InstallMode::Portable;
    }
    if has_immutable_system(distro_id) {
        return InstallMode::User;
    }
    InstallMode::System
}

/// Why the system install is unavailable — human text for the interface
/// (rule #13), or `None` if it is available.
///
/// The language is the translation catalog's, as in
/// [`system_mode_blocked_reason_in`] (previously a fixed Russian form was here).
pub fn system_mode_blocked_reason(distro_id: &str) -> Option<String> {
    system_mode_blocked_reason_in(distro_id, crate::distro_compat::Lang::En)
}

/// Install paths by mode. In portable — all `None` (Rexon runs from the
/// current folder). Port of `RexonInstaller.__init__`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallPaths {
    pub install_dir: Option<String>,
    pub lib_dir: Option<String>,
    pub desktop_dir: Option<String>,
    pub icon_root: Option<String>,
    pub pixmap_dir: Option<String>,
    pub gnome_ext_dir: Option<String>,
    pub cinnamon_ext_dir: Option<String>,
}

/// Port of the path computation in `RexonInstaller.__init__`.
///
/// * `home` — `os.path.expanduser("~")` (needed for user mode).
/// * `xdg_data_home` — the value of `XDG_DATA_HOME` (None → `{home}/.local/share`).
pub fn install_paths(
    mode: InstallMode,
    home: &str,
    xdg_data_home: Option<&str>,
) -> InstallPaths {
    match mode {
        InstallMode::System => InstallPaths {
            install_dir: Some("/usr/local/bin".into()),
            lib_dir: Some("/usr/local/lib/rexon".into()),
            desktop_dir: Some("/usr/share/applications".into()),
            icon_root: Some("/usr/share/icons/hicolor".into()),
            pixmap_dir: Some("/usr/share/pixmaps".into()),
            gnome_ext_dir: Some("/usr/share/gnome-shell/extensions".into()),
            cinnamon_ext_dir: Some("/usr/share/cinnamon/extensions".into()),
        },
        InstallMode::User => {
            let xdg = match xdg_data_home {
                Some(x) => x.to_string(),
                None => format!("{home}/.local/share"),
            };
            InstallPaths {
                install_dir: Some(format!("{home}/.local/bin")),
                lib_dir: Some(format!("{home}/.local/lib/rexon")),
                desktop_dir: Some(format!("{xdg}/applications")),
                icon_root: Some(format!("{xdg}/icons/hicolor")),
                pixmap_dir: Some(format!("{xdg}/pixmaps")),
                gnome_ext_dir: Some(format!("{xdg}/gnome-shell/extensions")),
                cinnamon_ext_dir: Some(format!("{xdg}/cinnamon/extensions")),
            }
        }
        InstallMode::Portable => InstallPaths {
            install_dir: None,
            lib_dir: None,
            desktop_dir: None,
            icon_root: None,
            pixmap_dir: None,
            gnome_ext_dir: None,
            cinnamon_ext_dir: None,
        },
    }
}

/// Port of `check_if_installed` for system/user modes: rexon-launcher + the lib
/// directory + rexon.desktop exist. `exists` — an FS check (executor).
pub fn system_or_user_installed(
    paths: &InstallPaths,
    exists: impl Fn(&str) -> bool,
    is_dir: impl Fn(&str) -> bool,
) -> bool {
    let (Some(install), Some(lib), Some(desktop)) =
        (&paths.install_dir, &paths.lib_dir, &paths.desktop_dir)
    else {
        return false;
    };
    exists(&format!("{install}/rexon"))
        && is_dir(lib)
        && exists(&format!("{desktop}/rexon.desktop"))
}

// ============================================================================
// First run: whether to offer the install wizard
// ============================================================================
//
// One decision for both entries: the Rust window and the Python window
// (`qt_main/app.py`) must answer the same, otherwise the wizard pops up in
// one and stays silent in the other (rule #12).
//
// The decision is PURE: everything visible only on the live machine
// (variables, the path of one's own executable, file presence, a directory
// symlink) comes through [`FirstRunProbe`]; settings — as a ready map. The
// live probe is [`LiveFirstRunProbe`], the entry per the live machine is
// [`first_run_offer_live`].
//
// ⭐ Fixes against the Python gate (`not RexonInstaller().installed`):
//   * «Not Now» was not remembered — the wizard popped up on EVERY launch.
//     Now there is "don't ask again" ([`KEY_FIRST_RUN_DECLINED`]).
//   * The Rust window from the package doesn't get [`PACKAGED_ENV`]: the
//     package's desktop entry calls `/usr/lib/rexon/rexon-main-window`
//     directly. The gate carried over as is would show the wizard to everyone
//     who installed the package. Now the package is also recognised by the
//     location of its own file ([`packaged_location`]).
//   * Only the default mode was checked: on Arch that is system, and a Rexon
//     installed into the home folder got the wizard on every launch. Now
//     "installed" means in either of the two modes.
//   * The developer tree wasn't recognised at all.

/// A variable for eyeball acceptance: [`FIRST_RUN_FORCE`] — always show the
/// wizard, [`FIRST_RUN_NEVER`] — never show it. Any other value has no effect.
pub const FIRST_RUN_ENV: &str = "REXON_FIRST_RUN";
/// [`FIRST_RUN_ENV`] value: offer the wizard under any conditions.
pub const FIRST_RUN_FORCE: &str = "force";
/// [`FIRST_RUN_ENV`] value: don't offer the wizard.
pub const FIRST_RUN_NEVER: &str = "never";

/// Settings key "don't offer the install again". It lives in the same
/// `settings.json` as the onboarding keys (`onboarding::KEY_ENABLED`) and is
/// read the same way: a non-boolean value means "didn't decline".
///
/// ⚠️ The old `wizard_done` key from `settings::defaults` is NOT fit here:
/// nobody reads it, and what it meant before is unknown — using it would
/// silently hide the wizard for those who once had `true` there.
pub const KEY_FIRST_RUN_DECLINED: &str = "installer_first_run_declined";

/// Variables set by Steam (Deck, Big Picture, client) — with them Rexon runs
/// without an install. The same set as in [`detect_default_mode`].
const STEAM_ENVS: &[&str] = &["SteamGameId", "SteamAppId", "STEAM_COMPAT_DATA_PATH"];

/// Directories whose files were laid out by the package manager: everything
/// under `/usr` except `/usr/local` (per FHS it is the distribution's
/// territory — `/usr/bin`, `/usr/lib/rexon`, `/usr/lib64/…`), the NixOS store
/// and the Flatpak application root.
const PACKAGE_PREFIXES: &[&str] = &["/usr/", "/nix/store/", "/app/"];
/// The exception from [`PACKAGE_PREFIXES`]: the manual install puts things here.
const LOCAL_PREFIX: &str = "/usr/local/";

/// What the decision sees on the live machine. Each probe is one observation
/// without conclusions: [`first_run_offer`] draws the conclusions.
pub trait FirstRunProbe {
    /// Value of an environment variable (`None` — not set).
    fn env(&self, key: &str) -> Option<String>;
    /// `ID` from os-release (an empty string — undetermined).
    fn distro_id(&self) -> String;
    /// Where the running Rexon itself lies: the window binary, and for the Python
    /// entry — the module file (NOT the interpreter: `/usr/bin/python3` looks like a package).
    fn own_path(&self) -> Option<String>;
    /// The developer tree root inside which [`Self::own_path`] lies
    /// (empty — not in a tree).
    fn repo_root(&self) -> String;
    /// The developer tree root, if `lib_dir` is a symlink into the tree.
    fn dev_tree_behind(&self, lib_dir: &str) -> Option<String>;
    /// Whether the path exists.
    fn exists(&self, path: &str) -> bool;
    /// Whether it is a directory.
    fn is_dir(&self, path: &str) -> bool;
}

/// Why the wizard is NOT offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstRunSkip {
    /// `REXON_FIRST_RUN=never`.
    ForcedNever,
    /// The package launcher set [`PACKAGED_ENV`].
    PackagedEnv,
    /// One's own file lies where only the package manager puts things.
    PackagedLocation(String),
    /// Launched from Steam — runs without an install.
    Steam(&'static str),
    /// Developer tree: one's own file is inside it, or the installed directory
    /// is a symlink into it.
    DevTree(String),
    /// Already installed into the system.
    InstalledSystem,
    /// Already installed into the home folder.
    InstalledUser,
    /// The person answered «Don't ask again».
    Declined,
}

/// The decision "whether to offer the install wizard".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstRunOffer {
    /// Offer.
    Offer {
        /// The mode chosen in advance: system, and on an immutable system — user.
        default_mode: InstallMode,
        /// Why the system install is unavailable (text for the person).
        system_blocked: Option<String>,
        /// Shown because of `REXON_FIRST_RUN=force`, not because of the machine's conditions.
        forced: bool,
    },
    /// Don't offer.
    Skip(FirstRunSkip),
}

impl FirstRunOffer {
    /// Whether to offer the wizard.
    pub fn is_offer(&self) -> bool {
        matches!(self, FirstRunOffer::Offer { .. })
    }

    /// A line for the log: the decision and the reason.
    pub fn log_line(&self) -> String {
        match self {
            FirstRunOffer::Offer { default_mode, forced, .. } => format!(
                "install wizard: offer (mode {}{})",
                default_mode.as_str(),
                if *forced { ", REXON_FIRST_RUN=force" } else { "" }
            ),
            FirstRunOffer::Skip(reason) => {
                let why = match reason {
                    FirstRunSkip::ForcedNever => "REXON_FIRST_RUN=never".to_string(),
                    FirstRunSkip::PackagedEnv => format!("distribution package ({PACKAGED_ENV})"),
                    FirstRunSkip::PackagedLocation(p) => {
                        format!("distribution package (file in {p})")
                    }
                    FirstRunSkip::Steam(var) => format!("started from Steam ({var})"),
                    FirstRunSkip::DevTree(root) => format!("developer tree ({root})"),
                    FirstRunSkip::InstalledSystem => "already installed system-wide".to_string(),
                    FirstRunSkip::InstalledUser => {
                        "already installed in the home folder".to_string()
                    }
                    FirstRunSkip::Declined => {
                        format!("the person chose \"don't ask again\" ({KEY_FIRST_RUN_DECLINED})")
                    }
                };
                format!("install wizard: don't offer — {why}")
            }
        }
    }
}

/// [`FIRST_RUN_ENV`] value: `Some(true)` — force, `Some(false)` — never,
/// `None` — not set or unclear.
pub fn first_run_override(value: Option<&str>) -> Option<bool> {
    let v = value?.trim();
    if v.eq_ignore_ascii_case(FIRST_RUN_FORCE) {
        Some(true)
    } else if v.eq_ignore_ascii_case(FIRST_RUN_NEVER) {
        Some(false)
    } else {
        None
    }
}

/// Whether one's own file lies where only the package manager puts things. The
/// " (deleted)" tail the kernel appends to the path of a rebuilt binary is
/// stripped. A relative path proves nothing — `false`.
pub fn packaged_location(own_path: &str) -> bool {
    let path = own_path
        .strip_suffix(" (deleted)")
        .unwrap_or(own_path);
    !path.starts_with(LOCAL_PREFIX) && PACKAGE_PREFIXES.iter().any(|p| path.starts_with(p))
}

/// Whether the person declined the wizard — by the settings content. A
/// non-boolean value (the file was edited by hand) reads as "didn't decline":
/// offering once more is safer than silently hiding the install forever.
pub fn first_run_declined_from(settings: &serde_json::Map<String, serde_json::Value>) -> bool {
    settings
        .get(KEY_FIRST_RUN_DECLINED)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The mode the wizard chooses in advance. Portable never happens here: where
/// it is appropriate (package, Steam), the wizard is not offered.
fn offered_mode(distro_id: &str) -> InstallMode {
    if has_immutable_system(distro_id) {
        InstallMode::User
    } else {
        InstallMode::System
    }
}

/// The decision "whether to offer the install wizard".
///
/// Order of checks (the first that fires gives the answer):
/// `REXON_FIRST_RUN` → package by variable → package by file location → Steam →
/// developer tree → installed into the system → into the home folder →
/// "don't ask again" → offer.
pub fn first_run_offer(
    probe: &dyn FirstRunProbe,
    settings: &serde_json::Map<String, serde_json::Value>,
) -> FirstRunOffer {
    let distro_id = probe.distro_id();
    let offer = |forced: bool| FirstRunOffer::Offer {
        default_mode: offered_mode(&distro_id),
        system_blocked: system_mode_blocked_reason(&distro_id),
        forced,
    };
    match first_run_override(probe.env(FIRST_RUN_ENV).as_deref()) {
        Some(true) => return offer(true),
        Some(false) => return FirstRunOffer::Skip(FirstRunSkip::ForcedNever),
        None => {}
    }
    if probe.env(PACKAGED_ENV).is_some() {
        return FirstRunOffer::Skip(FirstRunSkip::PackagedEnv);
    }
    if let Some(own) = probe.own_path().filter(|p| packaged_location(p)) {
        return FirstRunOffer::Skip(FirstRunSkip::PackagedLocation(own));
    }
    if let Some(var) = STEAM_ENVS.iter().find(|v| probe.env(v).is_some()) {
        return FirstRunOffer::Skip(FirstRunSkip::Steam(var));
    }

    let home = probe.env("HOME").unwrap_or_default();
    let xdg_data = probe.env("XDG_DATA_HOME").filter(|x| !x.is_empty());
    let user = (!home.is_empty())
        .then(|| install_paths(InstallMode::User, &home, xdg_data.as_deref()));

    let root = probe.repo_root();
    if !root.is_empty() {
        return FirstRunOffer::Skip(FirstRunSkip::DevTree(root));
    }
    let libs = std::iter::once(LIB_DIR.to_string())
        .chain(user.as_ref().and_then(|u| u.lib_dir.clone()));
    for lib in libs {
        if let Some(root) = probe.dev_tree_behind(&lib) {
            return FirstRunOffer::Skip(FirstRunSkip::DevTree(root));
        }
    }

    let exists = |p: &str| probe.exists(p);
    let is_dir = |p: &str| probe.is_dir(p);
    let system = install_paths(InstallMode::System, &home, None);
    if system_or_user_installed(&system, exists, is_dir) {
        return FirstRunOffer::Skip(FirstRunSkip::InstalledSystem);
    }
    if user
        .as_ref()
        .is_some_and(|u| system_or_user_installed(u, exists, is_dir))
    {
        return FirstRunOffer::Skip(FirstRunSkip::InstalledUser);
    }
    if first_run_declined_from(settings) {
        return FirstRunOffer::Skip(FirstRunSkip::Declined);
    }
    offer(false)
}

/// The live probe. The two entries differ only in WHAT to consider one's own file.
#[derive(Debug, Clone)]
pub struct LiveFirstRunProbe {
    own_path: Option<String>,
    repo_root: String,
}

impl LiveFirstRunProbe {
    /// For the Rust window binary: one's own executable and the tree around it.
    pub fn for_this_binary() -> Self {
        Self {
            own_path: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            repo_root: crate::cli::repo_root(),
        }
    }

    /// For an entry whose executable is foreign (the Python window: there
    /// `current_exe` is `/usr/bin/python3`). `own_path` — the real path of one's
    /// own module (`os.path.realpath(__file__)`), `repo_root` — the developer tree
    /// root around it or an empty string.
    pub fn for_path(own_path: Option<String>, repo_root: String) -> Self {
        Self { own_path, repo_root }
    }
}

impl FirstRunProbe for LiveFirstRunProbe {
    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
    fn distro_id(&self) -> String {
        crate::system::DistroDetector::detect_live().distro_id()
    }
    fn own_path(&self) -> Option<String> {
        self.own_path.clone()
    }
    fn repo_root(&self) -> String {
        self.repo_root.clone()
    }
    fn dev_tree_behind(&self, lib_dir: &str) -> Option<String> {
        dev_root_behind(lib_dir)
    }
    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }
    fn is_dir(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }
}

/// The decision per the live machine for the Rust window binary. Settings are
/// only read: the config directory is not created for the sake of an answer.
pub fn first_run_offer_live() -> FirstRunOffer {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    first_run_offer(&LiveFirstRunProbe::for_this_binary(), &settings)
}

/// The answer to the wizard's «Not Now». `remember` — «Don't ask again» is
/// checked: then the refusal is recorded and saved right away. Without the
/// check nothing is written — the wizard will be offered at the next launch.
///
/// The settings manager comes from outside, as in `onboarding::session_answer`:
/// the checks run the function on a temporary file.
pub fn session_first_run_decline(
    manager: &mut crate::settings::SettingsManager,
    remember: bool,
) -> std::io::Result<()> {
    if !remember {
        return Ok(());
    }
    manager.set(KEY_FIRST_RUN_DECLINED, serde_json::Value::Bool(true))
}

// ============================================================================
// Texts of the first-run wizard
// ============================================================================
//
// All wizard labels (the welcome with the mode choice, progress, summary) and
// the installer's progress and summary lines live here. The window (Rust
// bridge and QML) and the install executor take them from here — there is no
// second copy of the letters (rule #12).
//
// ⭐ Fixes against the Python wizard (`rexon/installer.py`, `QtInstallerWindow`):
//   * the paths `/usr/local/` and `~/.local/` stood in the mode labels — now
//     the mode is named in human terms, the path moved to the tooltip (the
//     `hint` field);
//   * jargon for the user: "Icons/extensions via pkexec", "PySide6 /
//     qt6-webengine / layer-shell-qt must be installed separately",
//     "User-install", "Portable mode", "$PATH — add to ~/.bashrc", "(pkexec)",
//     "try running it from a terminal", the raw dependency install command in
//     the log;
//   * "some features will be unavailable" didn't say which ones
//     ([`without_install_limits`]);
//   * any root script failure was reported as "Installation cancelled by the
//     user" — a password refusal, no way to ask for a password, a timeout and
//     a step failure are now different outcomes with their own advice
//     ([`InstallFailure`]);
//   * «Skip» was not remembered — there is «Don't ask again»
//     ([`KEY_FIRST_RUN_DECLINED`]);
//   * the person wasn't asked about the "no password for Rexon actions" rule —
//     now it is a separate consent, off by default
//     ([`PASSWORDLESS_CONSENT_DEFAULT`]);
//   * the line "Dependencies will be installed together with Rexon" was
//     printed twice — now once ([`dependencies_with_rexon_notice`]).
//
// The `hint` fields are a technical clarification for the tooltip: paths are
// allowed only there. The `details` field is raw output for the developer.
// Guard — `wizard_texts_tests`.
//
// ⭐ On 29.09.2026 the labels moved to the translation catalog: English is the
// key (`msgid`), Russian — verbatim as before in `po/ru.po` (the "as it was"
// snapshot — `tests/golden/installer_texts_ru_before.json`). The language is
// one per process and chosen by the catalog; the `Lang` parameter of the
// functions below no longer decides anything and stays for callers outside
// this pass (the wizard window, the menu, the Python bridge). It will go away
// together with `distro_compat::Lang`.

use crate::distro_compat::Lang;
use crate::i18n::{n_, tr, trf};

/// The "don't ask for a password for Rexon actions" consent is off by default:
/// the permission removes password protection, only the person can turn it on.
pub const PASSWORDLESS_CONSENT_DEFAULT: bool = false;

/// Modes the wizard lets one choose from, in display order. Portable mode is
/// not here: where it is appropriate (package, Steam), the wizard is not
/// offered at all ([`first_run_offer`]).
pub const WIZARD_MODES: [InstallMode; 2] = [InstallMode::System, InstallMode::User];

/// Why the system install is unavailable — in the interface language, or
/// `None`. The decision is the same as in [`system_mode_blocked_reason`].
pub fn system_mode_blocked_reason_in(distro_id: &str, _lang: Lang) -> Option<String> {
    if !has_immutable_system(distro_id) {
        return None;
    }
    Some(tr(
        "On this system (for example, Steam Deck) system folders are \
         write-protected and wiped by every system update. Rexon will be \
         installed into your home folder so that it survives updates.",
    ))
}

/// The install mode name for the person.
pub fn mode_title(mode: InstallMode, _lang: Lang) -> String {
    match mode {
        InstallMode::System => {
            tr("For everyone on this computer")
        }
        InstallMode::User => tr("Just for you"),
        InstallMode::Portable => tr("Standalone folder"),
    }
}

/// What the mode gives and what it costs — one or two phrases under the name.
pub fn mode_description(mode: InstallMode, _lang: Lang) -> String {
    match mode {
        InstallMode::System => tr(
            "Rexon will appear in the application menu for everyone who uses \
             this computer. The administrator password is needed once. Missing \
             components are installed along the way, and Rexon will ask for the \
             password less often when changing the system.",
        ),
        InstallMode::User => tr(
            "Rexon will appear in the application menu for you only. No password \
             is needed, but this option does not install missing components either.",
        ),
        InstallMode::Portable => tr(
            "Rexon runs straight from the folder it is in and installs nothing \
             into the system.",
        ),
    }
}

/// The mode tooltip: where the files will go. Paths come from
/// [`install_paths`], there are no copies of our own here. Portable mode has no
/// paths — an empty string.
pub fn mode_hint(
    mode: InstallMode,
    home: &str,
    xdg_data_home: Option<&str>,
    _lang: Lang,
) -> String {
    let paths = install_paths(mode, home, xdg_data_home);
    match (paths.lib_dir, paths.desktop_dir) {
        // TRANSLATORS: tooltip of an install option; %1 and %2 are folder paths.
        (Some(lib), Some(desktop)) => trf("Program files: %1\nMenu shortcut: %2", &[&lib, &desktop]),
        _ => String::new(),
    }
}

/// One mode on the welcome page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardModeTexts {
    pub mode: InstallMode,
    pub title: String,
    pub description: String,
    /// Where the files will go — for the tooltip.
    pub hint: String,
    /// Why the mode can't be chosen; empty — it can.
    pub blocked: String,
}

/// The welcome page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardWelcomeTexts {
    pub window_title: String,
    pub title: String,
    /// Why install.
    pub description: String,
    /// What installing gives — one item per line.
    pub benefits: Vec<String>,
    pub modes_title: String,
    pub modes: Vec<WizardModeTexts>,
    pub install_button: String,
    pub skip_button: String,
    /// The checkbox next to «Skip»; the key — [`KEY_FIRST_RUN_DECLINED`].
    pub dont_ask_again: String,
    /// What happens if the checkbox is ticked.
    pub dont_ask_again_hint: String,
}

/// The wizard window title.
pub fn wizard_window_title(_lang: Lang) -> String {
    tr("Install Rexon")
}

/// What won't work without an install. This list is the answer of the failure
/// summary and of declining the install — instead of the old "some features
/// will be unavailable".
pub fn without_install_limits(_lang: Lang) -> Vec<String> {
    vec![
        tr(
            "Rexon will not appear in the application menu — you will have to \
             start it from the folder it is in.",
        ),
        tr(
            "The menu will not have the emergency item for restoring the \
             desktop appearance.",
        ),
        tr(
            "Live wallpapers will not work in GNOME and Cinnamon: the add-on \
             they need is set up only by the installation.",
        ),
        tr(
            "Every system change (desktop environments, login and boot screen \
             themes) will ask for the administrator password again.",
        ),
    ]
}

/// The welcome page. `distro_id` decides whether the system install is
/// available; `home` and `xdg_data_home` — only for tooltips with paths.
pub fn welcome_texts(
    distro_id: &str,
    home: &str,
    xdg_data_home: Option<&str>,
    lang: Lang,
) -> WizardWelcomeTexts {
    let modes = WIZARD_MODES
        .iter()
        .map(|&mode| WizardModeTexts {
            mode,
            title: mode_title(mode, lang),
            description: mode_description(mode, lang),
            hint: mode_hint(mode, home, xdg_data_home, lang),
            blocked: match mode {
                InstallMode::System => {
                    system_mode_blocked_reason_in(distro_id, lang).unwrap_or_default()
                }
                _ => String::new(),
            },
        })
        .collect();
    WizardWelcomeTexts {
        window_title: wizard_window_title(lang),
        title: tr("Welcome to Rexon"),
        description: tr(
            "Rexon is a desktop environment manager for Linux. Install it to use \
             it like any other application:",
        ),
        benefits: vec![
            tr(
                "Rexon appears in the application menu, together with an \
                 emergency item for restoring the desktop appearance.",
            ),
            tr("Live wallpapers work in GNOME and Cinnamon too."),
        ],
        modes_title: tr("Install for"),
        modes,
        install_button: tr("Install"),
        skip_button: tr("Skip"),
        dont_ask_again: tr("Don't ask again"),
        dont_ask_again_hint: tr(
            "This window will not appear again. Rexon will keep running from \
             the folder it is in.",
        ),
    }
}

// ── The «Install Rexon on this computer…» button on the «⚙️ Settings» page ──
//
// ⭐ Wehrwolfmann's decision of 15.09.2026: the button sits next to «Take the
// tour again». It clears the «Don't ask again» answer
// ([`KEY_FIRST_RUN_DECLINED`]) and opens the same wizard, so it is shown while
// Rexon is not installed: the [`first_run_offer`] decision WITHOUT taking the
// refusal into account. An installed Rexon (package, Steam, install for
// everyone or for oneself) has no button at all — there would be nothing for
// it to do.

/// The button label on the «⚙️ Settings» page.
pub fn settings_install_button(_lang: Lang) -> String {
    tr("Install Rexon on this computer…")
}

/// The explanation under the button: why to press it.
pub fn settings_install_hint(_lang: Lang) -> String {
    tr("Rexon will appear in the application menu and work like any other program.")
}

/// The button's answer: the install window opened.
pub fn settings_install_started(_lang: Lang) -> String {
    tr("Opening the Rexon installation window.")
}

/// The button's answer when there is nobody to open the install window: the
/// «⚙️ Settings» tab is shown without the Rexon main window. «Take the tour
/// again» answers the same (`qt_settings`, key `ob.no_main_window`).
pub fn settings_install_no_window(_lang: Lang) -> String {
    tr(
        "Installation is available only in the main Rexon window — open the \
         whole window, not a single tab.",
    )
}

/// A separate step "don't ask for a password for Rexon actions".
///
/// The honesty of the text is checked against the rule itself
/// ([`crate::system_plan::passwordless_rules_js`]): without a password — only
/// the listed actions ([`crate::system_plan::PASSWORDLESS_ACTIONS`]), only for
/// the administrators group, only in an active local session. Installing and
/// removing packages (`org.rexon.pkg`) is NOT included. Removed by uninstalling
/// Rexon (`uninstall_system_paths` contains the rule's path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordlessConsentTexts {
    pub title: String,
    /// The checkbox label.
    pub checkbox: String,
    pub description: String,
    /// What exactly is permitted.
    pub what: String,
    /// To whom.
    pub who: String,
    /// What the risk is.
    pub risk: String,
    /// How to undo it.
    pub undo: String,
    /// Where the permission lies — for the tooltip.
    pub hint: String,
    /// The checkbox state when shown — [`PASSWORDLESS_CONSENT_DEFAULT`].
    pub default_on: bool,
}

/// Texts of the "don't ask for a password for Rexon actions" consent. Only the
/// install for all users has this step: the "just for you" install doesn't set
/// the permission.
///
/// ⭐ Fix of 29.09.2026: the menu item label was always inserted into the
/// Russian phrase in the catalog's language ("Uninstall Rexon" in the middle
/// of Russian text with an English interface), and the English phrase didn't
/// name the item at all. Now the phrase is one, from the catalog, and the item
/// in it is in the same language.
pub fn passwordless_consent_texts(_lang: Lang) -> PasswordlessConsentTexts {
    let menu = crate::window_menu::MENU_BUTTON;
    let uninstall_label = crate::window_menu::menu_items()
        .into_iter()
        .find(|i| i.id == crate::window_menu::ID_UNINSTALL)
        .map(|i| i.label)
        .unwrap_or_default();
    PasswordlessConsentTexts {
        title: tr("Don't ask for the password for Rexon actions"),
        checkbox: tr("Allow Rexon to change the system without a password"),
        description: tr(
            "Normally Rexon asks for the administrator password before changing \
             the system. With this permission on, Rexon actions will not ask for \
             the password. The permission is off by default.",
        ),
        // The truth about the permission is told by `system_plan::PASSWORDLESS_ACTIONS`:
        // installing and removing arbitrary programs is not included.
        what: tr(
            "What is allowed: only installing desktop environments from Rexon's \
             list. Login and boot screen themes and installing or removing other \
             software still ask for the password.",
        ),
        who: tr(
            "Who: members of the administrators group, and only while they are \
             sitting at this computer. Remote logins are asked for the password \
             as usual.",
        ),
        risk: tr(
            "The risk: any program running under your account will be able to \
             install desktop environments from Rexon's list through Rexon (and \
             turn on their login screen if the system has none) without your \
             confirmation.",
        ),
        // TRANSLATORS: %1 is the menu button (☰), %2 is the menu item that
        // uninstalls Rexon, both exactly as the window shows them.
        undo: trf(
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            "How to undo: uninstall Rexon from its menu (%1 → “%2”) — the \
             permission is removed together with the application.",
            &[&menu, &uninstall_label],
        ),
        // The group — by distribution family (`system_plan::polkit_admin_groups`);
        // the Debian group name is not put into the tooltip: the jargon guard.
        hint: {
            let arch = crate::system_plan::polkit_admin_groups(PkgFamily::Arch).join(", ");
            // TRANSLATORS: tooltip; %1 is the path of the permission file, %2 the
            // name(s) of the administrators group (e.g. "wheel").
            trf(
                "Permission file: %1\nAdministrators group: %2 (Debian and Ubuntu use their own administrators group)",
                &[&crate::system_plan::REXON_POLKIT_RULES_PATH, &arch],
            )
        },
        default_on: PASSWORDLESS_CONSENT_DEFAULT,
    }
}

/// Explanation of the dry run (`REXON_INSTALL_DRY_RUN`): the wizard is walked
/// through completely, but the machine is not touched.
///
/// ⚠️ Needed on the welcome page: the dry run reaches the «Done — Rexon is
/// installed» summary without installing anything, and without this line the
/// summary would read as a real install. The line lives here, not in the
/// markup, by the same rule as the other wizard labels (rule #12).
pub fn dry_run_notice(_lang: Lang) -> String {
    tr(
        "Trial run: Rexon will walk through the wizard to the end, but will \
         install nothing and change nothing on this computer.",
    )
}

/// The install progress page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardProgressTexts {
    pub title: String,
    pub subtitle: String,
}

/// Texts of the progress page. For the install for all users the subtitle warns
/// about the password dialog in advance — so it doesn't catch one off guard.
pub fn progress_texts(mode: InstallMode, _lang: Lang) -> WizardProgressTexts {
    WizardProgressTexts {
        title: tr("Installing Rexon"),
        subtitle: match mode {
            InstallMode::System => tr(
                "Please wait. When the password window appears, enter the \
                 administrator password.",
            ),
            _ => tr("Please wait."),
        },
    }
}

/// An install step — for the progress log and for explaining a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStep {
    /// Copying the program files (Python: "Copying the rexon/ package...").
    CopyFiles,
    /// The launch command (Python: "Creating the launcher...").
    Launcher,
    /// Menu entries (Python: "Creating the .desktop file...").
    MenuShortcuts,
    /// Icons (Python: "Copying icons (all sizes + SVG)...").
    Icons,
    /// The "fewer passwords" helpers (Python: "Polkit helper (one password per session)...").
    FewerPasswords,
    /// Missing components (Python: "Dependencies: <raw command>").
    Dependencies,
    /// Writing into the system with one password (Python: "Installing into the system (one password)...").
    SystemInstall,
    /// GNOME and Cinnamon wallpaper add-ons (Python: "GNOME + Cinnamon extensions").
    WallpaperAddons,
    /// Refreshing the application menu (Python: caches without a label).
    MenuRefresh,
    /// Settings and backup folders (Python: bare paths).
    UserFolders,
}

impl InstallStep {
    /// All steps in install order.
    pub const ALL: [InstallStep; 10] = [
        InstallStep::CopyFiles,
        InstallStep::Launcher,
        InstallStep::MenuShortcuts,
        InstallStep::Icons,
        InstallStep::FewerPasswords,
        InstallStep::Dependencies,
        InstallStep::SystemInstall,
        InstallStep::WallpaperAddons,
        InstallStep::MenuRefresh,
        InstallStep::UserFolders,
    ];

    /// The step name starting lowercase — for the phrase "Failed: …".
    pub fn name(self, _lang: Lang) -> String {
        match self {
            InstallStep::CopyFiles => {
                tr("preparing the program files")
            }
            InstallStep::Launcher => {
                tr("preparing the start command")
            }
            InstallStep::MenuShortcuts => tr("creating application menu shortcuts"),
            InstallStep::Icons => tr("preparing the icons"),
            InstallStep::FewerPasswords => tr("setting up fewer password prompts"),
            InstallStep::Dependencies => tr("installing missing components"),
            InstallStep::SystemInstall => {
                tr("installing into the system")
            }
            InstallStep::WallpaperAddons => tr("installing live wallpaper add-ons for GNOME and Cinnamon"),
            InstallStep::MenuRefresh => {
                tr("refreshing the application menu")
            }
            InstallStep::UserFolders => tr("creating folders for settings and backups"),
        }
    }

    /// A progress log line: "Preparing program files…".
    pub fn progress_line(self, lang: Lang) -> String {
        let name = self.name(lang);
        let mut chars = name.chars();
        match chars.next() {
            Some(first) => format!("{}{}…", first.to_uppercase(), chars.as_str()),
            None => String::new(),
        }
    }
}

/// There are no icons in the payload (Python: "⚠ Icons not found in the project").
pub fn icons_missing_notice(_lang: Lang) -> String {
    tr("Program icons were not found — Rexon will have a generic icon in the menu.")
}

/// The menu didn't refresh right away (Python: "The entry will appear in the
/// menu after logging in again (<package>)"). The package name goes to the developer log.
pub fn menu_refresh_later_notice(_lang: Lang) -> String {
    tr("The Rexon shortcut will appear in the application menu after you log in again.")
}

/// Missing components will be installed with the same password (Python:
/// "Dependencies will be installed together with Rexon" — printed twice).
pub fn dependencies_with_rexon_notice(_lang: Lang) -> String {
    tr("Missing components will be installed together with Rexon.")
}

/// Portable mode (Python: "Portable mode: no installation required").
pub fn portable_no_install_notice(_lang: Lang) -> String {
    tr("No installation needed: Rexon runs straight from its folder.")
}

/// The `rexon` command won't be found in a terminal (Python: "~/.local/bin is
/// not in $PATH — add it to ~/.bashrc / ~/.zshrc / ~/.config/fish/config.fish").
/// The user doesn't need a terminal at all: the menu is what matters.
pub fn command_path_notice(_lang: Lang) -> String {
    tr(
        "Start Rexon from the application menu: the “rexon” command may not be \
         found in a terminal with this option.",
    )
}

/// Tooltip for [`command_path_notice`]: where the command lies.
pub fn command_path_hint(install_dir: &str, _lang: Lang) -> String {
    // TRANSLATORS: tooltip; %1 is a folder path.
    trf(
        "The command is in %1, and that folder is not on the list of places the \
         terminal searches for programs.",
        &[&install_dir],
    )
}

/// Whether the directory is in the program search list (`PATH`). A trailing
/// slash doesn't prevent a match — Python compared strings as is, and
/// `~/.local/bin/` in `PATH` gave a false "not included".
pub fn command_dir_in_path(dir: &str, path_env: Option<&str>) -> bool {
    let want = dir.trim_end_matches('/');
    !want.is_empty()
        && path_env
            .unwrap_or("")
            .split(':')
            .any(|p| p.trim_end_matches('/') == want)
}

/// The last line of a successful install log (Python: "🎉 Rexon installed
/// successfully!" / "🎉 Rexon installed into ~/.local/ (without root)").
pub fn install_done_line(mode: InstallMode, lang: Lang) -> String {
    match mode {
        InstallMode::System => tr("Rexon is installed for everyone on this computer."),
        InstallMode::User => tr("Rexon is installed just for you."),
        InstallMode::Portable => portable_no_install_notice(lang),
    }
}

/// Why the install didn't finish. Each outcome has its own advice: previously
/// everything merged into "Installation cancelled by the user (pkexec)".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallFailure {
    /// The password dialog was closed or the password didn't fit — nothing changed.
    PasswordDenied,
    /// There is no way to ask for a password — nothing changed.
    PasswordPromptMissing,
    /// The install took longer than allowed and was stopped.
    TimedOut { secs: u64 },
    /// A step failed; `detail` — the output tail for the developer.
    StepFailed { step: InstallStep, detail: String },
}

/// The shared "no way to ask for a password" refusal — in the interface language.
fn prompt_missing_message() -> String {
    crate::system_plan::pkexec_missing_message()
}

/// The instruction from the shared "no way to ask for a password" refusal — the
/// second line of [`crate::system_plan::pkexec_missing_message`] (the package
/// name in it is what must be installed, not an explanation). No copy of the phrase of our own.
fn prompt_missing_instruction() -> String {
    let message = prompt_missing_message();
    message.split_once('\n').map(|(_, instruction)| instruction.to_string()).unwrap_or_default()
}

/// The explanation from the same shared refusal — the first line.
fn prompt_missing_explanation() -> String {
    let message = prompt_missing_message();
    match message.split_once('\n') {
        Some((explanation, _)) => explanation.to_string(),
        None => message,
    }
}

/// A refusal at the "missing parts" step: WHAT didn't install, named to the
/// person in words rather than package names.
///
/// ⭐ Fix of 16.09.2026: previously there was the generic phrase "Failed:
/// installing missing components" here — the person understood neither what
/// was missing nor whether the system had already been touched. The strict
/// line in the script is one ([`installer_exec::deps_script_lines`]), it comes
/// first, and it installs exactly four things — those are what we name. The
/// recommended base follows with `|| true` and can't lead here.
pub fn dependencies_failed_problem(_lang: Lang) -> String {
    tr(
        "The parts Rexon cannot start without could not be installed: the Qt6 \
         framework, the wallpaper layer under the desktop, the video player and \
         the Vulkan loader. Nothing in the system was changed — Rexon was not \
         installed.",
    )
}

impl InstallFailure {
    /// Summary title.
    pub fn title(&self, _lang: Lang) -> String {
        match self {
            InstallFailure::PasswordDenied => {
                tr("Password not confirmed")
            }
            InstallFailure::PasswordPromptMissing => {
                tr("Could not ask for the password")
            }
            InstallFailure::TimedOut { .. } => tr("The installation took too long"),
            InstallFailure::StepFailed { .. } => {
                tr("The installation did not finish")
            }
        }
    }

    /// What went wrong.
    pub fn problem(&self, lang: Lang) -> String {
        match self {
            InstallFailure::PasswordDenied => tr(
                "The password window was closed, or the password did not match. \
                 Nothing was changed.",
            ),
            InstallFailure::PasswordPromptMissing => prompt_missing_explanation(),
            InstallFailure::TimedOut { secs } => {
                let minutes = secs.div_ceil(60).max(1);
                // TRANSLATORS: %1 is a number of minutes.
                trf(
                    "The installation ran longer than %1 min and was stopped. Some \
                     files may already have been installed.",
                    &[&minutes],
                )
            }
            // Missing parts are installed as the FIRST step of the root script,
            // so nothing can be left over yet — and this is the only place
            // where the person can be told what exactly was missing.
            InstallFailure::StepFailed { step: InstallStep::Dependencies, .. } => {
                dependencies_failed_problem(lang)
            }
            // TRANSLATORS: %1 is the name of an installation step in lower
            // case, e.g. "preparing the icons".
            InstallFailure::StepFailed { step, .. } => trf(
                "This step failed: %1. Some changes made before it may remain.",
                &[&step.name(lang)],
            ),
        }
    }

    /// What to do — one piece of advice per line.
    pub fn advice(&self, lang: Lang) -> Vec<String> {
        let just_for_you = mode_title(InstallMode::User, lang);
        match self {
            InstallFailure::PasswordDenied => vec![
                tr("Press “Retry” and enter the administrator password of this computer."),
                // TRANSLATORS: %1 is the install option "Just for you".
                trf(
                    "If you do not have the administrator password, choose “%1” — it \
                     needs no password.",
                    &[&just_for_you],
                ),
            ],
            InstallFailure::PasswordPromptMissing => vec![
                // TRANSLATORS: %1 is the install option "Just for you".
                trf("Choose “%1” — it needs no password.", &[&just_for_you]),
                prompt_missing_instruction(),
            ],
            InstallFailure::TimedOut { .. } => vec![tr(
                "Check your internet connection — missing components are \
                 downloaded — and press “Retry”.",
            )],
            // Missing parts are downloaded from the network, and the first reason
            // for a refusal is not a Rexon breakage but an unreachable software source.
            InstallFailure::StepFailed { step: InstallStep::Dependencies, .. } => vec![
                tr(
                    "Check your internet connection — the missing parts are \
                     downloaded — and press “Retry”.",
                ),
                tr(
                    "If the error happens again, copy the details below and send \
                     them to the developer: these parts may be named differently \
                     in your system.",
                ),
            ],
            InstallFailure::StepFailed { .. } => vec![tr(
                "Press “Retry”. If the error happens again, copy the details below \
                 and send them to the developer.",
            )],
        }
    }

    /// Raw output for the developer; empty — no details.
    pub fn details(&self) -> String {
        match self {
            InstallFailure::StepFailed { detail, .. } => detail.clone(),
            _ => String::new(),
        }
    }

    /// One line for the progress log and for the `(ok, message)` return.
    pub fn log_line(&self, lang: Lang) -> String {
        format!("{}. {}", self.title(lang), self.problem(lang))
    }
}

/// How the install ended — the input for [`result_texts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardOutcome {
    Installed {
        mode: InstallMode,
        /// A system install was asked for but is unavailable — the text of
        /// [`system_mode_blocked_reason_in`]; `None` — the mode wasn't substituted.
        fell_back: Option<String>,
        /// The `rexon` command directory if it is not in `PATH`
        /// ([`command_dir_in_path`]); `None` — all is well.
        command_dir_not_in_path: Option<String>,
    },
    Failed(InstallFailure),
}

/// The summary page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardResultTexts {
    pub ok: bool,
    pub title: String,
    pub description: String,
    /// Advice and caveats — one per line.
    pub notes: Vec<String>,
    /// Technical clarification for the tooltip.
    pub hint: String,
    pub details_title: String,
    /// Raw output for the developer.
    pub details: String,
    pub unavailable_title: String,
    /// What won't work without an install.
    pub unavailable: Vec<String>,
    /// The main button: «Continue» on success, «Retry» on failure.
    pub primary_button: String,
    /// The second button; empty — there is none.
    pub secondary_button: String,
}

/// Label of the main button of the success summary.
///
/// ⭐ Fix against the Python wizard: there the button was called "Launch
/// Rexon" and launched nothing (`rexon/installer.py:1364, 1379`). It has
/// nothing to launch anyway: the wizard runs INSIDE an already running Rexon,
/// and after the install the person returns to the same window. The button is
/// named after what it does — restarting the process is not its job.
pub fn continue_button(_lang: Lang) -> String {
    tr("Continue")
}

/// Label of the «Retry» button of the failure summary.
pub fn retry_button(_lang: Lang) -> String {
    tr("Retry")
}

/// Label of the «Continue without installing» button of the failure summary.
pub fn continue_without_install_button(_lang: Lang) -> String {
    tr("Continue without installing")
}

/// Texts of the summary page.
pub fn result_texts(outcome: &WizardOutcome, lang: Lang) -> WizardResultTexts {
    match outcome {
        WizardOutcome::Installed { mode, fell_back, command_dir_not_in_path } => {
            let mut notes = Vec::new();
            if let Some(reason) = fell_back {
                notes.push(reason.clone());
            }
            let hint = match command_dir_not_in_path {
                Some(dir) => {
                    notes.push(command_path_notice(lang));
                    command_path_hint(dir, lang)
                }
                None => String::new(),
            };
            WizardResultTexts {
                ok: true,
                title: tr("Done — Rexon is installed"),
                description: match mode {
                    InstallMode::System => tr("Rexon is in the application menu for everyone on this computer."),
                    InstallMode::User => tr("Rexon is in the application menu — just for you."),
                    InstallMode::Portable => portable_no_install_notice(lang),
                },
                notes,
                hint,
                details_title: String::new(),
                details: String::new(),
                unavailable_title: String::new(),
                unavailable: Vec::new(),
                primary_button: continue_button(lang),
                secondary_button: String::new(),
            }
        }
        WizardOutcome::Failed(failure) => {
            let details = failure.details();
            WizardResultTexts {
                ok: false,
                title: failure.title(lang),
                description: failure.problem(lang),
                notes: failure.advice(lang),
                hint: String::new(),
                details_title: if details.is_empty() {
                    String::new()
                } else {
                    tr("Details for the developer")
                },
                details,
                unavailable_title: tr("Without installing:"),
                unavailable: without_install_limits(lang),
                primary_button: retry_button(lang),
                secondary_button: continue_without_install_button(lang),
            }
        }
    }
}

// ── Uninstalling Rexon: progress and summary ────────────────────────────────
//
// These are the words of the uninstall executor
// ([`crate::installer_exec::uninstall`]) and of the window's «Uninstall Rexon» menu.
// ⭐ Fix G8: previously the window showed "Uninstall error system: code 126: …" —
// a password refusal, no way to ask for a password, a timeout and a failure
// of the removal itself in one line with jargon.

/// A part of the uninstall — for the progress line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallPart {
    /// Distribution package.
    Package,
    /// Install for all users.
    System,
    /// Install just for oneself.
    User,
    /// Temporary files (cache) — with any choice.
    Cache,
    /// Settings, backups and installed content — only with
    /// «Remove Everything».
    Profile,
}

impl UninstallPart {
    pub const ALL: [UninstallPart; 5] = [
        UninstallPart::Package,
        UninstallPart::System,
        UninstallPart::User,
        UninstallPart::Cache,
        UninstallPart::Profile,
    ];

    /// A progress log line.
    pub fn progress_line(self, _lang: Lang) -> String {
        match self {
            UninstallPart::Package => {
                tr("Removing the Rexon package…")
            }
            UninstallPart::System => tr("Removing Rexon for everyone on this computer…"),
            UninstallPart::User => tr("Removing Rexon from your home folder…"),
            UninstallPart::Cache => tr("Removing Rexon's temporary files…"),
            UninstallPart::Profile => tr("Removing settings, backups and installed wallpapers, themes and add-ons…"),
        }
    }
}

// ── Rexon user data ─────────────────────────────────────────────────────────
//
// ⭐ Wehrwolfmann's decision of 15.09.2026. One truth about WHAT Rexon writes
// into the person's home folder besides the program itself — for uninstall and
// for creating the profile folders at install time
// ([`crate::installer_exec::profile_dirs`]). Previously uninstall knew only
// the settings and backups: the cache and installed wallpapers and themes
// remained after any choice.
//
// Who writes into these roots (checked by grepping the core on 15.09.2026):
//   * cache `<XDG_CACHE_HOME|~/.cache>/rexon` — `youtube_wallpaper::cache_dir`,
//     `session_splash::preview_cache_dir`, `qt_steam::rexon_cache_dir`,
//     `qml_modules::net_cache_dir`, the blur frames of `wallpaper_exec`; the
//     LITERAL `~/.cache/rexon` bypassing XDG — `appearance_cache::cache_dir`
//     (OCS), `installed_scan` (previews), the fallback wallpaper PID file of
//     `wallpaper_exec`;
//   * data `<XDG_DATA_HOME|~/.local/share>/rexon` — adaptive wallpapers
//     (`wallpapers/`), desktop backups (`backups/`), `qml_modules::module_dirs`
//     (`qml_modules/`, the developer build — NOT part of the install: the
//     install puts modules into `~/.local/lib/rexon`); the LITERAL
//     `~/.local/share/rexon` bypassing XDG — `de_customization`
//     (`login-themes/`, `plymouth-themes/`, `bundles/`, `wallpapers/`),
//     `upscaler::esrgan_local_exe` (`bin/realesrgan`);
//   * settings `<XDG_CONFIG_HOME|~/.config>/rexon`; the literal
//     `~/.config/rexon` is needed for installs made BEFORE fix 8a45776: before
//     it the bridge and screenshots wrote settings to the literal path even
//     with `XDG_CONFIG_HOME` set, and without the second root they would
//     survive «Remove Everything».
// So the roots of all three categories are taken BOTH — per XDG and literal,
// if they differ.

/// Name of the desktop backups folder inside the Rexon data root.
pub const USER_DATA_BACKUPS_DIR: &str = "backups";

/// What the person has: for the question dialog, progress lines and deciding
/// which choice removes what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDataKind {
    /// Rexon settings (the installed themes journal is in the same folder).
    Settings,
    /// Desktop backups.
    Backups,
    /// Installed wallpapers, themes and add-ons — everything in the Rexon data
    /// root except backups.
    Content,
    /// Temporary files (cache).
    Cache,
}

impl UserDataKind {
    pub const ALL: [UserDataKind; 4] =
        [UserDataKind::Cache, UserDataKind::Settings, UserDataKind::Backups, UserDataKind::Content];

    /// Whether it is removed even with «Uninstall but Keep Settings and
    /// Backups». The cache — yes: without the program nobody needs it.
    pub fn removed_when_keeping(self) -> bool {
        matches!(self, UserDataKind::Cache)
    }

    /// The name for the person — from the translation catalog, in the interface language.
    pub fn title(self) -> String {
        match self {
            UserDataKind::Settings => tr("Rexon settings"),
            UserDataKind::Backups => {
                tr("Desktop backups")
            }
            UserDataKind::Content => tr("Installed wallpapers, themes and add-ons"),
            UserDataKind::Cache => tr("Rexon's temporary files"),
        }
    }
}

/// Rexon user data roots by category, without repeats, in [`UserDataKind::ALL`]
/// order, and within each category: first the XDG path, then the literal one.
/// `Content` is the data ROOT; its content except [`USER_DATA_BACKUPS_DIR`]
/// is expanded by the executor itself. `Backups` — `<data root>/backups` of
/// each root.
///
/// Empty if the home folder isn't absolute: a relative path would look into
/// the process's current directory. A relative `XDG_*` value is ignored per
/// the spec.
pub fn user_data_roots(
    home: &str,
    xdg_config_home: &str,
    xdg_data_home: &str,
    xdg_cache_home: &str,
) -> Vec<(UserDataKind, String)> {
    if !home.starts_with('/') {
        return Vec::new();
    }
    fn xdg(v: &str) -> Option<&str> {
        Some(v).filter(|v| v.starts_with('/'))
    }
    let data: Vec<String> = [
        crate::utils::data_home(xdg(xdg_data_home), home),
        crate::utils::data_home(None, home),
    ]
    .into_iter()
    .map(|base| format!("{base}/rexon"))
    .collect();
    let config: Vec<String> = [
        crate::utils::config_home(xdg(xdg_config_home), home),
        crate::utils::config_home(None, home),
    ]
    .into_iter()
    .map(|base| format!("{base}/rexon"))
    .collect();
    let cache: Vec<String> = [
        crate::utils::cache_home(xdg(xdg_cache_home), home),
        crate::utils::cache_home(None, home),
    ]
    .into_iter()
    .map(|base| format!("{base}/rexon"))
    .collect();
    let mut out: Vec<(UserDataKind, String)> = Vec::new();
    let mut push = |kind: UserDataKind, path: String| {
        if !out.iter().any(|(_, p)| *p == path) {
            out.push((kind, path));
        }
    };
    for p in cache {
        push(UserDataKind::Cache, p);
    }
    for p in config {
        push(UserDataKind::Settings, p);
    }
    for root in &data {
        push(UserDataKind::Backups, format!("{root}/{USER_DATA_BACKUPS_DIR}"));
    }
    for root in data {
        push(UserDataKind::Content, root);
    }
    out
}

/// The summary of a successful uninstall. `keep_profile` — the «Uninstall but
/// Keep Settings and Backups» option was chosen: the summary names EVERYTHING
/// kept, says why it was kept, and that temporary files were removed here too.
pub fn uninstall_done_line(keep_profile: bool, _lang: Lang) -> String {
    if keep_profile {
        tr(
            "Rexon has been removed. Your settings, desktop backups and installed \
             wallpapers, themes and add-ons were kept: if you install Rexon again, it \
             will pick up where you left off. Rexon's temporary files were deleted.",
        )
    } else {
        tr(
            "Rexon has been removed completely, together with its settings, desktop \
             backups, installed wallpapers, themes, add-ons and temporary files.",
        )
    }
}

/// In place of the temporary files or installed content there was a link to
/// another folder: the link itself was removed, Rexon didn't follow it. The
/// path is needed in the text: the files on the far side of the link remained.
pub fn uninstall_removed_link_note(path: &str, _lang: Lang) -> String {
    // TRANSLATORS: %1 is a folder path.
    trf(
        "%1 was a link to another folder. Rexon removed only the link and did not \
         touch what it pointed to.",
        &[&path],
    )
}

/// Nothing to remove: no package, no copy for everyone, no copy for oneself.
pub fn uninstall_nothing_line(_lang: Lang) -> String {
    tr("Rexon is not installed — there is nothing to remove. Nothing was changed.")
}

/// A path from the removal list turned out to be a link to another folder (for
/// the developer the program folder leads into the working tree) — it was left
/// alone. The path is needed in the text: otherwise the person won't learn what remained.
pub fn uninstall_kept_link_note(path: &str, _lang: Lang) -> String {
    // TRANSLATORS: %1 is a folder path.
    trf(
        "%1 is a link to another folder. Rexon left it alone: neither the link nor \
         what it points to was touched.",
        &[&path],
    )
}

/// Rexon was installed by a package Rexon doesn't know how to remove. The
/// refusal comes before the first side effect.
pub fn uninstall_package_unsupported_line(name: &str, _lang: Lang) -> String {
    // TRANSLATORS: %1 is the name of the distribution package.
    trf(
        "Rexon was installed from the “%1” package, which Rexon cannot remove by \
         itself. Remove it with your system's software installer. Nothing was changed.",
        &[&name],
    )
}

/// The summary of a failed uninstall. Outcome parsing is shared with the
/// install ([`crate::installer_exec::classify_script_run`]); the words are its
/// own: the [`InstallFailure`] titles speak about installing. Raw output goes
/// as a tail, under a heading for the developer.
pub fn uninstall_failure_line(failure: &InstallFailure, _lang: Lang) -> String {
    let problem = match failure {
        InstallFailure::PasswordDenied => tr(
            "The password window was closed, or the password did not match — removal \
             was stopped.",
        ),
        InstallFailure::PasswordPromptMissing => prompt_missing_message(),
        InstallFailure::TimedOut { secs } => {
            let minutes = secs.div_ceil(60).max(1);
            // TRANSLATORS: %1 is a number of minutes.
            trf(
                "The removal ran longer than %1 min and was stopped. Some Rexon files \
                 may remain — try removing it again.",
                &[&minutes],
            )
        }
        InstallFailure::StepFailed { .. } => tr(
            "Rexon could not be removed completely: some files may remain. Try \
             removing it again; if that does not help, tell the developer.",
        ),
    };
    let details = failure.details();
    if details.is_empty() {
        problem
    } else {
        format!(
            "{problem}\n\n{}:\n{details}",
            tr("Details for the developer")
        )
    }
}

// ============================================================================
// Runtime dependencies per family (port of _get_dep_packages /
// _get_dep_install_cmd)
// ============================================================================

/// Packages without which the Rexon payload won't start AT ALL, for a family.
/// Fallback — DEBIAN (like `dep_map.get(family, dep_map.get(DEBIAN, []))` in
/// Python).
///
/// ⭐ Fix of 16.09.2026 (rule #16): the list was about PySide6, python-cairo
/// and the classic Python window — things the payload no longer carries. On
/// the user's machine it installed an unneeded interpreter with bindings and
/// at the same time did NOT install a single library the Rust window lives on.
/// By then the "what is missing" probe was already asking about the same thing
/// this list now talks about: the dynamic linker over the payload's own files
/// (`installer_exec::runtime_deps_present`).
///
/// ⭐⭐ A fix the same day, second pass: the list was split in two. The required
/// line goes in the root script under `set -e`, and ONE wrong name aborted the
/// whole install. The names are written out by each family's naming rules,
/// while only Arch was built live (`packaging/aur/PKGBUILD`) — so the cost of a
/// mistake must depend on the package tier:
///   * here — what the window won't open without: Qt6 (base, QML, web, audio,
///     websockets, gRPC), layer-shell, the Vulkan loader (Flash wallpapers) and
///     libmpv (the login splash). If it doesn't install, the install honestly
///     refuses and names what was missing ([`InstallStep::Dependencies`]);
///   * in [`optional_dep_packages`] — the base almost always present on a live
///     machine with graphics (bash, D-Bus, Wayland, GL, OpenSSL, polkit, an icon
///     theme). It goes on a separate line with `|| true`, exactly like the Qt
///     style switcher ([`qt_style_install_cmd`]): a typo in a name there costs
///     an unneeded log line, not a broken install.
///
/// The two tiers together are the same set as the distribution package's
/// `depends` (`packaging/aur/PKGBUILD`), without `glibc` and `gcc-libs`: that
/// is the base without which the package manager itself won't run. `qt6ct` is
/// not in the set and never was — it is not required (without it a Kvantum
/// theme applies via `QT_STYLE_OVERRIDE`, only without fonts and icons).
///
/// The order inside an entry is the same for all families and reads top down:
/// Qt6 (base, QML, web, audio, websockets, gRPC), layer-shell, the Vulkan
/// loader, libmpv.
///
/// ⚠️ The names are written out by each family's naming rules; only the Arch
/// family was verified live. That is why the tier is split: a mistake in a
/// name here is visible to the person right away instead of turning into a
/// silently half-installed product.
pub fn dep_packages(family: PkgFamily) -> Vec<String> {
    let list: &[&str] = match family {
        PkgFamily::Arch => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "qt6-grpc", "layer-shell-qt", "vulkan-icd-loader",
            "mpv",
        ],
        PkgFamily::Fedora => &[
            "qt6-qtbase-gui", "qt6-qtdeclarative", "qt6-qtwebengine",
            "qt6-qtmultimedia", "qt6-qtwebsockets", "qt6-qtgrpc",
            "layer-shell-qt", "vulkan-loader", "mpv-libs",
        ],
        PkgFamily::Suse => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "qt6-grpc", "layer-shell-qt", "libvulkan1",
            "libmpv2",
        ],
        PkgFamily::Gentoo => &[
            "dev-qt/qtbase", "dev-qt/qtdeclarative", "dev-qt/qtwebengine",
            "dev-qt/qtmultimedia", "dev-qt/qtwebsockets", "dev-qt/qtgrpc",
            "kde-frameworks/layer-shell-qt", "media-libs/vulkan-loader",
            "media-video/mpv",
        ],
        PkgFamily::Alpine => &[
            "qt6-qtbase", "qt6-qtdeclarative", "qt6-qtwebengine",
            "qt6-qtmultimedia", "qt6-qtwebsockets", "qt6-qtgrpc",
            "layer-shell-qt", "vulkan-loader", "mpv-libs",
        ],
        PkgFamily::Void => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "qt6-grpc", "layer-shell-qt", "Vulkan-Loader",
            "mpv",
        ],
        PkgFamily::Solus => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "layer-shell-qt", "vulkan", "mpv",
        ],
        // DEBIAN + everything not in dep_map (nix/clear/alt/unknown) → DEBIAN.
        // Debian and Ubuntu split Qt6 into libraries and QML modules separately,
        // so the names here don't look like the other families'.
        _ => &[
            "qt6-qpa-plugins", "qt6-wayland", "qml6-module-qtquick-controls",
            "qml6-module-qtwebengine", "qml6-module-qtmultimedia",
            "libqt6websockets6", "libqt6grpc6",
            "qml6-module-org-kde-layershell", "libvulkan1", "libmpv2",
        ],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// RECOMMENDED packages: Rexon doesn't work without them either, but on a
/// machine that has a desktop at all they are already installed — everything
/// else pulls them in. Fallback — DEBIAN, as in [`dep_packages`].
///
/// Why a separate tier: the names in eleven families out of twelve are written
/// out by naming rules rather than verified on a live machine, and ONE wrong
/// name in a line under `set -e` aborted the install for a person whom the
/// product promised "just press Install". Collected here is what must not be
/// paid for at such a price: bash, D-Bus, Wayland, GL, OpenSSL, polkit and an
/// icon theme. The executor appends `|| true` to this line
/// ([`installer_exec::deps_script_lines`]) — if it doesn't install, fine: if
/// something of this is really missing on the machine, the window won't open,
/// and the person will see it right away rather than instead of the install.
///
/// The order is the same for all families: Wayland, GL, D-Bus, OpenSSL,
/// polkit, bash, icon theme. On Debian `pkexec` takes polkit's place — the
/// policy arrives there as a separate package.
pub fn optional_dep_packages(family: PkgFamily) -> Vec<String> {
    let list: &[&str] = match family {
        PkgFamily::Arch => &[
            "wayland", "libglvnd", "dbus", "openssl", "polkit", "bash",
            "hicolor-icon-theme",
        ],
        PkgFamily::Fedora => &[
            "libwayland-client", "libglvnd-egl", "dbus-libs", "openssl-libs",
            "polkit", "bash", "hicolor-icon-theme",
        ],
        PkgFamily::Suse => &[
            "libwayland-client0", "libglvnd", "dbus-1", "libopenssl3",
            "polkit", "bash", "hicolor-icon-theme",
        ],
        PkgFamily::Gentoo => &[
            "dev-libs/wayland", "media-libs/libglvnd", "sys-apps/dbus",
            "dev-libs/openssl", "sys-auth/polkit", "app-shells/bash",
            "x11-themes/hicolor-icon-theme",
        ],
        PkgFamily::Alpine => &[
            "wayland-libs-client", "libglvnd", "dbus", "openssl", "polkit",
            "bash", "hicolor-icon-theme",
        ],
        PkgFamily::Void => &[
            "wayland", "libglvnd", "dbus", "openssl", "polkit", "bash",
            "hicolor-icon-theme",
        ],
        PkgFamily::Solus => &[
            "wayland", "libglvnd", "dbus", "openssl", "polkit", "bash",
            "hicolor-icon-theme",
        ],
        _ => &[
            "libwayland-client0", "libglvnd0", "dbus", "openssl", "pkexec",
            "bash", "hicolor-icon-theme",
        ],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// The command installing an arbitrary set of packages **as a line for the
/// install root script**. Empty — nothing to install with (see the three cases below).
///
/// One formatter for everything this script installs (rule #12): both the
/// required dependencies and the extra install of the Qt style switcher.
///
/// There is NO command table of our own here: the commands themselves come
/// from the engine ([`crate::system_plan::install_cmd`]) — the same one Rexon
/// uses to install packages everywhere else. This point differs from the
/// others in exactly one thing: the engine returns argv, while a bash line is
/// needed here, so every word goes through `shlex_quote` (the package name
/// comes from the tables, but the escaping is not "just in case": the line
/// goes into a script run as root).
///
/// ⚠️ `pkexec` is NOT prepended, and that is not forgetfulness: the whole root
/// script already runs under one `pkexec` (see [`system_install_script`]), a
/// second one inside would be a nested authorisation. So the bare
/// [`crate::system_plan::install_cmd`] is taken, not `install_argv`, which
/// chooses the privileged path.
///
/// Empty is returned in three cases:
/// * the package list is empty — "install nothing" can't be expressed as a
///   command (the same rule as `install_argv`);
/// * the `UNKNOWN` family — the manager isn't recognised, the engine answers with an error;
/// * **NixOS** — the only divergence from the engine, and a deliberate one.
///   The engine knows `nix-env -iA nixpkgs.<package>` and deliberately does NOT
///   put `pkexec` before it: that command fills the profile of THE user who ran
///   it. Here the one running it is root, because the whole script goes under
///   `pkexec`, and there is nothing to drop privileges with inside. So the
///   command would succeed and put the libraries into root's profile — the
///   user would see neither an error nor the packages. A silent dud is worse
///   than an honest "nothing to install with": in NixOS packages are installed
///   by editing the system configuration. The same fact about the user profile
///   as in the engine points the other way in the root script.
pub fn pkg_install_cmd(family: PkgFamily, packages: &[String]) -> String {
    if packages.is_empty() || family == PkgFamily::Nix {
        return String::new();
    }
    match crate::system_plan::install_cmd(family, packages) {
        // A bash line from argv: each word escaped separately. The table heads
        // (`pacman`, `-S`, `--needed`, `--ask=n`…) consist of safe characters, so
        // they get no quotes and the command's look doesn't change — escaping kicks
        // in only on package names.
        Ok(argv) => argv
            .iter()
            .map(|word| crate::ext_manager::shlex_quote(word))
            .collect::<Vec<String>>()
            .join(" "),
        // UNKNOWN: the engine answers "Package manager not detected".
        Err(_) => String::new(),
    }
}

/// The command installing the REQUIRED runtime dependencies for a family. In
/// the root script this line goes strictly, under `set -e`.
///
/// Empty — per the [`pkg_install_cmd`] rules: an empty package list, `UNKNOWN`
/// or NixOS (there the root script would fill root's profile).
pub fn dep_install_cmd(family: PkgFamily) -> String {
    pkg_install_cmd(family, &dep_packages(family))
}

/// The command installing the RECOMMENDED dependencies ([`optional_dep_packages`]).
///
/// A separate command, not the tail of the previous one: the executor appends
/// `|| true` to it ([`installer_exec::deps_script_lines`]), and a wrong package
/// name in this tier doesn't abort the install. Empty — per the same three
/// rules of [`pkg_install_cmd`].
pub fn optional_dep_install_cmd(family: PkgFamily) -> String {
    pkg_install_cmd(family, &optional_dep_packages(family))
}

/// The command installing the Qt style switcher (`qt6ct`) in THE SAME pass as
/// Rexon itself.
///
/// Why separately from [`dep_install_cmd`]: Rexon works without it — a Kvantum
/// theme outside KDE and LXQt applies via `QT_STYLE_OVERRIDE`, only without
/// fonts and icons. So an unreachable repository must not abort the whole
/// install (`set -e` in the root script), and the executor appends `|| true`
/// to this line. The required dependencies stay strict.
///
/// Empty — nothing to install with: in NixOS packages are installed by editing
/// the configuration, Clear Linux has no bundle.
pub fn qt_style_install_cmd(family: PkgFamily) -> String {
    match crate::themes::qt_style_helper_pkg(family.as_str()) {
        Some(pkg) => pkg_install_cmd(family, &[pkg]),
        None => String::new(),
    }
}

// ============================================================================
// Generating the install script (system mode)
// ============================================================================

/// The icon set prepared by the executor in temporary files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IconSet {
    /// (size, tmp path) for existing PNGs — order 16..512.
    pub sizes: Vec<(u32, String)>,
    /// tmp path to the SVG (scalable), if any.
    pub svg: Option<String>,
    /// tmp path to the master copy (rexon_icon.png) — only if there is no 128/256/512.
    pub master: Option<String>,
}

impl IconSet {
    fn size(&self, s: u32) -> Option<&String> {
        self.sizes.iter().find(|(sz, _)| *sz == s).map(|(_, p)| p)
    }

    /// Source for /usr/share/pixmaps/rexon.png: 128 → 256 → 512 → master.
    fn pixmap_src(&self) -> Option<&String> {
        self.size(128)
            .or_else(|| self.size(256))
            .or_else(|| self.size(512))
            .or(self.master.as_ref())
    }

    /// All tmp paths (for the cleanup list) in Python dict insertion order:
    /// sizes, then svg, then master.
    fn tmp_paths(&self) -> Vec<String> {
        let mut out: Vec<String> =
            self.sizes.iter().map(|(_, p)| p.clone()).collect();
        if let Some(s) = &self.svg {
            out.push(s.clone());
        }
        if let Some(m) = &self.master {
            out.push(m.clone());
        }
        out
    }
}

/// Where each icon goes: pairs (source, destination) under `icon_root`
/// (hicolor) and `pixmap_dir`. The rule is ONE for the system install
/// ([`icon_install_block`]) and the home folder install (`installer_exec`).
///
/// ⭐ Fixed in the port: the Python home folder install took the pixmaps icon
/// only from `rexon_icon_128.png` and didn't know the 256/512 fallbacks and the
/// master copy that the system install knew.
pub fn icon_targets(icons: &IconSet, icon_root: &str, pixmap_dir: &str) -> Vec<(String, String)> {
    let root = icon_root.trim_end_matches('/');
    let mut out: Vec<(String, String)> = Vec::new();
    for (size, src) in &icons.sizes {
        out.push((src.clone(), format!("{root}/{size}x{size}/apps/rexon.png")));
    }
    if let Some(svg) = &icons.svg {
        out.push((svg.clone(), format!("{root}/scalable/apps/rexon.svg")));
    }
    if let Some(px) = icons.pixmap_src() {
        out.push((px.clone(), format!("{}/rexon.png", pixmap_dir.trim_end_matches('/'))));
    }
    out
}

/// The `install -Dm644 …` block for icons — port of building `icon_install_cmds`.
pub fn icon_install_block(icons: &IconSet) -> String {
    let cmds: Vec<String> = icon_targets(icons, "/usr/share/icons/hicolor", "/usr/share/pixmaps")
        .into_iter()
        .map(|(src, dst)| format!("install -Dm644 {src} {dst}"))
        .collect();
    if cmds.is_empty() {
        "# Icons not found".to_string()
    } else {
        cmds.join("\n")
    }
}

/// Paths of the nine helper/policy/rules tmp files (created by the executor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperTmp {
    pub tmp_helper: String,
    pub tmp_policy: String,
    pub tmp_de_helper: String,
    pub tmp_de_policy: String,
    pub tmp_pkg_helper: String,
    pub tmp_pkg_policy: String,
    pub tmp_stage_helper: String,
    pub tmp_stage_policy: String,
    pub tmp_de_rules: String,
}

/// The block installing the privileged helpers + polkit policies — port of
/// `helper_install_block`.
pub fn helper_install_block(h: &HelperTmp) -> String {
    format!(
        "install -Dm755 {th} {SYSTEM_HELPER_PATH}\n\
         install -Dm644 {tp} {SYSTEM_POLICY_PATH}\n\
         install -Dm755 {tdh} {DE_HELPER_PATH}\n\
         install -Dm644 {tdp} {DE_POLICY_PATH}\n\
         install -Dm755 {tph} {PKG_HELPER_PATH}\n\
         install -Dm644 {tpp} {PKG_POLICY_PATH}\n\
         install -Dm755 {tsh} {STAGE_HELPER_PATH}\n\
         install -Dm644 {tsp} {STAGE_POLICY_PATH}",
        th = h.tmp_helper,
        tp = h.tmp_policy,
        tdh = h.tmp_de_helper,
        tdp = h.tmp_de_policy,
        tph = h.tmp_pkg_helper,
        tpp = h.tmp_pkg_policy,
        tsh = h.tmp_stage_helper,
        tsp = h.tmp_stage_policy,
    )
}

/// The "no repeated password" rule step in the root script.
///
/// ⭐⭐ Fix of 15.09.2026: the rule was installed ALWAYS and silently. Now —
/// only with the person's consent ([`PASSWORDLESS_CONSENT_DEFAULT`] = off).
/// Without consent an old rule installed silently is removed: otherwise a
/// reinstall would leave in the system a permission nobody asked the person about.
pub fn passwordless_rule_block(tmp_rules: &str, consent: bool) -> String {
    if consent {
        format!("install -Dm644 {tmp_rules} {REXON_POLKIT_RULES_PATH}")
    } else {
        format!(
            "# The «passwordless» rule is installed only with consent; a previous one, installed silently, is removed.\n\
             rm -f {REXON_POLKIT_RULES_PATH}"
        )
    }
}

/// Parameters of the system-mode install script generation. The tmp file paths
/// come from the executor (mkstemp/mkdtemp), so the text is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallScriptParams {
    pub lib_dir: String,
    pub install_dir: String,
    pub desktop_dir: String,
    pub tmp_pkg: String,
    pub tmp_launcher: String,
    pub tmp_desktop: String,
    pub tmp_recover: String,
    pub icons: IconSet,
    pub helpers: HelperTmp,
    /// Dependency install command ("" → "Dependencies already installed").
    pub dep_cmd: String,
    /// The person agreed to the "no repeated password" rule
    /// ([`passwordless_rule_block`]). Default — no.
    pub passwordless_rule: bool,
}

/// The full text of the system install bash script — port of the
/// `install_script` f-string in `install_to_system`.
pub fn system_install_script(p: &InstallScriptParams) -> String {
    let dep_line = if p.dep_cmd.is_empty() {
        "# Dependencies already installed".to_string()
    } else {
        format!("# Installing dependencies\n{}", p.dep_cmd)
    };
    let icon_block = icon_install_block(&p.icons);
    let helper_block = format!(
        "{}\n{}",
        helper_install_block(&p.helpers),
        passwordless_rule_block(&p.helpers.tmp_de_rules, p.passwordless_rule)
    );
    format!(
        "#!/bin/bash\n\
         set -e\n\
         {dep_line}\n\
         rm -rf {lib}\n\
         mkdir -p {lib}\n\
         cp -r {tmp_pkg}/* {lib}/\n\
         chmod -R 755 {lib}\n\
         install -Dm755 {tmp_launcher} {install_dir}/rexon\n\
         install -Dm644 {tmp_desktop} {desktop_dir}/rexon.desktop\n\
         install -Dm644 {tmp_recover} {desktop_dir}/rexon-recover.desktop\n\
         {icon_block}\n\
         {helper_block}\n\
         update-desktop-database {desktop_dir} 2>/dev/null || true\n\
         gtk4-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || \\\n\
         \x20 gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true\n\
         # KDE SyCoCa rebuild (so the panel/menu see the icon right away)\n\
         ( kbuildsycoca6 --noincremental 2>/dev/null || \\\n\
         \x20 kbuildsycoca5 --noincremental 2>/dev/null ) || true\n\
         \n\
         # Wehrwolfmann: GNOME Shell + Cinnamon extensions for the wallpaper mode.\n\
         # Mutter (GNOME) and Muffin (Cinnamon) do not support wlr-layer-shell —\n\
         # an extension through Clutter.Actor + mmap streaming. Always installed without\n\
         # the user's choice (in case of switching to GNOME/Cinnamon-Wayland).\n\
         for de_pair in \"gnome-extension|/usr/share/gnome-shell/extensions\" \\\n\
         \x20              \"cinnamon-extension|/usr/share/cinnamon/extensions\"; do\n\
         \x20   DE_DIR=\"${{de_pair%|*}}\"\n\
         \x20   DEST_BASE=\"${{de_pair#*|}}\"\n\
         \x20   EXT_SRC=\"{lib}/$DE_DIR/{ext}\"\n\
         \x20   EXT_DEST=\"$DEST_BASE/{ext}\"\n\
         \x20   # The extension used to be called {ext_legacy} (a domain of\n\
         \x20   # another project). The old directory is removed: two extensions with the same\n\
         \x20   # code would draw the wallpaper twice.\n\
         \x20   rm -rf \"$DEST_BASE/{ext_legacy}\"\n\
         \x20   if [ -d \"$EXT_SRC\" ]; then\n\
         \x20       rm -rf \"$EXT_DEST\"\n\
         \x20       mkdir -p \"$(dirname \"$EXT_DEST\")\"\n\
         \x20       cp -r \"$EXT_SRC\" \"$EXT_DEST\"\n\
         \x20       chmod -R 755 \"$EXT_DEST\"\n\
         \x20       if command -v glib-compile-schemas >/dev/null 2>&1 \\\n\
         \x20           && [ -d \"$EXT_DEST/schemas\" ]; then\n\
         \x20           glib-compile-schemas \"$EXT_DEST/schemas/\" 2>/dev/null || true\n\
         \x20       fi\n\
         \x20   fi\n\
         done\n",
        dep_line = dep_line,
        lib = p.lib_dir,
        tmp_pkg = p.tmp_pkg,
        tmp_launcher = p.tmp_launcher,
        install_dir = p.install_dir,
        tmp_desktop = p.tmp_desktop,
        desktop_dir = p.desktop_dir,
        tmp_recover = p.tmp_recover,
        icon_block = icon_block,
        helper_block = helper_block,
        ext = EXT_ID,
        ext_legacy = EXT_ID_LEGACY,
    )
}

/// The command running the install script: `pkexec bash {tmp_sh}`, timeout
/// [`SYSTEM_INSTALL_TIMEOUT_SECS`].
pub fn system_install_cmd(tmp_sh: &str) -> Cmd {
    Cmd {
        argv: vec!["pkexec".into(), "bash".into(), tmp_sh.to_string()],
        timeout_secs: Some(SYSTEM_INSTALL_TIMEOUT_SECS),
        lang_c: false,
    }
}

/// The system install plan: write the script → pkexec → clean up the temporary
/// files. The helper/policy/desktop/launcher texts are written by the executor
/// from the corresponding pub functions (here — only the wrapper script and argv).
pub fn system_install_plan(p: &InstallScriptParams, tmp_sh: &str) -> Vec<PlanStep> {
    vec![
        PlanStep::WriteFile {
            path: tmp_sh.to_string(),
            content: system_install_script(p),
            mode: 0o755,
        },
        PlanStep::Run {
            id: RunStepId::AppInstall,
            cmd: system_install_cmd(tmp_sh),
            needs_root: true,
            description: "System install (one password)".into(),
        },
        PlanStep::RemoveFiles {
            paths: install_cleanup_paths(p, tmp_sh),
        },
    ]
}

/// The list of temporary paths to clean up after the install.
///
/// ⚠ Fixed a bug of the original (rule #16): Python `cleanup_paths` did NOT
/// include `tmp_stage_helper` / `tmp_stage_policy` (both were created via
/// mkstemp and remained in /tmp as junk after every install). The port adds
/// them (after pkg-policy, mirroring the stage helper's install order).
pub fn install_cleanup_paths(p: &InstallScriptParams, tmp_sh: &str) -> Vec<String> {
    let h = &p.helpers;
    let mut out = vec![
        p.tmp_pkg.clone(),
        tmp_sh.to_string(),
        p.tmp_launcher.clone(),
        p.tmp_desktop.clone(),
        p.tmp_recover.clone(),
        h.tmp_helper.clone(),
        h.tmp_policy.clone(),
        h.tmp_de_helper.clone(),
        h.tmp_de_policy.clone(),
        h.tmp_de_rules.clone(),
        h.tmp_pkg_helper.clone(),
        h.tmp_pkg_policy.clone(),
        // ── leak fix: two stage files forgotten in the original ──
        h.tmp_stage_helper.clone(),
        h.tmp_stage_policy.clone(),
    ];
    out.extend(p.icons.tmp_paths());
    out
}

// ============================================================================
// Uninstall
// ============================================================================

/// Arguments for "which package owns the file" — port of `_DISTRO_PKG_OWNER`.
/// None — a family without owner detection support (manual/portable).
pub fn distro_pkg_owner(family: PkgFamily) -> Option<Vec<String>> {
    let v: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Qoq"],
        PkgFamily::Debian => &["dpkg", "-S"],
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => {
            &["rpm", "-qf", "--qf", "%{NAME}"]
        }
        _ => return None,
    };
    Some(v.iter().map(|s| s.to_string()).collect())
}

/// The command removing one package (no cascade) — port of `_DISTRO_PKG_REMOVE`.
/// None — auto-removal is not supported for the family.
pub fn distro_pkg_remove(family: PkgFamily) -> Option<Vec<String>> {
    let v: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-R", "--noconfirm"],
        PkgFamily::Debian | PkgFamily::Alt => &["apt-get", "remove", "-y"],
        PkgFamily::Fedora => &["dnf", "remove", "-y"],
        PkgFamily::Suse => &["zypper", "remove", "-y"],
        PkgFamily::Gentoo => &["emerge", "--unmerge"],
        PkgFamily::Alpine => &["apk", "del"],
        PkgFamily::Void => &["xbps-remove", "-y"],
        PkgFamily::Solus => &["eopkg", "rm", "-y"],
        _ => return None,
    };
    Some(v.iter().map(|s| s.to_string()).collect())
}

/// Candidate binaries whose owner we ask about (besides the module itself) —
/// port of the list in `_detect_distro_package`.
pub fn distro_candidate_bins() -> [&'static str; 2] {
    ["/usr/bin/rexon", "/usr/local/bin/rexon"]
}

/// Parsing the manager's output about the file owner — port of the
/// `_detect_distro_package` logic. `tool` = `owner[0]` (pacman/dpkg/rpm).
/// dpkg prints "pkg: /path" → take the part before ':'; the others
/// (`pacman -Qoq` / `rpm -qf`) → the first line. Empty → None.
pub fn parse_owner_output(tool: &str, stdout: &str) -> Option<String> {
    let out = stdout.trim();
    if out.is_empty() {
        return None;
    }
    let name = if tool == "dpkg" {
        out.split(':').next().unwrap_or("").trim().to_string()
    } else {
        out.lines().next().unwrap_or("").trim().to_string()
    };
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// The command removing the distribution package via the manager:
/// `pkexec {remove...} {pkg_name}`, timeout 300.
pub fn pkg_remove_cmd(remove: &[String], pkg_name: &str) -> Cmd {
    let mut argv = vec!["pkexec".to_string()];
    argv.extend(remove.iter().cloned());
    argv.push(pkg_name.to_string());
    Cmd { argv, timeout_secs: Some(300), lang_c: false }
}

/// Install files in the command directory beyond the launcher — the wallpaper
/// host and the splash. One truth with the install:
/// [`crate::installer_exec::InstallLayout::extra_bin_paths`].
///
/// ⭐ Fix G8: uninstall in both modes didn't know them, and after «Uninstall
/// Rexon» two executables remained in `bin`.
fn extra_bin_paths(p: &InstallPaths) -> Vec<String> {
    match (&p.lib_dir, &p.install_dir) {
        (Some(lib), Some(install)) => {
            crate::installer_exec::InstallLayout::new(lib, install).extra_bin_paths()
        }
        _ => Vec::new(),
    }
}

/// The list of paths to remove in the system uninstall script (the original's
/// order, the wallpaper host and the splash right after the launcher). Port of
/// `paths_to_remove` in `RexonUninstaller.uninstall`.
pub fn uninstall_system_paths(sys: &InstallPaths) -> Vec<String> {
    let lib = sys.lib_dir.clone().unwrap_or_default();
    let install = sys.install_dir.clone().unwrap_or_default();
    let desktop = sys.desktop_dir.clone().unwrap_or_default();
    let gnome = sys.gnome_ext_dir.clone().unwrap_or_default();
    let cinnamon = sys.cinnamon_ext_dir.clone().unwrap_or_default();
    let mut paths = vec![lib, format!("{install}/rexon")];
    paths.extend(extra_bin_paths(sys));
    paths.extend([
        format!("{desktop}/rexon.desktop"),
        format!("{desktop}/rexon-recover.desktop"),
        format!("{gnome}/{EXT_ID}"),
        format!("{cinnamon}/{EXT_ID}"),
        format!("{gnome}/{EXT_ID_LEGACY}"),
        format!("{cinnamon}/{EXT_ID_LEGACY}"),
        SYSTEM_HELPER_PATH.to_string(),
        SYSTEM_POLICY_PATH.to_string(),
        DE_HELPER_PATH.to_string(),
        DE_POLICY_PATH.to_string(),
        PKG_HELPER_PATH.to_string(),
        PKG_POLICY_PATH.to_string(),
        STAGE_HELPER_PATH.to_string(),
        STAGE_POLICY_PATH.to_string(),
        // The privilege session helper ("one password per run") and its policy.
        crate::system_plan::SESSION_HELPER_PATH.to_string(),
        crate::system_plan::SESSION_POLICY_PATH.to_string(),
        // The same on SteamOS (outside `/usr`, placed by `rexon-sysext-helper`).
        crate::steamos_sysext::SESSION_HELPER_PATH.to_string(),
        crate::steamos_sysext::SESSION_POLICY_PATH.to_string(),
        REXON_POLKIT_RULES_PATH.to_string(),
        // Traces of the DESKTOP SETUP, not of the program itself. We remove them
        // too: the files are nobody's by package accounting, lie in system-wide
        // directories and previously survived uninstalling Rexon — the person
        // removed the program, but its trace stayed on the machine forever.
        crate::system_plan::MATE_GSCHEMA_PATH.to_string(),
    ]);
    paths
}

/// The text of the system uninstall bash script — port of `script_lines`.
pub fn uninstall_system_script(sys: &InstallPaths) -> String {
    uninstall_system_script_for(sys, &uninstall_system_paths(sys))
}

/// The same script, but it removes exactly `paths` — the executor
/// ([`crate::installer_exec::uninstall`]) strikes links to foreign folders out
/// of [`uninstall_system_paths`].
///
/// As the first line after `set -e` the script prints the start marker
/// ([`crate::installer_exec::STEP_MARK`]): the run outcome is told apart by
/// FACT, as with the install ([`crate::installer_exec::classify_script_run`]) —
/// no markers and pkexec refused → the password was not confirmed; a marker is
/// there → the removal itself broke.
pub fn uninstall_system_script_for(sys: &InstallPaths, paths: &[String]) -> String {
    let desktop = sys.desktop_dir.clone().unwrap_or_default();
    let mut lines = vec![
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("echo {}{}", crate::installer_exec::STEP_MARK, UNINSTALL_SCRIPT_MARK),
    ];
    for p in paths {
        lines.push(format!("rm -rf {p}"));
    }
    lines.push(format!(
        "update-desktop-database {desktop} 2>/dev/null || true"
    ));
    // After removing the override the schemas must be recompiled, otherwise the
    // removed defaults keep acting from `gschemas.compiled`.
    lines.push(
        "glib-compile-schemas /usr/share/glib-2.0/schemas/ 2>/dev/null || true".to_string(),
    );
    // Ubuntu MATE panel layouts Rexon laid out into a system-wide directory. We
    // remove ONLY those owned by nobody per package accounting: a file with the
    // same name may come from the distribution, and removing it would break
    // someone else's desktop. Where there is no accounting (not Arch), we touch nothing.
    lines.push(
        "if command -v pacman >/dev/null 2>&1; then\n\
\x20 for f in /usr/share/mate-panel/layouts/*.layout; do\n\
\x20   [ -e \"$f\" ] || continue\n\
\x20   pacman -Qo \"$f\" >/dev/null 2>&1 || rm -f \"$f\"\n\
\x20 done\n\
fi"
            .to_string(),
    );
    lines.push(dm_fallback_cleanup_script());
    lines.join("\n") + "\n"
}

/// Remove the login screen safeguard laid out by the `switch-dm` helper
/// («Make default»): the [`crate::system_plan::DM_FALLBACK_DIR`] directory (the
/// rollback script, its data and the console hint), the rollback service and
/// its drop-in with `OnFailure=` on the chosen login screen. The files are
/// nobody's per package accounting: without this they survived uninstalling
/// Rexon, and `OnFailure=` would point at a removed service. The same text is
/// run by the package's `post_remove` (`packaging/aur/rexon-git.install`).
///
/// ⚠️ The chosen LightDM greeter ([`crate::system_plan::LIGHTDM_GREETER_CONF`])
/// is NOT removed: it is a setting of the login screen that is currently the
/// default, and without it LightDM could be left with an uninstalled greeter —
/// that is, without a login (rule #13). Doesn't start or stop services.
pub fn dm_fallback_cleanup_script() -> String {
    use crate::system_plan::{DM_FALLBACK_DIR, DM_FALLBACK_DROPIN, DM_FALLBACK_SERVICE};
    format!(
        "for f in /etc/systemd/system/*.service.d/{DM_FALLBACK_DROPIN}; do\n\
\x20 [ -e \"$f\" ] || continue\n\
\x20 rm -f \"$f\"\n\
\x20 rmdir \"$(dirname \"$f\")\" 2>/dev/null || true\n\
done\n\
rm -f /etc/systemd/system/{DM_FALLBACK_SERVICE}\n\
rm -rf {DM_FALLBACK_DIR}\n\
if command -v systemctl >/dev/null 2>&1; then systemctl daemon-reload 2>/dev/null || true; fi"
    )
}

/// The command running the uninstall script: `pkexec bash {tmp_sh}`, timeout 300.
pub fn uninstall_system_cmd(tmp_sh: &str) -> Cmd {
    Cmd {
        argv: vec!["pkexec".into(), "bash".into(), tmp_sh.to_string()],
        timeout_secs: Some(300),
        lang_c: false,
    }
}

/// Start marker of the uninstall root script (after
/// [`crate::installer_exec::STEP_MARK`]).
pub const UNINSTALL_SCRIPT_MARK: &str = "files";

/// The system uninstall plan: write the script → pkexec → delete tmp_sh.
pub fn uninstall_system_plan(sys: &InstallPaths, tmp_sh: &str) -> Vec<PlanStep> {
    uninstall_system_plan_for(sys, &uninstall_system_paths(sys), tmp_sh)
}

/// A plan that removes exactly `paths` ([`uninstall_system_script_for`]).
pub fn uninstall_system_plan_for(sys: &InstallPaths, paths: &[String], tmp_sh: &str) -> Vec<PlanStep> {
    vec![
        PlanStep::WriteFile {
            path: tmp_sh.to_string(),
            content: uninstall_system_script_for(sys, paths),
            mode: 0o755,
        },
        PlanStep::Run {
            id: RunStepId::AppUninstall,
            cmd: uninstall_system_cmd(tmp_sh),
            needs_root: true,
            description: "System uninstall (pkexec)".into(),
        },
        PlanStep::RemoveFiles { paths: vec![tmp_sh.to_string()] },
    ]
}

/// Candidates for the removal list of one install (system/user) — six paths in
/// `get_installed_files` order.
pub fn install_candidate_files(p: &InstallPaths) -> Vec<String> {
    let install = p.install_dir.clone().unwrap_or_default();
    let lib = p.lib_dir.clone().unwrap_or_default();
    let desktop = p.desktop_dir.clone().unwrap_or_default();
    let gnome = p.gnome_ext_dir.clone().unwrap_or_default();
    let cinnamon = p.cinnamon_ext_dir.clone().unwrap_or_default();
    let mut out = vec![format!("{install}/rexon")];
    out.extend(extra_bin_paths(p));
    out.extend([
        lib,
        format!("{desktop}/rexon.desktop"),
        format!("{desktop}/rexon-recover.desktop"),
        format!("{gnome}/{EXT_ID}"),
        format!("{cinnamon}/{EXT_ID}"),
        format!("{gnome}/{EXT_ID_LEGACY}"),
        format!("{cinnamon}/{EXT_ID_LEGACY}"),
    ]);
    out
}

/// Port of `RexonUninstaller.get_installed_files`: package + system + user +
/// user data, filtering each path through `exists`. `pkg_name` — the
/// distribution package name (None — not installed as a package).
/// `config_dir`/`backup_dir` — user data paths.
#[allow(clippy::too_many_arguments)]
pub fn get_installed_files(
    pkg_name: Option<&str>,
    sys: Option<&InstallPaths>,
    usr: Option<&InstallPaths>,
    config_dir: &str,
    backup_dir: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    if let Some(name) = pkg_name {
        // TRANSLATORS: a line in the list of what "Uninstall Rexon" removes,
        // among folder paths; %1 is the name of the distribution package.
        files.push(trf("the “%1” package (with all its files on the system)", &[&name]));
    }
    for inst in [sys, usr].into_iter().flatten() {
        for p in install_candidate_files(inst) {
            if exists(&p) {
                files.push(p);
            }
        }
    }
    if exists(config_dir) {
        files.push(config_dir.to_string());
    }
    if exists(backup_dir) {
        files.push(backup_dir.to_string());
    }
    files
}

// ============================================================================
// User-mode uninstall (port of the second half of `RexonUninstaller.uninstall`)
// ============================================================================

/// hicolor size directories where user-install puts the icon (the original's
/// order: PNG sizes ascending + `scalable`).
pub const ICON_SIZE_DIRS: &[&str] = &[
    "16x16", "24x24", "32x32", "48x48", "64x64", "128x128", "256x256",
    "512x512", "scalable",
];

/// What the user uninstall cleans: six main paths + per-user icons (only our
/// `rexon.png`/`rexon.svg`, not the whole size folder) + the pixmap.
///
/// `lexists` — live Python's `os.path.lexists`: a broken symlink is also to be
/// removed, hence `lexists` rather than `exists`. The original didn't filter
/// the main six paths at all (it removed "as it goes"), so they are always in
/// the list — the filter applies only to icons, as before.
pub fn uninstall_user_paths(
    usr: &InstallPaths,
    lexists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let lib = usr.lib_dir.clone().unwrap_or_default();
    let install = usr.install_dir.clone().unwrap_or_default();
    let desktop = usr.desktop_dir.clone().unwrap_or_default();
    let gnome = usr.gnome_ext_dir.clone().unwrap_or_default();
    let cinnamon = usr.cinnamon_ext_dir.clone().unwrap_or_default();
    let icon_root = usr.icon_root.clone().unwrap_or_default();
    let pixmap = usr.pixmap_dir.clone().unwrap_or_default();
    let mut paths = vec![lib, format!("{install}/rexon")];
    paths.extend(extra_bin_paths(usr));
    paths.extend([
        format!("{desktop}/rexon.desktop"),
        format!("{desktop}/rexon-recover.desktop"),
        format!("{gnome}/{EXT_ID}"),
        format!("{cinnamon}/{EXT_ID}"),
        format!("{gnome}/{EXT_ID_LEGACY}"),
        format!("{cinnamon}/{EXT_ID_LEGACY}"),
    ]);
    for size_dir in ICON_SIZE_DIRS {
        for ext in ["png", "svg"] {
            let p = format!("{icon_root}/{size_dir}/apps/rexon.{ext}");
            if lexists(&p) {
                paths.push(p);
            }
        }
    }
    let pm = format!("{pixmap}/rexon.png");
    if lexists(&pm) {
        paths.push(pm);
    }
    paths
}

/// The uninstall summary: `ok=false` — nothing was installed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UninstallSummary {
    pub ok: bool,
    pub message: String,
}

/// Port of the tail of `RexonUninstaller.uninstall`: a list of what was
/// removed, or an honest "not installed".
pub fn uninstall_summary(
    pkg_name: Option<&str>,
    has_system: bool,
    has_user: bool,
) -> UninstallSummary {
    if pkg_name.is_none() && !has_system && !has_user {
        return UninstallSummary {
            ok: false,
            message: "Rexon is not installed".to_string(),
        };
    }
    let mut which: Vec<String> = Vec::new();
    if let Some(name) = pkg_name {
        which.push(format!("package {name}"));
    }
    if has_system {
        which.push("system".to_string());
    }
    if has_user {
        which.push("user".to_string());
    }
    UninstallSummary {
        ok: true,
        message: format!("Rexon removed ({})", which.join(", ")),
    }
}

#[cfg(test)]
mod package_data_dir_tests {
    use super::*;
    use std::path::Path;

    /// Lay out a layout: `<base>/<rel>/qt_wallpaper_engine/qml_ui/PageHost.qml`.
    fn lay_out(base: &Path, rel: &str) {
        let dir = base.join(rel).join("qt_wallpaper_engine/qml_ui");
        std::fs::create_dir_all(&dir).expect("layout directory created");
        std::fs::write(dir.join("PageHost.qml"), b"// stub").expect("stub written");
    }

    /// Both live layouts — the developer tree and the package — are assembled
    /// from real files; the package prefix is substituted by a probe so the test
    /// doesn't write into `/usr`.
    #[test]
    fn picks_dev_tree_then_package_layout() {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let base = Path::new(&home).join(format!("rexon-pkgdir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        lay_out(&base, &format!("tree/{DEV_TREE_DATA_DIR}"));
        lay_out(&base, "usr/lib/rexon/rexon");

        let pkg_prefix = package_data_dir().to_string();
        let base_str = base.to_string_lossy().into_owned();
        let probe = |path: &str| {
            let mapped = match path.strip_prefix(&pkg_prefix) {
                Some(rest) => format!("{base_str}/usr/lib/rexon{rest}"),
                None => path.to_string(),
            };
            Path::new(&mapped).is_file()
        };

        // The developer tree is more precise than the package: that's where editing happens.
        let dev_root = format!("{base_str}/tree");
        assert_eq!(
            pick_package_data_dir(&dev_root, &probe),
            Some(format!("{dev_root}/{DEV_TREE_DATA_DIR}"))
        );
        // No tree (running from the package) — the package layout is taken.
        assert_eq!(
            pick_package_data_dir("", &probe),
            Some(format!("{pkg_prefix}/rexon"))
        );
        // No layout nearby — an honest "not found".
        assert_eq!(pick_package_data_dir("", &|_p: &str| false), None);

        std::fs::remove_dir_all(&base).expect("temporary layout removed");
    }
}

#[cfg(test)]
mod first_run_tests {
    use super::*;
    use serde_json::{Map, Value};
    use std::collections::{HashMap, HashSet};

    const HOME: &str = "/home/tester";

    /// A substitute machine: reads nothing from disk or the environment.
    #[derive(Default)]
    struct FakeProbe {
        env: HashMap<String, String>,
        distro: String,
        own: Option<String>,
        root: String,
        symlinks: HashMap<String, String>,
        files: HashSet<String>,
        dirs: HashSet<String>,
    }

    impl FakeProbe {
        /// An ordinary uninstalled launch of the Rust window from an unpacked archive on Arch.
        fn plain() -> Self {
            let mut p = FakeProbe {
                distro: "arch".into(),
                own: Some("/home/tester/Rexon/rexon-main-window".into()),
                ..Default::default()
            };
            p.env.insert("HOME".into(), HOME.into());
            p
        }
        fn with_env(mut self, k: &str, v: &str) -> Self {
            self.env.insert(k.into(), v.into());
            self
        }
        fn with_own(mut self, path: &str) -> Self {
            self.own = Some(path.into());
            self
        }
        /// Lay out the install markers along the mode's paths.
        fn installed(mut self, paths: &InstallPaths) -> Self {
            let install = paths.install_dir.clone().unwrap();
            let desktop = paths.desktop_dir.clone().unwrap();
            self.files.insert(format!("{install}/rexon"));
            self.files.insert(format!("{desktop}/rexon.desktop"));
            self.dirs.insert(paths.lib_dir.clone().unwrap());
            self
        }
    }

    impl FirstRunProbe for FakeProbe {
        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }
        fn distro_id(&self) -> String {
            self.distro.clone()
        }
        fn own_path(&self) -> Option<String> {
            self.own.clone()
        }
        fn repo_root(&self) -> String {
            self.root.clone()
        }
        fn dev_tree_behind(&self, lib_dir: &str) -> Option<String> {
            self.symlinks.get(lib_dir).cloned()
        }
        fn exists(&self, path: &str) -> bool {
            self.files.contains(path) || self.dirs.contains(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.contains(path)
        }
    }

    fn no_settings() -> Map<String, Value> {
        Map::new()
    }

    fn declined_settings() -> Map<String, Value> {
        let mut m = crate::settings::defaults();
        m.insert(KEY_FIRST_RUN_DECLINED.into(), Value::Bool(true));
        m
    }

    fn system_paths() -> InstallPaths {
        install_paths(InstallMode::System, HOME, None)
    }

    fn user_paths() -> InstallPaths {
        install_paths(InstallMode::User, HOME, None)
    }

    fn skip(reason: FirstRunSkip) -> FirstRunOffer {
        FirstRunOffer::Skip(reason)
    }

    #[test]
    fn plain_uninstalled_run_offers_system_mode() {
        assert_eq!(
            first_run_offer(&FakeProbe::plain(), &no_settings()),
            FirstRunOffer::Offer {
                default_mode: InstallMode::System,
                system_blocked: None,
                forced: false
            }
        );
    }

    #[test]
    fn immutable_system_offers_user_mode_with_reason() {
        let mut p = FakeProbe::plain();
        p.distro = "steamos".into();
        match first_run_offer(&p, &no_settings()) {
            FirstRunOffer::Offer { default_mode, system_blocked, forced } => {
                assert_eq!(default_mode, InstallMode::User);
                assert_eq!(system_blocked, system_mode_blocked_reason("steamos"));
                assert!(system_blocked.is_some());
                assert!(!forced);
            }
            other => panic!("expected an offer, got {other:?}"),
        }
    }

    #[test]
    fn packaged_env_skips() {
        let p = FakeProbe::plain().with_env(PACKAGED_ENV, "1");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::PackagedEnv));
        // An empty value is a marker too: detect_default_mode reads it the same way.
        let p = FakeProbe::plain().with_env(PACKAGED_ENV, "");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::PackagedEnv));
    }

    /// Defect 7 of the plan: the package's desktop entry calls the window binary without REXON_PACKAGED.
    #[test]
    fn package_binary_without_env_skips() {
        for own in [
            PACKAGE_MAIN_WINDOW,
            "/usr/bin/rexon-main-window",
            "/usr/lib64/rexon/rexon-main-window",
            "/usr/lib/rexon/rexon-main-window (deleted)",
            "/nix/store/abc-rexon/bin/rexon-main-window",
            "/app/bin/rexon-main-window",
            // The Python entry passes the module path, not the interpreter's.
            "/usr/lib/rexon/rexon/installer.py",
        ] {
            let p = FakeProbe::plain().with_own(own);
            assert_eq!(
                first_run_offer(&p, &no_settings()),
                skip(FirstRunSkip::PackagedLocation(own.into())),
                "{own}"
            );
        }
    }

    #[test]
    fn local_and_home_locations_are_not_package() {
        for own in [
            "/usr/local/bin/rexon-main-window",
            "/usr/local/lib/rexon/rexon/installer.py",
            "/home/tester/.local/bin/rexon-main-window",
            "/opt/rexon/rexon-main-window",
            "rexon-main-window",
            "/usrx/bin/rexon",
        ] {
            assert!(!packaged_location(own), "{own}");
            let p = FakeProbe::plain().with_own(own);
            assert!(first_run_offer(&p, &no_settings()).is_offer(), "{own}");
        }
        // No own path at all — we don't decide by location, the rest still holds.
        let mut p = FakeProbe::plain();
        p.own = None;
        assert!(first_run_offer(&p, &no_settings()).is_offer());
    }

    #[test]
    fn steam_envs_skip() {
        for var in ["SteamGameId", "SteamAppId", "STEAM_COMPAT_DATA_PATH"] {
            let p = FakeProbe::plain().with_env(var, "123");
            assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::Steam(var)));
        }
    }

    #[test]
    fn dev_tree_by_own_path_skips() {
        let mut p = FakeProbe::plain()
            .with_own("/home/tester/src/rexon_v5/rust/target/release/rexon-main-window");
        p.root = "/home/tester/src/rexon_v5".into();
        assert_eq!(
            first_run_offer(&p, &no_settings()),
            skip(FirstRunSkip::DevTree("/home/tester/src/rexon_v5".into()))
        );
    }

    /// Developer machine: `/usr/local/lib/rexon` is a symlink into the tree, the
    /// window was not launched from the tree (e.g. a copied binary).
    #[test]
    fn dev_tree_by_symlinked_lib_dir_skips() {
        let mut p = FakeProbe::plain();
        p.symlinks.insert(LIB_DIR.into(), "/home/tester/src/rexon_v5".into());
        assert_eq!(
            first_run_offer(&p, &no_settings()),
            skip(FirstRunSkip::DevTree("/home/tester/src/rexon_v5".into()))
        );
        let mut p = FakeProbe::plain();
        p.symlinks.insert(user_paths().lib_dir.unwrap(), "/home/tester/tree".into());
        assert_eq!(
            first_run_offer(&p, &no_settings()),
            skip(FirstRunSkip::DevTree("/home/tester/tree".into()))
        );
    }

    #[test]
    fn installed_system_skips() {
        let p = FakeProbe::plain().installed(&system_paths());
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledSystem));
    }

    /// Fix: Python checked only the default mode (on Arch — system).
    #[test]
    fn installed_user_on_system_default_distro_skips() {
        let p = FakeProbe::plain().installed(&user_paths());
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledUser));
    }

    #[test]
    fn installed_user_respects_xdg_data_home() {
        let paths = install_paths(InstallMode::User, HOME, Some("/data/xdg"));
        let p = FakeProbe::plain()
            .with_env("XDG_DATA_HOME", "/data/xdg")
            .installed(&paths);
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledUser));
        // An empty XDG_DATA_HOME means the default per the spec.
        let p = FakeProbe::plain()
            .with_env("XDG_DATA_HOME", "")
            .installed(&user_paths());
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledUser));
    }

    #[test]
    fn partial_install_still_offers() {
        let sys = system_paths();
        let mut p = FakeProbe::plain();
        p.files.insert(format!("{}/rexon", sys.install_dir.unwrap()));
        assert!(first_run_offer(&p, &no_settings()).is_offer());
    }

    #[test]
    fn no_home_skips_only_user_check() {
        let mut p = FakeProbe::plain();
        p.env.remove("HOME");
        assert!(first_run_offer(&p, &no_settings()).is_offer());
        let p = {
            let mut p = FakeProbe::plain().installed(&system_paths());
            p.env.remove("HOME");
            p
        };
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledSystem));
    }

    #[test]
    fn declined_is_remembered() {
        assert_eq!(
            first_run_offer(&FakeProbe::plain(), &declined_settings()),
            skip(FirstRunSkip::Declined)
        );
    }

    #[test]
    fn declined_non_bool_or_false_still_offers() {
        for v in [Value::Bool(false), Value::from("true"), Value::from(1), Value::Null] {
            let mut m = Map::new();
            m.insert(KEY_FIRST_RUN_DECLINED.into(), v.clone());
            assert!(!first_run_declined_from(&m), "{v}");
            assert!(first_run_offer(&FakeProbe::plain(), &m).is_offer(), "{v}");
        }
        // An orphaned `wizard_done` doesn't affect the decision.
        let mut m = crate::settings::defaults();
        m.insert("wizard_done".into(), Value::Bool(true));
        assert!(first_run_offer(&FakeProbe::plain(), &m).is_offer());
    }

    #[test]
    fn force_overrides_everything() {
        let mut p = FakeProbe::plain()
            .with_env(FIRST_RUN_ENV, " Force ")
            .with_env(PACKAGED_ENV, "1")
            .with_env("SteamAppId", "1")
            .with_own(PACKAGE_MAIN_WINDOW)
            .installed(&system_paths())
            .installed(&user_paths());
        p.root = "/tree".into();
        assert_eq!(
            first_run_offer(&p, &declined_settings()),
            FirstRunOffer::Offer {
                default_mode: InstallMode::System,
                system_blocked: None,
                forced: true
            }
        );
        // On an immutable system even force offers the home folder.
        p.distro = "bazzite".into();
        match first_run_offer(&p, &no_settings()) {
            FirstRunOffer::Offer { default_mode, system_blocked, forced } => {
                assert_eq!(default_mode, InstallMode::User);
                assert!(system_blocked.is_some());
                assert!(forced);
            }
            other => panic!("expected an offer, got {other:?}"),
        }
    }

    #[test]
    fn never_overrides_everything() {
        let p = FakeProbe::plain().with_env(FIRST_RUN_ENV, "NEVER");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::ForcedNever));
    }

    #[test]
    fn unknown_override_value_is_ignored() {
        assert_eq!(first_run_override(None), None);
        for v in ["", "1", "yes", "forced", "no"] {
            assert_eq!(first_run_override(Some(v)), None, "{v}");
            let p = FakeProbe::plain().with_env(FIRST_RUN_ENV, v);
            assert!(first_run_offer(&p, &no_settings()).is_offer(), "{v}");
        }
        assert_eq!(first_run_override(Some("force")), Some(true));
        assert_eq!(first_run_override(Some("never")), Some(false));
    }

    /// Order: the package before the developer tree, the tree before "installed",
    /// "installed" before the refusal — the reason in the log is the most precise.
    #[test]
    fn reasons_follow_documented_order() {
        let mut p = FakeProbe::plain()
            .with_own(PACKAGE_MAIN_WINDOW)
            .installed(&system_paths());
        p.root = "/tree".into();
        assert!(matches!(
            first_run_offer(&p, &declined_settings()),
            FirstRunOffer::Skip(FirstRunSkip::PackagedLocation(_))
        ));
        p.own = None;
        assert!(matches!(
            first_run_offer(&p, &declined_settings()),
            FirstRunOffer::Skip(FirstRunSkip::DevTree(_))
        ));
        p.root.clear();
        assert_eq!(
            first_run_offer(&p, &declined_settings()),
            skip(FirstRunSkip::InstalledSystem)
        );
        // The package by variable before Steam.
        let p = FakeProbe::plain()
            .with_env(PACKAGED_ENV, "1")
            .with_env("SteamGameId", "1");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::PackagedEnv));
    }

    #[test]
    fn log_lines_name_every_reason() {
        let cases = [
            FirstRunOffer::Offer {
                default_mode: InstallMode::System,
                system_blocked: None,
                forced: false,
            },
            FirstRunOffer::Offer {
                default_mode: InstallMode::User,
                system_blocked: Some("x".into()),
                forced: true,
            },
            skip(FirstRunSkip::ForcedNever),
            skip(FirstRunSkip::PackagedEnv),
            skip(FirstRunSkip::PackagedLocation("/usr/bin/rexon".into())),
            skip(FirstRunSkip::Steam("SteamAppId")),
            skip(FirstRunSkip::DevTree("/tree".into())),
            skip(FirstRunSkip::InstalledSystem),
            skip(FirstRunSkip::InstalledUser),
            skip(FirstRunSkip::Declined),
        ];
        let lines: HashSet<String> = cases.iter().map(FirstRunOffer::log_line).collect();
        assert_eq!(lines.len(), cases.len(), "every reason has its own line");
        assert!(cases[1].log_line().contains("force"));
        assert!(cases[4].log_line().contains("/usr/bin/rexon"));
        assert!(cases[5].log_line().contains("SteamAppId"));
    }

    /// «Don't ask again» is written to a real file — a temporary one, the person's
    /// profile isn't touched; without the check nothing is written.
    #[test]
    fn decline_session_writes_only_when_remembered() {
        let dir = std::env::temp_dir()
            .join(format!("rexon-first-run-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temporary directory created");
        let file = dir.join("settings.json");
        let mut manager = crate::settings::SettingsManager {
            config_dir: dir.clone(),
            config_file: file.clone(),
            settings: crate::settings::load_settings(&file),
        };

        session_first_run_decline(&mut manager, false).expect("no mark — no error");
        assert!(!file.exists(), "without the mark no file is written");
        assert!(first_run_offer(&FakeProbe::plain(), &manager.settings).is_offer());

        session_first_run_decline(&mut manager, true).expect("refusal saved");
        let reloaded = crate::settings::load_settings(&file);
        assert!(first_run_declined_from(&reloaded));
        assert_eq!(
            first_run_offer(&FakeProbe::plain(), &reloaded),
            skip(FirstRunSkip::Declined)
        );

        std::fs::remove_dir_all(&dir).expect("temporary directory removed");
    }
}

#[cfg(test)]
mod wizard_texts_tests {
    use super::*;
    use serde_json::Value;

    const HOME: &str = "/home/tester";

    /// The language of the checks is the source English, whatever the machine's
    /// locale and whatever catalog lies in the system. The Russian is checked word
    /// for word by `tests/installer_parity.rs` under a pinned Russian catalog.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    /// Jargon words the user must not see. Compared in lowercase.
    const JARGON: &[&str] = &[
        // i18n-exempt-begin: jargon words the guard looks for in Russian texts too
        "pkexec", "polkit", "helper", "хелпер", "лаунчер", "launcher", ".desktop", "$path",
        "bashrc", "zshrc", "config.fish", "pyside", "qt6-webengine", "layer-shell",
        "user-install", "user-режим", "portable", "extensions", "sudo", "из терминала",
        "/usr/", "~/.local",
        // i18n-exempt-end
    ];

    /// Jargon in the text; paths are allowed in tooltips (`allow_paths`).
    fn jargon_in(text: &str, allow_paths: bool) -> Option<String> {
        let checked: String = if allow_paths {
            text.split_whitespace()
                .filter(|w| {
                    let w = w.trim_start_matches(['(', '«', '“', '"']);
                    !(w.starts_with('/') || w.starts_with('~'))
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            text.to_string()
        };
        let lower = checked.to_lowercase();
        if let Some(word) = JARGON.iter().find(|j| lower.contains(*j)) {
            return Some((*word).to_string());
        }
        if !allow_paths {
            let path = text.split_whitespace().find(|w| {
                let w = w.trim_start_matches(['(', '«', '“', '"']);
                w.starts_with('/') || w.starts_with("~/")
            });
            if let Some(p) = path {
                return Some(format!("path {p}"));
            }
        }
        None
    }

    /// All the value's strings: (field name, text). The `mode` field is an
    /// identifier, not a label; `details` — raw output for the developer.
    fn walk(value: &Value, key: &str, out: &mut Vec<(String, String)>) {
        match value {
            Value::String(s) if key != "mode" && key != "details" => {
                out.push((key.to_string(), s.clone()))
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, key, out)),
            Value::Object(map) => map.iter().for_each(|(k, v)| walk(v, k, out)),
            _ => {}
        }
    }

    fn outcomes(lang: Lang) -> Vec<WizardOutcome> {
        let mut out = Vec::new();
        for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
            out.push(WizardOutcome::Installed {
                mode,
                fell_back: None,
                command_dir_not_in_path: None,
            });
        }
        out.push(WizardOutcome::Installed {
            mode: InstallMode::User,
            fell_back: system_mode_blocked_reason_in("steamos", lang),
            command_dir_not_in_path: Some(format!("{HOME}/.local/bin")),
        });
        out.push(WizardOutcome::Failed(InstallFailure::PasswordDenied));
        out.push(WizardOutcome::Failed(InstallFailure::PasswordPromptMissing));
        out.push(WizardOutcome::Failed(InstallFailure::TimedOut { secs: 300 }));
        for step in InstallStep::ALL {
            out.push(WizardOutcome::Failed(InstallFailure::StepFailed {
                step,
                detail: "pkexec: /usr/local/lib/rexon: PySide6 not found".into(),
            }));
        }
        out
    }

    /// All wizard strings in one language — in the same order for both languages.
    fn all_texts(lang: Lang) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut push_json = |v: Value| walk(&v, "", &mut out);
        for distro in ["arch", "steamos"] {
            push_json(serde_json::to_value(welcome_texts(distro, HOME, None, lang)).unwrap());
        }
        push_json(serde_json::to_value(passwordless_consent_texts(lang)).unwrap());
        for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
            push_json(serde_json::to_value(progress_texts(mode, lang)).unwrap());
        }
        for outcome in outcomes(lang) {
            push_json(serde_json::to_value(result_texts(&outcome, lang)).unwrap());
        }
        let text = |s: String| (String::from("text"), s);
        for step in InstallStep::ALL {
            out.push(text(step.name(lang)));
            out.push(text(step.progress_line(lang)));
        }
        for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
            out.push(text(install_done_line(mode, lang)));
            out.push(text(mode_title(mode, lang)));
            out.push(text(mode_description(mode, lang)));
        }
        for outcome in outcomes(lang) {
            if let WizardOutcome::Failed(f) = outcome {
                out.push(text(f.log_line(lang)));
            }
        }
        out.extend([
            text(icons_missing_notice(lang)),
            text(menu_refresh_later_notice(lang)),
            text(dependencies_with_rexon_notice(lang)),
            text(portable_no_install_notice(lang)),
            text(command_path_notice(lang)),
            (String::from("hint"), command_path_hint(&format!("{HOME}/.local/bin"), lang)),
            text(wizard_window_title(lang)),
            text(dry_run_notice(lang)),
            text(system_mode_blocked_reason_in("steamos", lang).unwrap()),
        ]);
        out.extend(without_install_limits(lang).into_iter().map(text));
        // The «Install Rexon on this computer…» button on the «⚙️ Settings» page (G5).
        out.extend([
            text(settings_install_button(lang)),
            text(settings_install_hint(lang)),
            text(settings_install_started(lang)),
            text(settings_install_no_window(lang)),
        ]);
        // Uninstalling Rexon (G8). There is no raw output in the checked strings:
        // the details go as a tail, like the wizard's `details`.
        for part in UninstallPart::ALL {
            out.push(text(part.progress_line(lang)));
        }
        for keep in [true, false] {
            out.push(text(uninstall_done_line(keep, lang)));
        }
        for kind in UserDataKind::ALL {
            out.push(text(kind.title()));
        }
        out.push(text(uninstall_nothing_line(lang)));
        out.push((String::from("hint"), uninstall_kept_link_note("/usr/local/lib/rexon", lang)));
        out.push((String::from("hint"), uninstall_removed_link_note("/home/u/.cache/rexon", lang)));
        out.push(text(uninstall_package_unsupported_line("rexon-git", lang)));
        for failure in [
            InstallFailure::PasswordDenied,
            InstallFailure::PasswordPromptMissing,
            InstallFailure::TimedOut { secs: 300 },
            InstallFailure::StepFailed { step: InstallStep::SystemInstall, detail: String::new() },
        ] {
            out.push(text(uninstall_failure_line(&failure, lang)));
        }
        out
    }

    /// The uninstall summary: details only as a tail and under their own heading;
    /// "system", "code 126" and other jargon don't appear in the window's words
    /// (previously: "Uninstall error system: code 126: …").
    /// The login screen safeguard is removed both by uninstalling from the program
    /// and by `pacman -R` (the package script) — with the same text; the chosen
    /// LightDM greeter stays, services are neither started nor stopped.
    #[test]
    fn package_post_remove_matches_core_cleanup() {
        let core = dm_fallback_cleanup_script();
        let sys = install_paths(InstallMode::System, "", None);
        assert!(uninstall_system_script(&sys).ends_with(&format!("{core}\n")));
        for want in [
            "/etc/systemd/system/*.service.d/50-rexon-fallback.conf",
            "rm -f /etc/systemd/system/rexon-dm-fallback.service",
            "rm -rf /etc/rexon",
        ] {
            assert!(core.contains(want), "{want}");
        }
        assert!(!core.contains(crate::system_plan::LIGHTDM_GREETER_CONF));
        assert!(!core.contains("50-rexon-greeter"), "the LightDM greeter is not touched");
        for word in [" start", " stop", "restart", "--now", "isolate"] {
            assert!(!core.contains(word), "service in the live session: {word}");
        }
        let install = include_str!("../../../packaging/aur/rexon-git.install");
        let body = install
            .split_once("post_remove() {\n")
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(body, _)| body)
            .expect("post_remove in rexon-git.install");
        let lines = |t: &str| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect::<Vec<_>>();
        assert_eq!(lines(body), lines(&core), "the package post_remove diverged from the core");
        assert!(include_str!("../../../packaging/aur/PKGBUILD").contains("\ninstall=rexon-git.install\n"));
    }

    #[test]
    fn uninstall_texts_speak_plainly_and_keep_raw_output_apart() {
        english();
        let step = InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: "rm: cannot remove '/usr/local/lib/rexon': Read-only file system".into(),
        };
        let line = uninstall_failure_line(&step, Lang::En);
        let (words, raw) = line.split_once("\n\nDetails for the developer:\n").expect("details tail");
        assert!(raw.contains("Read-only file system"));
        assert!(jargon_in(words, false).is_none(), "{words}");
        for (_, t) in all_texts(Lang::En) {
            assert!(!t.contains("system:") && !t.contains("code 126"), "{t}");
        }
        assert_ne!(uninstall_done_line(true, Lang::En), uninstall_done_line(false, Lang::En));
        // «Keep» names everything kept and that the temporary files were removed;
        // «Remove Everything» — everything removed (decision of 15.09.2026: the cache —
        // with any choice, wallpapers and themes — only with «Remove Everything»). The
        // Russian is checked word for word by the `installer_parity::russian_installer_texts_as_before` snapshot.
        let (kept, gone) = (uninstall_done_line(true, Lang::En), uninstall_done_line(false, Lang::En));
        for w in ["settings", "backups", "wallpapers", "themes", "add-ons", "kept", "temporary files were deleted"] {
            assert!(kept.contains(w), "the \"keep\" summary does not mention \"{w}\": {kept}");
        }
        for w in ["settings", "backups", "wallpapers", "themes", "add-ons", "temporary files"] {
            assert!(gone.contains(w), "the \"remove everything\" summary does not mention \"{w}\": {gone}");
        }
        assert!(jargon_in(&kept, false).is_none() && jargon_in(&gone, false).is_none());
        // No "cache" jargon in the words for the person.
        for t in [&kept, &gone, &UninstallPart::Cache.progress_line(Lang::En), &UserDataKind::Cache.title()] {
            assert!(!t.to_lowercase().contains("cache"), "{t}");
        }
        assert!(uninstall_kept_link_note("/x/y", Lang::En).starts_with("/x/y"));
        assert!(uninstall_removed_link_note("/x/y", Lang::En).starts_with("/x/y"));
        // Strictly: the cache is the only thing that goes even with «Keep».
        let when_keeping: Vec<_> =
            UserDataKind::ALL.into_iter().filter(|k| k.removed_when_keeping()).collect();
        assert_eq!(when_keeping, vec![UserDataKind::Cache]);
    }

    /// Data roots: XDG and literal, without repeats; relative XDG values and a
    /// relative home folder give no paths.
    #[test]
    fn user_data_roots_follow_xdg_and_keep_literal_roots() {
        use UserDataKind::*;
        let plain = user_data_roots("/h", "", "", "");
        assert_eq!(
            plain,
            vec![
                (Cache, "/h/.cache/rexon".to_string()),
                (Settings, "/h/.config/rexon".into()),
                (Backups, "/h/.local/share/rexon/backups".into()),
                (Content, "/h/.local/share/rexon".into()),
            ]
        );
        // The same root written with a trailing slash is not a second root.
        assert_eq!(user_data_roots("/h/", "/h/.config/", "/h/.local/share/", "/h/.cache/"), plain);
        let xdg = user_data_roots("/h", "/x/conf", "/x/data", "/x/cache");
        assert_eq!(
            xdg,
            vec![
                (Cache, "/x/cache/rexon".to_string()),
                (Cache, "/h/.cache/rexon".into()),
                (Settings, "/x/conf/rexon".into()),
                (Settings, "/h/.config/rexon".into()),
                (Backups, "/x/data/rexon/backups".into()),
                (Backups, "/h/.local/share/rexon/backups".into()),
                (Content, "/x/data/rexon".into()),
                (Content, "/h/.local/share/rexon".into()),
            ]
        );
        assert_eq!(user_data_roots("/h", "conf", "data", "cache"), plain, "a relative XDG path is ignored");
        assert!(user_data_roots("", "", "", "").is_empty());
        assert!(user_data_roots("h", "/x", "/x", "/x").is_empty());
    }

    /// ⭐ Guard: no wizard label carries jargon or paths; paths are allowed only in
    /// tooltips.
    #[test]
    fn wizard_texts_have_no_jargon() {
        english();
        // The instruction of the shared "no way to ask for a password" refusal names
        // the package — that is what must be installed (decided the same way in system_plan_parity).
        let instruction = prompt_missing_instruction();
        assert!(instruction.starts_with("Install the polkit package "), "{instruction}");
        let texts = all_texts(Lang::En);
        assert!(texts.len() > 100, "suspiciously few strings collected: {}", texts.len());
        for (key, text) in texts {
            // The same instruction can be the tail of an uninstall refusal.
            if text.contains(&instruction) {
                continue;
            }
            if let Some(word) = jargon_in(&text, key == "hint") {
                panic!("jargon \"{word}\" in field \"{key}\": {text}");
            }
        }
    }

    /// The guard checks itself on the old Python strings.
    #[test]
    fn jargon_guard_catches_python_wizard_strings() {
        // i18n-exempt-begin: test data, the old Russian wizard strings the guard must catch
        for bad in [
            "🛡  Системно  (/usr/local/, требуется пароль администратора)",
            "👤  В мою папку  (~/.local/, без пароля)",
            "Доступно всем пользователям машины. Иконки/extensions через pkexec.",
            "PySide6 / qt6-webengine / layer-shell-qt должны быть установлены отдельно.",
            "📁 Создание лаунчера...",
            "📝 Создание .desktop файла...",
            "🔐 Polkit helper (один пароль за сессию)...",
            "📦 Portable режим: установка не требуется",
            "📦 User-install в /home/x/.local/lib/rexon/...",
            "⚠️  /home/x/.local/bin не в $PATH — добавьте в ~/.bashrc",
            "User-режим установка завершена",
            "Установка отменена пользователем (pkexec)",
            "Таймаут установки — попробуйте выполнить из терминала",
        ] {
            assert!(jargon_in(bad, false).is_some(), "the guard missed: {bad}");
        }
        assert!(jargon_in("Файлы программы: /usr/local/lib/rexon", true).is_none());
        assert!(jargon_in("Файл: /usr/x — через pkexec", true).is_some());
        // i18n-exempt-end
    }

    /// In the English interface not a single Russian letter — and not a single
    /// empty label where one must be. The Russian is checked word for word by the
    /// `installer_parity::russian_installer_texts_as_before` snapshot.
    #[test]
    fn english_wizard_texts_have_no_russian() {
        english();
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        for (key, text) in all_texts(Lang::En) {
            assert!(!cyrillic(&text), "Cyrillic in an English string ({key}): {text}");
        }
    }

    #[test]
    fn welcome_buttons_and_modes() {
        english();
        let w = welcome_texts("arch", HOME, None, Lang::En);
        assert_eq!(w.install_button, "Install");
        assert_eq!(w.skip_button, "Skip");
        assert_eq!(w.dont_ask_again, "Don't ask again");
        let modes: Vec<InstallMode> = w.modes.iter().map(|m| m.mode).collect();
        assert_eq!(modes, WIZARD_MODES.to_vec());
        assert!(w.modes.iter().all(|m| m.blocked.is_empty()));
        assert_eq!(w.modes[0].title, "For everyone on this computer");
        assert_eq!(w.modes[1].title, "Just for you");
        // Paths — only in the tooltip and exactly those of install_paths.
        assert!(w.modes[0].hint.contains(LIB_DIR));
        assert!(w.modes[1].hint.contains(&format!("{HOME}/.local/lib/rexon")));
        let xdg = welcome_texts("arch", HOME, Some("/data/xdg"), Lang::En);
        assert!(xdg.modes[1].hint.contains("/data/xdg/applications"));
        assert_eq!(mode_hint(InstallMode::Portable, HOME, None, Lang::En), "");

        let deck = welcome_texts("steamos", HOME, None, Lang::En);
        assert_eq!(
            Some(deck.modes[0].blocked.clone()),
            system_mode_blocked_reason_in("steamos", Lang::En)
        );
        assert!(deck.modes[1].blocked.is_empty());
    }

    /// The old function without a language says the same as with a language: G1
    /// and Python read it.
    #[test]
    fn blocked_reason_forms_agree() {
        english();
        assert_eq!(
            system_mode_blocked_reason("steamos"),
            system_mode_blocked_reason_in("steamos", Lang::En)
        );
        assert!(system_mode_blocked_reason("steamos")
            .unwrap()
            .starts_with("On this system (for example, Steam Deck)"));
        assert_eq!(system_mode_blocked_reason_in("arch", Lang::En), None);
    }

    #[test]
    fn success_result_says_done_and_offers_to_continue() {
        english();
        let r = result_texts(
            &WizardOutcome::Installed {
                mode: InstallMode::System,
                fell_back: None,
                command_dir_not_in_path: None,
            },
            Lang::En,
        );
        assert!(r.ok);
        assert_eq!(r.title, "Done — Rexon is installed");
        // ⭐ Not "Launch Rexon": the wizard runs inside an already running Rexon.
        assert_eq!(r.primary_button, continue_button(Lang::En));
        assert!(r.secondary_button.is_empty() && r.notes.is_empty() && r.hint.is_empty());
        assert!(r.unavailable.is_empty());

        let dir = format!("{HOME}/.local/bin");
        let r = result_texts(
            &WizardOutcome::Installed {
                mode: InstallMode::User,
                fell_back: system_mode_blocked_reason("steamos"),
                command_dir_not_in_path: Some(dir.clone()),
            },
            Lang::En,
        );
        assert_eq!(r.notes.len(), 2);
        assert_eq!(r.notes[1], command_path_notice(Lang::En));
        assert!(r.hint.contains(&dir));
    }

    /// A password refusal and a step failure are different outcomes; neither is
    /// "cancelled by the user".
    #[test]
    fn failures_are_told_apart() {
        english();
        let failures = [
            InstallFailure::PasswordDenied,
            InstallFailure::PasswordPromptMissing,
            InstallFailure::TimedOut { secs: 300 },
            InstallFailure::StepFailed { step: InstallStep::Dependencies, detail: "x".into() },
        ];
        let lang = Lang::En;
        let titles: std::collections::HashSet<String> =
            failures.iter().map(|f| f.title(lang)).collect();
        assert_eq!(titles.len(), failures.len(), "every outcome has its own title");
        for f in &failures {
            let r = result_texts(&WizardOutcome::Failed(f.clone()), lang);
            assert!(!r.ok);
            assert_eq!(r.primary_button, retry_button(lang));
            assert_eq!(r.secondary_button, continue_without_install_button(lang));
            assert_eq!(r.unavailable, without_install_limits(lang));
            assert!(!r.notes.is_empty(), "what to do is said: {f:?}");
            let all = format!("{} {}", r.title, r.description).to_lowercase();
            assert!(!all.contains("cancel"), "{all}");
        }
        assert!(InstallFailure::PasswordDenied.title(lang).starts_with("Password"));
        // The first line of the shared password refusal is the same as in the whole product.
        assert_eq!(
            InstallFailure::PasswordPromptMissing.problem(lang),
            crate::system_plan::pkexec_missing_message().split('\n').next().unwrap()
        );
        // Raw output — only in the details.
        let step = InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: "exit 1".into(),
        };
        let r = result_texts(&WizardOutcome::Failed(step.clone()), lang);
        assert_eq!(r.details, "exit 1");
        assert!(!r.details_title.is_empty());
        assert!(!r.description.contains("exit 1"));
        assert!(r.description.contains("installing into the system"));
        let r = result_texts(&WizardOutcome::Failed(InstallFailure::PasswordDenied), lang);
        assert!(r.details.is_empty() && r.details_title.is_empty());
        assert!(InstallFailure::TimedOut { secs: 300 }.problem(lang).contains("5 min"));
        assert!(InstallFailure::TimedOut { secs: 1 }.problem(lang).contains("1 min"));
    }

    #[test]
    fn consent_is_off_by_default_and_honest() {
        english();
        assert!(!PASSWORDLESS_CONSENT_DEFAULT);
        let c = passwordless_consent_texts(Lang::En);
        assert!(!c.default_on);
        assert!(c.who.contains("administrators group"));
        assert!(c.risk.contains("without your confirmation"));
        // ⭐ The menu item is named, with exactly the label it has in the menu — in
        // the same language as the whole phrase (29.09.2026: the Russian phrase
        // carried an English label, the English one didn't name the item at all).
        let item = crate::window_menu::menu_items()
            .into_iter()
            .find(|i| i.id == crate::window_menu::ID_UNINSTALL)
            .expect("the uninstall menu item exists")
            .label;
        assert!(item.contains("Uninstall Rexon"), "{item}");
        assert!(c.undo.contains(&format!("“{item}”")), "{}", c.undo);
        assert!(c.undo.contains(crate::window_menu::MENU_BUTTON), "{}", c.undo);
        assert!(c.hint.contains(crate::system_plan::REXON_POLKIT_RULES_PATH));
        // The rule the text speaks of: the family's administrators group, local
        // only, without the package helper.
        let rule = crate::system_plan::passwordless_rules_js(PkgFamily::Arch);
        assert!(rule.contains("isInGroup(\"wheel\")") && rule.contains("subject.local"));
        assert!(c.hint.contains("wheel"));
        let deb = crate::system_plan::passwordless_rules_js(PkgFamily::Debian);
        assert!(deb.contains("isInGroup(\"sudo\")") && !deb.contains("\"wheel\""));
        assert!(!rule.contains(crate::system_plan::PKG_ACTION));
        assert!(c.what.contains("still ask for the password"), "{}", c.what);
        // 28.09.2026: login and boot screen themes were removed from the rule — the
        // consent text doesn't promise them "without a password" by a single word.
        assert!(!c.risk.to_lowercase().contains("theme"), "{}", c.risk);
        // The rule is removed by uninstalling Rexon — otherwise "how to undo it" lies.
        let sys = install_paths(InstallMode::System, HOME, None);
        assert!(uninstall_system_paths(&sys)
            .contains(&crate::system_plan::REXON_POLKIT_RULES_PATH.to_string()));
    }

    /// ⛔ Uninstalling Rexon removes both the privilege session helper and its
    /// policy (B11): otherwise a root helper with a polkit action that can still be
    /// raised after the program is gone would remain in the system.
    #[test]
    fn uninstall_removes_session_helper_and_policy() {
        let sys = install_paths(InstallMode::System, HOME, None);
        let paths = uninstall_system_paths(&sys);
        for p in [
            crate::system_plan::SESSION_HELPER_PATH,
            crate::system_plan::SESSION_POLICY_PATH,
            crate::steamos_sysext::SESSION_HELPER_PATH,
            crate::steamos_sysext::SESSION_POLICY_PATH,
        ] {
            assert!(paths.contains(&p.to_string()), "{p} is not removed");
            assert!(uninstall_system_script(&sys).contains(&format!("rm -rf {p}\n")), "{p}");
        }
    }

    #[test]
    fn steps_have_distinct_lines() {
        english();
        let lines: std::collections::HashSet<String> =
            InstallStep::ALL.iter().map(|s| s.progress_line(Lang::En)).collect();
        assert_eq!(lines.len(), InstallStep::ALL.len());
        assert_eq!(
            InstallStep::CopyFiles.progress_line(Lang::En),
            "Preparing the program files…"
        );
    }

    #[test]
    fn command_dir_in_path_ignores_trailing_slash() {
        let dir = "/home/tester/.local/bin";
        assert!(command_dir_in_path(dir, Some("/usr/bin:/home/tester/.local/bin/")));
        assert!(command_dir_in_path("/home/tester/.local/bin/", Some("/home/tester/.local/bin")));
        assert!(!command_dir_in_path(dir, Some("/usr/bin:/home/tester/.local/binx")));
        assert!(!command_dir_in_path(dir, None));
        assert!(!command_dir_in_path("", Some("")));
    }
}

// ============================================================================
// The login and boot screen theme helper: what it lets into the system
// ============================================================================
//
// ⛔ The checks stand where a real hole was (28.09.2026): plymouth builds the
// splash module path as `<modules dir>/<ModuleName>.so` without checking for
// `../`, and a theme with a `.so` inside gave root code on every boot. The
// helper is run with its REAL text, as an ordinary user and only up to the
// "Install" line — the test writes nothing into the system.

#[cfg(test)]
mod system_helper_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A theme directory under /tmp (the helper accepts only /tmp and /var/tmp).
    fn theme_dir(tag: &str) -> PathBuf {
        let dir = PathBuf::from("/tmp").join(format!(
            "rexon-system-helper-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, body: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// The helper text before the install, then a "checks passed" mark.
    fn validation_part() -> String {
        let full = system_helper_script();
        let cut = full.find("# ── Install").expect("раздел установки в помощнике");
        format!("{}echo REXON_VALIDATED\nexit 0\n", &full[..cut])
    }

    fn uid() -> String {
        let out = Command::new("id").arg("-u").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// (whether the check passed, the helper's stderr).
    fn run_helper(category: &str, theme_id: &str, staged: &Path, pkexec_uid: &str) -> (bool, String) {
        let out = Command::new("bash")
            .arg("-c")
            .arg(validation_part())
            .arg("rexon-system-helper")
            .args([category, theme_id, &staged.to_string_lossy(), "arch"])
            .env("PKEXEC_UID", pkexec_uid)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let ok = out.status.success() && stdout.contains("REXON_VALIDATED");
        (ok, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    fn good_plymouth(dir: &Path) {
        write(dir, "neon.plymouth", b"[Plymouth Theme]\nName=Neon\nModuleName=script\n\n[script]\nScriptFile=/usr/share/plymouth/themes/neon/neon.script\n");
        write(dir, "neon.script", b"Window.SetBackgroundTopColor(0, 0, 0);\n");
        write(dir, "img/logo.png", b"\x89PNG");
    }

    /// Both guards — the root helper and the pre-password check — answer the same.
    fn both_refuse(tag: &str, category: &str, theme_id: &str, build: impl Fn(&Path)) -> String {
        let dir = theme_dir(tag);
        build(&dir);
        let (ok, err) = run_helper(category, theme_id, &dir, &uid());
        let early = system_theme_problem(category, &dir, Some(theme_id));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!ok, "{tag}: the helper let the theme through");
        assert!(early.is_some(), "{tag}: the pre-password check let the theme through");
        err
    }

    #[test]
    fn stock_plymouth_theme_passes() {
        let dir = theme_dir("good");
        good_plymouth(&dir);
        let (ok, err) = run_helper("plymouth", "neon", &dir, &uid());
        assert!(ok, "{err}");
        assert_eq!(system_theme_problem("plymouth", &dir, Some("neon")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn module_from_theme_via_dots_rejected() {
        let err = both_refuse("dotdot", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName=../../../share/plymouth/themes/neon/evil\n");
            write(d, "evil.so", b"\x7fELF");
        });
        assert!(err.contains("library") || err.contains("ModuleName"), "{err}");
        // Without a .so — still a refusal: the module name must be a stock one.
        both_refuse("dotdot-noso", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName=../evil\n");
        });
    }

    /// plymouth's parser (`fscanf " %m[^= \t\n] = %m[^\n] "`) tolerates a newline
    /// between the key and `=`: the line-by-line check must see that.
    #[test]
    fn key_and_value_on_different_lines_rejected() {
        both_refuse("split", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName\n= ../evil\n");
        });
        both_refuse("case", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nmodulename=script\n");
        });
        both_refuse("nul", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName=script\0/../evil\n");
        });
        both_refuse("second", "plymouth", "neon", |d| {
            good_plymouth(d);
            write(d, "alt/other.plymouth", b"[Plymouth Theme]\nModuleName=/tmp/evil\n");
        });
        both_refuse("nomodule", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nName=Neon\n");
        });
    }

    #[test]
    fn library_in_boot_theme_rejected() {
        for name in ["lib/two-step.so", "x.so.1"] {
            both_refuse("so", "plymouth", "neon", |d| {
                good_plymouth(d);
                write(d, name, b"\x7fELF");
            });
        }
    }

    #[test]
    fn symlink_in_theme_rejected() {
        for category in ["sddm", "plymouth"] {
            let err = both_refuse("link", category, "neon", |d| {
                good_plymouth(d);
                write(d, "Main.qml", b"import QtQuick 2.0\nItem {}\n");
                symlink("/etc/shadow", d.join("shadow")).unwrap();
            });
            assert!(err.contains("link"), "{err}");
        }
    }

    /// An SDDM theme runs as the `sddm` user, not root: a QML module in it is no
    /// reason to refuse.
    #[test]
    fn sddm_theme_with_qml_module_passes() {
        let dir = theme_dir("sddm");
        write(&dir, "Main.qml", b"import QtQuick 2.0\nItem {}\n");
        write(&dir, "plugin/libx.so", b"\x7fELF");
        let (ok, err) = run_helper("sddm", "neon", &dir, &uid());
        assert!(ok, "{err}");
        assert_eq!(system_theme_problem("sddm", &dir, Some("neon")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The directory — by where we entered, not by the path string; a foreign one — refusal.
    #[test]
    fn foreign_or_external_directory_rejected() {
        let dir = theme_dir("where");
        good_plymouth(&dir);
        // A link under /tmp to a directory outside /tmp.
        let link = PathBuf::from("/tmp").join(format!("rexon-system-helper-test-lnk-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        symlink("/usr", &link).unwrap();
        let (ok, err) = run_helper("plymouth", "neon", &link, &uid());
        assert!(!ok && err.contains("outside /tmp"), "{err}");
        let _ = std::fs::remove_file(&link);
        // The directory owner is not the caller.
        let (ok, err) = run_helper("plymouth", "neon", &dir, "0");
        assert!(!ok && err.contains("does not belong to the caller"), "{err}");
        // Without pkexec — refusal.
        let out = Command::new("bash")
            .arg("-c")
            .arg(validation_part())
            .arg("rexon-system-helper")
            .args(["plymouth", "neon", &dir.to_string_lossy(), "arch"])
            .env_remove("PKEXEC_UID")
            .output()
            .unwrap();
        assert!(!out.status.success());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_name_not_flag_and_not_root() {
        let dir = theme_dir("names");
        good_plymouth(&dir);
        for bad in ["-R", ".", "..", "a/b", "", ".hidden", "a b"] {
            let (ok, _) = run_helper("plymouth", bad, &dir, &uid());
            assert!(!ok, "name {bad:?} let through");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The modules directory and the list itself — one truth in the helper and in Rust.
    #[test]
    fn helper_module_list_from_core() {
        let text = system_helper_script();
        assert!(text.contains(&PLYMOUTH_SPLASH_MODULES.join("|")));
        assert!(!text.contains("__PLY_MODULES__"));
        for m in PLYMOUTH_SPLASH_MODULES {
            assert!(m.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{m}");
        }
        // The helper executes nothing from the theme and doesn't accept a destination path.
        for banned in ["eval ", "source ", "bash -c", "sh -c"] {
            assert!(!text.contains(banned), "{banned} in the helper");
        }
    }

    // ── «Remove» mode (remove-<kind>) in a substituted root ───────────
    //
    // The helper is run IN FULL, as an ordinary user: the text's system paths
    // (`/usr/share/`, `/etc/`, `/usr/lib/sddm`) are rewritten to a sandbox under
    // /tmp, package managers are stubs at the front of PATH. The test doesn't
    // touch the live `/usr`.

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new(tag: &str) -> Self {
            let root = theme_dir(&format!("rm-{tag}"));
            let stub = root.join("stub");
            std::fs::create_dir_all(&stub).unwrap();
            // "The package owns" only the pkgtheme directory.
            write(&stub, "pacman", b"#!/bin/bash\ncase \"${@: -1}\" in */pkgtheme) exit 0 ;; esac\nexit 1\n");
            write(&stub, "dpkg-query", b"#!/bin/bash\nexit 1\n");
            write(&stub, "rpm", b"#!/bin/bash\nexit 1\n");
            for f in ["pacman", "dpkg-query", "rpm"] {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(stub.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            for kind in ["plymouth", "sddm"] {
                std::fs::create_dir_all(root.join(format!("usr/share/{kind}/themes"))).unwrap();
            }
            Sandbox { root }
        }

        fn theme(&self, kind: &str, name: &str) -> PathBuf {
            let d = self.root.join(format!("usr/share/{kind}/themes/{name}"));
            write(&d, &format!("{name}.plymouth"), b"[Plymouth Theme]\nModuleName=script\n");
            d
        }

        fn script(&self) -> String {
            let r = self.root.to_string_lossy();
            system_helper_script()
                .replace("/usr/share/", &format!("{r}/usr/share/"))
                .replace("/etc/", &format!("{r}/etc/"))
                .replace("/usr/lib/sddm", &format!("{r}/usr/lib/sddm"))
                .replace("export PATH=/usr/sbin", &format!("export PATH={r}/stub:/usr/sbin"))
        }

        /// (success, stderr).
        fn remove(&self, category: &str, theme: &str, staged: &str) -> (bool, String) {
            let out = Command::new("bash")
                .arg("-c")
                .arg(self.script())
                .arg("rexon-system-helper")
                .args([category, theme, staged, "arch"])
                .env("PKEXEC_UID", uid())
                .output()
                .unwrap();
            (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn removal_takes_only_own_directory() {
        let sb = Sandbox::new("ok");
        let neon = sb.theme("plymouth", "neon");
        let other = sb.theme("plymouth", "other");
        // A link inside the theme pointing outside: the link itself is removed, not what it points to.
        let outside = sb.root.join("outside");
        write(&outside, "keep.txt", b"keep");
        symlink(&outside, neon.join("out")).unwrap();
        let (ok, err) = sb.remove("remove-plymouth", "neon", "-");
        assert!(ok, "{err}");
        assert!(!neon.exists(), "theme not removed");
        assert!(other.is_dir(), "neighbouring theme touched");
        assert!(outside.join("keep.txt").is_file(), "removal followed a link outside");
        let sddm = sb.theme("sddm", "sugar");
        let (ok, err) = sb.remove("remove-sddm", "sugar", "-");
        assert!(ok && !sddm.exists(), "{err}");
    }

    #[test]
    fn removal_of_selected_theme_rejected() {
        let sb = Sandbox::new("active");
        let neon = sb.theme("plymouth", "neon");
        write(&sb.root, "etc/plymouth/plymouthd.conf", b"[Daemon]\nTheme = neon\n");
        let (ok, err) = sb.remove("remove-plymouth", "neon", "-");
        assert!(!ok && neon.is_dir() && err.contains("selected right now"), "{err}");
        // A choice via update-alternatives: default.plymouth leads into the theme.
        let sb = Sandbox::new("alt");
        let neon = sb.theme("plymouth", "neon");
        symlink(neon.join("neon.plymouth"), sb.root.join("usr/share/plymouth/themes/default.plymouth")).unwrap();
        let (ok, err) = sb.remove("remove-plymouth", "neon", "-");
        assert!(!ok && neon.is_dir(), "{err}");
        // SDDM: any settings file, including system defaults.
        for conf in ["etc/sddm.conf.d/kde_settings.conf", "usr/lib/sddm/sddm.conf.d/default.conf", "etc/sddm.conf"] {
            let sb = Sandbox::new("sddm-active");
            let sugar = sb.theme("sddm", "sugar");
            write(&sb.root, conf, b"[Theme]\r\n  Current=sugar  \r\n");
            let (ok, err) = sb.remove("remove-sddm", "sugar", "-");
            assert!(!ok && sugar.is_dir() && err.contains("selected right now"), "{conf}: {err}");
        }
        // A similar name is not the same theme.
        let sb = Sandbox::new("similar");
        let sugar = sb.theme("sddm", "sugar");
        write(&sb.root, "etc/sddm.conf", b"[Theme]\nCurrent=sugar-dark\n");
        let (ok, err) = sb.remove("remove-sddm", "sugar", "-");
        assert!(ok && !sugar.exists(), "{err}");
    }

    #[test]
    fn removal_outside_themes_directory_rejected() {
        let sb = Sandbox::new("guard");
        // A system package's directory.
        let pkg = sb.theme("plymouth", "pkgtheme");
        let (ok, err) = sb.remove("remove-plymouth", "pkgtheme", "-");
        assert!(!ok && pkg.is_dir() && err.contains("package"), "{err}");
        // A theme that is a link to a foreign directory.
        let target = sb.root.join("elsewhere");
        write(&target, "x", b"x");
        symlink(&target, sb.root.join("usr/share/sddm/themes/lnk")).unwrap();
        let (ok, err) = sb.remove("remove-sddm", "lnk", "-");
        assert!(!ok && target.join("x").is_file() && err.contains("not installed"), "{err}");
        // No such theme.
        let (ok, _) = sb.remove("remove-sddm", "absent", "-");
        assert!(!ok);
        // A directory instead of a dash, invalid names, a foreign kind.
        let keep = sb.theme("plymouth", "neon");
        let (ok, _) = sb.remove("remove-plymouth", "neon", "/tmp");
        assert!(!ok && keep.is_dir());
        for bad in ["..", ".", "-R", "a/b", "../themes", "", "neon/.."] {
            let (ok, _) = sb.remove("remove-plymouth", bad, "-");
            assert!(!ok, "name {bad:?} let through");
        }
        let (ok, _) = sb.remove("remove-gtk", "neon", "-");
        assert!(!ok && keep.is_dir());
        assert!(sb.root.join("usr/share/plymouth/themes").is_dir());
    }

    #[test]
    fn human_refusal_text() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let m = SystemThemeProblem::BootSplashCode("evil.so".into()).message();
        assert!(m.contains("evil.so") && m.contains("Nothing was changed"), "{m}");
        let m = SystemThemeProblem::OddFile("shadow".into()).message();
        assert!(m.contains("shadow"), "{m}");
    }
}
