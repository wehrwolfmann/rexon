// SettingsActions — the side-actions object of the "Settings" tab in Rust.
//
// WHY. `SettingsPage.qml` expects, in its `actions` property, an object
// that can do two things: give a SNAPSHOT of the machine (`facts`, `state`,
// `ambientState`) and CHANGE it (switch the skin, enable autostart, apply a
// file manager, restore the environment's native applications, write a
// sound theme, protect the default applications). While there was no such
// object in Rust, the "Settings" section under the Rust window host opened
// EMPTY: the page has no sections until the first `facts()`, and
// `MainWindow.qml` puts a `SettingsPage {}` into it without an action
// object. Here the object appears, and the section starts working.
//
// The contract with the page is taken from the Python `SettingsActions`
// class (`rexon/qt_settings/window.py`, 1100 lines together with the window
// host) TO THE LETTER — the same forty names, the same answer types, the
// same two signals (`status`, `refreshed`). So the page markup did not
// change by a single line except one: the page creates the object FOR
// ITSELF (`SettingsActions { id: ownActions }`), as "Extensions" and
// Wallpaper Engine already do. The window host has nothing to do for this.
//
// WHERE THE LOGIC LIVES. There is none of our own here at all:
//   * WHAT to show and with what words — the core
//     (`rexon_core::qt_settings`), the same `TEXTS` catalogue the widget tab
//     is typed with;
//   * WHAT to count as the machine state — the core too: `ui_base` (the
//     skin), `tray` (autostart), `settings::SettingsManager` (settings),
//     `appdefaults` (the environment's native applications and the file
//     manager), `defaults` (default application protection), `backup`
//     (Plasma panel copies), `system`/`screenshots` (recognizing the
//     environment), `onboarding` (tutorial keys), `aeroshell` (the look
//     check);
//   * WHAT to do it with — the core's shared command launch (`utils::run`)
//     and ordinary file reads and writes. No second set of rules is created
//     here.
//
// Warning: WHAT IS NOT PORTED AND WHY — FOUR BOUNDARIES, NAMED OUT LOUD.
// There must be no silent "nothing happened" here (rule #13): the customer
// would take such a button for a breakage. So every method not ported MUST
// answer with a line saying "what was pressed and why it did not work".
// There is no longer anything to refer this line to the classic window
// with: the shortcut item is gone since 16.09, the bundle does not ship the
// window itself, and the shared wording was removed together with it (see
// "Boundaries named out loud" below). The technical reasons below are for
// the developer, they are not shown to the customer:
//
//   1. THE WALLPAPER ENGINE — PORTED. The plan executor is in the core
//      (`wallpaper_exec::WallpaperExec`), and through it these work here:
//      "Stop live wallpaper", "Picture…", "Video…", YouTube, web
//      wallpapers, the library, blur, ambient sounds, the MATE buttons and
//      installing yt-dlp. The executor is one per process and shared with
//      the "Steam" page (`wallpaper_exec::live_slot`); applying goes through
//      the core's shared door (`wallpaper_exec::run_in_slot`), and live
//      videos and YouTube are under the load supervision it brings up.
//      Which formats the executor carries is decided by
//      `wallpaper_exec::carries_format`: pictures, video, GIF, web and
//      YouTube are applied, while scenes and application wallpapers from
//      the library get the same final refusal as in the classic window.
//      Nothing is referred to the classic window: Flash (SWF) is the
//      `--mode swf` mode of the wallpaper host.
//   2. THE COMPLETENESS CHECK — PORTED. "Check and install what is missing"
//      is executed by the core with the same executor that installs desktops
//      (`system_de_exec::completeness_survey_live` / `complete_de_live`): a
//      check without a password → a question with the list of what is
//      missing in the shared operation window (`DeOperationDialog.qml`) →
//      after "Yes" the installation with progress, cancellation while the
//      system is untouched and one explanation for a password refusal.
//   2a. THE AEROSHELL LOOK — PORTED. The check and the rebuild — with the
//      same core executor (`system_de_exec::aeroshell_survey_live` /
//      `rebuild_aeroshell_live`) and the same operation window. The check at
//      Rexon start-up is called by the main window (`aeroshellStartupCheck`,
//      its own action object and its own window), the "Check now" button by
//      this page.
//   3. (removed) PLASMA PANEL COPIES — see "what is open" below.
//   4. (removed) THE TUTORIAL. The Rust main window has a tutorial overlay
//      (`onboarding_overlay`), and "Take the tutorial again" passes the
//      request to it. Without the main window the button says where to go
//      (`ob.no_main_window`).
//
// ⭐ WHAT OPENED UP AFTER THE PORT.
//   * THE FONT CHOICE WINDOW. The boundary here was "`QFontDialog` is a
//     widget that does not exist in `QGuiApplication` at all". Now its role
//     is played by `FontDialog` from `QtQuick.Dialogs`, raised by the same
//     shim with a nested event loop with which "Extensions" chooses an .rxn
//     file (`cpp/rexon_ext_dialogs.h`) — no second shim was created for
//     this.
//   * PLASMA PANEL COPIES. The `PlasmaProtector` executor was written — but
//     not here, rather next to the decisions, in the core's `backup.rs`
//     (`plasma_backup_run`, `plasma_restore_run`, `plasma_restart_shell`):
//     the same buttons exist in the "Desktops" tab too, and there must not
//     be a second copy of the executor in the product (rule #12). What is
//     left to the tab is what only it can see: where the copies directory
//     lies, what the current environment is and what to run the commands
//     with.
//
// ⭐ WHAT WAS FIXED DURING THE PORT (rule #16 — we do not preserve defects):
//   1. The skin in the list no longer differs from what is drawn. Python
//      gave the page the RAW `ui_skin` value from the settings, while the
//      page takes the palette from `ui_base::active_skin_colors`, which
//      falls back to the standard one for an unknown name. Write someone
//      else's skin into the settings — and the tab was painted in one while
//      the list showed another. Now the name goes through the same
//      `active_skin_id_from` as the palette.
//   2. Importing Windows themes is no longer "unavailable on this build".
//      Python checked `hasattr(_rexon_py, "importers_import_to_rxn")` — that
//      is, the presence of the bridge in Python; in the Rust process the
//      importer is linked into the binary, and there is nothing to check.
//   3. An autostart refusal names the reason. Python returned a generic
//      "failed — details in the log", and the customer does not read the
//      log; now the reason goes into the same core text `status.error`.
//   4. Writing the wallpaper sound settings no longer reports success
//      blindly: Python did not look at the saving outcome at all.
//   5. The "Ambient sounds" section answers honestly about `mpv`. Without
//      the engine Python gave `mpv: false` without asking the system — a
//      person with mpv installed saw "the mpv package is not installed".

/// cxx-qt bridge of the action object.
///
/// The type is declared `#[qml_element]`, so the page creates it for itself
/// (`SettingsActions { id: ownActions }`). No registration in
/// `main_window.rs` is required for this: there is the section map, not a
/// list of side objects — and the window host stays untouched.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        /// The window for installing what is missing: a question, the progress or
        /// the outcome. The same contract as `DesktopsActions.operation` — the
        /// window is one for all (`DeOperationDialog.qml`).
        #[qproperty(QMap_QString_QVariant, operation, READ = operation, NOTIFY = operation_changed)]
        /// Whether to show the "Install Rexon into the system…" button next to
        /// "Take the tutorial again". The core's decision WITHOUT taking the "do not
        /// ask again" answer into account: the button itself removes the refusal,
        /// and an installed Rexon (a package, Steam, installation for everyone or
        /// for oneself) has no button at all. Recomputed together with the page
        /// snapshot.
        #[qproperty(bool, install_offered, cxx_name = "installOffered",
                    READ = install_offered, NOTIFY = refreshed)]
        /// The button label and the explanation under it: `{label, hint}`. The
        /// letters come from the core (`installer::settings_install_button` /
        /// `_hint`).
        #[qproperty(QMap_QString_QVariant, install_texts, cxx_name = "installTexts",
                    READ = install_texts, CONSTANT)]
        type SettingsActions = super::SettingsActionsRust;
    }

    unsafe extern "RustQt" {
        /// An answer of LONG work: show it as a pop-up message.
        #[qsignal]
        fn status(self: Pin<&mut SettingsActions>, message: QString);

        /// Long work changed the machine state — re-read the snapshot.
        #[qsignal]
        fn refreshed(self: Pin<&mut SettingsActions>);

        #[qsignal]
        #[cxx_name = "operationChanged"]
        fn operation_changed(self: Pin<&mut SettingsActions>);

        /// A line of the installation progress log — appended by the window.
        #[qsignal]
        #[cxx_name = "operationLog"]
        fn operation_log(self: Pin<&mut SettingsActions>, line: QString);
    }

    extern "RustQt" {
        fn operation(self: &SettingsActions) -> QMap_QString_QVariant;
        #[cxx_name = "installOffered"]
        fn install_offered(self: &SettingsActions) -> bool;
        #[cxx_name = "installTexts"]
        fn install_texts(self: &SettingsActions) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// "Yes" in the question window — start the installation.
        #[qinvokable]
        #[cxx_name = "confirmOperation"]
        fn confirm_operation(self: Pin<&mut SettingsActions>);

        /// "No", "Close" — remove the window (does not work while the installation
        /// is running).
        #[qinvokable]
        #[cxx_name = "dismissOperation"]
        fn dismiss_operation(self: Pin<&mut SettingsActions>);

        /// "Cancel" — only while the system is untouched.
        #[qinvokable]
        #[cxx_name = "cancelOperation"]
        fn cancel_operation(self: Pin<&mut SettingsActions>);
    }

    extern "RustQt" {
        // ── Machine snapshots ────────────────────────────────────────────

        /// Facts on which THE VERY COMPOSITION of the tab's sections depends. The
        /// decision "whether to show a section" is made by the core
        /// (`qt_settings::section_visible`), here there are only observations.
        #[qinvokable]
        fn facts(self: &SettingsActions) -> QString;

        /// A full snapshot of what the tab shows (JSON).
        #[qinvokable]
        fn state(self: &SettingsActions) -> QString;

        /// Only the playing ambient sounds — a snapshot by timer. Separate from
        /// `state`, because the full snapshot goes to the system for fonts, sound
        /// themes and default applications.
        #[qinvokable]
        #[cxx_name = "ambientState"]
        fn ambient_state(self: &SettingsActions) -> QString;

        // ── Interface language ───────────────────────────────────────────

        /// Remember the interface language (`ui_language`): `system`, `en`, `ru`…
        /// Applied after a restart — the snapshot (`lang.restart`) will say whether
        /// one is needed.
        #[qinvokable]
        #[cxx_name = "setLanguage"]
        fn set_language(self: &SettingsActions, code: &QString) -> QString;

        /// "Restart Rexon": start a new process that will wait for this one to exit,
        /// and exit. An empty answer means the restart is under way.
        #[qinvokable]
        #[cxx_name = "restartRexon"]
        fn restart_rexon(self: &SettingsActions) -> QString;

        // ── The right to change the system (B13) ─────────────────────────

        /// The password has already been entered, and Rexon may change the system
        /// until the window closes (`privilege::run::has_session`). Polled by the
        /// page's timer.
        #[qinvokable]
        #[cxx_name = "adminSession"]
        fn admin_session(self: &SettingsActions) -> bool;

        /// "Revoke now": close the privilege session. The answer is a line for a
        /// pop-up message.
        #[qinvokable]
        #[cxx_name = "revokeAdmin"]
        fn revoke_admin(self: &SettingsActions) -> QString;

        // ── Skin, tutorial, autostart ────────────────────────────────────

        /// Switch the interface skin and repaint the open tabs.
        #[qinvokable]
        #[cxx_name = "setSkin"]
        fn set_skin(self: &SettingsActions, skin_id: &QString) -> QString;

        /// Whether to offer the introductory tour at launch.
        #[qinvokable]
        #[cxx_name = "setOnboarding"]
        fn set_onboarding(self: &SettingsActions, enabled: bool) -> QString;

        /// Take the introductory tour again. Mutable access — for the sake of the
        /// "re-read the snapshot" push after the overlay enables the tutorial.
        #[qinvokable]
        #[cxx_name = "replayOnboarding"]
        fn replay_onboarding(self: Pin<&mut SettingsActions>) -> QString;

        /// "Install Rexon into the system…": remove the previous "do not ask again"
        /// answer and open the first-run wizard. Answers with a line for the footer
        /// — the button must not have a silent "nothing happened" (rule #13).
        #[qinvokable]
        #[cxx_name = "installRexon"]
        fn install_rexon(self: &SettingsActions) -> QString;

        /// Rexon autostart at login (into the tray, without a window).
        #[qinvokable]
        #[cxx_name = "setAutostart"]
        fn set_autostart(self: &SettingsActions, enabled: bool) -> QString;

        /// Whether to download icons and cursors for a theme without asking
        /// (`rexon_core::theme_watch::AUTO_HOME_KEY`).
        #[qinvokable]
        #[cxx_name = "setThemeDepsAuto"]
        fn set_theme_deps_auto(self: &SettingsActions, enabled: bool) -> QString;

        // ── File manager ─────────────────────────────────────────────────

        /// Remember the file manager choice (without applying it yet).
        #[qinvokable]
        #[cxx_name = "setFileManager"]
        fn set_file_manager(self: &SettingsActions, fm_id: &QString) -> QString;

        /// Make the chosen file manager the default file manager.
        #[qinvokable]
        #[cxx_name = "applyFileManager"]
        fn apply_file_manager(self: &SettingsActions) -> QString;

        /// Restore the environment's native applications.
        #[qinvokable]
        #[cxx_name = "restoreNative"]
        fn restore_native(self: &SettingsActions) -> QString;

        /// Check the desktop's completeness (with installation of what is missing).
        /// The check runs in the background; if something is missing — a question
        /// window, after "Yes" — a progress window.
        #[qinvokable]
        #[cxx_name = "checkCompleteness"]
        fn check_completeness(self: Pin<&mut SettingsActions>) -> QString;

        // ── Wallpaper ────────────────────────────────────────────────────

        /// Stop the live wallpaper.
        #[qinvokable]
        #[cxx_name = "stopWallpaper"]
        fn stop_wallpaper(self: &SettingsActions) -> QString;

        /// Write the wallpaper sound settings (the wallpaper is NOT restarted: the
        /// host watches the settings file and applies the edit itself).
        #[qinvokable]
        #[cxx_name = "setWallpaperAudio"]
        fn set_wallpaper_audio(
            self: &SettingsActions,
            enabled: bool,
            volume: i32,
            mute_other: bool,
            hide_covered: bool,
            pause_on_battery: bool,
            pause_in_game: bool,
        ) -> QString;

        /// Choose a wallpaper file and apply it. `kind` is `static` or `video`.
        /// Mutable access — for the sake of the applying outcome, which arrives from
        /// a background thread with a signal.
        #[qinvokable]
        #[cxx_name = "pickWallpaper"]
        fn pick_wallpaper(self: Pin<&mut SettingsActions>, kind: &QString) -> QString;

        /// Apply a YouTube wallpaper.
        #[qinvokable]
        #[cxx_name = "applyYoutube"]
        fn apply_youtube(
            self: Pin<&mut SettingsActions>,
            url: &QString,
            quality: &QString,
            mode: &QString,
            audio: bool,
        ) -> QString;

        /// The sound of a YouTube wallpaper is a launch property, it lives until the
        /// window closes.
        #[qinvokable]
        #[cxx_name = "setYoutubeAudio"]
        fn set_youtube_audio(self: &SettingsActions, on: bool) -> QString;

        /// Install or update yt-dlp. Mutable access — for the sake of the outcome,
        /// which arrives from a background thread with a signal.
        #[qinvokable]
        fn ytdlp(self: Pin<&mut SettingsActions>, action: &QString) -> QString;

        /// Apply a web wallpaper.
        #[qinvokable]
        #[cxx_name = "applyWeb"]
        fn apply_web(self: Pin<&mut SettingsActions>, url: &QString) -> QString;

        // ── Wallpaper library ────────────────────────────────────────────

        /// Open a wallpaper file with what the system opens it with.
        #[qinvokable]
        #[cxx_name = "previewLibrary"]
        fn preview_library(self: &SettingsActions, path: &QString) -> QString;

        /// Apply a wallpaper from the library.
        #[qinvokable]
        #[cxx_name = "applyLibrary"]
        fn apply_library(
            self: Pin<&mut SettingsActions>,
            path: &QString,
            fmt: &QString,
        ) -> QString;

        // ── Importing Windows themes ─────────────────────────────────────

        /// Choose a Windows theme file and import it in the background.
        #[qinvokable]
        #[cxx_name = "pickImport"]
        fn pick_import(self: Pin<&mut SettingsActions>) -> QString;

        // ── Wallpaper blur ───────────────────────────────────────────────

        /// Remember the blur strength (it will apply on the next "Blur").
        #[qinvokable]
        #[cxx_name = "setBlurRadius"]
        fn set_blur_radius(self: &SettingsActions, value: i32) -> QString;

        /// Blur the wallpaper or remove the blur. Mutable access — for the sake of
        /// the outcome, which arrives from a background thread with a signal.
        #[qinvokable]
        fn blur(self: Pin<&mut SettingsActions>, action: &QString) -> QString;

        // ── Ambient sounds ───────────────────────────────────────────────

        /// Choose an audio file and play it in a loop.
        #[qinvokable]
        #[cxx_name = "addAmbient"]
        fn add_ambient(self: &SettingsActions, volume: i32) -> QString;

        /// Set the volume of a playing sound.
        #[qinvokable]
        #[cxx_name = "setAmbientVolume"]
        fn set_ambient_volume(self: &SettingsActions, sound_id: &QString, volume: i32)
            -> QString;

        /// Stop one ambient sound.
        #[qinvokable]
        #[cxx_name = "stopAmbient"]
        fn stop_ambient(self: &SettingsActions, sound_id: &QString) -> QString;

        /// Stop all ambient sounds.
        #[qinvokable]
        #[cxx_name = "stopAllAmbient"]
        fn stop_all_ambient(self: &SettingsActions) -> QString;

        // ── Event sounds ─────────────────────────────────────────────────

        /// Write the environment's sound theme.
        #[qinvokable]
        #[cxx_name = "setSoundTheme"]
        fn set_sound_theme(self: &SettingsActions, theme: &QString) -> QString;

        /// Enable or disable event sounds.
        #[qinvokable]
        #[cxx_name = "setEventSounds"]
        fn set_event_sounds(self: &SettingsActions, enabled: bool) -> QString;

        /// Listen to an event sound of the theme.
        #[qinvokable]
        #[cxx_name = "playSound"]
        fn play_sound(self: &SettingsActions, theme: &QString, event_id: &QString) -> QString;

        // ── System fonts ─────────────────────────────────────────────────

        /// Choose a system font: `system` / `mono` / `title`.
        #[qinvokable]
        #[cxx_name = "pickFont"]
        fn pick_font(self: &SettingsActions, target: &QString) -> QString;

        /// Write font rendering: antialiasing (and the size range without it),
        /// subpixels, hinting.
        #[qinvokable]
        #[cxx_name = "setFontRendering"]
        fn set_font_rendering(
            self: &SettingsActions,
            antialias: bool,
            exclude: bool,
            exclude_from: i32,
            exclude_to: i32,
            subpixel: &QString,
            hinting: &QString,
        ) -> QString;

        /// "Change all fonts": a choice window, then one family and/or size for all
        /// fonts of the environment. `mode` is `both|family|size`.
        #[qinvokable]
        #[cxx_name = "changeAllFonts"]
        fn change_all_fonts(self: &SettingsActions, mode: &QString) -> QString;

        /// Restore the LXQt desktop icon label font as it was before Rexon
        /// (`qt_settings::pcmanfm_desktop_font_restore`) → the outcome.
        #[qinvokable]
        #[cxx_name = "restoreDesktopFont"]
        fn restore_desktop_font(self: &SettingsActions) -> QString;

        /// Forced font DPI: `enabled = false` — return to "by the screen".
        #[qinvokable]
        #[cxx_name = "setFontDpi"]
        fn set_font_dpi(self: &SettingsActions, enabled: bool, dpi: i32) -> QString;

        // ── Font management ──────────────────────────────────────────────

        /// Installed fonts by family (JSON): labels, a summary, filter options, the
        /// sample text, families with styles and files — under the filter `filter`
        /// (JSON `qt_settings::FontListFilter`: location, search, vendor, file type,
        /// script). `full = false` — only the labels (the section is collapsed).
        #[qinvokable]
        #[cxx_name = "fontLibrary"]
        fn font_library(self: &SettingsActions, full: bool, filter: &QString) -> QString;

        /// Character map of a style (JSON: Unicode blocks with the codes of the
        /// characters that are in the font file).
        #[qinvokable]
        #[cxx_name = "fontChars"]
        fn font_chars(self: &SettingsActions, file: &QString, style: &QString) -> QString;

        /// Remember our own sample text (empty — the default sample) → the text the
        /// samples are now drawn with.
        #[qinvokable]
        #[cxx_name = "setFontSampleText"]
        fn set_font_sample_text(self: &SettingsActions, text: &QString) -> QString;

        /// Enable or disable a font without removing it (`<rejectfont>` in our
        /// `fonts.conf` block): the whole family (`file` empty) or one style → the
        /// outcome for the user.
        #[qinvokable]
        #[cxx_name = "setFontEnabled"]
        fn set_font_enabled(self: &SettingsActions, family: &QString, file: &QString, enabled: bool) -> QString;

        /// Our own font groups (`fontgroups.xml`, shared with KDE): `op` is
        /// `create` (`arg` — the name), `rename` (`arg` — the new name), `remove`
        /// (with a question), `add` / `take` (`arg` — the family) → JSON
        /// `{"text": outcome, "origin": what to select in the filter after the
        /// edit}`.
        #[qinvokable]
        #[cxx_name = "fontGroupEdit"]
        fn font_group_edit(self: &SettingsActions, op: &QString, group: &QString, arg: &QString) -> QString;

        /// Enable or disable all fonts of our own group → the outcome.
        #[qinvokable]
        #[cxx_name = "setFontGroupEnabled"]
        fn set_font_group_enabled(self: &SettingsActions, group: &QString, enabled: bool) -> QString;

        /// Exporting a group: a save window, then a ZIP with the files of its fonts
        /// → the outcome (empty — the user changed their mind).
        #[qinvokable]
        #[cxx_name = "exportFontGroup"]
        fn export_font_group(self: &SettingsActions, group: &QString) -> QString;

        /// Duplicates: one style in several files (JSON: a summary and groups with
        /// copies). Copies are removed with the same `removeFont`.
        #[qinvokable]
        #[cxx_name = "fontDuplicates"]
        fn font_duplicates(self: &SettingsActions) -> QString;

        /// Install a font from a file (a choice window) → the outcome for the user.
        /// `scope` is for whom (`qt_settings::FONT_INSTALL_SCOPES`): "for everyone"
        /// goes through the privilege session in the background, the outcome comes
        /// with the `status` signal.
        #[qinvokable]
        #[cxx_name = "installFontFile"]
        fn install_font_file(self: Pin<&mut SettingsActions>, scope: &QString) -> QString;

        /// Remove a font: the whole family (`file` empty) or one style (`file`).
        /// Our own ones — at once, those installed for everyone — through the
        /// privilege session in the background (the outcome comes with the `status`
        /// signal), package fonts — not at all. With a question.
        #[qinvokable]
        #[cxx_name = "removeFont"]
        fn remove_font(self: Pin<&mut SettingsActions>, family: &QString, file: &QString) -> QString;

        // ── Default application protection ───────────────────────────────

        /// Toggle a protection rule (`enabled` is the main switch).
        #[qinvokable]
        #[cxx_name = "setProtection"]
        fn set_protection(self: &SettingsActions, key: &QString, enabled: bool) -> QString;

        /// Remember the current default applications.
        #[qinvokable]
        fn snapshot(self: &SettingsActions) -> QString;

        /// Restore the remembered default applications.
        #[qinvokable]
        #[cxx_name = "restoreDefaults"]
        fn restore_defaults(self: &SettingsActions) -> QString;

        /// Enable or disable protecting the terminal colours from themes
        /// (`terminal_guard`, a `settings.json` key).
        #[qinvokable]
        #[cxx_name = "setTerminalColorsGuard"]
        fn set_terminal_colors_guard(self: &SettingsActions, enabled: bool) -> QString;

        // ── Plasma panel copies ──────────────────────────────────────────

        /// Take a copy of the Plasma panel.
        #[qinvokable]
        #[cxx_name = "plasmaBackup"]
        fn plasma_backup(self: &SettingsActions) -> QString;

        /// Texts of the question before restoring a copy: `{title, question}`
        /// (JSON).
        #[qinvokable]
        #[cxx_name = "plasmaConfirm"]
        fn plasma_confirm(self: &SettingsActions, timestamp: &QString) -> QString;

        /// Restore a copy of the Plasma panel.
        #[qinvokable]
        #[cxx_name = "plasmaRestore"]
        fn plasma_restore(self: &SettingsActions, timestamp: &QString) -> QString;

        // ── Desktop look ─────────────────────────────────────────────────

        /// Whether to check the look at Rexon start-up.
        #[qinvokable]
        #[cxx_name = "setAeroshell"]
        fn set_aeroshell(self: &SettingsActions, enabled: bool) -> QString;

        /// Check the look right now. The check runs in the background; if it is
        /// behind — a question window, after "Restore" — a rebuild progress window.
        #[qinvokable]
        #[cxx_name = "aeroshellCheckNow"]
        fn aeroshell_check_now(self: Pin<&mut SettingsActions>) -> QString;

        /// The look check at Rexon start-up (called by the main window). The same
        /// check, but silent if everything is fine or the check is turned off in
        /// the settings.
        #[qinvokable]
        #[cxx_name = "aeroshellStartupCheck"]
        fn aeroshell_startup_check(self: Pin<&mut SettingsActions>);
    }

    // The trait declaration must be IN THE BRIDGE: cxx-qt emits the thread type
    // alias from it, without which the background import would not reach the
    // signal.
    impl cxx_qt::Threading for SettingsActions {}
}

use core::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtThread, CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};
use serde_json::{json, Map, Value};

use crate::desktops_actions::OpView;
use rexon_core::de_progress as dp;
use rexon_core::qt_settings::{self as qs, SoundSetting};
use rexon_core::system_de_exec as dx;
use rexon_core::wallpaper_exec::{
    self, live_slot, try_with_slot, LiveSys, WallpaperExec, WallpaperResult, WpSys,
};
use rexon_core::{
    aeroshell, appdefaults, backup, defaults, engine_update, ext_exec, gtk_sanitizer, importers,
    onboarding, qt_app, screenshots, settings as core_settings, system, system_plan, ui_base,
    utils, wallpaper_audio, wallpaper_engine, youtube_wallpaper,
};

// ============================================================================
// Object state
// ============================================================================

