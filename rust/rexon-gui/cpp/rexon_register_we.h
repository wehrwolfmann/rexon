// Registration of rexon-gui Rust types in the QML module of waywallen's
// vendored interface (`waywallen.ui`). Called from the Rust wrapper
// `register_we_types` (src/lib.rs) while preparing the process
// (`we_env::prepare_process`).
//
// The registration is imperative rather than through the `org.rexon.gui`
// QML module: the vendored QML imports `waywallen.ui`, and a Rust type must
// stand under the same URI and the same name that a Python class used to
// occupy.
#pragma once

#include <QtCore/QString>

extern "C" void rexon_do_register_we_types();

/// Language of the daemon's plugin labels — following the interface
/// language.
///
/// The original does this right in `App::setUiLanguage`
/// (`PluginTranslationStore::instance()->setLocale(...)`): plugin catalogues
/// arrive for all languages at once, and which one to show is decided by the
/// customer's choice. The door is here rather than in the singletons shim
/// because the `PluginTranslations` type is emitted by cxx-qt and is visible
/// only in this file.
///
/// `locale` is the resolved language in BCP 47 form (`ru`, `zh-CN`).
extern "C" void rexon_we_set_plugin_locale(const QString& locale);

/// Translating a plugin label by its `localized_text` from the wallpaper's
/// property schema.
///
/// The original calls `PluginTranslationStore::instance()->translate(...)`
/// right from the property model; for us the singleton is emitted by cxx-qt
/// and is visible only in `rexon_register_we.cpp`, so the door is here. The
/// input is the raw JSON of a `{pluginId, msgid}` object; an empty string on
/// output means "no translation", and the model shows the label from the
/// schema.
QString rexon_we_plugin_translate(const QString& message_json);
