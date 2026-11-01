//! Переводы того, что Rexon кладёт в систему: сообщения root-помощников,
//! ярлыки и записи автозапуска (`.desktop`), политики polkit, описание темы
//! заставки. Всё — из тех же `po/<код>.po`, вшитых при сборке (`build.rs`).
//!
//! ## Сообщения root-помощников
//!
//! Решение Wehrwolfmann 30.09.2026: то, что помощники (`rexon-de-helper`,
//! `rexon-pkg-helper`, `rexon-stage-helper`, `rexon-system-helper`) печатают
//! в журнал операции, покупатель читает — значит, это переводится.
//!
//! ⛔ Помощник работает от root, поэтому перевод не должен стать каналом
//! внедрения в него. Правила, которые нельзя ослаблять:
//!   1. язык приходит ОДНИМ аргументом `--lang=<код>` (первым, необязательно);
//!      код сверяется с белым списком, построенным из `po/LINGUAS` при
//!      генерации текста; неизвестное, пустое, с посторонними символами —
//!      английский, без ошибки;
//!   2. все тексты — из ТАБЛИЦЫ, вшитой в скрипт при его генерации: русский
//!      из `po/ru.po` (любой язык — из своего `.po`), английский — msgid.
//!      Ничего из пользовательских файлов и окружения во время работы;
//!   3. текст в скрипте — строка в одинарных кавычках (кавычка внутри
//!      экранирована), вставки `%1`..`%9` подставляются как ДАННЫЕ собственным
//!      циклом, не форматом `printf` и не оболочкой; перевод с управляющими
//!      символами или с другим набором вставок отбрасывается (английский).
//!
//! Rexon передаёт `--lang` только помощнику, в чьём тексте есть отметка
//! [`LANG_MARKER`]: помощник прежней версии (без неё) отказал бы на лишнем
//! аргументе, а так он просто продолжит говорить по-своему.

use crate::i18n::n_;

/// Отметка в тексте помощника: «понимаю `--lang=<код>`».
pub const LANG_MARKER: &str = "# REXON_HELPER_LANG_ARG=1";

/// Каталоги перевода на момент сборки: (код из `po/LINGUAS`, текст `.po`).
/// Файл печатает `build.rs`.
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/helper_catalogs.rs"));
}