/// What lives EXACTLY as long as the open tab and is not stored on disk:
/// launch properties and expensive snapshots.
///
/// Expensive is not a figure of speech: reading three system fonts is three
/// command launches, and the list of sound themes is a walk of the `sounds`
/// tree. The full snapshot is asked for after EVERY action (`root.did(...)`
/// in the markup), and without memory the tab would keep the machine
/// constantly busy for nothing. The reset points are the same as in the
/// Python original: fonts — after choosing a font, themes — never during the
/// tab's life (their set changes by installing packages).
#[derive(Default)]
struct Session {
    /// The YouTube wallpaper sound: a LAUNCH property, not a setting. `None`
    /// means not touched yet, on by default.
    youtube_audio: Option<bool>,
    /// The blur strength chosen by the slider in this window.
    blur_radius: Option<i32>,
    /// What the import section shows before the first import.
    import_note: String,
    /// What the completeness section shows before the first check.
    completeness_note: Option<String>,
    /// A snapshot of the system fonts: `target → ready label`.
    fonts: Option<Vec<(String, String)>>,
    /// A snapshot of font rendering (antialiasing, subpixels, hinting): reading
    /// takes up to four command launches, so it is kept until a write.
    rendering: Option<qs::FontRendering>,
    /// A snapshot of the forced font DPI (`Some(None)` — not set).
    font_dpi: Option<Option<i32>>,
    /// Installed fonts for "Font management" (`fc-list`): kept until a font is
    /// installed or removed.
    font_library: Option<Vec<qs::InstalledFont>>,
    /// The list of installed sound themes.
    sound_themes: Option<Vec<String>>,
    /// The yt-dlp version, if it is present. It is asked by launching the
    /// program, and this tab cannot install or update it yet — so the answer
    /// does not change during the tab's life.
    ytdlp_version: Option<String>,
    /// Installed wallpapers for the library. Walking the extensions directory
    /// reads every manifest, so the list is kept until an event that changes it
    /// — a theme import (the same order as Python's `_lib_cache`).
    library: Option<Vec<wallpaper_engine::LibraryItem>>,
}

/// Rust state of the action object.
#[derive(Default)]
pub struct SettingsActionsRust {
    session: Mutex<Session>,
    /// Whether a Windows theme import is in progress. As a separate flag rather
    /// than a `Session` field: it is read and cleared by the background thread,
    /// which has no access to the object.
    import_busy: Arc<AtomicBool>,
    // There is NO wallpaper executor of our own here: it is one per process
    // (`wallpaper_exec::live_slot`), shared with the "Steam" page. The tab's
    // former slot did not let "Stop" remove a video set from there.
    /// Whether applying a wallpaper is in progress (in the same way as the
    /// import).
    wallpaper_busy: Arc<AtomicBool>,
    /// Whether installing or updating yt-dlp is in progress.
    ytdlp_busy: Arc<AtomicBool>,
    /// Whether the "Check and install what is missing" button is busy: from the
    /// press to the outcome, including an open question window.
    completeness_busy: Arc<AtomicBool>,
    /// The operation window — of installing what is missing or of rebuilding
    /// the look (a common view with "Desktops").
    op: OpView,
    /// What waits for "Yes" in the question window.
    pending: Option<PendingOp>,
    /// The cancellation flag of the running operation.
    cancel: Option<dx::CancelToken>,
    /// What the update watcher found besides after removing an orphan: it will
    /// be shown when the user closes the window with the removal outcome.
    update_guard_rest: Option<rexon_core::update_guard::Advice>,
}

/// An operation waiting for "Yes" in the question window.
enum PendingOp {
    /// The login screen itself opened the session on which the orphan depends:
    /// make the session's Wayland pair the default session, then offer to remove
    /// the orphans again.
    WaylandDefault(&'static rexon_core::system_plan::SessionSuccessor, Vec<String>),
    /// Installing what is missing: the check key and the label for the user.
    Completeness(Vec<(String, String)>),
    /// Rebuilding the AeroShell look: the packages.
    Aeroshell(Vec<String>),
    /// Restoring the standard Plasma instead of a fork left without a shell.
    RestoreStock(Vec<String>),
    /// Remove the orphan packages found by the update watcher.
    RemoveOrphans(Vec<String>),
    /// The watcher's window: "Restore the previous versions" — then a
    /// confirmation window with the "was → will be" list.
    RollbackAsk(rexon_core::update_guard::RollbackOffer),
    /// Confirmed: restore the previous versions of the set.
    Rollback(rexon_core::update_guard::RollbackOffer),
}

/// Whether checking or rebuilding the look is in progress — for the WHOLE
/// process. There are two action objects (the main window's for the
/// start-up check and the settings page's), while only one build may run on
/// the machine.
static AEROSHELL_BUSY: AtomicBool = AtomicBool::new(false);

/// What is going on with the wallpaper at the slot's executor: whether it is
/// running, the format, the path. We do NOT wait for the lock (the snapshot
/// is asked for after every press): if it is busy applying from any page,
/// the answer is "not playing", and the next snapshot will tell the truth.
/// We do not create an executor here: there is nothing to look at.
fn wallpaper_now<S: WpSys>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
) -> (bool, Option<String>, Option<String>) {
    match slot.try_lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(exec) => {
                let st = exec.status();
                (st.running, st.format, st.path)
            }
            None => (false, None, None),
        },
        Err(_) => (false, None, None),
    }
}

/// "Stop live wallpaper" by the slot's executor — whichever page set it.
/// `None` means the executor is busy, and waiting for it in the window's
/// thread is not allowed.
fn stop_wallpaper_in<S: WpSys + Default>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
) -> Option<WallpaperResult> {
    try_with_slot(slot, |exec| exec.stop(true))
}

/// Clears the busy flag when it goes out of scope — including on a panic of
/// the background work.
struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// What is going on with the wallpaper — the observations from which the
/// "Wallpaper" section is assembled.
struct WallpaperView {
    running: bool,
    format: Option<String>,
    path: Option<String>,
    audio: wallpaper_audio::AudioSettings,
    youtube_audio: bool,
    /// The yt-dlp version; `None` means it is not in the system.
    ytdlp_version: Option<String>,
    /// Whether installing or updating yt-dlp is in progress.
    ytdlp_busy: bool,
    busy: bool,
}

/// The "Wallpaper" section of the snapshot — the same keys as in Python
/// (`_wallpaper_state`). The words and decisions are the core's; here there
/// is only laying them out by the page's keys.
fn wallpaper_section(view: &WallpaperView) -> Value {
    let (ytdlp_label, ytdlp_button, ytdlp_action) = qs::ytdlp_row(
        view.ytdlp_version.is_some(),
        view.ytdlp_version.as_deref().unwrap_or(""),
    );
    json!({
        // The core's executor is linked into the binary — the section is always
        // there.
        "engine": true,
        "running": view.running,
        "status": qs::wallpaper_status_text(
            view.running,
            view.format.as_deref(),
            view.path.as_deref(),
        ),
        "audio": {
            "enabled": view.audio.enabled,
            "volume": view.audio.volume_pct,
            "muteOther": view.audio.mute_when_other_audio,
            "hideCovered": wallpaper_audio::hide_when_covered(&view.audio.actions),
            "pauseBattery": wallpaper_audio::choices_of(&view.audio.actions).pause_on_battery,
            "pauseGame": wallpaper_audio::choices_of(&view.audio.actions).pause_in_game,
        },
        "ytAudio": view.youtube_audio,
        "ytdlpLabel": ytdlp_label,
        "ytdlpButton": ytdlp_button,
        "ytdlpAction": ytdlp_action,
        "ytdlpBusy": view.ytdlp_busy,
        "busy": view.busy,
    })
}

impl SettingsActionsRust {
    /// The session under a lock. A poisoned lock does not bring the tab down:
    /// it means the background thread crashed with a panic, but the machine
    /// snapshot still has to be shown to the user.
    fn session(&self) -> std::sync::MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// ============================================================================
// Boundaries named out loud
// ============================================================================

// What exactly is not ported — for the developer, it is NOT shown to the
// customer (rule #13: no module names, no polkit, no paths to .py in the
// interface).
//
//   * Installing, switching and removing a desktop — see the header of
//     `desktops_actions.rs`.

// ⭐⭐ Fix of 16.09.2026. The shared boundary wording `classic_window_only`
// used to live here — "this feature is so far only in the classic Rexon
// window, right-click the shortcut and choose "Rexon (classic window)"". It
// was REMOVED, and here is why:
//
//   * the advice became untrue the same day: the `[Desktop Action Classic]`
//     item was removed from the shortcut (40620d2) — the user would
//     right-click and not find what they were told. A hint leading into the
//     void is worse than no hint;
//   * there was nowhere to lead in substance either: neither the
//     distribution package nor the manual installation ships the classic
//     window, and the Python part itself is going away (the owner's
//     decision of 16.09);
//   * there was nobody left to call it: no boundaries to the classic window
//     remained on the pages (Flash moved last), the function stood under
//     `#[allow(dead_code)]`, and only the reconciliation tests touched it.
//
// The "no button stays silent" contract (rule #13) is not cancelled by
// this — it is held by the `nothing_goes_to_the_classic_window` guard
// below: a NEW boundary has no right to appear silently, and it has nothing
// to refer to the removed window with any more.
use rexon_core::installer as inst;

/// Why the font choice window did not open. This is NOT a porting boundary:
/// the dialog shim exists, but `FontDialog` from `QtQuick.Dialogs` might not
/// come up — for example, the `QtQuick.Dialogs` module is not installed in
/// the system at all. Silence about this is not allowed: a button after
/// which nothing happens reads as a broken program (rule #13). The package
/// name is kept — by it the user can install what is missing.
fn font_dialog_failed() -> String {
    // TRANSLATORS: shown after the font kind, e.g. “System Font — …”.
    rexon_core::i18n::tr("the font chooser didn't open: the part of Qt that draws such windows is \
                          missing on the system (package qt6-declarative)")
}

// ============================================================================
// Installing what is missing: delivering the progress from the worker thread
// ============================================================================

/// The reason in the "Check error: …" line when the worker thread itself
/// crashed.
fn completeness_internal_error() -> String {
    // TRANSLATORS: the reason shown after “Check failed:” when the check
    // itself crashed.
    rexon_core::i18n::tr("internal Rexon error, please try again")
}

/// What to write in the section after installing what was missing. Success
/// — the core's check outcome in words; a password refusal, cancellation
/// and a partial outcome — the first line of the executor's outcome: a
/// "failed: N" summary by refused commands would read as broken packages,
/// while they were not touched at all.
fn completeness_note_after(outcome: &dx::DeOpOutcome, results: &[(String, String)]) -> String {
    if outcome.ok && !results.is_empty() {
        return qs::compliance_summary(results);
    }
    let first = outcome.message.lines().next().unwrap_or("").trim();
    if outcome.ok {
        format!("✅ {first}")
    } else {
        format!("⚠️ {first}")
    }
}

/// The question window "the look stopped working": the core's texts and
/// buttons, the same as in the Python window (`offer_aeroshell_rebuild`).
/// Package names and versions do not go into the question — they are in the
/// program log.
///
/// The session journal named what exactly is disabled (`report.journal`) —
/// these lines go into the text: "effect such-and-such is disabled: built
/// for KWin X, installed Y — rebuild the package".
fn aeroshell_confirm_view(report: &aeroshell::Report) -> OpView {
    OpView {
        yes_text: aeroshell::confirm_button_for(report),
        no_text: dp::aeroshell_skip_button(),
        ..OpView::confirm(&aeroshell::confirm_title_for(report), &aeroshell::confirm_text_for(report))
    }
}

/// The window about third-party modules from the session journal (KWin
/// effects, widgets, window decorations not from the set): if there is
/// something to rebuild from AUR — a question with a rebuild button; if not
/// — only say what to do.
fn broken_plugins_view(report: &aeroshell::Report, rebuild: bool) -> OpView {
    let title = rexon_core::plugin_journal::plugins_title();
    let text = aeroshell::plugins_text(report, rebuild);
    if rebuild {
        OpView {
            yes_text: dp::aeroshell_fix_button(),
            no_text: dp::aeroshell_skip_button(),
            ..OpView::confirm(&title, &text)
        }
    } else {
        OpView {
            phase: "blocked".to_string(),
            title,
            text,
            ..OpView::default()
        }
    }
}

/// The question window "the fork was left without a shell — restore the
/// standard Plasma".
fn restore_stock_confirm_view() -> OpView {
    OpView {
        yes_text: aeroshell::restore_button(),
        no_text: dp::aeroshell_skip_button(),
        ..OpView::confirm(
            &aeroshell::restore_title(),
            &format!("{}<br><br>{}", aeroshell::restore_text(), aeroshell::restore_informative()),
        )
    }
}

/// A message window without an action (one "Got it" button): say why Rexon
/// offers nothing. The text is plain and is escaped here.
fn aeroshell_notice_view(title: &str, text: &str) -> OpView {
    OpView {
        phase: "blocked".to_string(),
        title: title.to_string(),
        text: rexon_core::ui_base::escape_dialog_text(text).replace('\n', "<br>"),
        ..OpView::default()
    }
}

/// Receiver of the core executor's progress: every event goes to the
/// window's thread.
struct SettingsSink {
    handle: CxxQtThread<qobject::SettingsActions>,
}

impl dx::DeOpSink for SettingsSink {
    fn progress(&self, stage: &str, message: &str, fraction: Option<f64>) {
        let (stage, message) = (stage.to_string(), message.to_string());
        deliver(&self.handle, move |object| {
            object.apply_op_progress(stage, message, fraction);
        });
    }

    fn cancellable(&self, allowed: bool) {
        deliver(&self.handle, move |object| object.apply_op_cancellable(allowed));
    }
}

/// Queue work into the window's thread; if the tab is already closed —
/// silently, otherwise into the log.
fn deliver(
    handle: &CxxQtThread<qobject::SettingsActions>,
    work: impl FnOnce(Pin<&mut qobject::SettingsActions>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("settings", "ход доустановки не доставлен вкладке: {e:?}");
        }
    }
}

// ============================================================================
// Small translation helpers
// ============================================================================

/// A label from the core's catalogue.
fn t(key: &str) -> String {
    qs::text(key, &[]).unwrap_or_default()
}

/// A label from the core's catalogue with one substitution.
fn t1(key: &str, a: &str) -> String {
    qs::text(key, &[a.to_string()]).unwrap_or_default()
}

/// The home directory (the original's `os.path.expanduser("~")`).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// The root of user configs (`$XDG_CONFIG_HOME`, otherwise `~/.config`).
fn config_home() -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
    utils::config_home(
        if xdg.is_empty() { None } else { Some(xdg.as_str()) },
        &home(),
    )
}

/// The root of user data (`$XDG_DATA_HOME`, otherwise `~/.local/share`).
fn data_home() -> String {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.local/share", home()),
    }
}

/// The current desktop environment; empty means not recognized.
///
/// The same answer that `SystemInfo.current_de` gives in Python and
/// `current_de()` in `desktops_actions.rs`: one door, no second environment
/// tables.
pub(crate) fn current_de() -> String {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    system::detect_current_de(&xdg, &session, wayland).unwrap_or_default()
}

/// Whether the binary is in PATH.
fn has(binary: &str) -> bool {
    utils::which(binary, None).is_some()
}

/// The found `kreadconfig6`/`kwriteconfig6`, or empty (a port of
/// `_kde_tool`).
fn kde_tool(name: &str) -> String {
    utils::which(name, None).unwrap_or_default()
}

/// Output of a read command; empty means it failed (a port of `_read_cmd`).
fn read_cmd(argv: &[String]) -> String {
    if argv.is_empty() {
        return String::new();
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::Capture,
            timeout_secs: Some(5),
            lang_c: true,
            ..Default::default()
        },
        None,
    );
    if report.returncode != Some(0) {
        return String::new();
    }
    utils::decode_text(&report.stdout)
}

/// Run a write command → whether it worked (a port of `_write_cmd`).
fn write_cmd(argv: &[String]) -> bool {
    if argv.is_empty() {
        return false;
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::Capture,
            timeout_secs: Some(10),
            ..Default::default()
        },
        None,
    );
    report.returncode == Some(0)
}

/// Read a whole file; `None` means there is no file or it is not readable.
fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// The parsed JSON of a file; an empty object means there is no file or it
/// is broken.
///
/// A broken settings file must not bring the tab down: the user will not
/// understand what happened, and the tab is the only place where it is
/// fixed.
fn read_json(path: &str) -> Value {
    read_file(path)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

/// Writing a file through the core's shared door
/// ([`rexon_core::utils::atomic_write`]).
fn write_file_atomic(path: &str, content: &str) -> Result<(), std::io::Error> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    rexon_core::utils::atomic_write(path, content)
}

/// Whether such a `.desktop` lies in the application directories (a port of
/// `_desktop_exists`).
fn desktop_exists(desktop_id: &str) -> bool {
    defaults::desktop_search_paths(desktop_id, &home())
        .iter()
        .any(|p| std::path::Path::new(p).is_file())
}

/// The package manager family of this machine — for "install the package"
/// hints.
fn pkg_family() -> String {
    system::DistroDetector::detect_live()
        .family_live()
        .as_str()
        .to_string()
}

// ============================================================================
// Machine snapshots
// ============================================================================

/// Keys of the `state()` snapshot that the page reads with a dot
/// (`root.st.wp`, `root.st.prot`…).
///
/// The list is named separately not for looks: a forgotten key arrives in
/// QML as `undefined`, and `undefined` in a toggle is a silent "off". Such a
/// divergence is visible neither in the build nor by eye.
const STATE_KEYS: [&str; 18] = [
    "polkit", "lang", "skin", "onboarding", "tray", "fm", "quickfix", "compl", "wp", "lib", "imp",
    "blur", "amb", "snd", "font", "prot", "plasma", "aero",
];

/// Assemble the snapshot by the LIST of keys — the only place where the
/// composition of the `state()` answer is written down.
fn state_object(values: [Value; 18]) -> Value {
    let mut out = Map::new();
    for (key, value) in STATE_KEYS.into_iter().zip(values) {
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

impl qobject::SettingsActions {
    fn facts(&self) -> QString {
        let de = current_de();
        let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();

        // The introductory tour: the scenario and the texts live in the core and
        // are linked into this binary — so it is always available. In Python there
        // was a check "did the `rexon_py` bridge come up" here, which makes no
        // sense in the Rust process.
        let onboarding_available = !onboarding::STEPS.is_empty();

        let plasma = backup::is_plasma_desktop(&xdg) || !plasma_backups().is_empty();
        let aeroshell = !plasma_forks().is_empty();

        QString::from(
            &json!({
                "onboarding": onboarding_available,
                "plasma": plasma,
                "aeroshell": aeroshell,
                "deDisplay": qs::de_display(&de, &xdg, &session),
            })
            .to_string(),
        )
    }

    fn state(&self) -> QString {
        let de = current_de();
        let manager = core_settings::SettingsManager::new().ok();
        let get = |key: &str| -> Option<Value> {
            manager
                .as_ref()
                .and_then(|m| m.get(key))
                .cloned()
        };

        let snapshot = state_object([
            // polkit is one reason for everything privileged, said once and at the top
            // of the page.
            Value::from(polkit_banner()),
            language_state(),
            // ⭐ Fix: the skin is taken by the same path as the page palette.
            Value::from(ui_base::active_skin_id_from(
                get("ui_skin").as_ref().and_then(Value::as_str),
            )),
            // No key yet = the first launch, the tour will be offered.
            Value::from(get(onboarding::KEY_ENABLED) != Some(Value::Bool(false))),
            json!({
                "autostart": crate::tray::autostart_enabled(),
                // We ask Qt itself rather than guessing by the shell name.
                "available": crate::tray::available(),
                // Downloading icons and the cursor for a theme (yes by default).
                "themeDepsAuto": rexon_core::theme_watch::auto_home_enabled(),
            }),
            file_manager_state(&de, manager.as_ref()),
            quickfix_state(&de),
            self.completeness_state(),
            self.wallpaper_state(),
            self.library_section(),
            self.import_state(),
            self.blur_section(),
            self.ambient_section(),
            self.sound_state(&de),
            self.fonts_state(&de),
            protection_state(),
            plasma_state(),
            json!({ "enabled": get(aeroshell_key()) != Some(Value::Bool(false)) }),
        ]);
        QString::from(&snapshot.to_string())
    }

    fn ambient_state(&self) -> QString {
        QString::from(&self.ambient_section().to_string())
    }

    /// The "Ambient sounds" section — the same keys as in Python
    /// (`_ambient_state`).
    ///
    /// The sounds are played by the core's executor, and it knows what is
    /// playing: finished ones are cleared from its list on every poll. While the
    /// executor is busy applying a wallpaper in the background, we do not wait
    /// for its lock — the rows are empty for this poll, and the next snapshot
    /// will bring them back.
    ///
    /// ⭐ Fix (carried over from the previous version): the system is ALWAYS
    /// asked about `mpv` — Python without the engine answered "not installed"
    /// without asking.
    fn ambient_section(&self) -> Value {
        let rows: Vec<Value> = try_with_slot(&live_slot(), |exec| {
            exec.active_ambient()
                .into_iter()
                .map(|sound| {
                    let (name, volume) = qs::ambient_row_texts(
                        Some(&sound.name),
                        Some(&sound.volume.to_string()),
                    );
                    json!({ "id": sound.id, "name": name, "volume": volume })
                })
                .collect()
        })
        .unwrap_or_default();
        json!({ "manager": true, "mpv": has("mpv"), "active": rows })
    }

    /// The "Desktop completeness" section.
    fn completeness_state(&self) -> Value {
        let busy = self.rust().completeness_busy.load(Ordering::SeqCst);
        let state = self.rust().session();
        json!({
            "text": state
                .completeness_note
                .clone()
                .unwrap_or_else(|| t("compl.idle")),
            // The button is disabled from the press to the outcome — and while the
            // question is open.
            "busy": busy,
        })
    }

    /// The "Wallpaper" section — the same keys as in Python
    /// (`_wallpaper_state`).
    ///
    /// The core's executor is linked into the binary, so the section is always
    /// there (`engine: true`). While applying is in progress, the executor is
    /// busy with a background thread: we do NOT wait for its lock — the snapshot
    /// is asked for after every press, and waiting would freeze the tab for the
    /// time of the shell command. The executor is shared by the process, so the
    /// wallpaper set from the "Steam" page is visible here too.
    fn wallpaper_state(&self) -> Value {
        let state = self.rust();
        let busy = state.wallpaper_busy.load(Ordering::SeqCst);
        let (running, format, path) = wallpaper_now(&live_slot());

        // The whole map, with the defaults merged — exactly what Python read
        // (`self._settings.settings`), and with the same core read.
        let audio = wallpaper_audio::AudioSettings::from_map(&core_settings::load_settings(
            &core_settings::settings_file(),
        ));

        let ytdlp_version = if has("yt-dlp") {
            Some(
                state
                    .session()
                    .ytdlp_version
                    .get_or_insert_with(|| {
                        let out = read_cmd(&youtube_wallpaper::version_argv());
                        let out = out.trim();
                        // The same answer Python gave for a failed poll.
                        if out.is_empty() {
                            "?".to_string()
                        } else {
                            out.to_string()
                        }
                    })
                    .clone(),
            )
        } else {
            None
        };

        wallpaper_section(&WallpaperView {
            running,
            format,
            path,
            audio,
            youtube_audio: state.session().youtube_audio.unwrap_or(true),
            ytdlp_version,
            ytdlp_busy: state.ytdlp_busy.load(Ordering::SeqCst),
            busy,
        })
    }

    /// The "Wallpaper blur" section — the same keys as in Python
    /// (`_blur_state`).
    ///
    /// `pillow` is always true here: the blur is done by the core with its own
    /// code, and it does not need a third-party Python library — a "no Pillow"
    /// warning would be untrue.
    fn blur_section(&self) -> Value {
        let state = self.rust();
        json!({
            "engine": true,
            "pillow": true,
            "radius": state.session().blur_radius.unwrap_or(qs::BLUR_RANGE.3),
            "busy": state.wallpaper_busy.load(Ordering::SeqCst),
        })
    }

    /// The "Wallpaper library" section — the same keys as in Python
    /// (`_library_state`). The package list is read by the core
    /// (`wallpaper_engine::list_installed_wallpapers`).
    fn library_section(&self) -> Value {
        let items = self
            .rust()
            .session()
            .library
            .get_or_insert_with(|| {
                let dir = rexon_core::qt_extensions::extensions_dir();
                wallpaper_engine::list_installed_wallpapers(&dir.to_string_lossy())
            })
            .clone();
        let rows: Vec<Value> = items
            .iter()
            .map(|item| {
                // The picture is given as an address, not a path: QML takes the source by
                // URL and would silently not open a path without a scheme (as in Python).
                let preview = if std::path::Path::new(&item.preview).is_file() {
                    QUrl::from_local_file(&QString::from(&item.preview))
                        .to_qstring()
                        .to_string()
                } else {
                    String::new()
                };
                json!({
                    "name": if item.name.is_empty() { t("lib.unnamed") } else { item.name.clone() },
                    "path": item.path,
                    "format": item.format,
                    // The label for the user comes from the core; the internal `format` stays
                    // for the card's logic (viewing, applying).
                    "formatLabel": wallpaper_engine::format_title(&item.format),
                    "preview": preview,
                })
            })
            .collect();
        json!({ "engine": true, "items": rows })
    }

    /// Apply a wallpaper in a background thread — one door for "Picture…" and
    /// the library.
    fn start_wallpaper_apply(self: Pin<&mut Self>, path: String, fmt: String) {
        // The scene name is empty: a file has none, and the executor forbids
        // inventing it from the file name.
        self.run_wallpaper_job(move |exec| {
            let res = exec.apply(&path, &fmt, "");
            qs::wallpaper_done_warned(res.ok, &res.message, res.warning.as_deref())
        });
    }

    /// Long work of the wallpaper executor in a background thread — one door
    /// for applying and blurring. Shell commands and image processing take
    /// seconds; the thread is needed precisely so that the window does not
    /// stand still during that time. `job` returns the outcome line — it arrives
    /// with the `status` signal.
    fn run_wallpaper_job(
        mut self: Pin<&mut Self>,
        job: impl FnOnce(&mut WallpaperExec<LiveSys>) -> String + Send + 'static,
    ) {
        let flag = self.as_ref().get_ref().rust().wallpaper_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // The flag is cleared on a panic of the executor too: otherwise the section
            // would stay "busy" forever, and the buttons would not respond until a
            // restart.
            let busy = BusyGuard(flag);
            // The process executor (shared with "Steam") and the core's shared door:
            // for live wallpapers that need load supervision (video, YouTube) it will
            // bring it up; for the others it will refuse by itself.
            let status = wallpaper_exec::run_in_slot(&live_slot(), job);
            // We clear it BEFORE the signal: the page will re-read the snapshot on
            // `refreshed`, and a raised flag would show it "work in progress".
            drop(busy);
            let queued = handle.queue(move |mut object| {
                object.as_mut().refreshed();
                object.as_mut().status(QString::from(&status));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("settings", "итог применения обоев не доставлен: {e:?}");
                }
            }
        });
    }

    /// The "Importing Windows themes" section.
    fn import_state(&self) -> Value {
        let busy = self.rust().import_busy.load(Ordering::SeqCst);
        json!({
            // ⭐ Fix: the importer is linked into the binary, there is nothing to
            // check.
            "available": true,
            "note": self.rust().session().import_note.clone(),
            "busy": busy,
        })
    }

    /// The "Event sounds" section.
    fn sound_state(&self, de: &str) -> Value {
        let dirs = qs::sound_theme_dirs(&data_home());
        let themes = {
            let mut state = self.rust().session();
            state
                .sound_themes
                .get_or_insert_with(|| qs::list_sound_themes(&dirs))
                .clone()
        };
        // The theme is given AS IS, even empty or unknown: substituting the first
        // installed one showed the user someone else's value as their own. The
        // "— not set —" line is drawn by the core.
        let current = qs::parse_sound_theme(&read_cmd(
            &qs::sound_read_argv_for(de, SoundSetting::Theme, &kde_tool("kreadconfig6"))
                .unwrap_or_default(),
        ));
        // The "listen" buttons stay even with an unset theme: in this case the
        // system plays `freedesktop`, and the sound test must play exactly the same.
        // The substitution is done by the core (`effective_sound_theme`) — it does
        // not get into the list label.
        let showcase = qs::showcase_events(
            &qs::list_theme_events(&dirs, &current),
            qs::SOUND_PREVIEW_BUTTONS,
        );
        json!({
            "themes": themes,
            "current": current,
            "events": qs::parse_event_sounds(&read_cmd(
                &qs::sound_read_argv_for(de, SoundSetting::EventSounds,
                                         &kde_tool("kreadconfig6"))
                    .unwrap_or_default(),
            )),
            "showcase": showcase,
        })
    }

    /// The "System fonts" section: `target → ready label`.
    fn fonts_state(&self, de: &str) -> Value {
        let cached = {
            let mut state = self.rust().session();
            state
                .fonts
                .get_or_insert_with(|| {
                    qs::font_targets_for_wm(de, title_wm(de))
                        .into_iter()
                        .map(|target| {
                            (
                                (*target).to_string(),
                                qs::font_value_text(&read_font(de, target)),
                            )
                        })
                        .collect()
                })
                .clone()
        };
        let mut values = Map::new();
        for (target, text) in cached {
            values.insert(target, Value::from(text));
        }
        let rendering = self
            .rust()
            .session()
            .rendering
            .get_or_insert_with(|| read_rendering(de))
            .clone();
        let dpi = *self.rust().session().font_dpi.get_or_insert_with(|| read_font_dpi(de));
        let store = font_dpi_store(de);
        let mut scope = qs::extra_fonts_scope_note(de);
        if let Some(why) = qs::title_font_note(de, title_wm(de)) {
            scope.push(' ');
            scope.push_str(&why);
        }
        scope.push(' ');
        scope.push_str(&qs::document_font_note());
        if let Some(why) = qs::desktop_font_note(de) {
            scope.push(' ');
            scope.push_str(&why);
        }
        let (label, tip, _title) = qs::change_all_texts();
        let modes: Vec<Value> = qs::change_all_mode_choices()
            .into_iter()
            .map(|(id, label)| json!({ "id": id, "label": label }))
            .collect();
        // Restoring the LXQt desktop icon label font — only if Rexon changed it
        // (there is an "as it was" record).
        let (restore_label, restore_tip) = qs::pcmanfm_restore_texts();
        let desktop_restore = json!({
            "visible": de == "lxqt" && qs::pcmanfm_font_record(&qs::PcmanfmPaths::from_env()).is_some(),
            "label": restore_label,
            "tip": restore_tip,
        });
        json!({
            "values": values,
            "scope": scope,
            "desktop_restore": desktop_restore,
            "render": rendering_state(&rendering),
            "all": { "label": label, "tip": tip, "modes": modes },
            "dpi": {
                "available": store.is_some(),
                "enabled": dpi.is_some(),
                "value": dpi.unwrap_or(qs::FONT_BASE_DPI),
                "min": qs::FONT_DPI_RANGE.0,
                "max": qs::FONT_DPI_RANGE.1,
                "note": qs::font_dpi_note_for(de, store.as_ref()),
            },
        })
    }
}

