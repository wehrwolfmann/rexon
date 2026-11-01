// Вкладка «Сравнение DE» на QML. Данные и решения — Rust-объект CompareModel
// (rust/rexon-gui/src/compare_page.rs), который сам ничего не решает, а
// спрашивает ядро (`rexon_core::qt_compare`). Здесь только РАЗМЕТКА.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из `ui_base::skin_colors("steam_dark")` — скина по
// умолчанию всего продукта (правило #17). Своих цветов страница не заводит:
// вторая палитра в .qml развела бы вкладку с остальным Rexon молча.
//
// Побочные действия (установка через pkexec, переключение сессии) живут
// снаружи: хозяин страницы кладёт в `root.actions` объект с методами
// install/switchTo, и они возвращают «получилось ли». Пока эти шаги на
// Python (rexon/qt_compare/window.py), после ухода с PySide6 на их место
// встанет Rust-объект с теми же двумя методами — страница не изменится.
//
// Цепочка фокуса заложена с самого начала (решение по контроллеру: сам
// контроллер отложен, фокус в QML — всегда): все кнопки достижимы Tab'ом,
// у сфокусированной видна рамка, страница сама подкручивается к ней.

// Делегаты обращаются к id внешних элементов (root, page). Без этой прагмы
// проверка QML справедливо ругается на неквалифицированный доступ, а Qt
// разрешает такие имена лишь через устаревший контекст делегата.
// (Строку «// qmllint …» здесь писать нельзя: она читается как директива.)
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    // Объект побочных действий; ставит хозяин страницы.
    // Методы: install(deId) -> bool, switchTo(deId) -> bool.
    property var actions: null

    // Хозяину окна: заголовок и минимальный размер живут в ядре, а не в
    // питоновской или QML-копии.
    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    // Имя типа — CompareModel, а не ComparePage: последнее занято самим этим
    // файлом (в QML-модуле каждый .qml — тип по имени файла).
    CompareModel {
        id: page
        // Имя нужно проверкам: из хозяина страницы модель достают
        // `root.findChild(QObject, "compareModel")` и спрашивают её
        // без единой правки в самой странице.
        // ⚠️ Снаружи зовутся только инвокаблы БЕЗ чисел в подписи
        // (`refreshInstalled`, `pairView`, `displayName`): у `i32` в
        // метаобъекте стоит имя `::std::int32_t`, метатипа с таким именем
        // нет, и `QMetaObject.invokeMethod("toggle", …)` молча вернёт False.
        // Проверять раскрытие карточки надо клавишами (Tab до шапки, Space)
        // — то есть тем же путём, каким её ведёт покупатель.
        objectName: "compareModel"
    }

    // ── Палитра стандартного скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    // Карточку у виджетов красит не палитра, а полупрозрачная накладка
    // таблицы стилей (`qt_compare::card_qss`); ядро кладёт её на фон
    // страницы и отдаёт готовой краской. Токен `bg_card` был похож, но не
    // тот: (30,45,61) против (36,48,64) на 1 712 261 точке.
    readonly property color colCard: root.pal.surface_card
    readonly property color colCardHover: root.pal.bg_card_hover
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // ── Побочные действия ─────────────────────────────────────────────────

    // Установка идёт синхронно: подтверждение, ход дела и итог показывает
    // хозяин, а сюда возвращается только «получилось ли». Пересчёт состава
    // столов делает страница — иначе карточка продолжала бы предлагать
    // «Установить» уже установленный стол (починка, перенесённая из
    // виджетной версии).
    function runInstall(deId) {
        if (!root.actions)
            return;
        if (root.actions.install(deId))
            page.refreshInstalled();
    }

    function runSwitch(deId) {
        if (!root.actions)
            return;
        root.actions.switchTo(deId);
    }

    // Подкрутить страницу к элементу, который только что получил фокус:
    // без этого Tab уводит фокус за нижний край, и с клавиатуры (а завтра с
    // геймпада) вкладка выглядит зависшей.
    function ensureVisible(item) {
        if (!item || !flick.contentItem)
            return;
        const pos = item.mapToItem(flick.contentItem, 0, 0);
        const top = pos.y;
        const bottom = pos.y + item.height;
        const maxY = Math.max(0, flick.contentHeight - flick.height);
        if (top < flick.contentY)
            flick.contentY = Math.max(0, top - 12);
        else if (bottom > flick.contentY + flick.height)
            flick.contentY = Math.min(maxY, bottom - flick.height + 12);
    }

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── Страница ──────────────────────────────────────────────────────────

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: pageColumn.implicitHeight + 48
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {}

        // ⚠️ Раскладка внутри ОБЫЧНОГО Item (не внутри другой раскладки)
        // обязана получить высоту руками: сама она остаётся нулевой, и
        // страница схлопывается в полоску.
        ColumnLayout {
            id: pageColumn
            x: 24
            y: 24
            width: flick.width - 48
            height: implicitHeight
            spacing: 14

            // ── Шапка: заголовок и быстрые пары ───────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: page.headerTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                }

                // Виджетная вкладка прижимает быстрые пары к ПРАВОМУ краю
                // окна, а не ставит их вплотную к заголовку: распорка
                // держит тот же вид.
                Item {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 0
                }

                Text {
                    Layout.leftMargin: 16
                    text: page.pairsLabel
                    color: root.colTextSecondary
                    // Вторичный текст у виджетной вкладки набран плотнее
                    // (`font-weight: 600` из `secondary_text_qss`); число
                    // приезжает из ядра палитрой, своего страница не заводит.
                    font.weight: root.pal.text_secondary_weight
                }

                // Кнопки пар — в Flow, а не в самой строке: на минимальной
                // ширине окна (900, число из ядра) четыре кнопки в один ряд
                // не помещаются, и последняя уезжала за правый край. Flow
                // переносит лишнюю на вторую строку вместо того, чтобы её
                // обрезать.
                Flow {
                    id: pairsFlow

                    // Ширина ряда кнопок, если бы он умещался в одну строку.
                    // Считается по кнопкам, а не спрашивается у самого Flow:
                    // у позиционера implicitWidth зависит от его же ширины,
                    // и связывание одного через другое зациклилось бы.
                    property real naturalWidth: 0

                    function measure() {
                        let total = 0;
                        let shown = 0;
                        for (let i = 0; i < pairsFlow.children.length; i++) {
                            const item = pairsFlow.children[i];
                            // ⚠️ Сам Repeater тоже числится ребёнком Flow, и
                            // он невидимой шириной ноль. Пока его считали
                            // наравне с кнопками, в потолок ширины попадал
                            // ЛИШНИЙ зазор, и весь блок пар отходил от
                            // правого края окна на 8 точек, хотя у
                            // оригинала он прижат вплотную (замер
                            // 02.09.2026: правый край 1367 против 1374).
                            if (!item.visible || item.implicitWidth <= 0)
                                continue;
                            // Каждая ширина округляется ВВЕРХ по
                            // отдельности. Ширина кнопки дробная, а Flow
                            // переносит по сравнению дробных сумм: потолок,
                            // посчитанный впритык, оказывался на сотую доли
                            // точки меньше нужного, и последняя пара
                            // уезжала на вторую строку на ровном месте.
                            total += Math.ceil(item.implicitWidth);
                            shown += 1;
                        }
                        pairsFlow.naturalWidth = shown > 1
                            ? total + pairsFlow.spacing * (shown - 1)
                            : total;
                    }

                    Layout.fillWidth: true
                    // Потолок ширины — свой же ряд кнопок: лишнее место
                    // забирает распорка слева, и блок стоит справа. Ниже
                    // потолка Flow сжимается и переносит кнопку.
                    Layout.maximumWidth: pairsFlow.naturalWidth
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 8

                    Repeater {
                        id: pairRepeater
                        model: page.quickPairs

                        onItemAdded: pairsFlow.measure()
                        onItemRemoved: pairsFlow.measure()

                        delegate: RexonButton {
                            id: pairButton
                            required property var modelData
                            pal: root.pal
                            text: pairButton.modelData.label
                            onClicked: root.openPair(pairButton.modelData.a,
                                                     pairButton.modelData.b)
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(pairButton)
                        }
                    }

                    Component.onCompleted: pairsFlow.measure()
                }
            }

            // SteamOS (Steam Deck и Steam Machine): какие столы ставятся —
            // отдельной строкой под шапкой, во всю ширину (в строке шапки
            // фраза налезала на быстрые пары). Почему неактивна кнопка
            // остальных — строкой в их карточке.
            Text {
                Layout.fillWidth: true
                visible: page.installNoteText !== ""
                text: page.installNoteText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
            }

            // ── Легенда «как читать таблицу» ──────────────────────────────
            Rectangle {
                Layout.fillWidth: true
                color: root.colCard
                border.color: root.colBorder
                border.width: 1
                radius: 6
                // Рамка у Rectangle рисуется ВНУТРЬ, у QSS виджетной
                // рамки — снаружи отступов. Поэтому её точку сверху и
                // снизу приходится добавлять руками, иначе блок ниже
                // оригинала на два пикселя, и к низу страницы набегает
                // столько же, сколько блоков (замер 02.09.2026).
                implicitHeight: legendColumn.implicitHeight + 22

                ColumnLayout {
                    id: legendColumn
                    x: 13
                    y: 11
                    width: parent.width - 26
                    height: implicitHeight
                    // Зазор между подписью и телом легенды у виджетной
                    // вкладки задаёт не код, а стиль (Breeze отдаёт 6):
                    // там `QVBoxLayout` оставлен со своим умолчанием.
                    spacing: 6

                    Text {
                        Layout.fillWidth: true
                        text: page.legendTitle
                        textFormat: Text.RichText
                        color: root.colText
                    }

                    Text {
                        Layout.fillWidth: true
                        text: page.legendBody
                        textFormat: Text.RichText
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                    }
                }
            }

            // ── Заголовок таблицы столов ──────────────────────────────────
            Text {
                text: page.tableHeader
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            // ── Карточки рабочих столов ───────────────────────────────────
            Repeater {
                model: page

                delegate: Rectangle {
                    id: card

                    required property int index
                    required property string deId
                    required property bool installed
                    required property bool expanded
                    required property string headerText
                    required property string ramLabel
                    required property int ramMax
                    required property int ramValue
                    required property string ramColor
                    required property var starsLabels
                    required property var specs
                    required property string description
                    required property string actionText
                    required property bool actionAccent
                    required property string actionKind
                    // SteamOS: почему «Установить» этого стола неактивна;
                    // пусто — активна (или кнопка не «Установить»).
                    readonly property string installBlocked: page.installBlockedText(card.deId)

                    Layout.fillWidth: true
                    color: root.colCard
                    border.color: root.colBorder
                    border.width: 1
                    radius: 6
                    // Плюс два пикселя рамки — та же поправка, что у
                    // легенды: у оригинала рамка карточки лежит снаружи
                    // содержимого, у Rectangle — внутри.
                    implicitHeight: cardColumn.implicitHeight + 2

                    ColumnLayout {
                        id: cardColumn
                        x: 1
                        y: 1
                        width: parent.width - 2
                        height: implicitHeight
                        spacing: 0

                        // Шапка карточки — кликабельная строка целиком, как
                        // в оригинале (там это была QPushButton во всю ширину).
                        Button {
                            id: cardHeader
                            // Имя нужно проверкам: раскрыть карточку снаружи
                            // иначе нечем — `toggle(i32)` через
                            // `QMetaObject.invokeMethod` не зовётся (см.
                            // оговорку у модели), а шапка кликается той же
                            // самой посылкой сигнала, какой её жмёт человек.
                            objectName: "cardHeader"
                            Layout.fillWidth: true
                            flat: true
                            text: card.headerText
                            // Подсветка под курсором и рамка фокуса — пара
                            // цветов из скина, а не жёстко прибитый чёрный:
                            // на тёмной теме тот темнил строку вместо
                            // подсветки (починка ядра, правило #9).
                            background: Rectangle {
                                radius: 6
                                color: cardHeader.hovered || cardHeader.down
                                       ? root.colCardHover : "transparent"
                                border.color: cardHeader.activeFocus
                                              ? root.colAccent : "transparent"
                                border.width: 1
                            }
                            contentItem: Text {
                                text: cardHeader.text
                                color: root.colText
                                horizontalAlignment: Text.AlignLeft
                                verticalAlignment: Text.AlignVCenter
                                elide: Text.ElideRight
                                leftPadding: 14
                                rightPadding: 14
                                topPadding: 10
                                bottomPadding: 10
                            }
                            onClicked: page.toggle(card.index)
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(cardHeader)
                        }

                        // Тело карточки: показывается только раскрытым, как
                        // в оригинале (там это был setVisible на виджете).
                        ColumnLayout {
                            id: cardBody
                            Layout.fillWidth: true
                            Layout.leftMargin: 14
                            Layout.rightMargin: 14
                            Layout.topMargin: 4
                            Layout.bottomMargin: card.expanded ? 12 : 0
                            spacing: 8
                            visible: card.expanded

                            // Память: подпись и полоса
                            Text {
                                Layout.fillWidth: true
                                text: card.ramLabel
                                textFormat: Text.RichText
                                color: root.colText
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: 8
                                radius: 4
                                // Канавка полосы — светлая на тёмной теме
                                // (та же починка пары цветов). Цвет приходит
                                // из ядра палитрой: у виджетной вкладки он
                                // тот же, только в обёртке QSS.
                                color: root.pal.ram_track

                                Rectangle {
                                    height: parent.height
                                    radius: 4
                                    width: card.ramMax > 0
                                           ? parent.width * card.ramValue / card.ramMax
                                           : 0
                                    color: card.ramColor
                                }
                            }

                            // Звёзды: настройка и гейминг
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 24

                                Repeater {
                                    model: card.starsLabels

                                    delegate: Text {
                                        id: starsText
                                        required property string modelData
                                        text: starsText.modelData
                                        textFormat: Text.RichText
                                        color: root.colText
                                    }
                                }

                                Item {
                                    Layout.fillWidth: true
                                }
                            }

                            // ТТХ
                            Repeater {
                                model: card.specs

                                delegate: RowLayout {
                                    id: specRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 10

                                    Text {
                                        Layout.fillWidth: true
                                        text: specRow.modelData.label
                                        color: root.colText
                                    }

                                    Text {
                                        text: specRow.modelData.valueHtml
                                        textFormat: Text.RichText
                                        // Пустой цвет из ядра = «не красить»,
                                        // тогда значение идёт обычным текстом.
                                        color: specRow.modelData.color === ""
                                               ? root.colText
                                               : specRow.modelData.color
                                    }
                                }
                            }

                            // Описание
                            Text {
                                Layout.fillWidth: true
                                Layout.topMargin: 6
                                text: card.description
                                wrapMode: Text.WordWrap
                                color: root.colTextSecondary
                                // Тот же вес, что у легенды: в оригинале
                                // описание — `secondary_text_qss(...)`.
                                // Без него строка выходила уже на 9 точек и
                                // тоньше (замер 03.09.2026, карточка KDE).
                                font.weight: root.pal.text_secondary_weight
                            }

                            // Кнопка действия: установить либо переключиться
                            RowLayout {
                                Layout.fillWidth: true

                                Item {
                                    Layout.fillWidth: true
                                }

                                // ⚠️ У виджетного оригинала эта кнопка
                                // выглядит голым текстом: сброс
                                // `QFrame#deCard QWidget { background:
                                // transparent }` в `card_qss` перекрывает
                                // стиль QPushButton (дефект класса из
                                // правила #9). Здесь она настоящая кнопка
                                // скина — дефект в порт не переносится.
                                // Синяя «Установить» красится тоном из ядра
                                // (те же краски, что `INSTALL_BUTTON_QSS`),
                                // а не `highlighted` — тот брал акцент скина
                                // с другой прозрачностью.
                                RexonButton {
                                    id: cardAction
                                    pal: root.pal
                                    text: card.actionText
                                    // SteamOS: «Установить» непроверенного
                                    // стола видна, но неактивна (причина —
                                    // строкой под кнопкой).
                                    enabled: card.actionKind !== "install"
                                             || card.installBlocked === ""
                                    tone: card.actionAccent
                                          ? root.pal.install_tone : null
                                    onClicked: {
                                        if (card.actionKind === "install")
                                            root.runInstall(card.deId);
                                        else
                                            root.runSwitch(card.deId);
                                    }
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardAction)
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                visible: card.actionKind === "install"
                                         && card.installBlocked !== ""
                                text: card.installBlocked
                                wrapMode: Text.WordWrap
                                color: root.pal.status_unavailable
                            }
                        }
                    }
                }
            }

            // ── Быстрые рекомендации ──────────────────────────────────────
            Text {
                text: page.recHeader
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            Repeater {
                model: page.recRows

                delegate: Rectangle {
                    id: recRow
                    required property var modelData

                    Layout.fillWidth: true
                    color: root.colCard
                    border.color: root.colBorder
                    border.width: 1
                    radius: 6
                    // Плюс два пикселя рамки — см. поправку у легенды.
                    implicitHeight: recLayout.implicitHeight + 18

                    RowLayout {
                        id: recLayout
                        x: 13
                        y: 9
                        width: parent.width - 26
                        height: implicitHeight
                        spacing: 10

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            Text {
                                Layout.fillWidth: true
                                text: recRow.modelData.labelHtml
                                textFormat: Text.RichText
                                color: root.colText
                            }

                            Text {
                                Layout.fillWidth: true
                                text: recRow.modelData.sub
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                            }
                        }

                        // Стол уже стоит — отметка; не стоит — кнопка;
                        // «none» (стол ядру неизвестен) — строка без действия.
                        Text {
                            visible: recRow.modelData.mark === "badge"
                            text: recRow.modelData.text
                            color: recRow.modelData.color === ""
                                   ? root.colText : recRow.modelData.color
                        }

                        RexonButton {
                            id: recButton
                            pal: root.pal
                            visible: recRow.modelData.mark === "install"
                            enabled: page.installBlockedText(recRow.modelData.deId) === ""
                            text: recRow.modelData.text
                            onClicked: root.runInstall(recRow.modelData.deId)
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(recButton)
                        }
                    }
                }
            }
        }
    }

    // ── Диалог «X против Y» ───────────────────────────────────────────────

    function openPair(aId, bId) {
        const view = page.pairView(aId, bId);
        // Пустой ответ — пара ядру неизвестна. Виджетная вкладка в этом
        // случае просто не открывала диалог; здесь так же.
        if (!view)
            return;
        pairDialog.view = view;
        pairDialog.open();
    }

    Popup {
        id: pairDialog

        property var view: null
        // Причины неактивных кнопок установки пары, без повторов.
        readonly property string blockedText: {
            const out = [];
            for (const b of (pairDialog.view ? pairDialog.view.buttons : [])) {
                const why = page.installBlockedText(b.deId);
                if (why !== "" && out.indexOf(why) < 0)
                    out.push(why);
            }
            return out.join("\n");
        }

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        width: Math.min(root.width - 40,
                        pairDialog.view ? pairDialog.view.minWidth : 720)
        height: Math.min(root.height - 40,
                         pairDialog.view ? pairDialog.view.minHeight : 520)
        padding: 16

        background: Rectangle {
            color: root.pal.bg_secondary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        // Затемнение страницы под модальным окном. Стиль по умолчанию его не
        // рисует, и получалось, что окно модальное, а на вид — нет: человек
        // тыкает в карточки под ним и не понимает, почему не отвечают
        // (защита от дурака, правило #13). Оттенок — общий для продукта
        // (`ui_base::MODAL_DIM_RGBA`, приезжает в палитре ключом
        // `modal_dim`): тем же гасит окно обучение, своего числа у вкладки
        // нет и быть не должно.
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        // Диалог перерисовывается на каждое открытие: содержимое приходит
        // от ядра целиком и от прошлой пары в нём не остаётся ничего.
        contentItem: ColumnLayout {
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: pairDialog.view ? pairDialog.view.heading : ""
                textFormat: Text.RichText
                color: root.colText
                font.pointSize: pairDialog.view
                                ? pairDialog.view.titlePointSize : 16
                font.bold: true
            }

            // SteamOS: почему кнопки установки внизу окна неактивны — те же
            // причины, что в карточках этих столов.
            Text {
                Layout.fillWidth: true
                visible: pairDialog.blockedText !== ""
                text: pairDialog.blockedText
                wrapMode: Text.WordWrap
                color: root.pal.status_unavailable
            }

            // Таблица «параметр — X — Y» приходит готовым HTML из ядра:
            // тот же текст, что показывал QTextBrowser виджетной вкладки.
            Flickable {
                id: pairFlick
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: width
                contentHeight: pairBody.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                ScrollBar.vertical: ScrollBar {}

                Text {
                    id: pairBody
                    width: pairFlick.width
                    text: pairDialog.view ? pairDialog.view.html : ""
                    textFormat: Text.RichText
                    wrapMode: Text.WordWrap
                    color: root.colText
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item {
                    Layout.fillWidth: true
                }

                Repeater {
                    model: pairDialog.view ? pairDialog.view.buttons : []

                    delegate: RexonButton {
                        id: pairInstall
                        required property var modelData
                        pal: root.pal
                        text: pairInstall.modelData.text
                        enabled: page.installBlockedText(pairInstall.modelData.deId) === ""
                        // Та же синяя кнопка ядра, что в карточке.
                        tone: pairInstall.modelData.accent
                              ? root.pal.install_tone : null
                        onClicked: {
                            const deId = pairInstall.modelData.deId;
                            pairDialog.close();
                            root.runInstall(deId);
                        }
                    }
                }

                RexonButton {
                    id: pairClose
                    pal: root.pal
                    text: pairDialog.view ? pairDialog.view.closeText
                                          : page.pairCloseText
                    onClicked: pairDialog.close()
                }
            }
        }

        // Фокус при открытии — на кнопке закрытия: диалог обязан быть
        // управляем с клавиатуры с первого кадра, не только мышью.
        onOpened: pairClose.forceActiveFocus()
    }

    // ── Фокус с первого кадра ─────────────────────────────────────────────

    // ⚠️ Фокус берёт ПЕРВЫЙ КОНТРОЛ, а не корень страницы. `forceActiveFocus`
    // на корне выглядит правильно, но делает `activeFocusItem` безымянным
    // Item'ом: ни одна кнопка фокуса не получает, и страница мертва и с
    // клавиатуры, и с геймпада. Поймано макетом каркаса вкладок, не гаданием.
    property int focusAttempts: 0

    function focusFirstControl() {
        const first = pairRepeater.count > 0 ? pairRepeater.itemAt(0) : null;
        if (first) {
            root.focusAttempts = 0;
            first.forceActiveFocus();
            return;
        }
        // Повторитель наполняется не мгновенно. Пробуем на следующем проходе
        // цикла событий, но не бесконечно: пустой список пар — это поломка
        // ядра, и вечный самовызов её только спрячет.
        if (root.focusAttempts < 20) {
            root.focusAttempts += 1;
            Qt.callLater(root.focusFirstControl);
        } else {
            console.warn("вкладка сравнения: кнопок пар нет, фокус взять не с чего");
        }
    }

    // ── Окно операции над столом ──────────────────────────────────────────
    //
    // То же окно, что у «Рабочих столов» (`DeOperationDialog.qml`), и тот же
    // объект действий: второго исполнителя и второго диалога нет.

    DeOperationDialog {
        anchors.fill: parent
        actions: root.actions
        pal: root.pal
        onDesktopsChanged: page.refreshInstalled()
    }

    Component.onCompleted: root.focusFirstControl()

    // Во встроенной вкладке страница создаётся заранее и оживает при
    // переключении на неё — фокус надо взять и там, иначе первый Tab
    // после смены вкладки уходит в никуда.
    onVisibleChanged: if (root.visible) root.focusFirstControl()
}
