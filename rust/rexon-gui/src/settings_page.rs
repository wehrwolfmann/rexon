// SettingsModel — the "Settings" tab on Rust + cxx-qt + QML.
//
// Moved to QML by the shared tab scheme: decisions and labels come from the
// core, here is the QObject wrapper, and QML has only markup.
//
// What is here:
//   * the set of the tab's sections on THIS machine — the order, titles,
//     descriptions and the very decision "whether to show a section" come
//     from the core (`rexon_core::qt_settings::SECTIONS` and
//     `section_visible`);
//   * the whole catalogue of fixed labels as one map (`texts`) and argument
//     substitution into those of them that have `{0}`/`{1}`;
//   * ready sets for combo boxes and button rows: skins, file managers,
//     YouTube quality and mode, font targets, MATE buttons, types of
//     protected applications, blur limits, background sound volume;
//   * the product's standard skin palette and the state colours.
//
// What is NOT here and must not be:
//   * not a single product wording in our own words — all of them are in
//     the core's `TEXTS`. A missing label is added there, not here;
//   * not a single side action. Everything that reads or writes the system
//     (kreadconfig6/gsettings, the wallpaper engine, panel copies,
//     background sounds, installing yt-dlp) lives in the page host's
//     `actions` object (rexon/qt_settings/window.py) and returns to the page
//     values READY for display, computed by the same core through
//     `rexon_py`. No second copy of the decisions appears: one core, two
//     wrappers.
//
// Warning: the type is called `SettingsModel`, not `SettingsPage`: in a QML
// module every .qml becomes a type by file name itself, and `SettingsPage`
// is taken by `qml/SettingsPage.qml`. A clash gives "Ambiguous type
// detected".

