// Вкладка «Настройки» на QML. Данные и решения — Rust-объект SettingsModel
// (rust/rexon-gui/src/settings_page.rs), который сам ничего не решает, а
// спрашивает ядро (`rexon_core::qt_settings`). Здесь только РАЗМЕТКА.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из `ui_base::skin_colors("steam_dark")` — скина по
// умолчанию всего продукта (правило #17). Своих цветов страница не заводит.
//
// Побочные действия живут снаружи: хозяин страницы кладёт в `root.actions`
// объект, который умеет спрашивать систему (`facts`, `state`) и менять её.
// Каждый его метод возвращает ГОТОВУЮ строку для всплывающего сообщения —
// собранную тем же ядром через `rexon_py`, а не своими словами. Долгие
// работы (применить обои, поставить yt-dlp, размыть, импортировать тему,
// проверить комплектность) идут в фоне и досылают ответ сигналами
// `status`/`refreshed`: без них кнопка молчала бы до конца работы.
//
// Состав разделов, их порядок, заголовки и описания приходят из ядра
// (`qt_settings::SECTIONS` + `section_visible`) — своей цепочки условий
// «показывать ли раздел» у страницы нет.
//
// Цепочка фокуса заложена сразу: все действия — контролы QtQuick.Controls,
// у сфокусированного видна рамка, страница подкручивается к нему.