/// Where the font DPI is kept in this session (`None` — nothing to set it
/// with).
fn font_dpi_store(de: &str) -> Option<qs::FontDpiStore> {
    qs::font_dpi_store(de, wayland_session())
}

/// The set font DPI (`None` — by the screen).
fn read_font_dpi(de: &str) -> Option<i32> {
    let store = font_dpi_store(de)?;
    match qs::font_dpi_read_argv(&store, &kde_tool("kreadconfig6")) {
        Some(argv) => qs::parse_font_dpi(&store, &read_cmd(&argv)),
        None if store == qs::FontDpiStore::Xresources => {
            read_file(&xresources_path()).and_then(|t| qs::xresources_dpi(&t))
        }
        None => read_file(&fontconfig_path()).and_then(|t| qs::fontconfig_rendering(&t).dpi),
    }
}

/// Whether a Wayland session is running.
/// Who draws the window titles in an environment without its own window
/// manager (LXQt, labwc) — `qt_settings::title_wm_for` by
/// `lxqt/session.conf`: our own, then the system ones (`$XDG_CONFIG_DIRS`,
/// the way `startlxqt` looks for them).
pub(crate) fn title_wm(de: &str) -> Option<qs::TitleWm> {
    let mut confs = Vec::new();
    if de == "lxqt" {
        let mut dirs = vec![config_home()];
        let system = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
        dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(str::to_string));
        dirs.extend(["/etc/xdg".to_string(), "/usr/share".to_string()]);
        confs = dirs.iter().filter_map(|d| read_file(&format!("{}/lxqt/session.conf", d.trim_end_matches('/')))).collect();
    }
    let refs: Vec<&str> = confs.iter().map(String::as_str).collect();
    qs::title_wm_for(de, &refs, wayland_session())
}

fn wayland_session() -> bool {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    rexon_core::wallpaper_mmap::is_wayland_session(&session, &display)
}

/// The `~/.Xresources` path.
fn xresources_path() -> String {
    format!("{}/{}", home().trim_end_matches('/'), qs::XRESOURCES_REL_PATH)
}

/// Write `Xft.dpi` into our block of `~/.Xresources` and, if an X11 session
/// is running, merge the same line into the live resource database — new
/// programs will see the DPI at once, without re-logging in. A removed DPI
/// is not taken out of the live database (xrdb can only remove everything
/// at once): it goes away at the next login.
fn apply_xresources_dpi(dpi: Option<i32>) -> bool {
    let path = xresources_path();
    let existing = read_file(&path);
    if existing.is_none() && dpi.is_none() {
        // There is nothing to remove, and there is no reason to create an empty
        // file.
        return true;
    }
    let Some(text) = qs::xresources_with_dpi(existing.as_deref(), dpi) else {
        return false;
    };
    if text.trim().is_empty() {
        // The file had only our block — we do not leave an empty file.
        return std::fs::remove_file(&path).is_ok();
    }
    if write_file_atomic(&path, &text).is_err() {
        return false;
    }
    let display = std::env::var("DISPLAY").unwrap_or_default();
    if let (Some(dpi), false, true) = (dpi, display.is_empty() || wayland_session(), has("xrdb")) {
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        let snippet = format!("{}/rexon-xft-dpi.ad", runtime.trim_end_matches('/'));
        if write_file_atomic(&snippet, &qs::xft_dpi_line(dpi)).is_ok() {
            let _ = write_cmd(&qs::xrdb_merge_argv(&snippet));
            let _ = std::fs::remove_file(&snippet);
        }
    }
    true
}

/// Write the font DPI into the environment's storage → whether it worked.
fn apply_font_dpi(de: &str, dpi: Option<i32>) -> bool {
    let Some(store) = font_dpi_store(de) else { return false };
    if store == qs::FontDpiStore::Xresources {
        return apply_xresources_dpi(dpi);
    }
    if store != qs::FontDpiStore::Fontconfig {
        return match qs::font_dpi_write_argv(&store, dpi, &kde_tool("kwriteconfig6")) {
            Some(argv) => write_cmd(&argv),
            None => false,
        };
    }
    // LXQt: the DPI lives in the same fonts.conf block as the antialiasing —
    // the block is rewritten whole with the current rendering.
    let path = fontconfig_path();
    let existing = read_file(&path);
    let block = qs::fontconfig_block(&read_rendering(de), dpi);
    let ok = match qs::fontconfig_with_block(existing.as_deref(), &block) {
        Ok(text) => write_file_atomic(&path, &text).is_ok(),
        Err(_) => false,
    };
    // LXQt's GTK programs read GNOME's text scaling factor.
    let gnome = qs::FontDpiStore::TextScaling { schema: "org.gnome.desktop.interface".to_string() };
    if ok && has("gsettings") {
        if let Some(argv) = qs::font_dpi_write_argv(&gnome, dpi, "") {
            let _ = write_cmd(&argv);
        }
    }
    // In an X11 session — also Xft.dpi: LXQt does not put it into the resource
    // database.
    if ok && qs::font_dpi_xresources_also(de, wayland_session()) {
        let _ = apply_xresources_dpi(dpi);
    }
    ok
}

/// The snapshot's "Font rendering" section: the current values and the
/// choice lists.
fn rendering_state(r: &qs::FontRendering) -> Value {
    let choices = |list: Vec<(&str, String)>| -> Vec<Value> {
        list.into_iter().map(|(id, label)| json!({ "id": id, "label": label })).collect()
    };
    let (from, to) = r.exclude.unwrap_or(qs::AA_EXCLUDE_DEFAULT);
    json!({
        "antialias": r.antialias,
        "exclude": r.exclude.is_some(),
        "from": from,
        "to": to,
        "min": qs::AA_EXCLUDE_LIMITS.0,
        "max": qs::AA_EXCLUDE_LIMITS.1,
        "subpixel": r.subpixel,
        "hinting": r.hinting,
        "subpixels": choices(qs::subpixel_choices()),
        "hintings": choices(qs::hinting_choices()),
        // Ready-made MATE presets: each carries its values whole — the button
        // passes them to the same `setFontRendering` as the manual setting.
        "presets": qs::render_preset_choices()
            .into_iter()
            .filter_map(|(id, label, tip)| {
                let p = qs::render_preset(id, r)?;
                Some(json!({
                    "id": id,
                    "label": label,
                    "tip": tip,
                    "antialias": p.antialias,
                    "subpixel": p.subpixel,
                    "hinting": p.hinting,
                }))
            })
            .collect::<Vec<Value>>(),
        "preset": qs::render_preset_of(r).unwrap_or(""),
    })
}

/// Whether there are rendering keys of old GNOME
/// (`qt_settings::LEGACY_XSETTINGS_SCHEMA`).
fn legacy_render_available() -> bool {
    has("gsettings") && qs::legacy_render_available(&read_cmd(&qs::legacy_render_probe_argv()))
}

/// Fonts installed according to `fc-list` and those disabled by Rexon:
/// `fc-list` does not see them, so each disabled file is asked from
/// `fc-query`.
fn read_font_library() -> Vec<qs::InstalledFont> {
    let dirs = font_user_dirs_checked();
    let mut fonts = qs::parse_fc_list_fonts(&read_cmd(&qs::fc_list_fonts_argv()), &dirs);
    for file in read_disabled_fonts() {
        // fontconfig still gives a disabled file for a while after the block is
        // written (until it updates its cache): such a font is already in the list
        // — we mark it rather than skip it. Otherwise a just disabled font kept a
        // "Disable" button, and there was nothing to enable it back with
        // (acceptance of 10.10.2026).
        let mut listed = false;
        for f in fonts.iter_mut().filter(|f| f.file == file) {
            f.disabled = true;
            listed = true;
        }
        if listed || !std::path::Path::new(&file).is_file() {
            continue;
        }
        let out = read_cmd(&qs::fc_query_fonts_argv(&file));
        fonts.extend(qs::parse_fc_list_fonts(&out, &dirs).into_iter().map(|mut f| {
            f.disabled = true;
            f
        }));
    }
    // Scripts that fontconfig recognizes not by language but by a character.
    for (script, ch) in qs::FONT_SCRIPT_PROBES {
        qs::font_add_probed_script(&mut fonts, script, &read_cmd(&qs::fc_list_char_argv(*ch)));
    }
    fonts
}

/// Our own sample text from the settings (or the default sample).
fn font_sample_text() -> String {
    let settings = core_settings::load_settings(&core_settings::settings_file());
    qs::font_sample_text(settings.get(qs::FONT_SAMPLE_TEXT_KEY).and_then(Value::as_str))
}

/// Files disabled by our `fonts.conf` block.
fn read_disabled_fonts() -> Vec<String> {
    read_file(&fontconfig_path())
        .map(|text| qs::fontconfig_disabled_files(&text))
        .unwrap_or_default()
}

/// Write the list of disabled fonts into our `fonts.conf` block → whether
/// it was written. `Err` means the file does not parse or our block in it
/// was broken by hand (then it is not touched). If the block became empty
/// and nothing but the skeleton we created is left in the file, the file is
/// removed: it did not exist before Rexon.
fn write_disabled_fonts(path: &str, existing: Option<&str>, files: &[String]) -> Result<bool, qs::FontconfigRefusal> {
    let text = qs::fontconfig_with_disabled(existing, files)?;
    Ok(if qs::fontconfig_is_bare(&text) {
        existing.is_none() || std::fs::remove_file(path).is_ok()
    } else {
        write_file_atomic(path, &text).is_ok()
    })
}

/// The user's font directories — as written and as they are on disk (the
/// home directory may be a link: `/home` → `/var/home`).
fn font_user_dirs_checked() -> Vec<String> {
    let mut dirs = qs::font_user_dirs(&home(), &data_home());
    for d in dirs.clone() {
        if let Ok(real) = std::fs::canonicalize(&d) {
            let real = real.to_string_lossy().into_owned();
            if !dirs.contains(&real) {
                dirs.push(real);
            }
        }
    }
    dirs
}

/// A file path with the directory resolved (a link file itself is not
/// resolved: the link is removed, not what it points to).
fn real_parent_path(file: &str) -> String {
    let path = std::path::Path::new(file);
    match (path.parent().and_then(|p| std::fs::canonicalize(p).ok()), path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name).to_string_lossy().into_owned(),
        _ => file.to_string(),
    }
}

/// Put a font file into the user's font directory and rebuild its cache.
/// The door is one for the product — the core
/// (`local_install::install_font_file`): "Install from file…" of the
/// "Appearance" tab installs fonts through it too.
fn install_font(source: &str) -> qs::FontInstallOutcome {
    rexon_core::local_install::install_font_file(source, &data_home(), true)
}

/// A removed disabled file no longer needs to be kept in our `fonts.conf`
/// block: remove from it those of `files` that no longer exist.
fn forget_removed_disabled(files: &[String]) {
    if files.is_empty() {
        return;
    }
    let path = fontconfig_path();
    let existing = read_file(&path);
    let disabled = existing.as_deref().map(qs::fontconfig_disabled_files).unwrap_or_default();
    let gone: Vec<String> = files.iter().filter(|f| !std::path::Path::new(f).exists()).cloned().collect();
    if disabled.iter().any(|f| gone.contains(f)) {
        let _ = write_disabled_fonts(&path, existing.as_deref(), &qs::font_disabled_after(&disabled, &gone, true));
    }
}

/// The path of our own font groups file — the same as KDE's.
fn font_groups_path() -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), qs::FONT_GROUPS_REL_PATH)
}

/// Our own font groups. No file — no groups; `Err` means the file does not
/// parse (we do not rewrite it).
fn read_font_groups() -> Result<Vec<qs::FontGroup>, ()> {
    match read_file(&font_groups_path()) {
        Some(text) => qs::parse_font_groups(&text),
        None => Ok(Vec::new()),
    }
}

/// The path of the user's `fonts.conf`.
fn fontconfig_path() -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), qs::FONTCONFIG_REL_PATH)
}

/// The current font rendering: the environment's native keys, then
/// `fonts.conf` (`qt_settings::merge_rendering`).
fn read_rendering(de: &str) -> qs::FontRendering {
    let native = match qs::rendering_read_argv(de, &kde_tool("kreadconfig6")) {
        Some((dialect, reads)) => {
            let answers: Vec<(qs::RenderKey, String)> =
                reads.into_iter().map(|(key, argv)| (key, read_cmd(&argv))).collect();
            qs::parse_render_answers(dialect, &answers)
        }
        None => qs::RenderingParts::default(),
    };
    // Old GNOME: there are no new keys, the rendering is in the settings
    // daemon's schema.
    let native = if qs::rendering_needs_legacy(&native, legacy_render_available()) {
        let (dialect, reads) = qs::legacy_rendering_read_argv();
        let answers: Vec<(qs::RenderKey, String)> =
            reads.into_iter().map(|(key, argv)| (key, read_cmd(&argv))).collect();
        qs::parse_render_answers(dialect, &answers)
    } else {
        native
    };
    let fc = read_file(&fontconfig_path())
        .map(|text| qs::fontconfig_rendering(&text))
        .unwrap_or_default();
    qs::merge_rendering(&native, &fc)
}

/// Write the font rendering by the core's plan: our own block in
/// `fonts.conf` (a shared layer for all programs), the environment's native
/// keys, the GNOME keys for GTK. A forced DPI, if already set by the block,
/// is kept.
fn apply_rendering(de: &str, r: &qs::FontRendering) -> qs::RenderingOutcome {
    let path = fontconfig_path();
    let existing = read_file(&path);
    let dpi = existing.as_deref().and_then(|t| qs::fontconfig_rendering(t).dpi);
    let text = match qs::fontconfig_with_block(existing.as_deref(), &qs::fontconfig_block(r, dpi)) {
        Ok(text) => text,
        Err(_) => return qs::RenderingOutcome::FontconfigRefused(path),
    };
    if write_file_atomic(&path, &text).is_err() {
        return qs::RenderingOutcome::FontconfigFailed(path);
    }
    let plan = qs::rendering_plan(de, r, &kde_tool("kwriteconfig6"), has("gsettings"));
    let mut native_ok = true;
    for argv in &plan.primary {
        native_ok &= write_cmd(argv);
    }
    for argv in &plan.also {
        let _ = write_cmd(argv);
    }
    // Old GNOME keeps the rendering in the settings daemon's schema — it is
    // written only if the schema still knows these keys.
    if legacy_render_available() {
        let mut legacy_ok = true;
        for argv in qs::legacy_rendering_argv(r) {
            legacy_ok &= write_cmd(&argv);
        }
        // Old GNOME: there are no new keys, the native ones are these.
        if legacy_ok && qs::legacy_render_is_native(de) {
            native_ok = true;
        }
    }
    if native_ok {
        qs::RenderingOutcome::Saved
    } else {
        qs::RenderingOutcome::NativeFailed
    }
}

/// The warning strip "there is no polkit in the system"; empty means there
/// is.
fn polkit_banner() -> String {
    if has("pkexec") {
        return String::new();
    }
    let hint = utils::tool_package_hint("pkexec", &pkg_family());
    t1("polkit.banner", &system::polkit_missing_message(&hint))
}

/// The "File manager" section.
fn file_manager_state(de: &str, manager: Option<&core_settings::SettingsManager>) -> Value {
    let current = manager
        .and_then(|m| m.get("file_manager"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    let mut badges = Map::new();
    for (fm_id, _label) in qs::fm_choices() {
        if fm_id != "auto" {
            badges.insert(fm_id.clone(), Value::from(has(&fm_id)));
        }
    }
    json!({
        "current": current,
        "badges": Value::Object(badges),
        "currentName": resolve_file_manager(de, &current),
    })
}

/// Which file manager is actually installed (`auto` — the one native to
/// this environment).
fn resolve_file_manager(de: &str, setting: &str) -> String {
    if setting != "auto" {
        return setting.to_string();
    }
    appdefaults::default_file_manager_for_de(Some(de), &|binary| has(binary))
}

/// The "Environment's native applications" section.
///
/// Warning: the environment name here is DIFFERENT from the one in the
/// section headers: the headers name the SESSION (`qt_settings::de_display`
/// — "KDE"), while this section returns the applications of a specific
/// desktop and must call it by its full name from the desktop database
/// ("KDE Plasma"). One name for both cases gave "Restore KDE applications"
/// versus "…KDE Plasma" in one product.
fn quickfix_state(de: &str) -> Value {
    let known = appdefaults::de_apps_info(de, &desktop_exists)
        .iter()
        .any(|(_, resolved)| resolved.is_some());
    json!({ "known": known, "de": screenshots::display_name(de) })
}


/// The "Default application protection" section.
fn protection_state() -> Value {
    let rules = protection_rules();
    let protected = protected_apps();
    let current = current_defaults();
    let summary = defaults::get_status_summary(&protected, &current, &rules);

    let changed: Vec<(String, String, String)> = summary
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row.get("changed") == Some(&Value::Bool(true)))
                .map(|row| {
                    let field = |key: &str| {
                        row.get(key).and_then(Value::as_str).unwrap_or("").to_string()
                    };
                    (
                        defaults::app_type_label(&field("type")).to_string(),
                        field("saved"),
                        field("current"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    // How many applications are really remembered. An empty string in the
    // reference file is "not remembered", not "remembered empty": otherwise
    // the section would again report success while storing nothing.
    let remembered_count = protected
        .iter()
        .filter(|(_, v)| v.as_deref().is_some_and(|s| !s.trim().is_empty()))
        .count();

    let mut rule_map = Map::new();
    let mut remembered = Map::new();
    for key in qs::protection_types() {
        rule_map.insert(key.to_string(), Value::from(rules.protect(key)));
        remembered.insert(
            key.to_string(),
            Value::from(
                protected
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| v.clone())
                    .unwrap_or_default(),
            ),
        );
    }

    let note = defaults::terminal_status_note(has("xdg-terminal-exec"));
    json!({
        "enabled": rules.enabled(),
        "rules": Value::Object(rule_map),
        "remembered": Value::Object(remembered),
        "note": if note.is_empty() { String::new() } else { t1("prot.note", &note) },
        "noteTip": if note.is_empty() {
            String::new()
        } else {
            utils::tool_package_hint("xdg-terminal-exec", &pkg_family())
        },
        "status": qs::protection_status_text(remembered_count, &changed),
        "canRestore": !changed.is_empty(),
        "terminalColors": rexon_core::terminal_guard::enabled(&rexon_core::utils::rexon_config_dir()),
    })
}

/// The "Plasma panel copies" section. The list is read for real: it is
/// ordinary JSON on disk taken by the live product, and hiding it would be
/// a lie.
fn plasma_state() -> Value {
    let backups = plasma_backups();
    let shown: Vec<Value> = backups
        .iter()
        .take(qs::PLASMA_BACKUPS_SHOWN)
        .map(|b| {
            json!({
                "ts": b.timestamp,
                "label": qs::backup_row_label(&b.timestamp, &b.reason, b.files.len() as i64),
                "tip": t1("plasma.backup_dir", &b.timestamp),
            })
        })
        .collect();
    json!({
        "header": qs::plasma_backups_header(backups.len()),
        "backups": shown,
    })
}

// ============================================================================
// Observations the core does not have
// ============================================================================

/// The Plasma panel copies directory (`PlasmaProtector._backup_dir`).
fn plasma_backup_dir() -> String {
    format!("{}/rexon/backups/plasma-panel", data_home())
}

/// Plasma panel copies from newest to oldest; the order is set by the core.
///
/// There is no longer a meta file read of our own here: it is read and
/// parsed by the same executor that writes it
/// (`backup::plasma_meta_entries`). Two readers of one file silently drift
/// apart — the list would show one thing, and the "Restore" button would
/// take another.
fn plasma_backups() -> Vec<backup::PlasmaBackup> {
    let raw = backup::plasma_meta_entries(&plasma_backup_dir());
    backup::plasma_backups(&backup::plasma_parse_entries(&raw))
}

/// `$XDG_CONFIG_HOME`, if it is set: with a non-standard config root the
/// panel copy would save and restore NOT the files that Plasma reads.
fn xdg_config_home_env() -> Option<String> {
    std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty())
}

/// The copy's timestamp — `strftime("%Y%m%d_%H%M%S")` of local time. The
/// product has one clock (`qt_app::Timestamp::now_local`), there is none of
/// our own here.
fn plasma_stamp() -> String {
    qt_app::Timestamp::now_local()
        .unwrap_or_default()
        .file_stamp()
}

/// Take a copy of the Plasma panel — ONE call for both tabs ("Settings" and
/// "Desktops", `desktops_actions.rs`), like one `PlasmaProtector` in Python.
/// The set of panel configs and expanding `~` are the core's; the snapshot
/// is made by its executor too. Here there is only the copies directory,
/// the timestamp and the reason.
///
/// `Err` is a DISK refusal (the directory was not created, the meta file
/// was not written); in Python this was an exception.
pub(crate) fn panel_backup_now(reason: &str) -> Result<(bool, String), String> {
    let configs = backup::expand_tilde_xdg(
        backup::PANEL_CONFIGS,
        &home(),
        xdg_config_home_env().as_deref(),
    );
    let outcome =
        backup::plasma_backup_run(&plasma_backup_dir(), &configs, reason, false, &plasma_stamp());
    if matches!(outcome, Ok((true, _))) {
        notify_backup_created();
    }
    outcome
}

/// Tell BonziBuddy about a new panel copy — as Python did
/// (`rexon_bridge.notify("notify_backup_created")` in `PlasmaProtector`).
///
/// The core's bridge is the same (`rexon_bridge::RexonBridge`): an event is
/// a file in `~/.config/rexon/bridge_commands`, there is no second
/// mechanism. The bridge is an auxiliary link: its refusal does not bring
/// the copy down but is only written into the log. Cleaning up events older
/// than 5 minutes is done by whoever litters, as in Python.
fn notify_backup_created() {
    use rexon_core::rexon_bridge::RexonBridge;

    let bridge = RexonBridge::new_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let now_iso = qt_app::Timestamp::now_local()
        .unwrap_or_default()
        .isoformat();
    let sent = bridge
        .ensure_dirs()
        .and_then(|_| bridge.notify_backup_created(&now_iso, now.as_millis() as u64));
    if let Err(e) = sent {
        rexon_core::log_warn!("bridge", "мост BonziBuddy: backup_created не отправлено — {e}");
    }
    bridge.cleanup_commands(300.0, now.as_secs_f64());
}

/// Restore the Plasma panel from a copy (`None` — from the last one) — the
/// shared call of both tabs, like `PlasmaProtector.restore_panel`.
///
/// Restarting the shell is the only step by which restoring a copy touches
/// the LIVE desktop, and it is done only on Plasma itself: on another
/// desktop there is nothing to restart. How exactly to restart (a unit →
/// kquitapp/kstart → the PID of the shell of its own session) is decided
/// and executed by the core in one function.
pub(crate) fn panel_restore_now(timestamp: Option<&str>) -> (bool, String) {
    let (ok, mut message) = backup::plasma_restore_run(&plasma_backup_dir(), timestamp);
    if ok && backup::is_plasma_desktop(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
    {
        let outcome = backup::plasma_restart_shell();
        message.push_str(&backup::plasma_restart_message(outcome));
    }
    (ok, message)
}

/// Installed heavy Plasma forks (AeroThemePlasma/AeroShell).
///
/// Arch only — the forks live exclusively in AUR; the selection is done by
/// the core.
fn plasma_forks() -> Vec<String> {
    if system::DistroDetector::detect_live().family_live() != system::PkgFamily::Arch {
        return Vec::new();
    }
    if !has("pacman") {
        return Vec::new();
    }
    let stdout = read_cmd(&["pacman".to_string(), "-Qq".to_string()]);
    let installed: Vec<String> = stdout.lines().map(|l| l.trim().to_string()).collect();
    system_plan::filter_plasma_forks(&installed)
}

/// Install the package update watcher (systemd user units, without a
/// password; `rexon_core::update_guard`) — in the background: it is
/// `systemctl --user`. Everyone on Arch needs it: protection against an
/// update that installs a set of packages out of step, and with Plasma
/// forks installed — also the AeroThemePlasma check (it checks its own
/// setting itself). A failure does not hinder the window's work — only a
/// log entry.
fn sync_update_watch() {
    std::thread::spawn(|| {
        if let Err(e) = dx::update_watch_live() {
            rexon_core::log_warn!("settings", "слежка за обновлениями: {e}");
        }
    });
}

/// Whether a "before update" check is already running (at launch and by
/// the button at once).
static UPDATE_GUARD_BUSY: AtomicBool = AtomicBool::new(false);

/// A font installation or removal "for everyone" is in progress (the
/// password window, the helper): a second press before the outcome is a
/// refusal in words, not a second operation.
static SHARED_FONT_BUSY: AtomicBool = AtomicBool::new(false);

fn aeroshell_key() -> &'static str {
    aeroshell::CHECK_ON_START_SETTING
}

/// Whether to check the AeroShell look at Rexon start-up.
///
/// AeroShell (AeroThemePlasma) is a Plasma look: its task bar and "Start"
/// live only in a Plasma session. A live check in MATE: at launch a "The
/// look needs to be rebuilt" window popped up about the panel and "Start",
/// which are not and will not be in this session — the question is out of
/// place. Outside Plasma we stay silent at launch (a log entry), and the
/// packages will be checked at the next login into Plasma: we will ask
/// there. Pressing the notification button (`from_notice`) is the user's
/// request, and we execute it in any session; "Check now" does not come
/// here at all.
fn startup_survey_wanted(enabled: bool, from_notice: bool, plasma: bool) -> bool {
    from_notice || (enabled && plasma)
}

/// The target's current system font, as the environment reads it: from the
/// native storage (`qt_settings::font_plan`) — a command or a file (COSMIC,
/// LXQt). The style stays in the string — [`read_font_face`] parses it.
fn read_font(de: &str, target: &str) -> String {
    let raw = match qs::font_plan_wm(de, target, title_wm(de)).primary {
        // sway: our block in its config, otherwise the `font` line of the config
        // or sway's default font.
        qs::FontStore::File(qs::FontFile::SwayConf) => qs::sway_title_font_spec_live(),
        // The LXQt desktop: what pcmanfm-qt reads (its own directory or the system
        // one); if `Font` is not set, the labels have the interface font.
        qs::FontStore::File(qs::FontFile::PcmanfmDesktop) => {
            let spec = qs::pcmanfm_desktop_font_spec(&qs::PcmanfmPaths::from_env());
            if spec.is_empty() {
                return read_font(de, "system");
            }
            spec
        }
        qs::FontStore::Cmd(backend) => read_cmd(
            &qs::sound_read_argv(&backend, &kde_tool("kreadconfig6")).unwrap_or_default(),
        ),
        // rc.xml of Openbox/labwc: without its own, the manager reads the system
        // one.
        qs::FontStore::File(file) => read_file(&font_file_path(&file))
            .or_else(|| matches!(file, qs::FontFile::RcXml { .. }).then(|| qs::FontFileIo::read_system(&ConfigFiles, &file.rel_path())).flatten())
            .map(|text| file.read_spec(&text))
            .unwrap_or_default(),
    };
    // xfdesktop keeps only the size: the effective desktop icon label font is
    // the interface font with this size (if our own size is enabled).
    if qs::font_target_size_only(de, target) {
        return qs::xfdesktop_font_spec(&read_cmd(&qs::xfdesktop_use_size_read_argv()), &raw, &read_font(de, "system"));
    }
    raw.trim().trim_matches(['\'', '"']).to_string()
}

/// [`read_font`] parsed: the family, the size and the style
/// (`qt_settings::parse_font_spec_face` — a Pango gsettings/xfconf
/// description, a KDE and Qt QFont record).
fn read_font_face(de: &str, target: &str) -> (String, i32, qs::FontFace) {
    qs::parse_font_spec_face(&read_font(de, target))
}

/// The full path of the font storage file.
fn font_file_path(file: &qs::FontFile) -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), file.rel_path())
}