/// cxx-qt bridge: the content of the "Settings" tab.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Window ────────────────────────────────────────────────────────
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        #[qproperty(QString, page_title, cxx_name = "pageTitle",
                    READ = page_title, CONSTANT)]
        #[qproperty(i32, page_title_point_size, cxx_name = "pageTitlePointSize",
                    READ = page_title_point_size, CONSTANT)]
        #[qproperty(i32, section_title_point_size, cxx_name = "sectionTitlePointSize",
                    READ = section_title_point_size, CONSTANT)]
        // ── Styling ───────────────────────────────────────────────────────
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Layout and colours of the tab that are not in the skin palette: the
        // group, the polkit strip, the blue and red buttons, rows, tiles, widths.
        // Some colours are "dark/light" pairs, hence the same signal as for the
        // palette.
        #[qproperty(QMap_QString_QVariant, styles, READ = styles,
                    NOTIFY = palette_changed)]
        // ── Label catalogue: key → ready string without substitutions ─────
        #[qproperty(QMap_QString_QVariant, texts, READ = texts, CONSTANT)]
        // ── Sets for lists and button rows ────────────────────────────────
        // The "Language" list: {id, label}, id is the `ui_language` value, the label
        // is the language's own name (`qt_settings::language_choices`).
        #[qproperty(QList_QVariant, languages, READ = languages, CONSTANT)]
        #[qproperty(QList_QVariant, skins, READ = skins, CONSTANT)]
        #[qproperty(QList_QVariant, fm_choices, cxx_name = "fmChoices",
                    READ = fm_choices, CONSTANT)]
        #[qproperty(QList_QVariant, yt_qualities, cxx_name = "ytQualities",
                    READ = yt_qualities, CONSTANT)]
        #[qproperty(QList_QVariant, yt_modes, cxx_name = "ytModes",
                    READ = yt_modes, CONSTANT)]
        #[qproperty(QList_QVariant, font_targets, cxx_name = "fontTargets",
                    READ = font_targets, CONSTANT)]
        #[qproperty(QList_QVariant, protection_types, cxx_name = "protectionTypes",
                    READ = protection_types, CONSTANT)]
        // ── Numbers set by the core ───────────────────────────────────────
        #[qproperty(i32, blur_min, cxx_name = "blurMin", READ = blur_min, CONSTANT)]
        #[qproperty(i32, blur_max, cxx_name = "blurMax", READ = blur_max, CONSTANT)]
        #[qproperty(i32, blur_step, cxx_name = "blurStep", READ = blur_step, CONSTANT)]
        #[qproperty(i32, blur_default, cxx_name = "blurDefault",
                    READ = blur_default, CONSTANT)]
        #[qproperty(i32, ambient_default_volume, cxx_name = "ambientDefaultVolume",
                    READ = ambient_default_volume, CONSTANT)]
        #[qproperty(i32, library_rows, cxx_name = "libraryRows",
                    READ = library_rows, CONSTANT)]
        #[qproperty(i32, library_card_width, cxx_name = "libraryCardWidth",
                    READ = library_card_width, CONSTANT)]
        #[qproperty(i32, library_preview_height, cxx_name = "libraryPreviewHeight",
                    READ = library_preview_height, CONSTANT)]
        #[qproperty(i32, sound_preview_buttons, cxx_name = "soundPreviewButtons",
                    READ = sound_preview_buttons, CONSTANT)]
        #[qproperty(i32, plasma_backups_shown, cxx_name = "plasmaBackupsShown",
                    READ = plasma_backups_shown, CONSTANT)]
        #[qproperty(i32, toast_timeout_ms, cxx_name = "toastTimeoutMs",
                    READ = toast_timeout_ms, CONSTANT)]
        #[qproperty(i32, ambient_refresh_ms, cxx_name = "ambientRefreshMs",
                    READ = ambient_refresh_ms, CONSTANT)]
        // ── Set of sections: depends on the machine, hence NOTIFY ─────────
        #[qproperty(QList_QVariant, sections, READ = sections,
                    NOTIFY = sections_changed)]
        #[qproperty(QString, de_display, cxx_name = "deDisplay",
                    READ = de_display, NOTIFY = sections_changed)]
        type SettingsModel = super::SettingsModelRust;
    }

    unsafe extern "RustQt" {
        /// The skin was changed on the fly — recompute the palette.
        ///
        /// The `palette` property is NOT `CONSTANT` precisely because of this:
        /// without a signal the QML bindings (`readonly property var pal:
        /// page.palette`) do not recompute, and an open tab stays in the previous
        /// skin's colours while the widget window is already repainted. The push
        /// comes from the shared registry (`crate::skin`), not from here.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut SettingsModel>);

        #[qsignal]
        #[cxx_name = "sectionsChanged"]
        fn sections_changed(self: Pin<&mut SettingsModel>);
    }

    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &SettingsModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &SettingsModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &SettingsModel) -> i32;
        #[cxx_name = "pageTitle"]
        fn page_title(self: &SettingsModel) -> QString;
        #[cxx_name = "pageTitlePointSize"]
        fn page_title_point_size(self: &SettingsModel) -> i32;
        #[cxx_name = "sectionTitlePointSize"]
        fn section_title_point_size(self: &SettingsModel) -> i32;
        fn palette(self: &SettingsModel) -> QMap_QString_QVariant;
        fn styles(self: &SettingsModel) -> QMap_QString_QVariant;
        fn texts(self: &SettingsModel) -> QMap_QString_QVariant;
        fn languages(self: &SettingsModel) -> QList_QVariant;
        fn skins(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "fmChoices"]
        fn fm_choices(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "ytQualities"]
        fn yt_qualities(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "ytModes"]
        fn yt_modes(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "fontTargets"]
        fn font_targets(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "protectionTypes"]
        fn protection_types(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "blurMin"]
        fn blur_min(self: &SettingsModel) -> i32;
        #[cxx_name = "blurMax"]
        fn blur_max(self: &SettingsModel) -> i32;
        #[cxx_name = "blurStep"]
        fn blur_step(self: &SettingsModel) -> i32;
        #[cxx_name = "blurDefault"]
        fn blur_default(self: &SettingsModel) -> i32;
        #[cxx_name = "ambientDefaultVolume"]
        fn ambient_default_volume(self: &SettingsModel) -> i32;
        #[cxx_name = "libraryRows"]
        fn library_rows(self: &SettingsModel) -> i32;
        #[cxx_name = "libraryCardWidth"]
        fn library_card_width(self: &SettingsModel) -> i32;
        #[cxx_name = "libraryPreviewHeight"]
        fn library_preview_height(self: &SettingsModel) -> i32;
        #[cxx_name = "soundPreviewButtons"]
        fn sound_preview_buttons(self: &SettingsModel) -> i32;
        #[cxx_name = "plasmaBackupsShown"]
        fn plasma_backups_shown(self: &SettingsModel) -> i32;
        #[cxx_name = "toastTimeoutMs"]
        fn toast_timeout_ms(self: &SettingsModel) -> i32;
        #[cxx_name = "ambientRefreshMs"]
        fn ambient_refresh_ms(self: &SettingsModel) -> i32;
        fn sections(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "deDisplay"]
        fn de_display(self: &SettingsModel) -> QString;
    }

    // ── API for QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Take the facts about the machine and recompute the set of sections.
        ///
        /// The argument is JSON from the side-actions object (`actions.facts()`):
        /// one QString instead of a dozen parameters, because from outside (from
        /// checks and from Python) only string invokables are called — for `i32`
        /// cxx-qt writes the name `::std::int32_t` into the metaobject, there is no
        /// metatype with that name, and `QMetaObject.invokeMethod` silently returns
        /// False.
        #[qinvokable]
        fn configure(self: Pin<&mut SettingsModel>, facts_json: &QString);

        /// A catalogue label with one substitution.
        #[qinvokable]
        fn t1(self: &SettingsModel, key: &QString, a: &QString) -> QString;

        /// A catalogue label with two substitutions.
        #[qinvokable]
        fn t2(self: &SettingsModel, key: &QString, a: &QString, b: &QString) -> QString;

        /// Skin description under the combo box.
        #[qinvokable]
        #[cxx_name = "skinDescription"]
        fn skin_description(self: &SettingsModel, skin_id: &QString) -> QString;

        /// The "installed / not installed" mark and its colour: {text, color}.
        #[qinvokable]
        #[cxx_name = "fmBadge"]
        fn fm_badge(self: &SettingsModel, installed: bool) -> QMap_QString_QVariant;

        /// Label of an application protection check box and its tooltip:
        /// {label, tip}. `remembered` is what is remembered (empty — nothing).
        #[qinvokable]
        #[cxx_name = "protectionRow"]
        fn protection_row(
            self: &SettingsModel,
            app_type: &QString,
            remembered: &QString,
        ) -> QMap_QString_QVariant;

        /// Icon and label of a sound event: {emoji, label}.
        #[qinvokable]
        #[cxx_name = "eventMeta"]
        fn event_meta(self: &SettingsModel, event_id: &QString) -> QMap_QString_QVariant;

        /// Label of the wallpaper volume slider.
        #[qinvokable]
        #[cxx_name = "volumeLabel"]
        fn volume_label(self: &SettingsModel, percent: i32) -> QString;

        /// Whether a library tile should show a preview button.
        #[qinvokable]
        #[cxx_name = "libraryShowsPreview"]
        fn library_shows_preview(self: &SettingsModel, format: &QString) -> bool;

        /// How many columns to put into the library grid at this width. The same
        /// calculation as for the Rexon widget grids (`ui_base::grid_columns`).
        #[qinvokable]
        #[cxx_name = "libraryColumns"]
        fn library_columns(self: &SettingsModel, viewport_width: i32) -> i32;

        /// How many tiles to show with this number of columns.
        #[qinvokable]
        #[cxx_name = "libraryPageSize"]
        fn library_page_size(self: &SettingsModel, cols: i32) -> i32;

        /// Width of the pop-up message banner at this page width and this
        /// single-line text width. The same calculation as for the widget banner
        /// (`ui_base::toast_width`); the paddings and the cap come from the core.
        #[qinvokable]
        #[cxx_name = "toastWidth"]
        fn toast_width(self: &SettingsModel, page_width: i32, text_width: i32) -> i32;
    }

    // Trait declarations must be IN THE BRIDGE: cxx-qt emits both the
    // constructor and `qt_thread()` from these lines.
    impl cxx_qt::Initialize for SettingsModel {}
    // Subscribing to skin changes goes through `CxxQtThread::queue`, and the
    // thread trait must be declared IN THE BRIDGE: cxx-qt emits both
    // `qt_thread()` itself and its type alias from this line.
    impl cxx_qt::Threading for SettingsModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::qt_settings as qs;
