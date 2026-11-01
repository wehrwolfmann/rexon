// Скрытая страница «Разработчик» вкладки «Wallpaper Engine» — РАЗМЕТКА.
//
// Порт `DeveloperPageWidget` из `rexon/qt_wallpaper_engine/window.py`.
// Данные, решения и ВСЕ подписи — Rust-объект `WeDeveloperModel`
// (rust/rexon-gui/src/we_dev_page.rs) поверх `rexon_core::we_window`.
// Здесь нет ни одной строки для человека и ни одного числа раскладки:
// подписи приезжают картой `texts`, числа — картой `metrics`.
//
// ⚠️ Страница НАША, а не вендорная: она лежит в статическом ресурсе модуля
// `org.rexon.gui` рядом с шеллом вкладки, а не в каталоге waywallen. Пункт
// рейла «Разраб.» так и помечен в ядре (`RailItem.vendor == false`), иначе
// шелл искал бы её файл среди страниц waywallen.
//
// ⚠️ Панели секций у оригинала БЕЗ фона и рамки. Их `QFrame` носит
// `objectName="wePane"`, но таблицы стилей с таким селектором в продукте
// нет ни одной (проверено `grep` по дереву 06.09) — то есть на экране это
// просто отступы. Своего фона здесь не заводим: это была бы отсебятина, а
// мерило страницы — паритет с виджетной версией.
//
// ⚠️ Три вещи страница делает НЕ сама:
//   * версию живого демона называет синглтон шины `DaemonDBusClient` —
//     Rust-модель в Qt не ходит и спросить его не может;
//   * установку плагина через демона ведёт `PluginInstallQuery` — общий
//     запрос вкладки, второго способа говорить с демоном заводить незачем;
//   * сторож ответа демона — обычный `Timer`, и номер попытки он несёт с
//     собой: старый сторож не должен обрывать НОВУЮ установку (починка 2
//     в `rexon_core::we_window`).