/// Все сообщения помощников: (ключ в скрипте, английский текст = msgid).
///
/// Ключ — то, что пишет скрипт (`die bad_de_id "$de_id"`); `%1`, `%2` —
/// вставки, подставляемые по порядку аргументов после ключа.
pub const MESSAGES: &[(&str, &str)] = &[
    // ── общие ───────────────────────────────────────────────────────────
    // TRANSLATORS: Printed by Rexon's root helper into the operation log (all helper messages are). "pkexec" is a program name.
    ("not_via_pkexec", n_("not started through pkexec")),
    // TRANSLATORS: Root helper message. "staged" is the name of the helper's argument (the prepared folder); %1 is a path.
    ("staged_not_dir", n_("staged is not a directory: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("staged_unreachable", n_("staged is not accessible: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("staged_outside_tmp", n_("staged is outside /tmp|/var/tmp: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("staged_not_callers", n_("staged does not belong to the caller: %1")),
    // TRANSLATORS: Root helper message.
    ("no_work_dir", n_("cannot create a working directory")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_category", n_("invalid category: %1")),
    // TRANSLATORS: Root helper message. %1 is a package manager family (arch, debian, …).
    ("unsupported_family", n_("unsupported family: %1")),
    // ── rexon-de-helper ─────────────────────────────────────────────────
    // TRANSLATORS: Root helper message. de_id, family, log_path are argument names.
    ("de_args", n_("expected 2-3 arguments: de_id family [log_path]")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_de_id", n_("bad de_id: %1")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_family_arg", n_("bad family: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("log_outside_tmp", n_("log_path is outside /tmp|/var/tmp: %1")),
    // TRANSLATORS: Root helper message. %1 is a numeric user id.
    ("no_user_group", n_("user %1 has no group")),
    // TRANSLATORS: Root helper message. setpriv and util-linux are program names.
    ("no_setpriv", n_("setpriv (util-linux) is missing — the progress log is not kept")),
    // TRANSLATORS: Root helper message. %1 looks like "plasma|arch".
    ("unknown_de_pair", n_("unknown desktop environment/family pair: %1")),
    // TRANSLATORS: Root helper message. %1 looks like "plasma|arch".
    ("empty_pkg_list", n_("empty package list for %1")),
    // TRANSLATORS: Root helper message.
    ("db_update_failed", n_("package database update failed")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name (sddm, gdm, …).
    ("dm_already", n_("a login screen is already set up (%1) — leaving it as is")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name.
    ("dm_enabled", n_("display manager enabled: %1")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name, %2 is the error text of systemctl.
    ("dm_enable_failed", n_("could not enable %1: %2")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name.
    ("dm_enable_failed_plain", n_("could not enable %1: systemctl returned an error")),
    // TRANSLATORS: Root helper message.
    ("dm_none", n_("login screen not enabled (no installed display manager from this desktop's list)")),
    // ── rexon-pkg-helper ────────────────────────────────────────────────
    // TRANSLATORS: Root helper message. family, op, pkg are argument names.
    ("pkg_args", n_("expected at least 2 arguments: family op [pkg...]")),
    // TRANSLATORS: Root helper message.
    ("family_unknown", n_("could not determine the package manager family of this machine")),
    // TRANSLATORS: Root helper message. %1 and %2 are package manager families (arch, debian, …).
    ("family_mismatch", n_("family from the argument (%1) does not match the one detected on this machine (%2) — using the detected one")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_op", n_("invalid operation: %1")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("arg_dash", n_("argument starts with '-' (flag injection?): %1")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("not_a_name", n_("a package name is expected, not a path: %1")),
    // TRANSLATORS: Root helper message. %1 is a package name.
    ("vital_pkg", n_("refusing to remove a vital package: %1")),
    // TRANSLATORS: Root helper message. %1 is a package name.
    ("current_de_pkg", n_("refusing to remove a package of the desktop or login screen in use: %1")),
    // TRANSLATORS: Root helper message. %1 is a package name.
    ("not_in_tables", n_("refusing to install a package that is not in Rexon's tables: %1")),
    // TRANSLATORS: Root helper message. %1 is a package manager family.
    ("no_cmd", n_("no command is set for %1")),
    // TRANSLATORS: Root helper message. %1 is a program name.
    ("no_pm", n_("package manager not found: %1")),
    // TRANSLATORS: Root helper message. "update" is an operation name.
    ("update_no_pkgs", n_("update takes no packages")),
    // TRANSLATORS: Root helper message. "enable-dm" is an operation name, DM = display manager.
    ("enable_dm_one", n_("enable-dm expects exactly one DM")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_dm", n_("invalid display manager: %1")),
    // TRANSLATORS: Root helper message. systemctl is a program name.
    ("no_systemctl", n_("systemctl not found")),
    // TRANSLATORS: Root helper message. %1 is an operation name (install, remove).
    ("op_needs_pkg", n_("%1 needs at least one package")),
    // TRANSLATORS: Root helper message. "switch-dm" is an operation name; unit and greeter are argument names.
    ("switch_dm_args", n_("switch-dm expects a login screen and, for LightDM, a greeter: unit [greeter]")),
    // TRANSLATORS: Root helper message. systemd is a program name.
    ("no_systemd", n_("this system isn't running systemd — the login screen is chosen by its own init")),
    // TRANSLATORS: Root helper message.
    ("dm_backend_unsupported", n_("on this system Rexon can't switch the login screen yet")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name (sddm, lightdm, …).
    ("dm_no_unit", n_("the service of the login screen %1 is not installed")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name.
    ("dm_masked", n_("the login screen %1 is switched off in the system (masked)")),
    // TRANSLATORS: Root helper message. %1 is a program name.
    ("dm_no_program", n_("the login screen program %1 is missing")),
    // TRANSLATORS: Root helper message. %1 is a PAM service file name.
    ("dm_no_pam", n_("the sign-in settings file (PAM) %1 is missing")),
    // TRANSLATORS: Root helper message. greetd is a program name.
    ("dm_text_login", n_("greetd is set up for text sign-in — the desktop won't start")),
    // TRANSLATORS: Root helper message. %1 is a program name.
    ("dm_no_greeter_prog", n_("the greeter %1 is not installed")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_greeter", n_("invalid greeter: %1")),
    // TRANSLATORS: Root helper message. LightDM is a program name.
    ("greeter_not_lightdm", n_("a greeter can only be chosen for LightDM")),
    // TRANSLATORS: Root helper message. Xorg is a program name.
    ("dm_no_xorg", n_("this login screen needs Xorg, and Xorg is not installed")),
    // TRANSLATORS: Root helper message. LightDM is a program name.
    ("lightdm_no_greeter", n_("LightDM has no greeter installed")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name.
    ("dm_already_default", n_("%1 is already the login screen — nothing to change")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name, %2 is the error text of systemctl.
    ("dm_switch_failed", n_("could not make %1 the login screen: %2 — the previous one is kept")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name.
    ("dm_switch_mismatch", n_("after switching, the system doesn't show %1 as the login screen — the previous one is restored")),
    // TRANSLATORS: Root helper message. %1 and %2 are login screen service names.
    ("dm_switched", n_("%1 is now the login screen (it was %2); it appears after a restart")),
    // TRANSLATORS: Root helper message. %1 is a login screen service name.
    ("dm_switched_first", n_("%1 is now the login screen; it appears after a restart")),
    // TRANSLATORS: Line of a text file read from a text console. %1 and %2 are login screen service names.
    ("prev_dm_header", n_("Rexon changed the login screen from %1 to %2.")),
    // TRANSLATORS: Line of a text file read from a text console. %1 is a login screen service name.
    ("prev_dm_back", n_("To bring back %1 from a text console (Ctrl+Alt+F3), sign in and run:")),
    // TRANSLATORS: Line of a text file read from a text console. %1 and %2 are login screen service names.
    ("prev_dm_auto", n_("If %1 fails to start, Rexon brings back %2 by itself on that boot.")),
    // TRANSLATORS: Root helper message. %1 is an operation name (build-deps, install-built); pacman is a program name.
    ("rebuild_arch_only", n_("%1 works only with pacman (Arch)")),
    // TRANSLATORS: Root helper message. "install-built" is an operation name; plain, replace, file, pkg are argument names.
    ("built_args", n_("install-built expects: plain|replace file [pkg...]")),
    // TRANSLATORS: Root helper message. makepkg is a program name; %1 is a path.
    ("built_bad_name", n_("not a package file made by makepkg: %1")),
    // TRANSLATORS: Root helper message. %1 is a numeric user id.
    ("built_no_home", n_("cannot find the home folder of user %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("built_outside", n_("the package file is outside the caller's Rexon rebuild folder: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("built_unreachable", n_("the package file is not accessible: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("built_not_callers", n_("the package file does not belong to the caller: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("built_not_file", n_("the package file is a link or not a regular file: %1")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("built_copy_failed", n_("cannot copy the package file: %1")),
    // TRANSLATORS: Root helper message: the packages installed only to build were not removed (not an error).
    ("build_only_kept", n_("build-only packages kept")),
    // TRANSLATORS: Root helper message. %1 is a package name: Rexon asked to remove it after the build, but this build did not install it, so it stays.
    ("build_only_not_ours", n_("%1 was not installed for this build — kept")),
    // ── rexon-stage-helper ──────────────────────────────────────────────
    // TRANSLATORS: Root helper message. category, name, staged are argument names.
    ("stage_args", n_("expected 3 arguments: category name staged")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_name", n_("invalid name: %1")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_name_chars", n_("invalid characters in the name: %1")),
    // TRANSLATORS: Root helper message. "staged" is the prepared folder.
    ("staged_copy_failed", n_("cannot copy staged")),
    // TRANSLATORS: Root helper message. %1 is a file inside the prepared folder.
    ("staged_odd", n_("staged contains a link or a special file: %1")),
    // TRANSLATORS: Root helper message.
    ("no_image", n_("staged has no image file")),
    // TRANSLATORS: Root helper message. %1 is a path.
    ("bg_installed", n_("background installed: %1")),
    // TRANSLATORS: Root helper message. lightdm-webkit2-greeter and web-greeter are package names.
    ("no_greeter_dir", n_("greeter theme folder not found (lightdm-webkit2-greeter or web-greeter is required)")),
    // TRANSLATORS: Root helper message. %1 is a theme name.
    ("webkit_theme", n_("lightdm-webkit theme: %1")),
    // TRANSLATORS: Root helper message. .layout/.dock are file types.
    ("no_layouts", n_("staged has no .layout/.dock files")),
    // TRANSLATORS: Root helper message. mate-layouts are MATE panel layouts.
    ("layouts_installed", n_("mate-layouts installed")),
    // ── rexon-system-helper ─────────────────────────────────────────────
    // TRANSLATORS: Root helper message. category, theme_id, staged, family are argument names.
    ("sys_args", n_("expected 4 arguments: category theme_id staged family")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_theme_name", n_("invalid theme name: %1")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_theme_chars", n_("invalid characters in the theme name: %1")),
    // TRANSLATORS: Root helper message. %1 is the rejected argument.
    ("bad_family", n_("invalid family: %1")),
    // TRANSLATORS: Root helper message.
    ("theme_copy_failed", n_("cannot copy the theme")),
    // TRANSLATORS: Root helper message. %1 is a file inside the theme.
    ("theme_odd", n_("the theme contains a link or a special file: %1")),
    // TRANSLATORS: Root helper message. %1 is a file inside the theme.
    ("nul_byte", n_("%1 contains a null byte")),
    // TRANSLATORS: Root helper message. %1 is a file inside the theme; ModuleName is a Plymouth key, keep it as is.
    ("bad_module", n_("%1: ModuleName is not one of Plymouth's standard modules")),
    // TRANSLATORS: Root helper message. %1 is a file inside the theme.
    ("theme_has_lib", n_("the theme contains a library (%1) — code in a boot splash theme is not allowed")),
    // TRANSLATORS: Root helper message. %1 is a file name like "neon.plymouth".
    ("no_plymouth_file", n_("the theme has no %1 file")),
    // TRANSLATORS: Root helper message: "Apply" was pressed for a login screen or boot splash theme that is no longer in the system folder. %1 is the theme name.
    ("theme_not_installed", n_("the theme %1 is not installed in the system folder")),
    // TRANSLATORS: Root helper message: "Remove" was pressed for the login screen or boot splash theme that is selected right now. %1 is the theme name.
    ("theme_active", n_("the theme %1 is selected right now — choose another one first")),
    // TRANSLATORS: Root helper message: the theme folder belongs to a distribution package. %1 is the theme name.
    ("theme_from_package", n_("the theme %1 belongs to a system package — remove it with the package manager")),
    // TRANSLATORS: Root helper message. %1 is a file name like "neon.plymouth"; ModuleName is a Plymouth key.
    ("no_stock_module", n_("%1 has no ModuleName of a standard Plymouth module")),
    // TRANSLATORS: Root helper message. Program names stay as they are.
    ("no_activation", n_("no way to enable the boot splash theme: neither plymouth-set-default-theme nor update-alternatives is available")),
    // TRANSLATORS: Root helper message: the boot splash theme was chosen, but the boot image (initramfs) could not be rebuilt, so the previous theme was put back.
    ("initramfs_failed", n_("couldn't rebuild the boot image, so the new boot splash can't be shown — the previous theme is kept")),
    // TRANSLATORS: Root helper message: the login screen theme is set in a file Rexon must not change (for example a link). %1 is the theme name.
    ("theme_set_elsewhere", n_("the login screen theme is set in a file Rexon doesn't change — %1 can't be selected here")),
    // ── SteamOS: рабочие столы образами systemd-sysext (steamos_sysext) ──
    // TRANSLATORS: Printed by Rexon's SteamOS scripts (desktops built as system extensions). %1 is the rejected value.
    ("sx_bad_desktop", n_("unknown desktop: %1")),
    // TRANSLATORS: SteamOS script message. VERSION_ID and /etc/os-release stay as they are.
    ("sx_no_version", n_("no usable VERSION_ID in /etc/os-release")),
    // TRANSLATORS: SteamOS script message. install and remove are the operation names, keep them.
    ("sx_helper_args", n_("expected: install|remove <desktop>")),
    // TRANSLATORS: SteamOS script message.
    ("sx_layout_failed", n_("could not write Rexon's SteamOS scripts")),
    // TRANSLATORS: SteamOS script message.
    ("sx_enable_failed", n_("could not enable the system extension services")),
    // TRANSLATORS: SteamOS script message. %1 is a desktop id.
    ("sx_rollback", n_("%1 was not built — removing it from the list again")),
    // TRANSLATORS: SteamOS script message. %1 is a desktop id.
    ("sx_removed", n_("%1 removed")),
    // TRANSLATORS: SteamOS script message.
    ("sx_keyring", n_("preparing the package signing keys")),
    // TRANSLATORS: SteamOS script message.
    ("sx_sync_db", n_("updating the package lists of this SteamOS version")),
    // TRANSLATORS: SteamOS script message.
    ("sx_no_network", n_("no network, or Valve's package server is unreachable")),
    // TRANSLATORS: SteamOS script message.
    ("sx_nothing_to_add", n_("nothing to add: everything is already in the system")),
    // TRANSLATORS: SteamOS script message. %1 is a number.
    ("sx_packages", n_("packages to add: %1")),
    // TRANSLATORS: SteamOS script message. %1 is a list of package names.
    ("sx_shadow_core", n_("the set would replace parts of SteamOS itself, stopping: %1")),
    // TRANSLATORS: SteamOS script message. %1 is a number.
    ("sx_downloading", n_("downloading and checking %1 packages")),
    // TRANSLATORS: SteamOS script message.
    ("sx_download_failed", n_("download failed")),
    // TRANSLATORS: SteamOS script message.
    ("sx_unpacking", n_("unpacking packages")),
    // TRANSLATORS: SteamOS script message.
    ("sx_checking_libs", n_("checking that all libraries are there")),
    // TRANSLATORS: SteamOS script message. %1 is a round number, %2 a list of library file names.
    ("sx_soname_round", n_("pass %1: adding packages for the libraries %2")),
    // TRANSLATORS: SteamOS script message. %1 is a list of library file names.
    ("sx_no_provider", n_("no package provides: %1")),
    // TRANSLATORS: SteamOS script message. %1 is a package name, %2 "name version" in the system, %3 the version and file the image adds.
    ("sx_also_in_os", n_("note: %1 is also in the system (%2), the image adds %3")),
    // TRANSLATORS: SteamOS script message. %1 is a file path.
    ("sx_unresolved", n_("libraries are missing for %1")),
    // TRANSLATORS: SteamOS script message. %1 is a file path, %2 a library file name.
    ("sx_drop_plugin", n_("optional plugin left out: %1 (needs %2)")),
    // TRANSLATORS: SteamOS script message. /usr/lib/pam.d stays as is; %1 is a list of service names.
    ("sx_pam", n_("login services via /usr/lib/pam.d: %1")),
    // TRANSLATORS: SteamOS script message. %1 is a list of file paths.
    ("sx_etc_left", n_("files under /etc not carried over: %1")),
    // TRANSLATORS: SteamOS script message. %1 is a list of file names.
    ("sx_hidden_autostart", n_("Plasma autostart entries hidden in this desktop: %1")),
    // TRANSLATORS: SteamOS script message.
    ("sx_packing", n_("packing the image")),
    // TRANSLATORS: SteamOS script message. %1 is a file path, %2 its size, %3 a number of packages.
    ("sx_built", n_("image ready: %1 (%2, packages: %3)")),
    // TRANSLATORS: SteamOS script message.
    ("sx_caches_removed", n_("no desktops left: the shared cache image is removed")),
    // TRANSLATORS: SteamOS script message.
    ("sx_caches_current", n_("the shared cache image is up to date")),
    // TRANSLATORS: SteamOS script message. %1 is a file path.
    ("sx_caches_built", n_("shared cache image ready: %1")),
    // TRANSLATORS: SteamOS script message.
    ("sx_no_desktops", n_("no desktops to build")),
    // TRANSLATORS: SteamOS script message. %1 is a desktop name.
    ("sx_up_to_date", n_("%1 is up to date")),
    // TRANSLATORS: SteamOS script message. %1 is a desktop name, %2 the old image file (may be empty).
    ("sx_rebuilding", n_("%1: the image is for another SteamOS version (%2), rebuilding")),
    // TRANSLATORS: SteamOS script message. %1 is a desktop name.
    ("sx_retry_later", n_("%1: no network, will try again in two minutes")),
    // TRANSLATORS: SteamOS script message. %1 is a desktop name, %2 an exit code.
    ("sx_build_failed", n_("%1: build failed (code %2)")),
    // TRANSLATORS: SteamOS script message: the system extensions are being attached to /usr.
    ("sx_merging", n_("attaching the images to the system")),
    // TRANSLATORS: SteamOS script message. %1 is a session file name, %2 how (steamosctl or files).
    ("sx_session_restored", n_("Desktop Mode session restored: %1 (%2)")),
    // TRANSLATORS: SteamOS script message. %1 is the notification title.
    ("sx_notified", n_("notification shown: %1")),
    // TRANSLATORS: SteamOS script message.
    ("sx_not_notified", n_("could not show a notification")),
    // TRANSLATORS: SteamOS script message. %1 is a session file name, %2 a config file path.
    ("sx_guard_fallback", n_("session %1 is missing, KDE Plasma takes its place in %2")),
    // TRANSLATORS: SteamOS script message. %1 is a file path, %2 a session file name.
    ("sx_guard_state", n_("%1: session %2 is missing, KDE Plasma takes its place")),
    // TRANSLATORS: Desktop notification title (SteamOS). %1 is a desktop name, %2 the SteamOS version.
    ("sx_n_updating_title", n_("Rexon: updating %1 for SteamOS %2")),
    // TRANSLATORS: Desktop notification text (SteamOS).
    ("sx_n_updating_body", n_("KDE Plasma is used until it is ready.")),
    // TRANSLATORS: Desktop notification title (SteamOS).
    ("sx_n_wait_title", n_("Rexon: waiting for the internet")),
    // TRANSLATORS: Desktop notification text (SteamOS). %1 is a desktop name, %2 the SteamOS version.
    ("sx_n_wait_body", n_("%1 will be rebuilt for SteamOS %2 as soon as the internet is available.")),
    // TRANSLATORS: Desktop notification title (SteamOS). %1 is one or more desktop names, %2 the SteamOS version.
    ("sx_n_ready_title", n_("Rexon: %1 ready for SteamOS %2")),
    // TRANSLATORS: Desktop notification text (SteamOS).
    ("sx_n_ready_body", n_("Log out or enter Desktop Mode again to use it.")),
    // TRANSLATORS: Desktop notification title (SteamOS). %1 is one or more desktop names.
    ("sx_n_failed_title", n_("Rexon: could not rebuild %1")),
    // TRANSLATORS: Desktop notification text (SteamOS).
    ("sx_n_failed_body", n_("KDE Plasma stays the desktop. Open Rexon for details.")),
];

/// Английский текст сообщения по ключу.
pub fn message(key: &str) -> Option<&'static str> {
    MESSAGES.iter().find(|(k, _)| *k == key).map(|(_, m)| *m)
}

/// Годится ли код языка в белый список помощника: буквы, цифры, `_`, `@`
/// (`ru`, `pt_BR`, `sr@latin`). Всё прочее в скрипт не попадает.
pub fn is_valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 16
        && code.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '@'))
}

/// Языки помощника: коды из вшитого `po/LINGUAS`, прошедшие проверку.
/// Английский (исходный) в список не входит — он действует по умолчанию.
pub fn languages() -> Vec<&'static str> {
    languages_in(embedded::HELPER_CATALOGS)
}

fn languages_in<'a>(catalogs: &[(&'a str, &str)]) -> Vec<&'a str> {
    catalogs
        .iter()
        .map(|(code, _)| *code)
        .filter(|c| is_valid_code(c) && *c != crate::i18n::SOURCE_LANGUAGE)
        .collect()
}

// ============================================================================
// Разбор .po (только то, что нужно таблице: msgid → msgstr без контекста)
// ============================================================================

fn unquote(s: &str) -> Option<String> {
    let s = s.trim();
    let body = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            // Незнакомое экранирование — запись битая, перевод не берём.
            _ => return None,
        }
    }
    Some(out)
}

/// Перевод `msgid` из текста `.po`: только запись без `msgctxt`, без
/// `fuzzy`, с непустым `msgstr`. `None` — перевода нет.
pub fn po_lookup(po: &str, msgid: &str) -> Option<String> {
    #[derive(PartialEq)]
    enum F {
        None,
        Ctx,
        Id,
        Plural,
        Str,
    }
    struct Entry {
        fuzzy: bool,
        ctx: bool,
        plural: bool,
        id: String,
        msg: String,
        broken: bool,
    }
    let fresh = || Entry { fuzzy: false, ctx: false, plural: false, id: String::new(), msg: String::new(), broken: false };
    let mut e = fresh();
    let mut field = F::None;
    let mut seen_str = false;
    let done = |e: &Entry| -> Option<String> {
        (!e.fuzzy && !e.ctx && !e.plural && !e.broken && e.id == msgid && !e.msg.is_empty())
            .then(|| e.msg.clone())
    };
    for line in po.lines() {
        let t = line.trim();
        let starts_entry = t.starts_with("#,") || t.starts_with("msgctxt") || t.starts_with("msgid ");
        if starts_entry && seen_str {
            if let Some(m) = done(&e) {
                return Some(m);
            }
            e = fresh();
            seen_str = false;
            field = F::None;
        }
        if t.is_empty() {
            continue;
        }
        if let Some(flags) = t.strip_prefix("#,") {
            if flags.split(',').any(|f| f.trim() == "fuzzy") {
                e.fuzzy = true;
            }
            continue;
        }
        if t.starts_with('#') {
            continue;
        }
        let (target, rest) = if let Some(r) = t.strip_prefix("msgctxt") {
            e.ctx = true;
            (F::Ctx, r)
        } else if let Some(r) = t.strip_prefix("msgid_plural") {
            e.plural = true;
            (F::Plural, r)
        } else if let Some(r) = t.strip_prefix("msgid") {
            (F::Id, r)
        } else if let Some(r) = t.strip_prefix("msgstr") {
            seen_str = true;
            if r.starts_with('[') {
                e.plural = true;
                (F::Plural, r.split_once(']').map(|(_, x)| x).unwrap_or(""))
            } else {
                (F::Str, r)
            }
        } else if t.starts_with('"') {
            (std::mem::replace(&mut field, F::None), t)
        } else {
            e.broken = true;
            continue;
        };
        match unquote(rest) {
            Some(part) => match target {
                F::Id => e.id.push_str(&part),
                F::Str => e.msg.push_str(&part),
                _ => {}
            },
            None => e.broken = true,
        }
        field = target;
    }
    if seen_str {
        return done(&e);
    }
    None
}

/// Вставки `%1`..`%9` текста — отсортированный набор цифр.
fn placeholders(s: &str) -> Vec<char> {
    let mut v: Vec<char> = s
        .as_bytes()
        .windows(2)
        .filter(|w| w[0] == b'%' && (b'1'..=b'9').contains(&w[1]))
        .map(|w| w[1] as char)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Годится ли перевод в скрипт от root: не пустой, без управляющих
/// символов (перевод строки сломал бы журнал, остальное — терминал) и с
/// тем же набором вставок, что у msgid.
pub fn translation_ok(msgid: &str, msgstr: &str) -> bool {
    !msgstr.is_empty()
        && !msgstr.chars().any(|c| c.is_control())
        && placeholders(msgid) == placeholders(msgstr)
}

/// Перевод сообщения на язык `code` из вшитого каталога (или `None`).
pub fn translated(code: &str, msgid: &str) -> Option<String> {
    translated_in(embedded::HELPER_CATALOGS, code, msgid)
}

fn translated_in(catalogs: &[(&str, &str)], code: &str, msgid: &str) -> Option<String> {
    let (_, po) = catalogs.iter().find(|(c, _)| *c == code)?;
    po_lookup(po, msgid).filter(|m| translation_ok(msgid, m))
}

// ============================================================================
// Текст для скрипта
// ============================================================================

/// Строка bash в одинарных кавычках: внутри них не раскрывается ничего
/// (`$`, обратная кавычка, `\`), а сама кавычка — `'\''`.
pub fn bash_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Ключи сообщений, которые зовёт текст скрипта (`die ключ`, `rx_say ключ`).
pub fn keys_used(script: &str) -> Vec<&str> {
    let mut keys: Vec<&str> = Vec::new();
    for word in ["die ", "rx_say "] {
        let mut rest = script;
        while let Some(at) = rest.find(word) {
            let before = rest[..at].chars().next_back();
            rest = &rest[at + word.len()..];
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let key: &str = rest
                .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
                .next()
                .unwrap_or("");
            if !key.is_empty() && !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys.sort_unstable();
    keys
}

/// Блок «язык + таблица + вывод» для помощника `name` — встаёт в шаблон на
/// место определения `die`. `script` — текст шаблона: в таблицу идут только
/// сообщения, которые он зовёт.
///
/// Паникует на ключе без текста в [`MESSAGES`]: это ошибка программиста,
/// и сверки ловят её раньше, чем текст уйдёт в систему.
pub fn bash_block(name: &str, script: &str) -> String {
    bash_block_in(embedded::HELPER_CATALOGS, name, script)
}

fn bash_block_in(catalogs: &[(&str, &str)], name: &str, script: &str) -> String {
    assert!(
        name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
        "имя помощника {name} требует кавычек"
    );
    let keys = keys_used(script);
    let langs = languages_in(catalogs);
    let mut out = String::new();
    out.push_str(
        "# ── Язык сообщений ─────────────────────────────────────────────────\n\
         # Отметка ниже говорит Rexon, что помощник понимает --lang.\n",
    );
    out.push_str(LANG_MARKER);
    out.push('\n');
    out.push_str(
        "# Первый аргумент --lang=<код> необязателен; код сверяется с белым\n\
         # списком из po/LINGUAS, всё прочее — английский. Тексты — ТОЛЬКО из\n\
         # таблицы ниже (вшиты при генерации); вставки %1..%9 подставляются как\n\
         # данные, а не как формат printf и не как команды.\n\
         rx_lang=en\n\
         case \"${1:-}\" in\n\
         \x20   --lang=*)\n\
         \x20       case \"${1#--lang=}\" in\n",
    );
    for code in &langs {
        out.push_str(&format!("            {code}) rx_lang={code} ;;\n"));
    }
    out.push_str(
        "        esac\n\
         \x20       shift ;;\n\
         esac\n\
         rx_text() {\n\
         \x20   case \"$rx_lang:$1\" in\n",
    );
    for key in &keys {
        let en = message(key).unwrap_or_else(|| panic!("нет текста сообщения помощника {key}"));
        for code in &langs {
            if let Some(tr) = translated_in(catalogs, code, en) {
                out.push_str(&format!("        {code}:{key}) rx_t={} ;;\n", bash_quote(&tr)));
            }
        }
        out.push_str(&format!("        *:{key}) rx_t={} ;;\n", bash_quote(en)));
    }
    out.push_str(&format!(
        "        *) rx_t=\"$1\" ;;\n\
         \x20   esac\n\
         }}\n\
         rx_say() {{\n\
         \x20   local rx_t rx_out=\"\" rx_n\n\
         \x20   rx_text \"$1\"; shift\n\
         \x20   while [ -n \"$rx_t\" ]; do\n\
         \x20       case \"$rx_t\" in\n\
         \x20           %[1-9]*) rx_n=\"${{rx_t:1:1}}\"; rx_out+=\"${{!rx_n-}}\"; rx_t=\"${{rx_t:2}}\" ;;\n\
         \x20           *) rx_out+=\"${{rx_t:0:1}}\"; rx_t=\"${{rx_t:1}}\" ;;\n\
         \x20       esac\n\
         \x20   done\n\
         \x20   printf '%s: %s\\n' {name} \"$rx_out\"\n\
         }}\n\
         die() {{ rx_say \"$@\" >&2; exit 2; }}"
    ));
    out
}

/// Подставить блок языка в шаблон помощника (метка `__L10N__`).
pub fn localize_script(name: &str, script: &str) -> String {
    assert_eq!(script.matches("__L10N__").count(), 1, "в шаблоне {name} нет места для блока языка");
    script.replace("__L10N__", &bash_block(name, script))
}

// ============================================================================
// Вызов: аргумент языка
// ============================================================================

/// Аргумент языка для помощника с текстом `helper_text` при языке `lang`.
/// `None` — английский (он и так по умолчанию), язык вне белого списка или
/// помощник прежней версии, который `--lang` не понимает.
pub fn lang_arg_for(lang: &str, helper_text: Option<&str>) -> Option<String> {
    if !is_valid_code(lang) || lang == crate::i18n::SOURCE_LANGUAGE {
        return None;
    }
    if !languages().contains(&lang) {
        return None;
    }
    if !helper_text?.lines().any(|l| l == LANG_MARKER) {
        return None;
    }
    Some(format!("--lang={lang}"))
}

/// То же для живой машины: язык интерфейса Rexon и файл помощника по пути.
/// Возвращает 0 или 1 аргумент — вставлять сразу после пути помощника.
pub fn lang_args(helper_path: &str) -> Vec<String> {
    let lang = crate::i18n::current_language();
    if lang == crate::i18n::SOURCE_LANGUAGE {
        return Vec::new();
    }
    // Помощник — короткий скрипт; больше 1 МиБ — не наш, не читаем.
    let text = std::fs::metadata(helper_path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= 1 << 20)
        .and_then(|_| std::fs::read_to_string(helper_path).ok());
    lang_arg_for(&lang, text.as_deref()).into_iter().collect()
}

// ============================================================================
// Файлы в системе: ярлыки, автозапуск, polkit, метаданные тем
// ============================================================================
//
// Их читает не окно Rexon, а рабочий стол и polkit — и язык они выбирают
// САМИ по локали сеанса. Поэтому в файл ложатся сразу все языки каталога
// (`Name[ru]=`, `xml:lang="ru"`), а не язык интерфейса Rexon. Переводы берутся
// из тех же вшитых `.po`: новый язык в `po/LINGUAS` попадает в ярлыки и
// политики без правки кода.

/// Переводы `msgid` на все языки каталога: (код, текст). Перевод с
/// управляющими символами или с другим набором вставок пропускается.
pub fn translations(msgid: &str) -> Vec<(&'static str, String)> {
    languages()
        .into_iter()
        .filter_map(|code| translated(code, msgid).map(|t| (code, t)))
        .collect()
}

/// Значение ключа `.desktop`: `\` — начало экранирования по спецификации.
fn desktop_value(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// Строки `Key=<msgid>` и `Key[код]=<перевод>` для `.desktop`, каждая с `\n`.
pub fn desktop_key(key: &str, msgid: &str) -> String {
    let mut out = format!("{key}={}\n", desktop_value(msgid));
    for (code, text) in translations(msgid) {
        out.push_str(&format!("{key}[{code}]={}\n", desktop_value(&text)));
    }
    out
}

/// Текст для XML: пять служебных символов — сущностями.
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Элемент политики polkit на всех языках: `<tag>msgid</tag>` и
/// `<tag xml:lang="код">перевод</tag>`, каждый с отступом `indent` и `\n`.
pub fn polkit_elem(indent: &str, tag: &str, msgid: &str) -> String {
    let mut out = format!("{indent}<{tag}>{}</{tag}>\n", xml_escape(msgid));
    for (code, text) in translations(msgid) {
        out.push_str(&format!("{indent}<{tag} xml:lang=\"{code}\">{}</{tag}>\n", xml_escape(&text)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_texts_are_unique() {
        let mut keys: Vec<&str> = MESSAGES.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(n, keys.len(), "ключ повторяется");
        let mut texts: Vec<&str> = MESSAGES.iter().map(|(_, m)| *m).collect();
        texts.sort_unstable();
        texts.dedup();
        assert_eq!(n, texts.len(), "msgid повторяется — у двух ключей был бы один перевод");
        for (k, m) in MESSAGES {
            assert!(k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "{k}");
            assert!(translation_ok(m, m), "{k}: {m}");
        }
    }

    #[test]
    fn language_codes() {
        for ok in ["ru", "pt_BR", "sr@latin", "de"] {
            assert!(is_valid_code(ok), "{ok}");
        }
        for bad in ["", "RU", "ru;id", "ru ru", "$(id)", "`id`", "ru'", "../x", "-x", "ru\n"] {
            assert!(!is_valid_code(bad), "{bad:?}");
        }
    }

    #[test]
    fn po_parsing() {
        let po = "msgid \"\"\nmsgstr \"Language: ru\\n\"\n\n\
                  #. c\nmsgid \"a %1\"\nmsgstr \"\"\n\"б \"\n\"%1\"\n\n\
                  #, fuzzy\nmsgid \"b\"\nmsgstr \"нечёткое\"\n\n\
                  msgctxt \"x\"\nmsgid \"c\"\nmsgstr \"с контекстом\"\n\n\
                  msgid \"c\"\nmsgstr \"без контекста\"\n\n\
                  msgid \"q\"\nmsgstr \"кавычка \\\" и \\\\\"\n\n\
                  msgid \"%n file\"\nmsgid_plural \"%n files\"\nmsgstr[0] \"%n файл\"\n\n\
                  msgid \"last\"\nmsgstr \"последнее\"\n";
        assert_eq!(po_lookup(po, "a %1").as_deref(), Some("б %1"));
        assert_eq!(po_lookup(po, "b"), None);
        assert_eq!(po_lookup(po, "c").as_deref(), Some("без контекста"));
        assert_eq!(po_lookup(po, "q").as_deref(), Some("кавычка \" и \\"));
        assert_eq!(po_lookup(po, "%n file"), None);
        assert_eq!(po_lookup(po, "last").as_deref(), Some("последнее"));
        assert_eq!(po_lookup(po, "нет"), None);
    }

    #[test]
    fn translation_is_validated() {
        assert!(translation_ok("a %1 b %2", "%2 — %1"));
        assert!(!translation_ok("a %1", "без вставки"));
        assert!(!translation_ok("a %1", "%1 и %2"));
        assert!(!translation_ok("a", "две\nстроки"));
        assert!(!translation_ok("a", "звонок\x07"));
        assert!(!translation_ok("a", ""));
    }

    #[test]
    fn bash_quoting() {
        assert_eq!(bash_quote("a'b"), "'a'\\''b'");
        assert_eq!(bash_quote("$x `y` \\z"), "'$x `y` \\z'");
    }

    #[test]
    fn keys_from_script() {
        let s = "die bad_x \"$a\"\n  || die  \nrx_say ok_y\nmydie nope\n";
        assert_eq!(keys_used(s), vec!["bad_x", "ok_y"]);
    }

    /// Помощник из блока языка и строки вызова — прогон системным bash.
    fn run_block(catalogs: &[(&str, &str)], call: &str, args: &[&str]) -> (String, String) {
        let script = format!("#!/bin/bash\nset -euo pipefail\nexport LC_ALL=C LANG=C\n__L10N__\n{call}\n");
        let block = bash_block_in(catalogs, "rexon-test-helper", &script);
        let text = script.replace("__L10N__", &block);
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(&text)
            .arg("rexon-test-helper")
            .args(args)
            .output()
            .expect("bash");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// ⛔ Перевод — данные, а не код: кавычка, `$(…)`, обратная кавычка,
    /// `\`, `%s` в переводе и во вставке печатаются как есть и ничего не
    /// исполняют. Сломанный перевод (другие вставки, перевод строки) не
    /// попадает в скрипт вовсе — вместо него английский.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn translation_never_becomes_code() {
        let hostile = "zz'; touch /tmp/rexon-pwned; echo '$(id) `id` \\ %s %n %1";
        let po_hostile = format!(
            "msgid \"bad de_id: %1\"\nmsgstr \"{}\"\n\n\
             msgid \"bad family: %1\"\nmsgstr \"без вставки\"\n\n\
             msgid \"invalid name: %1\"\nmsgstr \"две\\nстроки %1\"\n",
            hostile.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let cats: &[(&str, &str)] = &[("ru", &po_hostile), ("de'x", "")];
        let _ = std::fs::remove_file("/tmp/rexon-pwned");
        let call = "rx_say bad_de_id \"$1\"; rx_say bad_family_arg z; rx_say bad_name n";
        let (out, err) = run_block(cats, call, &["--lang=ru", "$(id)%2"]);
        assert!(!std::path::Path::new("/tmp/rexon-pwned").exists(), "перевод исполнился");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            [
                "rexon-test-helper: zz'; touch /tmp/rexon-pwned; echo '$(id) `id` \\ %s %n $(id)%2",
                "rexon-test-helper: bad family: z",
                "rexon-test-helper: invalid name: n",
            ],
            "{err}"
        );
        // Код языка с кавычкой в белый список не попал.
        let block = bash_block_in(cats, "rexon-test-helper", "__L10N__ die bad_de_id");
        assert!(!block.contains("de'x"), "{block}");
    }

    /// Язык: из белого списка — перевод; всё прочее — английский, без ошибки.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn unknown_language_falls_back_to_english() {
        let cats: &[(&str, &str)] = &[("ru", "msgid \"bad de_id: %1\"\nmsgstr \"плохой de_id: %1\"\n")];
        let call = "rx_say bad_de_id \"$1\"";
        let (out, _) = run_block(cats, call, &["--lang=ru", "x"]);
        assert_eq!(out, "rexon-test-helper: плохой de_id: x\n");
        for lang in ["--lang=", "--lang=xx", "--lang=ru;id", "--lang=$(id)", "--lang=RU", "--lang=ru "] {
            let (out, err) = run_block(cats, call, &[lang, "x"]);
            assert_eq!(out, "rexon-test-helper: bad de_id: x\n", "{lang}: {err}");
        }
        // Без --lang первый аргумент — данные, как раньше.
        let (out, _) = run_block(cats, call, &["x"]);
        assert_eq!(out, "rexon-test-helper: bad de_id: x\n");
    }

    /// Вставка, похожая на вставку, второй раз не раскрывается.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn placeholders_single_pass() {
        let call = "rx_say family_mismatch \"$1\" \"$2\"";
        let (out, _) = run_block(&[], call, &["%2", "arch"]);
        assert_eq!(
            out,
            "rexon-test-helper: family from the argument (%2) does not match the one \
             detected on this machine (arch) — using the detected one\n"
        );
    }

    fn helpers() -> Vec<(&'static str, String)> {
        vec![
            ("rexon-de-helper", crate::system_plan::de_helper_script()),
            ("rexon-pkg-helper", crate::system_plan::pkg_helper_script()),
            ("rexon-stage-helper", crate::system_plan::stage_helper_script()),
            ("rexon-helper", crate::installer::system_helper_script()),
        ]
    }

    /// Все четыре помощника: разбираются bash, несут отметку, и язык
    /// читается раньше первой проверки аргументов: неверный вызов отвечает
    /// на выбранном языке (русский — дословно прежний текст).
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn all_helpers_speak_the_requested_language() {
        let ru = languages().contains(&"ru");
        for (name, text) in helpers() {
            assert_eq!(text.lines().filter(|l| *l == LANG_MARKER).count(), 1, "{name}: отметка");
            assert!(!text.contains("__L10N__"), "{name}");
            let syntax = std::process::Command::new("bash").arg("-n").arg("-c").arg(&text).output().unwrap();
            assert!(syntax.status.success(), "{name}: bash -n: {}", String::from_utf8_lossy(&syntax.stderr));
            // Ни одного голого echo с текстом для человека: всё через таблицу.
            for line in text.lines().map(str::trim_start).filter(|l| !l.starts_with('#')) {
                assert!(!line.contains(&format!("echo \"{name}:")), "{name}: {line}");
            }
            let run = |args: &[&str]| {
                let out = std::process::Command::new("bash")
                    .arg("-c")
                    .arg(&text)
                    .arg(name)
                    .args(args)
                    .env_remove("PKEXEC_UID")
                    .output()
                    .unwrap();
                assert_eq!(out.status.code(), Some(2), "{name} {args:?}");
                String::from_utf8_lossy(&out.stderr).into_owned()
            };
            let en = run(&["--lang=en", "x"]);
            assert!(en.starts_with(&format!("{name}: expected ")), "{name}: {en}");
            assert_eq!(run(&["--lang=zz", "x"]), en);
            assert_eq!(run(&["x"]), en);
            let got = run(&["--lang=ru", "x"]);
            if ru {
                assert!(got.starts_with(&format!("{name}: ожидается ")), "{name}: {got}");
            } else {
                assert_eq!(got, en);
            }
        }
    }

    #[test]
    fn desktop_and_polkit_escaping() {
        assert_eq!(desktop_value("a\\b"), "a\\\\b");
        assert_eq!(xml_escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&apos;");
        // Английский всегда первым и без ключа языка.
        let d = desktop_key("Comment", "Some text that has no translation 42");
        assert_eq!(d, "Comment=Some text that has no translation 42\n");
        let p = polkit_elem("    ", "message", "Some text that has no translation 42");
        assert_eq!(p, "    <message>Some text that has no translation 42</message>\n");
    }

    #[test]
    fn language_argument() {
        let new = format!("#!/bin/bash\n{LANG_MARKER}\n");
        let has_ru = languages().contains(&"ru");
        assert_eq!(lang_arg_for("ru", Some(&new)).is_some(), has_ru);
        if has_ru {
            assert_eq!(lang_arg_for("ru", Some(&new)).as_deref(), Some("--lang=ru"));
        }
        // Английский — по умолчанию, аргумент не нужен.
        assert_eq!(lang_arg_for("en", Some(&new)), None);
        // Помощник прежней версии отказал бы на лишнем аргументе.
        assert_eq!(lang_arg_for("ru", Some("#!/bin/bash\n")), None);
        assert_eq!(lang_arg_for("ru", None), None);
        // Не из белого списка и мусор — ничего.
        for bad in ["xx", "ru;id", "", "$(id)"] {
            assert_eq!(lang_arg_for(bad, Some(&new)), None, "{bad:?}");
        }
    }
}
