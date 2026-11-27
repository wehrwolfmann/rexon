//! Desktops on SteamOS (Steam Deck and Steam Machine) via `systemd-sysext`
//! extension images.
//!
//! WHY. On SteamOS the system partition `/usr` is read-only, and an OS update
//! replaces it whole and wipes everything installed on top. So a desktop is not
//! installed by the package manager but built as an IMAGE: the missing packages
//! from Valve's repositories of the same OS version are unpacked into a squashfs
//! image in `/home/.rexon/sysext`, a link to it goes into `/var/lib/extensions`,
//! and `systemd-sysext` overlays it on top of `/usr`. The image is attached only
//! on an EXACT `VERSION_ID` match, so after each OS update a service rebuilds
//! it; until the rebuild finishes Plasma works, and a guard before the login
//! screen keeps autologin from running into a vanished session.
//! The mechanics were verified in a SteamOS VM on real updates 3.8.14 → 3.9.5.
//!
//! THE SPLIT is as with the neighbours (`system_plan` → `system_de_exec`): **the
//! core decides, the executor does**.
//!   * pure functions: parsing os-release and `passwd -S`, the desktops table,
//!     the list of installed ones, install/remove/switch plans, texts;
//!   * texts of scripts and units that run INSIDE SteamOS as root;
//!   * a thin executor (`*_live`) — running the helper via `pkexec`, progress
//!     into the operation window, `steamosctl` as the user.
//!
//! ⚠️ WHY THE BOOT PART IS bash SCRIPTS AND NOT RUST (rule #16).
//! After an OS update Rexon itself on SteamOS also lives in an extension image,
//! and its image is likewise not attached until the rebuild — i.e. exactly when
//! the rebuild service is needed, there is no Rexon binary on the system. Our
//! own program in `/var/lib/rexon` would depend on the glibc of the new OS (a
//! build on a fresh Arch doesn't guarantee running on Valve's image). The
//! scripts call only what is in the SteamOS image (`pacman`, `bsdtar`,
//! `readelf`, `mksquashfs`), and survive any update. So their TEXT is generated
//! by Rust (this module, with checks on the text and on the behaviour under
//! stubs), and they live in `/var/lib/rexon/bin` and `/etc/systemd/system` —
//! both places survive an update (the whitelist `*.service`, `*.wants/**`).
//!
//! ⛔ SECURITY. The root helper (`rexon-sysext-helper`) accepts only an
//! operation from a list and a desktop name from the table; it takes packages
//! only from the signed OS repositories (its own keyring `archlinux` + `holo`);
//! it accepts no paths or package lists from the UI. The first run (or after a
//! Rexon update) goes as `pkexec /bin/bash -c <text>`: the text is in argv, and
//! there is nothing to swap it with after the password prompt. After that — the
//! helper file in `/var/lib/rexon/bin` (root:root) with its own polkit policy.
//!
//! The privilege session helper (`rexon-priv-helper`, "one password per run")
//! on SteamOS also lives in `/var/lib/rexon/bin`, the policy
//! `org.rexon.session` is next to this helper's policy in `/etc` (and in the
//! update whitelist). ONLY the `bash -c` path puts it there: the window passes
//! the path of the delivery binary and its sha256, root copies the file to
//! itself, checks the sum and renames it ([`SessionHelperCopy`]).

use std::sync::Arc;

use crate::de_progress;
use crate::i18n::{tr, trf};
use crate::system_de_exec::{CancelToken, DeOpOutcome, DeOpSink};
use crate::system_l10n::bash_quote;

// ============================================================================
// Paths (one truth for the scripts, the helper and the UI)
// ============================================================================

/// Rexon state directory on SteamOS — survives OS updates.
pub const STATE_DIR: &str = "/var/lib/rexon";
/// Scripts and the helper.
pub const BIN_DIR: &str = "/var/lib/rexon/bin";
/// List of desktops to build — one identifier per line.
pub const DESKTOPS_LIST: &str = "/var/lib/rexon/sysext/desktops";
/// Language of the script messages (a code from `po/LINGUAS`), written by the helper.
pub const LANG_FILE: &str = "/var/lib/rexon/sysext/lang";
/// The session the guard replaced with Plasma and that has to be restored.
pub const WANTED_SESSION: &str = "/var/lib/rexon/session-guard/wanted-session";
/// Images (SteamOS has only ~230 MB in `/var` — the images live in `/home`).
pub const IMAGE_DIR: &str = "/home/.rexon/sysext";
/// Image build directories (`<desktop>-<version>`).
pub const BUILD_DIR: &str = "/home/.rexon/build";
/// Links to the images that `systemd-sysext` sees.
pub const EXTENSIONS_DIR: &str = "/var/lib/extensions";
/// The root helper.
pub const HELPER_PATH: &str = "/var/lib/rexon/bin/rexon-sysext-helper";
/// Helper name — for messages and `$0` when run via `bash -c`.
pub const HELPER_NAME: &str = "rexon-sysext-helper";
/// The helper's polkit policy (polkit ≥ 126 reads `/etc/polkit-1/actions`;
/// verified on SteamOS 3.9.5, polkit 127).
pub const HELPER_POLICY_PATH: &str = "/etc/polkit-1/actions/org.rexon.sysext.policy";
/// The helper's polkit action.
pub const HELPER_ACTION: &str = "org.rexon.sysext";
/// The privilege session helper ("one password per run") on SteamOS: `/usr` is
/// closed, and `/var` survives an OS update. The Rexon delivery binary —
/// `rexon-sysext-helper` puts it there when run via `bash -c`
/// ([`SessionHelperCopy`]); the client looks for it right here
/// (`privilege::client::STEAMOS_HELPER_DIR`).
pub const SESSION_HELPER_PATH: &str = "/var/lib/rexon/bin/rexon-priv-helper";
/// The `org.rexon.session` policy for the helper in `/var` — in the same place
/// as the `rexon-sysext-helper` policy (polkit ≥ 126 reads `/etc`).
pub const SESSION_POLICY_PATH: &str = "/etc/polkit-1/actions/org.rexon.session.policy";
/// Addition to the `/etc` whitelist that an OS update preserves.
pub const KEEP_CONF_PATH: &str = "/etc/atomic-update.conf.d/rexon.conf";
/// Session guard unit (before the login screen).
pub const GUARD_UNIT: &str = "rexon-session-guard.service";
/// Rebuild unit (after the network).
pub const SYNC_UNIT: &str = "rexon-sysext-sync.service";
/// The Plasma session — what any SteamOS image has.
pub const PLASMA_SESSION: &str = "plasma.desktop";
/// Name of the shared caches image. ⚠️ `systemd-sysext` stacks layers by name:
/// a greater name is the upper layer, hence `zz`.
pub const CACHES_NAME: &str = "rexon-zz-caches";
/// Autologin files written by `steamos-manager` (3.8 — `steamos`, after the
/// update to 3.9 the file is renamed to `holo`).
pub const AUTOLOGIN_CONFS: [&str; 2] = [
    "/etc/sddm.conf.d/zz-holo-autologin.conf",
    "/etc/sddm.conf.d/zz-steamos-autologin.conf",
];
/// Exit code "no network, retry later" (EX_TEMPFAIL).
pub const EXIT_NO_NETWORK: i32 = 75;
/// How long to wait for `steamosctl`: it hangs when the user session is closed
/// or in a restart loop (verified in a VM).
pub const STEAMOSCTL_TIMEOUT_SECS: u64 = 15;

// ============================================================================
// os-release and the device
// ============================================================================

/// Parsing os-release: "key, value" pairs without quotes, comments and empty
/// lines are skipped.
pub fn parse_os_release(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|x| x.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
            .unwrap_or(v);
        out.push((k.trim().to_string(), v.to_string()));
    }
    out
}

/// What Rexon knows about the SteamOS under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamOsRelease {
    /// `VERSION_ID` — `systemd-sysext` matches the image by it (3.9.5).
    pub version_id: String,
    /// `VARIANT_ID`: `steamdeck`, `steamdeck-oobe` (the recovery image) etc.
    /// Decisions don't rely on it: on the Steam Machine it is not confirmed on the
    /// device, and both machines have the same OS.
    pub variant_id: String,
    /// `BUILD_ID` — for the log.
    pub build_id: String,
}

/// Whether this is SteamOS (Steam Deck or Steam Machine) — by `/etc/os-release`.
///
/// ⚠️ Exactly `/etc/os-release`: the SteamOS `/usr/lib/os-release` has no
/// `VERSION_ID` (verified in a VM), and `systemd-sysext` reads `/etc`.
/// Without `VERSION_ID` there is nothing to build the image for — then `None`.
pub fn steamos_release(etc_os_release: &str) -> Option<SteamOsRelease> {
    let kv = parse_os_release(etc_os_release);
    let get = |key: &str| {
        kv.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    if !crate::distro_compat::is_steamos_id(&get("ID")) {
        return None;
    }
    let version_id = get("VERSION_ID");
    if !is_safe_version(&version_id) {
        return None;
    }
    Some(SteamOsRelease { version_id, variant_id: get("VARIANT_ID"), build_id: get("BUILD_ID") })
}

/// The version is fit for a file name and a script: letters, digits, `.`, `_`, `-`.
pub fn is_safe_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The machine under SteamOS — only for the words in the window (the mechanism is the same for both).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    SteamDeck,
    SteamMachine,
    /// Other hardware or not recognized (a VM, a PC with a SteamOS image).
    Unknown,
}

/// The machine by DMI (`/sys/class/dmi/id/board_name`, `product_name`):
/// Jupiter — Steam Deck LCD, Galileo — Steam Deck OLED, Fremont — Steam
/// Machine (the device list of `steamos-manager` 3.9).
pub fn device_from_dmi(board_name: &str, product_name: &str) -> Device {
    for v in [board_name.trim(), product_name.trim()] {
        match v.to_ascii_lowercase().as_str() {
            "jupiter" | "galileo" => return Device::SteamDeck,
            "fremont" => return Device::SteamMachine,
            _ => {}
        }
    }
    Device::Unknown
}

/// Machine name for the window texts.
pub fn device_label(device: Device) -> String {
    match device {
        Device::SteamDeck => "Steam Deck".to_string(),
        Device::SteamMachine => "Steam Machine".to_string(),
        // TRANSLATORS: used as %1 in "the administrator password of %1" when the device is neither a recognised Steam Deck nor a Steam Machine.
        Device::Unknown => tr("this SteamOS device"),
    }
}

// ============================================================================
// Password of the deck user
// ============================================================================

/// Password state by `passwd -S` (works without root for one's own user:
/// `deck P …`, `deck NP …`, `deck L …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordState {
    /// A password is set — `pkexec` will ask for it.
    Set,
    /// No password (this is how SteamOS ships from the factory) — `pkexec` can't be passed.
    Missing,
    /// The account is locked.
    Locked,
    /// Couldn't find out — don't get in the way, `pkexec` will say so itself.
    Unknown,
}

/// Parsing the `passwd -S` output.
pub fn parse_passwd_status(stdout: &str) -> PasswordState {
    let Some(line) = stdout.lines().find(|l| !l.trim().is_empty()) else {
        return PasswordState::Unknown;
    };
    match line.split_whitespace().nth(1) {
        Some("P" | "PS") => PasswordState::Set,
        Some("NP") => PasswordState::Missing,
        Some("L" | "LK") => PasswordState::Locked,
        _ => PasswordState::Unknown,
    }
}

/// Whether the password is fine: the two fields match, not empty, no control
/// characters (a newline would cut the `passwd` input). `Err` — text for the
/// window.
pub fn check_new_password(first: &str, second: &str) -> Result<(), String> {
    if first.is_empty() {
        return Err(tr("Enter a password."));
    }
    if first != second {
        return Err(tr("The two passwords are different — type the same password twice."));
    }
    if first.chars().any(|c| c.is_control()) {
        return Err(tr("The password can't contain line breaks or other control characters."));
    }
    Ok(())
}

/// What to feed to `passwd` when there is no password yet: the new password and
/// its repetition (SteamOS doesn't ask a user without a password for the current
/// one — verified in a VM on 3.9.5).
pub fn passwd_input(password: &str) -> String {
    format!("{password}\n{password}\n")
}

// ============================================================================
// Desktops that can be built
// ============================================================================

/// Session type — which directory its `.desktop` goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    X11,
    Wayland,
}

impl SessionKind {
    fn dir(self) -> &'static str {
        match self {
            SessionKind::X11 => "xsessions",
            SessionKind::Wayland => "wayland-sessions",
        }
    }
}

/// A desktop that Rexon can build as an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopSpec {
    /// Core identifier (`system::DE_META`).
    pub id: &'static str,
    /// Name in notifications and in the session.
    pub name: &'static str,
    /// What to install (Arch groups and packages; `--needed` drops what is in the OS).
    pub packages: &'static [&'static str],
    pub session: SessionKind,
    /// What starts the session.
    pub exec: &'static str,
    /// Session variables (name, value pairs).
    pub env: &'static [(&'static str, &'static str)],
    /// Shell command before start and after exit (empty — none).
    pub prestart: &'static str,
    /// Finishing touches for the desktop's XDG directory (`$X`) after moving `/etc/xdg`.
    pub xdg_fixup: &'static str,
}

/// Desktops verified in a SteamOS VM (the packages are in Valve's `extra`).
///
/// ⚠️ When adding a desktop, verify its build in a VM: Valve packages of one
/// series don't always match the image (`libdisplay-info.so.2` for XFCE on
/// 3.8.14).
pub const DESKTOPS: &[DesktopSpec] = &[
    DesktopSpec {
        id: "xfce",
        name: "Xfce",
        // xfce4-screensaver is the screen locker; it is not in the xfce4 group.
        packages: &[
            "xfce4", "xfce4-screensaver", "xfce4-whiskermenu-plugin", "xfce4-pulseaudio-plugin",
            "xfce4-screenshooter", "mousepad", "ristretto",
        ],
        session: SessionKind::X11,
        // ⛔ Not startxfce4: it hard-codes looking for /etc/xdg/xfce4/xinitrc.
        exec: "xfce4-session",
        env: &[("XDG_CURRENT_DESKTOP", "XFCE"), ("XDG_MENU_PREFIX", "xfce-"), ("DESKTOP_SESSION", "xfce")],
        // xfconfd is started by D-Bus activation with the previous session's
        // environment: without a restart — "Unable to load a failsafe session".
        prestart: "pkill -u \"$(id -u)\" -x xfconfd || true",
        xdg_fixup: "",
    },
    DesktopSpec {
        id: "lxqt",
        name: "LXQt",
        // openbox is named explicitly: lxqt-session without a window manager asks for
        // one on the first login.
        packages: &["lxqt", "openbox"],
        session: SessionKind::X11,
        exec: "startlxqt",
        env: &[("XDG_CURRENT_DESKTOP", "LXQt"), ("DESKTOP_SESSION", "lxqt")],
        prestart: "",
        xdg_fixup: "f=\"$X/lxqt/session.conf\"; mkdir -p \"${f%/*}\"\n\
                    [ -f \"$f\" ] || printf '[General]\\n' > \"$f\"\n\
                    grep -q '^window_manager=' \"$f\" || sed -i '/^\\[General\\]/a window_manager=openbox' \"$f\"",
    },
    DesktopSpec {
        id: "hyprland",
        name: "Hyprland",
        packages: &["hyprland", "xdg-desktop-portal-hyprland", "kitty", "wofi", "waybar"],
        session: SessionKind::Wayland,
        exec: "Hyprland",
        env: &[("XDG_CURRENT_DESKTOP", "Hyprland"), ("XDG_SESSION_DESKTOP", "Hyprland")],
        prestart: "",
        xdg_fixup: "",
    },
];