// Делегаты Repeater'а обращаются к id внешних элементов.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Наружу ────────────────────────────────────────────────────────────

    /// Расхождение паритета: шелл вкладки вешает по нему пометку на кнопку
    /// «Разраб.» в рейле — как это делал сигнал `parity_drift_changed`
    /// виджетной страницы.
    signal parityDrift(bool drift)

    // ── Ядро страницы ─────────────────────────────────────────────────────

    WeDeveloperModel {
        id: page

        // Имя нужно проверкам: объект достают
        // `root.findChild(QObject, "weDeveloperModel")` и спрашивают, не
        // трогая разметку.
        objectName: "weDeveloperModel"

        // Архив плагина проверен и ждёт установки ЧЕРЕЗ демона.
        onOweIpcRequested: (zipPath, attempt) => {
            oweIpc.zipPath = zipPath;
            oweIpc.reload();
            oweIpcGuard.attempt = attempt;
            oweIpcGuard.restart();
        }
    }

    readonly property var txt: page.texts
    readonly property var m: page.metrics

    // ── Палитра действующего скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colText: root.pal.text_primary
    readonly property color colBorder: root.pal.border
    // Приглушённый текст: та же роль текста с долей прозрачности из ядра —
    // так его красит виджетная версия (`muted_qss` поверх роли `Text`).
    readonly property color colTextMuted: Qt.rgba(root.colText.r, root.colText.g,
                                                  root.colText.b, page.variantAlpha)

    /// Краска строки статуса: пустая краска у ядра означает «обычный текст».
    function statusColor(color) {
        return color === "" ? root.colText : color;
    }

    // ── Версия живого демона ──────────────────────────────────────────────
    //
    // Её знает только Qt-слой (D-Bus), поэтому спрашиваем синглтон вкладки.
    // Оригинал звал `self._daemon.probe()` на каждой сверке паритета; здесь
    // опрос идёт один раз при открытии страницы, а дальше свойство ведёт
    // сам синглтон — он подписан на события шины.
    readonly property string daemonVersion: DaemonDBusClient.daemonVersion

    // ── Сверка паритета UI ────────────────────────────────────────────────

    WeParityQuery {
        id: parity

        objectName: "weParityQuery"
        daemonVersion: root.daemonVersion
        onChecked: (drift) => root.parityDrift(drift)
    }

    // ── Установка OWE-плагина через демона (hot-install) ──────────────────

    PluginInstallQuery {
        id: oweIpc

        onInstalled: (pluginId, needsRestart) => {
            oweIpcGuard.stop();
            page.oweIpcInstalled(pluginId, needsRestart);
        }
        onErrorOccurred: (error) => {
            oweIpcGuard.stop();
            page.oweIpcRefused(error);
        }
    }

    // Сторож ответа демона. Помнит СВОЮ попытку: ответ на первую установку
    // приходит быстро, а сторож живёт ещё до минуты — и следующая
    // установка, начатая в этом окне, получила бы чужой будильник.
    Timer {
        id: oweIpcGuard

        property int attempt: 0

        interval: root.m.oweIpcTimeoutMs
        repeat: false
        onTriggered: page.oweIpcTimeout(oweIpcGuard.attempt)
    }

    // ── Очередь событий фоновых задач ─────────────────────────────────────
    //
    // Задачи (сеть, распаковка, подпроцессы) живут в своём рантайме и
    // складывают исходы в очередь; страница вычерпывает её из своего
    // потока. Тот же приём и тот же период, что у вкладки «Steam».
    Timer {
        interval: 120
        repeat: true
        running: true
        onTriggered: page.pump()
    }

    // Отложенный старт: подпись версии и, если стоит галка, авто-проверка
    // обновлений и авто-сверка паритета. Задержку называет ядро — та же,
    // с какой стартует детект демона у самой вкладки.
    Timer {
        interval: root.m.startupDelayMs
        repeat: false
        running: true
        onTriggered: {
            // Разовый опрос шины: без него синглтон отдаёт пустую версию,
            // пока по шине ничего не случилось.
            DaemonDBusClient.refreshWsPort();
            page.refreshVersion(root.daemonVersion);
            if (page.autoUpstream)
                page.startCheck(root.daemonVersion);
            if (page.autoParity)
                parity.reloadIfConnected();
        }
    }

    Component.onDestruction: page.shutdown()

    // ── Разметка ──────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    ScrollView {
        anchors.fill: parent
        anchors.margins: root.m.pageMargin
        contentWidth: availableWidth
        // Горизонтальной полосы у оригинала нет вовсе (`ScrollBarAlwaysOff`).
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        ColumnLayout {
            width: parent.width
            spacing: root.m.columnSpacing

            // ── Заголовок страницы ────────────────────────────────────────
            Text {
                Layout.fillWidth: true
                text: root.txt.title
                color: root.colText
                font.pixelSize: root.m.titleFontSize
                font.weight: root.m.titleFontWeight
            }

            Text {
                Layout.fillWidth: true
                text: root.txt.subtitle
                color: root.colTextMuted
                font.pixelSize: root.m.mutedFontSize
                wrapMode: Text.WordWrap
            }

            // ══ Секция «Обновления upstream» ══════════════════════════════
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: root.m.panePadding
                Layout.rightMargin: root.m.panePadding
                Layout.topMargin: root.m.panePadding
                Layout.bottomMargin: root.m.panePadding
                spacing: root.m.paneSpacing

                Text {
                    Layout.fillWidth: true
                    text: root.txt.sectionUpstream
                    color: root.colText
                    font.pixelSize: root.m.sectionFontSize
                    font.weight: root.m.sectionFontWeight
                }

                RexonCheckBox {
                    Layout.fillWidth: true
                    objectName: "devAutoUpstreamCheck"
                    pal: root.pal
                    text: root.txt.cbAutoUpstream
                    checked: page.autoUpstream
                    onToggled: page.autoUpstream = checked
                }

                RexonCheckBox {
                    Layout.fillWidth: true
                    objectName: "devAutoDownloadCheck"
                    pal: root.pal
                    text: root.txt.cbAutoDownload
                    checked: page.autoDownload
                    onToggled: page.autoDownload = checked
                }

                // Подпись версии движка — одна на всю жизнь поля (починка 3
                // ядра: раньше поле называлось двумя разными именами).
                Text {
                    Layout.fillWidth: true
                    objectName: "devVersionLabel"
                    text: page.versionLabel
                    color: root.colText
                    font.pixelSize: root.m.statusFontSize
                    wrapMode: Text.WordWrap
                }

                // Строка статуса на каждую цель: waywallen (её ставим) и
                // open-wallpaper-engine (о ней отчитываемся). Состав целей
                // называет ядро, разметка их только рисует.
                Repeater {
                    model: page.targets

                    delegate: TextEdit {
                        required property var modelData

                        Layout.fillWidth: true
                        // Текст выделяется мышью — как у оригинала
                        // (`TextSelectableByMouse`): строку статуса
                        // копируют в отчёты.
                        readOnly: true
                        selectByMouse: true
                        text: modelData.text
                        color: root.statusColor(modelData.color)
                        font.pixelSize: root.m.statusFontSize
                        font.bold: modelData.bold
                        wrapMode: Text.WordWrap
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: root.m.rowSpacing

                    RexonButton {
                        objectName: "devCheckButton"
                        pal: root.pal
                        text: root.txt.btnCheck
                        enabled: page.checkEnabled
                        onClicked: page.startCheck(root.daemonVersion)
                    }

                    RexonButton {
                        objectName: "devUpdateButton"
                        pal: root.pal
                        text: root.txt.btnUpdate
                        enabled: page.updateEnabled
                        onClicked: root.ask(root.txt.confirmUpdateTitle,
                                            page.confirmUpdateText(),
                                            () => page.startEngineInstall())
                    }

                    // OWE-плагин: hot-install через демона (waywallen ≥
                    // 0.2.3) либо распаковка в каталог плагинов, если демон
                    // не запущен. Развилку решает ядро.
                    RexonButton {
                        objectName: "devOweButton"
                        pal: root.pal
                        text: root.txt.btnOwe
                        enabled: page.oweEnabled
                        onClicked: root.ask(root.txt.confirmOweTitle,
                                            page.confirmOweText(),
                                            () => page.startOweInstall(root.daemonVersion))
                    }

                    // «Изучено» — для случая «прочитал changelog, ставить
                    // не буду»: фиксирует базовую версию следующего отчёта.
                    RexonButton {
                        objectName: "devSeenButton"
                        pal: root.pal
                        text: root.txt.btnSeen
                        enabled: page.seenEnabled
                        onClicked: page.markOweSeen()
                    }

                    Item { Layout.fillWidth: true }
                }

                Text {
                    Layout.fillWidth: true
                    objectName: "devUpdateStatus"
                    text: page.updateStatus
                    color: root.colTextMuted
                    font.pixelSize: root.m.mutedFontSize
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: root.m.rowSpacing

                    Text {
                        text: root.txt.labelChangelog
                        color: root.colText
                        font.pixelSize: root.m.statusFontSize
                        font.weight: root.m.changelogLabelWeight
                    }

                    RexonButton {
                        objectName: "devCopyButton"
                        pal: root.pal
                        text: root.txt.btnCopy
                        enabled: page.copyEnabled
                        onClicked: page.copyChangelog()
                    }

                    Item { Layout.fillWidth: true }
                }

                // Панель changelog диапазона «наша версия → последняя».
                ScrollView {
                    Layout.fillWidth: true
                    Layout.minimumHeight: root.m.changelogMinHeight
                    Layout.maximumHeight: root.m.changelogMaxHeight
                    Layout.preferredHeight: root.m.changelogMinHeight

                    TextArea {
                        objectName: "devChangelogView"
                        readOnly: true
                        selectByMouse: true
                        text: page.changelog
                        placeholderText: root.txt.placeholderChangelog
                        placeholderTextColor: root.colTextMuted
                        color: root.colText
                        font.family: "monospace"
                        font.pixelSize: root.m.monoFontSize
                        wrapMode: TextEdit.NoWrap

                        // ⚠️ Подложка панели ЕСТЬ и она темнее страницы. У
                        // виджетной версии её рисует сам `QPlainTextEdit`
                        // ролью `Base`, а прозрачная панель QML показывала
                        // фон страницы: замер стенда 06.09 — сплошная
                        // область 232 910 точек, (22,32,45) против
                        // (27,40,56). Краска берётся из скина
                        // (`bg_secondary`), своей здесь нет.
                        background: Rectangle {
                            color: root.pal.bg_secondary
                            border.color: root.colBorder
                            border.width: 1
                        }
                    }
                }
            }

            // ══ Секция «Отчёт о паритете UI» ══════════════════════════════
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: root.m.panePadding
                Layout.rightMargin: root.m.panePadding
                Layout.topMargin: root.m.panePadding
                Layout.bottomMargin: root.m.panePadding
                spacing: root.m.paneSpacing

                Text {
                    Layout.fillWidth: true
                    text: root.txt.sectionParity
                    color: root.colText
                    font.pixelSize: root.m.sectionFontSize
                    font.weight: root.m.sectionFontWeight
                }

                RexonCheckBox {
                    Layout.fillWidth: true
                    objectName: "devAutoParityCheck"
                    pal: root.pal
                    text: root.txt.cbAutoParity
                    checked: page.autoParity
                    onToggled: page.autoParity = checked
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: root.m.rowSpacing

                    RexonButton {
                        objectName: "devParityButton"
                        pal: root.pal
                        text: root.txt.btnParity
                        // Пока движок отвечает, кнопка заперта — как у
                        // виджетной версии на время запроса.
                        enabled: !parity.querying
                        onClicked: parity.reload()
                    }

                    Text {
                        objectName: "devParityBadge"
                        text: parity.badgeText
                        color: root.statusColor(parity.badgeColor)
                        font.pixelSize: root.m.statusFontSize
                        font.bold: parity.badgeBold
                    }

                    Item { Layout.fillWidth: true }
                }

                TextEdit {
                    Layout.fillWidth: true
                    objectName: "devParityReport"
                    readOnly: true
                    selectByMouse: true
                    text: parity.reportText !== "" ? parity.reportText
                                                   : root.txt.placeholderParity
                    color: root.colText
                    font.family: "monospace"
                    font.pixelSize: root.m.monoFontSize
                    wrapMode: Text.WordWrap
                }
            }

            Item { Layout.fillHeight: true }
        }
    }

    // ── Вопрос перед установкой ───────────────────────────────────────────
    //
    // У виджетной версии это `QMessageBox.question`; в Big Picture
    // множественные системные окна неудобны (канон, п. 16), поэтому вопрос
    // задаётся своим модальным окном страницы — как это уже сделано в
    // «Настройках».

    /// Спросить и, при согласии, выполнить. Пустой текст — спрашивать не о
    /// чем (ставить нечего), тогда не открываем ничего.
    function ask(title, question, accepted) {
        if (question === "")
            return;
        confirmDialog.heading = title;
        confirmDialog.question = question;
        confirmDialog.accepted = accepted;
        confirmDialog.open();
    }

    Popup {
        id: confirmDialog

        property string heading: ""
        property string question: ""
        property var accepted: null

        objectName: "devConfirmDialog"
        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 520)
        padding: 16

        background: Rectangle {
            color: root.pal.bg_secondary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        // Затемнение под модальным окном — общее решение продукта
        // (`ui_base::modal_dim_argb_hex`, ключ `modal_dim`).
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: confirmDialog.heading
                color: root.colText
                font.bold: true
                font.pixelSize: root.m.sectionFontSize
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                text: confirmDialog.question
                color: root.colText
                font.pixelSize: root.m.statusFontSize
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: root.m.rowSpacing

                Item { Layout.fillWidth: true }

                RexonButton {
                    objectName: "devConfirmYes"
                    pal: root.pal
                    highlighted: true
                    text: root.txt.dialogYes
                    onClicked: {
                        const run = confirmDialog.accepted;
                        confirmDialog.close();
                        if (run)
                            run();
                    }
                }

                RexonButton {
                    id: confirmNo

                    objectName: "devConfirmNo"
                    pal: root.pal
                    text: root.txt.dialogNo
                    onClicked: confirmDialog.close()
                }
            }
        }

        // Фокус при открытии — на безопасном ответе: случайный Enter не
        // должен запускать установку на ~200 МиБ (защита от дурака).
        onOpened: confirmNo.forceActiveFocus()
    }
}