/// Write the font into one storage → whether it worked.
fn write_font_store(store: &qs::FontStore, family: &str, size: i32, face: qs::FontFace) -> bool {
    match store {
        // sway: our file, an `include` block in the config checked by sway itself
        // and `swaymsg font …` — the core, with the same mechanism as the accent.
        qs::FontStore::File(qs::FontFile::SwayConf) => qs::sway_title_font_apply_live(family, size, face),
        // The LXQt desktop: close the live pcmanfm-qt, a backup copy, editing one
        // line, starting the desktop — the core.
        qs::FontStore::File(qs::FontFile::PcmanfmDesktop) => apply_lxqt_desktop_font(family, size, face).is_ok(),
        qs::FontStore::Cmd(_) => {
            // The style — in Pango words in gsettings/xfconf, in QFont fields in
            // kdeglobals (`font_store_write_argv_face`).
            match qs::font_store_write_argv_face(store, family, size, face, &kde_tool("kwriteconfig6")) {
                Some(argv) => write_cmd(&argv),
                None => false,
            }
        }
        qs::FontStore::File(file) => {
            let base = config_home();
            let Some(writes) = file.file_writes_face(&base, &ConfigFiles, family, size, face) else {
                return false;
            };
            writes.iter().all(|w| write_config_file(&base, w))
        }
    }
}

/// The LXQt desktop icon label font with protection of the desktop settings
/// (`qt_settings::pcmanfm_desktop_font_apply`).
fn apply_lxqt_desktop_font(family: &str, size: i32, face: qs::FontFace) -> Result<qs::PcmanfmApplied, String> {
    let applied =
        qs::pcmanfm_desktop_font_apply(&qs::PcmanfmPaths::from_env(), &qs::LivePcmanfmBus, family, size, face);
    if let Err(e) = &applied {
        rexon_core::log_warn!("fonts", "шрифт стола LXQt не записан — {e}");
    }
    applied
}

/// Settings files under `$XDG_CONFIG_HOME` — for the core's plans
/// (`qt_settings::FontFileIo`).
struct ConfigFiles;

impl qs::FontFileIo for ConfigFiles {
    fn read(&self, rel_path: &str) -> Option<String> {
        read_file(&format!("{}/{rel_path}", config_home().trim_end_matches('/')))
    }

    fn read_system(&self, rel_path: &str) -> Option<String> {
        let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
        let found = dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .chain(["/etc/xdg"])
            .find_map(|d| read_file(&format!("{}/{rel_path}", d.trim_end_matches('/'))));
        found
    }

    fn link_target(&self, rel_path: &str) -> Option<String> {
        let path = format!("{}/{rel_path}", config_home().trim_end_matches('/'));
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.file_type().is_symlink() {
            return None;
        }
        std::fs::canonicalize(&path).ok().map(|p| p.to_string_lossy().into_owned())
    }
}

/// Execute one file write of a font plan. A link to someone else's theme is
/// removed rather than edited through (`FontFileWrite::replace_link`).
fn write_config_file(base: &str, w: &qs::FontFileWrite) -> bool {
    let path = format!("{}/{}", base.trim_end_matches('/'), w.rel_path);
    if w.replace_link && std::fs::remove_file(&path).is_err() {
        return false;
    }
    write_file_atomic(&path, &w.text).is_ok()
}

/// The font choice shim's answer → what to apply. `None` means there is
/// nothing to apply (the user closed the window without choosing anything).
///
/// Parsing the "Family,size" string is done by the core with the same
/// `parse_font_spec` that reads the environment setting. ONE decision stays
/// here: a non-positive size means "the font is set in pixel size, it has no
/// points" — then the same fallback size 10 as in the Python original is
/// taken (`font.pointSize() if font.pointSize() > 0 else 10`). Without it
/// "Noto Sans,-1" would go into the config, and the environment would show a
/// zero-size font.
fn chosen_font(spec: &str) -> Option<(String, i32)> {
    let (family, size) = qs::parse_font_spec(spec);
    if family.is_empty() {
        return None;
    }
    Some((family, if size > 0 { size } else { 10 }))
}

/// Write the target's font into the current environment → whether it
/// worked.
///
/// A port of `_apply_font`, extended to all environments: the plan is
/// decided entirely by the core (`qt_settings::font_plan`) — the
/// environment's native storage (KDE — `kwriteconfig6`, XFCE — `xfconf`,
/// GNOME/Cinnamon/MATE/Budgie — their own `gsettings` keys, COSMIC —
/// `cosmic-config`, LXQt — `lxqt.conf`), companions for GTK and Qt programs
/// and the transfer into `gtk-{3,4}.0/settings.ini`. The ini text is
/// assembled by `gtk_sanitizer::write_ini_setting_in` (the same engine that
/// repairs the GTK look — no second rule about the `[Settings]` section
/// appears in the project). Success is decided only by the native storage.
fn apply_font(de: &str, target: &str, family: &str, size: i32, face: qs::FontFace) -> bool {
    let plan = qs::font_plan_wm(de, target, title_wm(de));
    // Companion commands go before the main write: a removed "titles in the
    // system font" must get there before the window manager re-reads the font.
    for argv in &plan.extra {
        let _ = write_cmd(argv);
    }
    let ok = write_font_store(&plan.primary, family, size, face);
    // Auxiliary writes do not make the applying fail: the environment's native
    // font is already written, and lying about that is not allowed.
    for store in &plan.also {
        let _ = write_font_store(store, family, size, face);
    }
    // Tell the window manager to re-read its settings (Openbox, labwc).
    if ok {
        for argv in &plan.notify {
            let _ = write_cmd(argv);
        }
    }

    // The transfer to GTK — only for a font that has its own key there at all
    // (the monospace and title ones have none).
    if plan.gtk_ini {
        // `gtk-font-name` is a Pango description, the style in it is in words.
        let spec = qs::pango_font_desc(family, size, face);
        for path in qs::gtk_font_ini_paths(&config_home()) {
            let text = gtk_sanitizer::write_ini_setting_in(
                read_file(&path).as_deref(),
                "Settings",
                qs::GTK_FONT_KEY,
                &spec,
            );
            let _ = write_file_atomic(&path, &text);
        }
        remove_stray_gtk_font_dirs();
    }
    ok
}

/// Remove the `gtk-gtk-{3,4}.0` directories created by the former erroneous
/// font write — only if they contain exactly our file
/// (`qt_settings::stray_gtk_font_dir_removable`).
fn remove_stray_gtk_font_dirs() {
    for (dir, ini) in qs::stray_gtk_font_dirs(&config_home()) {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let entries: Vec<String> =
            read.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        if qs::stray_gtk_font_dir_removable(&entries, read_file(&ini).as_deref()) {
            let _ = std::fs::remove_file(&ini);
            let _ = std::fs::remove_dir(&dir);
        }
    }
}

/// Apply a sound setting by the core's shared plan
/// (`qt_settings::sound_plan`) — the same as in "Appearance": the
/// environment's native key, the shared layer for GTK
/// (`org.gnome.desktop.sound`) and KDE/Qt programs (`kdeglobals [Sounds]`),
/// then a check by reading.
///
/// Success only if the NATIVE mechanism gave back what was written
/// (`check`): `kwriteconfig6` and `gsettings` can exit with zero without
/// writing anything.
fn apply_sound_setting(setting: SoundSetting, value: &str, check: impl Fn(&str) -> bool) -> bool {
    let de = current_de();
    let plan = qs::sound_plan(
        &de,
        setting,
        value,
        &kde_tool("kwriteconfig6"),
        &kde_tool("kreadconfig6"),
        has("gsettings"),
    );
    let Some(primary) = plan.write.first() else {
        return false;
    };
    let primary_ok = write_cmd(primary);
    for argv in plan.write.iter().skip(1) {
        let _ = write_cmd(argv);
    }
    match plan.verify {
        Some(verify) => check(&read_cmd(verify.as_slice())),
        // There is nothing to read with — the write's exit code remains.
        None => primary_ok,
    }
}

/// Enable or disable the event sounds of the running environment — the
/// outcome as a line for the user. One door for the "Settings" toggle and
/// the "System sounds are off" strip in the "Appearance" tab.
pub(crate) fn switch_event_sounds(enabled: bool) -> String {
    let ok = apply_sound_setting(
        SoundSetting::EventSounds,
        qs::event_sounds_value(enabled),
        |raw| qs::parse_event_sounds(raw) == enabled,
    );
    qs::event_sounds_status(ok, enabled)
}

/// The raw answer of the event sounds toggle of environment `de`; empty
/// means there is nothing to read with.
pub(crate) fn event_sounds_raw(de: &str) -> String {
    read_cmd(
        &qs::sound_read_argv_for(de, SoundSetting::EventSounds, &kde_tool("kreadconfig6"))
            .unwrap_or_default(),
    )
}

/// Default application protection rules from disk.
fn protection_rules() -> defaults::Rules {
    let loaded = read_json(&format!("{}/rexon/protection_rules.json", config_home()));
    match loaded {
        Value::Object(map) => defaults::Rules::defaults().updated(&map),
        _ => defaults::Rules::defaults(),
    }
}

/// Remembered default applications from disk.
fn protected_apps() -> Vec<(String, Option<String>)> {
    defaults::load_protected_apps(&read_json(&format!(
        "{}/rexon/protected_defaults.json",
        config_home()
    )))
}

/// What is set in the system RIGHT NOW: `[(type, .desktop | None), …]` in
/// the same order as in Python (MIME_TYPES, then the terminal).
fn current_defaults() -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for (app_type, mime) in defaults::MIME_TYPES {
        let stdout = read_cmd(&[
            "xdg-mime".to_string(),
            "query".to_string(),
            "default".to_string(),
            (*mime).to_string(),
        ]);
        let value = stdout.lines().next().unwrap_or("").trim().to_string();
        out.push((
            (*app_type).to_string(),
            if value.is_empty() { None } else { Some(value) },
        ));
    }
    out.push(("terminal".to_string(), current_terminal()));
    out
}

/// The current default terminal.
///
/// The laziness of the sources is repeated after the original verbatim: a
/// non-empty first line of `<desktop>-xdg-terminals.list` or
/// `xdg-terminals.list` is conclusive, and neither `kdeglobals` nor
/// `gsettings` are read after it. The decision is made by the core.
fn current_terminal() -> Option<String> {
    let cfg = config_home();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    // `xdg-terminal-exec` reads its own desktop's list before the shared one —
    // its answer is the terminal that will open.
    let xdg_list = defaults::effective_terminals_list(
        rexon_core::appdefaults::terminals_list_filename(&desktop)
            .and_then(|name| read_file(&format!("{cfg}/{name}"))),
        read_file(&format!("{cfg}/xdg-terminals.list")),
    );

    if let Some(content) = &xdg_list {
        if content.lines().next().is_some_and(|l| !l.trim().is_empty()) {
            return defaults::get_current_terminal(Some(content), &desktop, None, None);
        }
    }

    let de = desktop.to_lowercase();
    let mut kdeglobals = None;
    let mut gsettings_exec = None;
    if de.contains("kde") || de.contains("plasma") {
        kdeglobals = read_file(&format!("{cfg}/kdeglobals"));
    } else if let Some(schema) = defaults::terminal_exec_schema(&desktop) {
        gsettings_exec = gsettings_get(schema, "exec");
    }

    defaults::get_current_terminal(
        xdg_list.as_deref(),
        &desktop,
        kdeglobals.as_deref(),
        gsettings_exec.as_deref(),
    )
    .or_else(installed_terminal)
}

/// `gsettings get <schema> <key>`; `None` means the command did not work.
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    if !has("gsettings") {
        return None;
    }
    let out = read_cmd(&[
        "gsettings".to_string(),
        "get".to_string(),
        schema.to_string(),
        key.to_string(),
    ]);
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// A terminal that is simply installed in the system is the last source
/// when the settings are silent (a typical fresh system with a tiling WM).
/// The choice is made by the core; here there are only the observations
/// "the binary is in PATH" and ".desktop on disk".
fn installed_terminal() -> Option<String> {
    let mut available: Vec<String> = Vec::new();
    let mut desktops: Vec<String> = Vec::new();
    for (binary, desktop_ids) in defaults::TERMINAL_CANDIDATES {
        if !has(binary) {
            continue;
        }
        available.push((*binary).to_string());
        for id in *desktop_ids {
            if desktop_exists(id) {
                desktops.push((*id).to_string());
            }
        }
    }
    defaults::pick_terminal(&available, &desktops)
}

// ============================================================================
// Actions
// ============================================================================

impl qobject::SettingsActions {
    // ── Interface language ───────────────────────────────────────────────

    fn set_language(&self, code: &QString) -> QString {
        let code = code.to_string();
        // An unknown value is not written: garbage in the setting would silently
        // give English.
        if !qs::language_choice_valid(&code) {
            return QString::default();
        }
        if let Err(text) = save_setting(rexon_core::i18n::SETTINGS_KEY, Value::from(code.as_str())) {
            rexon_core::log_warn!("lang", "выбор «{code}» не сохранён: {text}");
            return QString::from(&text);
        }
        rexon_core::log_info!(
            "lang",
            "выбран язык «{code}» (даст {}, сейчас {}; перезапуск {})",
            rexon_core::i18n::language_for_setting(Some(&code)),
            rexon_core::i18n::current_language(),
            if rexon_core::i18n::restart_needed(&code) { "нужен" } else { "не нужен" }
        );
        QString::from(&qs::language_saved_status(&code))
    }

    fn admin_session(&self) -> bool {
        // `REXON_SHOT_ADMIN_SESSION=1` — only for tab snapshots on a separate
        // screen: the strip is visible without a live helper. It grants no rights
        // — "Revoke now" then closes nothing.
        rexon_core::privilege::run::has_session()
            || std::env::var_os("REXON_SHOT_ADMIN_SESSION").is_some_and(|v| v == "1")
    }

    fn revoke_admin(&self) -> QString {
        rexon_core::privilege::run::shutdown();
        rexon_core::log_info!("privilege", "«Запретить сейчас» в «Настройках»: сеанс прав закрыт");
        QString::from(&rexon_core::privilege::revoke::Reason::RevokedByUser.message())
    }

    fn restart_rexon(&self) -> QString {
        match spawn_restart() {
            Ok(()) => {
                crate::tray::request_quit("перезапуск из «Настроек»");
                QString::default()
            }
            Err(reason) => {
                rexon_core::log_error!("restart", "отменён, Rexon остаётся открытым: {reason}");
                QString::from(&qs::language_restart_failed(&reason))
            }
        }
    }

    // ── Skin, tutorial, autostart ────────────────────────────────────────

    fn set_skin(&self, skin_id: &QString) -> QString {
        let skin_id = skin_id.to_string();
        // An unknown name is not written at all: saved garbage would fall back to
        // the standard skin on every read, and the setting would look "not
        // remembered".
        if skin_id.is_empty() || !ui_base::is_valid_skin(&skin_id) {
            return QString::default();
        }
        let saved = core_settings::SettingsManager::new()
            .and_then(|mut m| m.set("ui_skin", Value::from(skin_id.as_str())));
        if let Err(e) = saved {
            return QString::from(&t1("status.error", &e.to_string()));
        }
        // The push goes AFTER the write: the pages read the active skin from the
        // settings, and a broadcast before the write would repaint them in the
        // previous colour.
        crate::skin::dispatch();
        let label = ui_base::available_skins()
            .into_iter()
            .find(|(id, _)| *id == skin_id)
            .map(|(_, label)| label.to_string())
            .unwrap_or_else(|| skin_id.clone());
        QString::from(&t1("skin.applied", &label))
    }

    fn set_onboarding(&self, enabled: bool) -> QString {
        match save_setting(onboarding::KEY_ENABLED, Value::from(enabled)) {
            Ok(()) => QString::from(&qs::onboarding_toggle_status(enabled)),
            Err(text) => QString::from(&text),
        }
    }

