#include "rexon_ext_dialogs.h"

#include <cstdint>

#include <QtCore/QCoreApplication>
#include <QtCore/QEventLoop>
#include <QtCore/QMetaMethod>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtCore/QStringList>
#include <QtCore/QUrl>
#include <QtCore/QVariant>
#include <QtGui/QDesktopServices>
#include <QtGui/QGuiApplication>
#include <QtGui/QWindow>
#include <QtQml/QQmlComponent>
#include <QtQml/QQmlEngine>

namespace {

/// A separate QML engine for the dialogs.
///
/// The page's engine is deliberately not used: the actions object may be
/// created outside QML too (by checks, by a test bench), and then
/// `qmlEngine(this)` would return nullptr and the dialog would silently not
/// open. Our own engine works the same in both cases and lives until the
/// end of the process — like the tray icon itself.
QQmlEngine* dialog_engine()
{
    static QQmlEngine* engine = nullptr;
    if (engine == nullptr && QCoreApplication::instance() != nullptr) {
        engine = new QQmlEngine(QCoreApplication::instance());
    }
    return engine;
}

/// The application window the dialog will become a child of. Without it the
/// dialog is not modal and on Wayland may open behind the main window.
QWindow* parent_window()
{
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow* w : windows) {
        if (w != nullptr && w->isVisible())
            return w;
    }
    return windows.isEmpty() ? nullptr : windows.first();
}

/// Create an object from QML text and spin a nested loop until QML raises
/// the `finished` property.
///
/// Returns the object itself (owned by the caller) or nullptr if the QML
/// did not compile. Staying silent on an error is not allowed: the user
/// would take an empty dialog for a program breakage, so the reason goes to
/// a Qt warning.
QObject* run_dialog(const char* qml, const char* name,
                    const QList<QPair<const char*, QVariant>>& props)
{
    QQmlEngine* engine = dialog_engine();
    if (engine == nullptr)
        return nullptr;

    QQmlComponent component(engine);
    component.setData(QByteArray(qml), QUrl(QString("qrc:/rexon/ext_dialogs/%1.qml").arg(name)));
    if (component.isError()) {
        qWarning("rexon: диалог «%s» не скомпилировался: %s", name,
                 qPrintable(component.errorString()));
        return nullptr;
    }
    QObject* root = component.create();
    if (root == nullptr) {
        qWarning("rexon: диалог «%s» не создался", name);
        return nullptr;
    }
    QQmlEngine::setObjectOwnership(root, QQmlEngine::CppOwnership);
    for (const auto& p : props)
        root->setProperty(p.first, p.second);
    root->setProperty("win", QVariant::fromValue(parent_window()));

    // We wait for the `finished` property to change. The signal is taken as a
    // metamethod, not with the SIGNAL macro: the object type is known only at
    // run time (QML emitted it), and a lambda connection requires a
    // pointer-to-member.
    const QMetaObject* mo = root->metaObject();
    const int sig = mo->indexOfSignal("finishedChanged()");
    const int slot = QEventLoop::staticMetaObject.indexOfSlot("quit()");
    if (sig < 0 || slot < 0) {
        delete root;
        return nullptr;
    }

    QEventLoop loop;
    QPointer<QObject> guard(root);
    QObject::connect(root, mo->method(sig), &loop, QEventLoop::staticMetaObject.method(slot));
    if (!QMetaObject::invokeMethod(root, "start")) {
        delete root;
        return nullptr;
    }
    // `finished` may have been raised right in `start()` (the portal refused
    // at once) — then there is no loop left to spin, otherwise it would never
    // end.
    if (!root->property("finished").toBool())
        loop.exec();
    if (guard.isNull())
        return nullptr;
    return root;
}

}  // namespace

// ── choosing a file ───────────────────────────────────────────────────────

QString rexon_ext_pick_file(const QString& title, const QString& filter,
                            const QString& folder)
{
    static const char* kQml = R"QML(
import QtQuick
import QtQuick.Dialogs

QtObject {
    id: root
    property bool finished: false
    property bool ok: false
    property url chosen
    property string dlgTitle: ""
    property var dlgFilters: []
    property url dlgFolder
    property var win: null

    property FileDialog dlg: FileDialog {
        title: root.dlgTitle
        nameFilters: root.dlgFilters
        currentFolder: root.dlgFolder
        fileMode: FileDialog.OpenFile
        parentWindow: root.win
        onAccepted: { root.chosen = selectedFile; root.ok = true; root.finished = true; }
        onRejected: { root.ok = false; root.finished = true; }
    }

    function start() { dlg.open(); }
}
)QML";

    QStringList filters;
    for (const QString& part : filter.split(QStringLiteral(";;"), Qt::SkipEmptyParts))
        filters << part.trimmed();

    QList<QPair<const char*, QVariant>> props;
    props << qMakePair("dlgTitle", QVariant(title));
    props << qMakePair("dlgFilters", QVariant(filters));
    props << qMakePair("dlgFolder", QVariant(QUrl::fromLocalFile(folder)));

    QObject* root = run_dialog(kQml, "FilePick", props);
    if (root == nullptr)
        return QString();
    QString path;
    if (root->property("ok").toBool())
        path = root->property("chosen").toUrl().toLocalFile();
    delete root;
    return path;
}

