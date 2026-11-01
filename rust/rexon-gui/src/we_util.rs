// Util — QML-синглтон платформенных мелочей вкладки Wallpaper Engine.
//
// Порт
// `ui/src/util.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//
// Логики здесь нет: детект рабочего стола, BBCode→HTML и проводной цвет WE
// считает ядро (`rexon_core::we_util`, прикрыто `we_util_parity.rs`). Мосту
// остаётся оболочка QObject и перевод QColor ↔ компоненты — ровно та
// граница, что была у Python-шима.
//
// ⚠️ Чего в оригинале нет и что оставлено Rexon'ом:
//   * `wpTypeLabel` — единственная точка перевода ПОКАЗА типа обоины.
//     Технические строки («scene»/«video»/…) уходят в запросы и фильтры,
//     подменять их нельзя, поэтому переводится только вывод, и каталог
//     живёт в одном месте (`rexon_core::remote_l10n`, правило #12).
//   * `isFlatpak` отдаёт true не потому, что мы во Flatpak: в вендорном QML
//     этим свойством закрыт ЕДИНСТВЕННЫЙ переключатель — «Запускать при
//     входе» (SettingsPage.qml:115/127/400). Автозапуск Rexon делает
//     нативно (systemd user unit / XDG), поэтому свитч обязан быть виден.
//     ⚠️ При обновлении вендора перепроверять использования isFlatpak.
//
// Здесь же — синглтон `UpdateChecker` (у апстрима `ui/src/update_checker.cppm`,
// с 0.4.4): проверка нового выпуска waywallen на GitHub. Внутри Rexon она не
// нужна — движок едет в пакете Rexon и обновляется вместе с ним. Поэтому
// синглтон — заглушка ровно того вида, какой даёт сборка апстрима без
// `update-check`: `supported` = false, `enabled` всегда false (запись
// пропускается), `check()` ничего не делает, сети нет. Разметка новее 0.4.4
// по `supported` сама прячет кнопки и выключатель, и правка разметки
// (patches/0005) с ней не нужна.

/// Мост cxx-qt: синглтон `Util`.
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

    /// Рабочий стол, под которым идёт интерфейс. Значения — из оригинала
    /// (`util.cppm`), их читает вендорный QML как `Util.Desktop.Kde`.
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

        /// Заглушка проверки обновлений waywallen (шапка файла). Имена и
        /// сигналы — как у апстрима, чтобы разметка любой версии
        /// связывалась без ошибок.
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
        /// У апстрима — конец проверки. Здесь проверок нет, и сигнал не
        /// испускается никогда; объявлен ради `Connections` в разметке.
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

        /// У апстрима — проверить сейчас. Здесь ничего не делает.
        #[qinvokable]
        fn check(self: Pin<&mut UpdateChecker>);
    }

    extern "RustQt" {
        fn desktop(self: &Util) -> i32;
        #[cxx_name = "supportsDisplayRename"]
        fn supports_display_rename(self: &Util) -> bool;
        #[cxx_name = "isFlatpak"]
        fn is_flatpak(self: &Util) -> bool;

        /// Описание Workshop (BBCode) → подмножество HTML для Qt.StyledText.
        ///
        /// ⚠️ В ядре тут починен баг апстрима: авто-ссылки заворачивали уже
        /// готовый якорь из `[url]` во второй `<a>`.
        #[qinvokable]
        #[cxx_name = "bbcodeToHtml"]
        fn bbcode_to_html(self: &Util, src: &QString) -> QString;

        /// Проводной цвет WE («r g b [a]», числа 0..1) → QColor.
        #[qinvokable]
        #[cxx_name = "colorFromWire"]
        fn color_from_wire(self: &Util, s: &QString) -> QColor;

        #[qinvokable]
        #[cxx_name = "colorToWire"]
        fn color_to_wire(self: &Util, color: &QColor, include_alpha: bool) -> QString;

        #[qinvokable]
        #[cxx_name = "colorHasAlpha"]
        fn color_has_alpha(self: &Util, s: &QString) -> bool;

        /// Тип обоины по-русски. Добавлено Rexon (см. шапку файла).
        #[qinvokable]
        #[cxx_name = "wpTypeLabel"]
        fn wp_type_label(self: &Util, wp_type: &QString) -> QString;
    }
}

use core::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QColor, QDateTime, QString, QUrl};

/// Состояние синглтона: рабочий стол считается ОДИН раз на старте, как в
/// оригинале (`Util::Util` читает `XDG_CURRENT_DESKTOP` в конструкторе).
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
        // Осознанная развилка Rexon, разобранная в шапке файла.
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

/// Состояния у заглушки нет: всё, что она отдаёт, — постоянные.
#[derive(Default)]
pub struct UpdateCheckerRust;

impl qobject::UpdateChecker {
    pub fn supported(&self) -> bool {
        false
    }

    pub fn enabled(&self) -> bool {
        false
    }

    /// Как у апстрима без `update-check`: запись пропускается молча, и
    /// `enabledChanged` не испускается (значение не изменилось).
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