    fn replay_onboarding(mut self: Pin<&mut Self>) -> QString {
        // The tutorial lives as an overlay of the main window; the shared registry
        // passes the request to it. If there is no window with an overlay (the tab
        // is open as a single window), the user is told where to go.
        if !crate::onboarding_overlay::request_replay() {
            return QString::from(&t("ob.no_main_window"));
        }
        // The replay enables the tutorial in the settings
        // (`onboarding::session_replay`), but the overlay does it — with a signal
        // from the queue, already AFTER the page re-read the snapshot on this
        // button's answer. The "Show tutorial" checkbox would stay as before. A
        // push into the same queue gets in line after the request to the overlay
        // and re-reads the snapshot after its write.
        let queued = self
            .as_mut()
            .qt_thread()
            .queue(|mut object| object.as_mut().refreshed());
        if let Err(e) = queued {
            if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                rexon_core::log_warn!("settings", "перечитать настройки после повтора обучения не вышло: {e:?}");
            }
        }
        QString::from(&t("ob.started"))
    }

    // ── "Install Rexon into the system…" ─────────────────────────────────
    //
    // ⭐ Wehrwolfmann's decision of 15.09.2026: the button stands next to "Take
    // the tutorial again". The "whether to show" flag, the labels and the
    // installation itself are in the core; here there is only the door to the
    // wizard (`first_run_wizard`), just as "Take the tutorial again" has a door
    // to the tutorial overlay.

    fn install_offered(&self) -> bool {
        crate::first_run_wizard::offer_ignoring_decline().is_offer()
    }

    fn install_texts(&self) -> QMap<QMapPair_QString_QVariant> {
        let lang = ui_lang();
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        let mut put = |key: &str, value: String| {
            out.insert(QString::from(key), QVariant::from(&QString::from(&value)));
        };
        put("label", inst::settings_install_button(lang));
        put("hint", inst::settings_install_hint(lang));
        out
    }

    fn install_rexon(&self) -> QString {
        let lang = ui_lang();
        // The button itself is the answer "install after all": the previous
        // refusal is removed with the same settings key that the core's decision
        // reads.
        if let Err(text) = save_setting(inst::KEY_FIRST_RUN_DECLINED, Value::from(false)) {
            return QString::from(&text);
        }
        // The wizard lives in the main window; the page has no direct reference to
        // it.
        if !crate::first_run_wizard::request_show() {
            return QString::from(&inst::settings_install_no_window(lang));
        }
        QString::from(&inst::settings_install_started(lang))
    }

    fn set_autostart(&self, enabled: bool) -> QString {
        match crate::tray::set_autostart(enabled) {
            Ok(()) => QString::from(&t(if enabled {
                "tray.autostart_on"
            } else {
                "tray.autostart_off"
            })),
            // ⭐ Fix: the reason is named to the user, not only to the log.
            Err(e) => QString::from(&format!(
                "{} {}",
                t("tray.autostart_failed"),
                t1("status.error", &e)
            )),
        }
    }

    fn set_theme_deps_auto(&self, enabled: bool) -> QString {
        match save_setting(rexon_core::theme_watch::AUTO_HOME_KEY, Value::from(enabled)) {
            Ok(()) => QString::from(&t(if enabled { "tray.theme_deps_on" } else { "tray.theme_deps_off" })),
            Err(text) => QString::from(&text),
        }
    }

    // ── File manager ─────────────────────────────────────────────────────

    fn set_file_manager(&self, fm_id: &QString) -> QString {
        match save_setting("file_manager", Value::from(fm_id.to_string())) {
            // Empty — there is nothing to say: the choice is only written, a separate
            // button applies it.
            Ok(()) => QString::default(),
            Err(text) => QString::from(&text),
        }
    }

    fn apply_file_manager(&self) -> QString {
        let de = current_de();
        let setting = core_settings::SettingsManager::new()
            .ok()
            .and_then(|m| m.get("file_manager").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "auto".to_string());
        let fm_id = resolve_file_manager(&de, &setting);
        QString::from(&qs::fm_apply_status(set_file_manager_default(&fm_id)))
    }

    fn restore_native(&self) -> QString {
        let de = current_de();
        let de_display = screenshots::display_name(&de);
        let apps = appdefaults::de_apps_info(&de, &desktop_exists);

        let mut protected = protected_apps();
        let mut restored = 0usize;
        let mut missing: Vec<String> = Vec::new();

        for (app_type, desktop) in &apps {
            let Some(desktop) = desktop else {
                missing.push(defaults::app_type_label(app_type).to_string());
                continue;
            };
            if !apply_default(app_type, desktop) {
                missing.push(defaults::app_type_label(app_type).to_string());
                continue;
            }
            remember(&mut protected, app_type, desktop);
            restored += 1;
            if app_type == "file_manager" {
                // mimeapps is not enough: "show in folder" goes through D-Bus
                // FileManager1 → a user override is needed.
                set_file_manager_default(desktop);
            }
        }
        // The outcome of writing the snapshot is not shown here: what matters to
        // the user is the report on RESTORING, not on the memory for it, and a
        // second error line would cover the first.
        let _ = save_protected(&protected);

        let message = qs::restore_native_message(restored, &missing, &de_display);
        QString::from(&qs::restore_native_status(&message, !missing.is_empty()))
    }

    // ── Desktop completeness ─────────────────────────────────────────────
    //
    // A port of the Python tab's `checkCompleteness`. The executor is the core
    // (`system_de_exec::completeness_survey_live` / `complete_de_live`), the
    // same that installs desktops; the window is the shared
    // `DeOperationDialog.qml`. The order: a check without a password → what is
    // missing in words in the question window → after "Yes" the installation
    // with progress → the outcome in the section and as a pop-up line. If
    // everything is in place there is no window at all, only the outcome (as
    // in Python).

    fn check_completeness(mut self: Pin<&mut Self>) -> QString {
        let flag = self.as_ref().get_ref().rust().completeness_busy.clone();
        if flag.swap(true, Ordering::SeqCst) {
            return QString::from(&t("compl.busy"));
        }
        self.as_ref().get_ref().rust().session().completeness_note = Some(t("compl.running"));
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // A panic of the check must not leave the button disabled forever.
            let survey = std::panic::catch_unwind(dx::completeness_survey_live).ok();
            deliver(&handle, move |object| object.survey_done(survey));
        });
        // An empty answer: the page will re-read the snapshot and show
        // "Checking…".
        QString::default()
    }

    fn survey_done(mut self: Pin<&mut Self>, survey: Option<dx::CompletenessSurvey>) {
        let flag = self.as_ref().get_ref().rust().completeness_busy.clone();
        let Some(survey) = survey else {
            let msg = t1("compl.failed_log", &completeness_internal_error());
            self.as_ref().get_ref().rust().session().completeness_note = Some(msg.clone());
            flag.store(false, Ordering::SeqCst);
            self.as_mut().refreshed();
            self.as_mut().status(QString::from(&msg));
            return;
        };
        for line in &survey.logs {
            rexon_core::log_info!("completeness", "{line}");
        }
        let msg = qs::compliance_summary(&survey.results);
        self.as_ref().get_ref().rust().session().completeness_note = Some(msg.clone());
        if survey.fixes.is_empty() {
            flag.store(false, Ordering::SeqCst);
            self.as_mut().refreshed();
            self.as_mut().status(QString::from(&qs::compliance_status(
                &msg,
                !survey.results.is_empty(),
            )));
            return;
        }
        // Something is missing: the button stays disabled while the question is
        // open.
        let labels: Vec<String> = survey.fixes.iter().map(|(_, label)| label.clone()).collect();
        self.as_mut().rust_mut().pending = Some(PendingOp::Completeness(survey.fixes));
        self.as_mut().set_op(OpView::confirm(
            &dx::completeness_confirm_title(),
            &dx::completeness_confirm_text(&labels),
        ));
        self.as_mut().refreshed();
        self.as_mut().status(QString::from(&dx::awaiting_answer(
            &dx::completeness_operation_title(),
        )));
    }

    fn operation(&self) -> QMap<QMapPair_QString_QVariant> {
        self.rust().op.to_map()
    }

    fn set_op(mut self: Pin<&mut Self>, op: OpView) {
        self.as_mut().rust_mut().op = op;
        self.as_mut().operation_changed();
    }

    fn confirm_operation(mut self: Pin<&mut Self>) {
        if self.rust().op.phase == "running" {
            return;
        }
        let Some(pending) = self.as_mut().rust_mut().pending.take() else {
            return;
        };
        let token = dx::CancelToken::new();
        self.as_mut().rust_mut().cancel = Some(token.clone());
        let handle = self.as_mut().qt_thread();
        match pending {
            PendingOp::WaylandDefault(session, names) => {
                use rexon_core::system_plan as sp;
                self.as_mut().set_op(OpView::running(&sp::wayland_default_button(session)));
                std::thread::spawn(move || {
                    let done = std::panic::catch_unwind(|| sp::make_wayland_default_live(session))
                        .unwrap_or_else(|_| Err(sp::wayland_default_failed()));
                    deliver(&handle, move |object| object.finish_wayland_default(session, names, done));
                });
            }
            PendingOp::Completeness(fixes) => {
                self.as_mut()
                    .set_op(OpView::running(&dx::completeness_operation_title()));
                std::thread::spawn(move || {
                    let sink: Arc<dyn dx::DeOpSink> = Arc::new(SettingsSink {
                        handle: handle.clone(),
                    });
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dx::complete_de_live(sink, token, &fixes)
                    }))
                    .ok();
                    // The installation may have brought one more notification daemon.
                    rexon_core::notifications_slot::sync_live();
                    deliver(&handle, move |object| object.finish_completeness(done));
                });
            }
            PendingOp::RestoreStock(forks) => {
                self.as_mut().set_op(OpView::running(&aeroshell::restore_run_title()));
                std::thread::spawn(move || {
                    let sink: Arc<dyn dx::DeOpSink> = Arc::new(SettingsSink {
                        handle: handle.clone(),
                    });
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dx::restore_stock_plasma_live(sink, token, &forks)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_restore_stock(done));
                });
            }
            PendingOp::RollbackAsk(offer) => {
                // Foolproofing: before the rollback — what exactly will change.
                use rexon_core::update_guard as ug;
                self.as_mut().rust_mut().cancel = None;
                let view = OpView {
                    yes_text: ug::rollback_confirm_button(),
                    no_text: dx::dialog_cancel(),
                    danger: true,
                    ..OpView::confirm(
                        &ug::rollback_confirm_title(&offer),
                        &rexon_core::ui_base::escape_dialog_text(&ug::rollback_confirm_text(&offer)),
                    )
                };
                self.as_mut().rust_mut().pending = Some(PendingOp::Rollback(offer));
                self.as_mut().set_op(view);
            }
            PendingOp::Rollback(offer) => {
                use rexon_core::update_guard as ug;
                self.as_mut().set_op(OpView::running(&ug::rollback_title(&offer)));
                std::thread::spawn(move || {
                    let lines = handle.clone();
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let back = ug::rollback_live(&offer, &mut |l: &str| {
                            let l = l.to_string();
                            deliver(&lines, move |object| {
                                object.apply_op_progress(dp::STAGE_RAW.to_string(), l, None)
                            });
                        });
                        // A repeated check — after the rollback: the outcome in words, what
                        // remains — as a window when the outcome is closed.
                        let rest = back.is_ok().then(ug::advise_live);
                        (back, rest)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_rollback(offer, done));
                });
            }
            PendingOp::RemoveOrphans(names) => {
                use rexon_core::update_guard as ug;
                self.as_mut().set_op(OpView::running(&ug::orphan_remove_title(&names)));
                std::thread::spawn(move || {
                    let lines = handle.clone();
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let removed = ug::remove_orphans_live(&names, &mut |l: &str| {
                            let l = l.to_string();
                            deliver(&lines, move |object| {
                                object.apply_op_progress(dp::STAGE_RAW.to_string(), l, None)
                            });
                        });
                        // A repeated check — only after the removal: what remains will be shown
                        // when the outcome window is closed.
                        let rest = removed.is_ok().then(ug::advise_live);
                        (removed, rest)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_remove_orphans(names, done));
                });
            }
            PendingOp::Aeroshell(packages) => {
                self.as_mut().set_op(OpView::running(&dp::aeroshell_run_title()));
                std::thread::spawn(move || {
                    let sink: Arc<dyn dx::DeOpSink> = Arc::new(SettingsSink {
                        handle: handle.clone(),
                    });
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dx::rebuild_aeroshell_live(sink, token, &packages)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_aeroshell(done));
                });
            }
        }
    }

    fn finish_completeness(mut self: Pin<&mut Self>, done: Option<dx::CompletenessOutcome>) {
        let panicked = done.is_none();
        let done = done.unwrap_or_else(|| dx::CompletenessOutcome {
            outcome: dx::DeOpOutcome {
                ok: false,
                message: t1("compl.failed_log", &completeness_internal_error()),
                cancelled: false,
                restart_needed: false,
            },
            results: Vec::new(),
            logs: Vec::new(),
        });
        for line in &done.logs {
            rexon_core::log_info!("completeness", "{line}");
        }
        let mut op = self.rust().op.clone();
        if let Some(line) = op.apply_done(&done.outcome, String::new()) {
            self.as_mut().operation_log(QString::from(&line));
        }
        if panicked {
            // Nobody announced an error along the way — we announce it here.
            self.as_mut().operation_log(QString::from(&format!(
                "{}{}",
                rexon_core::de_progress::PREFIX_ERROR,
                done.outcome.message
            )));
        }
        let note = completeness_note_after(&done.outcome, &done.results);
        let footer = dx::operation_footer(&dx::completeness_operation_title(), &op.status);
        self.as_mut().rust_mut().cancel = None;
        self.as_ref().get_ref().rust().session().completeness_note = Some(note);
        self.as_ref()
            .get_ref()
            .rust()
            .completeness_busy
            .store(false, Ordering::SeqCst);
        self.as_mut().set_op(op);
        self.as_mut().refreshed();
        self.as_mut().status(QString::from(&footer));
    }

    fn apply_op_progress(
        mut self: Pin<&mut Self>,
        stage: String,
        message: String,
        fraction: Option<f64>,
    ) {
        let mut op = self.rust().op.clone();
        let Some(log_line) = op.apply_progress(&stage, &message, fraction) else {
            return;
        };
        if let Some(line) = log_line {
            self.as_mut().operation_log(QString::from(&line));
        }
        // As in "Desktops": a notification only on a visible change.
        let old = &self.rust().op;
        if op.bar != old.bar || op.status != old.status || op.current != old.current {
            self.set_op(op);
        }
    }

    fn apply_op_cancellable(self: Pin<&mut Self>, allowed: bool) {
        let mut op = self.rust().op.clone();
        if op.phase != "running" {
            return;
        }
        op.cancellable = allowed;
        self.set_op(op);
    }

    fn dismiss_operation(mut self: Pin<&mut Self>) {
        let phase = self.rust().op.phase.clone();
        if phase == "running" {
            return;
        }
        if phase == "confirm" {
            match self.as_mut().rust_mut().pending.take() {
                // "Not now": the login screen does not change; the window will offer to
                // remove the orphan again by the button or at the next check.
                Some(PendingOp::WaylandDefault(..)) => {}
                // "No": we do not start the installation, the button is available again,
                // and the section keeps the check outcome ("left to install").
                Some(PendingOp::Completeness(_)) => {
                    self.as_ref()
                        .get_ref()
                        .rust()
                        .completeness_busy
                        .store(false, Ordering::SeqCst);
                    self.as_mut().refreshed();
                    self.as_mut().status(QString::default());
                }
                // "Not now": the rebuild will never start by itself, we will ask again at
                // the next launch or by the button.
                Some(PendingOp::Aeroshell(_) | PendingOp::RestoreStock(_)) => {
                    AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                    self.as_mut().status(QString::default());
                }
                // "Not now": again — at the next launch, if the package is still in place
                // and the state has changed, or by the button.
                // The same with the rollback: nothing changes either on "Not now" or on
                // "Cancel".
                Some(PendingOp::RemoveOrphans(_) | PendingOp::RollbackAsk(_) | PendingOp::Rollback(_)) | None => {}
            }
        }
        self.as_mut().set_op(OpView::default());
        // The orphan removal outcome was closed, and the check found something
        // else — say so.
        if let Some(rest) = self.as_mut().rust_mut().update_guard_rest.take() {
            self.as_mut().show_update_guard(rest);
        }
    }

    fn cancel_operation(mut self: Pin<&mut Self>) {
        if self.rust().op.phase != "running" || !self.rust().op.cancellable {
            return;
        }
        if let Some(token) = &self.rust().cancel {
            token.cancel();
        }
        // The button is disabled at once; the executor will stop at a safe point.
        let mut op = self.rust().op.clone();
        op.cancellable = false;
        self.as_mut().set_op(op);
    }

    // ── Wallpaper ────────────────────────────────────────────────────────

    fn stop_wallpaper(&self) -> QString {
        let state = self.rust();
        // Applying is in progress — the executor is busy with a background thread.
        // Waiting for its lock in the window's thread would freeze the tab for the
        // time of the shell commands; it is more honest to say that the wallpaper
        // is already being applied.
        if state.wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        // What to remove, what to restore and in what order on KDE, GNOME, Xfce
        // and others is decided by the core's executor (`WallpaperExec::stop`),
        // here there is only the call and the ready answer line.
        //
        // Warning: Python then called `cleanup_orphans` — picking up wallpaper
        // processes of the PREVIOUS launch by a PID file and by name. It is not
        // ported here: the PID file is shared by all Rexon windows, and stopping in
        // this window would remove the wallpaper set by the classic window. The
        // button is enabled only for this window's OWN live wallpaper (`running`)
        // — from any of its pages: the executor is one per process, and a video
        // from "Steam" is removed here too. If the executor is busy applying from
        // another page, waiting for its lock in the window's thread is not allowed,
        // and the answer is the same "already applying".
        match stop_wallpaper_in(&live_slot()) {
            None => QString::from(&t("wp.busy")),
            Some(res) => QString::from(&qs::wallpaper_stop_status(res.ok, &res.message)),
        }
    }

    fn set_wallpaper_audio(
        &self,
        enabled: bool,
        volume: i32,
        mute_other: bool,
        hide_covered: bool,
        pause_on_battery: bool,
        pause_in_game: bool,
    ) -> QString {
        // What to write and with which keys is decided by the core; the wallpaper
        // is not restarted, the host watches the settings file itself.
        let patch = rexon_core::wallpaper_audio::AudioSettings {
            enabled,
            volume_pct: rexon_core::wallpaper_audio::clamp_volume_pct(volume as i64),
            mute_when_other_audio: mute_other,
            fade_ms: rexon_core::wallpaper_audio::DEFAULT_FADE_MS,
            actions: rexon_core::wallpaper_audio::actions_for_choices(
                rexon_core::wallpaper_audio::SimpleChoices {
                    hide_when_covered: hide_covered,
                    pause_on_battery,
                    pause_in_game,
                },
            ),
        }
        .to_map();

        let Ok(mut manager) = core_settings::SettingsManager::new() else {
            return QString::from(&t1("status.error", &rexon_core::i18n::tr("settings are unavailable")));
        };
        for (key, value) in patch {
            // ⭐ Fix: Python reported success without looking at the write.
            if let Err(e) = manager.set(&key, value) {
                return QString::from(&t1("status.error", &e.to_string()));
            }
        }
        QString::from(&qs::wallpaper_audio_live_status(enabled))
    }

    fn pick_wallpaper(mut self: Pin<&mut Self>, kind: &QString) -> QString {
        let kind = kind.to_string();
        // The core's executor carries all wallpaper formats itself
        // (`carries_format`): picture, video and Flash are applied without the
        // classic window.
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&qs::wallpaper_dialog_title(&kind)),
            &QString::from(&qs::wallpaper_dialog_filter(&kind)),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            // The user changed their mind — there is nothing to say.
            return QString::default();
        }

        // The format goes by the extension of the CHOSEN file, not by the pressed
        // button: the choice window has other groups too, and an .mp4 from under
        // "Picture…" would have been applied as a picture.
        let fmt = qs::detect_wallpaper_format(&path);
        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        self.as_mut().start_wallpaper_apply(path, fmt.to_string());
        // ⭐ Fix: Python substituted the internal format word ("Applying:
        // sunset.png (static)…"); the customer gets the name from the core.
        QString::from(&qs::wallpaper_applying_status(
            &basename,
            &wallpaper_engine::format_label(fmt),
        ))
    }

    fn apply_youtube(
        mut self: Pin<&mut Self>,
        url: &QString,
        quality: &QString,
        mode: &QString,
        audio: bool,
    ) -> QString {
        let url = url.to_string().trim().to_string();
        if url.is_empty() {
            return QString::from(&t("yt.no_url"));
        }
        if !has("yt-dlp") {
            return QString::from(&t("yt.no_ytdlp"));
        }
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        let quality = quality.to_string();
        let mode = mode.to_string();
        // Which mode it will actually be (on GNOME a stream becomes a download) is
        // decided by the core by the executor's environment.
        let effective = try_with_slot(&live_slot(), |exec| {
            wallpaper_engine::youtube_effective_mode(exec.de(), exec.session_type(), &mode)
                .to_string()
        })
        .unwrap_or_else(|| mode.clone());
        let (job_url, job_quality, job_mode) = (url, quality.clone(), mode);
        // The stream, the download and the setting are done by the core's
        // executor; the load supervision will be brought up by the shared
        // background door.
        self.as_mut().run_wallpaper_job(move |exec| {
            let res = exec.apply_youtube(&job_url, &job_quality, &job_mode, audio);
            qs::done_or_warned(res.ok, &res.message, res.warning.as_deref(), qs::youtube_done)
        });
        QString::from(&qs::youtube_starting_status(&quality, &effective, audio))
    }

    fn set_youtube_audio(&self, on: bool) -> QString {
        self.rust().session().youtube_audio = Some(on);
        QString::from(&qs::youtube_audio_status(on))
    }

    fn ytdlp(mut self: Pin<&mut Self>, action: &QString) -> QString {
        let action = action.to_string();
        let (_button, status) = qs::ytdlp_working(&action);
        let flag = self.as_ref().get_ref().rust().ytdlp_busy.clone();
        // Already in progress — we answer with the same move rather than silence
        // (Python stayed silent: the user pressed again and entered the password
        // for a repeated installation).
        if flag.swap(true, Ordering::SeqCst) {
            return QString::from(&status);
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // The flag is cleared on a panic of the work too.
            let busy = BusyGuard(flag);
            // Installation — by the distribution's package manager (polkit asks for
            // the password), then pip; an update — by yt-dlp itself, then pip. The
            // commands and the order are decided by the core.
            let (ok, message) = if action == "update" {
                youtube_wallpaper::update_ytdlp_live()
            } else {
                youtube_wallpaper::install_ytdlp_live()
            };
            drop(busy);
            let done = qs::ytdlp_done(ok, &message);
            let queued = handle.queue(move |mut object| {
                // Ask the system again: after installation the button must become
                // "Update", and the label must show the version.
                object.as_ref().get_ref().rust().session().ytdlp_version = None;
                object.as_mut().refreshed();
                object.as_mut().status(QString::from(&done));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("settings", "итог установки yt-dlp не доставлен: {e:?}");
                }
            }
        });
        QString::from(&status)
    }

    fn apply_web(mut self: Pin<&mut Self>, url: &QString) -> QString {
        let url = url.to_string().trim().to_string();
        if url.is_empty() {
            return QString::from(&t("web.no_url"));
        }
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        // A local path turns into `file://` — decided by the core (as in Python).
        let exists = url.starts_with('/') && std::path::Path::new(&url).exists();
        let target = qs::web_wallpaper_url(&url, exists);
        let job_target = target.clone();
        // The web wallpaper host is brought up by the core's executor (`apply` →
        // `web_plan`).
        self.as_mut().run_wallpaper_job(move |exec| {
            let res = exec.apply(&job_target, "web", "");
            qs::done_or_warned(res.ok, &res.message, res.warning.as_deref(), qs::web_done)
        });
        QString::from(&qs::web_loading_status(&target))
    }

    // ── Wallpaper library ────────────────────────────────────────────────

    fn preview_library(&self, path: &QString) -> QString {
        let path = path.to_string();
        if !path.is_empty() && std::path::Path::new(&path).is_file() {
            // The same shim "Extensions" opens a directory with: one door into
            // `QDesktopServices` for the whole crate.
            crate::extensions_actions::qobject::open_folder_url(&QString::from(&path));
        }
        QString::default()
    }

    fn apply_library(mut self: Pin<&mut Self>, path: &QString, fmt: &QString) -> QString {
        let path = path.to_string();
        let fmt = fmt.to_string();
        if path.is_empty() || !std::path::Path::new(&path).exists() {
            return QString::from(&t("lib.no_file"));
        }
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        // A tile of any format goes to the core's executor: pictures, video and
        // Flash (under load supervision), GIF and web are applied; scenes,
        // application wallpapers and an unknown package format are rejected by the
        // executor with the same words as the classic window.
        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.as_mut().start_wallpaper_apply(path, fmt.clone());
        // ⭐ Fix: Python substituted the internal format word ("(static)").
        QString::from(&qs::library_applying_status(
            &basename,
            &wallpaper_engine::format_label(&fmt),
        ))
    }

    // ── Importing Windows themes ─────────────────────────────────────────

    fn pick_import(mut self: Pin<&mut Self>) -> QString {
        if self.as_ref().get_ref().rust().import_busy.load(Ordering::SeqCst) {
            return QString::from(&t("imp.busy"));
        }
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&t("imp.dialog_title")),
            &QString::from(&importers::import_dialog_filter()),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            // The user changed their mind — there is nothing to say.
            return QString::default();
        }

        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        {
            let state = self.as_ref().get_ref().rust();
            state.session().import_note = t1("imp.working_file", &basename);
            state.import_busy.store(true, Ordering::SeqCst);
        }

        let flag = self.as_ref().get_ref().rust().import_busy.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Unpacking a CAB, converting pictures and copying theme files take
            // seconds and minutes. The work is done by the same executor with which
            // the "Extensions" tab installs .rxn: no second copy of the import rules
            // appears.
            let missing = !std::path::Path::new(&path).exists();
            let outcome = ext_exec::install_from_file(&path);
            flag.store(false, Ordering::SeqCst);

            let (note, status) = match outcome {
                Ok(done) => qs::import_success_texts(&done.name, &done.notes),
                Err(text) => {
                    let reason = qs::import_failure_reason(missing, &text);
                    qs::import_failure_texts(&reason)
                }
            };
            let queued = handle.queue(move |mut object| {
                {
                    let rust = object.as_ref().get_ref().rust();
                    let mut session = rust.session();
                    session.import_note = note;
                    // The import may have installed a wallpaper package — re-read the library.
                    session.library = None;
                }
                object.as_mut().refreshed();
                object.as_mut().status(QString::from(&status));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    // Silence is not allowed: the import outcome would be lost, and the
                    // section would stay with the "Importing the theme…" line forever.
                    rexon_core::log_warn!("settings", "итог импорта темы не доставлен: {e:?}");
                }
            }
        });
        QString::from(&t("imp.working_status"))
    }

    // ── Wallpaper blur ───────────────────────────────────────────────────

    fn set_blur_radius(&self, value: i32) -> QString {
        self.rust().session().blur_radius = Some(value);
        QString::default()
    }

    fn blur(mut self: Pin<&mut Self>, action: &QString) -> QString {
        let blurring = action.to_string() == "blur";
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("blur.busy"));
        }
        let radius = self
            .as_ref()
            .get_ref()
            .rust()
            .session()
            .blur_radius
            .unwrap_or(qs::BLUR_RANGE.3);
        // Blurring the picture and setting the copy are done by the core's
        // executor (without Pillow — the blur is its own); the outcome words are
        // the core's.
        self.as_mut().run_wallpaper_job(move |exec| {
            let ok = if blurring {
                exec.blur_current_wallpaper(radius as i64)
            } else {
                exec.unblur_wallpaper()
            };
            qs::blur_done(ok, &qs::blur_result(if blurring { "blur" } else { "unblur" }, ok))
        });
        QString::from(&if blurring {
            t1("blur.applying", &radius.to_string())
        } else {
            t("blur.removing")
        })
    }

    // ── Ambient sounds ───────────────────────────────────────────────────

    fn add_ambient(&self, volume: i32) -> QString {
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&t("amb.dialog_title")),
            &QString::from(&qs::ambient_file_filter()),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            // The user changed their mind — there is nothing to say.
            return QString::default();
        }
        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let volume = wallpaper_audio::clamp_volume_pct(volume as i64);
        // Launching and checking "did it start sounding" — the core's executor
        // (the same delay as in Python, fractions of a second): here there is only
        // the call and the answer line.
        let started = try_with_slot(&live_slot(), |exec| {
            exec.play_ambient(&qs::ambient_sound_id(&path), &path, &basename, volume, true)
        });
        match started {
            None => QString::from(&t("wp.busy")),
            Some(true) => QString::from(&qs::ambient_start_status(true, &basename, volume as i32)),
            // ⭐ Fix: Python wrote "mpv is needed" on any refusal — even with mpv
            // installed, when the file itself simply does not play.
            Some(false) => QString::from(&qs::ambient_start_failed_status(has("mpv"))),
        }
    }

    fn set_ambient_volume(&self, sound_id: &QString, volume: i32) -> QString {
        let id = sound_id.to_string();
        let volume = wallpaper_audio::clamp_volume_pct(volume as i64);
        // mpv's volume changes only by a restart — the core's executor does it, in
        // the same mode in which the sound was playing. The name is taken BEFORE
        // the restart: a failed restart removes the sound from the list.
        let outcome = try_with_slot(&live_slot(), |exec| {
            let name = exec
                .active_ambient()
                .into_iter()
                .find(|sound| sound.id == id)
                .map(|sound| sound.name);
            let ok = name.is_some() && exec.set_ambient_volume(&id, volume);
            (name, ok)
        });
        match outcome {
            None => QString::from(&t("wp.busy")),
            Some((name, ok)) => QString::from(&qs::ambient_volume_status(
                name.as_deref(),
                volume,
                ok,
                has("mpv"),
            )),
        }
    }

    fn stop_ambient(&self, sound_id: &QString) -> QString {
        let id = sound_id.to_string();
        // The sound is stopped by the core's executor (only its own process, by
        // the handle). The name is taken BEFORE stopping: after it the sound is no
        // longer in the list.
        let name = try_with_slot(&live_slot(), |exec| {
            let name = exec
                .active_ambient()
                .into_iter()
                .find(|sound| sound.id == id)
                .map(|sound| sound.name);
            exec.stop_ambient(&id);
            name
        });
        match name {
            None => QString::from(&t("wp.busy")),
            Some(name) => QString::from(&qs::ambient_stopped_status(name.as_deref())),
        }
    }

    fn stop_all_ambient(&self) -> QString {
        // All our own sounds are stopped by the core's executor — the same
        // processes it brought up itself; other mpv instances in the system are
        // not touched.
        match try_with_slot(&live_slot(), |exec| exec.stop_all_ambient()) {
            None => QString::from(&t("wp.busy")),
            Some(()) => QString::from(&t("amb.stopped_all")),
        }
    }

    // ── Event sounds ─────────────────────────────────────────────────────

    fn set_sound_theme(&self, theme: &QString) -> QString {
        let theme = theme.to_string();
        if theme.is_empty() {
            return QString::default();
        }
        // A name for which there is no theme on disk is not written: see
        // `qt_settings::sound_theme_exists`.
        if !qs::sound_theme_exists(&qs::live_xdg_sound_theme_dirs(), &theme) {
            return QString::from(&qs::sound_theme_missing_message(&theme));
        }
        let ok = apply_sound_setting(SoundSetting::Theme, &theme, |raw| {
            qs::parse_sound_theme(raw) == theme
        });
        QString::from(&qs::sound_theme_status(ok, &theme))
    }

    fn set_event_sounds(&self, enabled: bool) -> QString {
        QString::from(&switch_event_sounds(enabled))
    }

    /// Listen to a sound; if there is nothing to play it with — say so.
    ///
    /// Without a player the button must speak rather than stay silent:
    /// otherwise the user cannot tell "a quiet sound" from "nothing to play it
    /// with".
    fn play_sound(&self, theme: &QString, event_id: &QString) -> QString {
        let theme = theme.to_string();
        // The theme is not set — we play the one the system plays and name it in
        // the answer: an empty name would mean "any sound from any theme".
        let theme = qs::effective_sound_theme(&theme).to_string();
        let event_id = event_id.to_string();
        let dirs = qs::sound_theme_dirs(&data_home());
        let file = qs::find_sound_file(&dirs, &theme, &event_id);

        // Which player suits THIS file is decided by the core: the former
        // "paplay → aplay" tail lied on freedesktop themes (.oga).
        let available: Vec<String> = utils::SOUND_PLAYERS
            .iter()
            .filter(|(binary, _, _)| has(binary))
            .map(|(binary, _, _)| (*binary).to_string())
            .collect();

        if let Some(path) = &file {
            if let Some(argv) = utils::sound_play_argv(&available, path) {
                if spawn_detached(&argv) {
                    return QString::default();
                }
            }
        }
        // The file is not in the theme (or there is nothing to play it with) — a
        // libcanberra event.
        if let Some(argv) = utils::sound_play_event_argv(&available, &event_id) {
            if spawn_detached(&argv) {
                return QString::default();
            }
        }
        let problem = match &file {
            None => qs::msg_no_event_in_theme(&theme),
            Some(path) => utils::msg_no_sound_player(&utils::tool_package_hint(
                utils::sound_player_suggestion(path),
                &pkg_family(),
            )),
        };
        QString::from(&t1("snd.cannot_play", &problem))
    }

    // ── System fonts ─────────────────────────────────────────────────────

    fn pick_font(&self, target: &QString) -> QString {
        let target = target.to_string();
        let (_label, _tip, title) = qs::font_target_texts(&target);
        let de = current_de();
        // A title font that has nowhere to be written in this environment is not
        // written anywhere: without this barrier the plan would take it into
        // someone else's key.
        if !qs::font_target_available_wm(&de, &target, title_wm(&de)) {
            return QString::from(&qs::font_apply_status(false, &title, "", 0));
        }
        // Deepin: we do not open the choice window — we say where to change it
        // (the core).
        if let Some(why) = qs::font_refusal_live(&de) {
            return QString::from(&format!("{title} — {why}"));
        }
        let lxqt_desktop = matches!(
            qs::font_plan_wm(&de, &target, None).primary,
            qs::FontStore::File(qs::FontFile::PcmanfmDesktop)
        );
        // An LXQt desktop without a single settings file — they must not be
        // created from scratch.
        if lxqt_desktop {
            if let Some(why) = qs::pcmanfm_refusal(&qs::PcmanfmPaths::from_env(), &qs::LivePcmanfmBus) {
                return QString::from(&format!("{title} — {why}"));
            }
        }
        // What to show as selected on opening — the environment's current font
        // together with the style.
        let (current_family, current_size, current_face) = read_font_face(&de, &target);

        let mut outcome = 0i32;
        let chosen = crate::extensions_actions::qobject::pick_font(
            &QString::from(&title),
            &QString::from(&current_family),
            current_size,
            current_face.weight,
            current_face.italic,
            &mut outcome,
        )
        .to_string();

        if outcome < 0 {
            return QString::from(&format!("{title} — {}", font_dialog_failed()));
        }
        // The user changed their mind — there is nothing to say (as in the Python
        // original).
        let Some((family, size)) = chosen_font(&chosen) else {
            return QString::default();
        };

        // The weight and slant from the choice window go into all storages where
        // they exist (`apply_font`).
        let face = qs::parse_font_face(&chosen);
        // The LXQt desktop: the outcome says whether the desktop was restarted or
        // the font will apply at its start; a refusal — in the core's words.
        if lxqt_desktop {
            let result = apply_lxqt_desktop_font(&family, size, face);
            self.rust().session().fonts = None;
            let shown = qs::font_face_spec(&family, 0, face);
            return QString::from(&match result {
                Ok(applied) => format!(
                    "{} {}",
                    qs::font_apply_status(true, &title, &shown, size),
                    qs::pcmanfm_applied_note(applied)
                ),
                Err(why) => format!("{} {why}", qs::font_apply_status(false, &title, "", 0)),
            });
        }
        let applied = apply_font(&de, &target, &family, size, face);
        // The font snapshot went stale exactly here — otherwise the list would
        // show the old value until the tab is closed.
        self.rust().session().fonts = None;
        // In the outcome — the family with the style ("Sans Bold"), the size
        // separately. xfdesktop has only the size written: the family comes from
        // the interface font.
        let shown = if qs::font_target_size_only(&de, &target) {
            let (family, _, face) = read_font_face(&de, "system");
            qs::font_face_spec(&family, 0, face)
        } else {
            qs::font_face_spec(&family, 0, face)
        };
        QString::from(&qs::font_apply_status(applied, &title, &shown, size))
    }

    fn set_font_rendering(
        &self,
        antialias: bool,
        exclude: bool,
        exclude_from: i32,
        exclude_to: i32,
        subpixel: &QString,
        hinting: &QString,
    ) -> QString {
        let r = qs::FontRendering {
            antialias,
            exclude: exclude.then_some((exclude_from, exclude_to)),
            subpixel: subpixel.to_string(),
            hinting: hinting.to_string(),
        }
        .normalized();
        let outcome = apply_rendering(&current_de(), &r);
        self.rust().session().rendering = None;
        QString::from(&qs::rendering_status(&outcome))
    }

    fn restore_desktop_font(&self) -> QString {
        let result = qs::pcmanfm_desktop_font_restore(&qs::PcmanfmPaths::from_env(), &qs::LivePcmanfmBus);
        if let Err(e) = &result {
            rexon_core::log_warn!("fonts", "шрифт стола LXQt не возвращён — {e}");
        }
        self.rust().session().fonts = None;
        QString::from(&qs::pcmanfm_restore_status(&result))
    }

    fn change_all_fonts(&self, mode: &QString) -> QString {
        let mode = mode.to_string();
        let de = current_de();
        let (_label, _tip, title) = qs::change_all_texts();
        if let Some(why) = qs::font_refusal_live(&de) {
            return QString::from(&format!("{title} — {why}"));
        }
        let (current_family, current_size, current_face) = read_font_face(&de, "system");

        let mut outcome = 0i32;
        let chosen = crate::extensions_actions::qobject::pick_font(
            &QString::from(&title),
            &QString::from(&current_family),
            current_size,
            current_face.weight,
            current_face.italic,
            &mut outcome,
        )
        .to_string();
        if outcome < 0 {
            return QString::from(&format!("{title} — {}", font_dialog_failed()));
        }
        let Some((family, size)) = chosen_font(&chosen) else {
            return QString::default();
        };

        // Each font keeps its own style: "change all" changes the family and the
        // size, and a bold title stays bold.
        let mut faces = std::collections::HashMap::new();
        let now: Vec<qs::FontNow> = qs::font_targets_for_wm(&de, title_wm(&de))
            .into_iter()
            .map(|target| {
                let (family, size, face) = read_font_face(&de, target);
                faces.insert(target.to_string(), face);
                qs::FontNow { target: target.to_string(), family, size }
            })
            .collect();
        let is_mono = qs::parse_fc_spacing(&read_cmd(&qs::fc_spacing_argv(&family)));
        let (plan, mono_kept) = qs::change_all_fonts(&now, &family, size, &mode, is_mono);
        let mut failed = Vec::new();
        for font in &plan {
            let face = faces.get(&font.target).copied().unwrap_or_default();
            if !apply_font(&de, &font.target, &font.family, font.size, face) {
                failed.push(qs::font_target_texts(&font.target).2);
            }
        }
        self.rust().session().fonts = None;
        let kept = now.iter().find(|n| n.target == "mono").map(|n| n.family.clone());
        QString::from(&qs::change_all_status(
            &failed,
            &family,
            size,
            &mode,
            if mono_kept { kept.as_deref() } else { None },
        ))
    }

    fn set_font_dpi(&self, enabled: bool, dpi: i32) -> QString {
        let (lo, hi) = qs::FONT_DPI_RANGE;
        let value = enabled.then(|| dpi.clamp(lo, hi));
        let ok = apply_font_dpi(&current_de(), value);
        self.rust().session().font_dpi = None;
        QString::from(&qs::font_dpi_status(ok, value))
    }

    // ── Font management ──────────────────────────────────────────────────

    fn font_library(&self, full: bool, filter: &QString) -> QString {
        // A collapsed section needs only the labels — fc-list is not asked.
        let fonts = if full {
            self.rust().session().font_library.get_or_insert_with(read_font_library).clone()
        } else {
            Vec::new()
        };
        let groups = qs::group_font_families(&fonts);
        let user_families = groups.iter().filter(|g| g.has_user()).count();
        let mut texts = Map::new();
        for (key, text) in qs::font_manager_texts() {
            texts.insert(key.to_string(), Value::from(text));
        }
        let filter = qs::FontListFilter::from_json(&filter.to_string());
        // A groups file that does not parse does not hinder the list: the groups
        // are simply not visible, and editing honestly refuses (`fontGroupEdit`).
        let custom = read_font_groups();
        let groups_broken = custom.is_err();
        let custom = custom.unwrap_or_default();
        let families: Vec<Value> = qs::font_group_view_in(&groups, &filter, &custom)
            .iter()
            .map(|g| {
                let styles: Vec<Value> = g
                    .styles
                    .iter()
                    .map(|f| {
                        json!({ "style": f.style, "file": f.file, "user": f.user, "shared": f.shared, "disabled": f.disabled })
                    })
                    .collect();
                json!({
                    "family": g.family,
                    "user": g.has_user(),
                    "shared": g.has_shared(),
                    "packaged": g.has_packaged(),
                    "system": g.has_system(),
                    "disabled": g.all_disabled(),
                    "partly": g.any_disabled() && !g.all_disabled(),
                    "groups": qs::font_groups_of_family(&custom, &g.family),
                    "styles": styles,
                })
            })
            .collect();
        let origins: Vec<Value> = qs::font_origin_choices_with_groups(&custom)
            .into_iter()
            .map(|(id, label, own)| json!({ "id": id, "label": label, "custom": own }))
            .collect();
        let group_list: Vec<Value> = qs::font_origin_choices_with_groups(&custom)
            .into_iter()
            .filter(|(_, _, own)| *own)
            .map(|(_, name, _)| Value::from(name))
            .collect();
        let choices = qs::font_filter_choices(&fonts);
        let pairs = |v: &[(String, String)]| -> Vec<Value> {
            v.iter().map(|(id, label)| json!({ "id": id, "label": label })).collect()
        };
        let (min, max, start) = qs::FONT_SAMPLE_SIZE;
        let system = rexon_core::system::DistroDetector::detect_live();
        let shared_ok = qs::font_shared_supported(system.family_live(), system.is_steamos());
        let scopes: Vec<Value> = qs::font_install_scope_choices(shared_ok)
            .into_iter()
            .map(|(id, label)| json!({ "id": id, "label": label }))
            .collect();
        let json = json!({
            "available": has("fc-list"),
            "scopes": scopes,
            "texts": texts,
            "summary": qs::font_manager_summary(groups.len(), user_families),
            "origins": origins,
            "groups": group_list,
            "groupsBroken": groups_broken,
            "foundries": pairs(&choices.foundries),
            "fileTypes": pairs(&choices.file_types),
            "scripts": pairs(&choices.scripts),
            "sample": {
                "min": min,
                "max": max,
                "start": start,
                "text": font_sample_text(),
                "default": qs::font_sample_default(),
                "maxLength": qs::FONT_SAMPLE_TEXT_MAX,
            },
            "families": families,
        });
        QString::from(&json.to_string())
    }

    fn font_chars(&self, file: &QString, style: &QString) -> QString {
        let out = read_cmd(&qs::fc_query_charset_argv(&file.to_string()));
        let blocks: Vec<Value> = qs::font_char_blocks(&qs::fc_charset_for_style(&out, &style.to_string()))
            .into_iter()
            .map(|b| {
                json!({
                    // "Other symbols" has no start; the id must not be empty: empty means
                    // "no block chosen yet" for the page.
                    "id": b.start.map(|s| format!("{s:04X}")).unwrap_or_else(|| "other".to_string()),
                    "label": format!("{} ({})", b.name, b.chars.len()),
                    "chars": b.chars,
                })
            })
            .collect();
        QString::from(&json!({ "blocks": blocks }).to_string())
    }

    fn set_font_sample_text(&self, text: &QString) -> QString {
        let stored = qs::font_sample_text_to_store(&text.to_string());
        // Not written — the sample is still shown with what was typed: the user
        // sees their text, and a write error is no reason to take it away.
        let _ = save_setting(qs::FONT_SAMPLE_TEXT_KEY, Value::from(stored.as_str()));
        QString::from(&qs::font_sample_text(Some(&stored)))
    }

    fn set_font_enabled(&self, family: &QString, file: &QString, enabled: bool) -> QString {
        let family = family.to_string();
        let file = file.to_string();
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let files = qs::font_toggle_files(&fonts, &family, &file);
        if files.is_empty() {
            self.rust().session().font_library = None;
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::NotFound(family)));
        }
        if let Some(bad) = files.iter().find(|f| !qs::font_glob_safe(f)) {
            let name = std::path::Path::new(bad)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bad.clone());
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::UnsafeName(name)));
        }
        let path = fontconfig_path();
        let existing = read_file(&path);
        let current = existing.as_deref().map(qs::fontconfig_disabled_files).unwrap_or_default();
        let after = qs::font_disabled_after(&current, &files, enabled);
        if !enabled {
            let de = current_de();
            let in_use: Vec<String> = qs::font_targets_for_wm(&de, title_wm(&de))
                .into_iter()
                .map(|target| read_font_face(&de, target).0)
                .collect();
            if let Some(refusal) = qs::font_disable_refusal(&fonts, &after, &in_use) {
                return QString::from(&qs::font_toggle_status(&refusal));
            }
            let mut families: Vec<String> =
                fonts.iter().filter(|f| files.contains(&f.file)).map(|f| f.family.clone()).collect();
            families.sort();
            families.dedup();
            if let Some((title, question)) = qs::font_disable_confirm(&family, &families) {
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
                    return QString::default();
                }
            }
        }
        let ok = match write_disabled_fonts(&path, existing.as_deref(), &after) {
            Ok(ok) => ok,
            Err(_) => return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::Refused(path))),
        };
        self.rust().session().font_library = None;
        let outcome = match (ok, enabled) {
            (false, _) => qs::FontToggleOutcome::Failed(path),
            (true, true) => qs::FontToggleOutcome::Enabled(family),
            (true, false) => qs::FontToggleOutcome::Disabled(family),
        };
        QString::from(&qs::font_toggle_status(&outcome))
    }

    fn font_group_edit(&self, op: &QString, group: &QString, arg: &QString) -> QString {
        let (op, group, arg) = (op.to_string(), group.to_string(), arg.to_string());
        let path = font_groups_path();
        let reply = |outcome: &qs::FontGroupOutcome, origin: String| {
            QString::from(&json!({ "text": qs::font_group_status(outcome), "origin": origin }).to_string())
        };
        // Re-read before editing: the groups may have been changed in KDE.
        let Ok(groups) = read_font_groups() else {
            return reply(&qs::FontGroupOutcome::Refused(path), String::new());
        };
        let edit = match op.as_str() {
            "create" => qs::FontGroupEdit::Create(arg),
            "rename" => qs::FontGroupEdit::Rename { from: group, to: arg },
            "remove" => qs::FontGroupEdit::Remove(group),
            "add" => qs::FontGroupEdit::Add { group, families: vec![arg] },
            "take" => qs::FontGroupEdit::Take { group, families: vec![arg] },
            _ => return QString::default(),
        };
        if let qs::FontGroupEdit::Remove(name) = &edit {
            if groups.iter().any(|g| &g.name == name) {
                let (title, question) = qs::font_group_remove_confirm(name);
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
                    return QString::default();
                }
            }
        }
        let (next, outcome) = qs::font_groups_apply(&groups, &edit);
        if outcome.changed() && write_file_atomic(&path, &qs::font_groups_xml(&next)).is_err() {
            return reply(&qs::FontGroupOutcome::Failed(path), String::new());
        }
        // What to select in the filter: the new or renamed group; after removal —
        // "All". Otherwise the page keeps the previous choice.
        let origin = match &outcome {
            qs::FontGroupOutcome::Created(name) | qs::FontGroupOutcome::Renamed { to: name, .. } => {
                format!("{}{name}", qs::FONT_GROUP_ORIGIN_PREFIX)
            }
            qs::FontGroupOutcome::Removed(_) | qs::FontGroupOutcome::NotFound(_) => "all".to_string(),
            _ => String::new(),
        };
        reply(&outcome, origin)
    }

    fn set_font_group_enabled(&self, group: &QString, enabled: bool) -> QString {
        let name = group.to_string();
        let groups = match read_font_groups() {
            Ok(g) => g,
            Err(_) => return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::Refused(font_groups_path()))),
        };
        let Some(group) = groups.into_iter().find(|g| g.name == name) else {
            return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::NotFound(name)));
        };
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let files = qs::font_group_files(&fonts, &group);
        if files.is_empty() {
            return QString::from(&qs::font_group_empty_status(&name));
        }
        if let Some(bad) = files.iter().find(|f| !qs::font_glob_safe(f)) {
            let file_name = std::path::Path::new(bad)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bad.clone());
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::UnsafeName(file_name)));
        }
        let path = fontconfig_path();
        let existing = read_file(&path);
        let current = existing.as_deref().map(qs::fontconfig_disabled_files).unwrap_or_default();
        let after = qs::font_disabled_after(&current, &files, enabled);
        if !enabled {
            // The same prohibitions as for a single family: a system font and
            // "disable absolutely everything".
            let de = current_de();
            let in_use: Vec<String> = qs::font_targets_for_wm(&de, title_wm(&de))
                .into_iter()
                .map(|target| read_font_face(&de, target).0)
                .collect();
            if let Some(refusal) = qs::font_disable_refusal(&fonts, &after, &in_use) {
                return QString::from(&qs::font_toggle_status(&refusal));
            }
            let mut others: Vec<String> = fonts
                .iter()
                .filter(|f| files.contains(&f.file) && !group.families.contains(&f.family))
                .map(|f| f.family.clone())
                .collect();
            others.sort();
            others.dedup();
            if let Some((title, question)) = qs::font_group_disable_confirm(&name, &others) {
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
                    return QString::default();
                }
            }
        }
        let ok = match write_disabled_fonts(&path, existing.as_deref(), &after) {
            Ok(ok) => ok,
            Err(_) => return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::Refused(path))),
        };
        self.rust().session().font_library = None;
        if !ok {
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::Failed(path)));
        }
        let touched = fonts
            .iter()
            .filter(|f| group.families.contains(&f.family))
            .map(|f| f.family.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        QString::from(&qs::font_group_toggle_status(&name, touched, enabled))
    }

    fn export_font_group(&self, group: &QString) -> QString {
        let name = group.to_string();
        let groups = match read_font_groups() {
            Ok(g) => g,
            Err(_) => return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::Refused(font_groups_path()))),
        };
        let Some(group) = groups.into_iter().find(|g| g.name == name) else {
            return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::NotFound(name)));
        };
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let entries =
            qs::font_group_export_entries(&qs::font_group_files(&fonts, &group), &|p| std::path::Path::new(p).is_file());
        if entries.is_empty() {
            return QString::from(&qs::font_group_empty_status(&name));
        }
        let (title, filter) = qs::font_group_export_dialog();
        let suggested = format!("{}/{}", home().trim_end_matches('/'), qs::font_group_export_name(&name));
        let chosen = crate::extensions_actions::qobject::save_file(
            &QString::from(&title),
            &QString::from(&filter),
            &QString::from(&suggested),
        )
        .to_string();
        if chosen.is_empty() {
            return QString::default();
        }
        let dest = qs::font_group_export_path(&chosen);
        let result = qs::write_font_group_zip(&entries, std::path::Path::new(&dest)).map_err(|e| e.to_string());
        QString::from(&qs::font_group_export_status(&name, &dest, &result))
    }

    fn font_duplicates(&self) -> QString {
        let fonts = qs::parse_fc_list_fonts(&read_cmd(&qs::fc_list_scalable_argv()), &font_user_dirs_checked());
        let real = |file: &str| {
            std::fs::canonicalize(file)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| file.to_string())
        };
        let found = qs::find_font_duplicates(&fonts, &real);
        let groups: Vec<Value> = found
            .iter()
            .map(|d| {
                let copies: Vec<Value> =
                    d.copies.iter().map(|c| json!({ "file": c.file, "user": c.user, "shared": c.shared })).collect();
                json!({ "family": d.family, "style": d.style, "copies": copies })
            })
            .collect();
        let json = json!({
            "summary": qs::font_duplicates_summary(found.len()),
            "groups": groups,
        });
        QString::from(&json.to_string())
    }

    fn install_font_file(mut self: Pin<&mut Self>, scope: &QString) -> QString {
        let for_all = qs::font_install_scope_is_all(&scope.to_string());
        if for_all && SHARED_FONT_BUSY.load(Ordering::SeqCst) {
            return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
        }
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&qs::font_install_dialog_title()),
            &QString::from(&qs::font_install_filter()),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            return QString::default();
        }
        if !for_all {
            let outcome = install_font(&path);
            self.rust().session().font_library = None;
            return QString::from(&qs::font_install_status(&outcome));
        }
        if SHARED_FONT_BUSY.swap(true, Ordering::SeqCst) {
            return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
        }
        // The password window and the helper are not in the window's thread:
        // otherwise it "does not respond" while the user enters the password.
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(|| rexon_core::local_install::install_font_file_shared(&path))
                .unwrap_or_else(|_| qs::FontInstallOutcome::CopyFailed(path.clone()));
            let text = qs::font_install_status(&outcome);
            deliver(&handle, move |object| object.shared_font_done(text, Vec::new()));
        });
        QString::default()
    }

    fn remove_font(mut self: Pin<&mut Self>, family: &QString, file: &QString) -> QString {
        let family = family.to_string();
        let file = file.to_string();
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let dirs = font_user_dirs_checked();
        // Our own files are removed — if the path really leads into the user's
        // font directory (checked again, by the real path: the snapshot may be
        // stale) — and those installed for everyone (the root helper checks them).
        // Package fonts — not at all.
        let chosen = |f: &&qs::InstalledFont| f.family == family && (file.is_empty() || f.file == file);
        let mut files: Vec<String> = fonts
            .iter()
            .filter(|f| f.user && chosen(f))
            .map(|f| f.file.clone())
            .filter(|f| qs::font_file_is_user(&real_parent_path(f), &dirs))
            .collect();
        let mut shared: Vec<String> = fonts
            .iter()
            .filter(|f| f.shared && chosen(f))
            .map(|f| f.file.clone())
            .filter(|f| !files.contains(f))
            .collect();
        files.sort();
        files.dedup();
        shared.sort();
        shared.dedup();
        if files.is_empty() && shared.is_empty() {
            return QString::from(&qs::font_remove_status(&family, 0, 0));
        }
        if !shared.is_empty() && SHARED_FONT_BUSY.load(Ordering::SeqCst) {
            return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
        }
        let mut families: Vec<String> = fonts
            .iter()
            .filter(|f| files.contains(&f.file) || shared.contains(&f.file))
            .map(|f| f.family.clone())
            .collect();
        families.sort();
        families.dedup();
        let (title, mut question) = qs::font_remove_confirm(&family, files.len() + shared.len(), &families);
        if !shared.is_empty() {
            question.push(' ');
            question.push_str(&qs::font_remove_shared_note());
        }
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
            return QString::default();
        }
        if !shared.is_empty() {
            if SHARED_FONT_BUSY.swap(true, Ordering::SeqCst) {
                return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
            }
            let handle = self.as_mut().qt_thread();
            let family = family.clone();
            let shared = shared.clone();
            std::thread::spawn(move || {
                let done = std::panic::catch_unwind(|| rexon_core::local_install::remove_shared_fonts(&shared));
                let text = match done {
                    Ok(Ok(())) => qs::font_remove_status(&family, shared.len(), 0),
                    Ok(Err(why)) => format!("⚠️ {why}"),
                    Err(_) => qs::font_remove_status(&family, 0, shared.len()),
                };
                deliver(&handle, move |object| object.shared_font_done(text, shared));
            });
            if files.is_empty() {
                return QString::default();
            }
        }
        let mut removed = 0;
        let mut failed = 0;
        let mut touched_dirs: Vec<String> = Vec::new();
        for f in &files {
            match std::fs::remove_file(f) {
                Ok(()) => {
                    removed += 1;
                    if let Some(dir) = std::path::Path::new(f).parent() {
                        let dir = dir.to_string_lossy().into_owned();
                        if !touched_dirs.contains(&dir) {
                            touched_dirs.push(dir);
                        }
                    }
                }
                Err(_) => failed += 1,
            }
        }
        for dir in &touched_dirs {
            let _ = write_cmd(&qs::fc_cache_argv(dir));
        }
        forget_removed_disabled(&files);
        self.rust().session().font_library = None;
        QString::from(&qs::font_remove_status(&family, removed, failed))
    }

    /// The outcome of installing or removing a font "for everyone" (from a
    /// background thread): `removed` — which files were being removed (remove
    /// them from the disabled ones too).
    fn shared_font_done(mut self: Pin<&mut Self>, text: String, removed: Vec<String>) {
        SHARED_FONT_BUSY.store(false, Ordering::SeqCst);
        forget_removed_disabled(&removed);
        self.rust().session().font_library = None;
        self.as_mut().refreshed();
        self.as_mut().status(QString::from(&text));
    }

    // ── Default application protection ───────────────────────────────────

    fn set_protection(&self, key: &QString, enabled: bool) -> QString {
        let key = key.to_string();
        let mut rules = protection_rules().0;
        match rules.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = enabled,
            None => rules.push((key, enabled)),
        }
        if let Err(e) = write_file_atomic(
            &format!("{}/rexon/protection_rules.json", config_home()),
            &defaults::Rules(rules).serialize(),
        ) {
            return QString::from(&t1("status.error", &e.to_string()));
        }
        QString::default()
    }

    fn set_terminal_colors_guard(&self, enabled: bool) -> QString {
        let Ok(mut manager) = core_settings::SettingsManager::new() else {
            return QString::from(&t1("status.error", &rexon_core::i18n::tr("settings are unavailable")));
        };
        match manager.set(rexon_core::terminal_guard::SETTING_KEY, serde_json::Value::from(enabled)) {
            Ok(_) => QString::default(),
            Err(e) => QString::from(&t1("status.error", &e.to_string())),
        }
    }

    fn snapshot(&self) -> QString {
        let updated =
            defaults::compute_snapshot(&current_defaults(), &protection_rules(), &protected_apps());
        match save_protected(&updated) {
            Ok(()) => QString::from(&t("prot.snapshot_ok")),
            Err(e) => QString::from(&t1("prot.snapshot_error", &e)),
        }
    }

    fn restore_defaults(&self) -> QString {
        let protected = protected_apps();
        let outcome = defaults::restore_defaults(
            &protected,
            &protection_rules(),
            &current_defaults(),
            false,
            &[],
            &desktop_exists,
        );
        for (app_type, desktop) in &outcome.set_calls {
            apply_default(app_type, desktop);
        }
        // The deferred choice "keep the new or restore the old" is written into
        // the same file the widget tab reads: these two must not diverge, the
        // decision about them is made by the core.
        if let Err(e) = write_file_atomic(
            &format!("{}/rexon/pending_choices.json", config_home()),
            &defaults::serialize_pending(&outcome.pending),
        ) {
            return QString::from(&t1("status.error", &e.to_string()));
        }
        QString::from(&t("prot.restore_ok"))
    }

    // ── Plasma panel copies ──────────────────────────────────────────────

    fn plasma_backup(&self) -> QString {
        // "manual" — the user pressed the button themselves.
        match panel_backup_now("manual") {
            Ok((ok, message)) => QString::from(&qs::plasma_status(ok, &message)),
            // What was an exception in Python: the directory was not created, the meta
            // file was not written. Such trouble has its own text — the general
            // "⚠️ …" outcome would call it a snapshot refusal, while it is a DISK
            // refusal.
            Err(detail) => QString::from(&t1("plasma.backup_error", &detail)),
        }
    }

    fn plasma_confirm(&self, timestamp: &QString) -> QString {
        let (title, question) = qs::plasma_restore_confirm(&timestamp.to_string());
        QString::from(&json!({ "title": title, "question": question }).to_string())
    }

    fn plasma_restore(&self, timestamp: &QString) -> QString {
        let timestamp = timestamp.to_string();
        let (ok, message) =
            panel_restore_now(Some(timestamp.as_str()).filter(|ts| !ts.is_empty()));
        QString::from(&qs::plasma_status(ok, &message))
    }

    // ── Desktop look ─────────────────────────────────────────────────────

    fn set_aeroshell(&self, enabled: bool) -> QString {
        match save_setting(aeroshell_key(), Value::from(enabled)) {
            Ok(()) => {
                QString::from(&qs::aeroshell_toggle_status(enabled))
            }
            Err(text) => QString::from(&text),
        }
    }

    // ── The AeroShell look fell behind the system ────────────────────────
    //
    // A port of `offer_aeroshell_rebuild` (`rexon/qt_main/de_progress.py`) and
    // of the start-up check (`rexon/qt_main/window.py`). The executor is the
    // core (`system_de_exec::aeroshell_survey_live` / `rebuild_aeroshell_live`);
    // the window is the shared `DeOperationDialog.qml`. The order: a check in
    // the background → if behind — a question window ("🛠 Restore" / "Not
    // now") → after "Restore" a rebuild with progress → the outcome in the
    // window and in the footer.
    // Never: the rebuild never starts by itself — only by a press in the
    // window.

    /// "Check now".
    fn aeroshell_check_now(mut self: Pin<&mut Self>) -> QString {
        self.as_mut().start_update_guard(true);
        QString::from(&self.start_aeroshell_survey(true))
    }

    /// The check at Rexon start-up: only if it is not turned off in the
    /// settings (no key — enabled).
    fn aeroshell_startup_check(mut self: Pin<&mut Self>) {
        let saved = core_settings::SettingsManager::new()
            .ok()
            .and_then(|m| m.get(aeroshell_key()).and_then(Value::as_bool));
        let enabled = aeroshell::check_on_start(saved);
        sync_update_watch();
        // Protection against an out-of-step update — for everyone and always, not
        // only with AeroThemePlasma: silently if everything is fine.
        self.as_mut().start_update_guard(false);
        // Opened by the button of a breakage notification — check at once and
        // state the outcome out loud, even if the start-up check is turned off:
        // the user asked themselves.
        let from_notice = std::env::args().any(|a| a == aeroshell::OPEN_FLAG);
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let plasma = rexon_core::app::is_plasma_desktop(&desktop);
        if enabled && !from_notice && !plasma {
            // Only into the log: the question window does not pop up in someone else's
            // session.
            rexon_core::log_info!(
                "aeroshell",
                "сверка оформления при запуске пропущена: сеанс «{desktop}» — не Plasma"
            );
        }
        if !startup_survey_wanted(enabled, from_notice, plasma) {
            return;
        }
        let _ = self.start_aeroshell_survey(from_notice);
    }

    // ── An update will install a set of packages out of step ────────────
    //
    // Decisions and texts are the core's (`rexon_core::update_guard`); here
    // are the check in the background and the message window. About one state
    // — once (the fingerprint is shared with the background check's
    // notification); by the button — always.

    /// The "before update" check in the background. `manual` — by the "Check
    /// now" button.
    fn start_update_guard(self: Pin<&mut Self>, manual: bool) {
        if UPDATE_GUARD_BUSY.swap(true, Ordering::SeqCst) {
            return;
        }
        let handle = self.qt_thread();
        std::thread::spawn(move || {
            let found = std::panic::catch_unwind(rexon_core::update_guard::advise_live).ok();
            deliver(&handle, move |object| object.update_guard_done(found, manual));
        });
    }

    fn update_guard_done(self: Pin<&mut Self>, found: Option<rexon_core::update_guard::Advice>, manual: bool) {
        use rexon_core::update_guard as ug;
        UPDATE_GUARD_BUSY.store(false, Ordering::SeqCst);
        let Some(advice) = found.filter(|a| !a.findings.is_empty()) else {
            return;
        };
        let fingerprint = advice.fingerprint();
        let rollback = ug::rollback_offer_live(&advice).is_some();
        let removal = matches!(ug::orphan_offer_live(&advice.findings), ug::OrphanOffer::Remove(_));
        // A rollback after the "open Rexon to restore" notification — as a window;
        // an offer to remove a package does not pop up without the user's request
        // (the core's decision, `update_guard::window_should_show`).
        if !ug::window_should_show(
            manual,
            rollback,
            removal,
            ug::notice_seen_live(&fingerprint),
            ug::window_seen_live(&fingerprint),
        ) {
            if removal && !manual {
                rexon_core::log_info!(
                    "update",
                    "сирота без обновлений ({fingerprint}) — окно «Удалить» само не открываю, \
                     о ней говорит уведомление; удалить — «Проверить сейчас» в настройках"
                );
            }
            return;
        }
        if !self.rust().op.phase.is_empty() {
            // Another window is open — we do not speak over it and do not mark: we
            // will say it at the next launch or with a notification.
            return;
        }
        self.show_update_guard(advice);
    }

    /// The update watcher's window: the set has already diverged and the
    /// previous versions are in the cache — a question with a "Restore the
    /// previous versions" button; there are orphans that can be removed — with a
    /// "Remove …" button; otherwise a message without an action.
    fn show_update_guard(mut self: Pin<&mut Self>, advice: rexon_core::update_guard::Advice) {
        use rexon_core::update_guard as ug;
        let fingerprint = advice.fingerprint();
        rexon_core::log_warn!("update", "обновление вразнобой — {fingerprint}");
        ug::window_mark_live(&fingerprint);
        // Restoring the previous versions goes first: a diverged set threatens a
        // black screen after a reboot. An orphan, if any, comes as a window after
        // the rollback outcome (a repeated check).
        if let Some(offer) = ug::rollback_offer_live(&advice) {
            let text = advice.text_for(ug::Audience::Window { removable: false, rollback: true });
            let view = OpView {
                yes_text: ug::rollback_button(),
                no_text: dp::aeroshell_skip_button(),
                ..OpView::confirm(&advice.title(), &rexon_core::ui_base::escape_dialog_text(&text))
            };
            self.as_mut().rust_mut().pending = Some(PendingOp::RollbackAsk(offer));
            self.as_mut().set_op(view);
            return;
        }
        // Whether to remove and whether it can be done from here is decided by the
        // core (on SteamOS there is nothing to do it with; in a session that
        // depends on the package it is not allowed: then instead of a button say
        // where to remove it from).
        let removable = match ug::orphan_offer_live(&advice.findings) {
            ug::OrphanOffer::Remove(names) => names,
            ug::OrphanOffer::Blocked(why) => {
                let text = format!("{}\n\n{why}", advice.text_for(ug::Audience::Window { removable: false, rollback: false }));
                self.as_mut().set_op(aeroshell_notice_view(&advice.title(), &text));
                return;
            }
            ug::OrphanOffer::Nothing => {
                let text = advice.text_for(ug::Audience::Window { removable: false, rollback: false });
                self.as_mut().set_op(aeroshell_notice_view(&advice.title(), &text));
                return;
            }
        };
        let text = advice.text_for(ug::Audience::Window { removable: true, rollback: false });
        let view = OpView {
            yes_text: ug::orphan_remove_button(&removable),
            no_text: dp::aeroshell_skip_button(),
            danger: true,
            ..OpView::confirm(&advice.title(), &rexon_core::ui_base::escape_dialog_text(&text))
        };
        self.as_mut().rust_mut().pending = Some(PendingOp::RemoveOrphans(removable));
        self.as_mut().set_op(view);
    }

    /// The outcome of "Make … the default session" from the watcher's window:
    /// success — the "Remove …" question again (now the helper will not
    /// refuse); failure — why.
    fn finish_wayland_default(
        mut self: Pin<&mut Self>,
        session: &'static rexon_core::system_plan::SessionSuccessor,
        names: Vec<String>,
        done: Result<String, String>,
    ) {
        use rexon_core::update_guard as ug;
        let title = rexon_core::system_plan::wayland_default_button(session);
        self.as_mut().rust_mut().cancel = None;
        match done {
            Ok(text) => {
                rexon_core::log_info!("update", "сеанс по умолчанию: {} → {}", session.x11, session.wayland);
                let text = format!("{text}\n\n{}", ug::orphan_now_removable());
                let view = OpView {
                    yes_text: ug::orphan_remove_button(&names),
                    no_text: dp::aeroshell_skip_button(),
                    danger: true,
                    ..OpView::confirm(&title, &rexon_core::ui_base::escape_dialog_text(&text))
                };
                self.as_mut().rust_mut().pending = Some(PendingOp::RemoveOrphans(names));
                self.as_mut().set_op(view);
            }
            Err(why) => {
                rexon_core::log_warn!("update", "сеанс по умолчанию не сменён ({}): {why}", session.x11);
                self.as_mut().operation_log(QString::from(&format!("{}{why}", dp::PREFIX_ERROR)));
                self.as_mut().set_op(aeroshell_notice_view(&title, &why));
            }
        }
    }

    /// The outcome of removing an orphan: success — "Removed: …" (and
    /// "everything else is fine" if the repeated check is clean); failure — why,
    /// in words.
    fn finish_remove_orphans(
        mut self: Pin<&mut Self>,
        names: Vec<String>,
        done: Option<(Result<(), String>, Option<rexon_core::update_guard::Advice>)>,
    ) {
        use rexon_core::update_guard as ug;
        let (removed, rest) = done.unwrap_or_else(|| (Err(ug::orphan_remove_failed(&names)), None));
        let rest = rest.filter(|a| !a.findings.is_empty());
        let outcome = dx::DeOpOutcome {
            ok: removed.is_ok(),
            message: removed.as_ref().err().cloned().unwrap_or_default(),
            cancelled: false,
            restart_needed: false,
        };
        let mut op = self.rust().op.clone();
        let result = ug::orphan_removed_text(&names, rest.is_none());
        if let Some(line) = op.apply_done(&outcome, rexon_core::ui_base::escape_dialog_text(&result)) {
            self.as_mut().operation_log(QString::from(&line));
        }
        // The refusal "the login screen itself opens the session on this package"
        // is not a dead end: instead of a bare refusal — a "Make … the default
        // session" button.
        if let Some(session) = removed.as_ref().err().and_then(|why| ug::login_default_successor(&names, why)) {
            let why = removed.as_ref().err().cloned().unwrap_or_default();
            rexon_core::log_warn!("update", "сирота не удалена ({}): экран входа открывает {}", names.join(" "), session.x11);
            let confirm = rexon_core::system_plan::wayland_default_confirm(session);
            let button = rexon_core::system_plan::wayland_default_button(session);
            let view = OpView {
                yes_text: button.clone(),
                no_text: dp::aeroshell_skip_button(),
                ..OpView::confirm(&button, &rexon_core::ui_base::escape_dialog_text(&format!("{why}\n\n{confirm}")))
            };
            self.as_mut().rust_mut().cancel = None;
            self.as_mut().rust_mut().pending = Some(PendingOp::WaylandDefault(session, names));
            self.as_mut().set_op(view);
            return;
        }
        if let Err(why) = &removed {
            rexon_core::log_warn!("update", "сирота не удалена ({}): {why}", names.join(" "));
            self.as_mut().operation_log(QString::from(&format!("{}{why}", dp::PREFIX_ERROR)));
            op.result_text = rexon_core::ui_base::escape_dialog_text(why);
        }
        let footer = dx::operation_footer(&ug::orphan_remove_title(&names), &op.status);
        self.as_mut().rust_mut().cancel = None;
        self.as_mut().rust_mut().update_guard_rest = rest;
        self.as_mut().set_op(op);
        self.as_mut().status(QString::from(&footer));
    }

    /// The rollback outcome: success — "the previous versions came back" (the
    /// set converged; "everything else is fine" if the repeated check is clean)
    /// or "came back, but it still does not converge"; failure — why, in words.
    fn finish_rollback(
        mut self: Pin<&mut Self>,
        offer: rexon_core::update_guard::RollbackOffer,
        done: Option<(Result<(), String>, Option<rexon_core::update_guard::Advice>)>,
    ) {
        use rexon_core::update_guard as ug;
        let (back, rest) = done.unwrap_or_else(|| (Err(ug::rollback_failed(&offer)), None));
        let rest = rest.filter(|a| !a.findings.is_empty());
        let still_mixed = rest.as_ref().is_some_and(|a| {
            a.findings.iter().any(|f| f.set.name == offer.set_name && f.stage != ug::Stage::Orphaned)
        });
        let outcome = dx::DeOpOutcome {
            ok: back.is_ok(),
            message: back.as_ref().err().cloned().unwrap_or_default(),
            cancelled: false,
            restart_needed: false,
        };
        let mut op = self.rust().op.clone();
        let result = if still_mixed {
            ug::rollback_still_mixed_text(&offer)
        } else {
            ug::rollback_done_text(&offer, rest.is_none())
        };
        if let Some(line) = op.apply_done(&outcome, rexon_core::ui_base::escape_dialog_text(&result)) {
            self.as_mut().operation_log(QString::from(&line));
        }
        if let Err(why) = &back {
            rexon_core::log_warn!("update", "прежние версии {} не вернулись: {why}", offer.set_name);
            self.as_mut().operation_log(QString::from(&format!("{}{why}", dp::PREFIX_ERROR)));
            op.result_text = rexon_core::ui_base::escape_dialog_text(why);
        }
        let footer = dx::operation_footer(&ug::rollback_title(&offer), &op.status);
        self.as_mut().rust_mut().cancel = None;
        self.as_mut().rust_mut().update_guard_rest = rest;
        self.as_mut().set_op(op);
        self.as_mut().status(QString::from(&footer));
    }

    /// The check in the background. `manual` — the button was pressed: say the
    /// "everything is fine" outcome out loud; at Rexon start-up stay silent.
    fn start_aeroshell_survey(mut self: Pin<&mut Self>, manual: bool) -> String {
        if AEROSHELL_BUSY.swap(true, Ordering::SeqCst) {
            return if manual {
                aeroshell::rebuild_busy_message()
            } else {
                String::new()
            };
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let found = std::panic::catch_unwind(dx::aeroshell_advice_live).ok();
            deliver(&handle, move |object| object.aeroshell_survey_done(found, manual));
        });
        if manual {
            aeroshell::survey_running_message()
        } else {
            String::new()
        }
    }

    fn aeroshell_survey_done(
        mut self: Pin<&mut Self>,
        found: Option<(aeroshell::Report, aeroshell::Advice)>,
        manual: bool,
    ) {
        let Some((report, advice)) = found else {
            // The check itself crashed. At start-up we stay silent (it is a courtesy,
            // not a condition of work), by the button — we say it.
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            if manual {
                let msg = t1("status.error", &completeness_internal_error());
                self.as_mut().status(QString::from(&msg));
            }
            return;
        };
        if advice == aeroshell::Advice::Nothing {
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            if manual {
                self.as_mut().status(QString::from(&t("aero.up_to_date")));
            }
            return;
        }
        // The warning BEFORE an update at start-up — once per state, shared with
        // the background check's notification: people stop reading a window that
        // says the same thing every launch. By the button (and from the
        // notification) — always.
        let fingerprint = aeroshell::notify_fingerprint(&report, &advice);
        // Third-party modules from the journal — likewise once per state: this is
        // not a breakage of the whole desktop, and there is no reason to repeat it
        // every launch.
        let before_update = matches!(
            advice,
            aeroshell::Advice::HoldUpdate { .. }
                | aeroshell::Advice::RebuildAfterUpdate { .. }
                | aeroshell::Advice::BrokenPlugins { .. }
        );
        // The same — while the look works and can only break (`at_risk_only`): we
        // speak about the risk once per state.
        let risk_only =
            matches!(advice, aeroshell::Advice::Rebuild { .. }) && report.at_risk_only();
        if (before_update || risk_only) && !manual && dx::aeroshell_notice_seen_live(&fingerprint) {
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            return;
        }
        if !self.rust().op.phase.is_empty() {
            // Another operation window is open — we do not ask over it.
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            if manual {
                self.as_mut()
                    .status(QString::from(&aeroshell::rebuild_busy_message()));
            }
            return;
        }
        rexon_core::log_info!(
            "aeroshell",
            "сверка оформления — {advice:?}:\n{}",
            aeroshell::report_details(&report)
        );
        match advice {
            aeroshell::Advice::Nothing => {}
            aeroshell::Advice::Rebuild { packages } => {
                if risk_only {
                    dx::aeroshell_notice_mark_live(&fingerprint);
                }
                self.as_mut().rust_mut().pending = Some(PendingOp::Aeroshell(packages));
                self.as_mut().set_op(aeroshell_confirm_view(&report));
                self.as_mut()
                    .status(QString::from(&dx::awaiting_answer(&dp::aeroshell_run_title())));
            }
            aeroshell::Advice::RestoreStock { forks } => {
                self.as_mut().rust_mut().pending = Some(PendingOp::RestoreStock(forks));
                self.as_mut().set_op(restore_stock_confirm_view());
                self.as_mut()
                    .status(QString::from(&dx::awaiting_answer(&aeroshell::restore_run_title())));
            }
            aeroshell::Advice::WaitForUpstream { plasma, fork } => {
                // A rebuild now would give the same broken result: say so honestly and
                // launch nothing. We will ask again at the next launch.
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                self.as_mut().set_op(aeroshell_notice_view(
                    &aeroshell::wait_title(),
                    &aeroshell::wait_text(&plasma, &fork),
                ));
            }
            aeroshell::Advice::RestoreBlocked { message } => {
                // Only say it: there is nothing to launch and no answer to wait for.
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                self.as_mut()
                    .set_op(aeroshell_notice_view(&aeroshell::restore_blocked_title(), &message));
            }
            aeroshell::Advice::HoldUpdate { plasma, fork } => {
                // Everything works now, but an update will break it: only say it.
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                dx::aeroshell_notice_mark_live(&fingerprint);
                self.as_mut().set_op(aeroshell_notice_view(
                    &aeroshell::hold_title(),
                    &aeroshell::hold_text(&plasma, &fork),
                ));
            }
            aeroshell::Advice::RebuildAfterUpdate { to, .. } => {
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                dx::aeroshell_notice_mark_live(&fingerprint);
                self.as_mut().set_op(aeroshell_notice_view(
                    &aeroshell::rebuild_after_title(),
                    &aeroshell::rebuild_after_text(&to),
                ));
            }
            aeroshell::Advice::BrokenPlugins { rebuild } => {
                dx::aeroshell_notice_mark_live(&fingerprint);
                if rebuild.is_empty() {
                    AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                    self.as_mut().set_op(broken_plugins_view(&report, false));
                } else {
                    // The same rebuild as for the set: `paru/yay -S --rebuild` only after
                    // "Restore".
                    self.as_mut().rust_mut().pending = Some(PendingOp::Aeroshell(rebuild));
                    self.as_mut().set_op(broken_plugins_view(&report, true));
                    self.as_mut()
                        .status(QString::from(&dx::awaiting_answer(&dp::aeroshell_run_title())));
                }
            }
        }
    }

    fn finish_aeroshell(self: Pin<&mut Self>, done: Option<dx::DeOpOutcome>) {
        self.finish_aeroshell_op(done, &dp::aeroshell_run_title(), &dp::aeroshell_ok_text());
    }

    fn finish_restore_stock(self: Pin<&mut Self>, done: Option<dx::DeOpOutcome>) {
        self.finish_aeroshell_op(done, &aeroshell::restore_run_title(), &aeroshell::restore_ok_text());
    }

    /// The outcome of a rebuild or of restoring the standard Plasma: one track
    /// for both.
    fn finish_aeroshell_op(
        mut self: Pin<&mut Self>,
        done: Option<dx::DeOpOutcome>,
        title: &str,
        ok_text: &str,
    ) {
        let panicked = done.is_none();
        let outcome = done.unwrap_or_else(|| dx::DeOpOutcome {
            ok: false,
            message: format!("{}: {}", dp::aeroshell_fail_title(), completeness_internal_error()),
            cancelled: false,
            restart_needed: false,
        });
        let mut op = self.rust().op.clone();
        let result = if outcome.ok { ok_text.to_string() } else { String::new() };
        if let Some(line) = op.apply_done(&outcome, result) {
            self.as_mut().operation_log(QString::from(&line));
        }
        if panicked {
            // Nobody announced an error along the way — we announce it here.
            self.as_mut().operation_log(QString::from(&format!(
                "{}{}",
                dp::PREFIX_ERROR,
                outcome.message
            )));
        }
        let footer = dx::operation_footer(title, &op.status);
        self.as_mut().rust_mut().cancel = None;
        AEROSHELL_BUSY.store(false, Ordering::SeqCst);
        self.as_mut().set_op(op);
        self.as_mut().status(QString::from(&footer));
    }
}