// ── saving a file ─────────────────────────────────────────────────────────

QString rexon_ext_save_file(const QString& title, const QString& filter,
                            const QString& path)
{
    // The same dialog as for choosing a file, in save mode: the name is filled
    // in, and the dialog itself asks about replacing an existing file.
    static const char* kQml = R"QML(
import QtQuick
import QtQuick.Dialogs

QtObject {
    id: root
    property bool finished: false
    property bool ok: false
    property url chosen
    property string dlgTitle: ""
    property var dlgFilters: []
    property url dlgFolder
    property url dlgFile
    property var win: null

    property FileDialog dlg: FileDialog {
        title: root.dlgTitle
        nameFilters: root.dlgFilters
        currentFolder: root.dlgFolder
        selectedFile: root.dlgFile
        fileMode: FileDialog.SaveFile
        parentWindow: root.win
        onAccepted: { root.chosen = selectedFile; root.ok = true; root.finished = true; }
        onRejected: { root.ok = false; root.finished = true; }
    }

    function start() { dlg.open(); }
}
)QML";

    QStringList filters;
    for (const QString& part : filter.split(QStringLiteral(";;"), Qt::SkipEmptyParts))
        filters << part.trimmed();

    const QUrl file = QUrl::fromLocalFile(path);
    QList<QPair<const char*, QVariant>> props;
    props << qMakePair("dlgTitle", QVariant(title));
    props << qMakePair("dlgFilters", QVariant(filters));
    props << qMakePair("dlgFolder", QVariant(file.adjusted(QUrl::RemoveFilename)));
    props << qMakePair("dlgFile", QVariant(file));

    QObject* root = run_dialog(kQml, "FileSave", props);
    if (root == nullptr)
        return QString();
    QString chosen;
    if (root->property("ok").toBool())
        chosen = root->property("chosen").toUrl().toLocalFile();
    delete root;
    return chosen;
}

// ── a "yes/no" question ───────────────────────────────────────────────────

bool rexon_ext_confirm(const QString& title, const QString& text, const QString& skin)
{
    // The question window is drawn by Rexon itself, in its skin colours.
    // Previously it was the system `MessageDialog`: a light window of a
    // foreign style over the dark Rexon (acceptance 10.10.2026). The button
    // colours and labels arrive as the `skin` string
    // (`extensions_actions::confirm_skin`) — the shim has no decisions of its
    // own.
    static const char* kQml = R"QML(
import QtQuick
import QtQuick.Window

QtObject {
    id: root
    property bool finished: false
    property bool ok: false
    property string dlgTitle: ""
    property string dlgText: ""
    property var skin: []
    property var win: null

    function tone(i, fallback) { return (skin.length > i && skin[i] !== "") ? skin[i] : fallback; }
    readonly property color colBg: tone(0, "#1b2838")
    readonly property color colCard: tone(1, "#2a475e")
    readonly property color colText: tone(2, "#ffffff")
    readonly property color colAccent: tone(3, "#66c0f4")
    readonly property color colBorder: tone(4, "#3d4450")
    readonly property string yesText: tone(5, "Yes")
    readonly property string noText: tone(6, "No")

    // The answer is accepted — it does not change a second time. Closing the window itself calls
    // `answer(false)` (the close button means «Нет»); without this latch it
    // overwrote the «Да» just given.
    property bool answered: false

    function answer(yes) {
        if (root.answered)
            return;
        root.answered = true;
        root.ok = yes;
        dlg.close();
        root.finished = true;
    }

    property Window dlg: Window {
        title: root.dlgTitle
        transientParent: root.win
        modality: Qt.ApplicationModal
        flags: Qt.Dialog
        color: root.colBg
        // The height follows the text. The limits are bound to the same value, not to
        // `height`: the binding «limit = height» looped on itself, and
        // the window got stuck one line high.
        readonly property int wantHeight: body.implicitHeight + 40
        width: 480
        height: wantHeight
        minimumWidth: 480
        maximumWidth: 480
        minimumHeight: wantHeight
        maximumHeight: wantHeight
        // Closed with the close button — that is «Нет»: a silent «yes» is not allowed.
        onClosing: root.answer(false)

        Column {
            id: body
            x: 20
            y: 20
            width: parent.width - 40
            spacing: 18

            Text {
                width: parent.width
                text: root.dlgText
                textFormat: Text.PlainText
                wrapMode: Text.WordWrap
                color: root.colText
                font.pointSize: 11
            }

            Row {
                anchors.right: parent.right
                spacing: 10

                Repeater {
                    model: [ { "yes": true, "label": root.yesText }, { "yes": false, "label": root.noText } ]

                    Rectangle {
                        id: btn
                        required property var modelData
                        width: Math.max(96, label.implicitWidth + 32)
                        height: 40
                        radius: 3
                        color: btn.modelData.yes ? root.colAccent
                                                 : (hover.hovered ? Qt.lighter(root.colCard, 1.15) : root.colCard)
                        border.color: btn.activeFocus ? root.colText : root.colBorder
                        border.width: btn.activeFocus ? 2 : 1
                        activeFocusOnTab: true
                        // Focus on opening — on «Нет»: an accidental Enter
                        // deletes nothing.
                        focus: !btn.modelData.yes
                        Keys.onReturnPressed: root.answer(btn.modelData.yes)
                        Keys.onEnterPressed: root.answer(btn.modelData.yes)
                        Keys.onSpacePressed: root.answer(btn.modelData.yes)
                        Keys.onEscapePressed: root.answer(false)

                        Text {
                            id: label
                            anchors.centerIn: parent
                            text: btn.modelData.label
                            color: btn.modelData.yes ? root.colBg : root.colText
                            font.bold: true
                        }
                        HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: root.answer(btn.modelData.yes) }
                    }
                }
            }
        }
    }

    function start() { dlg.show(); dlg.requestActivate(); }
}
)QML";

    QList<QPair<const char*, QVariant>> props;
    props << qMakePair("dlgTitle", QVariant(title));
    props << qMakePair("dlgText", QVariant(text));
    props << qMakePair("skin", QVariant(skin.split(QChar(0x1f))));

    QObject* root = run_dialog(kQml, "Confirm", props);
    if (root == nullptr)
        return false;
    const bool ok = root->property("ok").toBool();
    delete root;
    return ok;
}

