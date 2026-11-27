// Base of the Action singleton: one single `toast` signal with DEFAULT
// arguments.
//
// Why a separate base rather than a signal in the cxx-qt bridge. In the
// original (`ui/src/action.cppm`) the signal is declared like this:
//
//     void toast(QString text, qint32 duration = 3000, qint32 flags = 0,
//                QObject* action = nullptr);
//
// and the vendored QML uses the defaults heavily — `W.Action.toast(text)`
// with ONE argument occurs in nine places. Default arguments are emitted
// by moc: it generates shortened clones of the method, and QML picks a
// clone by the number of arguments. cxx-qt cannot do defaults, and
// overloads of the same name break its own generator: it connects the
// signal through `&Action::toast`, and with four methods of the same name
// that is an "unresolved overloaded function type".
//
// So the signal stays what it was in the original — an ordinary Qt signal
// in a moc-processed header, and the cxx-qt bridge inherits it through
// `#[base]`. cxx-qt-build DOES run moc on headers from `cpp_files`
// (documented in `CxxQtBuilder::cpp_file`: "compile or run moc on"), but
// not on .cpp files.
//
// Warning: our Python bridge could not do this (a PySide6 Signal knows no
// defaults) and diverged from the original: the signal there was called
// `toasted`, and `toast` was a slot. Because of the renaming the vendored
// handler `Window.qml:86 function onToast(...)` never fired.
#pragma once

#include <QtCore/QObject>
#include <QtCore/QString>

class RexonWeActionBase : public QObject {
    Q_OBJECT

public:
    explicit RexonWeActionBase(QObject* parent = nullptr): QObject(parent) {}
    ~RexonWeActionBase() override = default;

Q_SIGNALS:
    /// A toast over the window. The signature and defaults are from the
    /// original.
    void toast(QString text, qint32 duration = 3000, qint32 flags = 0,
               QObject* action = nullptr);
};
