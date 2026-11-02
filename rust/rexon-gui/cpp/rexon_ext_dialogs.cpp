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

/// Отдельный движок QML для диалогов.
///
/// Не берётся движок страницы намеренно: объект действий может создаваться и
/// вне QML (проверками, стендом), и тогда `qmlEngine(this)` вернул бы nullptr,
/// а диалог молча не открылся бы. Свой движок работает одинаково в обоих
/// случаях и живёт до конца процесса — как и сам значок в лотке.
QQmlEngine* dialog_engine()
{
    static QQmlEngine* engine = nullptr;
    if (engine == nullptr && QCoreApplication::instance() != nullptr) {
        engine = new QQmlEngine(QCoreApplication::instance());
    }
    return engine;
}

/// Окно приложения, которому диалог станет дочерним. Без него диалог не
/// модален и на Wayland может открыться за главным окном.
QWindow* parent_window()
{
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow* w : windows) {
        if (w != nullptr && w->isVisible())
            return w;
    }
    return windows.isEmpty() ? nullptr : windows.first();
}

/// Создать объект из QML-текста и прокрутить вложенный цикл, пока QML не
/// поднимет свойство `finished`.
///
/// Возврат — сам объект (владение у вызывающего) либо nullptr, если QML не
/// скомпилировался. Молчать при ошибке нельзя: пустой диалог человек примет
/// за поломку программы, поэтому причина уходит в предупреждение Qt.
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

    // Ждём смены свойства `finished`. Сигнал берётся метаметодом, а не
    // макросом SIGNAL: тип объекта известен только в рантайме (его напечатал
    // QML), а лямбда-подключение требует указателя на член.
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
    // `finished` мог подняться прямо в `start()` (портал отказал сразу) — тогда
    // цикл крутить уже нечего, иначе он не завершился бы никогда.
    if (!root->property("finished").toBool())
        loop.exec();
    if (guard.isNull())
        return nullptr;
    return root;
}

}  // namespace

// ── выбор файла ───────────────────────────────────────────────────────────

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

// ── сохранение файла ──────────────────────────────────────────────────────

QString rexon_ext_save_file(const QString& title, const QString& filter,
                            const QString& path)
{
    // Тот же диалог, что выбор файла, в режиме сохранения: имя подставлено,
    // о замене существующего файла спрашивает сам диалог.
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

// ── вопрос «да/нет» ───────────────────────────────────────────────────────

bool rexon_ext_confirm(const QString& title, const QString& text, const QString& skin)
{
    // Окно вопроса рисуется самим Rexon, в цветах его скина. Прежде это был
    // системный `MessageDialog`: светлое окно чужого стиля поверх тёмного
    // Rexon (приёмка 10.10.2026). Цвета и подписи кнопок приходят строкой
    // `skin` (`extensions_actions::confirm_skin`) — своих решений у шима нет.
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

    // Ответ принят — второй раз не меняется. Закрытие окна само зовёт
    // `answer(false)` (крестик — это «Нет»); без этой защёлки оно
    // перезаписывало только что данное «Да».
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
        // Высота — по тексту. Пределы привязаны к той же величине, а не к
        // `height`: привязка «предел = высота» замыкалась сама на себя, и
        // окно застревало высотой в одну строку.
        readonly property int wantHeight: body.implicitHeight + 40
        width: 480
        height: wantHeight
        minimumWidth: 480
        maximumWidth: 480
        minimumHeight: wantHeight
        maximumHeight: wantHeight
        // Закрыли крестиком — это «Нет»: молчаливое «да» недопустимо.
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
                        // Фокус при открытии — на «Нет»: случайный Enter
                        // ничего не удалит.
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

// ── выбор шрифта ──────────────────────────────────────────────────────────

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
        // Семейство, кегль, жирность и наклон склеиваются здесь, а
        // разбирает их ядро (`parse_font_spec`, `parse_font_face`): своего
        // разбора шрифтовой строки в шиме не заводится.
        onAccepted: {
            root.chosen = selectedFont.family + "," + selectedFont.pointSize
                + "," + selectedFont.weight + "," + (selectedFont.italic ? 1 : 0);
            root.ok = true;
            root.finished = true;
        }
        onRejected: { root.ok = false; root.finished = true; }
    }

    // Нынешнее начертание (жирность по шкале Qt 6, наклон) — выбранным
    // при открытии: иначе окно предлагало бы обычный вместо жирного.
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
        // Окно не поднялось — это НЕ «передумал»: вкладка обязана сказать
        // человеку разное, иначе кнопка читается как поломка.
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

// ── каталог в файловом менеджере ──────────────────────────────────────────

bool rexon_ext_open_folder(const QString& path)
{
    if (path.isEmpty())
        return false;
    return QDesktopServices::openUrl(QUrl::fromLocalFile(path));
}

// ── страница товара в браузере ────────────────────────────────────────────

bool rexon_ext_open_url(const QString& url)
{
    if (url.isEmpty())
        return false;
    // ⚠️ Не `QUrl::fromLocalFile`, как у каталога выше: сюда приходит адрес
    // САЙТА (страница темы на pling, страница пакета на aur.archlinux.org).
    // Локальная обёртка превратила бы «https://…» в путь к несуществующему
    // файлу, и кнопка «Открыть страницу» молча ничего не делала бы.
    const QUrl parsed(url);
    if (!parsed.isValid() || parsed.scheme().isEmpty())
        return false;
    return QDesktopServices::openUrl(parsed);
}
