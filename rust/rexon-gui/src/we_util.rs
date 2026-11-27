// Util — the QML singleton of platform odds and ends for the Wallpaper
// Engine tab.
//
// A port of waywallen's `ui/src/util.cpp` (MIT, (c) hypengw; attribution in
// NOTICE).
//
// There is no logic here: desktop detection, BBCode→HTML and the WE wire
// colour are computed by the core (`rexon_core::we_util`, covered by
// `we_util_parity.rs`). The bridge is left with the QObject wrapper and
// the QColor ↔ components conversion — exactly the boundary the Python
// shim had.
//
// Warning: what the original does not have and Rexon keeps:
//   * `wpTypeLabel` — the only point where the DISPLAY of a wallpaper type
//     is translated. Technical strings ("scene"/"video"/…) go into queries
//     and filters and must not be replaced, so only the output is
//     translated, and the catalogue lives in one place
//     (`rexon_core::remote_l10n`, rule #12).
//   * `isFlatpak` returns true not because we are in Flatpak: in the
//     vendored QML this property hides the ONLY switch — "Start at login"
//     (SettingsPage.qml:115/127/400). Rexon does autostart natively
//     (systemd user unit / XDG), so the switch must be visible.
//     Warning: when updating the vendor, re-check the uses of isFlatpak.
//
// Also here is the `UpdateChecker` singleton (upstream
// `ui/src/update_checker.cppm`, since 0.4.4): checking for a new waywallen
// release on GitHub. Inside Rexon it is not needed — the engine ships in
// the Rexon package and is updated together with it. So the singleton is a
// stub of exactly the kind an upstream build without `update-check`
// provides: `supported` = false, `enabled` is always false (writes are
// skipped), `check()` does nothing, no network. Markup newer than 0.4.4
// hides the buttons and the switch by itself based on `supported`, and the
// markup edit (patches/0005) is not needed with it.

/// cxx-qt bridge: the `Util` singleton.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qcolor.h");
        type QColor = cxx_qt_lib::QColor;

        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;

        include!("cxx-qt-lib/qdatetime.h");
        type QDateTime = cxx_qt_lib::QDateTime;
    }

    /// The desktop the interface runs under. The values come from the original
    /// (`util.cppm`); the vendored QML reads them as `Util.Desktop.Kde`.
    #[qenum(Util)]
    enum Desktop {
        Unknown,
        Kde,
        Hyprland,
        Sway,
        Niri,
        Gnome,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, desktop, READ = desktop, CONSTANT)]
        #[qproperty(bool, supports_display_rename, cxx_name = "supportsDisplayRename",
                    READ = supports_display_rename, CONSTANT)]
        #[qproperty(bool, is_flatpak, cxx_name = "isFlatpak",
                    READ = is_flatpak, CONSTANT)]
        type Util = super::UtilRust;

        /// Stub of the waywallen update check (see the file header). Names and
        /// signals are as upstream, so that markup of any version binds without
        /// errors.
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, supported, READ = supported, CONSTANT)]
        #[qproperty(bool, enabled, READ = enabled, WRITE = set_enabled, NOTIFY = enabled_changed)]
        #[qproperty(bool, checking, READ = checking, NOTIFY = checking_changed)]
        #[qproperty(bool, update_available, cxx_name = "updateAvailable",
                    READ = update_available, NOTIFY = latest_version_changed)]
        #[qproperty(QString, latest_version, cxx_name = "latestVersion",
                    READ = latest_version, NOTIFY = latest_version_changed)]
        #[qproperty(QUrl, release_url, cxx_name = "releaseUrl",
                    READ = release_url, NOTIFY = latest_version_changed)]
        #[qproperty(QDateTime, last_check_time, cxx_name = "lastCheckTime",
                    READ = last_check_time, NOTIFY = last_check_time_changed)]
        type UpdateChecker = super::UpdateCheckerRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "enabledChanged"]
        fn enabled_changed(self: Pin<&mut UpdateChecker>);
        #[qsignal]
        #[cxx_name = "checkingChanged"]
        fn checking_changed(self: Pin<&mut UpdateChecker>);
        #[qsignal]
        #[cxx_name = "latestVersionChanged"]
        fn latest_version_changed(self: Pin<&mut UpdateChecker>);
        #[qsignal]
        #[cxx_name = "lastCheckTimeChanged"]
        fn last_check_time_changed(self: Pin<&mut UpdateChecker>);
        /// Upstream — the end of a check. Here there are no checks and the signal
        /// is never emitted; it is declared for the sake of `Connections` in the
        /// markup.
        #[qsignal]
        #[cxx_name = "checkFinished"]
        fn check_finished(self: Pin<&mut UpdateChecker>, ok: bool);
    }

    extern "RustQt" {
        fn supported(self: &UpdateChecker) -> bool;
        fn enabled(self: &UpdateChecker) -> bool;
        #[cxx_name = "setEnabled"]
        fn set_enabled(self: Pin<&mut UpdateChecker>, enabled: bool);
        fn checking(self: &UpdateChecker) -> bool;
        #[cxx_name = "updateAvailable"]
        fn update_available(self: &UpdateChecker) -> bool;
        #[cxx_name = "latestVersion"]
        fn latest_version(self: &UpdateChecker) -> QString;
        #[cxx_name = "releaseUrl"]
        fn release_url(self: &UpdateChecker) -> QUrl;
        #[cxx_name = "lastCheckTime"]
        fn last_check_time(self: &UpdateChecker) -> QDateTime;

        /// Upstream — check now. Here it does nothing.
        #[qinvokable]
        fn check(self: Pin<&mut UpdateChecker>);
    }

    extern "RustQt" {
        fn desktop(self: &Util) -> i32;
        #[cxx_name = "supportsDisplayRename"]
        fn supports_display_rename(self: &Util) -> bool;
        #[cxx_name = "isFlatpak"]
        fn is_flatpak(self: &Util) -> bool;

        /// Workshop description (BBCode) → a subset of HTML for Qt.StyledText.
        ///
        /// Warning: an upstream bug is fixed in the core here: auto-links wrapped an
        /// already finished anchor from `[url]` into a second `<a>`.
        #[qinvokable]
        #[cxx_name = "bbcodeToHtml"]
        fn bbcode_to_html(self: &Util, src: &QString) -> QString;

        /// WE wire colour ("r g b [a]", numbers 0..1) → QColor.
        #[qinvokable]
        #[cxx_name = "colorFromWire"]
        fn color_from_wire(self: &Util, s: &QString) -> QColor;

        #[qinvokable]
        #[cxx_name = "colorToWire"]
        fn color_to_wire(self: &Util, color: &QColor, include_alpha: bool) -> QString;

        #[qinvokable]
        #[cxx_name = "colorHasAlpha"]
        fn color_has_alpha(self: &Util, s: &QString) -> bool;

        /// Wallpaper type in Russian. Added by Rexon (see the file header).
        #[qinvokable]
        #[cxx_name = "wpTypeLabel"]
        fn wp_type_label(self: &Util, wp_type: &QString) -> QString;
    }
}

