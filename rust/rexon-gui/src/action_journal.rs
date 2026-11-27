// ActionJournal — журнал действий человека и пульс окна.
//
// ЗАЧЕМ. Живой случай 15.11.2026: окно зависло, стол предложил «Закрыть»,
// процесс убит — а в журнале ни что нажимали, ни где стоял главный поток.
//
// Действия пишутся в ОДНОЙ точке на всё приложение — фильтром ввода (шим
// `rexon_install_input_journal`): любая кнопка, флажок, строка списка,
// пункт меню, карточка магазина с `TapHandler`/`MouseArea` любой вкладки,
// в том числе вендорной вкладки обоев, попадает в журнал сама, без строчки
// в странице. QML остаётся только то, чего фильтр знать не может:
//   * смена раздела окна (`section`, полоса разделов `MainWindow.qml`);
//   * поле ввода (`typing`/`entered`, общий контрол `RexonTextField`):
//     по событиям клавиш не понять, что набрано, а пароль писать нельзя.
//
// Логики здесь нет (правило крейта): что из нажатия действие, тексты строк,
// текущее действие и сторож — ядро (`rexon_core::ui_watch`).
//
// Самопроверка и стенды журнала не заводят (`applog::is_active`), и
// синглтон в них молчит: строки действий не должны мешаться в вывод,
// который разбирают прогоны.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        type ActionJournal = super::ActionJournalRust;

        /// Человек набирает текст в поле `item` с подсказкой `field`;
        /// `secret` — поле пароля (его `value` приходит пустым).
        #[qinvokable]
        fn typing(self: &ActionJournal, item: &QVariant, field: &QString, value: &QString, secret: bool);

        /// Ввод в поле `item` закончен (Enter или уход фокуса). → номер для
        /// `finish`, 0 — строки не было.
        #[qinvokable]
        fn entered(self: &ActionJournal, item: &QVariant, field: &QString, value: &QString, secret: bool) -> f64;

        /// Обработчик действия `id` вернул управление циклу событий.
        #[qinvokable]
        fn finish(self: &ActionJournal, id: f64);

        /// Открыт раздел окна `id`.
        #[qinvokable]
        fn section(self: &ActionJournal, id: &QString);
    }
}

use std::os::raw::{c_char, c_int, c_void};

use rexon_core::applog;
use rexon_core::ui_watch;

extern "C" {
    fn rexon_start_ui_heartbeat(ms: c_int, tick: extern "C" fn()) -> bool;
    fn rexon_qml_ancestry(variant: *const c_void, buf: *mut c_char, len: c_int) -> c_int;
    fn rexon_install_input_journal(
        begin: extern "C" fn(*const c_char, c_int, c_int) -> u64,
        finish: extern "C" fn(u64),
    ) -> bool;
}

/// Нажатие от фильтра ввода (шим) → ядро. 0 — не действие.
extern "C" fn input_begin(desc: *const c_char, len: c_int, via_key: c_int) -> u64 {
    if desc.is_null() || len <= 0 || !applog::is_active() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(desc.cast::<u8>(), len as usize) };
    ui_watch::begin_input(&String::from_utf8_lossy(bytes), via_key != 0)
}

extern "C" fn input_finish(id: u64) {
    ui_watch::finish_deed(id);
}

#[derive(Default)]
pub struct ActionJournalRust;

/// Цепочка классов предков объекта из QML (см. шим).
fn ancestry(item: &cxx_qt_lib::QVariant) -> String {
    let mut buf = vec![0u8; 4096];
    let n = unsafe {
        rexon_qml_ancestry((item as *const cxx_qt_lib::QVariant).cast(), buf.as_mut_ptr().cast(), buf.len() as c_int)
    };
    buf.truncate(n.max(0) as usize);
    String::from_utf8_lossy(&buf).into_owned()
}

fn area_of(item: &cxx_qt_lib::QVariant) -> String {
    ui_watch::area_from_chain(&ui_watch::current_section(), &ancestry(item))
}

impl qobject::ActionJournal {
    fn typing(&self, item: &cxx_qt_lib::QVariant, field: &cxx_qt_lib::QString, value: &cxx_qt_lib::QString, secret: bool) {
        if !applog::is_active() {
            return;
        }
        let value = if secret { String::new() } else { value.to_string() };
        ui_watch::note_typing(&area_of(item), &field.to_string(), &value, secret);
    }

    fn entered(
        &self,
        item: &cxx_qt_lib::QVariant,
        field: &cxx_qt_lib::QString,
        value: &cxx_qt_lib::QString,
        secret: bool,
    ) -> f64 {
        if !applog::is_active() || secret {
            return 0.0;
        }
        ui_watch::note_entered(&area_of(item), &field.to_string(), &value.to_string(), false) as f64
    }

    fn finish(&self, id: f64) {
        if id >= 1.0 {
            ui_watch::finish_deed(id as u64);
        }
    }

    fn section(&self, id: &cxx_qt_lib::QString) {
        if applog::is_active() {
            ui_watch::note_section(&id.to_string());
        }
    }
}

/// Завести сторож главного потока, его пульс и фильтр ввода журнала
/// действий. Звать из потока GUI после создания приложения и до цикла событий.
pub fn install_main_watchdog() {
    if !unsafe { rexon_install_input_journal(input_begin, input_finish) } {
        applog::write(applog::Level::Warn, ui_watch::AREA, "журнал действий не встал: приложения Qt ещё нет");
    }
    ui_watch::install_main_watchdog();
    let ok = unsafe { rexon_start_ui_heartbeat(ui_watch::HEARTBEAT_MS as c_int, ui_watch::main_heartbeat) };
    if !ok {
        applog::write(applog::Level::Warn, ui_watch::AREA, "пульс окна не встал: приложения Qt ещё нет");
    }
}