// ============================================================================
// Executing the actions
// ============================================================================

/// Write one setting; the error comes as a READY text of the core.
/// The snapshot's "Language" section: the saved choice and whether a
/// restart is needed.
fn language_state() -> Value {
    let saved = rexon_core::i18n::saved_setting();
    // A language that is not in the list is saved (the catalogue is not
    // installed, a code with a mistake): then Rexon speaks in whatever it
    // resulted in (usually English) — we show that, rather than a silent first
    // item of the list.
    let choice = if qs::language_choice_valid(&saved) {
        saved
    } else {
        rexon_core::i18n::language_for_setting(Some(&saved))
    };
    json!({
        "choice": choice,
        "restart": rexon_core::i18n::restart_needed(&choice),
    })
}

/// The interface language as the installer's pair (`Lang`): the same as the
/// catalogue's. Previously `Lang::from_env` stood here, reading the locale
/// bypassing `ui_language`.
fn ui_lang() -> rexon_core::distro_compat::Lang {
    rexon_core::distro_compat::Lang::from_code(&rexon_core::i18n::current_language())
}

/// The restart watcher: how many 0.2 s ticks to watch whether the new
/// process died right after launch (60 s — with a margin for waiting for the
/// lock).
const RESTART_WATCH_TICKS: u32 = 300;