/// Variables that the session wrapper puts into the user manager and removes on
/// exit — the union of `env` of all desktops plus XDG_CONFIG_DIRS.
const SESSION_VARS: &str = "XDG_CONFIG_DIRS XDG_CURRENT_DESKTOP XDG_MENU_PREFIX DESKTOP_SESSION XDG_SESSION_DESKTOP";

/// OS packages the image must NEVER carry (it would shadow the SteamOS core:
/// drivers, boot, the login screen, Plasma, Steam).
const CORE_PACKAGES_RE: &str =
    "^(glibc|mesa|lib32-mesa|vulkan-radeon|linux[^ ]*|systemd[^ ]*|sddm|plasma-[^ ]*|kwin|qt6-base|gamescope|steam[^ ]*) ";

/// Desktop description by identifier.
pub fn desktop(de_id: &str) -> Option<&'static DesktopSpec> {
    DESKTOPS.iter().find(|d| d.id == de_id)
}

/// File name of the desktop's session (`rexon-xfce.desktop`).
pub fn session_file(de_id: &str) -> String {
    format!("rexon-{de_id}.desktop")
}

/// Which session to choose by default for a core desktop: Plasma — the stock
/// one, built ones — ours; others — `None`.
pub fn session_for(de_id: &str) -> Option<String> {
    if de_id == "kde" {
        return Some(PLASMA_SESSION.to_string());
    }
    desktop(de_id).map(|d| session_file(d.id))
}

/// Whether the desktop can be installed on SteamOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    Supported(&'static DesktopSpec),
    /// Plasma is the SteamOS desktop itself: nothing to install.
    Builtin,
    /// Not possible yet; the text says why, for the window.
    NotYet(String),
}

/// The decision "is this desktop possible on SteamOS" — one for the product.
pub fn support(de_id: &str) -> Support {
    if let Some(d) = desktop(de_id) {
        return Support::Supported(d);
    }
    if de_id == "kde" {
        return Support::Builtin;
    }
    let names: Vec<&str> = DESKTOPS.iter().map(|d| d.name).collect();
    Support::NotYet(trf(
        // TRANSLATORS: %1 is a comma-separated list of desktop names, e.g. "Xfce, LXQt, Hyprland".
        "On SteamOS Rexon builds a desktop as a system extension from Valve's own package \
         sources, and so far only %1 have been checked this way. This desktop hasn't been \
         checked yet: it may need parts that SteamOS keeps for itself (login screen, \
         drivers), so Rexon doesn't offer it.",
        &[&names.join(", ")],
    ))
}

// ============================================================================
// Desktops list (`/var/lib/rexon/sysext/desktops`)
// ============================================================================

/// Parsing the list: known identifiers in order, without repeats.
pub fn parse_desktop_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let id = line.trim();
        if desktop(id).is_some() && !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    }
    out
}

// ============================================================================
// Scripts running inside SteamOS
// ============================================================================

/// Before the language block: the language from the file if `--lang` is not
/// passed (the systemd unit doesn't know the language — the helper wrote it at
/// install time).
const LANG_FROM_FILE: &str = r#"case "${1:-}" in
    --lang=*) ;;
    *) rx_file_lang=$(head -c 16 "$R/var/lib/rexon/sysext/lang" 2>/dev/null | tr -cd 'a-zA-Z0-9_@') || true
       [ -n "${rx_file_lang:-}" ] && set -- "--lang=$rx_file_lang" "$@" ;;
esac"#;

fn ids_pattern() -> String {
    DESKTOPS.iter().map(|d| d.id).collect::<Vec<_>>().join("|")
}

fn pretty_cases() -> String {
    DESKTOPS
        .iter()
        .map(|d| format!("        {}) echo {} ;;\n", d.id, bash_quote(d.name)))
        .collect()
}

fn desktop_cases() -> String {
    let mut out = String::new();
    for d in DESKTOPS {
        let pkgs = d.packages.join(" ");
        let env: Vec<String> = d.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push_str(&format!(
            "    {id})\n        PKGS=({pkgs})\n        SESSION_DIR={dir}\n        SESSION_NAME={name}\n        \
             SESSION_EXEC={exec}\n        SESSION_ENV={env}\n        SESSION_PRESTART={pre}\n        \
             XDG_FIXUP={fix} ;;\n",
            id = d.id,
            dir = d.session.dir(),
            name = bash_quote(&format!("{} (Rexon)", d.name)),
            exec = bash_quote(d.exec),
            env = bash_quote(&format!("export {}", env.join(" "))),
            pre = bash_quote(d.prestart),
            fix = bash_quote(d.xdg_fixup),
        ));
    }
    out
}

fn fill(name: &str, template: &str) -> String {
    let text = template
        .replace("__LANG_FROM_FILE__", LANG_FROM_FILE)
        .replace("__IDS__", &ids_pattern())
        .replace("__PRETTY__", &pretty_cases())
        .replace("__DESKTOPS__", &desktop_cases())
        .replace("__CORE__", CORE_PACKAGES_RE)
        .replace("__VARS__", SESSION_VARS)
        .replace("__CACHES__", CACHES_NAME)
        .replace("__PLASMA__", PLASMA_SESSION);
    crate::system_l10n::localize_script(name, &text)
}

/// Desktop image builder — `rexon-sysext-build <desktop>`.
pub fn build_script() -> String {
    fill("rexon-sysext-build", BUILD_TMPL)
}

/// Shared caches image builder — `rexon-sysext-caches [--force]`.
pub fn caches_script() -> String {
    fill("rexon-sysext-caches", CACHES_TMPL)
}

/// Rebuild after an OS update — `rexon-sysext-sync`.
pub fn sync_script() -> String {
    fill("rexon-sysext-sync", SYNC_TMPL)
}

/// Session guard before the login screen — `session-guard.sh`.
pub fn guard_script() -> String {
    fill("rexon-session-guard", GUARD_TMPL)
}

/// Guard unit: before SDDM, after the extensions are attached.
pub fn guard_unit() -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: fall back to Plasma if the default desktop session is missing\n\
         DefaultDependencies=no\n\
         After=systemd-sysext.service local-fs.target\n\
         Before=sddm.service display-manager.service\n\
         ConditionPathExists={BIN_DIR}/session-guard.sh\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/bin/bash {BIN_DIR}/session-guard.sh\n\
         \n\
         [Install]\n\
         WantedBy=sddm.service\n"
    )
}

/// Rebuild unit: after the network, NOT before the login screen (login doesn't
/// wait for the build). Code 75 — no network: the unit restarts itself every 2
/// minutes. No timer is needed (and `*.timer` is not in the `/etc` whitelist).
pub fn sync_unit() -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: rebuild desktop system extensions for the current SteamOS version\n\
         After=network-online.target {GUARD_UNIT} systemd-sysext.service\n\
         Wants=network-online.target\n\
         ConditionPathExists={DESKTOPS_LIST}\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={BIN_DIR}/rexon-sysext-sync\n\
         Restart=on-failure\n\
         RestartSec=120\n\
         Nice=10\n\
         IOSchedulingClass=idle\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

/// Whitelist addition: the helper policy in `/etc` survives an OS update (units
/// and `*.wants` are already in Valve's list).
pub fn keep_conf() -> String {
    format!(
        "# Rexon: files under /etc that SteamOS must keep across system updates.\n\
         {HELPER_POLICY_PATH}\n\
         {SESSION_POLICY_PATH}\n\
         {KEEP_CONF_PATH}\n"
    )
}

/// The helper's polkit policy: administrator password, once confirmed it is
/// kept for a few minutes (as with the other Rexon helpers).
pub fn helper_policy_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <action id=\"{HELPER_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <icon_name>preferences-desktop</icon_name>\n\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">{HELPER_PATH}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", HELPER_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", HELPER_POLICY_MESSAGE),
    )
}

const HELPER_POLICY_DESCRIPTION: &str = crate::i18n::n_("Install or remove a desktop environment on SteamOS");
const HELPER_POLICY_MESSAGE: &str =
    crate::i18n::n_("Rexon needs the administrator password to install or remove a desktop environment on SteamOS.");

/// Write a file from the helper's here-doc. The delimiter is unique and is
/// checked by a test: it is not in the text of the embedded file.
fn heredoc(path: &str, mode: &str, tag: &str, body: &str) -> String {
    assert!(!body.contains(tag), "ограничитель {tag} встречается в тексте");
    let body = if body.ends_with('\n') { body.to_string() } else { format!("{body}\n") };
    format!("    put \"$R{path}\" {mode} <<'{tag}'\n{body}{tag}\n")
}

/// A layout file: where, with which permissions and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutFile {
    pub path: String,
    pub mode: u32,
    /// Here-doc delimiter in the `rexon-sysext-helper` text.
    pub tag: &'static str,
    pub text: String,
}

/// Everything the helper lays out before an operation: scripts, units, the
/// polkit policy and the `/etc` whitelist addition. One truth for
/// `rexon-sysext-helper` ([`helper_script`]) and the privilege session helper
/// (`privilege::helper`, operation `Sysext`).
pub fn layout_files() -> Vec<LayoutFile> {
    let f = |path: String, mode: u32, tag: &'static str, text: String| LayoutFile { path, mode, tag, text };
    vec![
        f(format!("{BIN_DIR}/rexon-sysext-build"), 0o755, "REXON_EOF_BUILD", build_script()),
        f(format!("{BIN_DIR}/rexon-sysext-caches"), 0o755, "REXON_EOF_CACHES", caches_script()),
        f(format!("{BIN_DIR}/rexon-sysext-sync"), 0o755, "REXON_EOF_SYNC", sync_script()),
        f(format!("{BIN_DIR}/session-guard.sh"), 0o755, "REXON_EOF_GUARD", guard_script()),
        f(format!("/etc/systemd/system/{GUARD_UNIT}"), 0o644, "REXON_EOF_GUARD_UNIT", guard_unit()),
        f(format!("/etc/systemd/system/{SYNC_UNIT}"), 0o644, "REXON_EOF_SYNC_UNIT", sync_unit()),
        f(HELPER_POLICY_PATH.to_string(), 0o644, "REXON_EOF_POLICY", helper_policy_xml()),
        // The session helper policy — the same text as the package's, only its own
        // path (`/var`). Without the binary it is harmless: the action launches only
        // the file at `exec.path`.
        f(
            SESSION_POLICY_PATH.to_string(),
            0o644,
            "REXON_EOF_SESSION_POLICY",
            crate::system_plan::session_policy_xml_for(SESSION_HELPER_PATH),
        ),
        f(KEEP_CONF_PATH.to_string(), 0o644, "REXON_EOF_KEEP", keep_conf()),
    ]
}

/// The root helper — `rexon-sysext-helper [--lang=code] install|remove <desktop>`.
///
/// Carries the texts of all scripts and units (embedded at generation time):
/// from the UI it accepts only the operation and a desktop name from the table.
pub fn helper_script() -> String {
    let layout: String = layout_files()
        .iter()
        .map(|f| heredoc(&f.path, &format!("{:o}", f.mode), f.tag, &f.text))
        .collect();
    let text = HELPER_TMPL
        .replace("__LAYOUT__", &layout)
        .replace("__HELPER_PATH__", HELPER_PATH)
        .replace("__SESSION_HELPER_PATH__", SESSION_HELPER_PATH);
    fill(HELPER_NAME, &text)
}

// ── Template texts ──────────────────────────────────────────────────────────
//
// `$R` is the root for tests (the REXON_SYSEXT_ROOT variable; pkexec and
// systemd clear the environment, it can't be injected from outside). Empty in
// the product.

const BUILD_TMPL: &str = r###"#!/bin/bash
# rexon-sysext-build <desktop> — builds the systemd-sysext image of a desktop
# for SteamOS (Steam Deck and Steam Machine). Generated by Rexon; any edit here
# is overwritten the next time a desktop is installed.
# Image: /home/.rexon/sysext/rexon-<desktop>-<VERSION_ID>.raw, link in
# /var/lib/extensions. Packages come from Valve's repositories of the SAME OS
# version, only those missing from the OS image. Runs as root.
# Exit: 0 built or nothing to do; 75 no network (retry later); 2 bad call;
# 3 would shadow the OS; 4 libraries missing; 1 anything else.
set -euo pipefail
R="${REXON_SYSEXT_ROOT:-}"
__LANG_FROM_FILE__
__L10N__
progress() { if [ "${REXON_SYSEXT_PROGRESS:-0}" = 1 ]; then echo "@progress $1"; fi; }
DE="${1:-}"
case "$DE" in __IDS__) ;; *) die sx_bad_desktop "$DE" ;; esac
# ⛔ /etc/os-release: SteamOS has no VERSION_ID in /usr/lib/os-release
V=$(sed -n 's/^VERSION_ID=//p' "$R/etc/os-release" | tail -n1 | tr -d "\"'")
case "$V" in ""|*[!A-Za-z0-9._-]*) die sx_no_version ;; esac
BASE="$R/home/.rexon"
OUT="$BASE/sysext/rexon-$DE-$V.raw"
LINK="$R/var/lib/extensions/rexon-$DE.raw"
B="$BASE/build/$DE-$V"
G="$BASE/gnupg"
OSDB="$R/usr/lib/holo/pacmandb"
case "$DE" in
__DESKTOPS__
esac
mkdir -p "$BASE/sysext" "$B/cache" "$R/var/lib/extensions"
rm -rf "$B/db"
# image tree and hook overlay live on tmpfs: overlayfs refuses the
# casefold-capable ext4 of /home on the 3.8 kernel
T=$(mktemp -d /tmp/rexon-sysext-$DE.XXXXXX)
cleanup() { umount "$T/merged" 2>/dev/null || true; rm -rf --one-file-system "$T"; }
trap cleanup EXIT
progress 10
# 1. private copy of the OS package database (local = what the image has)
cp -a "$OSDB" "$B/db"
PAC=(pacman --noconfirm --dbpath "$B/db" --cachedir "$B/cache" --gpgdir "$G" --logfile "$B/pacman.log")
# 2. own keyring (the image ships an uninitialised one)
if [ ! -s "$G/pubring.gpg" ] && [ ! -s "$G/pubring.kbx" ]; then
  rx_say sx_keyring
  pacman-key --gpgdir "$G" --init >/dev/null 2>&1
  pacman-key --gpgdir "$G" --populate archlinux holo >/dev/null 2>&1
