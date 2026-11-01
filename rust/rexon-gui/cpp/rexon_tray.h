// Qt-примитивы значка Rexon в системном лотке.
//
// Здесь НЕТ ни одного решения и ни одной подписи: что за пункты, как они
// названы, включены ли они и что делает щелчок — знает ядро
// (rexon-core::tray), передаёт исполнитель (rust/rexon-gui/src/tray.rs), а
// этот файл только строит QSystemTrayIcon с QMenu и дёргает окно.
//
// Почему шим, а не чистый Rust: QSystemTrayIcon, QMenu, QAction и перехват
// закрытия окна живут в QtWidgets/QtGui, и биндингов к ним в cxx-qt-lib нет
// ни в 0.9, ни в 0.10. Тот же приём и та же граница, что у плейсхолдера
// превью (rexon_local_preview.h). А вот САМ QApplication на Rust есть — в
// соседнем крейте того же выпуска (cxx-qt-lib-extras), и без него значка у
// процесса с окном на QML быть не могло: виджеты (QMenu) Qt без QApplication
// не создаёт вовсе.
//
// ⚠️ Окно шим ищет ДВУМЯ путями и держит оба: виджетное QMainWindow продукта
// на PySide6 и QQuickWindow бинаря главного окна на Rust, где виджетов в
// процессе нет ни одного.
#pragma once

#include <cstdint>

extern "C" {

// ── обратные вызовы в Rust ────────────────────────────────────────────────
//
// Ставятся один раз до создания значка. Установщиком, а не прямым `extern`
// на Rust-символ, — как у запуска демона вкладки обоев
// (rexon_we_do_set_launch_hook в rexon_we_singletons.h):
// шим остаётся собираемым сам по себе.

// Нажат пункт меню с этим номером.
typedef void (*RexonTrayClick)(int32_t id);
// Меню вот-вот раскроется — самое время пересобрать его состав.
typedef void (*RexonTrayVoid)();
// Закрывается главное окно. true — закрытие проглотить и окно спрятать.
typedef bool (*RexonTrayAsk)();

void rexon_tray_set_hooks(RexonTrayClick on_click, RexonTrayVoid on_menu_show,
                          RexonTrayVoid on_activate, RexonTrayAsk on_close);

// ── значок ────────────────────────────────────────────────────────────────

// Есть ли в сессии лоток, куда становиться.
bool rexon_tray_available();

// Создать значок. `icon_file` — путь к картинке («» — брать из темы),
// `icon_theme` — имя значка в теме. Повторный вызов ничего не делает.
// false — значок создать не вышло (в том числе когда в процессе нет
// QApplication: QMenu без него не создаётся, и попытка гасила бы процесс).
bool rexon_tray_create(const char *icon_file, const char *icon_theme,
                       const char *tip_title, const char *tip_body);

// Значок всех окон процесса (`QGuiApplication::setWindowIcon`) — та же
// картинка, что у значка в лотке: из темы, иначе из файла. На X11 без него у
// окна нет `_NET_WM_ICON`, и панель задач рисует общий значок вместо нашего.
// false — ни тема, ни файл картинку не дали. Звать после создания
// приложения и до первого окна.
bool rexon_set_app_window_icon(const char *icon_file, const char *icon_theme);

// Пересобрать меню целиком: begin → add… → end.
//
// `kind`: 0 — действие, 1 — разделитель, 2 — флажок, 3 — кружок в группе,
// 4 — начало вложенного меню, 5 — его конец. Значения задаёт ядро
// (rexon_core::tray::ItemKind), здесь они только числа.
void rexon_tray_menu_begin();
void rexon_tray_menu_add(int32_t id, int32_t kind, const char *label,
                         bool checked, bool enabled, bool is_default);
void rexon_tray_menu_end();

// Всплывающее сообщение от значка.
void rexon_tray_notify(const char *title, const char *body);

// ── главное окно ──────────────────────────────────────────────────────────
//
// ⭐ Все четыре двери можно звать ИЗ ЛЮБОГО ПОТОКА: работу с окном шим сам
// переносит в поток GUI и ждёт настоящего ответа. Это нужно слушателю
// второго запуска (rexon_core::single_instance) — он живёт в своём потоке, и
// без переноса подъём окна был невозможен в принципе.

// Запомнить главное окно приложения и перехватить его закрытие. false —
// окна не нашли (значит и прятать нечего).
bool rexon_tray_guard_window();

// Поднять окно и отдать ему фокус. false — окна нет.
bool rexon_tray_raise_window();

// Спрятать окно.
bool rexon_tray_hide_window();

// Переключить главное окно на раздел с этим номером. false — полосы
// разделов не нашли либо номера такого нет.
bool rexon_tray_select_tab(int32_t index);

// Выйти из приложения по-настоящему (сторож закрытия снимается).
void rexon_tray_quit();

// Открыть раздел (и подраздел) главного окна на QML. false — окна нет,
// раздела такого нет либо окно на виджетах.
bool rexon_tray_open_section(const char *id, const char *subpage);

// Запланировать выход через delay_ms и вернуться СРАЗУ (передача хозяина:
// ответ «ухожу» обязан уйти до выхода).
void rexon_tray_request_quit(int32_t delay_ms);

// ── демон обоев на шине ───────────────────────────────────────────────────

// Виден ли демон на session-bus.
bool rexon_tray_daemon_alive();

// Метод демона без аргументов / с одним аргументом. false — вызов не удался.
bool rexon_tray_daemon_call(const char *method);
bool rexon_tray_daemon_call_u32(const char *method, uint32_t arg);
bool rexon_tray_daemon_call_bool(const char *method, bool arg);

// Свойство демона: строка копируется в `buf` (с завершающим нулём).
// Возвращает число записанных байт без нуля; -1 — свойство недоступно.
int32_t rexon_tray_daemon_prop(const char *name, char *buf, int32_t buf_len);
}
