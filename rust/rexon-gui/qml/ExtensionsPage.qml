// Вкладка «Расширения» на QML. Данные и решения — Rust-объект
// ExtensionsModel (rust/rexon-gui/src/extensions_page.rs), который сам
// ничего не решает, а спрашивает ядро (`rexon_core::qt_extensions`). Здесь
// только РАЗМЕТКА.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из `ui_base::skin_colors("steam_dark")` — скина по
// умолчанию всего продукта (правило #17). Своих цветов страница не заводит;
// сторожит это тест ядра `qt_extensions_parity::page_colors_come_from_core`.
//
// Побочные действия (установка .rxn, удаление, включение/выключение) живут
// снаружи, объектом со свойством `root.actions`: installDialog / installPath /
// busy / confirm / uninstall / toggle / openFolder плюс сигналы
// installFinished / uninstallFinished. Обещание «страница не изменится»
// выполнено: с приходом Rust-объекта (`ExtensionsActions`,
// rust/rexon-gui/src/extensions_actions.rs) ни один вызов здесь не переписан,
// добавлено ровно умолчание ниже.
//
// Умолчание — Rust-объект, заведённый самой страницей. Так вкладка полна в
// процессе без Python вовсе; хозяин на PySide6 (`rexon/qt_extensions/window.py`)
// по-прежнему может подставить свой объект — присваивание свойства снимает
// привязку. Пустой `actions` больше не значит «кнопки молчат».
//
// Цепочка фокуса заложена с самого начала: все действия — на контролах, у
// сфокусированного видна рамка, страница подкручивается к нему.

