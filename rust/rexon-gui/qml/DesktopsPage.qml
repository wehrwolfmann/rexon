// Вкладка «Рабочие столы» на QML. Данные и решения — Rust-объект
// DesktopsModel (rust/rexon-gui/src/desktops_page.rs), который сам ничего не
// решает, а спрашивает ядро (`rexon_core::qt_desktops`). Здесь только
// РАЗМЕТКА.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из `ui_base::skin_colors("steam_dark")` — скина по
// умолчанию всего продукта (правило #17). Своих цветов страница не заводит.
//
// ⛔ Побочные действия этой вкладки СТАВЯТ И УДАЛЯЮТ рабочие столы через
// pacman и pkexec. Ни одного такого шага страница не делает сама: она зовёт
// объект `root.actions`, который кладёт хозяин окна
// (rexon/qt_desktops/window.py). Там же живут подтверждения, обе защиты
// удаления и окна хода дела — то есть ровно то, что было в виджетной
// вкладке. Страница знает только имена методов и «получилось ли».
//
// Цепочка фокуса заложена сразу: все действия — контролы, у сфокусированного
// видна рамка, страница подкручивается к нему.

// Делегаты обращаются к id внешних элементов (root, page). Без этой прагмы
// проверка QML справедливо ругается на неквалифицированный доступ.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    // Объект побочных действий; ставит хозяин страницы. Методы:
    //   install(deId) -> bool      установка стола (подтверждение + ход дела)
    //   repair(deId) -> bool       проверка и восстановление
    //   switchTo(deId) -> bool     переключение сессии
    //   remove(deId) -> bool       удаление с обеими защитами
    //   repairCurrent() -> bool    «проверить мой стол»
    //   panelBackup() / panelRestore() -> bool
    //   openCompare()              показать встроенное «Сравнение DE»
    //   requestVersion(deId)       асинхронный запрос версии из репозиториев
    //   shellInstall / shellSwitch / shellRemove(deId, shellId)
    //                              блок «Оболочки» (Noctalia, DMS)
    //   lookApply / lookRevert(lookId)
    //                              блок «Как в дистрибутиве»
    // Сигнал versionReady(deId, version) — ответ на requestVersion.
    property var actions: null

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight
    // Подпись кнопки возврата из встроенного «Сравнения DE»: рисует её
    // хозяин страницы (сравнение — отдельное окно, внутрь сцены не встаёт),
    // но текст всё равно приходит из ядра, а не второй копией на Python.
    readonly property string compareBackText: page.compareBackText
    // Выбранный стол и стол сеанса — для прогона `--selftest` главного окна:
    // вкладка обязана открываться на ТЕКУЩЕМ столе, а не на первом в списке.
    readonly property string selectedDe: page.selectedDe
    readonly property string currentDe: page.currentDe

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    DesktopsModel {
        id: page
        // Имя нужно проверкам: из хозяина страницы модель достают
        // `root.findChild(QObject, "desktopsModel")`.
        // ⚠️ Снаружи зовутся только инвокаблы БЕЗ чисел в подписи
        // (`refreshInstalled`, `select`, `removePlan`, `displayName`): у `i32`
        // в метаобъекте стоит имя `::std::int32_t`, метатипа с таким именем
        // нет, и `QMetaObject.invokeMethod` молча вернёт False.
        objectName: "desktopsModel"
    }

    // ── Палитра стандартного скина ────────────────────────────────────────

    readonly property var pal: page.palette
    // Размеры разметки — поля, кегли, стороны кнопок — той же картой от ядра.
    readonly property var m: page.metrics
    readonly property color colBg: root.pal.bg_primary
    readonly property color colCard: root.pal.bg_card
    readonly property color colCardHover: root.pal.bg_card_hover
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // ── Состояние вида ────────────────────────────────────────────────────

    readonly property var card: page.card
    readonly property var carouselTexts: page.carousel

    // Текущий слайд карусели. Сбрасывается при смене стола: иначе у стола с
    // одним снимком остался бы индекс от предыдущего, и карусель показала бы
    // пустоту.
    property int slide: 0
    // Версия пакета из репозиториев для показанного стола: "" — ещё не
    // спрашивали либо пакета нет, тогда строка скрыта.
    property string versionText: ""
    property bool versionPending: false
    // Блок «пакеты» по ответу пробной транзакции: "" — ответа ещё нет (или
    // спросить не вышло), тогда видна строка карточки.
    property string packagesLine: ""
    property string packagesNotice: ""
    // Набор не встанет из-за конфликта с установленным: «Установить» и
    // «Проверить / Восстановить» видны, но неактивны (причина — в
    // `packagesNotice` прямо над ними).
    property bool packagesBlocked: false
    // Пакет набора ждёт пересборки против новой библиотеки — его можно
    // собрать из исходников Arch: подпись кнопки и пояснение ("" — нечего).
    property string rebuildButton: ""
    property string rebuildHint: ""

    onCardChanged: {
        root.slide = 0;
        root.versionText = "";
        root.versionPending = false;
        root.packagesLine = "";
        root.packagesNotice = "";
        root.packagesBlocked = false;
        root.rebuildButton = "";
        root.rebuildHint = "";
        root.requestVersion();
        root.requestPackages();
    }

    // ── Побочные действия ─────────────────────────────────────────────────

    // Все они СИНХРОННЫ и возвращают «получилось ли»: подтверждение, ход
    // дела и итог показывает хозяин, а страница по ответу решает,
    // пересчитывать ли состав установленных столов.
    function runAction(kind, deId) {
        if (!root.actions)
            return;
        let changed = false;
        if (kind === "install")
            changed = root.actions.install(deId);
        else if (kind === "repair")
            changed = root.actions.repair(deId);
        else if (kind === "switch")
            changed = root.actions.switchTo(deId);
        else if (kind === "remove")
            changed = root.actions.remove(deId);
        if (changed)
            page.refreshInstalled();
    }

    // Блок «Оболочки»: установить, войти с оболочкой, убрать. Ход и итог —
    // общим окном операции; состав столов пересчитывает сигнал
    // `desktopsChanged`.
    function runShellAction(kind, deId, shellId) {
        if (!root.actions)
            return;
        if (kind === "shell_install")
            root.actions.shellInstall(deId, shellId);
        else if (kind === "shell_switch")
            root.actions.shellSwitch(deId, shellId);
        else if (kind === "shell_remove")
            root.actions.shellRemove(deId, shellId);
    }

    // Блок «Как в дистрибутиве»: «Применить» и «Вернуть как было». Вопрос,
    // ход и итог — общим окном операции; плитки перечитывает сигнал
    // `desktopsChanged` (→ `refreshInstalled`).
    function runLook(kind, lookId) {
        if (!root.actions)
            return;
        if (kind === "look_apply")
            root.actions.lookApply(lookId);
        else if (kind === "look_revert")
            root.actions.lookRevert(lookId);
    }

    function runTool(kind) {
        if (!root.actions)
            return;
        if (kind === "compare")
            root.actions.openCompare();
        else if (kind === "repair_current") {
            if (root.actions.repairCurrent())
                page.refreshInstalled();
        } else if (kind === "panel_backup")
            root.actions.panelBackup();
        else if (kind === "panel_restore")
            root.actions.panelRestore();
    }

    // Версия пакета из репозиториев дистрибутива запрашивается асинхронно:
    // `pacman -Si` занимает секунды, и синхронный вызов остановил бы вкладку.
    // Строка нужна только там, где есть кнопка «Установить».
    function requestVersion() {
        if (!root.actions || !root.card || root.card.deId === "")
            return;
        if (!root.card.available)
            return;
        // Запрос уже в пути — второй раз пакетный менеджер не дёргаем:
        // сюда приходят и со сменой карточки, и с появлением объекта
        // действий, а это может случиться в любом порядке.
        if (root.versionPending)
            return;
        root.versionPending = true;
        // У установленного стола — его версия (и, если в репозитории
        // новее, — она же рядом); у неустановленного — что придёт.
        if (root.card.installed)
            root.actions.requestInstalledVersion(root.card.deId);
        else
            root.actions.requestVersion(root.card.deId);
    }

    // Сколько пакетов РЕАЛЬНО поставится и не сломан ли набор — пробной
    // транзакцией в фоне; ответ `packagesReady`.
    function requestPackages() {
        if (!root.actions || !root.card || root.card.deId === "" || !root.card.packagesProbe)
            return;
        root.actions.requestPackages(root.card.deId, root.card.installed);
    }

    // Объект действий кладёт хозяин страницы, и делает это уже ПОСЛЕ того,
    // как страница собралась: в `Component.onCompleted` его ещё нет, там
    // `root.actions` равен null. Без этого обработчика запрос версии из
    // репозиториев уходил в пустоту — строка «Доступно в твоём дистрибутиве»
    // не появлялась НИКОГДА, сколько её ни жди. Тот же приём, что на
    // странице настроек: всё, что требует хозяина, спрашивается здесь.
    onActionsChanged: {
        root.requestVersion();
        root.requestPackages();
    }

    Connections {
        target: root.actions

        function onVersionReady(deId, version) {
            if (!root.card || deId !== root.card.deId)
                return;
            if (root.card.installed)
                return;
            root.versionPending = false;
            root.versionText = page.versionLine(version);
        }

        function onInstalledVersionReady(deId, line) {
            if (!root.card || deId !== root.card.deId || !root.card.installed)
                return;
            root.versionPending = false;
            root.versionText = line;
        }

        function onPackagesReady(deId, installed, text, notice, blocked) {
            if (!root.card || deId !== root.card.deId || installed !== root.card.installed)
                return;
            root.packagesLine = text;
            root.packagesNotice = notice;
            root.packagesBlocked = blocked;
        }

        function onRebuildOfferReady(deId, installed, button, hint) {
            if (!root.card || deId !== root.card.deId || installed !== root.card.installed)
                return;
            root.rebuildButton = button;
            root.rebuildHint = hint;
        }
    }

    // Подкрутить карточку к элементу, который получил фокус: без этого Tab
    // уводит фокус за нижний край, и вкладка выглядит зависшей.
    function ensureVisible(item) {
        if (!item || !cardFlick.contentItem)
            return;
        const pos = item.mapToItem(cardFlick.contentItem, 0, 0);
        const maxY = Math.max(0, cardFlick.contentHeight - cardFlick.height);
        if (pos.y < cardFlick.contentY)
            cardFlick.contentY = Math.max(0, pos.y - 12);
        else if (pos.y + item.height > cardFlick.contentY + cardFlick.height)
            cardFlick.contentY = Math.min(maxY,
                                          pos.y + item.height - cardFlick.height + 12);
    }

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── Три панели: список, «Как в дистрибутиве», карточка ────────────────

    SplitView {
        id: split
        anchors.fill: parent
        orientation: Qt.Horizontal

        // Рукоятка: ширина и краска — из ядра. Своей рукоятки стиль QtQuick
        // Basic не красит по палитре приложения и рисует светло-серую
        // полосу (198,200,201) во всю высоту вкладки; у оригинала на том же
        // месте почти незаметная накладка (73,83,96 на стандартном скине).
        handle: Rectangle {
            implicitWidth: root.m.splitHandleWidth
            implicitHeight: root.m.splitHandleWidth
            color: root.pal.split_handle
        }

        // ── Левая панель: список рабочих столов ───────────────────────────
        Item {
            SplitView.preferredWidth: root.m.listInitialWidth
            SplitView.minimumWidth: root.m.listMinWidth
            SplitView.maximumWidth: root.m.listMaxWidth

            ColumnLayout {
                anchors.fill: parent
                anchors.leftMargin: root.m.listMarginLeft
                anchors.rightMargin: root.m.listMarginRight
                anchors.topMargin: root.m.listMarginTop
                anchors.bottomMargin: root.m.listMarginBottom
                spacing: root.m.listSpacing

                Text {
                    Layout.fillWidth: true
                    text: page.listHeader
                    color: root.colText
                    font.pointSize: page.listHeaderPointSize
                    font.bold: true
                }

                ListView {
                    id: deList
                    // Имя нужно стенду приёмки: заказанный стол он выбирает
                    // сменой `currentIndex` — тем же путём, что стрелка с
                    // клавиатуры.
                    objectName: "deList"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: page
                    // Между пунктами оригинала зазора нет: поля пункта
                    // (`padding: 8px 12px`) и есть весь его воздух.
                    spacing: 0
                    boundsBehavior: Flickable.StopAtBounds
                    // Фокус ведёт сам список: кнопка ТЕКУЩЕЙ строки берёт
                    // его через ListView.isCurrentItem. Так снаружи никто не
                    // лезет в потроха делегата — а именно оттуда фокус и
                    // терялся молча при любой правке разметки.
                    focus: true
                    currentIndex: -1
                    keyNavigationEnabled: true

                    ScrollBar.vertical: ScrollBar {}

                    // Куда шли в прошлый раз — по этому перешагиваем
                    // заголовок секции в ТУ ЖЕ сторону.
                    property int lastRow: -1

                    // Строка списка и показанная карточка — одно целое, как в
                    // виджетной вкладке: там `itemSelectionChanged` перерисовывал
                    // карточку сразу на стрелку, и человеку не приходилось после
                    // каждого шага ещё и подтверждать выбор. Плюс заголовок
                    // секции виден, но не выбирается (`NoItemFlags` у оригинала),
                    // значит и фокус на нём стоять не должен — иначе с клавиатуры
                    // и с геймпада получается «мёртвый» шаг без подсветки.
                    onCurrentIndexChanged: {
                        if (deList.currentIndex < 0)
                            return;
                        if (page.roleValue(deList.currentIndex, "selectable") !== true) {
                            const dir = deList.currentIndex > deList.lastRow ? 1 : -1;
                            const next = deList.currentIndex + dir;
                            deList.currentIndex = (next >= 0 && next < deList.count)
                                                  ? next : deList.lastRow;
                            return;
                        }
                        deList.lastRow = deList.currentIndex;
                        page.select(page.roleValue(deList.currentIndex, "deId"));
                    }

                    delegate: Item {
                        id: listRow

                        required property int index
                        required property string kind
                        required property string deId
                        required property string text
                        required property string tooltip
                        required property bool selectable

                        width: ListView.view.width
                        // Обе строки — с полями пункта оригинала: у заголовка
                        // секции те же `padding: 8px 12px`, что у стола.
                        implicitHeight: listRow.selectable
                                        ? deButton.implicitHeight
                                        : sectionLabel.implicitHeight
                                          + 2 * root.m.listItemPaddingV

                        // Заголовок секции: виден, но не выбирается — в
                        // оригинале у него стоял `NoItemFlags`, а красился он
                        // ролью PlaceholderText палитры скина.
                        Text {
                            id: sectionLabel
                            visible: !listRow.selectable
                            anchors.left: parent.left
                            anchors.leftMargin: root.m.listItemPaddingH
                            anchors.verticalCenter: parent.verticalCenter
                            text: listRow.text
                            color: root.pal.placeholder_text
                            font.pointSize: page.sectionPointSize
                            font.bold: true
                        }

                        Button {
                            id: deButton
                            visible: listRow.selectable
                            anchors.fill: parent
                            flat: true
                            text: listRow.text

                            readonly property bool current: page.selectedDe === listRow.deId

                            // Фокус приходит от списка вместе с «текущестью».
                            focus: listRow.ListView.isCurrentItem

                            // Поля пункта — свои у кнопки, а не у текста: так
                            // высота строки складывается ровно как у
                            // оригинала (текст плюс `padding: 8px 12px`).
                            padding: 0
                            topPadding: root.m.listItemPaddingV
                            bottomPadding: root.m.listItemPaddingV
                            leftPadding: root.m.listItemPaddingH
                            rightPadding: root.m.listItemPaddingH

                            background: Rectangle {
                                radius: root.m.listItemRadius
                                // Краски списка — из ядра: синяя подложка
                                // выбранного и лёгкая под курсором.
                                color: deButton.current
                                       ? root.pal.list_selected
                                       : (deButton.hovered || deButton.down
                                          ? root.pal.list_hover : "transparent")
                                border.color: deButton.activeFocus ? root.colAccent
                                                                   : "transparent"
                                border.width: 1
                            }

                            contentItem: Text {
                                text: deButton.text
                                color: deButton.current ? root.pal.list_selected_text
                                                        : root.colText
                                font.pointSize: page.listItemPointSize
                                font.weight: root.pal.text_secondary_weight
                                elide: Text.ElideRight
                                horizontalAlignment: Text.AlignLeft
                                verticalAlignment: Text.AlignVCenter
                            }

                            ToolTip.visible: deButton.hovered && listRow.tooltip !== ""
                            ToolTip.text: listRow.tooltip

                            onClicked: {
                                deList.currentIndex = listRow.index;
                                page.select(listRow.deId);
                            }
                        }
                    }
                }
            }
        }

        // ── Вторая панель: «Как в дистрибутиве» и «Инструменты» ───────────
        // Решение владельца 25.10.2026: на месте прежних «Инструментов» —
        // образы целиком «как в дистрибутиве», по плитке на образ. Плитки,
        // пометку применённого и кнопки собирает ядро
        // (`qt_desktops::look_tiles`), ход «Применить» и «Вернуть как было» —
        // общее окно операции.
        // Решение владельца 28.10.2026: «Инструменты» вернулись в эту же
        // панель — СПРАВА от «Как в дистрибутиве», кнопки столбиком, как до
        // 25.10; полосы внизу карточки больше нет. На узкой панели (окно
        // 1280) два блока рядом не помещаются, и «Инструменты» встают ПОД
        // «Как в дистрибутиве» тем же столбиком — решает ядро
        // (`qt_desktops::looks_tools_side_by_side`).
        Rectangle {
            id: middlePanel
            // Ширина — делёж лишнего места окна между панелями, как у
            // сплиттера оригинала (растяжка 0:1:4 с потолком карточки):
            // считает ядро. Иначе QML отдавал всё лишнее карточке, и на окне
            // 1400 рамка скриншота выходила на 40 точек шире, чем у оригинала.
            SplitView.preferredWidth: page.looksPanelWidth(split.width, root.height)
            // ⚠️ НАИМЕНЬШАЯ ширина панели идёт от её СОДЕРЖИМОГО, а не от
            // одного числа: на штатном окне 1100×750 сплиттер даёт панели
            // ровно 160 точек, и заголовок обрезался бы краем соседней
            // карточки посреди слова. На узкой панели блоки стоят друг под
            // другом, поэтому пол — по более широкому из двух. Ширину надписей
            // меряет по шрифту Qt, пол, потолок и арифметику держит ядро.
            SplitView.minimumWidth: page.looksPanelMinWidth(
                Math.max(looksHeaderText.implicitWidth, toolsHeaderText.implicitWidth),
                Math.max(looksColumn.implicitWidth, toolColumn.implicitWidth))
            // Лёгкий фон и правая граница: без них растянутая панель читается
            // как пустая дыра между списком и карточкой.
            color: root.pal.tools_bg

            Rectangle {
                anchors.right: parent.right
                width: 1
                height: parent.height
                color: root.pal.tools_border
            }

            Flickable {
                id: looksFlick
                anchors.fill: parent
                anchors.leftMargin: root.m.toolsMarginLeft
                anchors.rightMargin: root.m.toolsMarginRight
                anchors.topMargin: root.m.toolsMarginTop
                anchors.bottomMargin: root.m.toolsMarginBottom
                contentWidth: width
                contentHeight: middleBody.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                ScrollBar.vertical: ScrollBar {}

                Item {
                    id: middleBody
                    objectName: "middleBody"

                    // Рядом или столбиком и ширина блока — ядро. Просят блоки
                    // свою НЕсжатую ширину (заголовок и колонка), от ширины
                    // самого блока она не зависит — петли раскладки нет.
                    readonly property bool sideBySide: page.looksToolsSideBySide(
                        looksFlick.width,
                        Math.max(looksHeaderText.implicitWidth, looksColumn.implicitWidth),
                        Math.max(toolsHeaderText.implicitWidth, toolColumn.implicitWidth))
                    readonly property int blockWidth: page.looksToolsBlockWidth(
                        looksFlick.width, middleBody.sideBySide)
                    readonly property int gap: root.m.looksToolsGap

                    width: looksFlick.width
                    implicitHeight: middleBody.sideBySide
                                    ? Math.max(looksBody.implicitHeight, toolsBody.implicitHeight)
                                    : looksBody.implicitHeight + middleBody.gap
                                      + toolsBody.implicitHeight

                    // ── «Как в дистрибутиве» ──────────────────────────────
                    ColumnLayout {
                        id: looksBody
                        x: 0
                        y: 0
                        width: middleBody.blockWidth
                        spacing: root.m.toolsSpacing

                        Text {
                            id: looksHeaderText
                            Layout.fillWidth: true
                            text: page.looksHeader
                            color: root.colText
                            font.pointSize: page.listHeaderPointSize
                            font.bold: true
                        }

                        // Пояснение по ширине блока (в колонку плиток не
                        // кладём: длинная строка растянула бы её по своей
                        // ширине).
                        Text {
                            Layout.fillWidth: true
                            Layout.preferredWidth: looksBody.width
                            text: page.looksHint
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.packagesPointSize
                        }

                        // Колонка плиток: по самой длинной подписи, с потолком
                        // ядра и НИКОГДА не шире самого блока.
                        ColumnLayout {
                            id: looksColumn
                            Layout.fillWidth: true
                            Layout.maximumWidth: looksBody.width
                            spacing: root.m.toolsSpacing

                            Repeater {
                                model: page.looks

                                delegate: Rectangle {
                                    id: lookTile
                                    required property var modelData
                                    objectName: "lookTile_" + lookTile.modelData.id
                                    Layout.fillWidth: true
                                    implicitWidth: tileBody.implicitWidth
                                                   + 2 * root.m.shellsPanelPadding
                                    implicitHeight: tileBody.implicitHeight
                                                    + 2 * root.m.shellsPanelPadding
                                    radius: root.m.carouselFrameRadius
                                    color: root.colCard
                                    // Применённый образ — в рамке цвета пометки.
                                    border.color: lookTile.modelData.applied
                                                  ? root.pal.current_badge : root.pal.tools_border
                                    border.width: 1

                                    // Описание образа из манифеста — подсказкой:
                                    // на плитку оно не помещается.
                                    HoverHandler {
                                        id: tileHover
                                    }
                                    ToolTip.visible: tileHover.hovered
                                                     && lookTile.modelData.description !== ""
                                    ToolTip.text: lookTile.modelData.description
                                    ToolTip.delay: 500

                                    ColumnLayout {
                                        id: tileBody
                                        anchors.left: parent.left
                                        anchors.right: parent.right
                                        anchors.top: parent.top
                                        anchors.margins: root.m.shellsPanelPadding
                                        spacing: root.m.statSpacing * 3

                                        Text {
                                            Layout.fillWidth: true
                                            text: lookTile.modelData.name
                                            color: root.colText
                                            font.bold: true
                                            wrapMode: Text.WordWrap
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            visible: lookTile.modelData.badge !== ""
                                            text: lookTile.modelData.badge
                                            color: root.pal.current_badge
                                            font.weight: Font.DemiBold
                                        }

                                        Repeater {
                                            model: lookTile.modelData.buttons

                                            delegate: RexonButton {
                                                id: lookButton
                                                required property var modelData
                                                Layout.fillWidth: true
                                                implicitHeight: page.toolButtonMinHeight
                                                pal: root.pal
                                                tone: lookButton.modelData.style === "primary"
                                                      ? root.pal.primary_tone : root.pal.neutral_tone
                                                text: lookButton.modelData.text
                                                ToolTip.visible: lookButton.hovered
                                                                 && lookButton.modelData.tooltip !== ""
                                                ToolTip.text: lookButton.modelData.tooltip
                                                onClicked: root.runLook(lookButton.modelData.kind,
                                                                        lookTile.modelData.id)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Граница между блоками — посередине промежутка:
                    // вертикальная, когда блоки рядом, горизонтальная, когда
                    // «Инструменты» ушли вниз.
                    Rectangle {
                        x: middleBody.sideBySide
                           ? middleBody.blockWidth + Math.floor(middleBody.gap / 2) : 0
                        y: middleBody.sideBySide
                           ? 0 : looksBody.implicitHeight + Math.floor(middleBody.gap / 2)
                        width: middleBody.sideBySide ? 1 : middleBody.width
                        height: middleBody.sideBySide
                                ? Math.max(looksFlick.height, middleBody.implicitHeight) : 1
                        color: root.pal.tools_border
                    }

                    // ── «Инструменты»: столбик кнопок ─────────────────────
                    // Кнопки и почему часть неактивна — ядро
                    // (`qt_desktops::tool_buttons`). Вид — как до 25.10.
                    ColumnLayout {
                        id: toolsBody
                        objectName: "toolsBlock"
                        x: middleBody.sideBySide ? middleBody.blockWidth + middleBody.gap : 0
                        y: middleBody.sideBySide ? 0 : looksBody.implicitHeight + middleBody.gap
                        width: middleBody.blockWidth
                        spacing: root.m.toolsSpacing

                        Text {
                            id: toolsHeaderText
                            Layout.fillWidth: true
                            text: page.toolsHeader
                            color: root.colText
                            font.pointSize: page.listHeaderPointSize
                            font.bold: true
                        }

                        // Колонка кнопок шириной по САМОЙ ДЛИННОЙ подписи и с
                        // потолком ядра, прижата влево и на весь блок не
                        // растягивается. ⚠️ И НИКОГДА ШИРЕ САМОГО БЛОКА: если
                        // человек сам стянет разделитель, кнопки останутся
                        // внутри панели.
                        ColumnLayout {
                            id: toolColumn
                            Layout.alignment: Qt.AlignLeft
                            Layout.fillWidth: false
                            Layout.maximumWidth: Math.min(root.m.toolButtonMaxWidth,
                                                          toolsBody.width)
                            spacing: root.m.toolsSpacing

                            Repeater {
                                model: page.toolButtons

                                delegate: RexonButton {
                                    id: toolButton
                                    required property var modelData
                                    Layout.fillWidth: true
                                    implicitHeight: page.toolButtonMinHeight
                                    pal: root.pal
                                    text: toolButton.modelData.text
                                    // SteamOS: пакетная кнопка видна, но
                                    // неактивна — причина строкой ниже
                                    // (`toolsNotice`).
                                    enabled: toolButton.modelData.enabled !== false
                                    ToolTip.visible: toolButton.hovered
                                    ToolTip.text: toolButton.modelData.tooltip
                                    onClicked: root.runTool(toolButton.modelData.kind)
                                }
                            }
                        }

                        // Почему часть кнопок неактивна — одной фразой по
                        // ширине блока (в колонку кнопок не кладём: длинная
                        // строка растянула бы её по своей ширине).
                        Text {
                            Layout.fillWidth: true
                            Layout.preferredWidth: toolsBody.width
                            visible: page.toolsNotice !== ""
                            text: page.toolsNotice
                            wrapMode: Text.WordWrap
                            color: root.pal.status_unavailable
                        }
                    }
                }
            }
        }

        // ── Правая панель: карточка выбранного стола ──────────────────────
        Item {
            SplitView.fillWidth: true
            // Потолок ширины — от высоты окна, как у сплиттера оригинала:
            // дальше лишняя ширина уходит средней панели («Как в
            // дистрибутиве» и «Инструменты»), а не в пустоту вокруг карточки.
            SplitView.maximumWidth: page.detailPanelMaxWidth(root.height)

            // Приглашение выбрать стол — когда выбранного нет вовсе.
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.verticalCenter: cardFlick.verticalCenter
                visible: !root.card || root.card.deId === ""
                text: page.placeholderText
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
                font.pointSize: root.m.placeholderPointSize
            }

            Flickable {
                id: cardFlick
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                // Карточка — во всю высоту панели (с 28.10.2026 полосы
                // «Инструменты» под ней нет); рамка снимка ужимается под
                // подвал окна (`carouselHeightFitted`).
                anchors.bottom: parent.bottom
                visible: root.card && root.card.deId !== ""
                contentWidth: width
                contentHeight: cardColumn.implicitHeight + root.m.cardMarginTop
                               + root.m.cardMarginBottom
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                // Полоса прокрутки видна всегда: её появление меняло бы
                // ширину области, а от ширины зависит высота рамки скриншота
                // (16:9) — получалась дрожащая петля.
                ScrollBar.vertical: ScrollBar {
                    id: cardScroll
                    policy: ScrollBar.AlwaysOn
                }

                // ⚠️ Раскладка внутри обычного Item обязана получить высоту
                // руками, иначе схлопывается в полоску.
                ColumnLayout {
                    id: cardColumn

                    // Полоса прокрутки у оригинала стоит рядом с карточкой, а
                    // не поверх неё — её ширина вычитается из вьюпорта.
                    readonly property int viewport: cardFlick.width - cardScroll.width
                    // Карточка прижата к ПРАВОМУ краю панели: у оригинала её
                    // обёртка делит ширину между распоркой и карточкой 100:1,
                    // и слева остаётся сотая доля вьюпорта. Считает ядро.
                    readonly property int gapLeft: page.cardLeftSpacer(viewport)
                                                   + page.cardGapLeft

                    x: cardColumn.gapLeft
                    y: root.m.cardMarginTop
                    width: Math.min(cardColumn.viewport - cardColumn.gapLeft
                                    - root.m.cardMarginRight,
                                    page.cardMaxWidth)
                    height: implicitHeight
                    spacing: root.m.cardSpacing

                    // ── Карусель скриншотов ───────────────────────────────
                    Item {
                        id: carousel
                        Layout.fillWidth: true
                        // Рамка всегда 16:9 и не прыгает по слайдам: по её
                        // левому краю выравнивается текст карточки.
                        implicitHeight: frame.height + dots.implicitHeight
                                        + root.m.carouselDotsSpacing

                        readonly property var images: root.card && root.card.images
                                                      ? root.card.images : []
                        readonly property int count: carousel.images.length
                        readonly property string source: carousel.count > 0
                            ? carousel.images[Math.min(root.slide, carousel.count - 1)]
                            : ""

                        function step(direction) {
                            if (carousel.count === 0)
                                return;
                            root.slide = (root.slide + direction + carousel.count)
                                         % carousel.count;
                        }

                        Rectangle {
                            id: frame
                            anchors.right: parent.right
                            // Рамка 16:9 от своей высоты плюс боковые поля
                            // картинки; высота — от ширины БЕЗ этих полей,
                            // как в оригинале (`self.width() - 16`).
                            readonly property int sidePadding: 2 * root.m.carouselFramePaddingH
                            // Высота рамки на окне, которому высоты хватает.
                            // От НЕЁ считается ширина — и только от неё, см.
                            // предупреждение про «храповик» у
                            // `carousel_height_fitted` в ядре.
                            readonly property int fullHeight:
                                page.carouselHeight(root.height,
                                                    parent.width - frame.sidePadding)
                            // Сколько высоты остаётся рамке после всего
                            // остального в карточке: полей, промежутка,
                            // точек-индикаторов и текстовой колонки. Меряется
                            // ВСЁ, кроме самой рамки, — иначе вышло бы
                            // круговое правило.
                            readonly property int room:
                                cardFlick.height - root.m.cardMarginTop
                                - root.m.cardMarginBottom - root.m.cardSpacing
                                - root.m.carouselDotsSpacing - dots.implicitHeight
                                - cardBody.implicitHeight
                            width: Math.min(parent.width,
                                            frame.fullHeight * 16 / 9 + frame.sidePadding)
                            // ⭐ Рамка отдаёт высоту карточке, когда окну её
                            // не хватает: на окне наименьшего размера подвал
                            // состояния срезал нижний ряд кнопок — теперь
                            // вместо этого снимок становится чуть мельче.
                            // Сколько отдать и где предел — решает ядро.
                            height: page.carouselHeightFitted(frame.fullHeight, frame.room)
                            color: root.pal.carousel_frame
                            radius: root.m.carouselFrameRadius
                            clip: true

                            // Поля вокруг снимка, у которого стороны не 16:9
                            // (Pantheon — почти 4:3): та же картинка, сжатая
                            // до горстки точек и растянутая обратно, — это
                            // и есть размытие, без модуля эффектов (его нет
                            // в Qt старше 6.5), — и приглушённая краской
                            // рамки, чтобы не спорить со снимком.
                            Image {
                                id: shotBackdrop
                                anchors.fill: shot
                                visible: shot.visible && !shot.fits
                                         && shot.status === Image.Ready
                                source: carousel.source
                                sourceSize: Qt.size(32, 18)
                                fillMode: Image.PreserveAspectCrop
                                smooth: true
                                asynchronous: true
                                cache: true

                                Rectangle {
                                    anchors.fill: parent
                                    color: root.pal.carousel_frame
                                    opacity: 0.55
                                }
                            }

                            Image {
                                id: shot
                                anchors.fill: parent
                                // Поля только по бокам: сверху и снизу у
                                // рамки оригинала их нет.
                                anchors.leftMargin: root.m.carouselFramePaddingH
                                anchors.rightMargin: root.m.carouselFramePaddingH
                                visible: carousel.source !== ""
                                source: carousel.source
                                // ⚠️ Снимок показывается ЦЕЛИКОМ: обрезка
                                // под рамку 16:9 срезала у снимков 4:3 и
                                // панель сверху, и док снизу — на карточке
                                // оставался «просто пейзаж» вместо стола
                                // (Pantheon, 08.10). Поля закрывает
                                // размытая копия под снимком.
                                // Маленький исходник (заглушка-SVG) НЕ
                                // растягивается — оригинал показывает его
                                // как есть, по центру.
                                readonly property bool fits: shot.sourceSize.width > 0
                                    && shot.sourceSize.width <= shot.width
                                    && shot.sourceSize.height <= shot.height
                                fillMode: shot.fits ? Image.Pad : Image.PreserveAspectFit
                                horizontalAlignment: Image.AlignHCenter
                                verticalAlignment: Image.AlignVCenter
                                asynchronous: true
                                cache: true

                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: zoom.open()
                                    ToolTip.visible: containsMouse
                                    ToolTip.text: root.carouselTexts.zoomTooltip
                                    hoverEnabled: true
                                }
                            }

                            Text {
                                anchors.centerIn: parent
                                visible: carousel.count === 0
                                text: root.carouselTexts.emptyText
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                font.pointSize: root.m.noScreenshotsPointSize
                            }

                            // Стрелки — оверлеем поверх краёв картинки, а не
                            // слотами раскладки: желоба отодвигали бы
                            // скриншот от края рамки. Круглые и тёмные, как
                            // в оригинале: тон и сторона — из ядра.
                            RexonButton {
                                id: prevBtn
                                visible: carousel.count > 1
                                anchors.left: parent.left
                                anchors.leftMargin: root.m.overlayMargin
                                anchors.verticalCenter: parent.verticalCenter
                                width: root.m.arrowSize
                                implicitHeight: root.m.arrowSize
                                padding: 0
                                pal: root.pal
                                tone: root.pal.arrow_tone
                                font.pointSize: root.m.arrowPointSize
                                text: "❮"
                                ToolTip.visible: prevBtn.hovered
                                ToolTip.text: root.carouselTexts.prevTooltip
                                onClicked: carousel.step(-1)
                            }

                            RexonButton {
                                id: nextBtn
                                visible: carousel.count > 1
                                anchors.right: parent.right
                                anchors.rightMargin: root.m.overlayMargin
                                anchors.verticalCenter: parent.verticalCenter
                                width: root.m.arrowSize
                                implicitHeight: root.m.arrowSize
                                padding: 0
                                pal: root.pal
                                tone: root.pal.arrow_tone
                                font.pointSize: root.m.arrowPointSize
                                text: "❯"
                                ToolTip.visible: nextBtn.hovered
                                ToolTip.text: root.carouselTexts.nextTooltip
                                onClicked: carousel.step(1)
                            }

                            RexonButton {
                                id: zoomBtn
                                visible: carousel.count > 0
                                anchors.right: parent.right
                                anchors.top: parent.top
                                anchors.margins: root.m.overlayMargin
                                width: root.m.zoomButtonSize
                                implicitHeight: root.m.zoomButtonSize
                                padding: 0
                                pal: root.pal
                                tone: root.pal.zoom_button_tone
                                font.pointSize: root.m.zoomButtonPointSize
                                text: "🔍"
                                ToolTip.visible: zoomBtn.hovered
                                ToolTip.text: root.carouselTexts.zoomButtonTooltip
                                onClicked: zoom.open()
                            }
                        }

                        // Точки-индикаторы по центру колонки со скриншотом:
                        // приглушённый текст оригинала — тот же вторичный
                        // цвет и вес, без своей прозрачности.
                        Text {
                            id: dots
                            anchors.top: frame.bottom
                            anchors.topMargin: root.m.carouselDotsSpacing
                            anchors.horizontalCenter: frame.horizontalCenter
                            text: page.carouselDots(root.slide, carousel.count)
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                        }
                    }

                    // ── Под снимком: текст стола слева, «Оболочки» справа ─
                    // Блок «Оболочки» есть только у Hyprland, niri и sway
                    // (решение владельца 01.10.2026); у остальных столов
                    // его нет, и текстовая колонка занимает всю ширину, как
                    // раньше. Справа блок стоит, когда карточка достаточно
                    // широкая; на узком окне он уходит под текст стола (с
                    // какой ширины — решает ядро).
                    GridLayout {
                        id: cardBody
                        readonly property bool sideBySide:
                            cardBody.width >= root.m.shellsSideBySideMinWidth
                        Layout.alignment: Qt.AlignRight
                        Layout.preferredWidth: frame.width
                        Layout.maximumWidth: frame.width
                        columns: cardBody.sideBySide ? 2 : 1
                        columnSpacing: root.m.cardSpacing
                        rowSpacing: root.m.cardSpacing

                    // ── Текстовая колонка ─────────────────────────────────
                    ColumnLayout {
                        id: textColumn
                        // Ширину отдаёт строка: всё, что осталось от блока
                        // «Оболочки». Без нулевой желаемой ширины колонка
                        // просила бы ширину своей самой длинной строки, и
                        // блок справа уезжал за край карточки.
                        Layout.fillWidth: true
                        Layout.preferredWidth: 0
                        Layout.minimumWidth: 0
                        Layout.alignment: Qt.AlignTop
                        spacing: root.m.cardSpacing

                        // Заголовок и статус
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: root.m.headerRowSpacing

                            Text {
                                Layout.fillWidth: true
                                text: root.card.title
                                color: root.colText
                                font.pointSize: page.cardTitlePointSize
                                font.bold: true
                                elide: Text.ElideRight
                            }

                            Text {
                                text: root.card.statusText
                                color: root.card.statusColor === ""
                                       ? root.colTextSecondary : root.card.statusColor
                                // Некрашеный статус — вторичный текст, а тот у
                                // оригинала набран весом 600.
                                font.weight: root.card.statusBold
                                             ? Font.Bold : root.pal.text_secondary_weight
                                font.pointSize: page.cardStatusPointSize
                            }
                        }

                        // Описание
                        Text {
                            Layout.fillWidth: true
                            text: root.card.description
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.descPointSize
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 1
                            color: root.pal.separator
                        }

                        // ТТХ-полоса: память, нагрузка, настройка, игры
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            Repeater {
                                model: root.card.stats

                                delegate: ColumnLayout {
                                    id: statCell
                                    required property var modelData
                                    Layout.fillWidth: true
                                    // Колонки РАВНОЙ ширины, как у растяжки
                                    // 1:1:1:1 в оригинале: без этого ширина
                                    // делилась по длине подписи, и «RAM»
                                    // уезжал на 16 точек влево.
                                    Layout.preferredWidth: 1
                                    spacing: root.m.statSpacing

                                    Text {
                                        Layout.fillWidth: true
                                        text: statCell.modelData.icon
                                        color: root.colText
                                        horizontalAlignment: Text.AlignHCenter
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        // Пустое значение держит место, но не
                                        // видно: колонки иначе разъезжаются
                                        // по высоте.
                                        text: statCell.modelData.value === ""
                                              ? "·" : statCell.modelData.value
                                        opacity: statCell.modelData.value === "" ? 0 : 1
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        horizontalAlignment: Text.AlignHCenter
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: statCell.modelData.label
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        font.pointSize: root.m.statLabelPointSize
                                        horizontalAlignment: Text.AlignHCenter
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 1
                            color: root.pal.separator
                        }

                        // Сколько пакетов поставится и откуда; у
                        // установленного — всё ли на месте.
                        Text {
                            Layout.fillWidth: true
                            text: root.packagesLine !== "" ? root.packagesLine
                                                           : root.card.packagesText
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.packagesPointSize
                        }

                        // Что из набора сейчас не ставится (ждёт пересборки
                        // в репозитории) — до нажатия «Установить», а не
                        // после пароля в журнале pacman.
                        Text {
                            Layout.fillWidth: true
                            visible: root.packagesNotice !== ""
                            text: root.packagesNotice
                            wrapMode: Text.WordWrap
                            color: root.pal.status_unavailable
                            font.pointSize: root.m.packagesPointSize
                        }

                        // Такой пакет можно собрать из официального рецепта
                        // Arch прямо здесь — кнопка с пояснением под
                        // предупреждением.
                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.rebuildButton !== ""
                            spacing: root.m.buttonRowSpacing

                            RexonButton {
                                id: rebuildButton
                                pal: root.pal
                                tone: root.pal.neutral_tone
                                topPadding: root.m.cardButtonPaddingV
                                bottomPadding: root.m.cardButtonPaddingV
                                leftPadding: root.m.cardButtonPaddingH
                                rightPadding: root.m.cardButtonPaddingH
                                text: root.rebuildButton
                                onClicked: if (root.actions)
                                    root.actions.rebuildFromSources(root.card.deId)
                                onActiveFocusChanged: if (activeFocus)
                                    root.ensureVisible(rebuildButton)
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.rebuildHint
                                wrapMode: Text.WordWrap
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                font.pointSize: root.m.packagesPointSize
                            }
                        }

                        // ── Кнопки действий ──────────────────────────────
                        RowLayout {
                            Layout.fillWidth: true
                            Layout.topMargin: root.m.buttonRowTopMargin
                            spacing: root.m.buttonRowSpacing

                            // Стол недоступен на этом дистрибутиве — вместо
                            // кнопок объяснение.
                            Text {
                                visible: !root.card.available
                                text: root.card.notice
                                color: root.pal.status_unavailable
                            }

                            // Это текущий стол: ни «Переключиться» (мы уже в
                            // нём), ни «Удалить» (остались бы без графики).
                            Text {
                                id: currentBadge
                                visible: root.card.isCurrent && root.card.available
                                text: root.card.currentBadgeText
                                color: root.pal.current_badge
                                font.weight: Font.DemiBold
                                ToolTip.visible: badgeHover.hovered
                                ToolTip.text: root.card.currentBadgeTooltip

                                HoverHandler {
                                    id: badgeHover
                                }
                            }

                            // Установка и переключение — синяя, проверка —
                            // нейтральная с рамкой, удаление — красная. Все
                            // три тона и поля приходят из ядра; кнопка одна
                            // (общая `RexonButton`), меняется только тон.
                            Repeater {
                                model: root.card.buttons

                                delegate: RexonButton {
                                    id: actionButton
                                    required property var modelData
                                    pal: root.pal
                                    tone: actionButton.modelData.style === "primary"
                                          ? root.pal.primary_tone
                                          : (actionButton.modelData.style === "destructive"
                                             ? root.pal.destructive_tone
                                             : root.pal.neutral_tone)
                                    topPadding: root.m.cardButtonPaddingV
                                    bottomPadding: root.m.cardButtonPaddingV
                                    leftPadding: root.m.cardButtonPaddingH
                                    rightPadding: root.m.cardButtonPaddingH
                                    text: actionButton.modelData.text
                                    // SteamOS: кнопка видна, но неактивна;
                                    // причина — строкой под кнопками.
                                    // Конфликт набора с установленным: то же
                                    // для кнопок, ставящих набор, причина —
                                    // ⛔-строкой над кнопками.
                                    enabled: actionButton.modelData.enabled !== false
                                             && !(root.packagesBlocked
                                                  && actionButton.modelData.installsSet === true)
                                    ToolTip.visible: actionButton.hovered
                                                     && actionButton.modelData.tooltip !== ""
                                    ToolTip.text: actionButton.modelData.tooltip
                                    onClicked: root.runAction(actionButton.modelData.kind,
                                                              root.card.deId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(actionButton)
                                }
                            }

                            Item {
                                Layout.fillWidth: true
                            }
                        }
                        // Версия, которая РЕАЛЬНО поставится из
                        // репозиториев этого дистрибутива. Пока идёт
                        // запрос — мягкий плейсхолдер, при отсутствии
                        // пакета строка тихо исчезает.
                        // Отдельной строкой под кнопками: рядом с тремя
                        // кнопками ей оставалось место на полтора слова
                        // («Установлена верс…», приёмка 10.10.2026).
                        Text {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WordWrap
                            maximumLineCount: 2
                            elide: Text.ElideRight
                            visible: root.versionPending || root.versionText !== ""
                            text: root.versionPending
                                  ? root.card.versionChecking : root.versionText
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.versionPointSize
                        }

                        // SteamOS (Steam Deck и Steam Machine): почему кнопки
                        // выше неактивны — одной фразой из ядра.
                        Text {
                            Layout.fillWidth: true
                            visible: (root.card.blockedNotice || "") !== ""
                            text: root.card.blockedNotice || ""
                            wrapMode: Text.WordWrap
                            color: root.pal.status_unavailable
                        }
                    }

                    // ── Блок «Оболочки» ───────────────────────────────────
                    // Надстройка над столом: у каждой оболочки своё
                    // «Установить», а после установки — «Переключиться» (вход
                    // в этот же стол с оболочкой) и «Убрать» (стол остаётся).
                    // Подписи, состав кнопок и почему кнопка неактивна —
                    // ядро (`qt_desktops::shell_rows`).
                    Rectangle {
                        id: shellsPanel
                        readonly property var rows: root.card && root.card.shells
                                                    ? root.card.shells : []
                        visible: shellsPanel.rows.length > 0
                        Layout.alignment: Qt.AlignTop
                        Layout.fillWidth: !cardBody.sideBySide
                        Layout.preferredWidth: cardBody.sideBySide
                                               ? root.m.shellsPanelWidth : -1
                        Layout.maximumWidth: cardBody.sideBySide
                                             ? root.m.shellsPanelWidth : Number.POSITIVE_INFINITY
                        implicitHeight: shellsColumn.implicitHeight
                                        + 2 * root.m.shellsPanelPadding
                        color: root.pal.tools_bg
                        border.color: root.pal.tools_border
                        radius: root.m.carouselFrameRadius

                        ColumnLayout {
                            id: shellsColumn
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: root.m.shellsPanelPadding
                            spacing: root.m.shellsRowSpacing

                            Text {
                                id: shellsHeaderText
                                Layout.fillWidth: true
                                text: root.card.shellsHeader || ""
                                color: root.colText
                                font.bold: true
                                font.pointSize: root.m.shellsHeaderPointSize
                                ToolTip.visible: shellsHeaderHover.hovered
                                ToolTip.text: root.card.shellsTooltip || ""

                                HoverHandler {
                                    id: shellsHeaderHover
                                }
                            }

                            Repeater {
                                model: shellsPanel.rows

                                delegate: ColumnLayout {
                                    id: shellRow
                                    required property var modelData
                                    required property int index
                                    Layout.fillWidth: true
                                    spacing: root.m.statSpacing * 2

                                    Rectangle {
                                        Layout.fillWidth: true
                                        implicitHeight: 1
                                        color: root.pal.separator
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true

                                        Text {
                                            Layout.fillWidth: true
                                            text: shellRow.modelData.name
                                            color: root.colText
                                            font.bold: true
                                            elide: Text.ElideRight
                                        }

                                        Text {
                                            visible: shellRow.modelData.statusText !== ""
                                            text: shellRow.modelData.statusText
                                            color: shellRow.modelData.running
                                                   ? root.pal.current_badge
                                                   : root.pal.status_installed
                                            font.weight: Font.DemiBold
                                        }
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: shellRow.modelData.description
                                        wrapMode: Text.WordWrap
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        font.pointSize: root.m.packagesPointSize
                                    }

                                    Flow {
                                        Layout.fillWidth: true
                                        spacing: root.m.buttonRowSpacing

                                        Repeater {
                                            model: shellRow.modelData.buttons

                                            delegate: RexonButton {
                                                id: shellButton
                                                required property var modelData
                                                pal: root.pal
                                                tone: shellButton.modelData.style === "primary"
                                                      ? root.pal.primary_tone
                                                      : (shellButton.modelData.style === "destructive"
                                                         ? root.pal.destructive_tone
                                                         : root.pal.neutral_tone)
                                                topPadding: root.m.cardButtonPaddingV
                                                bottomPadding: root.m.cardButtonPaddingV
                                                leftPadding: root.m.cardButtonPaddingH
                                                rightPadding: root.m.cardButtonPaddingH
                                                text: shellButton.modelData.text
                                                enabled: shellButton.modelData.enabled !== false
                                                ToolTip.visible: shellButton.hovered
                                                                 && shellButton.modelData.tooltip !== ""
                                                ToolTip.text: shellButton.modelData.tooltip
                                                onClicked: root.runShellAction(
                                                               shellButton.modelData.kind,
                                                               root.card.deId,
                                                               shellRow.modelData.shell)
                                                onActiveFocusChanged: if (activeFocus)
                                                    root.ensureVisible(shellButton)
                                            }
                                        }
                                    }

                                    // Почему кнопка неактивна — словами, а не
                                    // только подсказкой под курсором (геймпад).
                                    Text {
                                        Layout.fillWidth: true
                                        visible: shellRow.modelData.hint !== ""
                                        text: shellRow.modelData.hint
                                        wrapMode: Text.WordWrap
                                        color: root.pal.status_unavailable
                                        font.pointSize: root.m.packagesPointSize
                                    }
                                }
                            }
                        }
                    }
                    }
                }
            }
        }
    }

    // ── Увеличение скриншота ──────────────────────────────────────────────

    // ⭐ ОТДЕЛЬНОЕ ОКНО ВО ВЕСЬ ЭКРАН, а не всплывашка внутри вкладки.
    //
    // Раньше здесь стоял `Popup` размером во вкладку, и «увеличить во весь
    // экран» оказывалось неправдой: поверх снимка оставались полоса разделов
    // сверху и подвал состояния снизу, а поля вокруг были полями страницы.
    // Оправдание «сцена живёт внутри QQuickWidget» к делу не относится:
    // `Window` в QML — САМОСТОЯТЕЛЬНОЕ окно верхнего уровня, границами
    // сцены-хозяина оно не ограничено и одинаково открывается и из окна на
    // Rust, и из вкладки внутри QQuickWidget. У виджетного оригинала это
    // `QDialog` без рамки, модальный, с `showFullScreen()` в `showEvent`
    // (`rexon/qt_desktops/window.py`) — здесь ровно те же три свойства.
    //
    // Выход возвращает окно как было: своих размеров главному окну просмотр
    // не менял (он в нём не жил) и раскладку вкладки не трогал — закрытие
    // просто прячет окно и возвращает управление тому, кто его открыл.
    //
    // `transientParent: null` — намеренно. По умолчанию вложенное `Window`
    // становится подчинённым окну сцены, а у хозяина-QQuickWidget настоящего
    // окна нет вовсе: привязка к нему сделала бы просмотр неоткрываемым на
    // одном из двух входов.
    Window {
        id: zoom

        transientParent: null
        flags: Qt.Window | Qt.FramelessWindowHint
        modality: Qt.ApplicationModal
        // Заголовок — из ядра, как у оригинала (`setWindowTitle`). Рамки у
        // окна нет, но заголовок видят переключатель окон и панель задач.
        title: root.carouselTexts.zoomTitle
        // Фон просмотра — из ядра (`rgba(0,0,0,0.95)` у оригинала).
        color: root.pal.zoom_bg

        /// Открыть просмотр во весь экран. Имя `open()` оставлено прежним:
        /// его зовут и щелчок по снимку, и кнопка-лупа под ним.
        /// Фокус внутри окна здесь НЕ берётся: его берёт обработчик показа
        /// внизу, один на все пути появления окна.
        ///
        /// ⚠️ Размер экрана проставляется РУКАМИ, до просьбы о полном
        /// экране, и это не перестраховка ради красоты. `showFullScreen()` —
        /// именно просьба: её исполняет оконный менеджер, и там, где его нет
        /// или где он её не понял, окно осталось бы размером со своё
        /// содержимое. Замер на стенде без оконного менеджера: просмотр
        /// вышел окошком 160×150 в левом верхнем углу — то есть «во весь
        /// экран» опять оказалось бы неправдой, только по другой причине.
        /// С этими двумя строками окно занимает экран в любом случае, а
        /// полноэкранный режим сверху снимает ещё и панели композитора.
        function open() {
            zoom.x = 0;
            zoom.y = 0;
            zoom.width = Screen.width;
            zoom.height = Screen.height;
            zoom.showFullScreen();
            zoom.raise();
            zoom.requestActivate();
        }

        /// Закрыть просмотр. Точка выхода ОДНА, и зовут её все четыре пути:
        /// щелчок по снимку, крестик, Esc и Q.
        function hideZoom() {
            zoom.close();
        }

        Item {
            id: zoomBody

            anchors.fill: parent

            // Клавиши слушает САМО ТЕЛО окна: `Keys` — свойство элемента, а
            // окно элементом не является. И фокус на первом кадре берёт
            // тело, а не кнопка «×»: пока фокус стоял на кнопке, пробел её
            // НАЖИМАЛ (просмотр закрывался) вместо листания, хотя у
            // виджетного просмотра пробел листал вперёд.
            focus: true

            // Набор — как у виджетного просмотра (`keyPressEvent`):
            // ←/A назад, →/D/пробел вперёд, Esc/Q закрыть.
            //
            // ⚠️ Esc назван здесь ПОИМЁННО. У `Popup` его закрывал
            // `closePolicy`, у окна такого свойства нет вовсе: пока строки
            // ниже не было, просмотр на весь экран не закрывался клавишей
            // выхода — а других выходов из окна без рамки у человека ровно
            // два, и оба надо найти мышью.
            Keys.onPressed: (event) => {
                switch (event.key) {
                case Qt.Key_Left:
                case Qt.Key_A:
                    carousel.step(-1);
                    event.accepted = true;
                    break;
                case Qt.Key_Right:
                case Qt.Key_D:
                case Qt.Key_Space:
                    carousel.step(1);
                    event.accepted = true;
                    break;
                case Qt.Key_Escape:
                case Qt.Key_Q:
                    zoom.hideZoom();
                    event.accepted = true;
                    break;
                }
            }

            Image {
                anchors.fill: parent
                anchors.margins: root.m.zoomMargin
                anchors.bottomMargin: root.m.zoomArrowSize
                source: carousel.source
                fillMode: Image.PreserveAspectFit
                asynchronous: true

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: zoom.hideZoom()
                }
            }

            RexonButton {
                id: zoomPrev
                visible: carousel.count > 1
                anchors.left: parent.left
                anchors.leftMargin: root.m.zoomMargin
                anchors.verticalCenter: parent.verticalCenter
                width: root.m.zoomArrowSize
                implicitHeight: root.m.zoomArrowSize
                padding: 0
                pal: root.pal
                tone: root.pal.zoom_arrow_tone
                font.pointSize: root.m.zoomArrowPointSize
                text: "❮"
                ToolTip.visible: zoomPrev.hovered
                ToolTip.text: root.carouselTexts.zoomPrevTooltip
                onClicked: carousel.step(-1)
            }

            RexonButton {
                id: zoomNext
                visible: carousel.count > 1
                anchors.right: parent.right
                anchors.rightMargin: root.m.zoomMargin
                anchors.verticalCenter: parent.verticalCenter
                width: root.m.zoomArrowSize
                implicitHeight: root.m.zoomArrowSize
                padding: 0
                pal: root.pal
                tone: root.pal.zoom_arrow_tone
                font.pointSize: root.m.zoomArrowPointSize
                text: "❯"
                ToolTip.visible: zoomNext.hovered
                ToolTip.text: root.carouselTexts.zoomNextTooltip
                onClicked: carousel.step(1)
            }

            RexonButton {
                id: zoomClose
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: root.m.zoomMargin
                width: root.m.zoomArrowSize
                implicitHeight: root.m.zoomArrowSize
                padding: 0
                pal: root.pal
                tone: root.pal.zoom_arrow_tone
                font.pointSize: root.m.zoomClosePointSize
                text: "×"
                ToolTip.visible: zoomClose.hovered
                ToolTip.text: root.carouselTexts.zoomCloseTooltip
                onClicked: zoom.hideZoom()
            }

            Text {
                anchors.bottom: parent.bottom
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottomMargin: root.m.zoomHintPadding
                text: page.zoomHint(root.slide, carousel.count)
                color: root.pal.zoom_hint
            }
        }

        // Просмотр обязан быть управляем с клавиатуры с первого кадра, а
        // страница — с первого кадра ПОСЛЕ выхода.
        //
        // Фокус внутри просмотра берёт ТЕЛО окна (`zoomBody`): оно слушает
        // клавиши листания, а Tab с него уводит на «❮ / ❯ / ×», если человек
        // хочет кнопки. Показ окна не всегда идёт через `open()` — его
        // показывает и сам Qt, восстанавливая окно, — поэтому фокус берётся
        // на СОБЫТИИ показа, а не только в функции.
        //
        // ⚠️ Возврат фокуса странице — не мелочь. Пока его не было, после
        // закрытия просмотра клавиатура уходила в никуда: ←/→ не листали
        // список столов, Tab не ходил по кнопкам, и вкладка выглядела
        // зависшей, хотя мышью работала.
        //
        // Возвращается ИМЕННО фокус, а не выбор: `focusFirstControl()` здесь
        // звать нельзя — она перекидывает выбор на первый стол списка, и
        // выход из просмотра менял бы показанную карточку. Выбранная строка
        // остаётся той же, какой была до увеличения.
        onVisibleChanged: {
            if (zoom.visible)
                zoomBody.forceActiveFocus();
            else
                deList.forceActiveFocus();
        }
    }

    // ── Окно операции над столом ──────────────────────────────────────────
    //
    // Общее для «Рабочих столов» и «Сравнения DE»: вопрос или отказ → ход
    // дела → итог (`DeOperationDialog.qml`).

    DeOperationDialog {
        anchors.fill: parent
        actions: root.actions
        pal: root.pal
        onDesktopsChanged: page.refreshInstalled()
    }

    // ── Фокус с первого кадра ─────────────────────────────────────────────

    // ⚠️ Фокус берёт ПЕРВЫЙ УПРАВЛЯЕМЫЙ ЭЛЕМЕНТ, а не корень страницы:
    // `forceActiveFocus()` на корне делает `activeFocusItem` безымянным
    // Item'ом, и страница мертва и с клавиатуры, и с геймпада.
    property int focusAttempts: 0

    function focusFirstControl() {
        // Первой строкой идёт заголовок секции: он виден, но не выбирается,
        // и фокус берёт первый ВЫБИРАЕМЫЙ пункт.
        // Строка стола, который уже выбран моделью: при открытии это текущий
        // рабочий стол, после возврата на вкладку — тот, на котором человек
        // остановился. Прежде фокус всегда уходил на первую строку, и
        // вкладка открывалась на Cinnamon (приёмка 10.10.2026).
        const wanted = page.selectedDe;
        if (wanted !== "") {
            for (let i = 0; i < deList.count; i++) {
                if (page.roleValue(i, "selectable") === true
                        && page.roleValue(i, "deId") === wanted) {
                    root.focusAttempts = 0;
                    deList.currentIndex = i;
                    deList.forceActiveFocus();
                    return;
                }
            }
        }
        for (let i = 0; i < deList.count; i++) {
            // Заголовок секции виден, но не выбирается — фокус берёт первый
            // ВЫБИРАЕМЫЙ пункт. Спрашиваем модель, а не разметку: состав
            // секций решает ядро, и разметка о нём знать не должна.
            if (page.roleValue(i, "selectable") === true) {
                root.focusAttempts = 0;
                deList.currentIndex = i;
                deList.forceActiveFocus();
                return;
            }
        }
        // Список наполняется не мгновенно; пробуем на следующем проходе
        // цикла событий, но не бесконечно.
        if (root.focusAttempts < 20) {
            root.focusAttempts += 1;
            Qt.callLater(root.focusFirstControl);
        } else {
            console.warn("вкладка рабочих столов: список пуст, фокус взять не с чего");
        }
    }

    Component.onCompleted: {
        root.focusFirstControl();
        root.requestVersion();
    }

    // Во встроенной вкладке страница создаётся заранее и оживает при
    // переключении на неё — фокус надо взять и там.
    onVisibleChanged: if (root.visible) root.focusFirstControl()
}
