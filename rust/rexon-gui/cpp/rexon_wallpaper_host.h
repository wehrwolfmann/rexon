// Шим хоста видео-обоев: две вещи, до которых из Rust не дотянуться.
//
// Первая — идентификатор окна X11 (`QWindow::winId`), без него нечему
// ставить свойства EWMH. Вторая — поток кадров окна в общий mmap-файл: он держится
// на `QQuickWindow::grabWindow`, а тот отдаёт `QImage`, которого в
// cxx-qt-lib 0.9 нет вовсе.
//
// Границы те же, что у шима слоя (cpp/rexon_layer_shell.*): решений здесь
// нет. Раскладка кадра, права файла, интервал тика и план EWMH для X11
// приходят из ядра (`rexon-core::wallpaper_mmap`) уже посчитанными — шим
// только исполняет.
#pragma once

#include <cstddef>
#include <cstdint>

#include <QString>

class QObject;

/// Идентификатор окна для X11 (`QWindow::winId`). 0 — объект не QWindow.
///
/// Окно платформы, если его ещё нет, winId создаёт (без показа) — на X11
/// этим и пользуются, чтобы поставить тип окна ДО показа. На Wayland до
/// показа не звать: там это лишний повод для сюрпризов.
std::int64_t rexon_wp_window_id(QObject *window);

/// Запустить поток кадров окна в mmap-файл.
///
/// Кадры снимаются с `QQuickWindow` (само окно либо первый такой потомок),
/// приводятся к RGBA8888 и к заданному размеру и пишутся в файл целиком под
/// `flock(LOCK_EX)` — тот же контракт, что читают расширения оболочки GNOME
/// и Cinnamon: сырые RGBA8888, ровно width×height×4 байта, без заголовка.
///
/// Файл кадра при старте потока создаётся ЗАНОВО (прежний отцепляется) и до
/// первого настоящего кадра короче width×height×4 — по этому признаку
/// расширения оболочки и отличают «кадра ещё нет» от готового кадра. Без
/// этого при смене обоев они успевали показать последний кадр ПРЕЖНИХ обоев.
///
/// Источник кадра: `video_output` (VideoOutput из QML) — текущий кадр его
/// приёмника; `nullptr` — сцена окна рисуется `QQuickRenderControl` без
/// окна на экране (содержимое окна переносится в неё) и читается обратно.
/// Видео берётся из приёмника потому, что на платформе `offscreen`
/// VideoOutput не рисует. Если сцена без окна не поднялась, окно
/// показывается и снимается `grabWindow` — ТОЛЬКО при `may_show_window`
/// (решение ядра: платформа `offscreen`), иначе отказ.
///
/// Рядом с кадром ведётся счётчик поколения (`generation_path`, u64 LE):
/// нечётный на время записи кадра, чётный после. Им пользуются расширения
/// оболочки — замок `flock` им из GJS не взять. Пустой путь — без счётчика.
///
/// Таймер и дескриптор живут на объекте окна и умирают вместе с ним.
/// `false` — окно не найдено либо файл не открылся.
bool rexon_wp_mmap_stream_start(QObject *window,
                                QObject *video_output,
                                const QString &path,
                                const QString &generation_path,
                                std::int32_t width,
                                std::int32_t height,
                                std::int32_t interval_ms,
                                std::uint32_t file_mode,
                                bool may_show_window);

/// Чем снимается кадр потока: `rendercontrol-gpu <API> <устройство>`,
/// `rendercontrol-software`, `videosink`, `grabwindow`; пусто — потока нет.
QString rexon_wp_mmap_capture(QObject *window);

/// Доля не-чёрных пикселей последнего кадра, ‰ (выборка); -1 — кадров не было.
std::int64_t rexon_wp_mmap_nonblack_permille(QObject *window);

/// Сколько кадров ушло в файл с начала потока. Для самопроверки: «поток
/// запустился» и «кадры доехали» — разные утверждения.
std::int64_t rexon_wp_mmap_frames(QObject *window);

/// Центральный пиксель последнего записанного кадра (0xRRGGBB), -1 — кадров
/// не было. Для самопроверки: «кадры доехали» и «в кадрах есть картинка» —
/// тоже разные утверждения (чёрные кадры считались успехом).
std::int64_t rexon_wp_mmap_center_rgb(QObject *window);

// ── Flash-режим (SWF): голое окно, в которое рисует wgpu ─────────────────
// Типов QWindow, QScreen и native-интерфейса платформы в cxx-qt-lib 0.9 нет —
// отсюда шим. Решений здесь нет: размещение, размер и темп решает Rust.

/// Сколько экранов сейчас у сеанса. 0 — мониторов нет вовсе (такое бывает
/// на ходу: выдернули последний кабель), и обои тогда никуда не кладут.
std::int32_t rexon_wp_screen_count();

/// Закрывать ли приложение вместе с последним окном.
///
/// ⚠️ Обоям нужно `false`, и это не украшение. Окон у хоста столько,
/// сколько мониторов; когда монитор отключают, окно уничтожается, и при
/// умолчании Qt (`true`) уход последнего монитора убил бы весь процесс
/// обоев — вернувшийся через секунду монитор остался бы пустым навсегда.
void rexon_wp_set_quit_on_last_window_closed(bool enabled);

/// Новое окно без QML и без буфера Qt, на экране `screen_index` (< 0 —
/// основной). `width`/`height` ≤ 0 — размер этого экрана, и окно ставится
/// в его угол: на X11 иначе все окна обоев легли бы на один монитор.
/// Слой (LayerShellQt) ставится ДО показа.
QObject *rexon_wp_swf_window_new(std::int32_t width,
                                 std::int32_t height,
                                 std::int32_t screen_index);
void rexon_wp_swf_window_show(QObject *window);
/// Убрать окно совсем: видеокарта отказалась в него рисовать, и
/// оставленная пустая заслонка была бы чёрным прямоугольником на весь
/// монитор вместо обоев.
void rexon_wp_swf_window_close(QObject *window);
bool rexon_wp_swf_window_exposed(QObject *window);
/// Размер в физических пикселях (с учётом devicePixelRatio).
std::int32_t rexon_wp_swf_window_width(QObject *window);
std::int32_t rexon_wp_swf_window_height(QObject *window);
/// Прокрутить очередь событий Qt (у Flash-режима свой цикл отрисовки).
void rexon_wp_process_events();
/// Native-хэндлы для wgpu; 0 — платформа не та или окна нет.
std::size_t rexon_wp_wl_display();
std::size_t rexon_wp_wl_surface(QObject *window);
std::size_t rexon_wp_xcb_connection();
