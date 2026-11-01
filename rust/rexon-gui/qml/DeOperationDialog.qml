// Окно операции над рабочим столом — ОДНО на все вкладки, которые ставят,
// проверяют, меняют и удаляют стол («Рабочие столы» и «Сравнение DE»).
//
// Один мастер-диалог на операцию (архитектурное правило #16: цепочки окон
// неудобны в Big Picture): вопрос или отказ → (шаг пароля SteamOS) → ход
// дела → итог. Состояние и
// все подписи — карта `actions.operation` от Rust-объекта `DesktopsActions`
// (исполнитель — `rexon_core::system_de_exec`), своих букв здесь нет.
//
// У Python-хозяина вкладок такого свойства нет — окно тогда просто не
// показывается, а вопросы рисует он сам (`qt_main.de_progress`).
//
// Подключение: `DeOperationDialog { anchors.fill: parent; actions: …;
// pal: …; onDesktopsChanged: <пересчитать состав столов> }`.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    /// Объект побочных действий страницы.
    property var actions: null

    /// Палитра скина страницы (тоны кнопок берутся, если они в ней есть).
    property var pal: ({})

    /// Операция кончилась удачей — состав установленных столов мог
    /// измениться, страница пересчитает себя.
    signal desktopsChanged()

    readonly property var op: (root.actions && root.actions.operation !== undefined)
                              ? root.actions.operation : null
    readonly property string opPhase: (root.op && root.op.phase !== undefined)
                                      ? root.op.phase : ""

    readonly property var primaryTone: root.pal.primary_tone !== undefined
                                       ? root.pal.primary_tone : null
    readonly property var dangerTone: root.pal.destructive_tone !== undefined
                                      ? root.pal.destructive_tone : null

    Connections {
        target: root.actions
        ignoreUnknownSignals: true
        function onOperationLog(line) { opLog.append(line); }
        function onDesktopsChanged() { root.desktopsChanged(); }
    }

    readonly property bool passwordBusy: (root.op && root.op.passwordBusy === true)

    onOpPhaseChanged: {
        if (root.opPhase === "running")
            opLog.clear();
        // Пароль в полях не живёт дольше шага.
        if (root.opPhase !== "password") {
            pwNew.clear();
            pwRepeat.clear();
        }
        if (root.opPhase !== "")
            Qt.callLater(root.focusOperation);
    }

    function focusOperation() {
        // Безопасный ответ под фокусом: случайный Enter ничего не запускает.
        if (root.opPhase === "confirm")
            opNo.forceActiveFocus();
        else if (root.opPhase === "password")
            pwNew.forceActiveFocus();
        else if (root.opPhase === "blocked")
            opOk.forceActiveFocus();
        else if (opClose.enabled)
            opClose.forceActiveFocus();
    }

    function dismissOperation() {
        if (root.actions && root.opPhase !== "running" && !root.passwordBusy)
            root.actions.dismissOperation();
    }

    function submitPassword() {
        if (root.actions && root.opPhase === "password" && !root.passwordBusy)
            root.actions.submitPassword(pwNew.text, pwRepeat.text);
    }

    Popup {
        id: opDialog
        objectName: "desktopsOperationDialog"

        parent: root
        modal: true
        // Страница, спрятанная хозяином (вторая страница раздела), своё
        // окно не показывает: иначе у двух страниц одного объекта действий
        // встали бы два одинаковых окна друг на друге.
        visible: root.opPhase !== "" && root.visible
        closePolicy: Popup.NoAutoClose
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 760)
        height: (root.opPhase === "running" || root.opPhase === "done")
                ? Math.min(root.height - 40, 580) : implicitHeight
        padding: 20

        background: Rectangle {
            color: root.pal.bg_card
            border.color: root.pal.border
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            Keys.onEscapePressed: root.dismissOperation()

            Text {
                Layout.fillWidth: true
                text: root.op ? root.op.title : ""
                color: root.pal.text_primary
                font.pointSize: 14
                font.weight: Font.DemiBold
                wrapMode: Text.WordWrap
            }

            // Вопрос, отказ или объяснение шага пароля.
            Text {
                Layout.fillWidth: true
                visible: root.opPhase === "confirm" || root.opPhase === "blocked"
                         || root.opPhase === "password"
                text: root.op ? root.op.text : ""
                textFormat: Text.RichText
                color: root.pal.text_primary
                font.pointSize: 11
                wrapMode: Text.WordWrap
            }

            // Переключатели вариантов вопроса (стиль образа «Как в SteamOS»:
            // Vapor или VGUI2): подпись и варианты — из карты, выбранный
            // подсвечен. Щелчок пересобирает вопрос — текст выше меняется.
            Repeater {
                model: (root.opPhase === "confirm" && root.op && root.op.choices !== undefined)
                       ? root.op.choices : []

                delegate: ColumnLayout {
                    id: choiceRow
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4

                    Text {
                        text: choiceRow.modelData.label
                        color: root.pal.text_primary
                        font.pointSize: 11
                        font.weight: Font.DemiBold
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: 8

                        Repeater {
                            model: choiceRow.modelData.options

                            delegate: RexonButton {
                                id: choiceButton
                                required property var modelData
                                objectName: "desktopsOperationChoice_" + choiceButton.modelData.value
                                pal: root.pal
                                checkable: true
                                checked: choiceRow.modelData.current === choiceButton.modelData.value
                                checkedTone: root.primaryTone
                                text: choiceButton.modelData.label
                                onClicked: {
                                    // Выбранное — от карты, а не от щелчка.
                                    choiceButton.checked = Qt.binding(function () {
                                        return choiceRow.modelData.current === choiceButton.modelData.value;
                                    });
                                    if (root.actions && root.actions.setOperationChoice)
                                        root.actions.setOperationChoice(choiceRow.modelData.name,
                                                                        choiceButton.modelData.value);
                                }
                            }
                        }
                    }
                }
            }

            // Галочка «по желанию» вопроса (игровой режим образа «Как в
            // SteamOS»): есть, только когда её подпись пришла в карте.
            // Переключение пересобирает вопрос — текст выше меняется.
            RexonCheckBox {
                id: opOption
                objectName: "desktopsOperationOption"
                Layout.fillWidth: true
                visible: root.opPhase === "confirm" && root.op
                         && root.op.optionText !== undefined && root.op.optionText !== ""
                pal: root.pal
                text: (root.op && root.op.optionText !== undefined) ? root.op.optionText : ""
                checked: root.op ? root.op.optionChecked === true : false
                onToggled: {
                    const want = opOption.checked;
                    // Состояние галочки — от карты, а не от щелчка: щелчок
                    // рвёт привязку, и следующий вопрос показал бы старое.
                    opOption.checked = Qt.binding(function () {
                        return root.op ? root.op.optionChecked === true : false;
                    });
                    if (root.actions && root.actions.setOperationOption)
                        root.actions.setOperationOption(want);
                }
            }
            Text {
                Layout.fillWidth: true
                visible: opOption.visible && root.op && root.op.optionHint !== undefined
                         && root.op.optionHint !== ""
                text: (root.op && root.op.optionHint !== undefined) ? root.op.optionHint : ""
                textFormat: Text.RichText
                color: root.pal.status_unavailable !== undefined ? root.pal.status_unavailable
                                                                 : root.pal.text_secondary
                font.pointSize: 10
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                visible: root.opPhase === "blocked" && root.op && root.op.informative !== ""
                text: root.op ? root.op.informative : ""
                textFormat: Text.RichText
                color: root.pal.text_secondary
                font.pointSize: 11
                wrapMode: Text.WordWrap
            }

            // Шаг пароля SteamOS: у «deck» пароля нет, без него системное
            // окно пароля не пройти. Два поля, слова ядра под ними.
            RexonTextField {
                id: pwNew
                objectName: "desktopsPasswordNew"
                Layout.fillWidth: true
                visible: root.opPhase === "password"
                enabled: !root.passwordBusy
                pal: root.pal
                echoMode: TextInput.Password
                placeholderText: (root.op && root.op.passwordNewText !== undefined)
                                 ? root.op.passwordNewText : ""
                onAccepted: pwRepeat.forceActiveFocus()
            }
            RexonTextField {
                id: pwRepeat
                objectName: "desktopsPasswordRepeat"
                Layout.fillWidth: true
                visible: root.opPhase === "password"
                enabled: !root.passwordBusy
                pal: root.pal
                echoMode: TextInput.Password
                placeholderText: (root.op && root.op.passwordRepeatText !== undefined)
                                 ? root.op.passwordRepeatText : ""
                onAccepted: root.submitPassword()
            }
            Text {
                objectName: "desktopsPasswordError"
                Layout.fillWidth: true
                visible: root.opPhase === "password" && root.op && root.op.passwordError !== ""
                text: root.op ? root.op.passwordError : ""
                color: root.pal.danger !== undefined ? root.pal.danger : "#dc5050"
                font.pointSize: 10
                wrapMode: Text.WordWrap
            }

            // Ход дела.
            Text {
                Layout.fillWidth: true
                visible: root.opPhase === "running" || root.opPhase === "done"
                text: root.op ? root.op.status : ""
                color: root.pal.text_primary
                font.pointSize: 11
                font.weight: Font.DemiBold
            }

            // Полоса своей разметкой: стиль Basic палитре не следует.
            Rectangle {
                Layout.fillWidth: true
                visible: root.opPhase === "running" || root.opPhase === "done"
                implicitHeight: 10
                radius: 5
                color: root.pal.border

                Rectangle {
                    width: (root.op && root.op.barMax > 0)
                           ? parent.width * Math.max(0, Math.min(1, root.op.bar / root.op.barMax)) : 0
                    height: parent.height
                    radius: parent.radius
                    color: root.pal.accent
                }
            }

            Text {
                Layout.fillWidth: true
                visible: (root.opPhase === "running" || root.opPhase === "done")
                         && root.op && root.op.current !== ""
                text: root.op ? root.op.current : ""
                color: root.pal.text_secondary
                font.pointSize: 9
                elide: Text.ElideRight
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: root.opPhase === "running" || root.opPhase === "done"
                clip: true

                TextArea {
                    id: opLog
                    objectName: "desktopsOperationLog"
                    readOnly: true
                    // По словам; в любом месте — только длинный путь.
                    wrapMode: TextEdit.WrapAtWordBoundaryOrAnywhere
                    color: root.pal.text_primary
                    font.family: "monospace"
                    font.pointSize: 9
                    background: Rectangle {
                        color: root.pal.bg_primary
                        border.color: root.pal.border
                    }
                    // Новая строка — вид на хвост журнала.
                    onTextChanged: cursorPosition = length
                }
            }

            Text {
                Layout.fillWidth: true
                visible: root.opPhase === "done" && root.op && root.op.resultText !== ""
                text: root.op ? root.op.resultText : ""
                textFormat: Text.RichText
                color: root.pal.text_primary
                font.pointSize: 11
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 6
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: opYes
                    objectName: "desktopsOperationYes"
                    visible: root.opPhase === "confirm" || root.opPhase === "password"
                    enabled: !root.passwordBusy
                    pal: root.pal
                    // Необратимое (удаление) — красным, остальное — синим.
                    tone: (root.op && root.op.danger) ? root.dangerTone : root.primaryTone
                    text: root.op ? root.op.yesText : ""
                    onClicked: {
                        if (root.opPhase === "password")
                            root.submitPassword();
                        else
                            root.actions.confirmOperation();
                    }
                    Keys.onReturnPressed: clicked()
                }
                RexonButton {
                    id: opNo
                    visible: root.opPhase === "confirm" || root.opPhase === "password"
                    enabled: !root.passwordBusy
                    pal: root.pal
                    text: root.op ? root.op.noText : ""
                    onClicked: root.dismissOperation()
                    Keys.onReturnPressed: clicked()
                }
                RexonButton {
                    id: opOk
                    visible: root.opPhase === "blocked"
                    pal: root.pal
                    text: root.op ? root.op.okText : ""
                    onClicked: root.dismissOperation()
                    Keys.onReturnPressed: clicked()
                }
                RexonButton {
                    id: opCancel
                    visible: root.opPhase === "running" && root.op && root.op.cancellable
                    pal: root.pal
                    tone: root.dangerTone
                    text: root.op ? root.op.cancelText : ""
                    onClicked: root.actions.cancelOperation()
                }
                RexonButton {
                    id: opClose
                    visible: root.opPhase === "running" || root.opPhase === "done"
                    enabled: root.op ? root.op.closeEnabled : false
                    pal: root.pal
                    text: root.op ? root.op.closeText : ""
                    onClicked: root.dismissOperation()
                    onEnabledChanged: if (enabled) forceActiveFocus()
                    Keys.onReturnPressed: clicked()
                }
            }
        }
    }
}