/// The watcher shell of the new process. It launches it, watches for a
/// minute and, if it died with an error (missing libraries, crashed before
/// the window), writes a line into the log and shows a notification —
/// otherwise the user would only see that Rexon "did not come back". If it
/// exited with zero (knocked on a live copy), it stays silent. `$1` is the
/// log, `$2`/`$3` the notification title and text, then the command.
const RESTART_WATCHER: &str = r#"log=$1; summary=$2; body=$3; shift 3
"$@" &
c=$!
i=0
while [ $i -lt $REXON_RESTART_WATCH_TICKS ]; do
  st=X
  [ -r /proc/$c/stat ] && read -r _ _ st _ < /proc/$c/stat
  case $st in Z|X) break ;; esac
  sleep 0.2; i=$((i+1))
done
case $st in Z|X) ;; *) exit 0 ;; esac
wait $c; rc=$?
[ $rc -eq 0 ] && exit 0
printf '%s [%s] ERROR restart: новый процесс %s завершился с кодом %s сразу после запуска
' "$(date '+%Y-%m-%d %H:%M:%S.%3N')" "$$" "$c" "$rc" >> "$log"
if command -v dbus-send >/dev/null 2>&1; then
  dbus-send --session --dest=org.freedesktop.Notifications --type=method_call /org/freedesktop/Notifications org.freedesktop.Notifications.Notify string:Rexon uint32:0 string:rexon "string:$summary" "string:$body" array:string: dict:string:string: int32:0 >/dev/null 2>&1
elif command -v notify-send >/dev/null 2>&1; then
  notify-send --app-name=Rexon --icon=rexon "$summary" "$body"
fi
exit 0
"#;

/// A BENCH, not the product: how many more restarts to make in a chain.
/// Sets up a "restart N times in a row" run in a sandbox; the customer has
/// no such variable, and the branch sleeps.
pub const STAND_RESTARTS_ENV: &str = "REXON_STAND_RESTARTS";
/// The rest of this process's chain (read by `main_window` at start-up).
pub static STAND_RESTARTS_LEFT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A restart by the bench — the same path as the "Restart" button.
pub fn stand_restart_now() {
    match spawn_restart() {
        Ok(()) => crate::tray::request_quit("стенд перезапуска"),
        Err(reason) => rexon_core::log_error!("restart", "стенд: перезапуск отменён: {reason}"),
    }
}

/// The executable path for the restart. After a package update over a
/// running Rexon the kernel gives `…/rexon-main-window (deleted)` — there is
/// no such file, and the new one lies at the previous path without the
/// suffix.
pub(crate) fn restart_exe_path(raw: &str) -> String {
    raw.strip_suffix(" (deleted)").unwrap_or(raw).to_string()
}

/// A trial launch of the new copy with immediate binding of ALL symbols
/// (`LD_BIND_NOW=1`) and exiting on the first line of `main`. Catches
/// exactly the live case of 08.10.2026: Qt was half updated, and the new
/// copy crashed with "symbol lookup error" after the old one had already
/// closed. `Err` is the first line of the dynamic loader's answer.
fn probe_new_copy(exe: &str) -> Result<(), String> {
    let spec = rexon_core::utils::RunSpec {
        argv: vec![exe.to_string()],
        output: rexon_core::utils::RunOutput::Capture,
        timeout_secs: Some(15),
        env: vec![
            ("LD_BIND_NOW".to_string(), "1".to_string()),
            (rexon_core::cli::LINK_PROBE_ENV.to_string(), "1".to_string()),
        ],
        lang_c: false,
        stderr_to_stdout: true,
    };
    let report = rexon_core::utils::run(&spec, None);
    if report.outcome == rexon_core::utils::RunOutcome::Ok {
        return Ok(());
    }
    let text = rexon_core::utils::decode_text(&report.stdout);
    // The line with the loader error ("symbol lookup error", "error while
    // loading shared libraries"), not the warnings before it.
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let first = lines
        .iter()
        .find(|l| l.contains("error"))
        .or(lines.last())
        .copied()
        .unwrap_or("")
        .to_string();
    Err(if first.is_empty() {
        format!("код {:?} {}", report.returncode, report.error)
    } else {
        first
    })
}

/// Launch a new copy of Rexon (with the same executable, with the
/// `qt_settings::restart_args` arguments) under the [`RESTART_WATCHER`]
/// watcher.
///
/// The new process knows it is a restart (`single_instance::RESTART_OF_ENV`)
/// and itself waits until this one leaves and releases the lock; the former
/// waiting from outside (`kill -0` up to 30 s and launching blindly) is
/// removed. The language environment is the original one
/// (`i18n::restore_original_locale_env`): the `LANGUAGE` substituted by this
/// window would only hinder the new process from choosing "as in the
/// system".
fn spawn_restart() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    spawn_restart_with(&qs::restart_args(&args))
}

/// A restart for a package update (the "Rexon updated" notification
/// button): the same path as the "Restart" button in "Settings", but the
/// new copy opens the window where the user was, not the "Settings" section
/// (`single_instance::update_restart_args`). `Err` is a text for the user,
/// and Rexon stays open. Can be called from any thread.
pub(crate) fn restart_for_update() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match spawn_restart_with(&rexon_core::single_instance::update_restart_args(&args)) {
        Ok(()) => {
            crate::tray::request_quit("перезапуск после обновления Rexon");
            Ok(())
        }
        Err(reason) => {
            rexon_core::log_error!("restart", "после обновления отменён, Rexon остаётся открытым: {reason}");
            Err(qs::language_restart_failed(&reason))
        }
    }
}