use rexon_core::ui_base;

/// Width of a wallpaper library tile. CONSTANT: the shared grid engine
/// computes the columns from it — a "from–to" range gave tiles of different
/// widths in one row, and there simply was no "card width" to compute by.
pub const LIB_CARD_W: i32 = 260;

/// Height of the tile's preview strip.
pub const LIB_PREVIEW_H: i32 = 108;

/// How often the list of playing background sounds is re-read.
pub const AMBIENT_REFRESH_MS: i32 = 3000;

/// Minimum size of the tab window.
pub const WINDOW_MIN_SIZE: (i32, i32) = (900, 720);

/// Font size of the page title and of the section title.
pub const PAGE_TITLE_POINT_SIZE: i32 = 20;
pub const SECTION_TITLE_POINT_SIZE: i32 = 13;

/// Build a QVariantMap from pairs: QML reads such a map with a dot
/// (`row.label`).
fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

/// Frame of a group section (`sty.group`): background, border, rounding,
/// paddings, spacing. One for the tabs that have group sections: "Settings"
/// and "Appearance" (the "MATE panel" section moved from one to the other).
pub(crate) fn group_style() -> QMap<QMapPair_QString_QVariant> {
    let col = |css: &str| qstr(&ui_base::css_color_to_qml(css));
    let num = |v: i32| QVariant::from(&v);
    let (group_bg, group_border) = qs::group_colors();
    vmap(vec![
        ("background", col(&group_bg)),
        ("border", col(&group_border)),
        ("radius", num(qs::GROUP_RADIUS)),
        ("marginH", num(qs::GROUP_MARGIN_H)),
        ("marginV", num(qs::GROUP_MARGIN_V)),
        ("spacing", num(qs::GROUP_SPACING)),
    ])
}

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// Rust state of the page: only the observed facts about the machine.
///
/// Everything else is core constants, and keeping a copy of them in the
/// state would mean creating a second source of truth.
#[derive(Default)]
pub struct SettingsModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    /// 0 means the page is not subscribed yet.
    skin_watch: u64,
    facts: qs::SectionFacts,
    de_display: String,
    configured: bool,
}

