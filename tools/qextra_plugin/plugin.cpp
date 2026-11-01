// Динамический QML-плагин модуля `QExtra` (hypengw/QExtra, MIT OR Apache-2.0).
//
// Почему здесь C++ (правило #16): QExtra — чужая библиотека апстрима
// waywallen на модулях C++23, и её QML-типы регистрирует функция,
// порождённая её же сборкой. Этот файл — только склейка плагина: он держит
// ссылку на ту функцию, чтобы движок QML нашёл типы. Решений и логики здесь
// нет и быть не должно: какой модуль взять (настоящий или QML-подмену),
// решает ядро — `rust/rexon-core/src/qml_modules.rs`, `choose_qextra`.
//
// Исходников QExtra в дереве нет: ревизия закреплена в `lito.lock` рядом,
// собирает `tools/build_qml_qextra.sh`.
#include "plugin.hpp"
#include <QtCore/qtsymbolmacros.h>

QT_DECLARE_EXTERN_SYMBOL_VOID(qml_register_types_QExtra)

QExtraDynPlugin::QExtraDynPlugin(QObject* parent): QQmlEngineExtensionPlugin(parent) {
    QT_KEEP_SYMBOL(qml_register_types_QExtra)
}
