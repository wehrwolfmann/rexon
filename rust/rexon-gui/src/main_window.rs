// MainWindowModel — хозяин главного окна Rexon на Rust + cxx-qt + QML.
//
// ЗАЧЕМ. До сих пор зависимость смотрела не туда: программу запускал Python
// (`rexon.py` → `rexon/app.py` → `rexon/qt_main/app.py`), он создавал
// `QApplication`, он строил `QMainWindow` с `QTabWidget`, а Rust
// подключался к нему библиотекой (`librexon_gui.so` через `ctypes.CDLL`) и
// раздавал QML-страницы внутрь виджетов через `QQuickWidget`. Окно
// принадлежало виджетам, QML лежал ВНУТРИ них. Пока хозяин процесса —
// Python, уйти с него нельзя в принципе: снять можно только то, чем ты
// владеешь.
//
// Здесь то же окно перевёрнуто: хозяин — Rust, окно целиком `ApplicationWindow`
// на QML (`qml/MainWindow.qml`), а виджетов в процессе нет вовсе.
//
// ⚠️ ЭТО СКЕЛЕТ, А НЕ ЗАМЕНА ПРОДУКТА. Живой Rexon запускается по-прежнему
// и этим кодом не пользуется: бинарь `rexon-main-window` — отдельный вход
// для разработчика, ровно как стенд `rexon-gui-selftest` рядом. Растим рядом, за
// отдельным входом, как уже семь раз делали с вкладками.
//
// ЧТО ЗДЕСЬ ЕСТЬ:
//   * `MainWindowModel` — заголовок, минимальный размер, состав разделов и
//     палитра скина. Ни одного собственного решения: порядок и подписи
//     разделов приходят из `rexon_core::qt_main`, палитра — из
//     `ui_base::active_qml_palette`, как у всех перенесённых вкладок.
//
// Объекта побочных действий у «Сравнения DE» здесь больше нет: страница берёт
// тот же `DesktopsActions`, что и «Рабочие столы» (src/desktops_actions.rs),
// — один исполнитель (`rexon_core::system_de_exec`) и одно окно хода
// (`qml/DeOperationDialog.qml`) на обе вкладки.

/// Мост cxx-qt: хозяин главного окна.
///
/// ⚠️ Тип зовётся `MainWindowModel`, а не `MainWindow`: .qml-файлы модуля
/// сами становятся QML-типами по имени файла, и `MainWindow` занят
/// `qml/MainWindow.qml`. Правило на все вкладки и на окно: вид —
/// `<Имя>.qml`, объект — `<Имя>Model`.
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
        // ── Окно. CONSTANT: за время жизни окна не меняется.
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        // ── Разделы: [{id, label, page}, …] в порядке окна. Порядок и
        // подписи — из ядра, здесь только раскладка по карте для QML.
        #[qproperty(QList_QVariant, sections, READ = sections, CONSTANT)]
        // ── Палитра стандартного скина (steam_dark, правило #17). Не
        // CONSTANT по той же причине, что у вкладок: скин меняется на лету,
        // и без сигнала окно осталось бы в цветах прошлого скина.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // ── Плашка вкладки: краски и раскладка «чипа» в полосе разделов.
        // Своих чисел разметка не заводит — они в ядре (`qt_main::TAB_*`),
        // общие с виджетным окном. CONSTANT: нейтральный серый плашки от
        // скина не зависит.
        #[qproperty(QMap_QString_QVariant, tab_style, cxx_name = "tabStyle",
                    READ = tab_style, CONSTANT)]
        type MainWindowModel = super::MainWindowModelRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut MainWindowModel>);
    }

    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &MainWindowModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &MainWindowModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &MainWindowModel) -> i32;
        fn sections(self: &MainWindowModel) -> QList_QVariant;
        fn palette(self: &MainWindowModel) -> QMap_QString_QVariant;
        #[cxx_name = "tabStyle"]
        fn tab_style(self: &MainWindowModel) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// Текст на месте раздела, у которого Rust-хозяина ещё нет.
        #[qinvokable]
        #[cxx_name = "notPortedNotice"]
        fn not_ported_notice(self: &MainWindowModel, section_id: &QString) -> QString;

        /// Режим обхода (`--tour`): ответ на прошлую команду (JSON, пусто —
        /// первый вызов) → следующая команда (JSON). Решает ядро
        /// (`rexon_core::main_window_tour`).
        #[qinvokable]
        #[cxx_name = "tourNext"]
        fn tour_next(self: &MainWindowModel, report: &QString) -> QString;

        /// Режим обхода: можно ли позвать метод страницы. Исполнитель
        /// спрашивает перед КАЖДЫМ вызовом.
        #[qinvokable]
        #[cxx_name = "tourCallAllowed"]
        fn tour_call_allowed(self: &MainWindowModel, method: &QString) -> bool;

        /// Режим обхода: замер ожидания (JSON) → `wait`/`ready`/`timeout`.
        #[qinvokable]
        #[cxx_name = "tourWaitVerdict"]
        fn tour_wait_verdict(self: &MainWindowModel, sample: &QString) -> QString;

        /// Режим обхода: снять окно целиком в PNG `path`.
        #[qinvokable]
        #[cxx_name = "tourGrab"]
        fn tour_grab(self: &MainWindowModel, path: &QString) -> bool;
    }

    // Объявления трейтов обязаны быть В МОСТЕ: cxx-qt печатает по ним и
    // конструктор, и алиас типа потока.
    impl cxx_qt::Initialize for MainWindowModel {}
    impl cxx_qt::Threading for MainWindowModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::qt_main;
