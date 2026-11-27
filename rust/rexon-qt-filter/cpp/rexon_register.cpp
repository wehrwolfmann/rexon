// Imperative registration of the type in QML — as in the C++ original
// (filter_rule_model.cpp): the wallpaper tab window loads librexon_qt_filter.so
// via dlopen and calls rexon_register_filter_model() through dlsym before loading
// QML (rexon-gui, we_env::register_filter_model). We register the
// generated cxx-qt QObject in "waywallen.ui".
//
// Called through the bridge from the Rust wrapper (see filter_rule_model.rs), so
// this is a plain C++ function, not extern "C": the exported symbol is provided by
// Rust (#[no_mangle] pub extern "C" rexon_register_filter_model).
#include "rexon_register.h"

#include "rexon-qt-filter/src/filter_rule_model.cxxqt.h"

#include <QtQml/qqml.h>

void rexon_do_register_filter_model()
{
    static bool registered = false;
    if (registered)
        return;
    registered = true;
    qmlRegisterType<WallpaperFilterRuleModel>("waywallen.ui", 1, 0,
                                              "WallpaperFilterRuleModel");
}
