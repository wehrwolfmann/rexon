// A snapshot of a form made of real Qt widgets in a given style — the
// executor of the `qt` mode of the `rexon-theme-shot` helper
// (src/bin/theme_shot/).
//
// What to capture, the palette, labels and sizes are decided by the core
// (rexon_core::widget_shot) — here there is only what cannot be reached
// from Rust: QApplication, QStyleFactory, QWidget and its layouts,
// QWidget::render(). cxx-qt-lib has no widgets at all.
#pragma once

#include <cstdint>

extern "C" {

// Return codes.
enum {
    REXON_SHOT_OK = 0,
    // The style with this key did not load.
    REXON_SHOT_NO_STYLE = 3,
    // The snapshot was not written.
    REXON_SHOT_SAVE_FAILED = 4,
};

// Bring up QApplication, build a form of size form_w×form_h, give all
// widgets the style `style` and the palette, draw it `scale` times larger
// (rexon_core::widget_shot::render_scale), fit it with the aspect ratio
// preserved into a transparent out_w×out_h frame and write a PNG to `out`.
//
// The palette is `count` pairs of a `QPalette::ColorRole` role name (with
// the "Disabled:" prefix — only for the "disabled" group, otherwise for
// all groups) and a "#rrggbb" colour; count == 0 means the style's native
// palette. The labels `texts` go in order: two tabs, the main and the
// second button, a check box, a radio button, a list, the field text
// (text_count == 8).
int rexon_qt_style_shot(const char *style, const char *const *roles,
                        const char *const *colors, int32_t count,
                        const char *const *texts, int32_t text_count,
                        int32_t form_w, int32_t form_h, int32_t out_w,
                        int32_t out_h, int32_t scale, const char *out);

} // extern "C"
