// Снимок формы из настоящих виджетов Qt в заданном стиле — исполнитель
// режима `qt` помощника `rexon-theme-shot` (src/bin/theme_shot/).
//
// Что снимать, палитру, подписи, размеры решает ядро
// (rexon_core::widget_shot) — здесь только то, чего из Rust не достать:
// QApplication, QStyleFactory, QWidget и его раскладки, QWidget::render().
// Виджетов в cxx-qt-lib нет вовсе.
#pragma once

#include <cstdint>

extern "C" {

// Коды возврата.
enum {
    REXON_SHOT_OK = 0,
    // Стиль с таким ключом не загрузился.
    REXON_SHOT_NO_STYLE = 3,
    // Снимок не записался.
    REXON_SHOT_SAVE_FAILED = 4,
};

// Поднять QApplication, собрать форму размером form_w×form_h, назначить
// всем виджетам стиль `style` и палитру, нарисовать в `scale` раз крупнее
// (rexon_core::widget_shot::render_scale), вписать с сохранением сторон
// в прозрачный кадр out_w×out_h и записать PNG в `out`.
//
// Палитра — `count` пар имя роли `QPalette::ColorRole` (с приставкой
// "Disabled:" — только для группы «недоступно», иначе для всех групп) и
// цвет "#rrggbb"; count == 0 — родная палитра стиля. Подписи `texts` — по
// порядку: две вкладки, главная и вторая кнопка, флажок, переключатель,
// список, текст поля (text_count == 8).
int rexon_qt_style_shot(const char *style, const char *const *roles,
                        const char *const *colors, int32_t count,
                        const char *const *texts, int32_t text_count,
                        int32_t form_w, int32_t form_h, int32_t out_w,
                        int32_t out_h, int32_t scale, const char *out);

} // extern "C"
