// Фабрика дочерних объектов живых моделей App (рендереры, дисплеи, холсты,
// видеокарты).
//
// Отдельный заголовок, а не строки в rexon_gui_factory.h: cxx печатает
// шим-обёртку на каждое объявление `extern "C++"`, собранное по имени
// C++-функции, и одно и то же имя, объявленное в двух мостах крейта, даёт
// `duplicate symbol` на линковке. Здесь имена свои, и объявлены они ровно в
// одном мосте — src/we_objmodel.rs.
#pragma once

#include <QtCore/QString>

class QObject;
class QVariant;

/// Создать `Renderer`/`Display`/`Canvas`/`Gpu` ребёнком менеджера. Владение —
/// у Qt.
QObject* rexon_new_renderer(QObject* parent);
QObject* rexon_new_display(QObject* parent);
QObject* rexon_new_canvas(QObject* parent);
QObject* rexon_new_gpu(QObject* parent);

/// Создать живые модели ребёнком App (зовёт мост src/we_app.rs).
QObject* rexon_new_renderer_manager(QObject* parent);
QObject* rexon_new_display_manager(QObject* parent);
QObject* rexon_new_library_manager(QObject* parent);
QObject* rexon_new_gpu_manager(QObject* parent);
QObject* rexon_new_presentation_manager(QObject* parent);

/// Модель строк карточки «Текущие обои» — ребёнок менеджера показов.
QObject* rexon_new_now_playing_model(QObject* parent);

/// Строка каталога перевода по контексту и исходному тексту
/// (`QCoreApplication::translate`). Rust своих строк так не переводит — это
/// только для строк ВЕНДОРА, у которых каталог `.qm` и контекст C++-класса.
QString rexon_we_vendor_tr(const QString& context, const QString& source);

/// Снести дочерний объект отложенно.
///
/// Именно `deleteLater`, а не `delete`: на объект могут висеть привязки QML
/// текущего кадра (`Repeater { model: rendererManager.renderers }`), и
/// немедленное удаление вырвало бы у них землю из-под ног. Оригинал делает
/// то же самое (`renderer.cpp`, `display.cpp`).
void rexon_delete_child_later(QObject* object);

/// Указатель на QObject значением QVariant — так вендорный QML получает
/// список объектов (`QVariantList` свойств `renderers`/`displays`).
/// cxx-qt-lib свой QVariant указателем на QObject наполнять не умеет.
QVariant rexon_object_to_variant(QObject* object);