use core::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QColor, QDateTime, QString, QUrl};

/// Singleton state: the desktop is computed ONCE at start-up, as in the
/// original (`Util::Util` reads `XDG_CURRENT_DESKTOP` in the constructor).
pub struct UtilRust {
    desktop: i32,
}

impl Default for UtilRust {
    fn default() -> Self {
        let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        Self {
            desktop: rexon_core::we_util::detect_desktop(&xdg),
        }
    }
}

impl qobject::Util {
    pub fn desktop(&self) -> i32 {
        self.rust().desktop
    }

    pub fn supports_display_rename(&self) -> bool {
        rexon_core::we_util::supports_display_rename(self.rust().desktop)
    }

    pub fn is_flatpak(&self) -> bool {
        // A deliberate Rexon divergence, analysed in the file header.
        true
    }

    pub fn bbcode_to_html(&self, src: &QString) -> QString {
        QString::from(&rexon_core::we_util::bbcode_to_html(&src.to_string()))
    }

    pub fn color_from_wire(&self, s: &QString) -> QColor {
        let [r, g, b, a] = rexon_core::we_util::color_from_wire(&s.to_string());
        QColor::from_rgba_f(r as f32, g as f32, b as f32, a as f32)
    }

    pub fn color_to_wire(&self, color: &QColor, include_alpha: bool) -> QString {
        QString::from(&rexon_core::we_util::color_to_wire(
            color.red_f() as f64,
            color.green_f() as f64,
            color.blue_f() as f64,
            color.alpha_f() as f64,
            include_alpha,
        ))
    }

    pub fn color_has_alpha(&self, s: &QString) -> bool {
        rexon_core::we_util::color_has_alpha(&s.to_string())
    }

    pub fn wp_type_label(&self, wp_type: &QString) -> QString {
        QString::from(&rexon_core::remote_l10n::wp_type_label(
            &wp_type.to_string(),
        ))
    }
}

/// The stub has no state: everything it returns is constant.
#[derive(Default)]
pub struct UpdateCheckerRust;

impl qobject::UpdateChecker {
    pub fn supported(&self) -> bool {
        false
    }

    pub fn enabled(&self) -> bool {
        false
    }

    /// As upstream without `update-check`: the write is skipped silently, and
    /// `enabledChanged` is not emitted (the value did not change).
    pub fn set_enabled(self: Pin<&mut Self>, _enabled: bool) {}

    pub fn checking(&self) -> bool {
        false
    }

    pub fn update_available(&self) -> bool {
        false
    }

    pub fn latest_version(&self) -> QString {
        QString::default()
    }

    pub fn release_url(&self) -> QUrl {
        QUrl::default()
    }

    pub fn last_check_time(&self) -> QDateTime {
        QDateTime::default()
    }

    pub fn check(self: Pin<&mut Self>) {}
}
