// Registration of a cxx-qt QObject in the waywallen.ui QML module. Called from Rust
// (the rexon_register_filter_model wrapper): the symbol is exported by Rust, C++
// functions do not get into the cdylib's dynamic table.
#pragma once

void rexon_do_register_filter_model();
