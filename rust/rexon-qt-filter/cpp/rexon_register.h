// Регистрация cxx-qt QObject в QML-модуле waywallen.ui. Вызывается из Rust
// (обёртка rexon_register_filter_model): символ экспортирует Rust, C++-
// функции в динамическую таблицу cdylib не попадают.
#pragma once

void rexon_do_register_filter_model();