/// Remove the skin change subscription. Without this the registry would
/// keep the closed tab's entry until the next broadcast — that one would
/// filter it out by `ObjectDestroyed`, but there is no reason to accumulate
/// garbage.
impl Drop for SettingsModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

/// The only thing the constructor does is subscribe the page to skin
/// changes: the rest of the state comes from the page host (`setFacts`).
impl cxx_qt::Initialize for qobject::SettingsModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Subscribing to skin changes: one line per page, the broadcast is shared
        // (`crate::skin`). Removed in `Drop` of the same state.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::SettingsModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

/// Parse the facts sent by the page host.
///
/// An unknown or broken JSON is a breakage of the seam, not the customer's
/// choice: the sections are then shown at a safe minimum (only those that
/// always exist), and the page stays working.
fn parse_facts(raw: &str) -> (qs::SectionFacts, String) {
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return (qs::SectionFacts::default(), String::new()),
    };
    let flag = |key: &str| value.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    let de_display = value
        .get("deDisplay")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    (
        qs::SectionFacts::new(
            flag("onboarding"),
            flag("plasma"),
            flag("aeroshell"),
        ),
        de_display,
    )
}

impl qobject::SettingsModel {
    // ── Window ────────────────────────────────────────────────────────────

    fn window_title(&self) -> QString {
        QString::from(&qs::text("window.title", &[]).unwrap_or_default())
    }

    fn window_min_width(&self) -> i32 {
        WINDOW_MIN_SIZE.0
    }

    fn window_min_height(&self) -> i32 {
        WINDOW_MIN_SIZE.1
    }

    fn page_title(&self) -> QString {
        QString::from(&qs::text("page.title", &[]).unwrap_or_default())
    }

    fn page_title_point_size(&self) -> i32 {
        PAGE_TITLE_POINT_SIZE
    }

    fn section_title_point_size(&self) -> i32 {
        SECTION_TITLE_POINT_SIZE
    }

    // ── Styling ───────────────────────────────────────────────────────────

