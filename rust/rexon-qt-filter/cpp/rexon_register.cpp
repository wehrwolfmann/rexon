// Императивная регистрация типа в QML — как в C++-оригинале
// (filter_rule_model.cpp): окно вкладки обоев грузит librexon_qt_filter.so
// через dlopen и зовёт rexon_register_filter_model() по dlsym до загрузки
// QML (rexon-gui, we_env::register_filter_model). Регистрируем
// сгенерированный cxx-qt QObject в "waywallen.ui".
//
// Вызывается через мост из Rust-обёртки (см. filter_rule_model.rs), поэтому
// это обычная C++-функция, а не extern "C": экспортируемый символ отдаёт
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