// ── choosing a font ───────────────────────────────────────────────────────

QString rexon_ext_pick_font(const QString& title, const QString& family,
                            std::int32_t size, std::int32_t weight,
                            bool italic, std::int32_t& outcome)
{
    static const char* kQml = R"QML(
import QtQuick
import QtQuick.Dialogs

QtObject {
    id: root
    property bool finished: false
    property bool ok: false
    property string chosen: ""
    property string dlgTitle: ""
    property string famIn: ""
    property int sizeIn: 0
    property int weightIn: 0
    property bool italicIn: false
    property var win: null

    property FontDialog dlg: FontDialog {
        title: root.dlgTitle
        parentWindow: root.win
        // The family, size, weight and slant are joined here, while
        // the core parses them (`parse_font_spec`, `parse_font_face`): the shim does not get
        // its own font string parsing.
        onAccepted: {
            root.chosen = selectedFont.family + "," + selectedFont.pointSize
                + "," + selectedFont.weight + "," + (selectedFont.italic ? 1 : 0);
            root.ok = true;
            root.finished = true;
        }
        onRejected: { root.ok = false; root.finished = true; }
    }

    // The current face (weight on the Qt 6 scale, slant) — selected
    // on opening: otherwise the window would offer regular instead of bold.
    function start() {
        if (root.famIn !== "") {
            var spec = { family: root.famIn };
            if (root.sizeIn > 0)
                spec.pointSize = root.sizeIn;
            if (root.weightIn > 0)
                spec.weight = root.weightIn;
            if (root.italicIn)
                spec.italic = true;
            dlg.selectedFont = Qt.font(spec);
        }
        dlg.open();
    }
}
)QML";

    QList<QPair<const char*, QVariant>> props;
    props << qMakePair("dlgTitle", QVariant(title));
    props << qMakePair("famIn", QVariant(family));
    props << qMakePair("sizeIn", QVariant(static_cast<int>(size)));
    props << qMakePair("weightIn", QVariant(static_cast<int>(weight)));
    props << qMakePair("italicIn", QVariant(italic));

    QObject* root = run_dialog(kQml, "FontPick", props);
    if (root == nullptr) {
        // The window did not come up — this is NOT "changed their mind": the tab
        // must tell the user different things, otherwise the button reads as
        // broken.
        outcome = -1;
        return QString();
    }
    QString spec;
    if (root->property("ok").toBool()) {
        outcome = 1;
        spec = root->property("chosen").toString();
    } else {
        outcome = 0;
    }
    delete root;
    return spec;
}

// ── a directory in the file manager ───────────────────────────────────────

bool rexon_ext_open_folder(const QString& path)
{
    if (path.isEmpty())
        return false;
    return QDesktopServices::openUrl(QUrl::fromLocalFile(path));
}

// ── an item page in the browser ───────────────────────────────────────────

bool rexon_ext_open_url(const QString& url)
{
    if (url.isEmpty())
        return false;
    // Warning: not `QUrl::fromLocalFile` as for the directory above: what comes
    // here is a SITE address (a theme page on pling, a package page on
    // aur.archlinux.org). A local wrapper would turn "https://…" into a path to
    // a non-existent file, and the "Open page" button would silently do
    // nothing.
    const QUrl parsed(url);
    if (!parsed.isValid() || parsed.scheme().isEmpty())
        return false;
    return QDesktopServices::openUrl(parsed);
}