    /// Palette of the ACTIVE skin (the saved `ui_skin`, falling back to the
    /// standard one by rule #17) plus the tab's state colours. The page defines
    /// no colours of its own.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        out.insert(QString::from("state_ok"), qstr(qs::COLOR_OK));
        out.insert(QString::from("state_bad"), qstr(qs::COLOR_BAD));
        out.insert(QString::from("state_warn"), qstr(qs::COLOR_WARN));
        // Secondary and muted labels are set bolder (`font-weight: 600` from the
        // widgets' `secondary_text_qss`); the number comes from the core, the key
        // is the same as in "Compare" and "Extensions".
        out.insert(
            QString::from("text_secondary_weight"),
            QVariant::from(&ui_base::SECONDARY_TEXT_WEIGHT),
        );
        // Dimming under a modal window is a product-wide decision: the tutorial
        // and "DE comparison" dim the window with the same colour.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        // The "Yes" colours of the operation window (installing what is missing) —
        // with the same helper over the core table as for "Desktops" and
        // "Compare".
        crate::desktops_page::insert_operation_tones(&mut out);
        // The slider groove (`track`) comes with the shared palette
        // (`ui_base::qml_palette`).
        // The colour of an IRREVERSIBLE action ("Stop live wallpaper", "Stop all
        // sounds"). The hue and both opacities arrive from the core, and the
        // button itself is the shared `RexonDangerButton` type: no third copy of
        // the "red button" expression appears in the markup (rule #12).
        out.insert(QString::from("danger"), qstr(qs::COLOR_BAD));
        out.insert(
            QString::from("danger_fill_alpha"),
            QVariant::from(&qs::DANGER_FILL_ALPHA),
        );
        out.insert(
            QString::from("danger_fill_hover_alpha"),
            QVariant::from(&qs::DANGER_FILL_HOVER_ALPHA),
        );
        out
    }

    /// Layout and colours of the widget tab that are not in the skin palette.
    ///
    /// All numbers come from the "Layout and styling" of `qt_settings`; the QML
    /// markup defines none of its own. Colours go through `css_color_to_qml`:
    /// QML reads the `rgba(...)` notation as an invalid colour and draws it
    /// black.
    fn styles(&self) -> QMap<QMapPair_QString_QVariant> {
        let dark = ui_base::active_skin_is_dark();
        let col = |css: &str| qstr(&ui_base::css_color_to_qml(css));
        let num = |v: i32| QVariant::from(&v);
        let real = |v: f64| QVariant::from(&v);
        let yes = QVariant::from(&true);

        let page = vmap(vec![
            ("margin", num(qs::PAGE_MARGIN)),
            ("spacing", num(qs::PAGE_SPACING)),
        ]);

        let group = group_style();

        let (banner_bg, banner_border) = qs::polkit_banner_colors();
        let banner = vmap(vec![
            ("background", col(&banner_bg)),
            ("border", col(&banner_border)),
            ("radius", num(qs::POLKIT_BANNER_RADIUS)),
            ("padding", num(qs::POLKIT_BANNER_PADDING)),
        ]);

        // Tone of the blue action button — a map that `RexonButton` understands.
        let btn = qs::primary_button_colors();
        let primary = vmap(vec![
            ("background", col(&btn.background)),
            ("hover", col(&btn.hover)),
            ("pressed", col(&btn.pressed)),
            ("border", col(&btn.border)),
            ("disabled", col(&btn.disabled_background)),
            ("disabledBorder", col(&btn.disabled_border)),
            ("disabledColor", col(&btn.disabled_text)),
            ("radius", num(qs::PRIMARY_BTN_RADIUS)),
            ("paddingH", num(qs::PRIMARY_BTN_PADDING_H)),
            ("paddingV", num(qs::PRIMARY_BTN_PADDING_V)),
            ("bold", yes.clone()),
        ]);

        // Tone of the red stop button — a map for `RexonDangerButton`: the hue and
        // fill are already in the palette (`danger*`); here are the label, the
        // weight, the grey disabled state and the paddings.
        let (danger_off_bg, danger_off_text) = qs::danger_button_disabled_colors();
        let danger = vmap(vec![
            ("textColor", qstr(qs::COLOR_BAD)),
            ("bold", yes.clone()),
            ("disabledBackground", col(&danger_off_bg)),
            ("disabledColor", col(&danger_off_text)),
            ("paddingH", num(qs::DANGER_BTN_PADDING_H)),
            ("paddingV", num(qs::DANGER_BTN_PADDING_V)),
        ]);

        // Tone of the "⏹" button in a sound row.
        let ambient_stop = vmap(vec![
            ("color", qstr(qs::AMBIENT_STOP_COLOR)),
            ("fillAlpha", real(qs::AMBIENT_STOP_FILL_ALPHA)),
            ("fillHoverAlpha", real(qs::AMBIENT_STOP_FILL_HOVER_ALPHA)),
            ("paddingH", num(qs::AMBIENT_STOP_PADDING_H)),
            ("paddingV", num(qs::AMBIENT_STOP_PADDING_V)),
        ]);

        let fm_row = vmap(vec![
            ("marginH", num(qs::FM_ROW_MARGIN_H)),
            ("marginV", num(qs::FM_ROW_MARGIN_V)),
            ("spacing", num(qs::FM_ROW_SPACING)),
            ("radius", num(qs::FM_ROW_RADIUS)),
            ("focus", col(&qs::fm_row_focus_color())),
        ]);

        let widths = vmap(vec![
            ("fieldLabel", num(qs::FIELD_LABEL_MIN_WIDTH)),
            ("volumeLabel", num(qs::VOLUME_LABEL_MIN_WIDTH)),
            ("url", num(qs::URL_LABEL_MIN_WIDTH)),
            ("blurLabel", num(qs::BLUR_LABEL_MIN_WIDTH)),
            ("blurValue", num(qs::BLUR_VALUE_MIN_WIDTH)),
            ("ambientPercent", num(qs::AMBIENT_PERCENT_MIN_WIDTH)),
            ("volumeSlider", num(qs::VOLUME_SLIDER_MAX_WIDTH)),
            ("ambientSlider", num(qs::AMBIENT_SLIDER_MAX_WIDTH)),
            ("ambientButton", num(qs::AMBIENT_ROW_BUTTON_WIDTH)),
            ("libPreviewButton", num(qs::LIB_PREVIEW_BUTTON_WIDTH)),
        ]);

        let ambient = vmap(vec![
            ("listTopMargin", num(qs::AMBIENT_LIST_TOP_MARGIN)),
            ("listSpacing", num(qs::AMBIENT_LIST_SPACING)),
            ("rowMarginH", num(qs::AMBIENT_ROW_MARGIN_H)),
            ("rowMarginV", num(qs::AMBIENT_ROW_MARGIN_V)),
            ("rowSpacing", num(qs::AMBIENT_ROW_SPACING)),
            ("rowRadius", num(qs::AMBIENT_ROW_RADIUS)),
            ("rowBackground", col(&qs::ambient_row_color(dark))),
        ]);

        let (lib_border, lib_hover) = qs::lib_card_colors(dark);
        let lib = vmap(vec![
            ("gridSpacing", num(qs::LIB_GRID_SPACING)),
            ("gridTopMargin", num(qs::LIB_GRID_TOP_MARGIN)),
            ("cardRadius", num(qs::LIB_CARD_RADIUS)),
            ("cardBorder", col(&lib_border)),
            ("cardHover", col(&lib_hover)),
            ("infoMarginH", num(qs::LIB_INFO_MARGIN_H)),
            ("infoMarginV", num(qs::LIB_INFO_MARGIN_V)),
            ("infoSpacing", num(qs::LIB_INFO_SPACING)),
            ("buttonSpacing", num(qs::LIB_BUTTON_SPACING)),
            ("placeholderBackground", col(&qs::lib_placeholder_color(dark))),
            ("placeholderPointSize", num(qs::LIB_PLACEHOLDER_POINT_SIZE)),
            ("emptyPadding", num(qs::LIB_EMPTY_PADDING)),
        ]);

        // Pop-up message. Colours and metrics have ONE door for the whole product
        // (`crate::toast`), and the banner markup is also one
        // (`qml/RexonToast.qml`): there is nowhere for a banner of its own to
        // appear in the tab.
        let toast = crate::toast::style_map(dark);

        let spacing = vmap(vec![
            ("soundPreview", num(qs::SOUND_PREVIEW_SPACING)),
            ("fontRow", num(qs::FONT_ROW_SPACING)),
            ("fontCol", num(qs::FONT_COL_SPACING)),
            ("plasmaRow", num(qs::PLASMA_ROW_SPACING)),
            ("infoBox", num(qs::INFO_BOX_SPACING)),
        ]);

        vmap(vec![
            ("page", QVariant::from(&page)),
            ("group", QVariant::from(&group)),
            ("banner", QVariant::from(&banner)),
            ("primary", QVariant::from(&primary)),
            ("danger", QVariant::from(&danger)),
            ("ambientStop", QVariant::from(&ambient_stop)),
            ("fmRow", QVariant::from(&fm_row)),
            ("widths", QVariant::from(&widths)),
            ("ambient", QVariant::from(&ambient)),
            ("lib", QVariant::from(&lib)),
            ("toast", QVariant::from(&toast)),
            ("spacing", QVariant::from(&spacing)),
        ])
    }

    // ── Label catalogue ───────────────────────────────────────────────────

    /// All fixed strings of the tab as one map: `page.texts["fm.apply"]`.
    ///
    /// Strings with substitution places (`{0}`) get here as is — they are
    /// assembled by `t1`/`t2`. That way the page has not a single wording of its
    /// own, and a forgotten key shows up at once as an empty label.
    fn texts(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        // Templates go out translated: the core table holds English `msgid`s.
        for (key, _) in qs::TEXTS {
            out.insert(
                QString::from(*key),
                qstr(&qs::text_template(key).unwrap_or_default()),
            );
        }
        // Labels the tab does not store as a string but assembles: the "take the
        // tutorial again" button is labelled by the tutorial itself.
        out.insert(
            QString::from("ob.replay"),
            qstr(&qs::onboarding_replay_button()),
        );
        out
    }

    fn t1(&self, key: &QString, a: &QString) -> QString {
        QString::from(&qs::text(&key.to_string(), &[a.to_string()]).unwrap_or_default())
    }

    fn t2(&self, key: &QString, a: &QString, b: &QString) -> QString {
        QString::from(
            &qs::text(&key.to_string(), &[a.to_string(), b.to_string()]).unwrap_or_default(),
        )
    }

    // ── Sets ──────────────────────────────────────────────────────────────

    fn languages(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (id, label) in qs::language_choices() {
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&id)),
                ("label", qstr(&label)),
            ])));
        }
        out
    }

    fn skins(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (id, label) in ui_base::available_skins() {
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(id)),
                ("label", qstr(&label)),
            ])));
        }
        out
    }

    fn fm_choices(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (id, label) in qs::fm_choices() {
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&id)),
                ("label", qstr(&label)),
            ])));
        }
        out
    }

    fn yt_qualities(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (label, value) in qs::yt_qualities() {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(&label)),
                ("value", qstr(value)),
            ])));
        }
        out
    }

    fn yt_modes(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (label, value) in qs::yt_modes() {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(&label)),
                ("value", qstr(value)),
            ])));
        }
        out
    }

    /// The page's font targets (`qt_settings::font_targets_shown`): the title
    /// font button is not shown where titles are drawn by a window manager we
    /// do not configure (COSMIC, LXQt with an unrecognized manager,
    /// sway/Hyprland/niri) — otherwise "✅ applied" with zero effect. Fonts this
    /// environment does not have ("small", "toolbar", "menu" outside KDE) are
    /// visible but disabled and honestly labelled: `available = false`.
    fn font_targets(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        let de = crate::settings_actions::current_de();
        for (target, available) in qs::font_targets_shown_wm(&de, crate::settings_actions::title_wm(&de)) {
            let (label, tip, title) = qs::font_target_texts(target);
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(target)),
                ("label", qstr(&label)),
                ("tip", qstr(&tip)),
                ("title", qstr(&title)),
                ("available", QVariant::from(&available)),
            ])));
        }
        out
    }

    fn protection_types(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for key in qs::protection_types() {
            out.append(QVariant::from(&vmap(vec![("id", qstr(key))])));
        }
        out
    }

    // ── Core numbers ──────────────────────────────────────────────────────

    fn blur_min(&self) -> i32 {
        qs::BLUR_RANGE.0
    }

    fn blur_max(&self) -> i32 {
        qs::BLUR_RANGE.1
    }

    fn blur_step(&self) -> i32 {
        qs::BLUR_RANGE.2
    }

    fn blur_default(&self) -> i32 {
        qs::BLUR_RANGE.3
    }

    fn ambient_default_volume(&self) -> i32 {
        qs::AMBIENT_DEFAULT_VOLUME
    }

    fn library_rows(&self) -> i32 {
        qs::LIBRARY_ROWS as i32
    }

    fn library_card_width(&self) -> i32 {
        LIB_CARD_W
    }

    fn library_preview_height(&self) -> i32 {
        LIB_PREVIEW_H
    }

    fn sound_preview_buttons(&self) -> i32 {
        qs::SOUND_PREVIEW_BUTTONS as i32
    }

    fn plasma_backups_shown(&self) -> i32 {
        qs::PLASMA_BACKUPS_SHOWN as i32
    }

    fn toast_timeout_ms(&self) -> i32 {
        crate::toast::timeout_ms()
    }

    fn ambient_refresh_ms(&self) -> i32 {
        AMBIENT_REFRESH_MS
    }

    // ── Set of sections ───────────────────────────────────────────────────

    /// Sections of this machine: [{id, title, description}] in core order.
    ///
    /// Before `configure` is called the list is empty: drawing sections without
    /// asking the machine means showing the customer items they do not have.
    fn sections(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        if !self.rust().configured {
            return out;
        }
        for id in qs::visible_sections(&self.rust().facts) {
            // A section is in the list — so it has texts too: `section_texts` fails to
            // find only a foreign frame name.
            let Some((title, description)) = qs::section_texts(id, &self.rust().de_display) else {
                continue;
            };
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(id)),
                ("title", qstr(&title)),
                ("description", qstr(&description)),
            ])));
        }
        out
    }

    fn de_display(&self) -> QString {
        QString::from(&self.rust().de_display)
    }

    fn configure(mut self: Pin<&mut Self>, facts_json: &QString) {
        let (facts, de_display) = parse_facts(&facts_json.to_string());
        let changed = !self.rust().configured
            || self.rust().facts != facts
            || self.rust().de_display != de_display;
        {
            let mut rust = self.as_mut().rust_mut();
            rust.facts = facts;
            rust.de_display = de_display;
            rust.configured = true;
        }
        if changed {
            self.sections_changed();
        }
    }

    // ── Ready pieces of content ───────────────────────────────────────────

    fn skin_description(&self, skin_id: &QString) -> QString {
        QString::from(ui_base::skin_description(&skin_id.to_string()))
    }

    fn fm_badge(&self, installed: bool) -> QMap<QMapPair_QString_QVariant> {
        vmap(vec![
            ("text", qstr(&qs::fm_badge(installed))),
            ("color", qstr(qs::fm_badge_color(installed))),
        ])
    }

    fn protection_row(
        &self,
        app_type: &QString,
        remembered: &QString,
    ) -> QMap<QMapPair_QString_QVariant> {
        let (label, tip) =
            qs::protection_row_texts(&app_type.to_string(), &remembered.to_string());
        vmap(vec![("label", qstr(&label)), ("tip", qstr(&tip))])
    }

    fn event_meta(&self, event_id: &QString) -> QMap<QMapPair_QString_QVariant> {
        let (emoji, label) = qs::event_meta(&event_id.to_string());
        vmap(vec![("emoji", qstr(&emoji)), ("label", qstr(&label))])
    }

    fn volume_label(&self, percent: i32) -> QString {
        QString::from(&qs::wallpaper_volume_label(percent as i64))
    }

    fn library_shows_preview(&self, format: &QString) -> bool {
        qs::library_shows_preview(&format.to_string())
    }

    fn library_columns(&self, viewport_width: i32) -> i32 {
        // Grid margins and the gap between tiles — the same as in the widget
        // layout: 0 padding inside the section frame and 8 px between tiles. The
        // spacing of 8 and zero margins are the same numbers as in the widget
        // library grid, so both paths give the same number of columns.
        ui_base::grid_columns(viewport_width.max(0), LIB_CARD_W, 8, 0, 1, 99) as i32
    }

    fn library_page_size(&self, cols: i32) -> i32 {
        ui_base::grid_page_size(cols.max(0) as u32, qs::LIBRARY_ROWS as u32) as i32
    }

    fn toast_width(&self, page_width: i32, text_width: i32) -> i32 {
        crate::toast::width(page_width, text_width)
    }
}
