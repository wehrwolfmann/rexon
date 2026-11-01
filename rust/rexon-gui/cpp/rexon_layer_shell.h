// Шим LayerShellQt: перевод обычного QWindow в поверхность wlr-layer-shell.
//
// Зачем шим: LayerShellQt — библиотека KDE на C++ (пакет layer-shell-qt),
// привязки к Rust у неё нет и не будет. Из Rust её видно только так же, как
// cxx-qt видит собственные QObject'ы, — через тонкий C++-файл. Ровно тем же
// приёмом сделана фабрика моделей (cpp/rexon_gui_factory.cpp), это второй шим
// того же класса, а не новая архитектура.
//
// Чего шим НЕ делает: не создаёт окон, не решает, каким быть слою, и не знает
// слова «обои». Он исполняет то, что ему сказали из Rust, — решение «слой
// обоев = Background + четыре якоря + зона -1» живёт в src/layer_shell.rs
// единственным экземпляром (правило #12: общий движок, а не по копии на
// каждый вид обоев).
//
// ⚠️ Порядок обязателен: свойства слоя ставятся ДО первого показа окна.
// LayerShellQt подменяет интеграцию оболочки в момент создания платформенного
// окна; если окно уже показано, оно уже стало обычным xdg_toplevel, и
// поздний вызов не переселит его в слой. Стенд ловит это нарушение по
// протоколу — см. src/layer_shell.rs.
#pragma once

#include <cstdint>

#include <QString>

class QObject;

/// Возможен ли слой обоев в текущей сессии: платформа Qt — Wayland.
/// Наличие самого протокола wlr-layer-shell у композитора отсюда не видно
/// (Mutter/Muffin его не дают) — это выясняется только при показе окна.
bool rexon_layer_shell_available();

/// Перевести окно в слой. `window` обязан быть QWindow (обычно — корневой
/// объект QML-компонента Window), иначе возвращается false.
///
/// `layer` — LayerShellQt::Window::Layer (0 Background, 1 Bottom, 2 Top,
/// 3 Overlay); `anchors` — битовая маска Anchor (1 верх, 2 низ, 4 лево,
/// 8 право); `exclusion_zone` — та самая «зона вытеснения» (-1 = не
/// вытеснять панели и самому не резервировать место, значение обоев);
/// `keyboard` — KeyboardInteractivity (0 None, 1 Exclusive, 2 OnDemand).
/// `scope` — идентификатор поверхности для композитора (пустая строка = не
/// трогать умолчание LayerShellQt).
bool rexon_layer_shell_configure(QObject *window,
                                 std::int32_t layer,
                                 std::int32_t anchors,
                                 std::int32_t exclusion_zone,
                                 std::int32_t keyboard,
                                 const QString &scope);

/// Прицелить поверхность слоя на ТОТ ЖЕ экран, на котором стоит само окно
/// (`QWindow::screen()`), и снять «пусть решает композитор».
///
/// Зачем отдельным вызовом. Без него LayerShellQt берёт экран сам: при
/// `screen() == nullptr` и `wantsToBeOnActiveScreen() == false` он читает
/// `QWindow::screen()`, но оба умолчания — его внутреннее дело, и обои на
/// втором мониторе не должны зависеть от того, не поменяет ли KDE их в
/// следующей версии. Здесь выход поверхности назван явно.
///
/// Звать ПОСЛЕ `rexon_layer_shell_configure` и ДО первого показа окна:
/// выход уходит в запрос `get_layer_surface`, и после показа менять его
/// поздно. Возвращает false, если окно не QWindow, слой ему не ставили или
/// экрана у окна нет.
bool rexon_layer_shell_pin_to_window_screen(QObject *window);

// Обратное чтение — для самопроверки. Возвращают -1, если объект не QWindow.
//
// ⚠️ Это чтение НАШЕГО же состояния в LayerShellQt, а не ответа композитора:
// оно доказывает лишь, что сеттеры не были молча проигнорированы. Что окно
// действительно стало фоновой поверхностью, доказывается протоколом Wayland
// в стенде.
std::int32_t rexon_layer_shell_layer(QObject *window);
std::int32_t rexon_layer_shell_anchors(QObject *window);
std::int32_t rexon_layer_shell_exclusion_zone(QObject *window);
std::int32_t rexon_layer_shell_keyboard(QObject *window);
/// Имя экрана, на который нацелена поверхность слоя; пусто — экран не
/// назван (окно не в слое либо выход выбирает композитор).
QString rexon_layer_shell_screen_name(QObject *window);
