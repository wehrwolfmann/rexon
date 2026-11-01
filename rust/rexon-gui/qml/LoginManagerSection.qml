// Блок «Менеджер входа» в разделе экрана входа (над полкой тем SDDM и над
// панелью Plasma Login Manager): какие экраны входа установлены, какой
// основной и какой работает сейчас, «Сделать основным» с выбором
// приветствия LightDM, окно подтверждения и «Как вернуть».
//
// Только разметка. Данные, решения и тексты — ядро
// (`rexon_core::system_exec::login_screen_view`), приходят пробой
// `loginScreenReady`; переключение — `makeDefaultLoginManager` объекта
// действий вкладки, ответ — `loginManagerSwitched`. Полоса «экран входа
// сам открывает уходящий X11-сеанс» (`waylandDefault` пробы) — кнопка
// `makeWaylandDefaultSession`, ответ тот же.
//
// Защита от дурака (правило #13): кнопка заперта там, где нажатие не
// сработает, а причина видна серым сразу, до пароля; ядро всё равно
// проверяет заново и отказывает само.
//
// При SDDM и Plasma Login Manager ниже — блок «Фон и оформление экрана
// входа» (как «Изменить фон» и «Применить параметры Plasma» модулей KDE):
// данные — `sddmGreeter` или `plmGreeter` той же пробы (один вид,
// `system_plan::SddmGreeterView`), действия — `setLoginBackground` и
// `applyLoginLook` (ядро само выбирает путь по экрану входа), ответ —
// `loginLookDone`.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})
    /// Объект действий вкладки (`AppearanceActions`).
    property var actions: null

    /// Последний ответ пробы.
    property var st: ({})
    /// Выбранное приветствие по юниту (предвыбор — от ядра).
    property var greeterFor: ({})
    /// Идёт переключение — кнопки заперты.
    property bool busy: false
    /// Итог последнего переключения.
    property string message: ""
    property bool failed: false
    /// Блок фона и оформления экрана входа: идёт действие, итог.
    property bool lookBusy: false
    property string lookMessage: ""
    property bool lookFailed: false
    /// Открыто живое превью экрана входа.
    property bool previewRunning: false

    function take(payload) {
        if (!payload)
            return;
        try {
            section.st = JSON.parse(payload);
        } catch (e) {
            section.st = ({});
        }
        // Галочка после щелчка теряет привязку — состояние берётся из пробы
        // заново (не вышло — она возвращается как было).
        if (section.greeter)
            clockBox.checked = section.greeter.clock.checked === true;
    }

    function togglePreview() {
        if (!section.actions)
            return;
        if (section.previewRunning) {
            section.actions.closeLoginPreview();
            return;
        }
        const err = section.actions.previewLoginScreen("");
        if (err !== "") {
            section.lookFailed = true;
            section.lookMessage = err;
        }
    }

    function chosenGreeter(row) {
        const g = section.greeterFor[row.unit];
        return g !== undefined ? g : (row.greeter || "");
    }

    /// Итог переключения без повторов: после успеха «вступит после
    /// перезагрузки» уже говорит строка `pending`, а «как вернуть» — блок
    /// ниже; тогда итог не дублируется.
    function shownMessage() {
        if (section.failed || section.busy || section.message === "")
            return section.message;
        if ((section.st.pending || "") !== "")
            return "";
        return (section.st.rollback || "") !== "" ? section.message.split("\n\n")[0] : section.message;
    }

    function ask(row) {
        confirmDialog.lookAction = "";
        const greeter = section.chosenGreeter(row);
        const withGreeter = (row.greeters || []).find(g => g.id === greeter);
        confirmDialog.unit = row.unit;
        confirmDialog.session = "";
        confirmDialog.yesText = "";
        confirmDialog.greeter = withGreeter ? greeter : "";
        confirmDialog.heading = row.confirmTitle || "";
        confirmDialog.question = withGreeter ? withGreeter.confirm : (row.confirm || "");
        confirmDialog.open();
    }

    function askSession(offer) {
        confirmDialog.lookAction = "";
        confirmDialog.unit = "";
        confirmDialog.greeter = "";
        confirmDialog.session = offer.x11 || "";
        confirmDialog.yesText = offer.button || "";
        confirmDialog.heading = offer.button || "";
        confirmDialog.question = offer.confirm || "";
        confirmDialog.open();
    }

    function startSession(x11) {
        if (!section.actions || !section.actions.makeWaylandDefaultSession || section.busy)
            return;
        section.busy = true;
        section.failed = false;
        section.message = qsTranslate("rexon", "Changing the login screen…");
        section.actions.makeWaylandDefaultSession(x11);
    }

    function start(unit, greeter) {
        if (!section.actions || !section.actions.makeDefaultLoginManager || section.busy)
            return;
        section.busy = true;
        section.failed = false;
        section.message = qsTranslate("rexon", "Changing the login screen…");
        section.actions.makeDefaultLoginManager(unit, greeter);
    }

    Connections {
        target: section.actions
        ignoreUnknownSignals: true

        function onLoginScreenReady(payload) {
            section.take(payload);
        }

        function onLoginManagerSwitched(ok, text) {
            section.busy = false;
            section.failed = !ok;
            section.message = text;
        }

        function onLoginLookDone(ok, text) {
            section.lookBusy = false;
            section.lookFailed = !ok;
            section.lookMessage = text;
        }

        function onLoginPreviewChanged(running, text) {
            section.previewRunning = running;
            if (text !== "") {
                section.lookFailed = true;
                section.lookMessage = text;
            }
        }
    }

    function startLook(kind, path) {
        if (!section.actions || section.lookBusy)
            return;
        section.lookBusy = true;
        section.lookFailed = false;
        section.lookMessage = qsTranslate("rexon", "Changing the login screen…");
        if (kind === "background")
            section.actions.setLoginBackground(path);
        else
            section.actions.applyLoginLook(kind === "lookReset");
    }

    function askLook(kind) {
        const look = section.greeter ? section.greeter.look : null;
        if (!look)
            return;
        confirmDialog.unit = "";
        confirmDialog.greeter = "";
        confirmDialog.session = "";
        confirmDialog.lookAction = kind;
        confirmDialog.yesText = kind === "lookReset" ? qsTranslate("rexon", "Reset") : qsTranslate("rexon", "Apply");
        confirmDialog.heading = kind === "lookReset" ? (look.reset || "").replace("…", "") : (look.confirmTitle || "");
        confirmDialog.question = kind === "lookReset" ? (look.resetConfirm || "") : (look.confirm || "");
        confirmDialog.open();
    }

    Component.onCompleted: {
        // Ответ пробы мог прийти раньше, чем блок подключился.
        if (section.actions && section.actions.loginScreen)
            section.take(section.actions.loginScreen());
    }

    readonly property var managers: section.st.managers || []
    /// Экран входа сам открывает уходящий X11-сеанс (`null` — нет).
    readonly property var sessionOffer: section.st.waylandDefault || null
    /// Блок фона и оформления экрана входа (SDDM или Plasma Login Manager;
    /// `null` — нет).
    readonly property var greeter: section.st.sddmGreeter && section.st.sddmGreeter.visible
                                   ? section.st.sddmGreeter
                                   : (section.st.plmGreeter && section.st.plmGreeter.visible
                                      ? section.st.plmGreeter : null)

    visible: section.managers.length > 0 || section.sessionOffer !== null || section.greeter !== null
    color: section.sty.group ? section.sty.group.background : section.pal.bg_card
    border.color: section.sty.group ? section.sty.group.border : section.pal.border
    border.width: 1
    radius: section.sty.group ? section.sty.group.radius : 6
    implicitHeight: section.visible ? body.implicitHeight + 2 * (body.y) : 0

    ColumnLayout {
        id: body
        x: (section.sty.group ? section.sty.group.marginH : 12) + section.border.width
        y: (section.sty.group ? section.sty.group.marginV : 10) + section.border.width
        width: parent.width - 2 * x
        spacing: section.sty.group ? section.sty.group.spacing : 8

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Login manager")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "The program that shows the sign-in screen when the computer starts. A new default one appears after a restart; your current session isn't interrupted.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // Уходящий X11-сеанс по умолчанию — предупреждение и кнопка.
        RowLayout {
            Layout.fillWidth: true
            visible: section.sessionOffer !== null
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: section.sessionOffer ? section.sessionOffer.text : ""
                color: section.pal.state_warn
                wrapMode: Text.WordWrap
            }

            RexonButton {
                objectName: "loginWaylandDefault"
                pal: section.pal
                enabled: !section.busy
                text: section.sessionOffer ? section.sessionOffer.button : ""
                onClicked: section.askSession(section.sessionOffer)
            }
        }

        Repeater {
            model: section.managers

            delegate: Rectangle {
                id: row
                required property var modelData

                objectName: "loginManager_" + row.modelData.unit
                Layout.fillWidth: true
                implicitHeight: rowLayout.implicitHeight + 16
                radius: 6
                color: section.pal.bg_card
                border.width: row.modelData.active ? 2 : 1
                border.color: row.modelData.active ? section.pal.accent : section.pal.border

                RowLayout {
                    id: rowLayout
                    x: 12
                    y: 8
                    width: parent.width - 24
                    spacing: 10

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            Text {
                                text: row.modelData.label
                                color: section.pal.text_primary
                                font.bold: true
                            }

                            // «Основной» — метка, а не кнопка.
                            Text {
                                visible: row.modelData.active === true
                                text: qsTranslate("rexon", "Default", "login manager")
                                color: section.pal.accent
                                font.bold: true
                            }

                            Text {
                                visible: row.modelData.running === true && row.modelData.active !== true
                                text: qsTranslate("rexon", "Running now")
                                color: section.pal.text_secondary
                            }

                            Item { Layout.fillWidth: true }
                        }

                        Text {
                            Layout.fillWidth: true
                            visible: text !== ""
                            text: row.modelData.reason || ""
                            color: section.pal.text_disabled !== undefined ? section.pal.text_disabled
                                                                          : section.pal.text_secondary
                            wrapMode: Text.WordWrap
                        }

                        Repeater {
                            model: row.modelData.warnings || []

                            delegate: Text {
                                required property string modelData
                                Layout.fillWidth: true
                                text: "• " + modelData
                                color: section.pal.state_warn
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    Text {
                        visible: greeterCombo.visible
                        text: qsTranslate("rexon", "Greeter:")
                        color: section.pal.text_secondary
                    }

                    RexonComboBox {
                        id: greeterCombo
                        objectName: "loginGreeter_" + row.modelData.unit
                        pal: section.pal
                        visible: row.modelData.active !== true && (row.modelData.greeters || []).length > 0
                        enabled: !section.busy && row.modelData.canSwitch === true
                        model: row.modelData.greeters || []
                        textRole: "name"
                        valueRole: "id"
                        selectedValue: section.chosenGreeter(row.modelData)
                        onActivated: {
                            const next = Object.assign({}, section.greeterFor);
                            next[row.modelData.unit] = currentValue;
                            section.greeterFor = next;
                        }
                    }

                    RexonButton {
                        objectName: "loginMakeDefault_" + row.modelData.unit
                        visible: row.modelData.active !== true
                        enabled: !section.busy && section.st.switchable === true
                                 && row.modelData.canSwitch === true
                        pal: section.pal
                        text: qsTranslate("rexon", "Make default")
                        onClicked: section.ask(row.modelData)
                    }
                }
            }
        }

        Text {
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.pending || ""
            color: section.pal.accent
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            visible: text !== ""
            text: section.shownMessage()
            color: section.failed ? section.pal.state_warn : section.pal.text_primary
            wrapMode: Text.WordWrap
        }

        // «Как вернуть» — текст, который помощник оставил для консоли
        // (`/etc/rexon/previous-display-manager`). Выделяется и копируется.
        Text {
            Layout.fillWidth: true
            Layout.topMargin: 4
            visible: rollbackText.text !== ""
            text: qsTranslate("rexon", "How to bring back the previous login screen")
            color: section.pal.text_primary
            font.bold: true
            wrapMode: Text.WordWrap
        }

        TextEdit {
            id: rollbackText
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.rollback || ""
            readOnly: true
            selectByMouse: true
            color: section.pal.text_secondary
            selectionColor: section.pal.accent
            wrapMode: TextEdit.Wrap
        }

        // ── Фон и оформление экрана входа (SDDM, Plasma Login Manager) ───
        ColumnLayout {
            id: greeterBlock
            objectName: "sddmGreeterBlock"
            Layout.fillWidth: true
            Layout.topMargin: 8
            visible: section.greeter !== null
            spacing: 8

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 1
                color: section.pal.border
            }

            Text {
                Layout.fillWidth: true
                text: section.greeter ? section.greeter.title : ""
                color: section.pal.text_primary
                font.bold: true
                font.pointSize: 13
                wrapMode: Text.WordWrap
            }

            // Тема — только у SDDM: у Plasma Login Manager тем нет.
            Text {
                Layout.fillWidth: true
                visible: text !== ""
                text: section.greeter ? section.greeter.theme : ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                visible: text !== ""
                text: section.greeter ? section.greeter.blocked : ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                // Превью фона: картинка темы или своя.
                Rectangle {
                    Layout.preferredWidth: 192
                    Layout.preferredHeight: 108
                    radius: 4
                    color: section.pal.bg_secondary
                    border.color: section.pal.border
                    border.width: 1
                    clip: true

                    Image {
                        id: greeterPreview
                        objectName: "sddmGreeterPreview"
                        anchors.fill: parent
                        anchors.margins: 1
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: false
                        sourceSize.width: 384
                        source: section.greeter && section.greeter.background.image !== ""
                                ? "file://" + section.greeter.background.image : ""
                    }

                    Text {
                        anchors.centerIn: parent
                        visible: greeterPreview.status !== Image.Ready
                        text: qsTranslate("rexon", "No picture")
                        color: section.pal.text_secondary
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 6

                    Text {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: section.greeter ? (section.greeter.background.state || section.greeter.background.note) : ""
                        color: section.pal.text_secondary
                        wrapMode: Text.WordWrap
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: 8

                        RexonButton {
                            objectName: "sddmBackgroundChoose"
                            pal: section.pal
                            enabled: !section.lookBusy && section.greeter !== null
                                     && section.greeter.canChange && section.greeter.background.supported
                            text: section.greeter ? section.greeter.background.choose : ""
                            onClicked: backgroundDialog.open()
                        }

                        RexonButton {
                            objectName: "sddmBackgroundReset"
                            pal: section.pal
                            visible: section.greeter !== null && section.greeter.background.canReset
                            enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                            text: section.greeter ? section.greeter.background.reset : ""
                            onClicked: section.startLook("background", "")
                        }
                    }

                    // «Показывать часы» — как у модулей KDE: у SDDM ключ
                    // темы, у Plasma Login Manager — свой.
                    RexonCheckBox {
                        id: clockBox
                        objectName: "loginShowClock"
                        pal: section.pal
                        visible: section.greeter !== null && section.greeter.clock.supported
                        enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                        text: section.greeter ? section.greeter.clock.label : ""
                        onToggled: {
                            if (!section.actions || section.lookBusy)
                                return;
                            section.lookBusy = true;
                            section.lookFailed = false;
                            section.lookMessage = qsTranslate("rexon", "Changing the login screen…");
                            section.actions.setLoginClock(clockBox.checked);
                        }
                    }
                }
            }

            Flow {
                Layout.fillWidth: true
                spacing: 8

                RexonButton {
                    objectName: "sddmLookApply"
                    pal: section.pal
                    enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                    text: section.greeter ? section.greeter.look.apply : ""
                    onClicked: section.askLook("look")
                }

                RexonButton {
                    objectName: "sddmLookReset"
                    pal: section.pal
                    enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                    text: section.greeter ? section.greeter.look.reset : ""
                    onClicked: section.askLook("lookReset")
                }

                // Живое превью: приветствие в тестовом режиме, без выхода
                // из сеанса. Пока открыто — та же кнопка его закрывает.
                RexonButton {
                    id: previewButton
                    objectName: "loginPreview"
                    pal: section.pal
                    enabled: section.greeter !== null && (section.greeter.preview.available || section.previewRunning)
                    text: section.greeter
                          ? "▶ " + (section.previewRunning ? section.greeter.preview.close : section.greeter.preview.label)
                          : ""
                    onClicked: section.togglePreview()
                    HoverHandler { id: previewHover }
                    ToolTip.text: section.greeter
                                  ? (section.greeter.preview.available ? section.greeter.preview.tooltip
                                                                       : section.greeter.preview.note)
                                  : ""
                    ToolTip.visible: previewHover.hovered && ToolTip.text !== ""
                    ToolTip.delay: 500
                }
            }

            // Почему превью нет (запертая кнопка подсказку не покажет).
            Text {
                Layout.fillWidth: true
                visible: text !== "" && !section.previewRunning
                text: section.greeter && !section.greeter.preview.available ? section.greeter.preview.note : ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            Text {
                objectName: "sddmGreeterMessage"
                Layout.fillWidth: true
                visible: text !== ""
                text: section.lookMessage
                color: section.lookFailed ? section.pal.state_warn : section.pal.text_primary
                wrapMode: Text.WordWrap
            }
        }
    }

    FileDialog {
        id: backgroundDialog
        title: section.greeter ? section.greeter.background.dialogTitle : ""
        nameFilters: section.greeter ? [section.greeter.background.filter] : []
        fileMode: FileDialog.OpenFile
        onAccepted: {
            const url = selectedFile.toString();
            const path = decodeURIComponent(url.replace(/^file:\/\//, ""));
            if (path !== "")
                section.startLook("background", path);
        }
    }

    // ── Подтверждение ─────────────────────────────────────────────────────
    //
    // Внутри окна (канон Big Picture); фокус при открытии — на «Отмена»:
    // случайный Enter экран входа не сменит.

    Popup {
        id: confirmDialog

        property string unit: ""
        property string greeter: ""
        /// Не пусто — вопрос о сеансе по умолчанию (X11-сеанс из пробы).
        property string session: ""
        /// Своя подпись «Да» (пусто — «Сделать основным»).
        property string yesText: ""
        property string heading: ""
        property string question: ""
        /// Не пусто — вопрос блока оформления экрана входа (`look`, `lookReset`).
        property string lookAction: ""

        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape
        width: Math.min((parent ? parent.width : 600) - 40, 600)
        padding: 16

        background: Rectangle {
            color: section.pal.bg_secondary
            border.color: section.pal.border
            border.width: 1
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: section.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: confirmDialog.heading
                color: section.pal.text_primary
                font.bold: true
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            TextEdit {
                Layout.fillWidth: true
                text: confirmDialog.question
                readOnly: true
                selectByMouse: true
                color: section.pal.text_primary
                selectionColor: section.pal.accent
                wrapMode: TextEdit.Wrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    objectName: "loginConfirmYes"
                    pal: section.pal
                    highlighted: true
                    text: confirmDialog.yesText !== "" ? confirmDialog.yesText : qsTranslate("rexon", "Make default")
                    onClicked: {
                        const unit = confirmDialog.unit;
                        const greeter = confirmDialog.greeter;
                        const session = confirmDialog.session;
                        const lookAction = confirmDialog.lookAction;
                        confirmDialog.lookAction = "";
                        confirmDialog.close();
                        if (lookAction !== "")
                            section.startLook(lookAction, "");
                        else if (session !== "")
                            section.startSession(session);
                        else
                            section.start(unit, greeter);
                    }
                }

                RexonButton {
                    id: confirmNo
                    objectName: "loginConfirmNo"
                    pal: section.pal
                    text: qsTranslate("rexon", "Cancel", "dialog button")
                    onClicked: confirmDialog.close()
                }
            }
        }

        onOpened: confirmNo.forceActiveFocus()
    }
}