// Делегаты обращаются к id внешних элементов (root, page).
// (Строку «// qmllint …» здесь писать нельзя: она читается как директива.)
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    /// Объект побочных действий. По умолчанию — свой, на Rust
    /// (`rust/rexon-gui/src/settings_actions.rs`); хозяин страницы может
    /// подставить другой — так вкладку и держит живой Python-продукт
    /// (`rexon/qt_settings/window.py`), пока перенос не закончен.
    property var actions: ownActions

    /// Один раздел вместо всей вкладки — для «Оформления» (01.11.2026):
    /// анимация, шрифты, кнопки заголовка и прочее, что в KDE 6.8 стоит во
    /// «Внешнем виде и стиле», показываются там, а не здесь. Значение —
    /// ключ раздела боковой панели «Оформления»
    /// (`categories::SETTINGS_PANELS`). Пусто — обычная вкладка: на месте
    /// переехавших разделов — ссылки «Оформление → …».
    property string panel: ""

    /// Переехавшие разделы: содержимое то же, что было во вкладке, их
    /// показывает режим `panel`.
    readonly property bool fullPage: root.panel === ""

    SettingsActions {
        id: ownActions
        // Имя нужно проверкам — тем же приёмом, что у модели ниже.
        objectName: "settingsActions"
    }

    /// Снимок состояния машины (JSON от `actions.state()`), разобранный.
    property var st: ({})

    /// Пароль уже введён: Rexon может изменять систему до закрытия окна
    /// (Б13). Опрашивается таймером, пока страница видна; у объекта действий
    /// без этого метода (Python-продукт) полосы нет.
    property bool adminSession: false

    function reloadAdmin() {
        root.adminSession = !!(root.actions && root.actions.adminSession !== undefined
                               && root.actions.adminSession());
    }

    /// Подпись и пояснение кнопки «Установить Rexon в систему…»
    /// (`{label, hint}` от ядра). Пусто — хозяин страницы подставил объект
    /// действий без мастера установки (Python-продукт), и кнопки не будет.
    readonly property var installTexts: (root.actions
                                         && root.actions.installTexts !== undefined)
                                        ? root.actions.installTexts : ({})

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    SettingsModel {
        id: page
        // Имя нужно проверкам: из хозяина страницы модель достают
        // `root.findChild(QObject, "settingsModel")`.
        objectName: "settingsModel"
    }

    // ── Палитра стандартного скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property var txt: page.texts
    /// Раскладка и краски, которых нет в палитре: из ядра, своих чисел нет.
    readonly property var sty: page.styles
    readonly property color colBg: root.pal.bg_primary
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary
    readonly property color colWarn: root.pal.state_warn
    readonly property color colBad: root.pal.state_bad

    // ── Шов с побочными действиями ────────────────────────────────────────

    /// Спросить машину заново и перерисовать всё, что от неё зависит.
    function reload() {
        if (!root.actions)
            return;
        // Снимок машины нужен только разделам самой вкладки и шрифтам; у
        // прочих переехавших разделов — свои объекты действий.
        if (!root.fullPage && root.panel !== "fontsettings")
            return;
        root.st = JSON.parse(root.actions.state());
    }

    /// Выполнить действие и показать его ответ. Пустой ответ — сказать
    /// нечего (действие только сохранило значение).
    function say(message) {
        if (message)
            toast.show(message);
    }

    /// Перечитать ТОЛЬКО список играющих фоновых звуков.
    ///
    /// Отдельно от `reload`, потому что зовётся по таймеру: полный снимок
    /// состояния ходит в систему за шрифтами, звуковыми темами и списком
    /// установленных обоев, и делать это каждые три секунды значило бы
    /// держать вкладку в постоянной работе на ровном месте.
    function reloadAmbient() {
        if (!root.actions)
            return;
        root.st = Object.assign({}, root.st,
                                { amb: JSON.parse(root.actions.ambientState()) });
    }

    /// Действие изменило что-то, что видно на странице: ответ показать,
    /// состояние перечитать.
    function did(message) {
        root.say(message);
        root.reload();
    }

    Connections {
        target: root.actions

        // Долгая работа закончилась — её ответ приходит сюда.
        function onStatus(message) {
            root.say(message);
        }

        // Долгая работа изменила состояние машины.
        function onRefreshed() {
            root.reload();
        }
    }

    /// Подкрутить страницу к элементу, который получил фокус: без этого Tab
    /// уводит фокус за нижний край, и вкладка выглядит зависшей.
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
        contentHeight: pageColumn.implicitHeight + 2 * root.sty.page.margin
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {}

        // ⚠️ Раскладка внутри ОБЫЧНОГО Item обязана получить высоту руками:
        // сама она остаётся нулевой, и страница схлопывается в полоску.
        ColumnLayout {
            id: pageColumn
            x: root.sty.page.margin
            y: root.sty.page.margin
            width: flick.width - 2 * root.sty.page.margin
            height: implicitHeight
            spacing: root.sty.page.spacing

            Text {
                visible: root.fullPage
                text: page.pageTitle
                color: root.colText
                font.pointSize: page.pageTitlePointSize
                font.bold: true
            }

            // ── Полоса-предупреждение: в системе нет polkit ────────────────
            // Без него молча не работает ВСЁ, что требует прав администратора.
            // Причина одна и общая — говорим о ней один раз и вверху.
            Rectangle {
                Layout.fillWidth: true
                visible: (root.st.polkit || "") !== ""
                color: root.sty.banner.background
                border.color: root.sty.banner.border
                border.width: 1
                radius: root.sty.banner.radius
                implicitHeight: polkitText.implicitHeight
                                + 2 * root.sty.banner.padding

                Text {
                    id: polkitText
                    x: root.sty.banner.padding
                    y: root.sty.banner.padding
                    width: parent.width - 2 * root.sty.banner.padding
                    text: root.st.polkit || ""
                    wrapMode: Text.WordWrap
                    textFormat: Text.RichText
                    color: root.colText
                }
            }

            // ── Полоса «Rexon может изменять систему» (Б13) ────────────────
            // Видна, пока пароль введён и право живо; кнопка снимает его,
            // не закрывая окна. Та же подпись и то же действие, что в меню
            // значка в лотке.
            Rectangle {
                id: adminBanner
                objectName: "adminBanner"
                Layout.fillWidth: true
                visible: root.adminSession && root.fullPage
                color: root.sty.banner.background
                border.color: root.sty.banner.border
                border.width: 1
                radius: root.sty.banner.radius
                implicitHeight: adminRow.implicitHeight + 2 * root.sty.banner.padding

                RowLayout {
                    id: adminRow
                    x: root.sty.banner.padding
                    y: root.sty.banner.padding
                    width: parent.width - 2 * root.sty.banner.padding
                    spacing: root.sty.banner.padding

                    Text {
                        Layout.fillWidth: true
                        text: "🔓 " + root.txt["admin.info"]
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }

                    RexonButton {
                        id: adminRevoke
                        objectName: "adminRevoke"
                        pal: root.pal
                        text: root.txt["admin.revoke"]
                        onClicked: {
                            root.say(root.actions.revokeAdmin());
                            root.reloadAdmin();
                        }
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(adminRevoke)
                    }
                }
            }

            // ── Разделы ───────────────────────────────────────────────────
            Repeater {
                id: sectionRepeater
                // Шрифты переехали в «Оформление»: во вкладке их рамки нет,
                // в режиме `panel` = «fontsettings» есть только она.
                model: (page.sections || []).filter(
                           s => root.fullPage ? s.id !== "fontsGroup"
                                              : (root.panel === "fontsettings"
                                                 && s.id === "fontsGroup"))

                delegate: Rectangle {
                    id: sectionFrame
                    required property var modelData

                    Layout.fillWidth: true
                    color: root.sty.group.background
                    border.color: root.sty.group.border
                    border.width: 1
                    radius: root.sty.group.radius
                    // Поля раздела отсчитываются ОТ РАМКИ, а не от края:
                    // у оригинала рамка нарисована таблицей стилей, и поля
                    // раскладки (16 и 14) Qt откладывает внутри неё. Пока
                    // толщина рамки здесь не учитывалась, содержимое каждого
                    // раздела выходило на точку левее и шире оригинала
                    // (x 40 против 41, ширина 1320 против 1318), а сам
                    // раздел — на две точки ниже (замер 05.09).
                    implicitHeight: sectionColumn.implicitHeight
                                    + 2 * (root.sty.group.marginV + border.width)

                    ColumnLayout {
                        id: sectionColumn
                        x: root.sty.group.marginH + sectionFrame.border.width
                        y: root.sty.group.marginV + sectionFrame.border.width
                        width: parent.width - 2 * (root.sty.group.marginH
                                                   + sectionFrame.border.width)
                        height: implicitHeight
                        spacing: root.sty.group.spacing

                        Text {
                            Layout.fillWidth: true
                            text: sectionFrame.modelData.title
                            color: root.colText
                            font.pointSize: page.sectionTitlePointSize
                            font.bold: true
                            wrapMode: Text.WordWrap
                        }

                        Text {
                            Layout.fillWidth: true
                            visible: sectionFrame.modelData.description !== ""
                            text: sectionFrame.modelData.description
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            wrapMode: Text.WordWrap
                        }

                        Loader {
                            id: sectionBody
                            Layout.fillWidth: true
                            Layout.preferredHeight: item ? item.implicitHeight : 0
                            sourceComponent: root.sectionComponent(
                                                 sectionFrame.modelData.id)
                        }
                    }
                }
            }

            // ── Переехавшее в «Оформление» (01.11.2026) ───────────────────
            // ⏳ Ссылки — на одну версию: человек, привыкший искать здесь
            // анимацию и шрифты, видит, куда они ушли, и попадает туда одним
            // нажатием. Список и подписи — ядро
            // (`categories::moved_settings_links`).
            Rectangle {
                id: movedFrame
                objectName: "movedToAppearance"
                Layout.fillWidth: true
                visible: root.fullPage && page.movedPanels.length > 0
                color: root.sty.group.background
                border.color: root.sty.group.border
                border.width: 1
                radius: root.sty.group.radius
                implicitHeight: movedColumn.implicitHeight
                                + 2 * (root.sty.group.marginV + border.width)

                ColumnLayout {
                    id: movedColumn
                    x: root.sty.group.marginH + movedFrame.border.width
                    y: root.sty.group.marginV + movedFrame.border.width
                    width: parent.width - 2 * (root.sty.group.marginH + movedFrame.border.width)
                    height: implicitHeight
                    spacing: root.sty.group.spacing

                    Text {
                        Layout.fillWidth: true
                        text: page.movedHeading
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                        wrapMode: Text.WordWrap
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: root.sty.spacing.fontRow

                        Repeater {
                            model: page.movedPanels

                            delegate: RexonButton {
                                id: movedLink
                                required property var modelData
                                pal: root.pal
                                text: movedLink.modelData.text
                                onClicked: root.openAppearance(movedLink.modelData.key)
                                onActiveFocusChanged: if (activeFocus) root.ensureVisible(movedLink)
                            }
                        }
                    }
                }
            }

            // ── Разделы, которые показывает «Оформление» (режим `panel`) ──
            // Те же компоненты, что стояли здесь до переезда; строятся только
            // в своём разделе, чтобы вкладка не опрашивала машину зря.
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.panel === "animations"
                visible: active
                sourceComponent: Component {
                    AnimationsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Все эффекты KWin — только в KDE (раздел сам прячется в других).
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.panel === "desktopeffects"
                visible: active
                sourceComponent: Component {
                    DesktopEffectsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.panel === "windowbuttons"
                visible: active
                sourceComponent: Component {
                    WindowButtonsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Отклик при запуске (как в модуле «Курсоры» KDE; вне KDE —
            // честная подпись, почему настройки нет).
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.panel === "launchfeedback"
                visible: active
                sourceComponent: Component {
                    LaunchFeedbackSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Значки в меню и на кнопках, подписи панелей инструментов.
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.panel === "appwidgets"
                visible: active
                sourceComponent: Component {
                    AppWidgetsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Только в сеансе Deepin (раздел сам прячется в других).
            DdeLookSection {
                id: ddeLook
                Layout.fillWidth: true
                visible: root.fullPage && ddeLook.st.available === true
                pal: root.pal
                sty: root.sty
            }
        }
    }

    /// Открыть раздел `key` во вкладке «Оформление» — через ту же дверь
    /// окна, что у `--section=` и второго запуска (`openSection`).
    function openAppearance(key) {
        const w = root.Window.window;
        if (w && w.openSection !== undefined)
            w.openSection("appearance", key);
    }

    /// Содержимое раздела по его имени из ядра.
    ///
    /// Незнакомое имя — не «пустая рамка молча»: ядро знает раздел, а
    /// страница нет, и об этом надо сказать вслух (так же поступает
    /// виджетная вкладка).
    function sectionComponent(id) {
        switch (id) {
        case "langGroup": return cLanguage;
        case "skinGroup": return cSkin;
        case "obGroup": return cOnboarding;
        case "trayGroup": return cTray;
        case "fmGroup": return cFileManager;
        case "quickFix": return cQuickFix;
        case "completeness": return cCompleteness;
        case "wpGroup": return cWallpaper;
        case "libraryGroup": return cLibrary;
        case "importGroup": return cImport;
        case "blurGroup": return cBlur;
        case "ambientGroup": return cAmbient;
        case "soundThemeGroup": return cSound;
        case "mateIconsGroup": return cMate;
        case "fontsGroup": return cFonts;
        case "protGroup": return cProtection;
        case "plasmaGroup": return cPlasma;
        case "aeroGroup": return cAero;
        }
        console.warn("вкладка «Настройки» не умеет строить раздел «" + id
                     + "»: ядро его знает, страница — нет");
        return null;
    }

    // ══ 🌐 Язык интерфейса ═══════════════════════════════════════════════
    //
    // Список — из ядра (`qt_settings::language_choices`: po/LINGUAS и
    // найденные каталоги, подписи самоназванием). Язык меняется после
    // перезапуска: снимок говорит, нужен ли он (`st.lang.restart`), и тогда
    // под списком появляются пояснение и кнопка «Перезапустить Rexon».
    // У объекта действий Python-продукта `setLanguage` нет — список там
    // только показывает выбор.

    Component {
        id: cLanguage

        ColumnLayout {
            id: langBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var lang: root.st.lang || ({})
            readonly property bool canChange: root.actions
                                              && root.actions.setLanguage !== undefined

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    Layout.minimumWidth: root.sty.widths.fieldLabel
                    text: root.txt["lang.row"]
                    textFormat: Text.RichText
                    color: root.colText
                }

                RexonComboBox {
                    id: langCombo
                    objectName: "languageCombo"
                    Layout.fillWidth: true
                    enabled: langBody.canChange
                    pal: root.pal
                    model: page.languages
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: langBody.lang.choice || "system"
                    onActivated: root.did(root.actions.setLanguage(langCombo.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(langCombo)
                }
            }

            Text {
                Layout.fillWidth: true
                visible: langBody.lang.restart === true
                text: root.txt["lang.restart_hint"]
                wrapMode: Text.WordWrap
                color: root.colWarn
            }

            RexonButton {
                id: langRestart
                objectName: "languageRestart"
                visible: langBody.lang.restart === true && langBody.canChange
                pal: root.pal
                highlighted: true
                text: root.txt["lang.restart"]
                onClicked: root.say(root.actions.restartRexon())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(langRestart)
            }
        }
    }

    // ══ 🎨 Скин интерфейса ════════════════════════════════════════════════

    Component {
        id: cSkin

        ColumnLayout {
            id: skinBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    Layout.minimumWidth: root.sty.widths.fieldLabel
                    text: root.txt["skin.row"]
                    textFormat: Text.RichText
                    color: root.colText
                }

                RexonComboBox {
                    id: skinCombo
                    Layout.fillWidth: true
                    pal: root.pal
                    model: page.skins
                    textRole: "label"
                    valueRole: "id"
                    // Значение ставится ДО подписки на изменения: стартовая
                    // установка не должна выглядеть как выбор человека
                    // (скин уже применён при запуске приложения).
                    selectedValue: root.st.skin || ""
                    onActivated: root.did(root.actions.setSkin(skinCombo.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(skinCombo)
                }
            }

            Text {
                Layout.fillWidth: true
                text: page.skinDescription(root.st.skin || "")
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["skin.hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🎓 Обучение при первом запуске ════════════════════════════════════

    Component {
        id: cOnboarding

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonCheckBox {
                id: obCheck
                pal: root.pal
                text: root.txt["ob.check"]
                checked: root.st.onboarding === true
                onToggled: root.did(root.actions.setOnboarding(obCheck.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(obCheck)
            }

            RexonButton {
                id: obReplay
                // Полоса во всю ширину карточки, как в виджетной вкладке:
                // кнопка по содержимому терялась под галочкой и читалась
                // как второстепенная приписка, а это единственный способ
                // вернуть обучение.
                Layout.fillWidth: true
                pal: root.pal
                text: root.txt["ob.replay"]
                onClicked: root.did(root.actions.replayOnboarding())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(obReplay)
            }

            // «Установить Rexon в систему…» — решение Wehrwolfmann
            // 15.09.2026: кнопка стоит рядом с «Пройти обучение заново».
            // Показывать ли её, решает ядро (`installOffered`): у
            // установленного Rexon — пакет, Steam, установка для всех или
            // для себя — нажимать её не на что, и кнопки нет вовсе. Подпись
            // и пояснение — оттуда же, своих букв здесь нет.
            RexonButton {
                id: obInstall
                Layout.fillWidth: true
                visible: root.actions && root.actions.installOffered === true
                pal: root.pal
                text: root.installTexts.label !== undefined
                      ? root.installTexts.label : ""
                onClicked: root.did(root.actions.installRexon())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(obInstall)
            }

            Text {
                Layout.fillWidth: true
                visible: obInstall.visible
                text: root.installTexts.hint !== undefined
                      ? root.installTexts.hint : ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🔔 Значок в системном лотке ═══════════════════════════════════════

    Component {
        id: cTray

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonCheckBox {
                id: trayAutostart
                pal: root.pal
                text: root.txt["tray.autostart"]
                checked: root.st.tray !== undefined
                         && root.st.tray.autostart === true
                onToggled: root.did(root.actions.setAutostart(
                                        trayAutostart.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(trayAutostart)
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["tray.autostart_hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // Лотка в сессии может не быть вовсе — тогда и значка нет, и
            // крестик окна закрывает Rexon как раньше. Молчать об этом
            // нельзя: настройка выше в такой сессии ничего не даст.
            Text {
                Layout.fillWidth: true
                visible: root.st.tray !== undefined
                         && root.st.tray.available === false
                text: root.txt["tray.missing"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // Докачка значков и курсора к теме: пока Rexon запущен, он
            // следит за темой окружения (`rexon_core::theme_watch`).
            RexonCheckBox {
                id: trayThemeDeps
                pal: root.pal
                text: root.txt["tray.theme_deps"]
                checked: root.st.tray === undefined
                         || root.st.tray.themeDepsAuto !== false
                onToggled: root.did(root.actions.setThemeDepsAuto(
                                        trayThemeDeps.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(trayThemeDeps)
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["tray.theme_deps_hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 📁 Файловый менеджер ══════════════════════════════════════════════

    Component {
        id: cFileManager

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            Repeater {
                model: page.fmChoices

                delegate: Rectangle {
                    id: fmRow
                    required property var modelData

                    readonly property bool isAuto: fmRow.modelData.id === "auto"
                    readonly property var badge: page.fmBadge(
                        (root.st.fm && root.st.fm.badges
                         && root.st.fm.badges[fmRow.modelData.id]) === true)

                    Layout.fillWidth: true
                    radius: root.sty.fmRow.radius
                    // Подсветка строки под фокусом — та же «полоса фокуса»,
                    // что у радиокнопок виджетной вкладки; краска из ядра.
                    color: fmButton.activeFocus ? root.sty.fmRow.focus
                                                : "transparent"
                    implicitHeight: fmRowLayout.implicitHeight
                                    + 2 * root.sty.fmRow.marginV

                    RowLayout {
                        id: fmRowLayout
                        x: root.sty.fmRow.marginH
                        y: root.sty.fmRow.marginV
                        width: parent.width - 2 * root.sty.fmRow.marginH
                        height: implicitHeight
                        spacing: root.sty.fmRow.spacing

                        RadioButton {
                            id: fmButton
                            Layout.fillWidth: true
                            implicitHeight: 36
                            text: fmRow.modelData.label
                            checked: (root.st.fm ? root.st.fm.current : "")
                                     === fmRow.modelData.id
                            onClicked: root.did(
                                root.actions.setFileManager(fmRow.modelData.id))
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(fmRow)

                            indicator: Rectangle {
                                implicitWidth: 18
                                implicitHeight: 18
                                x: fmButton.leftPadding
                                y: fmButton.topPadding
                                   + (fmButton.availableHeight - height) / 2
                                radius: 9
                                color: "transparent"
                                border.color: fmButton.activeFocus
                                              || fmButton.hovered
                                              ? root.colAccent : root.colBorder
                                border.width: fmButton.activeFocus ? 2 : 1

                                Rectangle {
                                    anchors.centerIn: parent
                                    visible: fmButton.checked
                                    width: 9
                                    height: 9
                                    radius: 5
                                    color: root.colAccent
                                }
                            }

                            contentItem: Text {
                                leftPadding: fmButton.indicator.width
                                             + fmButton.spacing
                                text: fmButton.text
                                color: root.colText
                                verticalAlignment: Text.AlignVCenter
                            }
                        }

                        Text {
                            visible: !fmRow.isAuto
                            text: fmRow.badge.text
                            color: fmRow.badge.color
                        }

                        Text {
                            visible: fmRow.isAuto
                            text: root.txt["fm.auto_hint"]
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                        }
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["fm.wm_note"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: fmApply
                    pal: root.pal
                    tone: root.sty.primary
                    text: root.txt["fm.apply"]
                    onClicked: root.did(root.actions.applyFileManager())
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(fmApply)
                }
            }

            Text {
                Layout.fillWidth: true
                text: page.t1("fm.current",
                              (root.st.fm && root.st.fm.currentName)
                              || root.txt["fm.unknown"])
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🔧 Быстрое восстановление приложений ══════════════════════════════

    Component {
        id: cQuickFix

        ColumnLayout {
            id: qfBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var qf: root.st.quickfix || ({})

            // ⚠️ Имя окружения тут приходит из снимка (`qf.de`), а НЕ из
            // `page.deDisplay`. Заголовки разделов называют сессию («KDE»),
            // а этот раздел возвращает приложения конкретного стола и зовёт
            // его полным именем из базы столов («KDE Plasma») — так же, как
            // виджетная вкладка.

            // Окружение ядру неизвестно — восстанавливать нечего, и об этом
            // говорится прямо, а не пустой кнопкой.
            Text {
                Layout.fillWidth: true
                visible: qfBody.qf.known !== true
                text: page.t1("quickfix.unknown_de", qfBody.qf.de || "")
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true
                visible: qfBody.qf.known === true
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    text: page.t1("quickfix.current", qfBody.qf.de || "")
                    textFormat: Text.RichText
                    wrapMode: Text.WordWrap
                    color: root.colText
                }

                RexonButton {
                    id: quickFixButton
                    pal: root.pal
                    tone: root.sty.primary
                    text: page.t1("quickfix.button", qfBody.qf.de || "")
                    onClicked: root.did(root.actions.restoreNative())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(quickFixButton)
                }
            }
        }
    }

    // ══ 🧩 Комплектность рабочего стола ═══════════════════════════════════

    Component {
        id: cCompleteness

        RowLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            Text {
                Layout.fillWidth: true
                text: (root.st.compl && root.st.compl.text)
                      || root.txt["compl.idle"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonButton {
                id: complButton
                pal: root.pal
                tone: root.sty.primary
                enabled: !(root.st.compl && root.st.compl.busy)
                text: root.txt["compl.button"]
                onClicked: root.did(root.actions.checkCompleteness())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(complButton)
            }
        }
    }

    // ══ 🎬 Обои ═══════════════════════════════════════════════════════════

    Component {
        id: cWallpaper

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            // Движок обоев не поднялся — раздела как такового нет, есть
            // объяснение почему.
            Text {
                Layout.fillWidth: true
                visible: !(root.st.wp && root.st.wp.engine === true)
                text: root.txt["wp.no_engine"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }

            ColumnLayout {
                id: wpBody
                Layout.fillWidth: true
                visible: (root.st.wp && root.st.wp.engine) === true
                spacing: root.sty.group.spacing

                readonly property var wp: root.st.wp || ({})
                readonly property var audio: wpBody.wp.audio || ({})

                Text {
                    Layout.fillWidth: true
                    text: wpBody.wp.status || ""
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                RowLayout {
                    Layout.fillWidth: true

                    // Красная кнопка необратимого действия — общий тип
                    // (`RexonDangerButton`), а не подкрашенная обычная: цвет
                    // «опасного действия» один на продукт и приезжает из
                    // ядра палитрой (правило #12).
                    RexonDangerButton {
                        id: wpStop
                        pal: root.pal
                        tone: root.sty.danger
                        enabled: wpBody.wp.running === true
                        text: root.txt["wp.stop"]
                        onClicked: root.did(root.actions.stopWallpaper())
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(wpStop)
                    }

                    Item { Layout.fillWidth: true }
                }

                Text {
                    Layout.fillWidth: true
                    text: root.txt["wp.from_file"]
                    color: root.colText
                }

                Text {
                    Layout.fillWidth: true
                    text: root.txt["wp.file_hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                // ── Звук обоев ────────────────────────────────────────────
                // Все четыре контрола пишут в те же ключи настроек, за
                // которыми следит хост обоев: правка применяется НА ХОДУ,
                // перезапускать обои не нужно.
                RexonCheckBox {
                    id: wpAudio
                    pal: root.pal
                    text: root.txt["wp.video_audio"]
                    checked: wpBody.audio.enabled === true
                    onToggled: root.did(root.actions.setWallpaperAudio(
                                            wpAudio.checked, Math.round(wpVolume.value),
                                            wpMuteOther.checked,
                                            wpHideCovered.checked))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(wpAudio)
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        Layout.minimumWidth: root.sty.widths.volumeLabel
                        text: page.volumeLabel(wpVolume.value)
                        color: root.colText
                    }

                    RexonSlider {
                        id: wpVolume
                        Layout.maximumWidth: root.sty.widths.volumeSlider
                        Layout.fillWidth: true
                        pal: root.pal
                        from: 0
                        to: 100
                        stepSize: 1
                        value: wpBody.audio.volume !== undefined
                               ? wpBody.audio.volume : 0
                        onMoved: root.did(root.actions.setWallpaperAudio(
                                              wpAudio.checked, Math.round(wpVolume.value),
                                              wpMuteOther.checked,
                                              wpHideCovered.checked))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(wpVolume)
                    }

                    Item { Layout.fillWidth: true }
                }

                RexonCheckBox {
                    id: wpMuteOther
                    pal: root.pal
                    text: root.txt["wp.audio_mute_other"]
                    checked: wpBody.audio.muteOther === true
                    onToggled: root.did(root.actions.setWallpaperAudio(
                                            wpAudio.checked, Math.round(wpVolume.value),
                                            wpMuteOther.checked,
                                            wpHideCovered.checked))
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(wpMuteOther)
                }

                RexonCheckBox {
                    id: wpHideCovered
                    pal: root.pal
                    text: root.txt["wp.audio_mute_covered"]
                    checked: wpBody.audio.hideCovered === true
                    onToggled: root.did(root.actions.setWallpaperAudio(
                                            wpAudio.checked, Math.round(wpVolume.value),
                                            wpMuteOther.checked,
                                            wpHideCovered.checked))
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(wpHideCovered)
                }

                Text {
                    Layout.fillWidth: true
                    text: root.txt["wp.audio_hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        id: wpPickImage
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["wp.pick_image"]
                        onClicked: root.did(root.actions.pickWallpaper("static"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(wpPickImage)
                    }

                    RexonButton {
                        id: wpPickVideo
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["wp.pick_video"]
                        onClicked: root.did(root.actions.pickWallpaper("video"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(wpPickVideo)
                    }

                    Item { Layout.fillWidth: true }
                }

                // ── YouTube / Vimeo ───────────────────────────────────────
                Text {
                    Layout.fillWidth: true
                    text: root.txt["yt.header"]
                    color: root.colText
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        Layout.minimumWidth: root.sty.widths.url
                        text: root.txt["yt.url_label"]
                        color: root.colText
                    }

                    RexonTextField {
                        id: ytUrl
                        Layout.fillWidth: true
                        pal: root.pal
                        placeholderText: root.txt["yt.url_hint"]
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytUrl)
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        text: root.txt["yt.quality_label"]
                        color: root.colText
                    }

                    RexonComboBox {
                        id: ytQuality
                        Layout.preferredWidth: 150
                        pal: root.pal
                        model: page.ytQualities
                        textRole: "label"
                        valueRole: "value"
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ytQuality)
                    }

                    Text {
                        text: root.txt["yt.mode_label"]
                        color: root.colText
                    }

                    RexonComboBox {
                        id: ytMode
                        Layout.preferredWidth: 150
                        pal: root.pal
                        model: page.ytModes
                        textRole: "label"
                        valueRole: "value"
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytMode)
                    }

                    Item { Layout.fillWidth: true }

                    RexonButton {
                        id: ytApply
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: !(wpBody.wp.busy === true)
                        text: wpBody.wp.busy === true ? root.txt["wp.working"]
                                                      : root.txt["yt.apply"]
                        onClicked: root.did(root.actions.applyYoutube(
                                                ytUrl.text, ytQuality.currentValue,
                                                ytMode.currentValue,
                                                ytAudio.checked))
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytApply)
                    }
                }

                // Отдельная галочка звука для YouTube — защита от дурака: она
                // не зависит от галочки видео-обоев.
                RexonCheckBox {
                    id: ytAudio
                    pal: root.pal
                    text: root.txt["yt.audio"]
                    checked: wpBody.wp.ytAudio !== false
                    onToggled: root.did(root.actions.setYoutubeAudio(ytAudio.checked))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytAudio)
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        Layout.fillWidth: true
                        text: wpBody.wp.ytdlpLabel || ""
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }

                    RexonButton {
                        id: ytdlpButton
                        pal: root.pal
                        tone: root.sty.primary
                        // Пока идёт установка — кнопка выключена и говорит об
                        // этом: иначе человек жмёт второй раз и вводит пароль
                        // ради повторной установки.
                        enabled: !(wpBody.wp.ytdlpBusy === true)
                        text: wpBody.wp.ytdlpBusy === true
                              ? root.txt["wp.working"]
                              : (wpBody.wp.ytdlpButton || "")
                        onClicked: root.did(root.actions.ytdlp(
                                                wpBody.wp.ytdlpAction || ""))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ytdlpButton)
                    }
                }

                // ── Веб-обои ──────────────────────────────────────────────
                Text {
                    Layout.fillWidth: true
                    text: root.txt["web.header"]
                    color: root.colText
                }

                Text {
                    Layout.fillWidth: true
                    text: root.txt["web.hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        Layout.minimumWidth: root.sty.widths.url
                        text: root.txt["yt.url_label"]
                        color: root.colText
                    }

                    RexonTextField {
                        id: webUrl
                        Layout.fillWidth: true
                        pal: root.pal
                        placeholderText: root.txt["web.url_hint"]
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(webUrl)
                    }

                    RexonButton {
                        id: webApply
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: !(wpBody.wp.busy === true)
                        text: wpBody.wp.busy === true ? root.txt["wp.working"]
                                                      : root.txt["web.apply"]
                        onClicked: root.did(root.actions.applyWeb(webUrl.text))
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(webApply)
                    }
                }
            }
        }
    }

    // ══ 📚 Библиотека обоев ═══════════════════════════════════════════════

    Component {
        id: cLibrary

        ColumnLayout {
            id: libBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var lib: root.st.lib || ({})
            readonly property var items: libBody.lib.items || []
            // Столбцы считает ядро тем же движком сеток, что и виджетные
            // вкладки, — своих ступеней по ширине у страницы нет.
            readonly property int cols: Math.max(
                1, page.libraryColumns(libBody.width))
            readonly property int shown: Math.min(
                libBody.items.length, page.libraryPageSize(libBody.cols))

            Text {
                Layout.fillWidth: true
                visible: libBody.lib.engine !== true
                text: root.txt["lib.no_engine"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            Text {
                Layout.fillWidth: true
                visible: libBody.lib.engine === true
                         && libBody.items.length === 0
                // Надпись «пусто» у виджетов лежит с полем 8 со всех сторон.
                Layout.topMargin: root.sty.lib.gridTopMargin
                padding: root.sty.lib.emptyPadding
                text: root.txt["lib.empty"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            GridLayout {
                Layout.fillWidth: true
                visible: libBody.items.length > 0
                Layout.topMargin: root.sty.lib.gridTopMargin
                columns: libBody.cols
                columnSpacing: root.sty.lib.gridSpacing
                rowSpacing: root.sty.lib.gridSpacing

                Repeater {
                    model: libBody.shown

                    delegate: Rectangle {
                        id: libCard
                        required property int index
                        readonly property var wp: libBody.items[libCard.index]

                        Layout.preferredWidth: page.libraryCardWidth
                        Layout.alignment: Qt.AlignTop | Qt.AlignLeft
                        implicitHeight: libCardColumn.implicitHeight
                        radius: root.sty.lib.cardRadius
                        color: "transparent"
                        border.width: 1
                        border.color: libHover.hovered ? root.sty.lib.cardHover
                                                       : root.sty.lib.cardBorder

                        HoverHandler { id: libHover }

                        ColumnLayout {
                            id: libCardColumn
                            width: parent.width
                            height: implicitHeight
                            spacing: 0

                            // Полоска предпросмотра. Картинки нет — рисуем
                            // ту же заглушку, что и виджетная плитка.
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: page.libraryPreviewHeight
                                color: libPreview.status === Image.Ready
                                       ? "transparent"
                                       : root.sty.lib.placeholderBackground
                                radius: root.sty.lib.cardRadius

                                Image {
                                    id: libPreview
                                    anchors.fill: parent
                                    source: libCard.wp.preview || ""
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    cache: true
                                    clip: true
                                    visible: status === Image.Ready
                                }

                                Text {
                                    anchors.centerIn: parent
                                    visible: libPreview.status !== Image.Ready
                                    text: "🖼️"
                                    font.pointSize: root.sty.lib.placeholderPointSize
                                    color: root.colTextSecondary
                                    font.weight: root.pal.text_secondary_weight
                                }

                                RexonZoomArea {
                                    anchors.fill: parent
                                    pal: root.pal
                                    radius: root.sty.lib.cardRadius
                                    enabled: libPreview.status === Image.Ready
                                    visible: enabled
                                    onActivated: root.openLibraryViewer(libBody.items,
                                                                        libCard.index)
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                Layout.leftMargin: root.sty.lib.infoMarginH
                                Layout.rightMargin: root.sty.lib.infoMarginH
                                Layout.topMargin: root.sty.lib.infoMarginV
                                Layout.bottomMargin: root.sty.lib.infoMarginV
                                spacing: root.sty.lib.infoSpacing

                                Text {
                                    Layout.fillWidth: true
                                    text: libCard.wp.name || root.txt["lib.unnamed"]
                                    font.bold: true
                                    // Имя обрезается многоточием, полное — в
                                    // подсказке: перенос спасает от длинной
                                    // фразы, но не от одного длинного слова.
                                    elide: Text.ElideRight
                                    maximumLineCount: 2
                                    wrapMode: Text.WordWrap
                                    color: root.colText

                                    ToolTip.visible: nameHover.hovered
                                    ToolTip.text: libCard.wp.name
                                                  || root.txt["lib.unnamed"]
                                    HoverHandler { id: nameHover }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    // Подпись формата для человека приходит из
                                    // ядра (`formatLabel`); служебное `format`
                                    // — только для логики кнопок ниже.
                                    text: libCard.wp.formatLabel || libCard.wp.format || ""
                                    elide: Text.ElideRight
                                    color: root.colTextSecondary
                                    font.weight: root.pal.text_secondary_weight
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: root.sty.lib.buttonSpacing

                                    RexonButton {
                                        id: libPreviewBtn
                                        Layout.preferredWidth: root.sty.widths.libPreviewButton
                                        leftPadding: 0
                                        visible: page.libraryShowsPreview(
                                                     libCard.wp.format || "")
                                        pal: root.pal
                                        text: root.txt["lib.preview"]
                                        ToolTip.visible: hovered
                                        ToolTip.text: root.txt["lib.preview_tip"]
                                        onClicked: root.say(
                                            root.actions.previewLibrary(
                                                libCard.wp.path || ""))
                                        onActiveFocusChanged: if (activeFocus)
                                            root.ensureVisible(libPreviewBtn)
                                    }

                                    RexonButton {
                                        id: libApply
                                        Layout.fillWidth: true
                                        pal: root.pal
                                        tone: root.sty.primary
                                        text: root.txt["lib.apply"]
                                        onClicked: root.did(
                                            root.actions.applyLibrary(
                                                libCard.wp.path || "",
                                                libCard.wp.format || ""))
                                        onActiveFocusChanged: if (activeFocus)
                                            root.ensureVisible(libApply)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ══ 🪟 Импорт тем Windows ═════════════════════════════════════════════

    Component {
        id: cImport

        ColumnLayout {
            id: impBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var imp: root.st.imp || ({})

            Text {
                Layout.fillWidth: true
                text: root.txt["imp.formats"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            Text {
                Layout.fillWidth: true
                visible: (impBody.imp.note || "") !== ""
                text: impBody.imp.note || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true

                RexonButton {
                    id: impPick
                    pal: root.pal
                    tone: root.sty.primary
                    enabled: impBody.imp.available === true
                             && !(impBody.imp.busy === true)
                    text: impBody.imp.busy === true ? root.txt["imp.working"]
                                                    : root.txt["imp.pick"]
                    onClicked: root.did(root.actions.pickImport())
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(impPick)
                }

                Item { Layout.fillWidth: true }
            }

            Text {
                Layout.fillWidth: true
                visible: impBody.imp.available !== true
                text: root.txt["imp.unavailable_note"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }
        }
    }

    // ══ 🌫 Размытие обоев ═════════════════════════════════════════════════

    Component {
        id: cBlur

        ColumnLayout {
            id: blurBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var blur: root.st.blur || ({})

            Text {
                Layout.fillWidth: true
                visible: blurBody.blur.engine !== true
                text: root.txt["wp.no_engine"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: blurBody.blur.engine === true
                spacing: root.sty.group.spacing

                Text {
                    Layout.fillWidth: true
                    visible: blurBody.blur.pillow !== true
                    text: root.txt["blur.no_pillow"]
                    wrapMode: Text.WordWrap
                    color: root.colWarn
                    font.bold: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        Layout.minimumWidth: root.sty.widths.blurLabel
                        text: root.txt["blur.strength"]
                        color: root.colText
                    }

                    RexonSlider {
                        id: blurSlider
                        Layout.fillWidth: true
                        pal: root.pal
                        from: page.blurMin
                        to: page.blurMax
                        stepSize: page.blurStep
                        tickStep: page.blurStep
                        snapMode: Slider.SnapAlways
                        value: blurBody.blur.radius !== undefined
                               ? blurBody.blur.radius : page.blurDefault
                        onMoved: root.say(root.actions.setBlurRadius(Math.round(blurSlider.value)))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(blurSlider)
                    }

                    Text {
                        Layout.minimumWidth: root.sty.widths.blurValue
                        text: page.t1("blur.px", String(Math.round(blurSlider.value)))
                        color: root.colText
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        id: blurApply
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: blurBody.blur.pillow === true
                                 && !(blurBody.blur.busy === true)
                        text: blurBody.blur.busy === true ? root.txt["wp.working"]
                                                          : root.txt["blur.apply"]
                        onClicked: root.did(root.actions.blur("blur"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(blurApply)
                    }

                    RexonButton {
                        id: blurRemove
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: !(blurBody.blur.busy === true)
                        text: blurBody.blur.busy === true ? root.txt["wp.working"]
                                                          : root.txt["blur.remove"]
                        onClicked: root.did(root.actions.blur("unblur"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(blurRemove)
                    }

                    Item { Layout.fillWidth: true }
                }
            }
        }
    }

    // ══ 🔊 Фоновые звуки ══════════════════════════════════════════════════

    Component {
        id: cAmbient

        ColumnLayout {
            id: ambBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var amb: root.st.amb || ({})
            readonly property var active: ambBody.amb.active || []

            Text {
                Layout.fillWidth: true
                visible: ambBody.amb.manager !== true
                text: root.txt["amb.no_manager"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: ambBody.amb.manager === true
                spacing: root.sty.group.spacing

                Text {
                    Layout.fillWidth: true
                    visible: ambBody.amb.mpv !== true
                    text: root.txt["amb.no_mpv"]
                    wrapMode: Text.WordWrap
                    color: root.colWarn
                    font.bold: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        id: ambAdd
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["amb.add"]
                        ToolTip.visible: hovered
                        ToolTip.text: root.txt["amb.add_tip"]
                        onClicked: root.did(root.actions.addAmbient(Math.round(ambVolume.value)))
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ambAdd)
                    }

                    Text {
                        text: root.txt["amb.volume"]
                        color: root.colText
                    }

                    RexonSlider {
                        id: ambVolume
                        Layout.maximumWidth: root.sty.widths.ambientSlider
                        Layout.fillWidth: true
                        pal: root.pal
                        from: 0
                        to: 100
                        stepSize: 1
                        value: page.ambientDefaultVolume
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ambVolume)
                    }

                    Text {
                        Layout.minimumWidth: root.sty.widths.ambientPercent
                        text: page.t1("amb.percent",
                                      String(Math.round(ambVolume.value)))
                        color: root.colText
                    }

                    Item { Layout.fillWidth: true }

                    RexonDangerButton {
                        id: ambStopAll
                        pal: root.pal
                        tone: root.sty.danger
                        text: root.txt["amb.stop_all"]
                        onClicked: root.did(root.actions.stopAllAmbient())
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ambStopAll)
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: root.txt["amb.volume_hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                // Список играющих звуков: у виджетов это отдельная колонка с
                // отступом сверху 4 и шагом строк 4, а не строки группы.
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: root.sty.ambient.listTopMargin
                    spacing: root.sty.ambient.listSpacing

                    Text {
                        Layout.fillWidth: true
                        visible: ambBody.active.length === 0
                        text: root.txt["amb.empty"]
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                    }

                    Repeater {
                        model: ambBody.active

                        delegate: Rectangle {
                            id: ambRow
                            required property var modelData

                            Layout.fillWidth: true
                            radius: root.sty.ambient.rowRadius
                            color: root.sty.ambient.rowBackground
                            implicitHeight: ambRowLayout.implicitHeight
                                            + 2 * root.sty.ambient.rowMarginV

                            RowLayout {
                                id: ambRowLayout
                                x: root.sty.ambient.rowMarginH
                                y: root.sty.ambient.rowMarginV
                                width: parent.width - 2 * root.sty.ambient.rowMarginH
                                height: implicitHeight
                                spacing: root.sty.ambient.rowSpacing

                                Text {
                                    Layout.fillWidth: true
                                    text: ambRow.modelData.name || ""
                                    textFormat: Text.RichText
                                    elide: Text.ElideRight
                                    color: root.colText
                                }

                                Text {
                                    text: ambRow.modelData.volume || ""
                                    color: root.colTextSecondary
                                    font.weight: root.pal.text_secondary_weight
                                }

                                RexonButton {
                                    id: ambSetVol
                                    Layout.preferredWidth: root.sty.widths.ambientButton
                                    // Узкая кнопка-значок: боковые поля базовой
                                    // кнопки не влезают в её ширину.
                                    leftPadding: 0
                                    pal: root.pal
                                    text: root.txt["amb.setvol"]
                                    ToolTip.visible: hovered
                                    ToolTip.text: root.txt["amb.setvol_tip"]
                                    onClicked: root.did(root.actions.setAmbientVolume(
                                                            ambRow.modelData.id || "",
                                                            Math.round(ambVolume.value)))
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(ambSetVol)
                                }

                                RexonDangerButton {
                                    id: ambStop
                                    Layout.preferredWidth: root.sty.widths.ambientButton
                                    pal: root.pal
                                    tone: root.sty.ambientStop
                                    text: root.txt["amb.stop"]
                                    ToolTip.visible: hovered
                                    ToolTip.text: root.txt["amb.stop_tip"]
                                    onClicked: root.did(root.actions.stopAmbient(
                                                            ambRow.modelData.id || ""))
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(ambStop)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ══ 🔔 Звуки событий ══════════════════════════════════════════════════

    Component {
        id: cSound

        ColumnLayout {
            id: sndBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var snd: root.st.snd || ({})
            readonly property var showcase: sndBody.snd.showcase || []

            // Тема, которую ДЕЙСТВИТЕЛЬНО можно показать выбранной: пустая
            // строка, если системная тема не задана вовсе или названа так,
            // что среди установленных её нет.
            readonly property string themeValue: {
                var themes = sndBody.snd.themes || [];
                var current = sndBody.snd.current || "";
                return themes.indexOf(current) >= 0 ? current : "";
            }

            // Строки списка тем: подпись = имя темы, значение = оно же.
            // Когда показывать нечего, первой строкой идёт «— не задана —»
            // с пустым значением. Без неё список показывал ПЕРВУЮ
            // установленную тему, и покупатель читал чужое значение как
            // своё.
            readonly property var themeChoices: {
                var out = [];
                if (sndBody.themeValue === "") {
                    // Имя есть, а темы нет — так и сказать: «не задана»
                    // здесь было бы неправдой.
                    var named = sndBody.snd.current || "";
                    out.push({
                        "label": named !== ""
                                 ? (root.txt["snd.theme_missing"] || "").replace("%1", named)
                                 : root.txt["snd.theme_unset"],
                        "value": ""
                    });
                }
                var themes = sndBody.snd.themes || [];
                for (var i = 0; i < themes.length; ++i)
                    out.push({ "label": themes[i], "value": themes[i] });
                return out;
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    Layout.minimumWidth: root.sty.widths.fieldLabel
                    text: root.txt["snd.theme_label"]
                    color: root.colText
                }

                RexonComboBox {
                    id: sndTheme
                    // Имя нужно проверкам: стенд достаёт список
                    // `root.findChild(QObject, "soundThemeCombo")`.
                    objectName: "soundThemeCombo"
                    Layout.fillWidth: true
                    pal: root.pal
                    model: sndBody.themeChoices
                    textRole: "label"
                    valueRole: "value"
                    // Выбор ставится ЗНАЧЕНИЕМ, а не номером строки: договор
                    // самого списка (см. шапку RexonComboBox.qml).
                    selectedValue: sndBody.themeValue
                    // Пустое значение — это строка «— не задана —»: она
                    // ничего не применяет, системная тема остаётся своей.
                    onActivated: if (sndTheme.currentValue)
                                     root.did(root.actions.setSoundTheme(
                                                  sndTheme.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(sndTheme)
                }
            }

            RexonCheckBox {
                id: sndEvents
                pal: root.pal
                text: root.txt["snd.events_check"]
                checked: sndBody.snd.events === true
                onToggled: root.did(root.actions.setEventSounds(sndEvents.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(sndEvents)
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["snd.test_label"]
                color: root.colText
            }

            Text {
                Layout.fillWidth: true
                visible: sndBody.showcase.length === 0
                text: root.txt["snd.no_events"]
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // Кнопки «послушать» переносятся на вторую строку, а не уезжают
            // за край: RowLayout переносить не умеет, и на минимальной ширине
            // окна последняя кнопка обрезалась бы.
            Flow {
                Layout.fillWidth: true
                spacing: root.sty.spacing.soundPreview

                Repeater {
                    model: sndBody.showcase

                    delegate: RexonButton {
                        id: sndButton
                        required property string modelData
                        readonly property var meta: page.eventMeta(sndButton.modelData)

                        pal: root.pal
                        text: sndButton.meta.emoji + " " + sndButton.meta.label
                        ToolTip.visible: hovered
                        ToolTip.text: page.t1("snd.event_tip", sndButton.modelData)
                        onClicked: root.say(root.actions.playSound(
                                                sndBody.snd.current || "",
                                                sndButton.modelData))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(sndButton)
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["snd.themes_hint"]
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🌽 MATE: иконки рабочего стола ════════════════════════════════════

    Component {
        id: cMate

        RowLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            Repeater {
                model: page.mateButtons

                delegate: RexonButton {
                    id: mateButton
                    required property var modelData
                    pal: root.pal
                    text: mateButton.modelData.label
                    onClicked: root.did(root.actions.mateAction(
                                            mateButton.modelData.action))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(mateButton)
                }
            }

            Item { Layout.fillWidth: true }
        }
    }

    // ══ 🔤 Системные шрифты ═══════════════════════════════════════════════

    Component {
        id: cFonts

        ColumnLayout {
            id: fontBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var values: (root.st.font && root.st.font.values) || ({})
            readonly property var render: (root.st.font && root.st.font.render) || ({})
            readonly property var all: (root.st.font && root.st.font.all) || ({})
            readonly property var dpi: (root.st.font && root.st.font.dpi) || ({})
            // Выбор «что менять» переживает обновление снимка после действия.
            property string allMode: "both"

            // Записать отрисовку целиком: каждое изменение отдаёт ядру все
            // значения разом — так в файле не остаётся половины настройки.
            function applyRender(aa, exclude, from, to, subpixel, hinting) {
                root.did(root.actions.setFontRendering(aa, exclude, from, to,
                                                       subpixel, hinting));
            }
            function applyCurrent() {
                applyRender(aaCheck.checked, aaExclude.checked,
                            parseInt(aaFrom.text) || fontBody.render.from,
                            parseInt(aaTo.text) || fontBody.render.to,
                            subpixelCombo.currentValue, hintingCombo.currentValue);
            }

            // Шесть кнопок в ряд не помещаются на узком окне: переносятся.
            Flow {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                Repeater {
                    model: page.fontTargets

                    delegate: ColumnLayout {
                        id: fontCol
                        required property var modelData
                        // Столбец по содержимому, не шире кнопки: подпись
                        // под ней переносится, а не распирает столбец.
                        // С запасом: ровно по `implicitWidth` подпись
                        // кнопки теряла последние буквы («Системны…»,
                        // «Панель инструменто…») — значок-эмодзи шире, чем
                        // его считает раскладка.
                        width: Math.ceil(fontButton.implicitWidth) + 18
                        spacing: root.sty.spacing.fontCol

                        RexonButton {
                            id: fontButton
                            Layout.fillWidth: true
                            pal: root.pal
                            tone: root.sty.primary
                            enabled: fontCol.modelData.available !== false
                            text: fontCol.modelData.label
                            ToolTip.visible: hovered
                            ToolTip.text: fontCol.modelData.tip
                            onClicked: root.did(root.actions.pickFont(
                                                    fontCol.modelData.id))
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(fontButton)
                        }

                        Text {
                            // Ширину столбца задаёт кнопка; подпись под ней
                            // переносится, а не распирает столбец.
                            Layout.fillWidth: true
                            Layout.preferredWidth: 0
                            text: fontBody.values[fontCol.modelData.id]
                                  || root.txt["font.placeholder"]
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                        }
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                text: page.t1("font.de_hint", page.deDisplay)
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // На что действуют «маленький», «панели инструментов» и «меню»
            // там, где у окружения своих таких шрифтов нет.
            Text {
                Layout.fillWidth: true
                visible: text.length > 0
                text: (root.st.font && root.st.font.scope) || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Изменить все шрифты ──────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                RexonButton {
                    id: allFontsButton
                    pal: root.pal
                    tone: root.sty.primary
                    text: fontBody.all.label || ""
                    ToolTip.visible: hovered
                    ToolTip.text: fontBody.all.tip || ""
                    onClicked: root.did(root.actions.changeAllFonts(allModeCombo.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(allFontsButton)
                }
                Text {
                    Layout.leftMargin: root.sty.group.spacing
                    text: root.txt["font.all_mode"]
                    color: root.colText
                }
                RexonComboBox {
                    id: allModeCombo
                    Layout.preferredWidth: 200
                    pal: root.pal
                    model: fontBody.all.modes || []
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: fontBody.allMode
                    onActivated: fontBody.allMode = allModeCombo.currentValue
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(allModeCombo)
                }
                Item { Layout.fillWidth: true }
            }

            // ── Сглаживание, субпиксели, хинтинг ─────────────────────────
            Text {
                Layout.fillWidth: true
                Layout.topMargin: root.sty.group.spacing
                text: root.txt["font.render_title"]
                textFormat: Text.StyledText
                wrapMode: Text.WordWrap
                color: root.colText
            }

            RexonCheckBox {
                id: aaCheck
                pal: root.pal
                text: root.txt["font.aa"]
                checked: fontBody.render.antialias === true
                onToggled: fontBody.applyCurrent()
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaCheck)
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                RexonCheckBox {
                    id: aaExclude
                    pal: root.pal
                    enabled: aaCheck.checked
                    text: root.txt["font.aa_exclude"]
                    checked: fontBody.render.exclude === true
                    onToggled: fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaExclude)
                }
                RexonTextField {
                    id: aaFrom
                    Layout.preferredWidth: 64
                    pal: root.pal
                    enabled: aaCheck.checked && aaExclude.checked
                    text: String(fontBody.render.from || "")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: fontBody.render.min || 1
                        top: fontBody.render.max || 99
                    }
                    onEditingFinished: if (parseInt(text) !== fontBody.render.from)
                        fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaFrom)
                }
                Text {
                    text: root.txt["font.aa_to"]
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }
                RexonTextField {
                    id: aaTo
                    Layout.preferredWidth: 64
                    pal: root.pal
                    enabled: aaCheck.checked && aaExclude.checked
                    text: String(fontBody.render.to || "")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: fontBody.render.min || 1
                        top: fontBody.render.max || 99
                    }
                    onEditingFinished: if (parseInt(text) !== fontBody.render.to)
                        fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaTo)
                }
                Text {
                    text: root.txt["font.pt"]
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }
                Item { Layout.fillWidth: true }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                Text {
                    text: root.txt["font.subpixel"]
                    color: root.colText
                }
                RexonComboBox {
                    id: subpixelCombo
                    Layout.preferredWidth: 200
                    pal: root.pal
                    enabled: aaCheck.checked
                    model: fontBody.render.subpixels || []
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: fontBody.render.subpixel || ""
                    onActivated: fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(subpixelCombo)
                }
                Text {
                    Layout.leftMargin: root.sty.group.spacing
                    text: root.txt["font.hinting"]
                    color: root.colText
                }
                RexonComboBox {
                    id: hintingCombo
                    Layout.preferredWidth: 160
                    pal: root.pal
                    model: fontBody.render.hintings || []
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: fontBody.render.hinting || ""
                    onActivated: fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(hintingCombo)
                }
                Item { Layout.fillWidth: true }
            }

            Text {
                Layout.fillWidth: true
                text: root.txt["font.render_hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Принудительное DPI шрифтов ───────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: root.sty.group.spacing
                spacing: root.sty.spacing.fontRow

                RexonCheckBox {
                    id: dpiCheck
                    pal: root.pal
                    enabled: fontBody.dpi.available === true
                    text: root.txt["font.dpi"]
                    checked: fontBody.dpi.enabled === true
                    onToggled: root.did(root.actions.setFontDpi(
                                            dpiCheck.checked,
                                            parseInt(dpiField.text) || fontBody.dpi.value))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(dpiCheck)
                }
                RexonTextField {
                    id: dpiField
                    Layout.preferredWidth: 72
                    pal: root.pal
                    enabled: dpiCheck.enabled && dpiCheck.checked
                    text: String(fontBody.dpi.value || "")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: fontBody.dpi.min || 48
                        top: fontBody.dpi.max || 480
                    }
                    onEditingFinished: if (parseInt(text) !== fontBody.dpi.value)
                        root.did(root.actions.setFontDpi(true, parseInt(text)))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(dpiField)
                }
                Item { Layout.fillWidth: true }
            }

            Text {
                Layout.fillWidth: true
                visible: text.length > 0
                text: fontBody.dpi.note || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Управление шрифтами (свой компонент) ─────────────────────
            FontManagerSection {
                Layout.fillWidth: true
                Layout.topMargin: root.sty.group.spacing
                actions: root.actions
                pal: root.pal
                sty: root.sty
                onMessage: text => root.say(text)
                onFocused: item => root.ensureVisible(item)
            }
        }
    }

    // ══ 🔒 Защита приложений по умолчанию ═════════════════════════════════

    Component {
        id: cProtection

        ColumnLayout {
            id: protBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var prot: root.st.prot || ({})

            RexonCheckBox {
                id: protMain
                pal: root.pal
                text: root.txt["prot.enable"]
                checked: protBody.prot.enabled === true
                onToggled: root.did(root.actions.setProtection("enabled",
                                                               protMain.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(protMain)
            }

            Repeater {
                model: page.protectionTypes

                delegate: RexonCheckBox {
                    id: protRow
                    required property var modelData
                    readonly property var meta: page.protectionRow(
                        protRow.modelData.id,
                        (protBody.prot.remembered
                         && protBody.prot.remembered[protRow.modelData.id]) || "")

                    Layout.fillWidth: true
                    pal: root.pal
                    text: protRow.meta.label
                    ToolTip.visible: hovered && protRow.meta.tip !== ""
                    ToolTip.text: protRow.meta.tip
                    checked: (protBody.prot.rules
                              && protBody.prot.rules[protRow.modelData.id]) === true
                    onToggled: root.did(root.actions.setProtection(
                                            protRow.modelData.id, protRow.checked))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(protRow)
                }
            }

            // Без утилиты xdg-terminal-exec выбор терминала записан, но часть
            // программ откроет свой — говорим об этом прямо.
            Text {
                Layout.fillWidth: true
                visible: (protBody.prot.note || "") !== ""
                text: protBody.prot.note || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
                ToolTip.visible: protNoteHover.hovered
                                 && (protBody.prot.noteTip || "") !== ""
                ToolTip.text: protBody.prot.noteTip || ""
                HoverHandler { id: protNoteHover }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.infoBox

                    Text {
                        Layout.fillWidth: true
                        text: root.txt["prot.snapshot_title"]
                        color: root.colText
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.txt["prot.snapshot_sub"]
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                    }
                }

                RexonButton {
                    id: protSnapshot
                    pal: root.pal
                    tone: root.sty.primary
                    text: root.txt["prot.snapshot"]
                    onClicked: root.did(root.actions.snapshot())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(protSnapshot)
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    text: protBody.prot.status || ""
                    wrapMode: Text.WordWrap
                    color: root.colText
                }

                RexonButton {
                    id: protRestore
                    pal: root.pal
                    tone: root.sty.primary
                    enabled: protBody.prot.canRestore === true
                    text: root.txt["prot.restore"]
                    onClicked: root.did(root.actions.restoreDefaults())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(protRestore)
                }
            }
        }
    }

    // ══ 🛡 Защита панели KDE Plasma ═══════════════════════════════════════

    Component {
        id: cPlasma

        ColumnLayout {
            id: plasmaBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var plasma: root.st.plasma || ({})
            readonly property var backups: plasmaBody.plasma.backups || []

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.infoBox

                    Text {
                        Layout.fillWidth: true
                        text: root.txt["plasma.backup_title"]
                        color: root.colText
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.txt["plasma.backup_sub"]
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                        ToolTip.visible: plasmaSubHover.hovered
                        ToolTip.text: root.txt["plasma.backup_tip"]
                        HoverHandler { id: plasmaSubHover }
                    }
                }

                RexonButton {
                    id: plasmaBackup
                    pal: root.pal
                    tone: root.sty.primary
                    text: root.txt["plasma.backup"]
                    onClicked: root.did(root.actions.plasmaBackup())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(plasmaBackup)
                }
            }

            Text {
                Layout.fillWidth: true
                visible: plasmaBody.backups.length > 0
                text: plasmaBody.plasma.header || ""
                color: root.colText
            }

            Text {
                Layout.fillWidth: true
                visible: plasmaBody.backups.length === 0
                text: root.txt["plasma.no_backups"]
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            Repeater {
                model: plasmaBody.backups

                delegate: RowLayout {
                    id: backupRow
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.plasmaRow

                    Text {
                        Layout.fillWidth: true
                        text: backupRow.modelData.label || ""
                        elide: Text.ElideRight
                        color: root.colText
                        ToolTip.visible: backupHover.hovered
                        ToolTip.text: backupRow.modelData.tip || ""
                        HoverHandler { id: backupHover }
                    }

                    RexonButton {
                        id: backupRestore
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["plasma.restore"]
                        onClicked: root.askPlasmaRestore(backupRow.modelData.ts || "")
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(backupRestore)
                    }
                }
            }
        }
    }

    // ══ 🪟 Оформление рабочего стола ══════════════════════════════════════

    Component {
        id: cAero

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonCheckBox {
                id: aeroCheck
                Layout.fillWidth: true
                pal: root.pal
                text: root.txt["aero.check"]
                ToolTip.visible: hovered
                ToolTip.text: root.txt["aero.check_tip"]
                checked: (root.st.aero && root.st.aero.enabled) === true
                onToggled: root.did(root.actions.setAeroshell(aeroCheck.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(aeroCheck)
            }

            RexonButton {
                id: aeroNow
                // Кнопка тянется во всю ширину раздела: у оригинала она
                // добавлена в вертикальную раскладку группы (`gl.addWidget`),
                // а та растягивает виджет по ширине. Без этого QML рисовал
                // её по содержимому — 167 точек вместо 1318 (замер 05.09).
                Layout.fillWidth: true
                pal: root.pal
                tone: root.sty.primary
                text: root.txt["aero.check_now"]
                onClicked: root.did(root.actions.aeroshellCheckNow())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(aeroNow)
            }
        }
    }

    // ── Подтверждение возврата копии панели ───────────────────────────────

    function askPlasmaRestore(ts) {
        if (!root.actions || !ts)
            return;
        const confirm = JSON.parse(root.actions.plasmaConfirm(ts));
        confirmDialog.timestamp = ts;
        confirmDialog.heading = confirm.title;
        confirmDialog.question = confirm.question;
        confirmDialog.open();
    }

    Popup {
        id: confirmDialog

        property string timestamp: ""
        property string heading: ""
        property string question: ""

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
        // (`ui_base::MODAL_DIM_RGBA`, ключ `modal_dim`): тем же цветом гасит
        // окно обучение и «Сравнение DE». Стиль по умолчанию фон не гасит, и
        // окно выходило модальным на деле, но не на вид.
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
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                text: confirmDialog.question
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colText
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: confirmYes
                    pal: root.pal
                    highlighted: true
                    text: root.txt["dialog.yes"]
                    onClicked: {
                        const ts = confirmDialog.timestamp;
                        confirmDialog.close();
                        root.did(root.actions.plasmaRestore(ts));
                    }
                }

                RexonButton {
                    id: confirmNo
                    pal: root.pal
                    text: root.txt["dialog.no"]
                    onClicked: confirmDialog.close()
                }
            }
        }

        // Фокус при открытии — на безопасном ответе: случайный Enter не
        // должен возвращать копию панели (защита от дурака).
        onOpened: confirmNo.forceActiveFocus()
    }

    // ── Всплывающее сообщение ─────────────────────────────────────────────
    //
    // Плашка общая на весь продукт (`RexonToast.qml`): там же и таймер
    // гашения — вкладке остаётся только позвать `show`.

    // ── Крупный просмотр картинки обоев ───────────────────────────────────
    //
    // Слой общий на весь продукт (`RexonImageViewer.qml`). Листание — по
    // обоям библиотеки, у которых есть картинка.
    function openLibraryViewer(all, index) {
        const items = [];
        let at = 0;
        for (let i = 0; i < all.length; i++) {
            const wp = all[i];
            if (!wp.preview)
                continue;
            if (i === index)
                at = items.length;
            items.push({
                "key": wp.path || String(i),
                "title": wp.name || root.txt["lib.unnamed"],
                "subtitle": wp.formatLabel || "",
                "thumb": wp.preview,
                "source": wp.preview
            });
        }
        viewer.show(items, at);
    }

    RexonImageViewer {
        id: viewer
        objectName: "imageViewer"
        pal: root.pal
    }

    RexonToast {
        id: toast
        skin: root.sty.toast
        timeoutMs: page.toastTimeoutMs
        metrics: page
        availableWidth: root.width
    }

    // ── Перечитывание списка играющих фоновых звуков ──────────────────────
    // Список меняется и без нас (звук доиграл, его остановили снаружи).

    Timer {
        interval: page.ambientRefreshMs
        running: root.visible && root.actions !== null && root.fullPage
        repeat: true
        onTriggered: root.reloadAmbient()
    }

    // ── Опрос права изменять систему (Б13) ────────────────────────────────
    // Право гаснет и без нас (простой, блокировка экрана, сон), а
    // появляется после пароля в любой вкладке — раз в две секунды, только
    // пока страница видна. Опрос дешёвый: флаг в памяти процесса.

    Timer {
        interval: 2000
        running: root.visible && root.actions !== null && root.fullPage
        repeat: true
        triggeredOnStart: true
        onTriggered: root.reloadAdmin()
    }

    // ── Окно доустановки недостающего ─────────────────────────────────────
    //
    // «Проверить и доустановить»: вопрос со списком недостающего → ход дела
    // → итог. То же окно, что у «Рабочих столов» (`DeOperationDialog.qml`),
    // и тот же исполнитель ядра (`system_de_exec`). У Python-объекта
    // действий свойства `operation` нет — окно тогда просто не показывается.

    DeOperationDialog {
        anchors.fill: parent
        actions: root.actions
        pal: root.pal
    }

    // ── Фокус с первого кадра ─────────────────────────────────────────────

    // ⚠️ Фокус берёт ПЕРВЫЙ КОНТРОЛ, а не корень страницы: `forceActiveFocus`
    // на корне делает `activeFocusItem` безымянным Item'ом, и страница
    // мертва и с клавиатуры, и с геймпада.
    property int focusAttempts: 0

    function focusFirstControl() {
        // Раздел внутри «Оформления» фокус себе не забирает: он остаётся в
        // боковой панели, откуда человек раздел выбрал (геймпад и Tab).
        if (!root.fullPage)
            return;
        if (sectionRepeater.count > 0) {
            const first = root.nextItemInFocusChain(true);
            if (first && first !== root) {
                root.focusAttempts = 0;
                focusRetry.stop();
                first.forceActiveFocus();
                return;
            }
        }
        // Разделы наполняются не мгновенно. Пробуем ещё раз, но не
        // бесконечно: пустой список разделов — это поломка ядра, и вечный
        // самовызов её только спрячет.
        if (root.focusAttempts < 40) {
            root.focusAttempts += 1;
            focusRetry.restart();
        } else {
            console.warn("вкладка «Настройки»: разделов нет, фокус взять не с чего");
        }
    }

    // ⚠️ Починка при переносе: повтор идёт ПО ВРЕМЕНИ, а не через
    // `Qt.callLater`. `callLater` сворачивает вызовы в тот же проход цикла
    // событий, поэтому все сорок попыток сгорали ДО того, как Repeater успевал
    // построить хоть один раздел, — и страница с шестнадцатью разделами честно
    // писала в журнал «разделов нет, фокус взять не с чего», а фокус так и не
    // брала. Под Python это оставалось незаметным: там объект действий
    // подставлялся позже, и цепочка вовсе не запускалась.
    // 40 × 50 мс = 2 с на построение — замер на этой машине: 1 проход.
    Timer {
        id: focusRetry
        interval: 50
        onTriggered: root.focusFirstControl()
    }

    /// Спросить машину и построить по её ответу страницу.
    ///
    /// Сначала факты (от них зависит сам СОСТАВ разделов), потом состояние,
    /// потом фокус: до фактов разделов нет, и брать фокус не с чего.
    function start() {
        if (!root.actions)
            return;
        page.configure(root.actions.facts());
        root.reload();
        root.focusFirstControl();
    }

    /// Объект действий появился — только теперь можно спросить машину.
    ///
    /// Срабатывает и на СВОЁМ объекте (его создаёт сама страница), и на
    /// подставленном снаружи: так вкладку держит живой Python-продукт — он
    /// кладёт в `actions` свой объект уже после создания страницы.
    onActionsChanged: root.start()

    // Во встроенной вкладке страница создаётся заранее и оживает при
    // переключении на неё: состояние могло измениться в других вкладках.
    onVisibleChanged: if (root.visible) {
        root.reload();
        root.focusFirstControl();
    }

    // В «Оформлении» один экземпляр показывает разные разделы: шрифтам
    // нужен свежий снимок, когда на них перешли.
    onPanelChanged: root.reload()
}
