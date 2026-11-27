// Factory of child objects of the App live models (renderers, displays,
// canvases, GPUs).
//
// A separate header rather than lines in rexon_gui_factory.h: cxx emits a
// shim wrapper for every `extern "C++"` declaration, keyed by the C++
// function name, and the same name declared in two bridges of the crate
// gives a `duplicate symbol` at link time. The names here are our own, and
// they are declared in exactly one bridge — src/we_objmodel.rs.
#pragma once

#include <QtCore/QString>

class QObject;
class QVariant;

/// Create a `Renderer`/`Display`/`Canvas`/`Gpu` as a child of the manager.
/// Ownership belongs to Qt.
QObject* rexon_new_renderer(QObject* parent);
QObject* rexon_new_display(QObject* parent);
QObject* rexon_new_canvas(QObject* parent);
QObject* rexon_new_gpu(QObject* parent);

/// Create the live models as children of App (called by the bridge
/// src/we_app.rs).
QObject* rexon_new_renderer_manager(QObject* parent);
QObject* rexon_new_display_manager(QObject* parent);
QObject* rexon_new_library_manager(QObject* parent);
QObject* rexon_new_gpu_manager(QObject* parent);
QObject* rexon_new_presentation_manager(QObject* parent);

/// The row model of the "Current wallpapers" card — a child of the display
/// manager.
QObject* rexon_new_now_playing_model(QObject* parent);

/// A translation catalogue string by context and source text
/// (`QCoreApplication::translate`). Rust does not translate its own strings
/// this way — this is only for the VENDOR's strings, which have a `.qm`
/// catalogue and a C++ class context.
QString rexon_we_vendor_tr(const QString& context, const QString& source);

/// Delete a child object deferred.
///
/// Exactly `deleteLater`, not `delete`: QML bindings of the current frame
/// may hang on the object (`Repeater { model: rendererManager.renderers }`),
/// and immediate deletion would pull the rug out from under them. The
/// original does the same (`renderer.cpp`, `display.cpp`).
void rexon_delete_child_later(QObject* object);

/// A pointer to a QObject as a QVariant value — that is how the vendored QML
/// gets a list of objects (the `QVariantList` of the `renderers`/`displays`
/// properties). cxx-qt-lib cannot fill its QVariant with a QObject pointer.
QVariant rexon_object_to_variant(QObject* object);