use rexon_core::ui_base;

// ============================================================================
// Разделы: какой из них Rust-каркас уже показывает по-настоящему
// ============================================================================

/// Раздел → страница QML, которую каркас в него кладёт.
///
/// Пусто — страницы у раздела нет, каркас покажет объяснение из ядра
/// (`qt_main::not_ported_notice`), а не пустой прямоугольник.
///
/// ⚠️ Таблица живёт ЗДЕСЬ, а не в ядре, намеренно: ядро не знает и не
/// должно знать о существовании .qml-файлов. Порядок и состав разделов оно
/// при этом задаёт целиком — здесь только отметка готовности.
///
/// Идентификатор страницы разметку НЕ ищет: разметку за ним держит карта
/// `pageComponents` в `qml/MainWindow.qml`, и сторож
/// `every_page_is_in_the_qml_map` не даёт этим двум спискам разъехаться.
///
/// У «desktops» стоит своя страница, а не подраздел «compare», и это правка
/// по факту, а не по плану: объект побочных действий у «Рабочих столов»
/// появился (`src/desktops_actions.rs`), и страница перестала отдавать
/// мёртвые кнопки. «Сравнение DE» при этом никуда не делось — с 08.07.2026
/// оно и есть страница ВНУТРИ «Рабочих столов», и окно показывает его там
/// же, второй страницей раздела (карта `pageComponents` в
/// qml/MainWindow.qml). Поэтому подраздела у раздела больше нет: подпись
/// «Сравнение DE» на самой вкладке означала бы, что сравнение — это и есть
/// весь раздел.
const SECTION_PAGES: &[(&str, &str)] = &[
    ("desktops", "desktops"),
    ("appearance", "appearance"),
    ("extensions", "extensions"),
    ("wallpaper_engine", "wallpaper_engine"),
    ("steam", "steam"),
    ("settings", "settings"),
];

fn page_of(section_id: &str) -> &'static str {
    SECTION_PAGES
        .iter()
        .find(|(id, _)| *id == section_id)
        .map(|(_, page)| *page)
        .unwrap_or("")
}

// ============================================================================
// Мелочи перевода в Qt
// ============================================================================

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

// ============================================================================
// Хозяин окна
// ============================================================================

/// Rust-состояние окна.
#[derive(Default)]
pub struct MainWindowModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    /// 0 — окно ещё не подписано.
    skin_watch: u64,
}

impl Drop for MainWindowModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl qobject::MainWindowModel {
    fn window_title(&self) -> QString {
        // Тот же заголовок, что у питоновского окна: ярлык рабочего стола
        // переходит на это окно, и приписка для разработчика в нём была бы
        // жаргоном перед покупателем.
        QString::from(&qt_main::window_title())
    }

    fn window_min_width(&self) -> i32 {
        qt_main::WINDOW_MIN_SIZE.0
    }

    fn window_min_height(&self) -> i32 {
        qt_main::WINDOW_MIN_SIZE.1
    }

