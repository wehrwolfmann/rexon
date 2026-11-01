// Обучение при первом запуске — накладка-«прожектор» поверх окна на Rust.
//
// Порт виджетной накладки `rexon/onboarding.py`: затемнить окно, вырезать
// окно вокруг нужного места, показать карточку с «Далее» и «Пропустить».
// Сценарий, порядок, тексты, решение «что показывать», сохранение прогресса,
// отступ выреза, место карточки и цвета скина — у ядра
// (`rexon_core::onboarding` через `OnboardingModel`). Здесь только рисование
// и поиск цели в окне (функция `locate` от хозяина).
//
// Первый шаг — вопрос «Показать краткое обучение?», как у Python-окна
// (`Tour._ask`), только не отдельным окном, а той же карточкой: в Big
// Picture череда системных окон неудобна (архитектурное правило #16).
//
// Управление без мыши: фокус сразу на главной кнопке, Enter/Return — нажать
// кнопку в фокусе, Esc — отказаться («Не нужно» / «Пропустить»; на последнем
// шаге — «Готово»), стрелки влево-вправо и Tab — между кнопками. Steam Input
// переводит крестовину в стрелки, A — в Enter, B — в Esc. Кнопки не ниже
// 40×40 (правило #16).
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes

Popup {
    id: tour

    /// Объект обучения на Rust.
    required property var core

    /// Найти цель в координатах окна: `(kind, tabId)` → `{x, y, width,
    /// height}` либо null. `kind` — "tab" (заголовок вкладки) или
    /// "tab_content" (содержимое; хозяин заодно переключается на вкладку).
    property var locate: null

    /// "ask" — вопрос первого запуска, "step" — шаг сценария, "" — закрыто.
    property string mode: ""
    /// Идентификатор показанного шага.
    property string stepId: ""
    /// Содержимое карточки шага от ядра.
    property var content: ({})
    /// Вырез вокруг цели от ядра.
    property var hole: ({ valid: false, x: 0, y: 0, width: 0, height: 0 })

    readonly property var st: tour.core.style
    readonly property var texts: tour.core.texts()
    readonly property var sizes: tour.core.pointSizes(
        tour.font.pointSize > 0 ? tour.font.pointSize : 10)

    parent: Overlay.overlay
    x: 0
    y: 0
    width: parent ? parent.width : 0
    height: parent ? parent.height : 0
    padding: 0
    modal: true
    // Своё затемнение с вырезом ниже; штатное закрасило бы окно целиком.
    dim: false
    // Закрывается только кнопками: случайный клик мимо карточки не должен
    // ни закрыть обучение, ни провалиться в окно под ним.
    closePolicy: Popup.NoAutoClose
    focus: true
    enter: null
    exit: null

    // ── Ход обучения ──────────────────────────────────────────────────────

    /// Исполнить решение ядра `{kind, step}`.
    function begin(decision) {
        if (!decision || decision.kind === "nothing") {
            tour.finish();
        } else if (decision.kind === "ask") {
            tour.mode = "ask";
            tour.stepId = "";
            tour.hole = tour.core.hole(0, 0, 0, 0);
            tour.open();
            Qt.callLater(tour.settle);
        } else if (decision.kind === "show" && decision.step !== "") {
            tour.show(decision.step);
        }
    }

    function show(id) {
        const c = tour.core.card(id);
        if (c.title === undefined) {
            // Шага нет в сценарии — пустая карточка хуже, чем ничего.
            tour.finish();
            return;
        }
        tour.content = c;
        tour.stepId = id;
        tour.mode = "step";
        tour.open();
        // Цель ищется после показа: вкладка могла только что переключиться.
        Qt.callLater(tour.settle);
    }

    /// Навести подсветку, поставить карточку и фокус на главную кнопку.
    function settle() {
        tour.aim();
        primaryButton.forceActiveFocus();
    }

    function aim() {
        if (tour.mode !== "step") {
            tour.hole = tour.core.hole(0, 0, 0, 0);
            tour.place();
            return;
        }
        const a = tour.core.aim(tour.stepId);
        let rect = null;
        if (a.kind !== "window" && a.tab !== "" && tour.locate)
            rect = tour.locate(a.kind, a.tab);
        // Цель не нашлась — карточка по центру, без пустой рамки.
        tour.hole = rect
            ? tour.core.hole(Math.round(rect.x), Math.round(rect.y),
                             Math.round(rect.width), Math.round(rect.height))
            : tour.core.hole(0, 0, 0, 0);
        tour.place();
    }

    function place() {
        const p = tour.core.placement(
            Math.round(tour.width), Math.round(tour.height),
            Math.round(card.width), Math.round(card.height),
            tour.hole.valid === true, tour.hole.x, tour.hole.y,
            tour.hole.width, tour.hole.height);
        card.x = p.x;
        card.y = p.y;
    }

    /// Главная кнопка: «Показать» / «Далее» / «Готово».
    function primary() {
        if (tour.mode === "ask") {
            tour.begin(tour.core.answerAsk(true));
        } else if (tour.mode === "step") {
            const next = tour.core.advance(tour.stepId);
            if (next === "")
                tour.finish();
            else
                tour.show(next);
        }
    }

    /// Отказ: «Не нужно» (запоминается) / «Пропустить» (весь сценарий).
    function secondary() {
        if (tour.mode === "ask")
            tour.core.answerAsk(false);
        else if (tour.mode === "step")
            tour.core.skipAll();
        tour.finish();
    }

    /// Esc. На последнем шаге отказываться не от чего — это «Готово».
    function dismiss() {
        if (tour.mode === "step" && tour.content.skipVisible !== true)
            tour.primary();
        else
            tour.secondary();
    }

    function finish() {
        tour.mode = "";
        tour.stepId = "";
        tour.close();
    }

    onWidthChanged: if (tour.opened) tour.aim()
    onHeightChanged: if (tour.opened) tour.aim()

    Connections {
        target: tour.core

        // «Пройти обучение заново» из Настроек.
        function onReplayRequested() { tour.begin(tour.core.replay()); }
    }

    // ── Затемнение с вырезом ──────────────────────────────────────────────

    background: Item {
        Shape {
            anchors.fill: parent

            ShapePath {
                fillColor: tour.st.dim
                strokeWidth: -1
                fillRule: ShapePath.OddEvenFill

                PathRectangle {
                    x: 0
                    y: 0
                    width: tour.width
                    height: tour.height
                }

                PathRectangle {
                    x: tour.hole.x
                    y: tour.hole.y
                    width: tour.hole.valid ? tour.hole.width : 0
                    height: tour.hole.valid ? tour.hole.height : 0
                    radius: tour.st.cornerRadius
                }
            }
        }

        // Неоновая рамка вокруг подсвеченного места.
        Rectangle {
            visible: tour.hole.valid === true
            x: tour.hole.x
            y: tour.hole.y
            width: tour.hole.width
            height: tour.hole.height
            radius: tour.st.cornerRadius
            color: "transparent"
            border.width: tour.st.haloWidth
            border.color: tour.st.halo
        }
    }

    // ── Карточка ──────────────────────────────────────────────────────────

    contentItem: FocusScope {
        id: scope
        focus: true

        Keys.onEscapePressed: (event) => { event.accepted = true; tour.dismiss(); }
        Keys.onReturnPressed: (event) => { event.accepted = true; scope.pressFocused(); }
        Keys.onEnterPressed: (event) => { event.accepted = true; scope.pressFocused(); }

        // ⚠️ Фокус держится на карточке, пока обучение открыто. Шаг «Каталог
        // тем» переключает вкладку, и страница, достроившись асинхронно,
        // забирает фокус себе (поле «Поиск тем…»). Поймано стендом на Xvfb
        // 12.09.2026: Esc и стрелки уходили в поле поиска, и с клавиатуры и
        // геймпада обучение становилось не закрыть.
        Connections {
            target: scope.Window.window
            enabled: tour.opened

            function onActiveFocusItemChanged() { Qt.callLater(scope.keepFocus); }
        }

        function keepFocus() {
            if (!tour.opened)
                return;
            for (let it = scope.Window.activeFocusItem; it; it = it.parent)
                if (it === scope)
                    return;
            primaryButton.forceActiveFocus();
        }

        function pressFocused() {
            if (secondaryButton.visible && secondaryButton.activeFocus)
                tour.secondary();
            else
                tour.primary();
        }

        Rectangle {
            id: card
            objectName: "onboardingCard"
            width: Math.min(tour.st.cardMaxWidth, tour.width - 2 * tour.st.cardGap)
            height: column.implicitHeight + tour.st.marginTop + tour.st.marginBottom
            color: tour.st.cardBackground
            border.width: tour.st.haloWidth
            border.color: tour.st.halo
            radius: tour.st.cardRadius

            onHeightChanged: tour.place()
            onWidthChanged: tour.place()

            ColumnLayout {
                id: column
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.leftMargin: tour.st.marginLeft
                anchors.rightMargin: tour.st.marginRight
                anchors.topMargin: tour.st.marginTop
                spacing: tour.st.spacing

                Label {
                    Layout.fillWidth: true
                    visible: tour.mode === "step"
                    text: tour.content.progress ?? ""
                    color: tour.st.cardMuted
                    font.pointSize: tour.sizes.progress
                }

                Label {
                    objectName: "onboardingTitle"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: tour.mode === "ask" ? tour.texts.askTitle
                                              : (tour.content.title ?? "")
                    color: tour.st.cardText
                    font.pointSize: tour.sizes.title
                    font.bold: true
                }

                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: tour.mode === "ask" ? tour.texts.askText
                                              : (tour.content.text ?? "")
                    color: tour.st.cardText
                    font.weight: tour.st.bodyWeight
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    spacing: tour.st.buttonSpacing

                    // «Не нужно» / «Пропустить» — плоская надпись.
                    Button {
                        id: secondaryButton
                        objectName: "onboardingSecondary"
                        visible: tour.mode === "ask" || tour.content.skipVisible === true
                        text: tour.mode === "ask" ? tour.texts.askNo
                                                  : (tour.content.skipText ?? "")
                        implicitHeight: Math.max(40, implicitContentHeight + topPadding + bottomPadding)
                        implicitWidth: Math.max(40, implicitContentWidth + leftPadding + rightPadding)
                        topPadding: tour.st.skipPadV
                        bottomPadding: tour.st.skipPadV
                        leftPadding: tour.st.skipPadH
                        rightPadding: tour.st.skipPadH
                        KeyNavigation.right: primaryButton
                        onClicked: tour.secondary()

                        contentItem: Text {
                            text: secondaryButton.text
                            font: secondaryButton.font
                            color: secondaryButton.hovered || secondaryButton.activeFocus
                                   ? tour.st.halo : tour.st.cardMuted
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            color: "transparent"
                            radius: tour.st.nextRadius
                            border.width: secondaryButton.activeFocus ? 2 : 0
                            border.color: tour.st.halo
                        }
                    }

                    Item { Layout.fillWidth: true }

                    // «Показать» / «Далее» / «Готово» — заливка акцентом.
                    Button {
                        id: primaryButton
                        objectName: "onboardingPrimary"
                        text: tour.mode === "ask" ? tour.texts.askYes
                                                  : (tour.content.nextText ?? "")
                        implicitHeight: Math.max(40, implicitContentHeight + topPadding + bottomPadding)
                        implicitWidth: Math.max(40, implicitContentWidth + leftPadding + rightPadding)
                        topPadding: tour.st.nextPadV
                        bottomPadding: tour.st.nextPadV
                        leftPadding: tour.st.nextPadH
                        rightPadding: tour.st.nextPadH
                        font.weight: tour.st.nextWeight
                        KeyNavigation.left: secondaryButton.visible ? secondaryButton : null
                        onClicked: tour.primary()

                        contentItem: Text {
                            text: primaryButton.text
                            font: primaryButton.font
                            color: tour.st.onAccent
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            color: primaryButton.hovered || primaryButton.down
                                   ? tour.st.haloHover : tour.st.halo
                            radius: tour.st.nextRadius
                            // Рамка фокуса цветом текста карточки: на заливке
                            // акцентом акцентная рамка была бы не видна.
                            border.width: primaryButton.activeFocus ? 2 : 0
                            border.color: tour.st.cardText
                        }
                    }
                }
            }
        }
    }
}