fi
# 3. sync databases of this OS version's repositories (needs network)
rx_say sx_sync_db
if ! "${PAC[@]}" -Sy >"$B/sync.log" 2>&1; then
  rx_say sx_no_network; exit 75
fi
progress 20
# 4. only what the OS image lacks
mapfile -t LIST < <("${PAC[@]}" -Sp --needed --print-format '%n %v %f' "${PKGS[@]}")
if [ ${#LIST[@]} -eq 0 ]; then rx_say sx_nothing_to_add; exit 0; fi
printf '%s\n' "${LIST[@]}" > "$B/packages.txt"
rx_say sx_packages "${#LIST[@]}"
CORE='__CORE__'
if hit=$(grep -E "$CORE" "$B/packages.txt"); then
  rx_say sx_shadow_core "$(printf '%s' "$hit" | cut -d' ' -f1 | tr '\n' ' ')"; exit 3
fi
# 5. download + signature check
rx_say sx_downloading "${#LIST[@]}"
"${PAC[@]}" -Sw --needed "${PKGS[@]}" >"$B/download.log" 2>&1 || { rx_say sx_download_failed; tail -5 "$B/download.log"; exit 75; }
progress 50
# 6. unpack into the extension root
rx_say sx_unpacking
mkdir -p "$T/root"
R0=$T/root
unpack() { bsdtar -xpf "$B/cache/$1" -C "$R0" --exclude '.PKGINFO' --exclude '.BUILDINFO' \
             --exclude '.MTREE' --exclude '.INSTALL' --exclude '.CHANGELOG'; }
while read -r n v f; do unpack "$f"; done < "$B/packages.txt"
progress 60
# 6b. soname closure. Valve's repositories of one series are not always
# consistent with the image (3.8.14 + repo 3.8.1x: xfce4-session wants
# libdisplay-info.so.2, the image has .so.3). Missing ELF deps are judged
# against the OS package FILE LISTS, not the live /usr (an older image of ours
# may still be merged there). Optional plugins whose libraries are absent are
# dropped; everything else gets its provider ("libX.so=N-64").
rx_say sx_checking_libs
OPTIONAL='/(plugins|modules|tumbler-1|gtk-2\.0|thunarx-3)/'
unresolved() {
  cat "$OSDB"/local/*/files | sed -n 's#^usr/lib/\(.*/\)\{0,1\}\([^/]*\.so[^/]*\)$#\2#p' | sort -u > "$B/hostlibs"
  find "$R0" \( -type f -o -type l \) -name '*.so*' -printf '%f\n' | sort -u > "$B/imagelibs"
  find "$R0/usr" -type f \( -perm -u+x -o -name '*.so*' \) -print0 | while IFS= read -r -d '' f; do
    readelf -d "$f" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | while read -r so; do
      grep -qxF "$so" "$B/hostlibs" "$B/imagelibs" || printf '%s\t%s\n' "$f" "$so"; done
  done
}
for round in 1 2 3 4 5; do
  unresolved > "$B/unresolved.txt" || true
  [ -s "$B/unresolved.txt" ] || break
  mapfile -t need < <(awk -F'\t' -v re="$OPTIONAL" '$1 !~ re {print $2}' "$B/unresolved.txt" | sort -u)
  [ ${#need[@]} -gt 0 ] || break
  rx_say sx_soname_round "$round" "${need[*]}"
  want=(); for so in "${need[@]}"; do want+=("${so%%.so.*}.so=${so##*.so.}-64"); done
  if ! "${PAC[@]}" -Sp --print-format '%n %v %f' "${want[@]}" >"$B/soname.out" 2>"$B/soname.err"; then
    rx_say sx_no_provider "${need[*]}"; exit 4
  fi
  mapfile -t extra < "$B/soname.out"
  printf '%s\n' "${extra[@]}" >> "$B/packages.txt"
  if hit=$(grep -E "$CORE" "$B/packages.txt"); then
    rx_say sx_shadow_core "$(printf '%s' "$hit" | cut -d' ' -f1 | tr '\n' ' ')"; exit 3
  fi
  for e in "${extra[@]}"; do
    n=${e%% *}
    if h=$(pacman --dbpath "$OSDB" -Q "$n" 2>/dev/null); then rx_say sx_also_in_os "$n" "$h" "${e#* }"; fi
  done
  "${PAC[@]}" -Sw "${want[@]}" >>"$B/download.log" 2>&1 || { rx_say sx_download_failed; exit 75; }
  for e in "${extra[@]}"; do unpack "${e##* }"; done
done
# whatever is still unresolved is needed only by optional plugins: drop them
unresolved > "$B/unresolved.txt" || true
while IFS=$'\t' read -r f so; do
  [ -n "$f" ] || continue
  if ! printf '%s' "$f" | grep -Eq "$OPTIONAL"; then rx_say sx_unresolved "${f#"$R0"}"; exit 4; fi
  [ -e "$f" ] || continue
  rx_say sx_drop_plugin "${f#"$R0"}" "$so"; rm -f "$f"
done < "$B/unresolved.txt"
progress 70
# 7. /etc payload (sysext overlays only /usr):
#  * xdg defaults -> /usr/share/rexon-<de>/xdg, first in XDG_CONFIG_DIRS of OUR session only
#  * pam.d -> /usr/lib/pam.d: SteamOS libpam (1.7) falls back to the vendor
#    directory when /etc/pam.d/<service> is absent — the file updates with the image
#  * everything else is reported
X=$R0/usr/share/rexon-$DE/xdg
mkdir -p "$X"
if [ -d "$R0/etc/xdg" ]; then cp -a "$R0/etc/xdg/." "$X/"; fi
if [ -n "$XDG_FIXUP" ]; then eval "$XDG_FIXUP"; fi
if [ -d "$R0/etc/pam.d" ]; then
  mkdir -p "$R0/usr/lib/pam.d"; cp -a "$R0/etc/pam.d/." "$R0/usr/lib/pam.d/"
  rx_say sx_pam "$(ls "$R0/usr/lib/pam.d" | tr '\n' ' ')"
fi
if [ -d "$R0/etc" ]; then
  left=$(cd "$R0" && find etc -type f ! -path 'etc/xdg/*' ! -path 'etc/pam.d/*' | tr '\n' ' ')
  if [ -n "$left" ]; then rx_say sx_etc_left "$left"; fi
fi
# Plasma autostart entries of the OS without OnlyShowIn are hidden in our
# session: an entry of the same name earlier in XDG_CONFIG_DIRS wins
mkdir -p "$X/autostart"
hidden=""
for a in "$R"/etc/xdg/autostart/*.desktop; do
  [ -e "$a" ] || continue
  n=${a##*/}; [ -e "$X/autostart/$n" ] && continue
  grep -q '^OnlyShowIn=' "$a" && continue
  case "$n" in org.kde.*|kde*|plasma*|pam_kwallet*|print-applet*|baloo*|powerdevil*|polkit-kde*|kaccess*|kglobalaccel*|xembedsniproxy*|gmenudbusmenuproxy*) ;;
    *) continue ;; esac
  printf '[Desktop Entry]\nType=Application\nName=%s (hidden by Rexon outside Plasma)\nHidden=true\n' "$n" > "$X/autostart/$n"
  hidden+="$n "
done
if [ -n "$hidden" ]; then rx_say sx_hidden_autostart "$hidden"; fi
rm -rf "$R0/etc" "$R0/var" "$R0/run" "$R0/tmp"
for d in "$R0"/*; do
  [ -e "$d" ] || continue
  case "${d##*/}" in usr|opt) ;; *) rm -rf "$d" ;; esac
done
# trim what a desktop session never needs
rm -rf "$R0/usr/include" "$R0/usr/share/doc" "$R0/usr/share/man" "$R0/usr/share/gtk-doc" \
       "$R0/usr/share/info" "$R0/usr/lib/pkgconfig" "$R0/usr/share/pkgconfig"
find "$R0/usr/lib" -name '*.a' -delete 2>/dev/null || true
# 8. session entry + wrapper. The desktop's environment exists ONLY inside this
#    session: pushed into the user manager / D-Bus activation environment at
#    start (xfconfd & co. are D-Bus activated) and removed when the session ends,
#    so Plasma never inherits it.
mkdir -p "$R0/usr/lib/rexon-$DE" "$R0/usr/share/$SESSION_DIR"
cat > "$R0/usr/lib/rexon-$DE/session" <<EOS
#!/bin/sh
export XDG_CONFIG_DIRS="/usr/share/rexon-$DE/xdg:\${XDG_CONFIG_DIRS:-/etc/xdg}"
$SESSION_ENV
VARS="__VARS__"
dbus-update-activation-environment --systemd \$VARS 2>/dev/null || true
$SESSION_PRESTART
cleanup() {
  systemctl --user unset-environment \$VARS 2>/dev/null || true
  echo "rexon-$DE session ended: removed \$VARS from the user manager environment" | systemd-cat -t rexon-session 2>/dev/null || true
  $SESSION_PRESTART
}
trap cleanup EXIT
trap 'exit 0' HUP INT TERM
$SESSION_EXEC "\$@"
EOS
chmod 755 "$R0/usr/lib/rexon-$DE/session"
SESSION_FILE="$R0/usr/share/$SESSION_DIR/rexon-$DE.desktop"
cat > "$SESSION_FILE" <<EOS
[Desktop Entry]
Name=$SESSION_NAME
Comment=Installed by Rexon as a system extension (SteamOS $V)
Exec=/usr/lib/rexon-$DE/session
Type=Application
EOS
# only our session is offered
for f in "$R0"/usr/share/xsessions/*.desktop "$R0"/usr/share/wayland-sessions/*.desktop; do
  [ -e "$f" ] && [ "$f" != "$SESSION_FILE" ] && rm -f "$f"
done
# 9. no generated caches in a desktop image: they are built once over ALL
#    desktops by rexon-sysext-caches (a separate, topmost image)
find "$R0/usr" \( -name icon-theme.cache -o -name mimeinfo.cache -o -name gschemas.compiled \
  -o -name giomodule.cache -o -name loaders.cache \) -delete
# 10. extension-release
mkdir -p "$R0/usr/lib/extension-release.d"
{ echo "ID=steamos"; echo "VERSION_ID=$V"; echo "ARCHITECTURE=x86-64"
  if [ -d "$R0/usr/lib/systemd" ]; then echo "EXTENSION_RELOAD_MANAGER=1"; fi; } \
  > "$R0/usr/lib/extension-release.d/extension-release.rexon-$DE"
progress 80
# 11. pack + link
rx_say sx_packing
mksquashfs "$R0" "$OUT.tmp" -comp zstd -all-root -quiet -noappend >/dev/null
mv -f "$OUT.tmp" "$OUT"
ln -sfn "${OUT#"$R"}" "$LINK"
# images and build trees of other OS versions
for o in "$BASE"/sysext/rexon-$DE-*.raw; do [ "$o" = "$OUT" ] || rm -f "$o"; done
for d in "$BASE"/build/$DE-*; do [ "$d" = "$B" ] || rm -rf --one-file-system "$d"; done
rx_say sx_built "${OUT#"$R"}" "$(du -h "$OUT" | cut -f1)" "$(wc -l < "$B/packages.txt")"
rm -rf "$B/db"
progress 85
"###;

const CACHES_TMPL: &str = r###"#!/bin/bash
# rexon-sysext-caches [--force] — generated by Rexon.
# Generated caches (icon themes, MIME, GSettings schemas, desktop database,
# GIO modules, gdk-pixbuf loaders) are built ONCE over the OS /usr plus ALL
# desktop images and shipped as the separate image __CACHES__-<V>.raw, the
# topmost layer (systemd-sysext stacks images by name). Rebuilt when the set
# of desktop images changes (manifest). Prints "changed" when the link changed.
set -euo pipefail
R="${REXON_SYSEXT_ROOT:-}"
__LANG_FROM_FILE__
__L10N__
force=0; [ "${1:-}" = --force ] && force=1
V=$(sed -n 's/^VERSION_ID=//p' "$R/etc/os-release" | tail -n1 | tr -d "\"'")
case "$V" in ""|*[!A-Za-z0-9._-]*) die sx_no_version ;; esac
BASE="$R/home/.rexon"
NAME=__CACHES__
OUT="$BASE/sysext/$NAME-$V.raw"
LINK="$R/var/lib/extensions/$NAME.raw"
imgs=()
for l in "$R"/var/lib/extensions/rexon-*.raw; do
  [ -e "$l" ] || [ -L "$l" ] || continue
  [ "$l" = "$LINK" ] && continue
  t=$(readlink "$l" || true)
  case "$t" in *-"$V".raw) [ -f "$R$t" ] && imgs+=("$R$t") ;; esac
done
if [ ${#imgs[@]} -eq 0 ]; then
  if [ -e "$LINK" ] || [ -L "$LINK" ]; then
    rm -f "$LINK" "$BASE"/sysext/$NAME-*.raw; rx_say sx_caches_removed >&2; echo changed
  fi
  exit 0
fi
manifest=$(for i in "${imgs[@]}"; do stat -c '%n %s %Y' "$i"; done | sort)
if [ $force = 0 ] && [ -f "$OUT" ] && [ "$(readlink "$LINK" 2>/dev/null)" = "${OUT#"$R"}" ] \
   && [ "$(unsquashfs -cat "$OUT" usr/share/rexon-caches/manifest 2>/dev/null)" = "$manifest" ]; then
  rx_say sx_caches_current >&2; exit 0
fi
T=$(mktemp -d /tmp/rexon-caches.XXXXXX)
cleanup() { umount "$T/m" 2>/dev/null || true; for d in "$T"/img/*; do umount "$d" 2>/dev/null || true; done
            umount "$T/base" 2>/dev/null || true; rm -rf --one-file-system "$T"; }
trap cleanup EXIT
mkdir -p "$T/base" "$T/img" "$T/up" "$T/work" "$T/m" "$T/root/usr"
mount --bind / "$T/base"     # non-recursive: the OS /usr WITHOUT any sysext overlay
lower=""
for i in "${imgs[@]}"; do n=${i##*/}; mkdir -p "$T/img/$n"; mount -o loop,ro "$i" "$T/img/$n"; lower+="$T/img/$n/usr:"; done
mount -t overlay overlay -o "lowerdir=${lower}$T/base/usr,upperdir=$T/up,workdir=$T/work" "$T/m"
M=$T/m
glib-compile-schemas "$M/share/glib-2.0/schemas"
for d in "$M"/share/icons/*/; do d=${d%/}; [ -f "$d/index.theme" ] && gtk-update-icon-cache -q -f -t "$d" || true; done
update-desktop-database -q "$M/share/applications" 2>/dev/null || true
update-mime-database "$M/share/mime" >/dev/null 2>&1 || true
if [ -d "$M/lib/gdk-pixbuf-2.0/2.10.0/loaders" ]; then
  GDK_PIXBUF_MODULEDIR="$M/lib/gdk-pixbuf-2.0/2.10.0/loaders" gdk-pixbuf-query-loaders \
    | sed "s#$M/#/usr/#g" > "$M/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
fi
if [ -d "$M/lib/gio/modules" ]; then gio-querymodules "$M/lib/gio/modules"; fi
umount "$M"
(cd "$T/up" && find . -type f -print0) | while IFS= read -r -d '' f; do
  mkdir -p "$T/root/usr/$(dirname "$f")"; cp -a "$T/up/$f" "$T/root/usr/$f"; done
mkdir -p "$T/root/usr/share/rexon-caches" "$T/root/usr/lib/extension-release.d"
printf '%s\n' "$manifest" > "$T/root/usr/share/rexon-caches/manifest"
printf 'ID=steamos\nVERSION_ID=%s\nARCHITECTURE=x86-64\n' "$V" > "$T/root/usr/lib/extension-release.d/extension-release.$NAME"
mksquashfs "$T/root" "$OUT.tmp" -comp zstd -all-root -quiet -noappend >/dev/null
mv -f "$OUT.tmp" "$OUT"; ln -sfn "${OUT#"$R"}" "$LINK"
for o in "$BASE"/sysext/$NAME-*.raw; do [ "$o" = "$OUT" ] || rm -f "$o"; done
rx_say sx_caches_built "${OUT#"$R"}" >&2
echo changed
"###;

const SYNC_TMPL: &str = r###"#!/bin/bash
# rexon-sysext-sync [--interactive] [--only <desktop>|none] — generated by Rexon.
# Runs as a systemd service after boot, NOT before the login screen: login is
# never blocked by a rebuild.
#  * every desktop in /var/lib/rexon/sysext/desktops whose image is not built
#    for the current VERSION_ID is rebuilt (network needed);
#  * the shared caches image is rebuilt when the set of images changed;
#  * merge, users/tmpfiles of the images, reload D-Bus;
#  * the session the guard replaced with Plasma is given back;
#  * the user is told what happens (desktop notifications).
# --interactive: started by Rexon's window — progress lines, no notifications.
# --only: touch just this desktop ("none": only caches + merge).
# Exit 75 = try again later (no network); the unit restarts itself.
set -uo pipefail
R="${REXON_SYSEXT_ROOT:-}"
__LANG_FROM_FILE__
__L10N__
interactive=0; only=""
while [ $# -gt 0 ]; do
  case "$1" in
    --interactive) interactive=1 ;;
    --only) only="${2:-}"; shift ;;
  esac
  shift
done
V=$(sed -n 's/^VERSION_ID=//p' "$R/etc/os-release" | tail -n1 | tr -d "\"'")
case "$V" in ""|*[!A-Za-z0-9._-]*) die sx_no_version ;; esac
LIST="$R/var/lib/rexon/sysext/desktops"
WANT="$R/var/lib/rexon/session-guard/wanted-session"
BIN="$R/var/lib/rexon/bin"
IMG="/home/.rexon/sysext"
EXT="$R/var/lib/extensions"
NETFLAG="$R/run/rexon-sysext-netwait-notified"
progress() { if [ $interactive = 1 ]; then echo "@progress $1"; fi; }
pretty() {
    case "$1" in
__PRETTY__        *) echo "$1" ;;
    esac
}
# message text without the "name: " prefix of rx_say
msg() { local m; m=$("$@"); printf '%s' "${m#*: }"; }
user_buses() {
  for bus in "$R"/run/user/*/bus; do [ -S "$bus" ] && echo "$bus"; done
}
bus_user() { id -nu "$(stat -c %u "$1")"; }
as_user() { local bus="$1" u; shift
  u=$(bus_user "$bus") || return 1
  runuser -u "$u" -- env DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" "$@"; }
# notify <title> <body> [low|normal|critical] — to every logged-in session;
# waits up to 60 s for a notification daemon (we run right after autologin)
notify() {
  [ $interactive = 1 ] && return 0
  local bus
  for bus in $(user_buses); do
    for _ in $(seq 30); do
      as_user "$bus" busctl --user status org.freedesktop.Notifications >/dev/null 2>&1 && break; sleep 2; done
    as_user "$bus" notify-send -a Rexon -i system-software-update -u "${3:-normal}" "$1" "$2" 2>/dev/null \
      && rx_say sx_notified "$1" || rx_say sx_not_notified
  done
}
if [ ! -s "$LIST" ] && [ -z "$only" ]; then rx_say sx_no_desktops; exit 0; fi
rc=0; changed=0; built=(); failed=()
[ "$only" = none ] && changed=1
progress 5
while read -r de; do
  [ -n "$de" ] || continue
  case "$de" in __IDS__) ;; *) rx_say sx_bad_desktop "$de"; continue ;; esac
  if [ -n "$only" ] && [ "$de" != "$only" ]; then continue; fi
  cur=$(readlink "$EXT/rexon-$de.raw" 2>/dev/null || true)
  if [ "$cur" = "$IMG/rexon-$de-$V.raw" ] && [ -f "$R$cur" ]; then
    rx_say sx_up_to_date "$(pretty "$de")"; continue
  fi
  rx_say sx_rebuilding "$(pretty "$de")" "${cur##*/}"
  if [ ! -e "$NETFLAG.started-$de" ]; then
    notify "$(msg rx_say sx_n_updating_title "$(pretty "$de")" "$V")" "$(msg rx_say sx_n_updating_body)" low
    touch "$NETFLAG.started-$de" 2>/dev/null || true
  fi
  REXON_SYSEXT_PROGRESS=$interactive "$BIN/rexon-sysext-build" "$de"; r=$?
  case $r in
    0) changed=1; built+=("$(pretty "$de")") ;;
    75) rx_say sx_retry_later "$(pretty "$de")"; rc=75
        if [ ! -e "$NETFLAG" ]; then
          notify "$(msg rx_say sx_n_wait_title)" "$(msg rx_say sx_n_wait_body "$(pretty "$de")" "$V")"
          touch "$NETFLAG" 2>/dev/null || true
        fi ;;
    *) rx_say sx_build_failed "$(pretty "$de")" "$r"; failed+=("$(pretty "$de")"); [ $rc = 0 ] && rc=$r ;;
  esac
