// Крейт rexon-qt-filter — модель редактора фильтров вкладки Wallpaper
// Engine на Rust через cxx-qt (первая ласточка миграции GUI).
//
// Собирается в cdylib librexon_qt_filter.so; tools/build_qml_control.sh
// кладёт её в каталог QML-модулей рядом с waywallen.control, оттуда её
// грузит окно вкладки обоев (dlopen, rexon-gui we_env). Промежуточная C++-копия модели удалена
// 2026-07-27 — это единственная реализация.

pub mod filter_rule_model;