    /// Состав таб-бара: [{id, label, page, subsection}, …] в порядке окна.
    fn sections(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for id in qt_main::SECTIONS {
            let page = page_of(id);
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(id)),
                ("label", qstr(&qt_main::section_tab_label(id))),
                // Какую страницу класть в раздел; пусто — заглушка.
                ("page", qstr(page)),
                // Подпись подраздела: «Сравнение DE» живёт ВНУТРИ «Рабочих
                // столов», и окно обязано это показать, а не выдать
                // подраздел за раздел.
                (
                    "subsection",
                    qstr(&if page.is_empty() || page == *id {
                        // Подраздела нет: раздел показывает СВОЮ страницу
                        // (или пока заглушку), и второго имени ему не надо.
                        // Без этой проверки «Оформление» подписалось бы
                        // подразделом «appearance» — идентификатором вместо
                        // человеческого имени.
                        String::new()
                    } else {
                        qt_main::subsection_label(page)
                    }),
                ),
            ])));
        }
        out
    }

    /// Палитра ДЕЙСТВУЮЩЕГО скина — тот же источник, что у перенесённых
    /// вкладок. Своих цветов окно не заводит: вторая палитра развела бы
    /// каркас с продуктом молча.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // Окно операции поверх главного окна (восстановление оформления при
        // запуске): затемнение и краски «Да» — теми же помощниками, что у
        // страниц «Рабочие столы», «Сравнение» и «Настройки».
        out.insert(QString::from("modal_dim"), qstr(&ui_base::modal_dim_argb_hex()));
        crate::desktops_page::insert_operation_tones(&mut out);
        out
    }

    /// Плашка вкладки: краски и раскладка, общие с виджетным окном.
    ///
    /// ЗАЧЕМ ОНА ЕСТЬ. Неактивные вкладки рисовались прозрачными, и полоса
    /// разделов выходила сплошной: границы между соседями не было видно
    /// вовсе (замечание Wehrwolfmann 18.09.2026). В виджетном окне у каждой
    /// вкладки своя плашка с зазором — здесь те же числа, взятые из ядра, а
    /// не подобранные на глаз второй раз.
    ///
    /// Краски идут через `css_color_to_qml`: запись `rgba(...)` QML читает
    /// как недопустимый цвет и рисует ЧЁРНЫМ.
    fn tab_style(&self) -> QMap<QMapPair_QString_QVariant> {
        let col = |css: &str| qstr(&ui_base::css_color_to_qml(css));
        let num = |v: i32| QVariant::from(&v);
        vmap(vec![
            ("idleFill", col(&qt_main::tab_idle_fill())),
            ("hoverFill", col(&qt_main::tab_hover_fill())),
            ("radius", num(qt_main::TAB_RADIUS)),
            ("marginH", num(qt_main::TAB_MARGIN_H)),
            ("marginV", num(qt_main::TAB_MARGIN_V)),
        ])
    }

    fn not_ported_notice(&self, section_id: &QString) -> QString {
        QString::from(qt_main::not_ported_notice(&section_id.to_string()))
    }

    fn tour_next(&self, report: &QString) -> QString {
        QString::from(&tour_step(&report.to_string()))
    }

    fn tour_call_allowed(&self, method: &QString) -> bool {
        tour::call_allowed(&method.to_string())
    }

    fn tour_wait_verdict(&self, sample: &QString) -> QString {
        let sample: tour::WaitSample = serde_json::from_str(&sample.to_string()).unwrap_or_default();
        QString::from(tour::wait_verdict(&sample).as_str())
    }

    fn tour_grab(&self, path: &QString) -> bool {
        crate::tray::grab_main_window(&path.to_string())
    }
}

// ============================================================================
// Режим обхода: состояние живёт здесь, решения — в ядре
// ============================================================================

use rexon_core::main_window_tour as tour;

/// Обход этого процесса. Под замком, а не в объекте окна: общий тайм-аут
/// пишет отчёт из своего потока (`tour_abort`).
static TOUR: std::sync::Mutex<Option<tour::Tour>> = std::sync::Mutex::new(None);

/// Завести обход (зовёт бинарь до загрузки QML).
pub fn tour_install(cfg: tour::TourConfig) {
    if let Ok(mut t) = TOUR.lock() {
        *t = Some(tour::Tour::new(cfg));
    }
}