done < <(cat "$LIST" 2>/dev/null; [ -n "$only" ] && [ "$only" != none ] && ! grep -qx "$only" "$LIST" 2>/dev/null && echo "$only")
progress 88
# shared caches over all desktop images; skipped while a desktop waits for the
# network (its old image is not for this OS anyway)
if [ $rc != 75 ] && [ "$("$BIN/rexon-sysext-caches")" = changed ]; then changed=1; fi
progress 92
if [ $changed = 1 ]; then
  rx_say sx_merging
  systemd-sysext refresh
  systemd-sysusers 2>&1 | grep -v "Conflict with earlier configuration" || true
  systemd-tmpfiles --create >/dev/null 2>&1 || true
  busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig >/dev/null 2>&1 || true
  for bus in $(user_buses); do
    as_user "$bus" busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig >/dev/null 2>&1 || true
  done
fi
# give the wanted session back once it exists again
if [ -s "$WANT" ]; then
  s=$(head -n1 "$WANT")
  if [ -f "$R/usr/share/wayland-sessions/$s" ] || [ -f "$R/usr/share/xsessions/$s" ]; then
    done_by=""
    for bus in $(user_buses); do
      u=$(bus_user "$bus") || continue
      if timeout 15 runuser -u "$u" -- env DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
           steamosctl set-default-desktop-session "$s"; then done_by=steamosctl; fi
    done
    if [ -z "$done_by" ]; then   # nobody logged in: edit the files the guard edited
      for conf in "$R/etc/sddm.conf.d/zz-holo-autologin.conf" "$R/etc/sddm.conf.d/zz-steamos-autologin.conf"; do
        [ -f "$conf" ] && sed -i "s/^Session=__PLASMA__\$/Session=$s/" "$conf"
      done
      for st in "$R"/home/*/.config/steamos-manager/state.toml; do
        [ -f "$st" ] || continue
        t=$(sed "s/^desktop_session = \"__PLASMA__\"/desktop_session = \"$s\"/" "$st"); printf '%s\n' "$t" > "$st"
      done
      done_by=files
    fi
    rx_say sx_session_restored "$s" "$done_by"; rm -f "$WANT"
  fi
fi
progress 97
if [ ${#built[@]} -gt 0 ]; then
  names=$(printf '%s, ' "${built[@]}"); names=${names%, }
  notify "$(msg rx_say sx_n_ready_title "$names" "$V")" "$(msg rx_say sx_n_ready_body)"
  rm -f "$NETFLAG" "$NETFLAG".started-*
fi
if [ ${#failed[@]} -gt 0 ]; then
  names=$(printf '%s, ' "${failed[@]}"); names=${names%, }
  notify "$(msg rx_say sx_n_failed_title "$names")" "$(msg rx_say sx_n_failed_body)" critical
fi
exit $rc
"###;

const GUARD_TMPL: &str = r###"#!/bin/bash
# session-guard.sh — generated by Rexon. Runs before the login screen. If the
# autologin session points to a .desktop file that does not exist (our image
# was not merged after an OS update because VERSION_ID changed), fall back to
# Plasma so Desktop Mode still logs in without a password prompt; the wanted
# session is remembered and given back by rexon-sysext-sync.
set -u
R="${REXON_SYSEXT_ROOT:-}"
__LANG_FROM_FILE__
__L10N__
STATE="$R/var/lib/rexon/session-guard"
mkdir -p "$STATE"
exists() {
  local d
  for d in usr/share/wayland-sessions usr/share/xsessions usr/local/share/wayland-sessions usr/local/share/xsessions; do
    [ -f "$R/$d/$1" ] && return 0
  done
  return 1
}
for conf in "$R/etc/sddm.conf.d/zz-holo-autologin.conf" "$R/etc/sddm.conf.d/zz-steamos-autologin.conf"; do
  [ -f "$conf" ] || continue
  s=$(sed -n 's/^Session=//p' "$conf" | tail -n1)
  [ -n "$s" ] || continue
  if ! exists "$s"; then
    rx_say sx_guard_fallback "$s" "${conf#"$R"}"
    printf '%s\n' "$s" > "$STATE/wanted-session"
    sed -i "s/^Session=.*/Session=__PLASMA__/" "$conf"
  fi
