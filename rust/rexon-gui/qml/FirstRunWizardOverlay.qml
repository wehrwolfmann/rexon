// Мастер первого запуска — ОДНО окно поверх главного.
//
// ЗАЧЕМ. Человек, распаковавший архив и запустивший Rexon из папки, не
// получал ни ярлыка в меню приложений, ни дополнения для живых обоев в
// GNOME и Cinnamon — и ничего об этом не узнавал. Здесь ему один раз
// предлагают установку и честно называют цену: пароль администратора либо
// установка только для себя.
//
// ЧЬИ ЗДЕСЬ БУКВЫ — НИ ОДНОЙ СВОЕЙ. Решение «предлагать ли», режимы, все
// подписи приветствия и согласия на разрешение «не спрашивать пароль»
// приходят готовой картой от моста `FirstRunWizard`
// (rust/rexon-gui/src/first_run_wizard.rs), а он берёт их у ядра
// (`rexon_core::installer`). Набрать подпись здесь значило бы завести
// вторую копию, которая молча разойдётся с продуктом (правило #12).
//
// ⛔ НИКАКОЙ ЦЕПОЧКИ ОКОН (архитектурное правило #16). Выбор «для кого
// установить» и согласие на разрешение стоят в ОДНОМ окне: в Big Picture и
// на Steam Deck череда диалогов неудобна. Ход и итог показывает общее окно
// операции `DeOperationDialog.qml` — то же, что у «Рабочих столов»: у моста
// мастера свойство `operation` и сигнал `operationLog` того же вида, и окно
// подключается строкой `actions: wizard`, без единой правки в нём.
//
// Управление без мыши: фокус встаёт на выбранный режим (случайный Enter
// только переподтверждает уже выбранное и ничего не запускает), стрелки
// вверх-вниз ходят по режимам, Tab — дальше по окну, Esc — «Пропустить»
// без запоминания. Steam Input переводит крестовину в стрелки, A — в Enter,
// B — в Esc. Высота кнопок и галочек — не меньше 40 (правило #16).
//
// ⚠️ ИМЯ ФАЙЛА. Накладка зовётся `FirstRunWizardOverlay`, а не
// `FirstRunWizard`: имя `FirstRunWizard` в модуле `org.rexon.gui` уже занял
// сам мост (`#[qml_element]`), и файл с таким же именем сделал бы тип
// двойственным — «Ambiguous type detected. FirstRunWizard 1.0 is defined
// multiple times», после чего мост из разметки не достать вовсе. Пара
// «мост + накладка» названа так же, как у обучения: `OnboardingModel` и
// `OnboardingOverlay.qml`.
//
// Подключение: `FirstRunWizardOverlay { anchors.fill: parent; pal: …;
// onFinished: <отпустить обучение> }`, затем `begin()`.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    /// Палитра скина окна (та же карта, что у остальных страниц).
    property var pal: ({})

    readonly property color colAccent: root.pal.accent

    /// Мастер закончил работу: человек установил Rexon, отказался или
    /// предлагать было нечего. Хозяин окна держит обучение до этого сигнала.
    signal finished()

    /// Открыт ли мастер — приветствие либо окно хода и итога.
    readonly property bool opened: welcomeDialog.visible || root.opPhase !== ""

    // ── Мост ──────────────────────────────────────────────────────────────

    FirstRunWizard {
        id: wizard
        objectName: "firstRunWizard"
    }

    readonly property string opPhase: (wizard.operation
                                       && wizard.operation.phase !== undefined)
                                      ? wizard.operation.phase : ""

    /// Подписи приветствия от ядра: `{windowTitle, title, description,
    /// benefits, modesTitle, modes, installButton, skipButton,
    /// dontAskAgain, dontAskAgainHint}`.
    property var texts: ({})
    /// Подписи согласия «не спрашивать пароль для действий Rexon».
    property var consent: ({})
    /// Выбранный режим: "system" либо "user".
    property string mode: ""
    /// Сухой прогон: мастер проходится целиком, но машину не трогает.
    property string dryNotice: ""
    /// Мастер ведёт человека прямо сейчас (от приветствия до закрытия итога).
    property bool active: false

    // ── Ход мастера ───────────────────────────────────────────────────────

    /// Спросить ядро и, если оно предлагает установку, показать приветствие.
    /// Предлагать нечего — `finished()` сразу, и обучение не ждёт впустую.
    function begin() {
        if (root.active)
            return;
        const decision = wizard.start();
        // В журнал: по нему видно, почему мастер показался или нет.
        console.info("мастер установки: " + decision.reason);
        if (decision.kind !== "offer") {
            root.finished();
            return;
        }
        root.texts = decision.welcome;
        root.consent = decision.passwordless;
        root.dryNotice = decision.dryRunNotice !== undefined
                         ? decision.dryRunNotice : "";
        root.mode = decision.defaultMode;
        // Галочки возвращаются к безопасному состоянию при каждом показе:
        // разрешение «не спрашивать пароль» выключено (`defaultOn` от ядра),
        // «больше не спрашивать» снято.
        pwCheck.checked = root.consent.defaultOn === true;
        wizard.setPasswordlessRule(pwCheck.checked);
        dontAsk.checked = false;
        root.active = true;
        // Окно всегда открывается СВЕРХУ: показанный второй раз (кнопка в
        // «Настройках») мастер помнил прежнюю прокрутку и встречал человека
        // серединой согласия, а не приветствием.
        flick.contentY = 0;
        welcomeDialog.open();
        Qt.callLater(root.settle);
    }

    /// Фокус на выбранный режим: первое, о чём человека спрашивают.
    function settle() {
        const item = modes.itemAt(root.modeIndex(root.mode));
        if (item && item.enabled)
            item.forceActiveFocus();
        else
            installButton.forceActiveFocus();
    }

    /// Номер режима в списке ядра; -1 — такого нет.
    function modeIndex(id) {
        const list = root.modeList();
        for (let i = 0; i < list.length; ++i)
            if (String(list[i].mode) === id)
                return i;
        return -1;
    }

    function modeList() {
        return (root.texts && root.texts.modes !== undefined)
               ? root.texts.modes : [];
    }

    /// Выбран режим `id`. Недоступный режим не выбирается: объяснение под
    /// ним уже стоит, и загонять человека в тупик нечем.
    function selectMode(id) {
        const index = root.modeIndex(id);
        const list = root.modeList();
        if (index < 0 || String(list[index].blocked || "") !== "")
            return;
        root.mode = id;
    }

    /// «Установить».
    function install() {
        if (!wizard.install(root.mode)) {
            console.warn("мастер установки: режим «" + root.mode
                         + "» не принят мостом");
            return;
        }
        welcomeDialog.close();
    }

    /// «Пропустить». Галочка «больше не спрашивать» уезжает в настройки.
    function skip() {
        if (!wizard.decline(dontAsk.checked))
            console.warn("мастер установки: отказ не сохранён, "
                         + "мастер предложится снова");
        welcomeDialog.close();
        root.done();
    }

    function done() {
        root.active = false;
        root.finished();
    }

    // Окно хода закрыли — мастер отработал целиком.
    onOpPhaseChanged: {
        if (root.active && root.opPhase === "" && !welcomeDialog.visible)
            root.done();
    }

    Connections {
        target: wizard

        // Кнопка «Установить Rexon в систему…» на странице «Настройки».
        function onShowRequested() { root.begin(); }

        function onInstallFinished(ok) {
            console.info("мастер установки: итог "
                         + (ok ? "успех" : "неудача"));
        }
    }

    // ── Ход и итог: общее окно операции ───────────────────────────────────

    DeOperationDialog {
        anchors.fill: parent
        actions: wizard
        pal: root.pal
    }

    // ── Приветствие ───────────────────────────────────────────────────────

    Popup {
        id: welcomeDialog
        objectName: "firstRunWizardDialog"

        parent: root
        modal: true
        visible: false
        // Закрывается только кнопками: случайный щелчок мимо окна не должен
        // ни отказаться от установки, ни провалиться в окно под ним.
        closePolicy: Popup.NoAutoClose
        focus: true
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 720)
        height: Math.min(root.height - 40, column.implicitHeight + 40)
        padding: 20

        background: Rectangle {
            color: root.pal.bg_card
            border.color: root.pal.border
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: FocusScope {
            id: scope
            focus: true

            Keys.onEscapePressed: (event) => { event.accepted = true; root.skip(); }

            Flickable {
                id: flick
                anchors.fill: parent
                contentWidth: width
                contentHeight: column.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                ScrollBar.vertical: ScrollBar {
                    policy: flick.contentHeight > flick.height ? ScrollBar.AlwaysOn
                                                               : ScrollBar.AlwaysOff
                }

                ColumnLayout {
                    id: column
                    width: flick.width - (flick.contentHeight > flick.height ? 14 : 0)
                    spacing: 10

                    // ── Значок и заголовок ────────────────────────────────
                    //
                    // Значок нарисован фигурами, а не картинкой: страница
                    // обязана оставаться самодостаточной, а файлов значков
                    // до установки может не быть вовсе.

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 14

                        Rectangle {
                            Layout.alignment: Qt.AlignTop
                            implicitWidth: 56
                            implicitHeight: 56
                            radius: 10
                            color: root.pal.bg_primary
                            border.color: root.pal.accent
                            border.width: 2

                            // Стилизованное окно рабочего стола: шапка и поле.
                            Rectangle {
                                x: 12
                                y: 14
                                width: parent.width - 24
                                height: 7
                                radius: 2
                                color: root.pal.accent
                            }

                            Rectangle {
                                x: 12
                                y: 25
                                width: parent.width - 24
                                height: parent.height - 39
                                radius: 2
                                color: "transparent"
                                border.color: root.pal.accent
                                border.width: 1
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4

                            Text {
                                objectName: "firstRunWizardTitle"
                                Layout.fillWidth: true
                                text: root.texts.title !== undefined ? root.texts.title : ""
                                color: root.pal.text_primary
                                font.pointSize: 15
                                font.weight: Font.DemiBold
                                wrapMode: Text.WordWrap
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.texts.description !== undefined
                                      ? root.texts.description : ""
                                color: root.pal.text_primary
                                font.pointSize: 11
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    // ── Что даёт установка ────────────────────────────────

                    Repeater {
                        model: root.texts.benefits !== undefined ? root.texts.benefits : []

                        delegate: RowLayout {
                            id: benefitRow
                            required property string modelData

                            Layout.fillWidth: true
                            spacing: 8

                            Rectangle {
                                Layout.alignment: Qt.AlignTop
                                Layout.topMargin: 7
                                implicitWidth: 6
                                implicitHeight: 6
                                radius: 3
                                color: root.pal.accent
                            }

                            Text {
                                Layout.fillWidth: true
                                text: benefitRow.modelData
                                color: root.pal.text_secondary
                                font.pointSize: 11
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    // Сухой прогон: «готово» без единой записи на диск иначе
                    // читалось бы как настоящая установка.
                    Text {
                        Layout.fillWidth: true
                        Layout.topMargin: 4
                        visible: root.dryNotice !== ""
                        text: root.dryNotice
                        color: root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                 : root.pal.accent
                        font.pointSize: 11
                        wrapMode: Text.WordWrap
                    }

                    // ── Для кого установить ───────────────────────────────

                    Text {
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        text: root.texts.modesTitle !== undefined ? root.texts.modesTitle : ""
                        color: root.pal.text_primary
                        font.pointSize: 12
                        font.weight: Font.DemiBold
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        id: modes
                        model: root.modeList()

                        delegate: Button {
                            id: modeCard
                            objectName: "firstRunWizardMode_" + modeCard.modeId

                            required property int index
                            required property var modelData

                            readonly property string modeId: String(modeCard.modelData.mode)
                            readonly property string blocked: String(modeCard.modelData.blocked || "")
                            readonly property bool chosen: root.mode === modeCard.modeId

                            Layout.fillWidth: true
                            // Выбор режима — не кнопка-переключатель: состояние
                            // держит `root.mode`, иначе щелчок порвал бы
                            // привязку и два режима остались бы выбранными.
                            enabled: modeCard.blocked === ""
                            implicitHeight: Math.max(40, modeBody.implicitHeight + 24)
                            padding: 12
                            onClicked: root.selectMode(modeCard.modeId)

                            Keys.onUpPressed: (event) => {
                                event.accepted = true;
                                const prev = modes.itemAt(modeCard.index - 1);
                                if (prev && prev.enabled)
                                    prev.forceActiveFocus();
                            }
                            Keys.onDownPressed: (event) => {
                                event.accepted = true;
                                const next = modes.itemAt(modeCard.index + 1);
                                if (next && next.enabled)
                                    next.forceActiveFocus();
                                else if (pwCheck.visible)
                                    pwCheck.forceActiveFocus();
                                else
                                    installButton.forceActiveFocus();
                            }

                            background: Rectangle {
                                radius: 6
                                color: modeCard.chosen
                                       ? Qt.rgba(root.colAccent.r, root.colAccent.g,
                                                 root.colAccent.b, 0.18)
                                       : (modeCard.hovered ? root.pal.bg_card_hover
                                                           : root.pal.bg_primary)
                                border.width: (modeCard.activeFocus || modeCard.chosen) ? 2 : 1
                                border.color: modeCard.activeFocus
                                              ? root.colAccent
                                              : (modeCard.chosen ? root.colAccent
                                                                 : root.pal.border)
                            }

                            contentItem: ColumnLayout {
                                id: modeBody
                                spacing: 4

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 10

                                    // Точка выбора: видно, что вариант один из двух.
                                    Rectangle {
                                        Layout.alignment: Qt.AlignTop
                                        Layout.topMargin: 3
                                        implicitWidth: 18
                                        implicitHeight: 18
                                        radius: 9
                                        color: "transparent"
                                        border.width: 2
                                        border.color: modeCard.enabled ? root.colAccent
                                                                       : root.pal.border

                                        Rectangle {
                                            anchors.centerIn: parent
                                            visible: modeCard.chosen
                                            width: 8
                                            height: 8
                                            radius: 4
                                            color: root.colAccent
                                        }
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: String(modeCard.modelData.title)
                                        color: modeCard.enabled ? root.pal.text_primary
                                                                : root.pal.text_disabled
                                        font.pointSize: 12
                                        font.weight: Font.DemiBold
                                        wrapMode: Text.WordWrap
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 28
                                    text: String(modeCard.modelData.description)
                                    color: modeCard.enabled ? root.pal.text_secondary
                                                            : root.pal.text_disabled
                                    font.pointSize: 11
                                    wrapMode: Text.WordWrap
                                }

                                // Почему режим недоступен — прямо под ним, а не
                                // молчанием выключенной кнопки.
                                Text {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 28
                                    visible: modeCard.blocked !== ""
                                    text: modeCard.blocked
                                    color: root.pal.state_warn !== undefined
                                           ? root.pal.state_warn : root.pal.accent
                                    font.pointSize: 11
                                    wrapMode: Text.WordWrap
                                }
                            }

                            // Путь к файлам — уточнение, а не подпись: место
                            // ему во всплывающей подсказке (так решило ядро).
                            ToolTip.visible: modeCard.hovered && modeCard.hint !== ""
                            ToolTip.text: modeCard.hint
                            ToolTip.delay: 600
                            readonly property string hint: String(modeCard.modelData.hint || "")
                        }
                    }

                    // ── Не спрашивать пароль для действий Rexon ───────────
                    //
                    // Отдельное согласие, по умолчанию ВЫКЛЮЧЕННОЕ: разрешение
                    // снимает защиту паролем, и включить его может только сам
                    // человек, прочитав, что именно разрешается. Шаг есть
                    // только у установки для всех пользователей — при
                    // установке «только для вас» разрешение не ставится.

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        visible: root.mode === "system" && root.consent.title !== undefined
                        implicitHeight: consentBody.implicitHeight + 24
                        radius: 6
                        color: root.pal.bg_primary
                        border.color: root.pal.border
                        border.width: 1

                        ColumnLayout {
                            id: consentBody
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: 12
                            spacing: 4

                            Text {
                                Layout.fillWidth: true
                                text: root.consent.title !== undefined ? root.consent.title : ""
                                color: root.pal.text_primary
                                font.pointSize: 12
                                font.weight: Font.DemiBold
                                wrapMode: Text.WordWrap
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.consent.description !== undefined
                                      ? root.consent.description : ""
                                color: root.pal.text_secondary
                                font.pointSize: 11
                                wrapMode: Text.WordWrap
                            }

                            Repeater {
                                model: [root.consent.what, root.consent.who,
                                        root.consent.risk, root.consent.undo]

                                delegate: Text {
                                    id: consentLine
                                    required property var modelData

                                    Layout.fillWidth: true
                                    visible: consentLine.modelData !== undefined
                                             && String(consentLine.modelData) !== ""
                                    text: consentLine.modelData !== undefined
                                          ? String(consentLine.modelData) : ""
                                    color: root.pal.text_secondary
                                    font.pointSize: 11
                                    wrapMode: Text.WordWrap
                                }
                            }

                            RexonCheckBox {
                                id: pwCheck
                                objectName: "firstRunWizardPasswordless"
                                Layout.fillWidth: true
                                Layout.topMargin: 4
                                pal: root.pal
                                text: root.consent.checkbox !== undefined
                                      ? root.consent.checkbox : ""
                                onToggled: wizard.setPasswordlessRule(pwCheck.checked)
                                ToolTip.visible: pwCheck.hovered
                                                 && root.consent.hint !== undefined
                                                 && root.consent.hint !== ""
                                ToolTip.text: root.consent.hint !== undefined
                                              ? root.consent.hint : ""
                                ToolTip.delay: 600
                                Keys.onDownPressed: (event) => {
                                    event.accepted = true;
                                    dontAsk.forceActiveFocus();
                                }
                            }
                        }
                    }

                    // ── Ответ ─────────────────────────────────────────────

                    RexonCheckBox {
                        id: dontAsk
                        objectName: "firstRunWizardDontAsk"
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        pal: root.pal
                        text: root.texts.dontAskAgain !== undefined
                              ? root.texts.dontAskAgain : ""
                        ToolTip.visible: dontAsk.hovered
                                         && root.texts.dontAskAgainHint !== undefined
                        ToolTip.text: root.texts.dontAskAgainHint !== undefined
                                      ? root.texts.dontAskAgainHint : ""
                        ToolTip.delay: 600
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.texts.dontAskAgainHint !== undefined
                              ? root.texts.dontAskAgainHint : ""
                        color: root.pal.text_secondary
                        font.pointSize: 10
                        wrapMode: Text.WordWrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.topMargin: 10
                        spacing: 8

                        Item { Layout.fillWidth: true }

                        RexonButton {
                            id: skipButton
                            objectName: "firstRunWizardSkip"
                            pal: root.pal
                            text: root.texts.skipButton !== undefined
                                  ? root.texts.skipButton : ""
                            onClicked: root.skip()
                            KeyNavigation.right: installButton
                            Keys.onReturnPressed: clicked()
                        }

                        RexonButton {
                            id: installButton
                            objectName: "firstRunWizardInstall"
                            pal: root.pal
                            tone: root.pal.primary_tone !== undefined
                                  ? root.pal.primary_tone : null
                            text: root.texts.installButton !== undefined
                                  ? root.texts.installButton : ""
                            onClicked: root.install()
                            KeyNavigation.left: skipButton
                            Keys.onReturnPressed: clicked()
                        }
                    }
                }
            }
        }
    }
}