fn journal_of(cfg: &tour::TourConfig) -> String {
    std::fs::read(cfg.out_dir.join(tour::JOURNAL_FILE))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn write_report(cfg: &tour::TourConfig, text: &str) {
    let path = cfg.out_dir.join(tour::REPORT_FILE);
    match std::fs::write(&path, text) {
        Ok(()) => println!("обход: отчёт {}", path.display()),
        Err(e) => println!("обход: отчёт {} не записан: {e}", path.display()),
    }
}

/// Один шаг обхода: принять ответ, выдать команду. Конец — отчёт на диск.
fn tour_step(report: &str) -> String {
    let Ok(mut guard) = TOUR.lock() else {
        return tour::Command::Finish { exit_code: tour::EXIT_BAD_ARGS, report: String::new() }.to_json();
    };
    let Some(t) = guard.as_mut() else {
        return tour::Command::Finish { exit_code: tour::EXIT_BAD_ARGS, report: String::new() }.to_json();
    };
    let journal = journal_of(&t.cfg);
    let report = (!report.trim().is_empty()).then(|| {
        let mut r = tour::Report::from_json(report);
        r.log_offset = journal.len() as u64;
        r
    });
    let cmd = t.next(report, &journal);
    match &cmd {
        tour::Command::Finish { exit_code, report } => {
            write_report(&t.cfg, report);
            println!("обход окончен: снимков {}, код {exit_code}", t.screens().len());
        }
        other => println!("обход: {}", other.to_json()),
    }
    cmd.to_json()
}

/// Общий тайм-аут: отчёт о том, что успели, и код возврата. Зовётся из
/// потока сторожа; обход уже окончен — `None`.
pub fn tour_abort(reason: &str) -> Option<i32> {
    let guard = TOUR.lock().ok()?;
    let t = guard.as_ref()?;
    if t.is_finished() {
        return None;
    }
    let (text, code) = t.report(&journal_of(&t.cfg), Some(reason));
    write_report(&t.cfg, &text);
    Some(code)
}

impl cxx_qt::Initialize for qobject::MainWindowModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::MainWindowModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Каждому разделу ядра — строка в таблице готовности, и наоборот.
    ///
    /// Без этого сторожа новый раздел в ядре молча получил бы пустую
    /// страницу, а снятый — осиротевшую строку здесь (правило #12).
    #[test]
    fn section_pages_cover_core_sections() {
        let core: Vec<&str> = qt_main::SECTIONS.to_vec();
        let here: Vec<&str> = SECTION_PAGES.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            core, here,
            "таблица готовности разделов разъехалась с ядром \
             (rexon_core::qt_main::SECTIONS)"
        );
    }

    /// Каждая непустая страница таблицы обязана значиться в карте
    /// `pageComponents` окна.
    ///
    /// Разъезд этих двух списков означал бы раздел, который Rust объявил
    /// готовым, а QML открыть не умеет: человек увидел бы вместо страницы
    /// объяснение «ещё не перенесён» — то есть враньё о причине, и молча
    /// (правило #12).
    #[test]
    fn every_page_is_in_the_qml_map() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("qml/MainWindow.qml");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("не читается {}: {e}", path.display()));
        let map = src
            .split_once("readonly property var pageComponents: ({")
            .and_then(|(_, rest)| rest.split_once("})"))
            .map(|(body, _)| body)
            .expect("в qml/MainWindow.qml не найдена карта pageComponents");
        for (id, page) in SECTION_PAGES {
            if page.is_empty() {
                continue;
            }
            assert!(
                map.contains(&format!("\"{page}\":")),
                "страница «{page}» раздела «{id}» не значится в карте \
                 pageComponents окна (qml/MainWindow.qml)"
            );
        }
    }

    /// Раздел, у которого страница называется как он сам, подраздела не
    /// имеет: подпись подраздела — это «Сравнение DE» внутри «Рабочих
    /// столов», а не идентификатор страницы, выданный за имя.
    #[test]
    fn only_a_real_subsection_gets_a_subsection_label() {
        for (id, page) in SECTION_PAGES {
            if page.is_empty() || page == id {
                continue;
            }
            assert_ne!(
                qt_main::subsection_label(page),
                *page,
                "у подраздела «{page}» (раздел «{id}») нет человеческой \
                 подписи в rexon_core::qt_main::subsection_label"
            );
        }
    }

}