done
# keep steamos-manager state consistent (rewritten in place to keep the owner)
for st in "$R"/home/*/.config/steamos-manager/state.toml; do
  [ -f "$st" ] || continue
  s=$(sed -n 's/^desktop_session = "\(.*\)"/\1/p' "$st")
  if [ -n "$s" ] && ! exists "$s"; then
    rx_say sx_guard_state "${st#"$R"}" "$s"
    [ -s "$STATE/wanted-session" ] || printf '%s\n' "$s" > "$STATE/wanted-session"
    t=$(sed 's/^desktop_session = ".*"/desktop_session = "__PLASMA__"/' "$st")
    printf '%s\n' "$t" > "$st"
  fi
done
exit 0
"###;

const HELPER_TMPL: &str = r###"#!/bin/bash
# rexon-sysext-helper [--lang=<code>] install|remove <desktop> — generated by
# Rexon. Runs ONLY through pkexec. Sets up and removes desktop environments on
# SteamOS (Steam Deck and Steam Machine) as systemd-sysext images. Accepts an
# operation from a fixed list and a desktop from a fixed list; carries the
# texts of all scripts and units itself — nothing is taken from the caller.
set -uo pipefail
R="${REXON_SYSEXT_ROOT:-}"
__L10N__
[ -n "$R" ] || [ "$(id -u)" = 0 ] || die not_via_pkexec
# the one-password session helper from the Rexon delivery and its sha256
rx_priv_src=""; rx_priv_sum=""
while :; do
  case "${1:-}" in
    --priv-helper=*) rx_priv_src="${1#--priv-helper=}"; shift ;;
    --priv-sha256=*) rx_priv_sum="${1#--priv-sha256=}"; shift ;;
    *) break ;;
  esac
done
op="${1:-}"; de="${2:-}"
case "$op" in install|remove) ;; *) die sx_helper_args ;; esac
case "$de" in __IDS__) ;; *) die sx_bad_desktop "$de" ;; esac
LIST="$R/var/lib/rexon/sysext/desktops"
BIN="$R/var/lib/rexon/bin"
WANT="$R/var/lib/rexon/session-guard/wanted-session"
put() {  # put <path> <mode>  (text on stdin) — atomic replace
  mkdir -p "${1%/*}" && cat > "$1.rexon-new" && chmod "$2" "$1.rexon-new" && mv -f "$1.rexon-new" "$1"
}
put_priv() {  # copy the session helper: root's own directory first, then sha256, then rename
  local dst="$R__SESSION_HELPER_PATH__"
  case "$rx_priv_sum" in *[!0-9a-f]*) return 1 ;; esac
  [ "${#rx_priv_sum}" = 64 ] || return 1
  case "$rx_priv_src" in /*) ;; *) return 1 ;; esac
  [ -f "$rx_priv_src" ] && [ ! -L "$rx_priv_src" ] || return 1
  # the window refuses a helper whose directories are writable by others
  mkdir -p "${dst%/*}" && chmod 755 "$R/var/lib/rexon" "${dst%/*}" || return 1
  rm -f "$dst.rexon-new"
  if cp -- "$rx_priv_src" "$dst.rexon-new" && { [ -n "$R" ] || chown 0:0 "$dst.rexon-new"; } \
    && chmod 755 "$dst.rexon-new" \
    && [ "$(sha256sum < "$dst.rexon-new" | cut -d' ' -f1)" = "$rx_priv_sum" ]; then
    mv -f "$dst.rexon-new" "$dst"
  else
    rm -f "$dst.rexon-new"; return 1
  fi
}
layout() {
__LAYOUT__    printf '%s\n' "$rx_lang" | put "$R/var/lib/rexon/sysext/lang" 644
    # started as `bash -c <text>` (first run or a newer Rexon): keep the text
    # as the helper file, so the next run goes through its own polkit action
    if [ -n "${BASH_EXECUTION_STRING:-}" ]; then printf '%s' "$BASH_EXECUTION_STRING" | put "$R__HELPER_PATH__" 755; fi
    # the session helper is taken ONLY on that path: the file helper keeps its
    # password for minutes (auth_admin_keep), and a path from its caller would
    # let any process place its own root binary without asking
    if [ -n "${BASH_EXECUTION_STRING:-}" ] && [ -n "$rx_priv_src" ]; then put_priv || true; fi
}
in_list() { grep -qx "$1" "$LIST" 2>/dev/null; }
list_remove() {
  [ -f "$LIST" ] || return 0
  grep -vx "$1" "$LIST" > "$LIST.rexon-new" || true
  mv -f "$LIST.rexon-new" "$LIST"
}
forget_images() {
  rm -f "$R/var/lib/extensions/rexon-$1.raw" "$R"/home/.rexon/sysext/rexon-"$1"-*.raw
  rm -rf --one-file-system "$R"/home/.rexon/build/"$1"-* 2>/dev/null || true
}
echo "@progress 1"
layout || die sx_layout_failed
systemctl daemon-reload || true
case "$op" in
  install)
    systemctl enable systemd-sysext.service rexon-session-guard.service rexon-sysext-sync.service >/dev/null 2>&1 \
      || rx_say sx_enable_failed
    fresh=0
    if ! in_list "$de"; then fresh=1; printf '%s\n' "$de" >> "$LIST"; fi
    "$BIN/rexon-sysext-sync" --interactive --only "$de"; rc=$?
    if [ $rc != 0 ] && [ $fresh = 1 ]; then
      # all or nothing: a desktop that did not build is not left behind
      rx_say sx_rollback "$de"
      list_remove "$de"; forget_images "$de"
      "$BIN/rexon-sysext-sync" --interactive --only none >/dev/null 2>&1 || true
    fi
    echo "@progress 100"
    exit $rc ;;
  remove)
    list_remove "$de"; forget_images "$de"
    "$BIN/rexon-sysext-sync" --interactive --only none; rc=$?
    "$BIN/session-guard.sh"
    if [ "$(head -n1 "$WANT" 2>/dev/null)" = "rexon-$de.desktop" ]; then rm -f "$WANT"; fi
    rx_say sx_removed "$de"
    echo "@progress 100"
    exit $rc ;;
esac
"###;

// ============================================================================
// Calling the helper
// ============================================================================

/// Helper operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperOp {
    Install,
    Remove,
}

impl HelperOp {
    pub fn as_str(self) -> &'static str {
        match self {
            HelperOp::Install => "install",
            HelperOp::Remove => "remove",
        }
    }
}

/// argv to run the helper. `installed` is the text of the helper file on the
/// system: matches ours — call the file (its own polkit action, the password is
/// kept for a few minutes); missing or different (the first time, a new Rexon)
/// — the same text via `bash -c`: it is in argv and can't be swapped after the
/// password prompt, and the helper saves it as a file itself.
/// `lang` is the UI language code (`ru`); English and unknown ones — without
/// the argument.
pub fn helper_argv(op: HelperOp, de_id: &str, installed: Option<&str>, lang: &str) -> Vec<String> {
    helper_argv_with(op, de_id, installed, lang, None)
}

/// The same argv, but with a copy of the session helper ([`session_helper_copy`]).
/// With a copy the helper goes only via `bash -c`, even if the
/// `rexon-sysext-helper` file matched: it accepts the binary path only there
/// (the file keeps the password for minutes, and a path from the caller would
/// be root for any process).
pub fn helper_argv_with(
    op: HelperOp,
    de_id: &str,
    installed: Option<&str>,
    lang: &str,
    copy: Option<&SessionHelperCopy>,
) -> Vec<String> {
    let text = helper_script();
    let lang_arg = crate::system_l10n::lang_arg_for(lang, Some(&text));
    let mut argv = vec!["pkexec".to_string()];
    if installed == Some(text.as_str()) && copy.is_none() {
        argv.push(HELPER_PATH.to_string());
    } else {
        argv.extend(["/bin/bash".to_string(), "-c".to_string(), text, HELPER_NAME.to_string()]);
    }
    argv.extend(lang_arg);
    if let Some(c) = copy {
        argv.push(format!("--priv-helper={}", c.source));
        argv.push(format!("--priv-sha256={}", c.sha256));
    }
    argv.extend([op.as_str().to_string(), de_id.to_string()]);
    argv
}

/// What to put into [`SESSION_HELPER_PATH`]: the delivery binary and its
/// sha256. Root copies the file to itself and checks the sum against this one
/// (in argv, can't be swapped after the password window) — swapping the file
/// between computing and copying won't pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHelperCopy {
    pub source: String,
    pub sha256: String,
}

/// Whether the session helper needs to be put in place. `delivery` is the
/// delivery binary path and its sha256 (`None` — it's not in the delivery:
/// nothing to put, the window rejects the old file by build);
/// `installed_sha256` is the sum of the file in `/var`; `installed_ok` — owners
/// and permissions of the whole chain are fine
/// (`privilege::client::check_ownership`). The same binary with fine
/// permissions — no copy; otherwise (missing, another build, broken
/// permissions) — there is one.
pub fn session_helper_copy(
    delivery: Option<(&str, &str)>,
    installed_sha256: Option<&str>,
    installed_ok: bool,
) -> Option<SessionHelperCopy> {
    let (source, sha256) = delivery?;
    if !source.starts_with('/') || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return None;
    }
    if installed_ok && installed_sha256 == Some(sha256) {
        return None;
    }
    Some(SessionHelperCopy { source: source.to_string(), sha256: sha256.to_string() })
}

/// sha256 of a file in lowercase hex digits.
fn file_sha256(path: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Live copy of the session helper: the delivery binary of the running Rexon
/// (`installer_exec::PayloadSource::for_running` — the same truth as the
/// install), without symlinks in the path, against the file in `/var`.
fn session_helper_copy_live() -> Option<SessionHelperCopy> {
    let own_exe = std::env::current_exe().ok()?.to_string_lossy().into_owned();
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_DATA_HOME").ok().filter(|x| !x.is_empty());
    let payload = crate::installer_exec::PayloadSource::for_running(
        &crate::system_exec::LiveSystemIo,
        &own_exe,
        &crate::cli::repo_root(),
        &home,
        xdg.as_deref(),
    );
    let source = std::fs::canonicalize(payload.priv_helper?).ok()?.to_string_lossy().into_owned();
    let sha256 = file_sha256(&source)?;
    let installed = file_sha256(SESSION_HELPER_PATH);
    let fs = crate::privilege::client::RealFs;
    let ok = crate::privilege::client::check_ownership(&fs, std::path::Path::new(SESSION_HELPER_PATH)).is_ok();
    session_helper_copy(Some((&source, &sha256)), installed.as_deref(), ok)
}

/// A helper output line: a progress fraction or a log line.
#[derive(Debug, Clone, PartialEq)]
pub enum HelperLine {
    Progress(f64),
    Log(String),
    Skip,
}

/// Parsing a helper output line (`@progress 40` — a fraction in percent).
pub fn parse_helper_line(line: &str) -> HelperLine {
    let t = line.trim_end();
    if let Some(p) = t.strip_prefix("@progress ") {
        return match p.trim().parse::<f64>() {
            Ok(v) => HelperLine::Progress((v / 100.0).clamp(0.0, 1.0)),
            Err(_) => HelperLine::Skip,
        };
    }
    if t.trim().is_empty() {
        HelperLine::Skip
    } else {
        HelperLine::Log(t.to_string())
    }
}

// ============================================================================
// Plans for the window
// ============================================================================

/// What is known about the machine for the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamOsState {
    pub release: Option<SteamOsRelease>,
    pub device: Device,
    pub password: PasswordState,
    /// Desktops from the build list.
    pub desktops: Vec<String>,
    /// Default session (`steamosctl get-default-desktop-session`).
    pub default_session: Option<String>,
}

/// A plan step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The helper with the administrator password.
    Helper(HelperOp, String),
    /// `steamosctl set-default-desktop-session` as the user.
    DefaultSession(String),
}

/// Why it is not possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotSteamOs,
    Builtin,
    NotYet(String),
    /// The user has no password — set one first (the window can do it).
    NeedPassword,
    /// The account is locked.
    Locked,
    /// The desktop the person is working in now can't be removed.
    Current,
}

impl Refusal {
    /// Text for the window.
    pub fn text(&self, device: Device) -> String {
        match self {
            Refusal::NotSteamOs => tr("This is not SteamOS."),
            Refusal::Builtin => tr("KDE Plasma is SteamOS's own desktop — it is always there."),
            Refusal::NotYet(why) => why.clone(),
            Refusal::NeedPassword => password_text(device),
            Refusal::Locked => tr(
                "The “deck” account is locked, so SteamOS can't confirm changes to the system. \
                 Unlock it first (for example, set a new password in Konsole with “passwd”).",
            ),
            Refusal::Current => tr(
                "You are working in this desktop right now. Switch to another desktop first, \
                 then remove this one.",
            ),
        }
    }
}

fn need_admin(state: &SteamOsState) -> Result<(), Refusal> {
    match state.password {
        PasswordState::Missing => Err(Refusal::NeedPassword),
        PasswordState::Locked => Err(Refusal::Locked),
        PasswordState::Set | PasswordState::Unknown => Ok(()),
    }
}

/// What the operation window should do before the helper: a user without a
/// password can't pass `pkexec`, so the window first lets them set one (the
/// password step), and only then starts the operation — `pkexec` will ask for
/// the just-set password in the stock window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminGate {
    /// There is a password (or couldn't find out — `pkexec` will say so itself).
    Ready,
    /// No password — the "set a password" step.
    AskPassword,
    /// Can't go further (the account is locked) — text for the window.
    Refused(String),
}

/// Decision for the window by the password state.
pub fn admin_gate(password: PasswordState, device: Device) -> AdminGate {
    match need_admin(&SteamOsState {
        release: None,
        device,
        password,
        desktops: Vec::new(),
        default_session: None,
    }) {
        Ok(()) => AdminGate::Ready,
        Err(Refusal::NeedPassword) => AdminGate::AskPassword,
        Err(r) => AdminGate::Refused(r.text(device)),
    }
}

/// Install plan (also «Check / Repair»): the helper builds the image, then the
/// desktop becomes the desktop mode session.
pub fn install_plan(de_id: &str, state: &SteamOsState) -> Result<Vec<Step>, Refusal> {
    if state.release.is_none() {
        return Err(Refusal::NotSteamOs);
    }
    let spec = match support(de_id) {
        Support::Supported(s) => s,
        Support::Builtin => return Err(Refusal::Builtin),
        Support::NotYet(why) => return Err(Refusal::NotYet(why)),
    };
    need_admin(state)?;
    Ok(vec![Step::Helper(HelperOp::Install, spec.id.to_string()), Step::DefaultSession(session_file(spec.id))])
}

/// Removal plan: if the desktop is the default session, first restore Plasma
/// (otherwise autologin runs into a vanished session — the password screen),
/// then the helper removes the image. `current_de` is the desktop the person is
/// in now.
pub fn remove_plan(de_id: &str, current_de: &str, state: &SteamOsState) -> Result<Vec<Step>, Refusal> {
    if state.release.is_none() {
        return Err(Refusal::NotSteamOs);
    }
    let spec = match support(de_id) {
        Support::Supported(s) => s,
        Support::Builtin => return Err(Refusal::Builtin),
        Support::NotYet(why) => return Err(Refusal::NotYet(why)),
    };
    if current_de == de_id {
        return Err(Refusal::Current);
    }
    need_admin(state)?;
    let mut steps = Vec::new();
    if state.default_session.as_deref() == Some(session_file(spec.id).as_str()) {
        steps.push(Step::DefaultSession(PLASMA_SESSION.to_string()));
    }
    steps.push(Step::Helper(HelperOp::Remove, spec.id.to_string()));
    Ok(steps)
}

// ============================================================================
// Window texts
// ============================================================================

/// Question before installing.
pub fn install_confirm_text(de_name: &str, version: &str) -> String {
    trf(
        // TRANSLATORS: %1 is a desktop name (Xfce), %2 the SteamOS version (3.9.5).
        "Rexon will build %1 for SteamOS %2 from Valve's package sources and add it to \
         the system as an extension — SteamOS itself stays untouched. You need the internet \
         and the administrator password.\n\nAfter every SteamOS update Rexon rebuilds %1 by \
         itself as soon as the internet is available; until then Desktop Mode starts KDE Plasma.\n\n\
         When it's done, Desktop Mode will start %1. KDE Plasma stays available.",
        &[&de_name, &version],
    )
}

/// Question before removing.
pub fn remove_confirm_text(de_name: &str) -> String {
    trf(
        // TRANSLATORS: %1 is a desktop name.
        "Rexon will remove %1 from SteamOS. If %1 is the desktop Desktop Mode starts, \
         KDE Plasma takes its place. Your files and settings in the home folder stay.",
        &[&de_name],
    )
}

/// Title of the password step.
pub fn password_title() -> String {
    tr("Set the administrator password")
}

/// Explanation of the password step.
pub fn password_text(device: Device) -> String {
    trf(
        // TRANSLATORS: %1 is "Steam Deck", "Steam Machine" or "this SteamOS device".
        "Installing a desktop changes the system, so SteamOS asks for the administrator \
         password of %1 — the password of the user “deck”. It isn't set yet. Choose it now \
         and remember it: SteamOS will ask for it whenever the system is changed (it is not \
         your Steam account password).",
        &[&device_label(device)],
    )
}

pub fn password_field_new() -> String {
    tr("New password")
}
pub fn password_field_repeat() -> String {
    tr("Repeat the password")
}
pub fn password_button() -> String {
    tr("Set password")
}
/// The password is set — the first log line of the operation that follows.
pub fn password_set_text() -> String {
    tr("The administrator password is set. SteamOS now asks for it once more to confirm the change.")
}
/// `passwd` refused — its words follow.
pub fn password_failed(detail: &str) -> String {
    trf("SteamOS didn't accept the password: %1", &[&detail])
}

/// Install result.
pub fn install_done_text(de_name: &str) -> String {
    trf(
        // TRANSLATORS: %1 is a desktop name.
        "%1 is installed. Log out (or switch to Desktop Mode from Game Mode) — %1 starts. \
         To go back to KDE Plasma, press “Switch” on the KDE Plasma card.",
        &[&de_name],
    )
}

/// The desktop is installed, but making it the session failed.
pub fn default_session_failed(de_name: &str) -> String {
    trf(
        // TRANSLATORS: %1 is a desktop name.
        "%1 is installed, but SteamOS didn't answer when Rexon tried to make it the Desktop \
         Mode desktop. Press “Switch” on the %1 card.",
        &[&de_name],
    )
}

/// Result by the helper's code.
pub fn helper_failure_text(rc: Option<i32>, de_name: &str) -> String {
    match rc {
        Some(EXIT_NO_NETWORK) => trf(
            // TRANSLATORS: %1 is a desktop name.
            "No internet connection: Rexon downloads %1 from Valve's package sources. Connect \
             to the internet and press “Install” again. Nothing was changed.",
            &[&de_name],
        ),
        Some(126) | Some(127) => tr("The password wasn't entered or wasn't accepted. Nothing was changed."),
        Some(3) => trf(
            "%1 would have to replace parts of SteamOS itself, so Rexon stopped. Nothing was changed.",
            &[&de_name],
        ),
        Some(4) => trf(
            "Valve's package sources for this SteamOS version don't have everything %1 needs. \
             Nothing was changed; try again after the next SteamOS update.",
            &[&de_name],
        ),
        _ => trf(
            // TRANSLATORS: %1 is a desktop name.
            "Rexon couldn't build %1 — the details are in the log above. KDE Plasma stays the desktop.",
            &[&de_name],
        ),
    }
}

// ============================================================================
// Executor
// ============================================================================

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn run_capture(argv: &[&str], timeout: u64, stdin: Option<&str>) -> Option<(i32, String, String)> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let (bin, rest) = argv.split_first()?;
    let mut child = Command::new(bin)
        .args(rest)
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed().as_secs() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// The SteamOS under Rexon (`/etc/os-release`); `None` — not SteamOS.
pub fn live_release() -> Option<SteamOsRelease> {
    read("/etc/os-release").and_then(|t| steamos_release(&t))
}

/// Which machine this is (by DMI).
pub fn live_device() -> Device {
    let dmi = |f: &str| read(&format!("/sys/class/dmi/id/{f}")).unwrap_or_default();
    device_from_dmi(&dmi("board_name"), &dmi("product_name"))
}

/// Own user's password by `passwd -S` (without root, fractions of a second).
pub fn live_password_state() -> PasswordState {
    let user = std::env::var("USER").unwrap_or_default();
    let mut argv = vec!["passwd", "-S"];
    if !user.is_empty() {
        argv.push(&user);
    }
    run_capture(&argv, 10, None).map(|(_, out, _)| parse_passwd_status(&out)).unwrap_or(PasswordState::Unknown)
}

/// State of the live machine. Read-only, without root.
pub fn live_state() -> SteamOsState {
    let release = live_release();
    let device = live_device();
    let password = if release.is_some() { live_password_state() } else { PasswordState::Unknown };
    let desktops = read(DESKTOPS_LIST).map(|t| parse_desktop_list(&t)).unwrap_or_default();
    let default_session = if release.is_some() {
        run_capture(&["steamosctl", "get-default-desktop-session"], STEAMOSCTL_TIMEOUT_SECS, None)
            .filter(|(rc, _, _)| *rc == 0)
            .map(|(_, out, _)| out.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    SteamOsState { release, device, password, desktops, default_session }
}

/// Make a session the desktop mode session (as the user).
pub fn set_default_session_live(session: &str) -> Result<(), String> {
    match run_capture(&["steamosctl", "set-default-desktop-session", session], STEAMOSCTL_TIMEOUT_SECS, None) {
        Some((0, _, _)) => Ok(()),
        Some((_, out, err)) => Err(format!("{}{}", out.trim(), err.trim())),
        None => Err(tr("steamosctl didn't answer in time")),
    }
}

/// Set a password for a user without one (`passwd` reads it from stdin).
pub fn set_password_live(password: &str) -> Result<(), String> {
    match run_capture(&["passwd"], 30, Some(&passwd_input(password))) {
        Some((0, _, _)) => Ok(()),
        Some((_, out, err)) => {
            Err(password_failed(&passwd_complaint(&format!("{out}\n{err}"))))
        }
        None => Err(password_failed("passwd")),
    }
}

/// Carry out the plan; progress goes to the operation window.
fn run_plan(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_name: &str,
    steps: &[Step],
    done_text: String,
) -> DeOpOutcome {
    let outcome = |ok: bool, message: String| DeOpOutcome { ok, message, cancelled: false, restart_needed: false };
    sink.cancellable(true);
    if cancel.is_cancelled() {
        return DeOpOutcome {
            ok: false,
            message: crate::system_de_exec::cancelled_message(),
            cancelled: true,
            restart_needed: false,
        };
    }
    sink.cancellable(false);
    let mut warning: Option<String> = None;
    for step in steps {
        match step {
            Step::DefaultSession(session) => {
                sink.progress("post_install", "", Some(0.97));
                if let Err(e) = set_default_session_live(session) {
                    crate::log_warn!("steamos", "steamosctl set-default-desktop-session {session}: {e}");
                    if session == PLASMA_SESSION {
                        // Before removal: without this we don't go further — otherwise
                        // autologin runs into a vanished session.
                        let text = format!("{}: {e}", tr("SteamOS didn't switch Desktop Mode back to KDE Plasma"));
                        sink.progress(de_progress::STAGE_ERROR, &text, None);
                        return outcome(false, text);
                    }
                    warning = Some(default_session_failed(de_name));
                }
            }
            Step::Helper(op, de) => {
                sink.progress("install", "", Some(0.02));
                let rc = helper_step_rc(*op, de, &sink, &session_run_live, &|| {
                    let installed = read(HELPER_PATH);
                    // The former path — the same moment the session helper is put in
                    // place too: from the next window run there is one password.
                    let copy = session_helper_copy_live();
                    let lang = crate::i18n::current_language();
                    let argv = helper_argv_with(*op, de, installed.as_deref(), &lang, copy.as_ref());
                    run_helper(&argv, sink.clone())
                });
                if rc != Some(0) {
                    let text = helper_failure_text(rc, de_name);
                    sink.progress(de_progress::STAGE_ERROR, &text, None);
                    return outcome(false, text);
                }
            }
        }
    }
    if let Some(w) = &warning {
        sink.progress(de_progress::STAGE_WARNING, w, None);
    }
    sink.progress("done", "", Some(1.0));
    outcome(true, warning.unwrap_or(done_text))
}

/// Privilege session operation for the helper step.
pub fn priv_op(op: HelperOp, de_id: &str) -> crate::privilege::op::PrivOp {
    use crate::privilege::op::{PrivOp, SysextOp};
    let op = match op {
        HelperOp::Install => SysextOp::Install,
        HelperOp::Remove => SysextOp::Remove,
    };
    PrivOp::Sysext { op, de_id: de_id.to_string() }
}

/// The window's live privilege session (`privilege::run`); `None` — there is
/// none or the helper doesn't carry out the operation: then the former path.
fn session_run_live(
    op: &crate::privilege::op::PrivOp,
    line: &mut dyn FnMut(&str),
) -> Option<crate::system_exec::RunOut> {
    crate::privilege::run::run_live(op, &[], None, line)
}

/// Code of the helper step: first the window's privilege session (one password
/// for the whole run; on SteamOS the session helper lives in
/// `/var/lib/rexon/bin`), the progress lines are the same `@progress`/log as
/// with `rexon-sysext-helper`. No session — `legacy` (the helper's former
/// `pkexec`); a session refusal is not repeated by the former path (Z5).
pub fn helper_step_rc(
    op: HelperOp,
    de_id: &str,
    sink: &Arc<dyn DeOpSink>,
    session: &dyn Fn(&crate::privilege::op::PrivOp, &mut dyn FnMut(&str)) -> Option<crate::system_exec::RunOut>,
    legacy: &dyn Fn() -> Option<i32>,
) -> Option<i32> {
    let mut line = |l: &str| match parse_helper_line(l) {
        HelperLine::Progress(f) => sink.progress("install", "", Some(0.02 + f * 0.93)),
        HelperLine::Log(t) => sink.progress(de_progress::STAGE_RAW, &t, None),
        HelperLine::Skip => {}
    };
    match session(&priv_op(op, de_id), &mut line) {
        Some(r) => r.returncode,
        None => legacy(),
    }
}

/// Run the helper, showing its output; the exit code.
fn run_helper(argv: &[String], sink: Arc<dyn DeOpSink>) -> Option<i32> {
    let dir = std::env::temp_dir();
    let log = dir.join(format!("rexon-sysext-{}.log", std::process::id()));
    let log_path = log.to_string_lossy().into_owned();
    let emit_sink = sink.clone();
    let mut tail = crate::system_plan::tail_log(&log_path, crate::system_plan::LogTailKind::Raw, move |_stage, line| {
        match parse_helper_line(line) {
            HelperLine::Progress(f) => emit_sink.progress("install", "", Some(0.02 + f * 0.93)),
            HelperLine::Log(l) => emit_sink.progress(de_progress::STAGE_RAW, &l, None),
            HelperLine::Skip => {}
        }
    });
    let spec = crate::utils::RunSpec {
        argv: argv.to_vec(),
        output: crate::utils::RunOutput::File(log_path.clone()),
        timeout_secs: Some(crate::utils::DOWNLOAD_TIMEOUT_SECS),
        stderr_to_stdout: true,
        ..crate::utils::RunSpec::default()
    };
    let report = crate::utils::run(&spec, None);
    tail.stop();
    let _ = std::fs::remove_file(&log);
    report.returncode
}

fn de_name(de_id: &str) -> String {
    desktop(de_id).map(|d| d.name.to_string()).unwrap_or_else(|| de_id.to_string())
}

fn refused(sink: &Arc<dyn DeOpSink>, text: String) -> DeOpOutcome {
    sink.progress(de_progress::STAGE_ERROR, &text, None);
    DeOpOutcome { ok: false, message: text, cancelled: false, restart_needed: false }
}

/// Installing a desktop on SteamOS — from the «Desktops» window.
pub fn install_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, de_id: &str) -> DeOpOutcome {
    let state = live_state();
    match install_plan(de_id, &state) {
        Ok(steps) => run_plan(sink, cancel, &de_name(de_id), &steps, install_done_text(&de_name(de_id))),
        Err(r) => refused(&sink, r.text(state.device)),
    }
}

/// Removing a desktop on SteamOS.
pub fn remove_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, de_id: &str, current_de: &str) -> DeOpOutcome {
    let state = live_state();
    match remove_plan(de_id, current_de, &state) {
        Ok(steps) => run_plan(sink, cancel, &de_name(de_id), &steps, crate::de_progress::MSG_OK.to_string()),
        Err(r) => refused(&sink, r.text(state.device)),
    }
}

/// The gist of a `passwd` refusal in one line: input prompts ("New password:")
/// and the program name are dropped ("BAD PASSWORD: The password is shorter than
/// 8 characters" → "The password is shorter than 8 characters").
pub fn passwd_complaint(output: &str) -> String {
    let parts: Vec<String> = output
        .split(['\n', '\r'])
        .map(|l| l.trim().trim_end_matches(':').trim().to_string())
        .map(|l| l.rsplit(": ").next().unwrap_or(&l).trim().to_string())
        .filter(|l| !l.ends_with("password"))
        .filter(|l| !l.is_empty())
        .collect();
    let mut seen: Vec<String> = Vec::new();
    for p in parts {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    if seen.is_empty() { "passwd".to_string() } else { seen.join(" ") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    // ── helper step via the window's privilege session ─────────────────

    #[test]
    fn helper_step_goes_through_session_then_legacy_only_without_it() {
        use crate::privilege::op::{PrivOp, SysextOp};
        use crate::system_de_exec::tests::Recorder;
        use std::cell::Cell;
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let legacy_calls = Cell::new(0);
        let legacy = || {
            legacy_calls.set(legacy_calls.get() + 1);
            Some(0)
        };
        // There is a session: operation Sysext, the helper's progress goes to the window, no former path.
        let seen = std::sync::Mutex::new(Vec::new());
        let session = |op: &PrivOp, line: &mut dyn FnMut(&str)| {
            seen.lock().unwrap().push(op.clone());
            line("@progress 50");
            line("sx_built");
            Some(crate::system_exec::RunOut { returncode: Some(3), ..crate::system_exec::RunOut::missing() })
        };
        assert_eq!(helper_step_rc(HelperOp::Remove, "xfce", &sink, &session, &legacy), Some(3));
        assert_eq!(legacy_calls.get(), 0, "отказ или сбой сеанса прежним путём не повторяется");
        assert_eq!(seen.lock().unwrap().as_slice(), [PrivOp::Sysext { op: SysextOp::Remove, de_id: "xfce".into() }]);
        let events = rec.events.lock().unwrap().clone();
        assert!(events.iter().any(|(st, _, f)| st == "install" && (f.unwrap_or(0.0) - 0.485).abs() < 1e-9), "{events:?}");
        assert!(events.iter().any(|(st, m, _)| st == de_progress::STAGE_RAW && m == "sx_built"), "{events:?}");
        // No session — the helper's former pkexec.
        let none = |_: &PrivOp, _: &mut dyn FnMut(&str)| None;
        assert_eq!(helper_step_rc(HelperOp::Install, "xfce", &sink, &none, &legacy), Some(0));
        assert_eq!(legacy_calls.get(), 1);
    }

    const OS_395: &str = "NAME=\"SteamOS\"\nID=steamos\nID_LIKE=arch\nVARIANT_ID=steamdeck\n\
                          VERSION_ID=3.9.5\nBUILD_ID=20261023.100\n";

    fn state(password: PasswordState, default_session: Option<&str>) -> SteamOsState {
        SteamOsState {
            release: steamos_release(OS_395),
            device: Device::SteamDeck,
            password,
            desktops: vec![],
            default_session: default_session.map(str::to_string),
        }
    }

    // ── Parsing ────────────────────────────────────────────────────────

    #[test]
    fn os_release_of_steam_deck_and_recovery_image() {
        let r = steamos_release(OS_395).unwrap();
        assert_eq!(r.version_id, "3.9.5");
        assert_eq!(r.variant_id, "steamdeck");
        assert_eq!(r.build_id, "20261023.100");
        // The 3.8.14 recovery image — a different VARIANT_ID, the same decision.
        let oobe = "ID=steamos\nVARIANT_ID=steamdeck-oobe\nVERSION_ID='3.8.14'\n";
        assert_eq!(steamos_release(oobe).unwrap().version_id, "3.8.14");
    }

    #[test]
    fn no_steamos_without_version_or_with_other_id() {
        // The SteamOS /usr/lib/os-release — without VERSION_ID.
        assert_eq!(steamos_release("ID=steamos\nVARIANT_ID=steamdeck\n"), None);
        assert_eq!(steamos_release("ID=arch\nVERSION_ID=3.9.5\n"), None);
        assert_eq!(steamos_release("ID=steamos\nVERSION_ID=3.9;rm -rf /\n"), None);
    }

    #[test]
    fn device_by_dmi() {
        assert_eq!(device_from_dmi("Jupiter\n", ""), Device::SteamDeck);
        assert_eq!(device_from_dmi("Galileo", ""), Device::SteamDeck);
        assert_eq!(device_from_dmi("", "Fremont"), Device::SteamMachine);
        assert_eq!(device_from_dmi("", "Standard PC (Q35 + ICH9, 2009)"), Device::Unknown);
    }

    #[test]
    fn admin_gate_asks_password_only_when_missing() {
        assert_eq!(admin_gate(PasswordState::Set, Device::SteamDeck), AdminGate::Ready);
        assert_eq!(admin_gate(PasswordState::Unknown, Device::SteamDeck), AdminGate::Ready);
        assert_eq!(admin_gate(PasswordState::Missing, Device::SteamDeck), AdminGate::AskPassword);
        assert_eq!(
            admin_gate(PasswordState::Locked, Device::SteamDeck),
            AdminGate::Refused(Refusal::Locked.text(Device::SteamDeck))
        );
    }

    #[test]
    fn passwd_status_and_new_password() {
        assert_eq!(parse_passwd_status("deck P 2026-10-24 0 99999 7 -1\n"), PasswordState::Set);
        assert_eq!(parse_passwd_status("deck NP 2026-10-24 0 99999 7 -1\n"), PasswordState::Missing);
        assert_eq!(parse_passwd_status("deck L 2026-10-24 0 99999 7 -1\n"), PasswordState::Locked);
        assert_eq!(parse_passwd_status(""), PasswordState::Unknown);
        assert!(check_new_password("abc", "abc").is_ok());
        assert!(check_new_password("", "").is_err());
        assert!(check_new_password("abc", "abd").is_err());
        assert!(check_new_password("a\nb", "a\nb").is_err());
        assert_eq!(passwd_input("s3"), "s3\ns3\n");
        assert_eq!(
            passwd_complaint("New password: Retype new password: BAD PASSWORD: The password is shorter than 8 characters\n"),
            "The password is shorter than 8 characters"
        );
    }

    #[test]
    fn desktop_list_keeps_known_ids_once() {
        assert_eq!(parse_desktop_list("xfce\n\nhyprland\nxfce\ngnome\n rm \n"), vec!["xfce", "hyprland"]);
    }

    #[test]
    fn support_per_desktop() {
        for id in ["xfce", "lxqt", "hyprland"] {
            assert!(matches!(support(id), Support::Supported(d) if d.id == id));
            // Every buildable desktop is known to the core.
            assert!(crate::system::DE_META.iter().any(|m| m.id == id), "{id}");
        }
        assert_eq!(support("kde"), Support::Builtin);
        for id in ["gnome", "cinnamon-mint", "mate", "budgie", "sway", "niri"] {
            match support(id) {
                Support::NotYet(why) => assert!(why.contains("Xfce, LXQt, Hyprland"), "{why}"),
                other => panic!("{id}: {other:?}"),
            }
        }
        assert_eq!(session_for("kde").as_deref(), Some("plasma.desktop"));
        assert_eq!(session_for("xfce").as_deref(), Some("rexon-xfce.desktop"));
        assert_eq!(session_for("gnome"), None);
    }

    // ── Plans ──────────────────────────────────────────────────────────

    #[test]
    fn install_plan_by_state() {
        let ok = install_plan("xfce", &state(PasswordState::Set, None)).unwrap();
        assert_eq!(
            ok,
            vec![Step::Helper(HelperOp::Install, "xfce".into()), Step::DefaultSession("rexon-xfce.desktop".into())]
        );
        // Password "unknown" — don't get in the way, pkexec will say so itself.
        assert!(install_plan("xfce", &state(PasswordState::Unknown, None)).is_ok());
        assert_eq!(install_plan("xfce", &state(PasswordState::Missing, None)), Err(Refusal::NeedPassword));
        assert_eq!(install_plan("xfce", &state(PasswordState::Locked, None)), Err(Refusal::Locked));
        assert!(matches!(install_plan("gnome", &state(PasswordState::Set, None)), Err(Refusal::NotYet(_))));
        assert_eq!(install_plan("kde", &state(PasswordState::Set, None)), Err(Refusal::Builtin));
        let mut arch = state(PasswordState::Set, None);
        arch.release = None;
        assert_eq!(install_plan("xfce", &arch), Err(Refusal::NotSteamOs));
    }

    #[test]
    fn remove_plan_gives_plasma_back_first() {
        let s = state(PasswordState::Set, Some("rexon-xfce.desktop"));
        assert_eq!(
            remove_plan("xfce", "kde", &s).unwrap(),
            vec![Step::DefaultSession("plasma.desktop".into()), Step::Helper(HelperOp::Remove, "xfce".into())]
        );
        // Another default desktop — the session is not touched.
        assert_eq!(
            remove_plan("hyprland", "kde", &s).unwrap(),
            vec![Step::Helper(HelperOp::Remove, "hyprland".into())]
        );
        assert_eq!(remove_plan("xfce", "xfce", &s), Err(Refusal::Current));
        assert_eq!(remove_plan("xfce", "kde", &state(PasswordState::Missing, None)), Err(Refusal::NeedPassword));
    }

    #[test]
    fn helper_argv_bootstrap_and_installed() {
        let text = helper_script();
        let boot = helper_argv(HelperOp::Install, "xfce", None, "en");
        assert_eq!(&boot[..3], &["pkexec", "/bin/bash", "-c"]);
        assert_eq!(boot[3], text);
        assert_eq!(&boot[4..], &[HELPER_NAME, "install", "xfce"]);
        // An outdated helper file — via the text again.
        assert_eq!(helper_argv(HelperOp::Install, "xfce", Some("old"), "en")[1], "/bin/bash");
        let file = helper_argv(HelperOp::Remove, "lxqt", Some(&text), "en");
        assert_eq!(file, vec!["pkexec", HELPER_PATH, "remove", "lxqt"]);
        // The language comes right after the helper.
        if crate::system_l10n::languages().contains(&"ru") {
            assert_eq!(helper_argv(HelperOp::Remove, "lxqt", Some(&text), "ru")[2], "--lang=ru");
        }
        // The text fits into one argument (MAX_ARG_STRLEN = 128 KiB).
        assert!(text.len() < 120_000, "{}", text.len());
    }

    #[test]
    fn helper_lines() {
        assert_eq!(parse_helper_line("@progress 40"), HelperLine::Progress(0.4));
        assert_eq!(parse_helper_line("@progress x"), HelperLine::Skip);
        assert_eq!(parse_helper_line("   "), HelperLine::Skip);
        assert_eq!(parse_helper_line("rexon-sysext-build: x\n"), HelperLine::Log("rexon-sysext-build: x".into()));
    }

    #[test]
    fn helper_failure_texts_differ_by_code() {
        let net = helper_failure_text(Some(75), "Xfce");
        assert!(net.contains("internet"), "{net}");
        assert_ne!(net, helper_failure_text(Some(126), "Xfce"));
        assert_ne!(net, helper_failure_text(Some(1), "Xfce"));
    }

    // ── Texts of scripts and units ─────────────────────────────────────

    fn all_scripts() -> Vec<(&'static str, String)> {
        vec![
            ("build", build_script()),
            ("caches", caches_script()),
            ("sync", sync_script()),
            ("guard", guard_script()),
            ("helper", helper_script()),
        ]
    }

    #[test]
    fn scripts_are_filled_and_parse() {
        for (name, text) in all_scripts() {
            for ph in ["__L10N__", "__IDS__", "__PRETTY__", "__DESKTOPS__", "__CORE__", "__VARS__", "__LAYOUT__", "__CACHES__", "__PLASMA__", "__LANG_FROM_FILE__", "__HELPER_PATH__"] {
                assert!(!text.contains(ph), "{name}: {ph}");
            }
            assert!(text.starts_with("#!/bin/bash\n"), "{name}");
            if Path::new("/bin/bash").exists() {
                let out = Command::new("bash").args(["-n", "-c", &text]).output().unwrap();
                assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
    }

    #[test]
    fn units_follow_the_tested_layout() {
        let g = guard_unit();
        assert!(g.contains("DefaultDependencies=no"));
        assert!(g.contains("Before=sddm.service display-manager.service"));
        assert!(g.contains("WantedBy=sddm.service"));
        assert!(g.contains("ExecStart=/bin/bash /var/lib/rexon/bin/session-guard.sh"));
        let s = sync_unit();
        assert!(s.contains("After=network-online.target"));
        assert!(!s.contains("Before=sddm"), "пересборка не держит вход");
        assert!(s.contains("Restart=on-failure\nRestartSec=120"));
        assert!(s.contains("ConditionPathExists=/var/lib/rexon/sysext/desktops"));
        assert!(helper_policy_xml().contains(HELPER_PATH));
        assert!(keep_conf().contains(HELPER_POLICY_PATH));
    }

    #[test]
    fn build_script_carries_the_lessons_of_the_vm() {
        let b = build_script();
        assert!(b.contains("\"$R/etc/os-release\""), "VERSION_ID только из /etc");
        assert!(b.contains("--gpgdir \"$G\""));
        assert!(b.contains("--populate archlinux holo"));
        assert!(b.contains("-Sp --needed"));
        assert!(b.contains("exit 75"));
        assert!(b.contains("readelf -d"));
        assert!(b.contains("local/*/files"), "библиотеки сверяются со списками файлов ОС");
        assert!(b.contains("/usr/lib/pam.d"));
        assert!(b.contains("extension-release.rexon-$DE"));
        assert!(b.contains("mksquashfs"));
        assert!(b.contains("unset-environment"));
        assert!(b.contains("XDG_FIXUP"));
        assert!(b.contains("SESSION_EXEC='xfce4-session'"), "не startxfce4");
        assert!(b.contains("mktemp -d /tmp/"), "дерево на tmpfs");
        assert!(sync_script().contains("timeout 15 runuser"));
        assert!(caches_script().contains("NAME=rexon-zz-caches"));
    }

    // ── Behaviour of the scripts on a fake root ────────────────────────

    struct Stand {
        dir: PathBuf,
    }

    impl Stand {
        fn new(tag: &str) -> Stand {
            let dir = std::env::temp_dir().join(format!("rexon-sysext-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("root/etc/sddm.conf.d")).unwrap();
            std::fs::create_dir_all(dir.join("stub")).unwrap();
            let s = Stand { dir };
            s.write("root/etc/os-release", OS_395);
            for cmd in [
                "systemd-sysext", "systemd-sysusers", "systemd-tmpfiles", "busctl", "systemctl", "runuser",
                "steamosctl", "notify-send", "umount", "mount", "pacman-key",
            ] {
                s.stub(cmd, "exit 0");
            }
            s
        }
        fn root(&self) -> PathBuf {
            self.dir.join("root")
        }
        fn write(&self, rel: &str, text: &str) {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
        fn exec(&self, rel: &str, text: &str) {
            self.write(rel, text);
            std::fs::set_permissions(self.dir.join(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        fn stub(&self, cmd: &str, body: &str) {
            self.exec(
                &format!("stub/{cmd}"),
                &format!("#!/bin/bash\necho \"{cmd} $*\" >> \"{}\"\n{body}\n", self.dir.join("calls").display()),
            );
        }
        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.dir.join(rel)).unwrap_or_default()
        }
        fn calls(&self) -> String {
            self.read("calls")
        }
        fn run(&self, script: &str, args: &[&str]) -> (i32, String) {
            let path = format!("{}:/usr/bin:/bin", self.dir.join("stub").display());
            let out = Command::new("bash")
                .arg("-c")
                .arg(script)
                .arg("test")
                .args(args)
                .env("REXON_SYSEXT_ROOT", self.root())
                .env("PATH", path)
                .env("LC_ALL", "C")
                .output()
                .unwrap();
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            (out.status.code().unwrap_or(-1), text)
        }
        /// A fake builder: an image for the current version or code `rc`.
        fn fake_build(&self, rc: i32) {
            self.exec(
                "root/var/lib/rexon/bin/rexon-sysext-build",
                &format!(
                    "#!/bin/bash\necho \"build $1\" >> \"{calls}\"\n[ {rc} = 0 ] || exit {rc}\n\
                     V=$(sed -n 's/^VERSION_ID=//p' \"$REXON_SYSEXT_ROOT/etc/os-release\")\n\
                     mkdir -p \"$REXON_SYSEXT_ROOT/home/.rexon/sysext\" \"$REXON_SYSEXT_ROOT/var/lib/extensions\"\n\
                     touch \"$REXON_SYSEXT_ROOT/home/.rexon/sysext/rexon-$1-$V.raw\"\n\
                     ln -sfn \"/home/.rexon/sysext/rexon-$1-$V.raw\" \"$REXON_SYSEXT_ROOT/var/lib/extensions/rexon-$1.raw\"\n",
                    calls = self.dir.join("calls").display()
                ),
            );
            self.exec(
                "root/var/lib/rexon/bin/rexon-sysext-caches",
                &format!("#!/bin/bash\necho caches >> \"{}\"\necho changed\n", self.dir.join("calls").display()),
            );
        }
        fn image_for(&self, de: &str, v: &str) {
            self.write(&format!("root/home/.rexon/sysext/rexon-{de}-{v}.raw"), "");
            let link = self.root().join(format!("var/lib/extensions/rexon-{de}.raw"));
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(format!("/home/.rexon/sysext/rexon-{de}-{v}.raw"), link).unwrap();
        }
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn bash() -> bool {
        Path::new("/bin/bash").exists()
    }

    #[test]
    fn guard_falls_back_to_plasma_and_remembers_the_session() {
        if !bash() {
            return;
        }
        let s = Stand::new("guard");
        s.write("root/etc/sddm.conf.d/zz-holo-autologin.conf", "[Autologin]\nSession=rexon-xfce.desktop\n");
        s.write(
            "root/home/deck/.config/steamos-manager/state.toml",
            "version = 1\n\n[session_manager]\ndefault_login_mode = \"Desktop\"\ndesktop_session = \"rexon-xfce.desktop\"\n",
        );
        let (rc, out) = s.run(&guard_script(), &[]);
        assert_eq!(rc, 0, "{out}");
        assert_eq!(s.read("root/etc/sddm.conf.d/zz-holo-autologin.conf"), "[Autologin]\nSession=plasma.desktop\n");
        assert!(s.read("root/home/deck/.config/steamos-manager/state.toml").contains("desktop_session = \"plasma.desktop\""));
        assert!(s.read("root/home/deck/.config/steamos-manager/state.toml").contains("default_login_mode = \"Desktop\""));
        assert_eq!(s.read("root/var/lib/rexon/session-guard/wanted-session"), "rexon-xfce.desktop\n");
    }

    #[test]
    fn guard_leaves_an_existing_session_alone() {
        if !bash() {
            return;
        }
        let s = Stand::new("guard-ok");
        s.write("root/etc/sddm.conf.d/zz-steamos-autologin.conf", "[Autologin]\nSession=rexon-xfce.desktop\n");
        s.write("root/usr/share/xsessions/rexon-xfce.desktop", "[Desktop Entry]\n");
        let (rc, _) = s.run(&guard_script(), &[]);
        assert_eq!(rc, 0);
        assert_eq!(s.read("root/etc/sddm.conf.d/zz-steamos-autologin.conf"), "[Autologin]\nSession=rexon-xfce.desktop\n");
        assert_eq!(s.read("root/var/lib/rexon/session-guard/wanted-session"), "");
    }

    #[test]
    fn sync_does_nothing_when_images_match_the_os() {
        if !bash() {
            return;
        }
        let s = Stand::new("sync-current");
        s.fake_build(0);
        s.write("root/var/lib/rexon/sysext/desktops", "xfce\n");
        s.image_for("xfce", "3.9.5");
        let (rc, out) = s.run(&sync_script(), &[]);
        assert_eq!(rc, 0, "{out}");
        assert!(out.contains("Xfce is up to date"), "{out}");
        assert!(!s.calls().contains("build "), "{}", s.calls());
    }

    #[test]
    fn sync_rebuilds_two_desktops_after_an_os_update() {
        if !bash() {
            return;
        }
        let s = Stand::new("sync-update");
        s.fake_build(0);
        s.write("root/var/lib/rexon/sysext/desktops", "xfce\nhyprland\n");
        s.image_for("xfce", "3.8.14");
        s.image_for("hyprland", "3.8.14");
        // The guard has already restored Plasma; nobody in the session — the files are edited.
        s.write("root/var/lib/rexon/session-guard/wanted-session", "rexon-xfce.desktop\n");
        s.write("root/etc/sddm.conf.d/zz-holo-autologin.conf", "[Autologin]\nSession=plasma.desktop\n");
        s.write("root/usr/share/xsessions/rexon-xfce.desktop", "[Desktop Entry]\n");
        let (rc, out) = s.run(&sync_script(), &[]);
        assert_eq!(rc, 0, "{out}");
        let calls = s.calls();
        assert!(calls.contains("build xfce") && calls.contains("build hyprland"), "{calls}");
        assert!(calls.contains("caches"), "{calls}");
        assert!(calls.contains("systemd-sysext refresh"), "{calls}");
        assert_eq!(s.read("root/etc/sddm.conf.d/zz-holo-autologin.conf"), "[Autologin]\nSession=rexon-xfce.desktop\n");
        assert!(!s.root().join("var/lib/rexon/session-guard/wanted-session").exists());
    }

    #[test]
    fn sync_without_network_retries_and_keeps_plasma() {
        if !bash() {
            return;
        }
        let s = Stand::new("sync-nonet");
        s.fake_build(EXIT_NO_NETWORK);
        s.write("root/var/lib/rexon/sysext/desktops", "xfce\n");
        s.image_for("xfce", "3.8.14");
        s.write("root/var/lib/rexon/session-guard/wanted-session", "rexon-xfce.desktop\n");
        s.write("root/etc/sddm.conf.d/zz-holo-autologin.conf", "[Autologin]\nSession=plasma.desktop\n");
        let (rc, out) = s.run(&sync_script(), &[]);
        assert_eq!(rc, EXIT_NO_NETWORK, "{out}");
        assert!(out.contains("no network, will try again"), "{out}");
        let calls = s.calls();
        assert!(!calls.contains("caches"), "кэши не трогаем, пока стол ждёт сети: {calls}");
        assert!(!calls.contains("systemd-sysext refresh"), "{calls}");
        assert_eq!(s.read("root/etc/sddm.conf.d/zz-holo-autologin.conf"), "[Autologin]\nSession=plasma.desktop\n");
        assert_eq!(s.read("root/var/lib/rexon/session-guard/wanted-session"), "rexon-xfce.desktop\n");
    }

    #[test]
    fn helper_install_without_network_leaves_nothing_behind() {
        if !bash() {
            return;
        }
        let s = Stand::new("helper-nonet");
        std::fs::create_dir_all(s.root().join("usr/lib/holo/pacmandb/local")).unwrap();
        s.stub("pacman", "exit 1");
        let (rc, out) = s.run(&helper_script(), &["install", "xfce"]);
        assert_eq!(rc, EXIT_NO_NETWORK, "{out}");
        assert!(out.contains("no network, or Valve's package server is unreachable"), "{out}");
        assert!(out.contains("removing it from the list again"), "{out}");
        assert_eq!(s.read("root/var/lib/rexon/sysext/desktops"), "");
        // The layout is in place: scripts, units, policy, whitelist.
        for f in [
            "var/lib/rexon/bin/rexon-sysext-build",
            "var/lib/rexon/bin/rexon-sysext-sync",
            "var/lib/rexon/bin/rexon-sysext-caches",
            "var/lib/rexon/bin/session-guard.sh",
            "etc/systemd/system/rexon-session-guard.service",
            "etc/systemd/system/rexon-sysext-sync.service",
            "etc/polkit-1/actions/org.rexon.sysext.policy",
            "etc/atomic-update.conf.d/rexon.conf",
        ] {
            assert!(s.root().join(f).is_file(), "{f}");
        }
        assert_eq!(s.read("root/var/lib/rexon/bin/rexon-sysext-sync"), sync_script());
        // Run via bash -c — the helper saved itself as a file.
        assert_eq!(s.read(&format!("root{HELPER_PATH}")), helper_script());
        assert!(s.calls().contains("systemctl enable systemd-sysext.service rexon-session-guard.service rexon-sysext-sync.service"));
    }

    #[test]
    fn helper_remove_drops_the_image_and_gives_plasma_back() {
        if !bash() {
            return;
        }
        let s = Stand::new("helper-remove");
        s.write("root/var/lib/rexon/sysext/desktops", "xfce\n");
        s.image_for("xfce", "3.9.5");
        s.write("root/etc/sddm.conf.d/zz-holo-autologin.conf", "[Autologin]\nSession=rexon-xfce.desktop\n");
        let (rc, out) = s.run(&helper_script(), &["remove", "xfce"]);
        assert_eq!(rc, 0, "{out}");
        assert_eq!(s.read("root/var/lib/rexon/sysext/desktops"), "");
        assert!(!s.root().join("var/lib/extensions/rexon-xfce.raw").exists());
        assert!(!s.root().join("home/.rexon/sysext/rexon-xfce-3.9.5.raw").exists());
        assert!(s.calls().contains("systemd-sysext refresh"), "{}", s.calls());
        assert_eq!(s.read("root/etc/sddm.conf.d/zz-holo-autologin.conf"), "[Autologin]\nSession=plasma.desktop\n");
        // The guard must not "restore" a removed session.
        assert!(!s.root().join("var/lib/rexon/session-guard/wanted-session").exists());
    }

    // ── privilege session helper on SteamOS (B14′) ─────────────────────

    /// The helper path is where the client looks for it; the policy is the same as
    /// the package's, with its own `exec.path`; `/etc` survives an update via the
    /// whitelist, `/var` by itself.
    #[test]
    fn session_helper_lives_where_the_client_looks_and_its_policy_is_kept() {
        use crate::privilege::client;
        use crate::system_plan as plan;
        assert_eq!(SESSION_HELPER_PATH, plan::helper_path_in(client::STEAMOS_HELPER_DIR, client::HELPER_NAME));
        assert!(client::helper_candidates().contains(&PathBuf::from(SESSION_HELPER_PATH)));
        assert_eq!(SESSION_POLICY_PATH, format!("/etc/polkit-1/actions/{}.policy", plan::SESSION_ACTION));
        assert_eq!(Path::new(SESSION_POLICY_PATH).parent(), Path::new(HELPER_POLICY_PATH).parent());
        let policy = layout_files().into_iter().find(|f| f.path == SESSION_POLICY_PATH).expect("политика в раскладке");
        assert_eq!(policy.mode, 0o644);
        assert_eq!(policy.text, plan::session_policy_xml_for(SESSION_HELPER_PATH));
        assert!(policy.text.contains(&format!(
            "<annotate key=\"org.freedesktop.policykit.exec.path\">{SESSION_HELPER_PATH}</annotate>"
        )));
        assert!(keep_conf().lines().any(|l| l == SESSION_POLICY_PATH));
        assert!(SESSION_HELPER_PATH.starts_with("/var/"), "не в /usr и не в /etc");
        // The binary itself is not part of the layout: only the bash -c path puts it there.
        assert!(layout_files().iter().all(|f| f.path != SESSION_HELPER_PATH));
    }

    const SUM_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SUM_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn session_helper_copied_only_when_missing_other_or_with_bad_rights() {
        let d = Some(("/home/deck/Rexon/bin/rexon-priv-helper", SUM_A));
        let want = Some(SessionHelperCopy { source: d.unwrap().0.into(), sha256: SUM_A.into() });
        assert_eq!(session_helper_copy(None, None, false), None, "в поставке нет — класть нечего");
        assert_eq!(session_helper_copy(d, Some(SUM_A), true), None, "тот же бинарь, права годны");
        assert_eq!(session_helper_copy(d, None, false), want, "нет файла");
        assert_eq!(session_helper_copy(d, Some(SUM_B), true), want, "другая сборка");
        assert_eq!(session_helper_copy(d, Some(SUM_A), false), want, "права испорчены");
        // Invalid path or sum doesn't go into root's argv.
        assert_eq!(session_helper_copy(Some(("rel/helper", SUM_A)), None, false), None);
        assert_eq!(session_helper_copy(Some(("/x", &SUM_A.to_uppercase())), None, false), None);
        assert_eq!(session_helper_copy(Some(("/x", "abc")), None, false), None);
    }

    #[test]
    fn helper_argv_with_copy_always_goes_through_bash_c() {
        let text = helper_script();
        let copy = SessionHelperCopy { source: "/home/deck/r/bin/rexon-priv-helper".into(), sha256: SUM_A.into() };
        let argv = helper_argv_with(HelperOp::Install, "xfce", Some(&text), "ru", Some(&copy));
        assert_eq!(&argv[..3], ["pkexec", "/bin/bash", "-c"], "путь к бинарю файл помощника не получает");
        let tail: Vec<&str> = argv[5..].iter().map(String::as_str).collect();
        assert_eq!(
            tail,
            [
                "--lang=ru",
                "--priv-helper=/home/deck/r/bin/rexon-priv-helper",
                &format!("--priv-sha256={SUM_A}"),
                "install",
                "xfce"
            ]
        );
        // Without a copy — as before: a matching file is called by itself.
        assert_eq!(helper_argv_with(HelperOp::Install, "xfce", Some(&text), "en", None), helper_argv(HelperOp::Install, "xfce", Some(&text), "en"));
        assert_eq!(helper_argv(HelperOp::Remove, "xfce", Some(&text), "en"), ["pkexec", HELPER_PATH, "remove", "xfce"]);
    }

    /// Bench for the copy: the delivery binary in the "home" directory and its sum.
    fn delivery(s: &Stand, body: &str) -> (String, String) {
        s.exec("home/deck/rexon/bin/rexon-priv-helper", body);
        let src = s.dir.join("home/deck/rexon/bin/rexon-priv-helper").to_string_lossy().into_owned();
        let sum = file_sha256(&src).unwrap();
        (src, sum)
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn helper_bash_c_places_the_session_helper_atomically_with_its_rights() {
        if !bash() {
            return;
        }
        use std::os::unix::fs::MetadataExt;
        let s = Stand::new("priv-copy");
        std::fs::create_dir_all(s.root().join("usr/lib/holo/pacmandb/local")).unwrap();
        s.stub("pacman", "exit 1");
        // A former helper of another build and an unfinished tail of a previous copy.
        s.exec(&format!("root{SESSION_HELPER_PATH}"), "old build\n");
        s.write(&format!("root{SESSION_HELPER_PATH}.rexon-new"), "junk\n");
        let old_ino = std::fs::metadata(s.root().join(&SESSION_HELPER_PATH[1..])).unwrap().ino();
        std::fs::set_permissions(s.root().join("var/lib/rexon/bin"), std::fs::Permissions::from_mode(0o775)).unwrap();
        let (src, sum) = delivery(&s, "#!/bin/sh\necho new build\n");
        let (rc, out) = s.run(&helper_script(), &[&format!("--priv-helper={src}"), &format!("--priv-sha256={sum}"), "install", "xfce"]);
        assert_eq!(rc, EXIT_NO_NETWORK, "операция идёт своим ходом: {out}");
        let dst = s.root().join(&SESSION_HELPER_PATH[1..]);
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "#!/bin/sh\necho new build\n");
        assert_eq!(mode(&dst), 0o755);
        assert_ne!(std::fs::metadata(&dst).unwrap().ino(), old_ino, "замена переименованием, не записью поверх");
        assert!(!s.root().join(format!("{}.rexon-new", &SESSION_HELPER_PATH[1..])).exists());
        // Directories without write access for group and others — otherwise the client refuses.
        for d in ["var/lib/rexon", "var/lib/rexon/bin"] {
            assert_eq!(mode(&s.root().join(d)), 0o755, "{d}");
        }
        assert_eq!(s.read(&format!("root{SESSION_POLICY_PATH}")), crate::system_plan::session_policy_xml_for(SESSION_HELPER_PATH));
        assert!(s.read(&format!("root{KEEP_CONF_PATH}")).contains(SESSION_POLICY_PATH));
    }

    #[test]
    fn helper_refuses_a_session_helper_with_another_sum_or_behind_a_link() {
        if !bash() {
            return;
        }
        let s = Stand::new("priv-refuse");
        std::fs::create_dir_all(s.root().join("usr/lib/holo/pacmandb/local")).unwrap();
        s.stub("pacman", "exit 1");
        s.exec(&format!("root{SESSION_HELPER_PATH}"), "old build\n");
        let (src, _) = delivery(&s, "#!/bin/sh\necho swapped\n");
        let dst = s.root().join(&SESSION_HELPER_PATH[1..]);
        // The sum is wrong (the file was swapped after the window computed it).
        let (rc, out) = s.run(&helper_script(), &[&format!("--priv-helper={src}"), &format!("--priv-sha256={SUM_A}"), "install", "xfce"]);
        assert_eq!(rc, EXIT_NO_NETWORK, "{out}");
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "old build\n", "прежний не тронут");
        assert!(!s.root().join(format!("{}.rexon-new", &SESSION_HELPER_PATH[1..])).exists());
        // A symlink instead of a file and a relative path — refusal.
        let link = s.dir.join("home/deck/link");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        let sum = file_sha256(&src).unwrap();
        for bad in [link.to_string_lossy().into_owned(), "home/deck/rexon/bin/rexon-priv-helper".to_string()] {
            let (_, out) = s.run(&helper_script(), &[&format!("--priv-helper={bad}"), &format!("--priv-sha256={sum}"), "install", "xfce"]);
            assert_eq!(std::fs::read_to_string(&dst).unwrap(), "old build\n", "{bad}: {out}");
        }
    }

    /// Run as a FILE (like `pkexec /var/lib/rexon/bin/rexon-sysext-helper`): the
    /// file keeps the password for minutes, and it doesn't accept the binary path.
    #[test]
    fn helper_file_never_takes_a_session_helper_from_its_caller() {
        if !bash() {
            return;
        }
        let s = Stand::new("priv-file");
        std::fs::create_dir_all(s.root().join("usr/lib/holo/pacmandb/local")).unwrap();
        s.stub("pacman", "exit 1");
        let (src, sum) = delivery(&s, "#!/bin/sh\necho evil\n");
        s.exec("helper-file", &helper_script());
        let path = format!("{}:/usr/bin:/bin", s.dir.join("stub").display());
        let out = Command::new("bash")
            .arg(s.dir.join("helper-file"))
            .args([format!("--priv-helper={src}"), format!("--priv-sha256={sum}"), "install".into(), "xfce".into()])
            .env_remove("BASH_EXECUTION_STRING")
            .env("REXON_SYSEXT_ROOT", s.root())
            .env("PATH", path)
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(EXIT_NO_NETWORK), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(!s.root().join(&SESSION_HELPER_PATH[1..]).exists());
        // The policy gets laid out meanwhile (the text from the helper itself).
        assert!(s.root().join(&SESSION_POLICY_PATH[1..]).is_file());
    }

    #[test]
    fn helper_refuses_foreign_operations_and_desktops() {
        if !bash() {
            return;
        }
        let s = Stand::new("helper-args");
        assert_eq!(s.run(&helper_script(), &["install", "../../etc"]).0, 2);
        assert_eq!(s.run(&helper_script(), &["exec", "xfce"]).0, 2);
        assert!(!s.root().join("var/lib/rexon/bin").exists());
    }
}
