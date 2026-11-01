// SettingsModel — вкладка «Настройки» на Rust + cxx-qt + QML.
//
// Перенесена на QML по общей схеме вкладок: решения и подписи — ядро,
// здесь оболочка QObject, в QML только разметка.
//
// Что здесь есть:
//   * состав разделов вкладки на ЭТОЙ машине — порядок, заголовки, описания
//     и само решение «показывать ли раздел» приезжают из ядра
//     (`rexon_core::qt_settings::SECTIONS` и `section_visible`);
//   * весь каталог постоянных подписей одной картой (`texts`) и подстановка
//     аргументов в те из них, где есть `{0}`/`{1}`;
//   * готовые наборы для выпадающих списков и рядов кнопок: скины, файловые
//     менеджеры, качество и режим YouTube, цели шрифтов, кнопки MATE, типы
//     защищаемых приложений, границы размытия, громкость фоновых звуков;
//   * палитра стандартного скина продукта и цвета состояний.
//
// Чего здесь НЕТ и быть не должно:
//   * ни одной продуктовой формулировки своими словами — все они в `TEXTS`
//     ядра. Подпись, которой нет, добавляется туда, а не сюда;
//   * ни одного побочного действия. Всё, что читает или пишет систему
//     (kreadconfig6/gsettings, движок обоев, копии панели, фоновые звуки,
//     установка yt-dlp), живёт в объекте `actions` хозяина страницы
//     (rexon/qt_settings/window.py) и возвращает странице ГОТОВЫЕ к показу
//     значения, посчитанные тем же ядром через `rexon_py`. Второй копии
//     решений не появляется: ядро одно, обёрток две.
//
// ⚠️ Тип зовётся `SettingsModel`, а не `SettingsPage`: в QML-модуле каждый
// .qml сам становится типом по имени файла, и `SettingsPage` занят
// `qml/SettingsPage.qml`. Совпадение даёт «Ambiguous type detected».