// Делегаты обращаются к id внешних элементов (root, page). Без этой прагмы
// проверка QML справедливо ругается на неквалифицированный доступ.
// (Строку «// qmllint …» здесь писать нельзя: она читается как директива.)
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    // Объект побочных действий. По умолчанию — свой, на Rust; хозяин
    // страницы может подставить другой.
    // Методы: installDialog() -> string, installPath(path) -> bool,
    //         busy() -> bool, confirm(title, text) -> bool,
    //         uninstall(extId) -> bool, toggle(extId) -> string,
    //         openFolder(dir) -> bool.
    property var actions: ownActions

    ExtensionsActions {
        id: ownActions
        // Имя нужно проверкам — тем же приёмом, что у модели ниже.
        objectName: "extensionsActions"
    }

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    ExtensionsModel {
        id: page
        // Имя нужно проверкам: модель достают
        // `root.findChild(QObject, "extensionsModel")` и спрашивают её без
        // единой правки в самой странице.
        objectName: "extensionsModel"
    }

    // ── Палитра стандартного скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary
    readonly property color colInfo: root.pal.accent_info
    readonly property color colDanger: root.pal.danger

    // Оформление вкладки, которого нет в палитре скина: чипы, плитка,
    // заглушка превью, строка списка, блок «Установить», раскладка. Всё
    // приходит готовым от ядра (`qt_extensions`) — тем же набором чисел и
    // красок, каким набраны компоновки и таблицы стилей виджетной вкладки.
    readonly property var st: page.styles
    // Чип (кнопки вида и фильтры): рамка без заливки, выбранный — синяя
    // заливка с рамкой. Тот же чип, что у подвкладок Steam.
    readonly property var toneChip: ({
        "background": "transparent",
        "border": root.st.chip.border,
        "hover": root.st.chip.hoverBackground,
        "radius": root.st.chip.radius
    })
    readonly property var toneChipOn: ({
        "background": root.st.chip.checkedBackground,
        "border": root.st.chip.checkedBorder,
        "hover": root.st.chip.checkedBackground,
        "radius": root.st.chip.radius
    })
    // Кнопка «🗑 Удалить» подробного разбора: заливка у неё своя, гуще
    // плиточной (0,25 против 0,2 — так набрана таблица стилей оригинала).
    readonly property var toneDetailDelete: ({
        "fillAlpha": root.st.dialog.dangerFillAlpha
    })
    // Крупная кнопка блока «Установить»: рамка и лёгкая заливка, скругление 6.
    readonly property var toneSource: ({
        "background": root.st.source.background,
        "border": root.st.source.border,
        "hover": root.st.source.hover,
        "radius": root.st.source.radius
    })

    // ── Побочные действия ─────────────────────────────────────────────────

    // Установка идёт в фоне у хозяина: сюда возвращается только «взялись ли
    // за дело». Итог приходит сигналом (см. Connections ниже) — так строка
    // состояния и витрина обновляются тогда же, когда в виджетной вкладке.
    // Что делает кнопка блока «Установить». Разбор вида действия — здесь, а
    // не в делегате: так его можно позвать и с клавиатуры, и проверкой, не
    // изображая нажатие мышью по координатам.
    function runSource(kind) {
        if (kind === "file")
            root.runInstallDialog();
        else if (kind === "create")
            createDialog.open();
        // «workshop» выключен: кнопка неактивна, нажать её нельзя.
    }

    function runInstallDialog() {
        if (!root.actions)
            return;
        const path = root.actions.installDialog();
        if (path)
            root.runInstallPath(path);
    }

    function runInstallPath(path) {
        if (!root.actions)
            return;
        if (root.actions.busy()) {
            page.noteInstallBusy();
            return;
        }
        page.noteInstalling(path);
        root.actions.installPath(path);
    }

    function runUninstall(extId, detailed) {
        if (!root.actions)
            return;
        const question = detailed ? page.confirmUninstallDetailed(extId)
                                  : page.confirmUninstall(extId);
        if (!root.actions.confirm(page.confirmTitle, question))
            return;
        if (root.actions.busy()) {
            page.noteUninstallBusy();
            return;
        }
        page.noteUninstalling(page.displayName(extId));
        root.actions.uninstall(extId);
    }

    function runToggle(extId) {
        if (!root.actions)
            return;
        // Ответ — «on» / «off» / текст ошибки с приставкой «!». Пары
        // «получилось ли + новое состояние» одним значением не выразить, а
        // два вызова подряд разошлись бы с состоянием на диске.
        const answer = root.actions.toggle(extId);
        if (answer === "on")
            page.noteEnabled(extId);
        else if (answer === "off")
            page.noteDisabled(extId);
        else
            page.noteToggleError(answer.substring(1));
        page.refresh();
    }

    Connections {
        target: root.actions

        function onInstallFinished(ok, message) {
            if (ok)
                page.noteInstalled(message);
            else
                page.noteInstallError(message);
            page.refresh();
        }

        function onUninstallFinished(ok, message) {
            if (ok)
                page.noteUninstalled(message);
            else
                page.noteUninstallError(message);
            page.refresh();
        }

        function onApplyFinished(message) {
            page.noteText(message);
        }
    }

    // «Применить» у набора обоев: ставит обои на рабочий стол прямо отсюда.
    // Прежде у плитки была одна «Удалить», и поставленное из файла нечем
    // было включить, не уходя в «Настройки».
    function runApply(extId) {
        if (root.actions && root.actions.apply)
            page.noteText(root.actions.apply(extId));
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

    // ── Перетаскивание файлов ─────────────────────────────────────────────
    //
    // Тот же набор расширений, что принимала виджетная вкладка; решает о нём
    // ядро (`qt_extensions::is_supported_drop`), а не список в разметке.
    DropArea {
        id: dropZone
        anchors.fill: parent
        keys: ["text/uri-list"]

        onEntered: (drag) => {
            drag.accepted = root.acceptableDrop(drag.urls).length > 0;
        }
        onDropped: (drop) => {
            const good = root.acceptableDrop(drop.urls);
            if (good.length === 0)
                return;
            drop.acceptProposedAction();
            root.runInstallPath(good[0]);
        }
    }

    function acceptableDrop(urls) {
        const out = [];
        for (let i = 0; i < urls.length; ++i) {
            const path = String(urls[i]).replace(/^file:\/\//, "");
            if (page.acceptsDrop(path))
                out.push(path);
        }
        return out;
    }

    // Подсветка окна под перетаскиванием: без неё человек не понимает,
    // возьмёт ли программа файл (защита от дурака, правило #13).
    Rectangle {
        anchors.fill: parent
        visible: dropZone.containsDrag
        color: "transparent"
        border.color: root.colAccent
        border.width: 3
        z: 5
    }

    // ── Страница ──────────────────────────────────────────────────────────

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: pageColumn.implicitHeight + 2 * root.st.page.margin
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {}

        // ⚠️ Раскладка внутри ОБЫЧНОГО Item (не внутри другой раскладки)
        // обязана получить высоту руками: сама она остаётся нулевой, и
        // страница схлопывается в полоску.
        ColumnLayout {
            id: pageColumn
            x: root.st.page.margin
            y: root.st.page.margin
            width: flick.width - 2 * root.st.page.margin
            height: implicitHeight
            spacing: root.st.page.spacing

            // ── Шапка ─────────────────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: page.headerTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                }

                Item {
                    Layout.fillWidth: true
                }

                // Кнопки вида — чипы оригинала (`QToolButton:checked` синим),
                // а не выделенные кнопки скина: кегль значка 14 и наименьшая
                // ширина 32 — из ядра. Выбор рисуется тоном, а не `checked`:
                // повторное нажатие на выбранную не должно её «снимать».
                RexonButton {
                    id: gridButton
                    pal: root.pal
                    text: page.viewGridText
                    tone: page.viewMode === "grid" ? root.toneChipOn : root.toneChip
                    font.pointSize: root.st.viewButton.pointSize
                    leftPadding: root.st.chip.paddingH
                    rightPadding: root.st.chip.paddingH
                    implicitWidth: Math.max(implicitContentWidth, root.st.viewButton.minWidth)
                                   + leftPadding + rightPadding
                    ToolTip.visible: hovered
                    ToolTip.text: page.viewGridTooltip
                    onClicked: page.setViewMode("grid")
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(gridButton)
                }

                RexonButton {
                    id: listButton
                    pal: root.pal
                    text: page.viewListText
                    tone: page.viewMode === "list" ? root.toneChipOn : root.toneChip
                    font.pointSize: root.st.viewButton.pointSize
                    leftPadding: root.st.chip.paddingH
                    rightPadding: root.st.chip.paddingH
                    implicitWidth: Math.max(implicitContentWidth, root.st.viewButton.minWidth)
                                   + leftPadding + rightPadding
                    ToolTip.visible: hovered
                    ToolTip.text: page.viewListTooltip
                    onClicked: page.setViewMode("list")
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(listButton)
                }

                // Вход в библиотеку Razer Axon — ПОКА ДЛЯ ПРОВЕРКИ: подпись
                // «(тест)», окно простое; потом его заменит полноценный раздел.
                RexonButton {
                    id: axonButton
                    objectName: "axonButton"
                    pal: root.pal
                    text: axonDialog.texts.button || ""
                    onClicked: axonDialog.openLibrary()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(axonButton)
                }

                RexonButton {
                    id: installButton
                    pal: root.pal
                    text: page.installButtonText
                    highlighted: true
                    // Оригинал: `font-weight: bold` в таблице стилей кнопки.
                    font.bold: true
                    onClicked: root.runInstallDialog()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(installButton)
                }
            }

            // ── Поиск ─────────────────────────────────────────────────────
            RexonTextField {
                id: searchField
                Layout.fillWidth: true
                pal: root.pal
                placeholderText: page.searchPlaceholder
                text: page.searchText
                onTextEdited: page.setSearch(text)
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(searchField)
            }

            // ── Кнопки фильтров ───────────────────────────────────────────
            //
            // ⚠️ Flow, а не RowLayout: типов расширений тридцать пять, и на
            // минимальной ширине окна (900, число из ядра) кнопки в одну
            // строку не помещаются. RowLayout переносить не умеет и просто
            // обрезал бы хвост — тот же дефект, что был у шапки «Сравнения».
            Flow {
                Layout.fillWidth: true
                spacing: root.st.page.filterSpacing

                Repeater {
                    model: page.filters

                    delegate: RexonButton {
                        id: filterButton
                        required property var modelData
                        pal: root.pal
                        text: filterButton.modelData.label
                        tone: filterButton.modelData.checked ? root.toneChipOn
                                                             : root.toneChip
                        leftPadding: root.st.chip.paddingH
                        rightPadding: root.st.chip.paddingH
                        onClicked: page.setFilter(filterButton.modelData.id)
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(filterButton)
                    }
                }
            }

            // ── Статистика ────────────────────────────────────────────────
            Text {
                Layout.fillWidth: true
                text: page.statsLine
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                // Оригинал: `font-weight: 600` у всех вторичных подписей —
                // тонкое начертание нечитаемо на тёмном (правило владельца
                // 2026-06-11). Число приходит из ядра палитрой
                // (`ui_base::SECONDARY_TEXT_WEIGHT`), как у «Сравнения».
                font.weight: root.pal.text_secondary_weight
            }

            // ── «Мои расширения» ──────────────────────────────────────────
            Text {
                text: page.myHeader
                visible: page.myHeader !== ""
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            // ── Витрина: сетка плиток ─────────────────────────────────────
            //
            // Число столбцов считается по ширине полосы и ПОСТОЯННОЙ ширине
            // плитки (260 из ядра) — так же, как это делал общий движок
            // сеток виджетной вкладки. Свои ступени «<700 → 2, <1100 → 3»
            // там уже были признаны неверными: выше 1500 px число столбцов
            // замирало на пяти и справа оставалось пустое поле.
            // ⚠️ GridLayout, а не Flow: плитки в ОДНОМ РЯДУ обязаны быть
            // одной высоты, иначе кнопки «Выключить» и «🗑» встают на разной
            // высоте — у расширений набор строк разный (автор есть не у
            // всех, статус «Активно» — только у скриптовых). В виджетной
            // вкладке это делала распорка `card_actions_spacer`, которая
            // снимала карточке вертикальную фиксацию и растягивала её на
            // высоту ряда. Flow высоту ряда не выравнивает — вышла бы
            // деградация, а не перенос.
            //
            // Выравнивание влево (без AlignTop) — тоже из оригинала: с
            // AlignTop плитка зажимается на своей высоте, и ряд снова
            // разъезжается.
            GridLayout {
                id: cardsGrid
                Layout.alignment: Qt.AlignLeft
                visible: page.viewMode === "grid"
                columnSpacing: root.st.page.gridSpacing
                rowSpacing: root.st.page.gridSpacing
                columns: Math.max(1, Math.floor((pageColumn.width + columnSpacing)
                                                / (page.cardWidth + columnSpacing)))

                Repeater {
                    model: page.viewMode === "grid" ? page : null

                    delegate: Rectangle {
                        id: card

                        required property int index
                        required property string extId
                        required property string name
                        required property string nameTooltip
                        required property string typeLine
                        required property string authorLine
                        required property string imageUrl
                        required property string placeholder
                        required property bool hasToggle
                        required property string stateText
                        required property string stateColor
                        required property string toggleText
                        /// Картинка плитки прочиталась — её можно открыть
                        /// крупно (заглушку-значок открывать незачем).
                        readonly property bool imageReady: cardImage.visible
                        /// Запись плитки для крупного просмотра.
                        readonly property var viewerItem: card.imageReady ? ({
                            "key": card.extId,
                            "title": card.name,
                            "subtitle": card.typeLine,
                            "thumb": card.imageUrl,
                            "source": card.imageUrl
                        }) : null
                        onViewerItemChanged: root.registerViewerItem(card.index, card.viewerItem)
                        Component.onCompleted: root.registerViewerItem(card.index, card.viewerItem)
                        Component.onDestruction: root.registerViewerItem(card.index, null)

                        Layout.preferredWidth: page.cardWidth
                        Layout.maximumWidth: page.cardWidth
                        Layout.fillHeight: true
                        Layout.alignment: Qt.AlignLeft
                        implicitHeight: cardColumn.implicitHeight + 2 * root.st.card.margin
                        // Краски плитки — из ядра (`QFrame#extCard` оригинала):
                        // под курсором меняется только рамка, фон остаётся.
                        color: root.st.card.background
                        border.color: cardHover.hovered ? root.st.card.hover
                                                        : root.st.card.border
                        border.width: 1
                        radius: root.st.card.radius

                        // Клик по плитке — подробный разбор, как в оригинале
                        // (там это был mousePressEvent всей карточки).
                        HoverHandler {
                            id: cardHover
                        }
                        TapHandler {
                            onTapped: root.openDetail(card.extId)
                        }

                        ColumnLayout {
                            id: cardColumn
                            x: root.st.card.margin
                            y: root.st.card.margin
                            width: parent.width - 2 * root.st.card.margin
                            // Высоту берём от плитки, а не от своего
                            // implicitHeight: плитку растянул ряд, и ряд
                            // кнопок ниже обязан этим воспользоваться, чтобы
                            // прижаться к нижнему краю.
                            height: parent.height - 2 * root.st.card.margin
                            spacing: root.st.card.spacing

                            // Превью либо значок типа. Заглушка показывается
                            // ровно тогда, когда картинка не прочиталась —
                            // тот же выбор делал виджет по пустому пиксмапу.
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: page.previewHeight
                                radius: root.st.card.radius
                                color: root.st.placeholder.background
                                clip: true

                                Image {
                                    id: cardImage
                                    anchors.fill: parent
                                    source: card.imageUrl
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    visible: status === Image.Ready
                                }

                                Text {
                                    anchors.centerIn: parent
                                    visible: !cardImage.visible
                                    text: card.placeholder
                                    color: root.st.placeholder.color
                                    font.pointSize: root.st.placeholder.pointSize
                                }

                                // Щелчок по плитке занят описанием, поэтому
                                // крупный просмотр открывает лупа.
                                RexonZoomArea {
                                    anchors.fill: parent
                                    pal: root.pal
                                    badgeOnly: true
                                    radius: root.st.card.radius
                                    enabled: card.imageReady
                                    visible: enabled
                                    onActivated: root.openViewer(card.extId)
                                }
                            }

                            // Тип и автор — вторичные подписи оригинала:
                            // цвет из пары `ui_base` (починка №2 в ядре),
                            // начертание 600 — как там.
                            Text {
                                Layout.fillWidth: true
                                text: card.typeLine
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                elide: Text.ElideRight
                            }

                            Text {
                                Layout.fillWidth: true
                                text: card.name
                                color: root.colText
                                font.bold: true
                                wrapMode: Text.Wrap
                                maximumLineCount: 2
                                elide: Text.ElideRight
                                ToolTip.visible: nameHover.hovered
                                                 && truncated
                                ToolTip.text: card.nameTooltip
                                HoverHandler {
                                    id: nameHover
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                visible: card.authorLine !== ""
                                text: card.authorLine
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                elide: Text.ElideRight
                            }

                            Text {
                                visible: card.hasToggle
                                text: card.stateText
                                color: card.stateColor
                            }

                            // Ряд кнопок забирает остаток высоты ряда и
                            // прижимает их к низу — это распорка
                            // `card_actions_spacer` оригинала: у расширений
                            // набор строк разный (автор есть не у всех,
                            // статус — только у скриптовых), и без неё
                            // кнопки в ряду стояли бы на разной высоте.
                            // Отдельный Item-распорка здесь не годится:
                            // раскладка добавляла к нему свой шаг, и над
                            // кнопками выходило на 10 точек больше, чем у
                            // оригинала (замер 03.09: 20 против 8).
                            // ⚠️ Одного `fillHeight` мало, и это не
                            // придирка: у ряда есть ещё и ПОТОЛОК высоты,
                            // который он считает по своим детям. Кнопки
                            // прижаты `Qt.AlignBottom`, выровненная ячейка
                            // не тянется, и потолок ряда становится равен
                            // высоте кнопки. Остаток высоты плитке деть
                            // некуда — раскладка размазывает его по ВСЕМ
                            // промежуткам: над превью появлялся лишний
                            // отступ, а кнопки до нижнего края так и не
                            // доходили (замер 18.09 на стенде: отступ
                            // сверху 19 против 7 точек, кнопки выше
                            // соседних на 4). Потолок снят — весь остаток
                            // забирает ряд кнопок, как забирала его
                            // распорка оригинала.
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                Layout.maximumHeight: Number.POSITIVE_INFINITY
                                spacing: root.st.card.buttonSpacing

                                RexonButton {
                                    id: cardToggle
                                    Layout.alignment: Qt.AlignBottom
                                    visible: card.hasToggle
                                    pal: root.pal
                                    text: card.toggleText
                                    onClicked: root.runToggle(card.extId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardToggle)
                                }

                                RexonButton {
                                    id: cardApply
                                    Layout.alignment: Qt.AlignBottom
                                    readonly property string label:
                                        (root.actions && root.actions.applyText)
                                        ? root.actions.applyText(card.extId) : ""
                                    visible: cardApply.label !== ""
                                    pal: root.pal
                                    text: cardApply.label
                                    onClicked: root.runApply(card.extId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardApply)
                                }

                                RexonDangerButton {
                                    id: cardDelete
                                    Layout.alignment: Qt.AlignBottom
                                    pal: root.pal
                                    // Подпись, а не одна корзина: удаление
                                    // необратимо, и угадывать значок
                                    // покупатель не обязан (правило #13).
                                    // Подробности отступления от эталона —
                                    // у ключа `UNINSTALL_TEXT` в ядре.
                                    // Ширину кнопка меряет по подписи сама,
                                    // как соседний выключатель.
                                    text: page.uninstallText
                                    ToolTip.visible: hovered
                                    ToolTip.text: page.uninstallTooltip
                                    onClicked: root.runUninstall(card.extId, false)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardDelete)
                                }

                                Item {
                                    Layout.fillWidth: true
                                }
                            }
                        }
                    }
                }
            }

            // ── Витрина: список с группами по типам ───────────────────────
            // У виджетной вкладки заголовки групп и строки — соседние ряды
            // ОДНОЙ сетки с шагом плиток (10), поэтому и здесь шаг сетки —
            // и между блоками, и между заголовком и его первой строкой.
            ColumnLayout {
                Layout.fillWidth: true
                visible: page.viewMode === "list"
                spacing: root.st.page.gridSpacing

                Repeater {
                    model: page.viewMode === "list" ? page : null

                    delegate: ColumnLayout {
                        id: rowBlock

                        required property string extId
                        required property string typeLine
                        required property string titleHtml
                        required property string subtitle
                        required property string placeholder
                        required property bool hasToggle
                        required property string toggleIcon
                        required property string toggleTooltip
                        required property string groupHeader

                        Layout.fillWidth: true
                        spacing: root.st.page.gridSpacing

                        Text {
                            Layout.fillWidth: true
                            Layout.topMargin: root.st.group.paddingTop
                            Layout.bottomMargin: root.st.group.paddingBottom
                            visible: rowBlock.groupHeader !== ""
                            text: rowBlock.groupHeader
                            textFormat: Text.RichText
                            color: root.colInfo
                        }

                        // Поля строки — поля компоновки плюс `padding` её
                        // таблицы стилей: у виджетов они складываются.
                        Rectangle {
                            id: rowFrame
                            readonly property int padH: root.st.row.marginH + root.st.row.padding
                            readonly property int padV: root.st.row.marginV + root.st.row.padding
                            Layout.fillWidth: true
                            implicitHeight: rowLayout.implicitHeight + 2 * rowFrame.padV
                            radius: root.st.row.radius
                            color: rowHover.hovered ? root.st.row.hover : root.st.row.background

                            HoverHandler {
                                id: rowHover
                            }
                            TapHandler {
                                onTapped: root.openDetail(rowBlock.extId)
                            }

                            RowLayout {
                                id: rowLayout
                                x: rowFrame.padH
                                y: rowFrame.padV
                                width: parent.width - 2 * rowFrame.padH
                                height: implicitHeight
                                spacing: root.st.row.spacing

                                Text {
                                    Layout.preferredWidth: root.st.row.iconWidth
                                    text: rowBlock.placeholder
                                    color: root.colText
                                    font.pointSize: root.st.row.iconPointSize
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0

                                    Text {
                                        Layout.fillWidth: true
                                        text: rowBlock.titleHtml
                                        textFormat: Text.RichText
                                        color: root.colText
                                        elide: Text.ElideRight
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        visible: rowBlock.subtitle !== ""
                                        text: rowBlock.subtitle
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        elide: Text.ElideRight
                                    }
                                }

                                RexonButton {
                                    id: rowToggle
                                    visible: rowBlock.hasToggle
                                    pal: root.pal
                                    text: rowBlock.toggleIcon
                                    implicitWidth: root.st.row.buttonWidth
                                    leftPadding: 0
                                    rightPadding: 0
                                    ToolTip.visible: hovered
                                    ToolTip.text: rowBlock.toggleTooltip
                                    onClicked: root.runToggle(rowBlock.extId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(rowToggle)
                                }

                                RexonDangerButton {
                                    id: rowDelete
                                    pal: root.pal
                                    text: page.uninstallIcon
                                    implicitWidth: root.st.row.buttonWidth
                                    leftPadding: 0
                                    rightPadding: 0
                                    ToolTip.visible: hovered
                                    ToolTip.text: page.uninstallTooltip
                                    onClicked: root.runUninstall(rowBlock.extId, false)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(rowDelete)
                                }
                            }
                        }
                    }
                }
            }

            // ── Пустое состояние ──────────────────────────────────────────
            Text {
                Layout.fillWidth: true
                Layout.margins: root.st.page.emptyPadding
                visible: page.emptyText !== ""
                text: page.emptyText
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Блок «⬇ Установить» ───────────────────────────────────────
            Text {
                text: page.installSectionHeader
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            Flow {
                Layout.fillWidth: true
                spacing: root.st.source.spacing

                Repeater {
                    model: page.installSources

                    delegate: RexonButton {
                        id: sourceButton
                        required property var modelData
                        pal: root.pal
                        text: sourceButton.modelData.text
                        enabled: sourceButton.modelData.enabled
                        tone: root.toneSource
                        padding: root.st.source.padding
                        leftPadding: root.st.source.padding
                        rightPadding: root.st.source.padding
                        implicitWidth: Math.max(page.sourceMinWidth,
                                                Math.min(page.sourceMaxWidth,
                                                         implicitContentWidth
                                                         + leftPadding + rightPadding))
                        implicitHeight: page.sourceMinHeight
                        ToolTip.visible: hovered
                        ToolTip.text: sourceButton.modelData.tooltip
                        onClicked: root.runSource(sourceButton.modelData.kind)
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(sourceButton)
                    }
                }
            }

            // ── Строка состояния ──────────────────────────────────────────
            //
            // Место под строку занято ВСЕГДА, даже когда сообщать нечего:
            // у оригинала это постоянный `QLabel`, который прячут не по
            // пустому тексту, а никогда, и пустая строка держит свои 21
            // точку между блоком «Установить» и планами. Пока здесь стояло
            // `visible: statusText !== ""`, строка схлопывалась, планы
            // поднимались на 30 точек выше оригинала, а при первой же
            // установке вся страница дёргалась вниз.
            Text {
                Layout.fillWidth: true
                text: page.statusText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Планы ─────────────────────────────────────────────────────
            Text {
                Layout.fillWidth: true
                text: page.roadmapNote
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ── Подробный разбор расширения ───────────────────────────────────────

    // ── Крупный просмотр картинки расширения ──────────────────────────────
    //
    // Слой общий на весь продукт (`RexonImageViewer.qml`). Листание — по
    // плиткам витрины, у которых картинка есть.
    property var viewerRegistry: ({})

    function registerViewerItem(row, info) {
        if (info)
            root.viewerRegistry[row] = info;
        else
            delete root.viewerRegistry[row];
    }

    function openViewer(extId) {
        const items = [];
        let at = 0;
        // Что показать, плитки говорят сами (`viewerRegistry`): номер плитки
        // на витрине → её запись. Плитки с заглушкой вместо картинки в
        // список не попадают.
        const rows = Object.keys(root.viewerRegistry).map(Number).sort((x, y) => x - y);
        for (const row of rows) {
            const info = root.viewerRegistry[row];
            if (info.key === extId)
                at = items.length;
            items.push(info);
        }
        viewer.show(items, at);
    }

    RexonImageViewer {
        id: viewer
        objectName: "imageViewer"
        pal: root.pal
    }

    function openDetail(extId) {
        const view = page.detailFor(extId);
        // Пустой ответ — такого расширения на витрине нет. Виджетная вкладка
        // в этом случае просто не открывала окно; здесь так же.
        if (!view)
            return;
        detailDialog.view = view;
        detailDialog.open();
    }

    Popup {
        id: detailDialog
        // Имя нужно стенду приёмки: окно находят по нему, чтобы снять его
        // отдельно от страницы.
        objectName: "detailDialog"

        property var view: null

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        // Числа из ядра — это МИНИМУМ окна, а не его размер: у виджетного
        // окна `setMinimumSize` не мешало ему занять больше, и на большом
        // мониторе README читался целиком. Модальное окно QML своего
        // размера не имеет вовсе, поэтому берём минимум как нижнюю границу,
        // а не как потолок.
        width: Math.min(root.width - 40,
                        Math.max(page.detailMinWidth, root.width * 0.6))
        height: Math.min(root.height - 40,
                         Math.max(page.detailMinHeight, root.height * 0.8))
        padding: root.st.dialog.margin

        background: Rectangle {
            // Фон окна — тот же, что у виджетного диалога: роль `Window`
            // палитры скина. Вторичный фон здесь был темнее оригинала на
            // весь диалог, а он принадлежит области описания (см. ниже).
            color: root.pal.bg_primary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        // Затемнение страницы под модальным окном. Стиль по умолчанию его не
        // рисует, и получалось бы, что окно модальное, а на вид нет: человек
        // тыкает в плитки под ним и не понимает, почему не отвечают (защита
        // от дурака, правило #13). Оттенок — общий для продукта
        // (`ui_base::MODAL_DIM_RGBA`, приезжает в палитре ключом `modal_dim`).
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: root.st.dialog.detailSpacing

            Text {
                Layout.fillWidth: true
                text: detailDialog.view ? detailDialog.view.headingHtml : ""
                textFormat: Text.RichText
                color: root.colText
                font.pointSize: page.detailTitlePointSize
                font.bold: true
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                text: detailDialog.view ? detailDialog.view.metaLine : ""
                color: root.colTextSecondary
                // Оригинал: `font-weight: 600` у мета-строки, техсведений и
                // пути — тот же набор вторичных подписей, что на витрине.
                font.weight: root.pal.text_secondary_weight
                wrapMode: Text.WordWrap
            }

            Image {
                Layout.fillWidth: true
                // Превью занимает свою высоту, пока она есть, и УСТУПАЕТ её,
                // когда окно ужато до минимума. Без этого картинка держала
                // свою натуральную высоту как нижнюю границу и выдавливала
                // описание вовсе: на окне 680×540 (а это разрешённый
                // минимум) текста не оставалось ни строки, тогда как у
                // виджетного окна сжимаются оба блока (правило #13).
                Layout.fillHeight: true
                Layout.preferredHeight: page.detailPreviewHeight
                Layout.minimumHeight: 0
                Layout.maximumHeight: page.detailPreviewHeight
                source: detailDialog.view ? detailDialog.view.previewUrl : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                visible: status === Image.Ready

                RexonZoomArea {
                    anchors.fill: parent
                    pal: root.pal
                    onActivated: viewer.show([{
                        "key": detailDialog.view.extId,
                        "title": page.displayName(detailDialog.view.extId),
                        "subtitle": detailDialog.view.metaLine,
                        "thumb": detailDialog.view.previewUrl,
                        "source": detailDialog.view.previewUrl
                    }], 0)
                }
            }

            // Подложка области описания. У виджетного окна это
            // `QTextBrowser`: своя рамка, своё скругление и фон роли `Base`,
            // отличный от фона окна. Голый Flickable этого не давал, и текст
            // висел прямо на подложке окна — область не читалась как поле.
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                // Ниже этой границы описание не сжимается — так же, как
                // `QTextBrowser` виджетного окна. Нехватку места на ужатом
                // окне забирает превью, а текст остаётся читаемым.
                Layout.minimumHeight: root.st.dialog.bodyMinHeight
                color: root.pal.bg_secondary
                border.color: root.colBorder
                border.width: root.st.dialog.bodyBorderWidth
                radius: root.st.dialog.bodyRadius

                Flickable {
                    id: bodyFlick
                    anchors.fill: parent
                    anchors.margins: root.st.dialog.bodyPadding
                                     + root.st.dialog.bodyBorderWidth
                    contentWidth: width
                    contentHeight: bodyText.implicitHeight
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds

                    // Полоса прокрутки ОТНИМАЕТ ширину у текста, а не лежит
                    // поверх него: так устроен QTextBrowser виджетных окон, и
                    // без этого строки шли на полтора десятка точек длиннее,
                    // переносились в других местах и весь текст съезжал
                    // (замер справки: сдвиг −19 точек на каждой полосе).
                    // Ширина берётся у самой полосы, своего числа нет.
                    ScrollBar.vertical: ScrollBar { id: bodyBar }

                    // README и описание приходят готовой разметкой из ядра —
                    // тем же текстом, что показывал QTextBrowser виджетной
                    // вкладки, только с экранированными угловыми скобками
                    // (починка #1: оригинал терял на них куски текста).
                    Text {
                        id: bodyText
                        width: bodyFlick.width
                                - (bodyBar.visible ? bodyBar.width : 0)
                        text: detailDialog.view ? detailDialog.view.bodyHtml : ""
                        textFormat: Text.RichText
                        wrapMode: Text.WordWrap
                        color: root.colText
                        onLinkActivated: (link) => Qt.openUrlExternally(link)
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                visible: detailDialog.view && detailDialog.view.tagsLine !== ""
                text: detailDialog.view ? detailDialog.view.tagsLine : ""
                wrapMode: Text.WordWrap
                color: root.colInfo
            }

            Text {
                Layout.fillWidth: true
                Layout.topMargin: root.st.dialog.techPaddingV
                Layout.bottomMargin: root.st.dialog.techPaddingV
                text: detailDialog.view ? detailDialog.view.techHtml : ""
                visible: text !== ""
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
                onLinkActivated: (link) => Qt.openUrlExternally(link)
            }

            Text {
                Layout.fillWidth: true
                text: detailDialog.view ? detailDialog.view.pathHtml : ""
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonButton {
                    id: detailOpen
                    pal: root.pal
                    text: page.detailOpenText
                    onClicked: {
                        if (root.actions && detailDialog.view)
                            root.actions.openFolder(detailDialog.view.dir);
                    }
                }

                Item {
                    Layout.fillWidth: true
                }

                RexonButton {
                    id: detailApply
                    readonly property string label:
                        (root.actions && root.actions.applyText && detailDialog.view)
                        ? root.actions.applyText(detailDialog.view.extId) : ""
                    visible: detailApply.label !== ""
                    pal: root.pal
                    text: detailApply.label
                    onClicked: {
                        const extId = detailDialog.view.extId;
                        detailDialog.close();
                        root.runApply(extId);
                    }
                }

                RexonDangerButton {
                    id: detailDelete
                    pal: root.pal
                    tone: root.toneDetailDelete
                    text: page.detailUninstallText
                    onClicked: {
                        const extId = detailDialog.view.extId;
                        detailDialog.close();
                        root.runUninstall(extId, true);
                    }
                }

                RexonButton {
                    id: detailClose
                    pal: root.pal
                    text: page.detailCloseText
                    onClicked: detailDialog.close()
                }
            }
        }

        // Фокус при открытии — на кнопке закрытия: окно обязано быть
        // управляемо с клавиатуры с первого кадра, не только мышью.
        onOpened: detailClose.forceActiveFocus()
    }

    // ── Razer Axon (тест) ─────────────────────────────────────────────────
    //
    // Обои из библиотеки Razer Axon под Wine: найденные сами и из папок
    // WallpaperSource, указанных вручную. Данные и тексты — ядро
    // (`rexon_core::axon_library`) через `actions.axonTexts/axonList`;
    // «Применить» готовит файл и ставит его общим исполнителем обоев, итог
    // приходит тем же `applyFinished`, что у наборов обоев.

    FolderDialog {
        id: axonFolderDialog
        title: axonDialog.texts.pickTitle || ""
        onAccepted: {
            const answer = JSON.parse(root.actions.axonAddFolder(selectedFolder.toString()));
            axonDialog.status = answer.message;
            axonDialog.reload();
        }
    }

    Popup {
        id: axonDialog
        objectName: "axonDialog"

        readonly property var texts: (root.actions && root.actions.axonTexts)
                                     ? JSON.parse(root.actions.axonTexts()) : ({})
        property var lib: ({ items: [], dirs: [], note: "" })
        property string status: ""
        // Подготовка заранее: {line, running, enabled} — строка «Подготовлено:
        // N, осталось: M» и состояние переключателя (ядро, `axon_prepare`).
        property var prep: ({ line: "", running: false, enabled: true })
        property bool confirmRestore: false

        function reload() {
            axonDialog.lib = JSON.parse(root.actions.axonList());
            axonDialog.refreshPrep();
        }

        function refreshPrep() {
            if (root.actions && root.actions.axonPrepareStatus)
                axonDialog.prep = JSON.parse(root.actions.axonPrepareStatus());
        }

        function openLibrary() {
            if (!root.actions || !root.actions.axonList)
                return;
            axonDialog.status = "";
            axonDialog.confirmRestore = false;
            axonDialog.reload();
            axonDialog.open();
        }

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        width: Math.min(root.width - 40, Math.max(page.detailMinWidth, root.width * 0.6))
        height: Math.min(root.height - 40, Math.max(page.detailMinHeight, root.height * 0.8))
        padding: root.st.dialog.margin

        background: Rectangle {
            color: root.pal.bg_primary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        Connections {
            target: root.actions
            function onApplyFinished(message) {
                if (axonDialog.opened) {
                    axonDialog.status = message;
                    axonDialog.refreshPrep();
                }
            }
        }

        // Фоновая подготовка идёт сама — строку итога освежаем, пока окно открыто.
        Timer {
            interval: 3000
            repeat: true
            running: axonDialog.opened
            onTriggered: axonDialog.refreshPrep()
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    text: axonDialog.texts.title || ""
                    color: root.colText
                    font.bold: true
                    font.pointSize: page.headerTitlePointSize
                    elide: Text.ElideRight
                }

                RexonButton {
                    objectName: "axonPick"
                    pal: root.pal
                    text: axonDialog.texts.pick || ""
                    ToolTip.visible: hovered
                    ToolTip.text: axonDialog.texts.pickTooltip || ""
                    onClicked: axonFolderDialog.open()
                }

                RexonButton {
                    objectName: "axonOpenRoot"
                    pal: root.pal
                    enabled: axonDialog.lib.dirs.length > 0
                    text: axonDialog.texts.open || ""
                    ToolTip.visible: hovered
                    ToolTip.text: axonDialog.lib.dirs.length > 0 ? axonDialog.lib.dirs[0] : ""
                    onClicked: root.actions.openFolder(axonDialog.lib.dirs[0])
                }
            }

            Text {
                Layout.fillWidth: true
                visible: text !== ""
                text: axonDialog.lib.note
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonCheckBox {
                    id: axonPrepareAhead
                    objectName: "axonPrepareAhead"
                    pal: root.pal
                    text: axonDialog.texts.prepareAhead || ""
                    checked: axonDialog.prep.enabled
                    ToolTip.visible: hovered
                    ToolTip.text: axonDialog.texts.prepareAheadTooltip || ""
                    onToggled: {
                        root.actions.axonSetPrepareAhead(axonPrepareAhead.checked);
                        axonDialog.refreshPrep();
                    }
                }

                Text {
                    objectName: "axonPrepareLine"
                    Layout.fillWidth: true
                    text: axonDialog.prep.line
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                }

                RexonButton {
                    objectName: "axonRestore"
                    pal: root.pal
                    text: axonDialog.texts.restore || ""
                    onClicked: axonDialog.confirmRestore = true
                }
            }

            // Подтверждение возврата — на месте, без второго окна: честно
            // сказать, что ролики уже расшифрованы.
            Rectangle {
                Layout.fillWidth: true
                visible: axonDialog.confirmRestore
                implicitHeight: axonRestoreBox.implicitHeight + 16
                radius: 4
                color: root.pal.bg_card
                border.color: root.colBorder
                border.width: 1

                ColumnLayout {
                    id: axonRestoreBox
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 8

                    Text {
                        Layout.fillWidth: true
                        text: axonDialog.texts.restoreWarning || ""
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }

                    RowLayout {
                        spacing: 8

                        RexonButton {
                            objectName: "axonRestoreConfirm"
                            pal: root.pal
                            highlighted: true
                            text: axonDialog.texts.restoreConfirm || ""
                            onClicked: {
                                axonDialog.confirmRestore = false;
                                root.actions.axonRestoreOriginals();
                                axonDialog.refreshPrep();
                            }
                        }

                        RexonButton {
                            pal: root.pal
                            text: axonDialog.texts.cancel || ""
                            onClicked: axonDialog.confirmRestore = false
                        }
                    }
                }
            }

            ListView {
                id: axonList
                objectName: "axonList"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 4
                model: axonDialog.lib.items
                ScrollBar.vertical: ScrollBar {}

                delegate: Rectangle {
                    id: axonRow
                    required property var modelData
                    width: axonList.width
                    height: axonRowLayout.implicitHeight + 12
                    radius: 4
                    color: root.pal.bg_card
                    border.color: root.colBorder
                    border.width: 1

                    RowLayout {
                        id: axonRowLayout
                        anchors.fill: parent
                        anchors.margins: 6
                        spacing: 8

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2

                            Text {
                                Layout.fillWidth: true
                                text: axonRow.modelData.title
                                color: root.colText
                                font.bold: true
                                elide: Text.ElideRight
                            }

                            Text {
                                Layout.fillWidth: true
                                text: [axonRow.modelData.author, axonRow.modelData.line]
                                          .filter(s => s !== "").join(" · ")
                                color: root.colTextSecondary
                                elide: Text.ElideRight
                            }
                        }

                        RexonButton {
                            pal: root.pal
                            enabled: axonRow.modelData.folder !== ""
                            text: axonDialog.texts.open || ""
                            ToolTip.visible: hovered
                            ToolTip.text: axonRow.modelData.folder
                            onClicked: root.actions.openFolder(axonRow.modelData.folder)
                        }

                        RexonButton {
                            objectName: "axonApply_" + axonRow.modelData.id
                            pal: root.pal
                            highlighted: true
                            enabled: axonRow.modelData.downloaded
                            text: axonDialog.texts.apply || ""
                            onClicked: axonDialog.status = root.actions.axonApply(axonRow.modelData.id)
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    objectName: "axonStatus"
                    Layout.fillWidth: true
                    text: axonDialog.status
                    wrapMode: Text.WordWrap
                    color: root.colText
                }

                RexonButton {
                    id: axonClose
                    pal: root.pal
                    text: axonDialog.texts.close || ""
                    onClicked: axonDialog.close()
                }
            }
        }

        onOpened: axonClose.forceActiveFocus()
    }

    // ── Справка «Создать своё» ────────────────────────────────────────────

    Popup {
        id: createDialog
        objectName: "createDialog"

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        width: Math.min(root.width - 40,
                        Math.max(page.createMinWidth, root.width * 0.6))
        height: Math.min(root.height - 40,
                         Math.max(page.createMinHeight, root.height * 0.85))
        padding: root.st.dialog.margin

        background: Rectangle {
            color: root.pal.bg_primary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            // Своего шага у справки нет: работает шаг компоновки по
            // умолчанию, и он ýже разборного (6 против 10).
            spacing: root.st.dialog.createSpacing

            Text {
                Layout.fillWidth: true
                text: page.createHeader
                textFormat: Text.RichText
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            // Та же подложка, что у описания разбора: у виджетной справки
            // текст тоже лежит в `QTextBrowser`.
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumHeight: root.st.dialog.bodyMinHeight
                color: root.pal.bg_secondary
                border.color: root.colBorder
                border.width: root.st.dialog.bodyBorderWidth
                radius: root.st.dialog.bodyRadius

                Flickable {
                    id: createFlick
                    anchors.fill: parent
                    anchors.margins: root.st.dialog.bodyPadding
                                     + root.st.dialog.bodyBorderWidth
                    contentWidth: width
                    contentHeight: createText.implicitHeight
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds

                    ScrollBar.vertical: ScrollBar { id: createBar }

                    Text {
                        id: createText
                        width: createFlick.width
                                - (createBar.visible ? createBar.width : 0)
                        text: page.createBody
                        textFormat: Text.RichText
                        wrapMode: Text.WordWrap
                        color: root.colText
                        onLinkActivated: (link) => Qt.openUrlExternally(link)
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true

                Item {
                    Layout.fillWidth: true
                }

                RexonButton {
                    id: createClose
                    pal: root.pal
                    text: page.createCloseText
                    onClicked: createDialog.close()
                }
            }
        }

        onOpened: createClose.forceActiveFocus()
    }

    // ── Фокус с первого кадра ─────────────────────────────────────────────

    // ⚠️ Фокус берёт ПЕРВЫЙ КОНТРОЛ, а не корень страницы. `forceActiveFocus`
    // на корне выглядит правильно, но делает `activeFocusItem` безымянным
    // Item'ом: ни одна кнопка фокуса не получает, и страница мертва и с
    // клавиатуры, и с геймпада.
    // На холодном первом кадре сцена может быть ещё не в окне, и
    // `forceActiveFocus()` уходит в пустоту: замер поймал прогон, где фокус
    // не взялся вовсе и вкладка осталась мёртвой и с клавиатуры, и с
    // геймпада. Поэтому попытка повторяется на следующем проходе цикла
    // событий — так же, как у «Сравнения DE» и «Рабочих столов», — но не
    // бесконечно.
    property int focusAttempts: 0

    function focusFirstControl() {
        gridButton.forceActiveFocus();
        if (gridButton.activeFocus) {
            root.focusAttempts = 0;
            return;
        }
        if (root.focusAttempts < 20) {
            root.focusAttempts += 1;
            Qt.callLater(root.focusFirstControl);
        } else {
            console.warn("вкладка расширений: фокус взять не удалось");
        }
    }

    Component.onCompleted: root.focusFirstControl()

    // Во встроенной вкладке страница создаётся заранее и оживает при
    // переключении на неё — фокус надо взять и там, иначе первый Tab после
    // смены вкладки уходит в никуда.
    onVisibleChanged: {
        if (!root.visible)
            return;
        // Расширение могло встать и мимо этой вкладки («Установить из
        // файла…» в «Оформлении») — список перечитываем при каждом показе,
        // иначе новое не видно до перезапуска.
        page.refresh();
        root.focusFirstControl();
    }
}
