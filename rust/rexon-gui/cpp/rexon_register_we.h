// Регистрация Rust-типов rexon-gui в QML-модуле вендорного интерфейса
// waywallen (`waywallen.ui`). Зовётся из Rust-обёртки `register_we_types`
// (src/lib.rs) при подготовке процесса (`we_env::prepare_process`).
//
// Регистрация именно императивная, а не через QML-модуль `org.rexon.gui`:
// вендорный QML импортирует `waywallen.ui`, и Rust-тип обязан встать под
// тем же URI и тем же именем, что раньше занимал Python-класс.
#pragma once

#include <QtCore/QString>

extern "C" void rexon_do_register_we_types();

/// Язык подписей плагинов демона — вслед за языком интерфейса.
///
/// Оригинал делает это прямо в `App::setUiLanguage`
/// (`PluginTranslationStore::instance()->setLocale(...)`): каталоги плагинов
/// приходят на все языки сразу, и какой из них показывать, решает выбор
/// покупателя. Дверь здесь, а не в шиме синглтонов, потому что тип
/// `PluginTranslations` печатает cxx-qt и виден он только в этом файле.
///
/// `locale` — разрешённый язык в форме BCP 47 (`ru`, `zh-CN`).
extern "C" void rexon_we_set_plugin_locale(const QString& locale);

/// Перевод подписи плагина по её `localized_text` из схемы свойств обоины.
///
/// Оригинал зовёт `PluginTranslationStore::instance()->translate(...)` прямо
/// из модели свойств; у нас синглтон печатает cxx-qt и виден он только в
/// `rexon_register_we.cpp`, поэтому дверь здесь. На входе — сырой JSON
/// объекта `{pluginId, msgid}`; пустая строка на выходе означает «перевода
/// нет», и модель показывает подпись из схемы.
QString rexon_we_plugin_translate(const QString& message_json);