/// Мост cxx-qt: содержимое вкладки «Настройки».
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
        // ── Окно ──────────────────────────────────────────────────────────
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
        // ── Оформление ────────────────────────────────────────────────────
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Раскладка и краски вкладки, которых нет в палитре скина: группа,
        // полоса polkit, синяя и красные кнопки, строки, плитки, ширины.
        // Часть красок — пары «тёмная/светлая», поэтому тот же сигнал, что
        // у палитры.
        #[qproperty(QMap_QString_QVariant, styles, READ = styles,
                    NOTIFY = palette_changed)]
        // ── Каталог подписей: ключ → готовая строка без подстановок ───────
        #[qproperty(QMap_QString_QVariant, texts, READ = texts, CONSTANT)]
        // ── Наборы для списков и рядов кнопок ─────────────────────────────
        // Список «Язык»: {id, label}, id — значение `ui_language`, подпись —
        // самоназвание языка (`qt_settings::language_choices`).
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
        #[qproperty(QList_QVariant, mate_buttons, cxx_name = "mateButtons",
                    READ = mate_buttons, CONSTANT)]
        #[qproperty(QList_QVariant, protection_types, cxx_name = "protectionTypes",
                    READ = protection_types, CONSTANT)]
        // Ссылки на разделы, переехавшие в «Оформление» (01.11.2026):
        // {key, text}, ключ — раздел боковой панели «Оформления»
        // (`categories::moved_settings_links`), и пояснение над ними.
        #[qproperty(QList_QVariant, moved_panels, cxx_name = "movedPanels",
                    READ = moved_panels, CONSTANT)]
        #[qproperty(QString, moved_heading, cxx_name = "movedHeading",
                    READ = moved_heading, CONSTANT)]
        // ── Числа, заданные ядром ─────────────────────────────────────────
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
        // ── Состав разделов: зависит от машины, поэтому NOTIFY ────────────
        #[qproperty(QList_QVariant, sections, READ = sections,
                    NOTIFY = sections_changed)]
        #[qproperty(QString, de_display, cxx_name = "deDisplay",
                    READ = de_display, NOTIFY = sections_changed)]
        type SettingsModel = super::SettingsModelRust;
    }

    unsafe extern "RustQt" {
        /// Скин сменили на лету — палитру пересчитать.
        ///
        /// Свойство `palette` НЕ `CONSTANT` именно поэтому: без сигнала
        /// привязки QML (`readonly property var pal: page.palette`) не
        /// пересчитываются, и открытая вкладка остаётся в цветах прошлого
        /// скина, пока окно на виджетах уже перекрашено. Толчок приходит из
        /// общего реестра (`crate::skin`), а не отсюда.
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
        #[cxx_name = "mateButtons"]
        fn mate_buttons(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "protectionTypes"]
        fn protection_types(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "movedPanels"]
        fn moved_panels(self: &SettingsModel) -> QList_QVariant;
        #[cxx_name = "movedHeading"]
        fn moved_heading(self: &SettingsModel) -> QString;
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

    // ── API для QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Принять факты о машине и пересчитать состав разделов.
        ///
        /// Аргумент — JSON от объекта побочных действий (`actions.facts()`):
        /// один QString вместо десятка параметров, потому что снаружи (из
        /// проверок и из Python) зовутся только строковые инвокаблы — у `i32`
        /// в метаобъекте cxx-qt пишет имя `::std::int32_t`, метатипа с таким
        /// именем нет, и `QMetaObject.invokeMethod` молча возвращает False.
        #[qinvokable]
        fn configure(self: Pin<&mut SettingsModel>, facts_json: &QString);

        /// Подпись из каталога с одной подстановкой.
        #[qinvokable]
        fn t1(self: &SettingsModel, key: &QString, a: &QString) -> QString;

        /// Подпись из каталога с двумя подстановками.
        #[qinvokable]
        fn t2(self: &SettingsModel, key: &QString, a: &QString, b: &QString) -> QString;

        /// Описание скина под выпадающим списком.
        #[qinvokable]
        #[cxx_name = "skinDescription"]
        fn skin_description(self: &SettingsModel, skin_id: &QString) -> QString;

        /// Отметка «установлен / не установлен» и её цвет: {text, color}.
        #[qinvokable]
        #[cxx_name = "fmBadge"]
        fn fm_badge(self: &SettingsModel, installed: bool) -> QMap_QString_QVariant;

        /// Подпись галочки защиты приложения и подсказка к ней:
        /// {label, tip}. `remembered` — что запомнено (пусто — ничего).
        #[qinvokable]
        #[cxx_name = "protectionRow"]
        fn protection_row(
            self: &SettingsModel,
            app_type: &QString,
            remembered: &QString,
        ) -> QMap_QString_QVariant;

        /// Значок и подпись звукового события: {emoji, label}.
        #[qinvokable]
        #[cxx_name = "eventMeta"]
        fn event_meta(self: &SettingsModel, event_id: &QString) -> QMap_QString_QVariant;

        /// Подпись ползунка громкости обоев.
        #[qinvokable]
        #[cxx_name = "volumeLabel"]
        fn volume_label(self: &SettingsModel, percent: i32) -> QString;

        /// Показывать ли плитке библиотеки кнопку предпросмотра.
        #[qinvokable]
        #[cxx_name = "libraryShowsPreview"]
        fn library_shows_preview(self: &SettingsModel, format: &QString) -> bool;

        /// Сколько столбцов положить в сетку библиотеки при такой ширине.
        /// Тот же расчёт, что у виджетных сеток Rexon (`ui_base::grid_columns`).
        #[qinvokable]
        #[cxx_name = "libraryColumns"]
        fn library_columns(self: &SettingsModel, viewport_width: i32) -> i32;

        /// Сколько плиток показать при таком числе столбцов.
        #[qinvokable]
        #[cxx_name = "libraryPageSize"]
        fn library_page_size(self: &SettingsModel, cols: i32) -> i32;

        /// Ширина плашки всплывающего сообщения при такой ширине страницы и
        /// такой ширине текста в одну строку. Тот же расчёт, что у виджетной
        /// плашки (`ui_base::toast_width`), поля и потолок — из ядра.
        #[qinvokable]
        #[cxx_name = "toastWidth"]
        fn toast_width(self: &SettingsModel, page_width: i32, text_width: i32) -> i32;
    }

    // Объявление трейтов обязано быть В МОСТЕ: cxx-qt печатает по этим
    // строкам и конструктор, и `qt_thread()`.
    impl cxx_qt::Initialize for SettingsModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`, и трейт
    // потока обязан быть объявлен В МОСТЕ: cxx-qt печатает по этой
    // строке и сам `qt_thread()`, и его алиас типа.
    impl cxx_qt::Threading for SettingsModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::qt_settings as qs;
use rexon_core::ui_base;

/// Ширина плитки библиотеки обоев. ПОСТОЯННАЯ: по ней общий движок сеток
/// считает столбцы — вилка «от и до» давала в одном ряду плитки разной
/// ширины, и «ширины карточки», по которой можно считать, просто не было.
pub const LIB_CARD_W: i32 = 260;

/// Высота полоски предпросмотра плитки.
pub const LIB_PREVIEW_H: i32 = 108;

/// Как часто перечитывается список играющих фоновых звуков.
pub const AMBIENT_REFRESH_MS: i32 = 3000;

/// Минимальный размер окна вкладки.
pub const WINDOW_MIN_SIZE: (i32, i32) = (900, 720);

/// Кегль заголовка страницы и заголовка раздела.
pub const PAGE_TITLE_POINT_SIZE: i32 = 20;
pub const SECTION_TITLE_POINT_SIZE: i32 = 13;

/// Собрать QVariantMap из пар: QML читает такую карту точкой (`row.label`).
fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

/// Рамка раздела-группы (`sty.group`): фон, рамка, скругление, поля,
/// промежуток. Одна на вкладки, где стоят разделы-группы: «Настройки» и
/// «Оформление» (раздел «Панель MATE» переехал из одной в другую).
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

/// Rust-состояние страницы: только наблюдаемые факты о машине.
///
/// Всё остальное — константы ядра, и держать их копию в состоянии значило бы
/// завести второй источник истины.
#[derive(Default)]
pub struct SettingsModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    /// 0 — страница ещё не подписана.
    skin_watch: u64,
    facts: qs::SectionFacts,
    de_display: String,
    configured: bool,
}

/// Снять подписку на смену скина. Без этого реестр держал бы запись
/// закрытой вкладки до первой рассылки — та её отсеет по
/// `ObjectDestroyed`, но копить мусор незачем.
impl Drop for SettingsModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

/// Единственное, что делает конструктор, — подписывает страницу на смену
/// скина: остальное состояние приходит от хозяина страницы (`setFacts`).
impl cxx_qt::Initialize for qobject::SettingsModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Подписка на смену скина: одна строка на страницу, рассылка — общая
        // (`crate::skin`). Снимается в `Drop` того же состояния.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::SettingsModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

/// Разобрать факты, присланные хозяином страницы.
///
/// Неизвестный или битый JSON — это поломка шва, а не выбор покупателя:
/// разделы тогда показываются по безопасному минимуму (только те, что есть
/// всегда), и страница остаётся рабочей.
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
            flag("mate"),
            flag("plasma"),
            flag("aeroshell"),
        ),
        de_display,
    )
}

impl qobject::SettingsModel {
    // ── Окно ──────────────────────────────────────────────────────────────

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

    // ── Оформление ────────────────────────────────────────────────────────

    /// Палитра ДЕЙСТВУЮЩЕГО скина (сохранённый `ui_skin`, откат к
    /// стандартному по правилу #17) плюс цвета состояний вкладки. Своих
    /// цветов страница не заводит.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        out.insert(QString::from("state_ok"), qstr(qs::COLOR_OK));
        out.insert(QString::from("state_bad"), qstr(qs::COLOR_BAD));
        out.insert(QString::from("state_warn"), qstr(qs::COLOR_WARN));
        // Вторичные и приглушённые подписи набраны плотнее (`font-weight:
        // 600` из `secondary_text_qss` у виджетов); число из ядра, ключ тот
        // же, что у «Сравнения» и «Расширений».
        out.insert(
            QString::from("text_secondary_weight"),
            QVariant::from(&ui_base::SECONDARY_TEXT_WEIGHT),
        );
        // Затемнение под модальным окном — общее решение продукта: тем же
        // цветом гасит окно обучение и «Сравнение DE».
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        // Краски «Да» окна операции (доустановка недостающего) — тем же
        // помощником по таблице ядра, что у «Рабочих столов» и «Сравнения».
        crate::desktops_page::insert_operation_tones(&mut out);
        // Подложка неактивной части ползунка: та же нейтральная заливка, что
        // у виджетных карточек, чтобы полоса читалась на любом скине.
        // ⚠️ Через переводчик: `rgba(...)` — запись QSS, QML такой цвет
        // читает как недопустимый и рисует ЧЁРНЫМ (см. `css_color_to_qml`).
        out.insert(
            QString::from("track"),
            qstr(&ui_base::css_color_to_qml(&ui_base::neutral_rgba(0.18))),
        );
        // Цвет НЕОБРАТИМОГО действия («Остановить живые обои», «Остановить
        // все звуки»). Оттенок и обе прозрачности приезжают из ядра, а сама
        // кнопка — общий тип `RexonDangerButton`: третьей копии выражения
        // «красная кнопка» в разметке не появляется (правило #12).
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

    /// Раскладка и краски виджетной вкладки, которых нет в палитре скина.
    ///
    /// Все числа — из «Раскладки и оформления» `qt_settings`; QML-разметка
    /// своих не заводит. Краски идут через `css_color_to_qml`: запись
    /// `rgba(...)` QML читает как недопустимый цвет и рисует чёрным.
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

        // Тон синей кнопки действия — карта, которую понимает `RexonButton`.
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

        // Тон красной кнопки остановки — карта для `RexonDangerButton`:
        // оттенок и заливка уже в палитре (`danger*`), здесь подпись,
        // жирность, серое выключенное состояние и поля.
        let (danger_off_bg, danger_off_text) = qs::danger_button_disabled_colors();
        let danger = vmap(vec![
            ("textColor", qstr(qs::COLOR_BAD)),
            ("bold", yes.clone()),
            ("disabledBackground", col(&danger_off_bg)),
            ("disabledColor", col(&danger_off_text)),
            ("paddingH", num(qs::DANGER_BTN_PADDING_H)),
            ("paddingV", num(qs::DANGER_BTN_PADDING_V)),
        ]);

        // Тон кнопки «⏹» в строке звука.
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

        // Всплывающее сообщение. Краски и метрики — ОДНА дверь на весь
        // продукт (`crate::toast`), и разметка у плашки тоже одна
        // (`qml/RexonToast.qml`): своей плашке во вкладке завестись негде.
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

    // ── Каталог подписей ──────────────────────────────────────────────────

    /// Все постоянные строки вкладки одной картой: `page.texts["fm.apply"]`.
    ///
    /// Строки с местами подстановки (`{0}`) сюда попадают как есть — их
    /// собирают `t1`/`t2`. Так у страницы нет ни одной собственной
    /// формулировки, а забытый ключ виден сразу пустой надписью.
    fn texts(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        // Шаблоны уходят переведёнными: в таблице ядра — английские `msgid`.
        for (key, _) in qs::TEXTS {
            out.insert(
                QString::from(*key),
                qstr(&qs::text_template(key).unwrap_or_default()),
            );
        }
        // Подписи, которые вкладка не хранит строкой, а собирает: кнопка
        // «пройти обучение заново» подписана самим обучением.
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

    // ── Наборы ────────────────────────────────────────────────────────────

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

    /// Цели шрифтов страницы (`qt_settings::font_targets_shown`): кнопку
    /// шрифта заголовков там, где заголовки рисует не настраиваемый нами
    /// оконный менеджер (COSMIC, LXQt с неопознанным менеджером,
    /// sway/Hyprland/niri), не показываем —
    /// иначе «✅ применено» при нулевом эффекте. Шрифты, которых в этом
    /// окружении нет («маленький», «панели инструментов», «меню» вне KDE),
    /// видны, но выключены и подписаны честно: `available = false`.
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

    fn mate_buttons(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (label, action) in qs::mate_buttons() {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(&label)),
                ("action", qstr(&action)),
            ])));
        }
        out
    }

    /// Переехавшие в «Оформление» разделы — для запущенного стола: те же,
    /// что видны в его боковой панели.
    fn moved_panels(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        let de = crate::settings_actions::current_de();
        for (key, text) in rexon_core::categories::moved_settings_links(&de) {
            out.append(QVariant::from(&vmap(vec![("key", qstr(&key)), ("text", qstr(&text))])));
        }
        out
    }

    fn moved_heading(&self) -> QString {
        QString::from(&rexon_core::categories::moved_settings_heading())
    }

    fn protection_types(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for key in qs::protection_types() {
            out.append(QVariant::from(&vmap(vec![("id", qstr(key))])));
        }
        out
    }

    // ── Числа ядра ────────────────────────────────────────────────────────

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

    // ── Состав разделов ───────────────────────────────────────────────────

    /// Разделы этой машины: [{id, title, description}] в порядке ядра.
    ///
    /// До вызова `configure` список пуст: рисовать разделы, не спросив
    /// машину, значит показать покупателю пункты, которых у него нет.
    fn sections(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        if !self.rust().configured {
            return out;
        }
        for id in qs::visible_sections(&self.rust().facts) {
            // Раздел есть в списке — значит, тексты у него есть тоже:
            // `section_texts` не находит только чужое имя рамки.
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

    // ── Готовые куски содержимого ─────────────────────────────────────────

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
        // Отступы сетки и зазор между плитками — те же, что у виджетной
        // раскладки: 0 полей внутри рамки раздела и 8 px между плитками.
        // Промежуток 8 и нулевые поля — те же числа, что стоят у виджетной
        // сетки библиотеки, поэтому оба пути дают одинаковое число столбцов.
        ui_base::grid_columns(viewport_width.max(0), LIB_CARD_W, 8, 0, 1, 99) as i32
    }

    fn library_page_size(&self, cols: i32) -> i32 {
        ui_base::grid_page_size(cols.max(0) as u32, qs::LIBRARY_ROWS as u32) as i32
    }

    fn toast_width(&self, page_width: i32, text_width: i32) -> i32 {
        crate::toast::width(page_width, text_width)
    }
}