fn spawn_restart_with(new_args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    use rexon_core::{log_info, log_warn};
    let pid = std::process::id();
    let raw = std::env::current_exe().map_err(|e| e.to_string())?;
    let raw = raw.to_string_lossy().into_owned();
    let exe = restart_exe_path(&raw);
    if exe != raw {
        log_warn!("restart", "исполняемый файл заменён обновлением, беру путь {exe}");
    }
    log_info!(
        "restart",
        "шаг 1: проверка новой копии {exe} (язык: выбор {}, станет {})",
        rexon_core::i18n::saved_setting(),
        rexon_core::i18n::language_for_setting(Some(&rexon_core::i18n::saved_setting()))
    );
    if let Err(why) = probe_new_copy(&exe) {
        rexon_core::log_error!("restart", "новая копия не запускается: {why}");
        return Err(rexon_core::i18n::trf(
            // TRANSLATORS: %1 is the technical error from the system loader (usually English). Rexon stays open.
            "the new copy of Rexon can't start (%1). Rexon stays open. This usually means a system update hasn't finished — finish it or restart the computer.",
            &[&why],
        ));
    }
    let log_path = rexon_core::applog::log_path();
    // TRANSLATORS: Title of a desktop notification when Rexon failed to come back after a restart.
    let summary = rexon_core::i18n::tr("Rexon didn't restart");
    let body = rexon_core::i18n::trf(
        // TRANSLATORS: Body of a desktop notification; %1 is the path of the Rexon log file.
        "The new copy of Rexon exited with an error right after starting. Open Rexon from the application menu; details are in the log: %1",
        &[&log_path.to_string_lossy()],
    );
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(RESTART_WATCHER)
        .arg("rexon-restart")
        .arg(&log_path)
        .arg(&summary)
        .arg(&body)
        .arg(&exe)
        .args(new_args)
        .env(rexon_core::single_instance::RESTART_OF_ENV, pid.to_string())
        .env("REXON_RESTART_WATCH_TICKS", RESTART_WATCH_TICKS.to_string())
        .stdin(std::process::Stdio::null())
        .process_group(0);
    rexon_core::i18n::restore_original_locale_env(&mut cmd);
    // The restart chain bench: one less for the next copy.
    let left = STAND_RESTARTS_LEFT.load(std::sync::atomic::Ordering::SeqCst);
    if left > 0 {
        cmd.env(STAND_RESTARTS_ENV, (left - 1).to_string());
    }
    if let Some(stderr) = rexon_core::applog::original_stderr_for_child() {
        cmd.stderr(stderr);
    }
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    log_info!(
        "restart",
        "шаг 2: сторож {} запустил новую копию: {exe} {}; прежний процесс {pid} уходит",
        child.id(),
        new_args.join(" ")
    );
    Ok(())
}

fn save_setting(key: &str, value: Value) -> Result<(), String> {
    core_settings::SettingsManager::new()
        .and_then(|mut m| m.set(key, value))
        .map_err(|e| t1("status.error", &e.to_string()))
}

/// Save the remembered default applications.
fn save_protected(protected: &[(String, Option<String>)]) -> Result<(), String> {
    // The timestamp is printed by the core (`engine_update::now_iso`) — there
    // is no second record format in the product.
    let stamp = engine_update::now_iso(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    );
    write_file_atomic(
        &format!("{}/rexon/protected_defaults.json", config_home()),
        &defaults::serialize_snapshot(protected, &stamp),
    )
    .map_err(|e| e.to_string())
}

/// Remember an application in the list (the order is kept, new ones are
/// appended).
fn remember(protected: &mut Vec<(String, Option<String>)>, app_type: &str, desktop: &str) {
    match protected.iter_mut().find(|(k, _)| k == app_type) {
        Some(entry) => entry.1 = Some(desktop.to_string()),
        None => protected.push((app_type.to_string(), Some(desktop.to_string()))),
    }
}

/// Apply a default application. WHAT exactly to launch is decided by the
/// core (`defaults::set_default`), here there is only the launch.
fn apply_default(app_type: &str, desktop: &str) -> bool {
    match defaults::set_default(app_type, desktop) {
        defaults::SetDefault::Terminal(terminal) => {
            // The terminal is not set through xdg-mime: it has its own plan of three
            // write places (the freedesktop list, gsettings, kdeglobals).
            run_terminal_plan(&defaults::set_terminal_plan(
                &terminal,
                &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
                &home(),
            ));
            true
        }
        defaults::SetDefault::Xdg(cmds) => {
            if cmds.is_empty() {
                return false;
            }
            cmds.iter().all(|argv| write_cmd(argv))
        }
    }
}

/// Execute the terminal setting plan. The steps' exit codes are not looked
/// at — as in the original: some of the write places do not exist in every
/// environment.
fn run_terminal_plan(plan: &[defaults::TermStep]) {
    let mut env_written = false;
    for step in plan {
        match step {
            defaults::TermStep::WriteFile { path, content } => {
                if write_file_atomic(path, content).is_ok() {
                    env_written |= rexon_core::compositor_autostart::is_environment_d_path(path);
                }
            }
            defaults::TermStep::MakeDirs { path } => {
                let _ = std::fs::create_dir_all(path);
            }
            defaults::TermStep::Run { argv } => {
                write_cmd(argv);
            }
            defaults::TermStep::SetIni {
                path,
                section,
                key,
                value,
            } => {
                let existing = read_file(path);
                let text = defaults::set_ini_value(existing.as_deref(), section, key, value);
                let _ = write_file_atomic(path, &text);
            }
        }
    }
    // `TERMINAL` — also into sessions that do not read `environment.d` (D1).
    if env_written {
        let _ = rexon_core::compositor_autostart::sync_session_env_live();
    }
}

/// Make `fm_id` the default file manager → whether it was applied.
///
/// A port of `appdefaults.set_file_manager_default`: choosing the target
/// `.desktop`, writing through `xdg-mime`, a per-user override of the D-Bus
/// FileManager1 activation ("show in folder" does NOT go through mimeapps)
/// and checking what was actually written. All decisions are the core's.
fn set_file_manager_default(fm_id: &str) -> bool {
    let Some(desktop) = appdefaults::fm_default_target(fm_id, &desktop_exists) else {
        return false;
    };
    if !write_cmd(&appdefaults::xdg_mime_default_cmd(&desktop)) {
        return false;
    }
    override_filemanager1(&appdefaults::fm_short_name(fm_id, &desktop));
    // The environment's native place beyond mimeapps: in XFCE —
    // `helpers.rc FileManager=` (read by `exo-open`, the panel and the
    // desktop; D7, environment check of 02.10.2026). The steps are decided by
    // the core.
    run_terminal_plan(&appdefaults::file_manager_home_steps(
        &desktop,
        &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        &config_home(),
        &defaults::xfce_system_helper_exists,
    ));
    // Verification: what was actually written. Without it the button reports
    // success where `xdg-mime` ran idly.
    read_cmd(&appdefaults::xdg_mime_query_cmd()).contains(&desktop)
}

/// A per-user override of the `org.freedesktop.FileManager1` activation.
///
/// On Arch the system service file is owned by the nautilus package: when
/// desktops coexist, GNOME Files intercepts "show in folder" whatever is in
/// mimeapps. A file in `~/.local/share/dbus-1/services/` overrides the
/// system one.
fn override_filemanager1(binary: &str) {
    let Ok(entries) = std::fs::read_dir("/usr/share/dbus-1/services") else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("service") {
            continue;
        }
        let Some(text) = path.to_str().and_then(read_file) else {
            continue;
        };
        // Whether this is the right file manager is decided by the core
        // (`fm1_matches`).
        if !appdefaults::fm1_matches(&text, binary) {
            continue;
        }
        let target = format!(
            "{}/dbus-1/services/{}.service",
            data_home(),
            appdefaults::FM1_NAME
        );
        let _ = write_file_atomic(&target, &text);
        return;
    }
}

/// Launch and forget: the sound player lives its own life, and waiting for
/// it is not allowed — the "listen" button would hang for the whole length
/// of the sound.
fn spawn_detached(argv: &[String]) -> bool {
    let Some((binary, args)) = argv.split_first() else {
        return false;
    };
    std::process::Command::new(binary)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The name of the shortcut item that opened the classic window. The item
    /// has not been in the shortcut since 16.09, and the wording that pointed
    /// there is removed — the name stayed only here, as a guard: no answer to
    /// the customer has the right to refer them to the removed window again.
    use rexon_core::installer::CLASSIC_WINDOW_ENTRY;

    // ── The process's shared wallpaper executor ──────────────────────────

    /// A fake executor machine: launches and writes nothing, only remembers
    /// what was brought up and what was taken down. The owner's live wallpaper
    /// is not touched.
    #[derive(Default)]
    struct QuietSys {
        spawned: Vec<Vec<String>>,
        killed: Vec<i64>,
        next: i64,
        clock: f64,
    }

    impl WpSys for QuietSys {
        fn run(&mut self, _argv: &[String], _timeout: f64) -> Option<rexon_core::session_splash::RunOut> {
            Some(rexon_core::session_splash::RunOut {
                rc: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
        fn spawn(&mut self, argv: &[String], _new_session: bool) -> Result<i64, String> {
            self.spawned.push(argv.to_vec());
            self.next += 1;
            Ok(self.next)
        }
        fn poll(&mut self, proc: i64) -> Option<i64> {
            self.killed.contains(&proc).then_some(-15)
        }
        fn pid(&mut self, proc: i64) -> Option<i64> {
            Some(4000 + proc)
        }
        fn kill_process(&mut self, proc: i64) {
            self.killed.push(proc);
        }
        fn has_tool(&mut self, _tool: &str) -> bool {
            false
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, _path: &str) -> bool {
            true
        }
        fn is_dir(&mut self, _path: &str) -> bool {
            false
        }
        fn listdir(&mut self, _path: &str) -> Vec<String> {
            Vec::new()
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
        fn write_text(&mut self, _path: &str, _content: &str) {}
        fn remove_file(&mut self, _path: &str) {}
        fn env(&mut self, name: &str) -> String {
            if name == "HOME" { "/fake-home".to_string() } else { String::new() }
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 0.01;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, _live: bool, _name: &str, _announce: bool) {}
        fn video_host(&mut self) -> Option<String> {
            Some("/opt/rexon/rexon-wallpaper-host".to_string())
        }
        fn file_size(&mut self, _path: &str) -> Option<u64> {
            Some(4096)
        }
    }

    /// Repeat the door until the lock is free: after applying a video the
    /// core's shared door brings up load supervision, and its first measurement
    /// takes the same lock for a moment. The window answers "already applying"
    /// at that moment — while here the outcome is needed.
    fn when_free<R>(mut door: impl FnMut() -> Option<R>) -> R {
        for _ in 0..200 {
            if let Some(out) = door() {
                return out;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("исполнитель так и не освободился");
    }

    /// ⭐ One executor per process: a video set by the "Steam" page (its live
    /// `ExecApplier` engine, the sound checkbox off) is seen by "Settings" in
    /// its snapshot and taken down by its "Stop". The slot here has a fake
    /// seam, and the doors are the same as for the live pages.
    #[test]
    fn steam_applied_video_is_seen_and_stopped_by_settings() {
        let slot: wallpaper_exec::ExecSlot<QuietSys> = Arc::new(Mutex::new(Some(
            WallpaperExec::with_env(QuietSys::default(), "sway", "wayland"),
        )));

        let steam = crate::steam_workers::ExecApplier::on(slot.clone());
        let (ok, message) = crate::steam_workers::WallpaperApply::apply(
            &steam,
            "/fake-home/.cache/rexon/steam-bg/7.webm",
            "video",
            false,
            "Ночной город",
        );
        assert!(ok, "{message}");

        let (running, format, path) = when_free(|| {
            let now = wallpaper_now(&slot);
            now.0.then_some(now)
        });
        assert!(running, "«Настройки» не видят видео из «Steam»");
        assert_eq!(format.as_deref(), Some("video"));
        assert_eq!(path.as_deref(), Some("/fake-home/.cache/rexon/steam-bg/7.webm"));
        let host_argv = with_slot_sys(&slot, |sys| sys.spawned.last().cloned())
            .expect("хост видео не поднят");
        assert!(host_argv.iter().any(|a| a == "--no-audio"), "галочка звука потерялась: {host_argv:?}");

        let res = when_free(|| stop_wallpaper_in(&slot));
        assert!(res.ok, "{}", res.message);
        assert!(!wallpaper_now(&slot).0, "видео из «Steam» не снято «Остановить»");
        assert!(!with_slot_sys(&slot, |sys| sys.killed.is_empty()), "процесс хоста не снят");
    }

    fn with_slot_sys<R>(
        slot: &wallpaper_exec::ExecSlot<QuietSys>,
        look: impl FnOnce(&mut QuietSys) -> R,
    ) -> R {
        wallpaper_exec::with_slot(slot, |exec| look(exec.sys_mut()))
    }

    /// The snapshot must name ALL eighteen keys — on an empty machine too. The
    /// page reads them with a dot (`root.st.prot.enabled`), and a forgotten key
    /// would arrive in it as `undefined`, that is, the toggle would silently go
    /// to "off", and the section would show emptiness instead of content.
    ///
    /// Warning: the names here are written out ANEW, not taken from
    /// `STATE_KEYS`. A comparison with the list that it checks itself would be
    /// green on any renaming of it — that is, it would check nothing. The real
    /// source of these seventeen words is `qml/SettingsPage.qml`
    /// (`root.st.polkit`, `root.st.quickfix`, `root.st.aero`…), and they can be
    /// rewritten only together with it.
    #[test]
    fn state_names_every_key_the_page_reads() {
        let wanted = [
            "polkit", "lang", "skin", "onboarding", "tray", "fm", "quickfix", "compl", "wp", "lib",
            "imp", "blur", "amb", "snd", "font", "prot", "plasma", "aero",
        ];
        let empty = state_object(std::array::from_fn(|_| Value::Null));
        let map = empty.as_object().expect("снимок обязан быть объектом");
        for key in wanted {
            assert!(map.contains_key(key), "в снимке нет ключа «{key}»");
        }
        assert_eq!(map.len(), wanted.len(), "в снимке появился лишний ключ");
    }

    /// The "Wallpaper" section must name all the keys the page reads with a
    /// dot (`wpBody.wp.running`, `wpBody.audio.muteOther`…): a forgotten one
    /// would arrive as `undefined`, and the "Stop" button would silently stay
    /// disabled, and the yt-dlp button label empty. The names are written out
    /// anew from `qml/SettingsPage.qml`, not taken from the code being checked.
    #[test]
    fn wallpaper_section_names_every_key_the_page_reads() {
        let view = WallpaperView {
            running: false,
            format: None,
            path: None,
            audio: wallpaper_audio::AudioSettings::default(),
            youtube_audio: true,
            ytdlp_version: None,
            ytdlp_busy: false,
            busy: false,
        };
        let section = wallpaper_section(&view);
        for key in [
            "engine", "running", "status", "audio", "ytAudio", "ytdlpLabel", "ytdlpButton",
            "ytdlpAction", "ytdlpBusy", "busy",
        ] {
            assert!(section.get(key).is_some(), "в разделе обоев нет ключа «{key}»");
        }
        for key in ["enabled", "volume", "muteOther", "hideCovered", "pauseBattery", "pauseGame"] {
            assert!(section["audio"].get(key).is_some(), "в звуке обоев нет ключа «{key}»");
        }
        // The section exists: without this the page hides both ported buttons.
        assert_eq!(section["engine"], Value::Bool(true));
        // The yt-dlp button is not left without a label under any answer of the
        // system.
        assert!(!section["ytdlpButton"].as_str().unwrap_or("").is_empty());
        let with_ytdlp = wallpaper_section(&WallpaperView {
            ytdlp_version: Some("2026.09.01".to_string()),
            ..view
        });
        assert_eq!(with_ytdlp["ytdlpAction"], "update");
    }

    /// Which wallpaper files are applied is decided by the core: picture, video
    /// and Flash are applied without the classic window.
    #[test]
    fn picture_video_and_flash_buttons_are_carried() {
        assert!(wallpaper_exec::carries_format("static"));
        assert!(wallpaper_exec::carries_format("video"));
        assert!(wallpaper_exec::carries_format("swf"));
    }

    /// Library tiles of any format except Flash go to the executor: GIF is
    /// applied by its plan, a scene and an application wallpaper get the same
    /// words as in the classic window. The slot has a fake seam, the door is
    /// the same as for the "Apply" button (`run_in_slot` → `apply` →
    /// `wallpaper_done`).
    #[test]
    fn library_tiles_except_flash_are_answered_here() {
        // The texts come from the translation catalogue; the source language is
        // pinned so that the comparison does not depend on the machine's locale
        // (the core compares Russian).
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        for (fmt, want) in [
            ("scene", wallpaper_engine::scene_unsupported()),
            ("application", wallpaper_engine::application_unsupported()),
            ("parallax", wallpaper_engine::unknown_format_message("parallax")),
            // On sway without swww and mpvpaper — the engine plan's "what to install"
            // refusal.
            (
                "animated",
                rexon_core::i18n::tr("Neither swww nor mpvpaper is installed for GIF wallpapers on Wayland"),
            ),
        ] {
            assert!(wallpaper_exec::carries_format(fmt), "«{fmt}» отослан бы в классическое окно");
            let slot: wallpaper_exec::ExecSlot<QuietSys> = Arc::new(Mutex::new(Some(
                WallpaperExec::with_env(QuietSys::default(), "sway", "wayland"),
            )));
            let path = format!("/fake-home/.local/share/rexon/extensions/пак/data/обои.{fmt}");
            let line = wallpaper_exec::run_in_slot(&slot, move |exec| {
                let res = exec.apply(&path, fmt, "");
                assert!(!res.ok, "{fmt}: {}", res.message);
                qs::wallpaper_done(res.ok, &res.message)
            });
            assert!(line.contains(&want), "{fmt}: {line}");
            assert!(!line.contains(CLASSIC_WINDOW_ENTRY), "{fmt}: {line}");
            assert!(with_slot_sys(&slot, |sys| sys.spawned.is_empty()), "{fmt}: что-то поднято");
        }
    }

    /// Boundary guard: the page's working code refers NOWHERE to the classic
    /// window — Flash moved last, and on 16.09 the wording itself
    /// (`classic_window_only`) was removed together with the shortcut item it
    /// named. A button referring there again will turn red here: both by the
    /// name of the removed function and by the name of the shortcut item that
    /// is no longer in the shortcut.
    #[test]
    fn nothing_goes_to_the_classic_window() {
        const SRC: &str = include_str!("settings_actions.rs");
        let code = &SRC[..SRC.find("#[cfg(test)]").expect("нет модуля тестов")];
        for (i, line) in code.lines().enumerate() {
            let text = line.trim_start();
            if text.starts_with("//") {
                continue;
            }
            assert!(
                !text.contains("classic_window_only(")
                    && !text.contains("blocked(")
                    && !text.contains("CLASSIC_WINDOW_ENTRY"),
                "строка {}: отсылает в классическое окно: {line}",
                i + 1
            );
        }
        let known = ["static", "video", "animated", "web", "youtube", "swf", "scene", "application"];
        for fmt in known {
            assert!(wallpaper_exec::carries_format(fmt), "«{fmt}» снова граница");
        }
    }

    /// The font window refusal is NOT "a feature in the classic window": there
    /// is a window there, it just did not open, and referring the user to
    /// another window would be untrue.
    #[test]
    fn font_dialog_failure_is_not_a_porting_border() {
        assert!(!font_dialog_failed().contains(CLASSIC_WINDOW_ENTRY));
        assert!(!font_dialog_failed().contains("FontDialog"));
    }

    /// ⭐ The door is open: the font choice shim's answer must be read by the
    /// CORE, not by a second parser. The "Family,size" string is glued by QML
    /// (`cpp/rexon_ext_dialogs.cpp`, the FontPick dialog), and it is
    /// deliberately made in the KDE notation — the very one `parse_font_spec`
    /// understands.
    ///
    /// The font names here are real: "DejaVu Sans Mono" has spaces in its name,
    /// and a naive split by space would tear "Mono" off it.
    #[test]
    fn chosen_font_is_read_by_the_core() {
        assert_eq!(
            chosen_font("DejaVu Sans Mono,11"),
            Some(("DejaVu Sans Mono".to_string(), 11))
        );
        // A window refusal and "nothing chosen" are an empty string, there is
        // nothing to apply.
        assert_eq!(chosen_font(""), None);
    }

    /// Warning: the size `-1` always comes from Qt when the font is set in a
    /// PIXEL size (`QFont::pointSize()` returns -1 in this case). Letting it into
    /// the environment config means writing "Noto Sans,-1" — that is, a
    /// zero-size font across the whole desktop. The fallback size is the same as
    /// in the Python original.
    #[test]
    fn pixel_sized_font_never_reaches_the_config() {
        assert_eq!(chosen_font("Noto Sans,-1"), Some(("Noto Sans".to_string(), 10)));
        assert_eq!(chosen_font("Noto Sans,0"), Some(("Noto Sans".to_string(), 10)));
        for (_, size) in [chosen_font("Noto Sans,-1"), chosen_font("Noto Sans,0")]
            .into_iter()
            .flatten()
        {
            assert!(size > 0, "в конфиг уехал непригодный кегль {size}");
        }
    }

    /// Transferring the interface font to GTK: only `system` has the key, and
    /// the ini text is assembled by the shared look engine. EXACTLY the pair of
    /// calls `apply_font` makes is checked — otherwise the comparison would be
    /// green on its own copy of the rules.
    #[test]
    fn only_the_interface_font_goes_to_gtk() {
        assert!(qs::font_syncs_to_gtk("system"));
        assert!(!qs::font_syncs_to_gtk("mono"));
        assert!(!qs::font_syncs_to_gtk("title"));

        let text = gtk_sanitizer::write_ini_setting_in(
            Some("[Settings]\ngtk-theme-name=Breeze\n"),
            "Settings",
            qs::GTK_FONT_KEY,
            &qs::gnome_font_spec("Noto Sans", 11),
        );
        assert!(
            text.contains("gtk-font-name=Noto Sans 11"),
            "шрифт не доехал до settings.ini: {text}"
        );
        assert!(
            text.contains("gtk-theme-name=Breeze"),
            "перенос шрифта затёр чужую настройку: {text}"
        );
    }

    /// ⭐ A fix during the port: the skin name the list sees must match the one
    /// the page is DRAWN with. Python gave the raw setting value, while the
    /// palette fell back to the standard skin — and the tab was painted in one
    /// skin while the list showed another.
    #[test]
    fn skin_shown_is_the_skin_painted() {
        // A foreign name in the settings: the palette takes the standard skin…
        let painted = ui_base::active_skin_id_from(Some("нет-такого-скина"));
        assert_eq!(painted, ui_base::DEFAULT_SKIN);
        // …and the snapshot must name the same one, not the saved garbage.
        assert!(ui_base::is_valid_skin(painted));
        // A known name passes as is.
        assert_eq!(
            ui_base::active_skin_id_from(Some(ui_base::DEFAULT_SKIN)),
            ui_base::DEFAULT_SKIN
        );
    }

    /// The installation outcome in the section says what really happened.
    /// Success — the core's check summary; a password refusal and cancellation
    /// — the first line of the executor's outcome, not "failed: N" for commands
    /// that were not launched.
    #[test]
    fn completeness_note_says_what_happened() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let ok = dx::DeOpOutcome {
            ok: true,
            message: dx::completeness_done_message(),
            cancelled: false,
            restart_needed: false,
        };
        let fixed = vec![
            ("cosmic_wallpapers".to_string(), "fixed".to_string()),
            ("cosmic_keyring".to_string(), "ok".to_string()),
        ];
        assert_eq!(completeness_note_after(&ok, &fixed), qs::compliance_summary(&fixed));

        let denied = dx::DeOpOutcome {
            ok: false,
            message: dx::completeness_denied_message(),
            cancelled: false,
            restart_needed: false,
        };
        let refused = vec![("cosmic_wallpapers".to_string(), "failed".to_string())];
        let note = completeness_note_after(&denied, &refused);
        let head = dx::completeness_denied_message();
        let head = head.lines().next().unwrap();
        assert_eq!(note, format!("⚠️ {head}"), "{note}");
        assert!(!note.contains('\n'), "{note}");
        assert!(!note.contains(&qs::compliance_summary(&refused)), "{note}");

        let cancelled = dx::DeOpOutcome {
            ok: false,
            message: dx::cancelled_message(),
            cancelled: true,
            restart_needed: false,
        };
        assert_eq!(
            completeness_note_after(&cancelled, &refused),
            format!("⚠️ {}", dx::cancelled_message())
        );
        // The button itself no longer refers to the classic window.
        assert!(!completeness_internal_error().contains(CLASSIC_WINDOW_ENTRY));
    }

    /// The question window about restoring the look speaks in the core's words,
    /// like the Python window: "🛠 Restore" / "Not now", a blue (not red)
    /// consent button, no package names and no builders in the text.
    #[test]
    fn aeroshell_confirm_window_speaks_core_texts() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let view = aeroshell_confirm_view(&aeroshell::Report::empty());
        assert_eq!(view.phase, "confirm");
        assert_eq!(view.title, dp::aeroshell_title());
        assert_eq!(view.yes_text, dp::aeroshell_fix_button());
        assert_eq!(view.no_text, dp::aeroshell_skip_button());
        assert!(view.text.contains(&dp::aeroshell_text()), "{}", view.text);
        assert!(view.text.contains(&dp::aeroshell_informative()), "{}", view.text);
        assert!(view.text.contains("log out and log back in"), "{}", view.text);
        assert!(!view.danger);
        for jargon in ["aeroshell-", "pkexec", "paru", "yay", "AUR"] {
            assert!(!view.text.contains(jargon), "жаргон «{jargon}»: {}", view.text);
        }
    }

    /// Outside Plasma the AeroShell window does not pop up at start-up; at the
    /// user's request (the notification button) we check in any session.
    #[test]
    fn aeroshell_startup_survey_only_in_plasma() {
        assert!(startup_survey_wanted(true, false, true));
        assert!(!startup_survey_wanted(true, false, false), "MATE: молчим");
        assert!(!startup_survey_wanted(false, false, true), "выключено в настройках");
        assert!(startup_survey_wanted(false, true, false), "из уведомления — всегда");
        assert!(startup_survey_wanted(true, true, false));
    }

    /// "Check now" and the start-up check no longer refer to the classic
    /// window: none of their functions calls `blocked`.
    #[test]
    fn aeroshell_is_no_longer_a_border() {
        const SRC: &str = include_str!("settings_actions.rs");
        for head in [
            "fn aeroshell_check_now(mut self: Pin<&mut Self>)",
            "fn aeroshell_startup_check(mut self: Pin<&mut Self>)",
            "fn start_update_guard(self",
            "fn update_guard_done(",
            "fn start_aeroshell_survey(mut self",
            "fn aeroshell_survey_done(",
            "fn finish_aeroshell(self",
        ] {
            let start = SRC.find(head).unwrap_or_else(|| panic!("нет «{head}»"));
            let body = &SRC[start..];
            let end = body.find("\n    }\n").expect("конец функции не найден");
            assert!(!body[..end].contains("blocked("), "«{head}» отсылает в классическое окно");
        }
    }

    /// An autostart refusal must name the reason rather than send the user to
    /// the log: the customer does not read the log.
    #[test]
    fn autostart_failure_names_the_reason() {
        let line = format!(
            "{} {}",
            t("tray.autostart_failed"),
            t1("status.error", "нет прав на ~/.config/autostart")
        );
        assert!(line.contains("нет прав"), "причина потерялась: {line}");
        assert!(!t("tray.autostart_failed").is_empty());
    }
}
