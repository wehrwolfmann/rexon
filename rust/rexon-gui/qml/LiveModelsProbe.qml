// Живые модели в настоящем QML — то, чего не видит ни `cargo test`, ни
// безоконные стенды `--remote-selftest` / `--discover-selftest`.
//
// Сценарии жили в стенд-окне `qml/Main.qml` (бинарь `rexon-gui-demo`) и
// сняты вместе с ним коммитом a669701. Сюда они вернулись без окна-витрины:
// один документ, сценарий выбирается начальным свойством `scenario`, после
// приговора процесс выходит сам (`Qt.exit`). Бегун — `src/bin/selftest.rs`,
// ключи `--remote-model-signals-selftest`, `--user-property-selftest`,
// `--toast-selftest`, `--image-viewer-selftest`; в `cargo test -p rexon-gui`
// их зовёт
// `tests/selftest_scenarios.rs`.
//
// Что сторожится и почему только здесь:
//   * `remote` — сетка Discover над `RemoteListModel`: СТРУКТУРНЫЕ сигналы
//     `modelReset`/`rowsInserted` при подаче и догрузке страниц (у Qt они
//     приватные, из Rust на них не подписаться, а из QML можно), точечные
//     `dataChanged` с одной ролью и перепривязка ролей в ЖИВОМ делегате
//     карточки после подписки и после дозаполнения авторов;
//   * `userprop` — редактор свойств обоев над `UserPropertyListModel`: все
//     строки по всем ролям доезжают до QML, `dataChanged` несёт ровно две
//     зависимые от значения роли, `valueChanged`/`resetRequested` доходят до
//     QML, правка и сброс перепривязывают живой делегат;
//   * `toast` — плашка состояния `RexonToast` гаснет сама (дефект «сообщение
//     не гаснет никогда» заводился трижды);
//   * `storefiles` — выбор файла у товара магазина (`StoreFileChoice`):
//     по умолчанию выбран автоматический файл ядра и помечен «будет
//     установлен», настоящий щелчок выбирает другой (`differs`), новый
//     список возвращает автоматический выбор, у одного файла блока нет;
//   * `viewer` — крупный просмотр картинки (`RexonImageViewer` +
//     `RexonZoomArea`): настоящий щелчок по миниатюре открывает просмотр и
//     не доходит до карточки, ← → и кнопки листают, Esc и щелчок мимо
//     картинки закрывают, у карточки с занятым щелчком просмотр открывает
//     только лупа, с клавиатуры — Enter.
//
// Окно видимое намеренно: делегаты ListView/GridView создаются в цикле
// отрисовки, у невидимого окна их нет вовсе. Бегун ставит offscreen сам,
// если платформа не задана, — на живой стол окно не попадает.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import org.rexon.gui 1.0

Window {
    id: root
    width: 900
    height: 600
    visible: true
    title: "Rexon · проверка живых моделей"
    color: "#1b2838"

    /// Какой сценарий гонять: "remote" | "userprop" | "toast" | "viewer" |
    /// "storefiles" | "steambg" | "steamwiz". Ставится
    /// бегуном через setInitialProperties — до первой привязки.
    property string scenario: ""

    /// Строки блока выбора файла от ядра (`ap::store_file_choices_json`):
    /// два файла и один. Ставит бегун.
    property string storeFilesJson: "[]"
    property string storeFilesSingleJson: "[]"

    /// Список провалов: пусто — зелёно.
    property var failures: []

    function check(condition, message) {
        if (!condition)
            root.failures.push(message);
    }

    function checkEqual(actual, expected, message) {
        root.check(actual === expected, message + ": получено «" + actual
                   + "», ожидалось «" + expected + "»");
    }

    // ── ⭐ Сторож пустоты: «пустая выборка = провал» ───────────────────────
    // Стенд, который сравнивает «столько же, сколько строк» на нуле строк,
    // зеленеет, не проверив ничего (найдено 19.09 на сценарии Мастерской).
    // Поэтому сравнение счётчиков само краснеет, когда сравнивать нечего, а
    // каждый сценарий объявляет выборку, и ноль в ней валит прогон.

    function checkCount(actual, expected, message) {
        if (expected <= 0) {
            root.failures.push(message + ": выборка пуста (ожидалось "
                               + expected + ") — сверка прошла бы впустую");
            return;
        }
        root.checkEqual(actual, expected, message);
    }

    function checkPopulations(cells) {
        var shown = [];
        for (var i = 0; i < cells.length; ++i) {
            var size = cells[i].size();
            shown.push(cells[i].what + "=" + size);
            root.check(size > 0, "ПУСТАЯ ВЫБОРКА · " + cells[i].what
                       + ": сценарий шёл на нуле элементов и проверил ровно ничего");
        }
        console.log("REXON-LIVE-POPULATION " + shown.join(" · "));
    }

    function verdict() {
        var ok = root.failures.length === 0;
        for (var i = 0; i < root.failures.length; ++i)
            console.log("REXON-LIVE-FAIL " + root.failures[i]);
        console.log("REXON-LIVE-VERDICT " + root.scenario + " " + (ok ? "OK" : "FAIL")
                    + " failures=" + root.failures.length);
        Qt.exit(ok ? 0 : 1);
    }

    // Сторож срока внутри цикла событий: сценарий, застрявший на шаге,
    // обязан покраснеть, а не висеть. У бегуна свой сторож — на случай,
    // если QML не загрузился вовсе.
    Timer {
        interval: 20000
        running: true
        onTriggered: {
            root.failures.push("сценарий «" + root.scenario + "» не дошёл до приговора за 20 с");
            root.verdict();
        }
    }

    Component.onCompleted: {
        if (root.scenario === "remote") {
            remote.applyPageJson(JSON.stringify(root.remotePageOne), true, false);
            remoteSteps.running = true;
        } else if (root.scenario === "userprop") {
            // Схема ставится до шагов: модель собирает строки в сеттере
            // свойства, ровно как привязка вендорного QML к
            // wallpaper.userPropertiesSchema.
            props.schemaJson = JSON.stringify(root.schemaFixture);
            propSteps.running = true;
        } else if (root.scenario === "toast") {
            toastSteps.running = true;
        } else if (root.scenario === "viewer") {
            viewerSteps.running = true;
        } else if (root.scenario === "storefiles") {
            storeFilesSteps.running = true;
        } else if (root.scenario === "steambg") {
            root.steamPlanStart("фоны", root.steamBgPlan);
        } else if (root.scenario === "steamwiz") {
            root.steamPlanStart("помощник", root.steamWizPlan);
        } else {
            root.failures.push("неизвестный сценарий «" + root.scenario + "»");
            Qt.callLater(root.verdict);
        }
    }

    // ══ Редактор свойств обоев ═════════════════════════════════════════════

    UserPropertyListModel { id: props }

    // Схема-образец — ровно та, на которой снят голден
    // tests/golden/user_property_cases.json (случай «полная схема без
    // оверрайдов»); ожидания ниже взяты оттуда, то есть Qt-слой сверяется не
    // сам с собой.
    readonly property var schemaFixture: ({
        "waywallen.scheme_color": { "text": "Цвет схемы", "type": "color",
                                    "value": [0.2, 0.4, 0.6, 1.0] },
        "waywallen.enable_audio": { "text": "", "type": "bool", "value": true },
        "brightness": { "text": "Яркость", "type": "slider", "value": 0.5,
                        "min": 0, "max": 2, "order": 2 },
        "speed": { "type": "slider", "value": 1234567, "min": 0.5, "max": 0, "order": 1 },
        "mode": { "text": "Режим", "type": "combo", "value": "day", "order": 3,
                  "options": [{ "label": "День", "value": "day" },
                              { "value": "night" },
                              { "label": "", "value": 3 }] },
        "caption": { "text": "Надпись", "type": "textinput", "value": "привет", "order": 3 },
        "tint": { "text": "Оттенок", "type": "color", "value": "0.1 0.2 0.3", "order": 0 },
        "hidden": { "text": "Странный", "type": "scenetexture", "value": { "a": 1 }, "order": -1 },
        "flag": { "type": "bool", "value": 0, "order": 5 },
        "broken": "не объект",
        "listval": { "type": "text", "value": [1, 2.5], "order": 4 },
        "combo_empty": { "text": "Пустой список", "type": "combo", "value": "x", "order": 6 }
    })

    // Ожидаемое содержимое ВСЕХ строк и ВСЕХ ролей, порядок строк тоже.
    // ⚠️ Секции — технические ключи вендора (`user_property_model::SECTION_*`),
    // а не подписи: по ним сравнивают, а показывают перевод.
    readonly property var expectedRows: [
        { key: "waywallen.scheme_color", label: "Цвет схемы", type: "color",
          section: "property", kind: "property", supported: true,
          minVal: 0, maxVal: 1, currentValue: "0.2000 0.4000 0.6000 1.0000",
          hasAlpha: true, optionLabels: [], optionValues: [] },
        { key: "waywallen.enable_audio", label: "Enable audio", type: "bool",
          section: "property", kind: "property", supported: true,
          minVal: 0, maxVal: 1, currentValue: "true", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "hidden", label: "Странный", type: "scenetexture",
          section: "user", kind: "user", supported: false,
          minVal: 0, maxVal: 1, currentValue: "", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "tint", label: "Оттенок", type: "color",
          section: "user", kind: "user", supported: true,
          minVal: 0, maxVal: 1, currentValue: "0.1 0.2 0.3", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "broken", label: "broken", type: "",
          section: "user", kind: "user", supported: false,
          minVal: 0, maxVal: 1, currentValue: "", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "speed", label: "speed", type: "slider",
          section: "user", kind: "user", supported: true,
          minVal: 0.5, maxVal: 1, currentValue: "1.23457e+06", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "brightness", label: "Яркость", type: "slider",
          section: "user", kind: "user", supported: true,
          minVal: 0, maxVal: 2, currentValue: "0.5", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "mode", label: "Режим", type: "combo",
          section: "user", kind: "user", supported: true,
          minVal: 0, maxVal: 1, currentValue: "day", hasAlpha: false,
          optionLabels: ["День", "night", "3"], optionValues: ["day", "night", "3"] },
        { key: "caption", label: "Надпись", type: "textinput",
          section: "user", kind: "user", supported: true,
          minVal: 0, maxVal: 1, currentValue: "привет", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "listval", label: "listval", type: "text",
          section: "user", kind: "user", supported: true,
          minVal: 0, maxVal: 1, currentValue: "1.0000 2.5000", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "flag", label: "flag", type: "bool",
          section: "user", kind: "user", supported: true,
          minVal: 0, maxVal: 1, currentValue: "false", hasAlpha: false,
          optionLabels: [], optionValues: [] },
        { key: "combo_empty", label: "Пустой список", type: "combo",
          section: "user", kind: "user", supported: false,
          minVal: 0, maxVal: 1, currentValue: "x", hasAlpha: false,
          optionLabels: [], optionValues: [] }
    ]

    // Роли, которые обязаны приезжать в dataChanged редактора: ровно две
    // зависимые от значения, ни больше ни меньше.
    readonly property int currentValueRole: props.roleNumber("currentValue")
    readonly property int hasAlphaRole: props.roleNumber("hasAlpha")

    property int propPointUpdates: 0   // dataChanged по ОДНОЙ строке
    property int propRangeUpdates: 0   // dataChanged по диапазону строк
    property int propWideUpdates: 0    // не ровно две ожидаемые роли — провал
    property int propValueChanged: 0
    property var propValueLast: ["", ""]
    property var propResetKeys: []
    // Значение, прочитанное из ЖИВОГО делегата строки «tint».
    property string tintDelegateValue: "(делегат не создан)"
    property int tintDelegateUpdates: 0
    property int propStep: 0

    Connections {
        target: props
        function onDataChanged(topLeft, bottomRight, roles) {
            if (roles.length !== 2
                    || roles[0] !== root.currentValueRole
                    || roles[1] !== root.hasAlphaRole) {
                root.propWideUpdates += 1;
            } else if (topLeft.row === bottomRight.row) {
                root.propPointUpdates += 1;
            } else {
                root.propRangeUpdates += 1;
            }
        }
        function onValueChanged(key, value) {
            root.propValueChanged += 1;
            root.propValueLast = [key, value];
        }
        function onResetRequested(key) {
            root.propResetKeys.push(key);
        }
    }

    function checkPropRows() {
        root.checkCount(props.count, root.expectedRows.length, "число строк редактора");
        for (var i = 0; i < root.expectedRows.length && i < props.count; ++i) {
            var want = root.expectedRows[i];
            var at = "строка " + i + " (" + want.key + ")";
            root.checkEqual(props.roleValue(i, "key"), want.key, at + " · key");
            root.checkEqual(props.roleValue(i, "label"), want.label, at + " · label");
            root.checkEqual(props.roleValue(i, "type"), want.type, at + " · type");
            root.checkEqual(props.roleValue(i, "section"), want.section, at + " · section");
            root.checkEqual(props.roleValue(i, "kind"), want.kind, at + " · kind");
            root.checkEqual(props.roleValue(i, "supported"), want.supported, at + " · supported");
            root.checkEqual(props.roleValue(i, "minVal"), want.minVal, at + " · minVal");
            root.checkEqual(props.roleValue(i, "maxVal"), want.maxVal, at + " · maxVal");
            root.checkEqual(props.roleValue(i, "currentValue"), want.currentValue,
                            at + " · currentValue");
            root.checkEqual(props.roleValue(i, "hasAlpha"), want.hasAlpha, at + " · hasAlpha");
            root.checkEqual(JSON.stringify(props.roleValue(i, "optionLabels")),
                            JSON.stringify(want.optionLabels), at + " · optionLabels");
            root.checkEqual(JSON.stringify(props.roleValue(i, "optionValues")),
                            JSON.stringify(want.optionValues), at + " · optionValues");
        }
    }

    // Шаги разнесены по таймеру: между ними обязан прокрутиться цикл
    // событий, иначе делегаты не успевают перепривязаться и проверка «дошло
    // до вида» ничего не значит.
    Timer {
        id: propSteps
        interval: 150
        repeat: true
        running: false
        onTriggered: root.propertyStep()
    }

    function propertyStep() {
        root.propStep += 1;
        if (root.propStep === 1) {
            root.checkPropRows();
            root.checkEqual(props.roleNumber("такойРолиНет"), -1,
                            "несуществующая роль редактора");
            root.checkEqual(props.hasPredefinedPropertyOverrides, false,
                            "до оверрайдов: hasPredefinedPropertyOverrides");
            root.checkEqual(props.hasUserPropertyOverrides, false,
                            "до оверрайдов: hasUserPropertyOverrides");
            // Значение делегата ДО правок не проверяется: сработает ли
            // onCurrentValueChanged при создании делегата — деталь
            // инициализации QML. Детерминированы ИЗМЕНЕНИЯ — правка и сброс.
            // Оверрайды приходят от демона строкой; пустая строка у caption —
            // валидное значение, а не сброс.
            props.overridesJson = JSON.stringify({
                "waywallen.scheme_color": "1.0000 0.0000 0.0000 0.5000",
                "caption": ""
            });
        } else if (root.propStep === 2) {
            root.checkEqual(root.propRangeUpdates, 1, "оверрайды: dataChanged по диапазону");
            root.checkEqual(root.propValueChanged, 0,
                            "оверрайды НЕ должны поднимать valueChanged");
            root.checkEqual(props.roleValue(0, "currentValue"),
                            "1.0000 0.0000 0.0000 0.5000", "оверрайд цвета схемы");
            root.checkEqual(props.roleValue(8, "currentValue"), "",
                            "пустая строка — валидное значение caption");
            root.checkEqual(props.hasPredefinedPropertyOverrides, true,
                            "после оверрайдов: hasPredefinedPropertyOverrides");
            root.checkEqual(props.hasUserPropertyOverrides, true,
                            "после оверрайдов: hasUserPropertyOverrides");
            props.setValue("tint", "0.9000 0.8000 0.7000 0.6000");
        } else if (root.propStep === 3) {
            root.checkEqual(root.propPointUpdates, 1, "правка: точечный dataChanged");
            root.checkEqual(root.propValueChanged, 1, "правка: valueChanged");
            root.checkEqual(root.propValueLast[0], "tint", "valueChanged · ключ");
            root.checkEqual(root.propValueLast[1], "0.9000 0.8000 0.7000 0.6000",
                            "valueChanged · значение");
            root.checkEqual(props.roleValue(3, "hasAlpha"), true,
                            "hasAlpha после правки цвета с альфой");
            // Главное доказательство Qt-слоя: значение доехало до ЖИВОГО
            // делегата, а не только до модели.
            root.checkEqual(root.tintDelegateValue, "0.9000 0.8000 0.7000 0.6000",
                            "делегат строки tint после правки");
            props.resetUserProperties();
        } else if (root.propStep === 4) {
            propSteps.running = false;
            root.checkEqual(root.propResetKeys.length, 2, "сброс: число ключей");
            root.checkEqual(root.propResetKeys[0], "tint", "сброс · первый ключ");
            root.checkEqual(root.propResetKeys[1], "caption", "сброс · второй ключ");
            root.checkEqual(props.roleValue(3, "currentValue"), "0.1 0.2 0.3",
                            "tint вернулся к значению по умолчанию");
            root.checkEqual(props.roleValue(8, "currentValue"), "привет",
                            "caption вернулся к значению по умолчанию");
            root.checkEqual(props.hasUserPropertyOverrides, false,
                            "после сброса: hasUserPropertyOverrides");
            root.checkEqual(props.hasPredefinedPropertyOverrides, true,
                            "сброс пользовательских НЕ трогает предопределённые");
            root.checkEqual(root.tintDelegateValue, "0.1 0.2 0.3",
                            "делегат строки tint после сброса");
            // Две перепривязки минимум — правка и сброс; больше допустимо
            // (при создании делегата QML может прислать ещё одну).
            root.check(root.tintDelegateUpdates >= 2,
                       "делегат строки tint получил меньше двух перепривязок: "
                       + root.tintDelegateUpdates);
            root.check(root.propWideUpdates === 0,
                       "dataChanged редактора не с двумя ожидаемыми ролями: "
                       + root.propWideUpdates);
            root.checkCount(propList.count, props.count, "делегатов редактора меньше строк");
            root.checkPopulations([
                { what: "строк модели", size: function () { return props.count; } },
                { what: "делегатов", size: function () { return propList.count; } },
                { what: "ожиданий в списке сверки",
                  size: function () { return root.expectedRows.length; } }
            ]);
            console.log("REXON-LIVE-USERPROP rows=" + props.count
                        + " delegates=" + propList.count
                        + " pointUpdates=" + root.propPointUpdates
                        + " rangeUpdates=" + root.propRangeUpdates
                        + " wideUpdates=" + root.propWideUpdates
                        + " valueChanged=" + root.propValueChanged
                        + " resetRequested=" + JSON.stringify(root.propResetKeys)
                        + " delegateTintUpdates=" + root.tintDelegateUpdates
                        + " delegateTint=" + root.tintDelegateValue);
            root.verdict();
        }
    }

    // Упрощённый двойник панели деталей вендорного QML: те же роли, те же
    // секции. Редакторов по типам нет — проверяется мост, а не виджеты.
    ListView {
        id: propList
        x: 10
        y: 10
        width: 420
        height: root.height - 20
        clip: true
        model: root.scenario === "userprop" ? props : null
        // Все строки обязаны быть живыми: иначе перепривязку в делегате
        // нижней строки проверить нечем.
        cacheBuffer: 100000

        section.property: "section"
        section.criteria: ViewSection.FullString
        section.delegate: Text {
            required property string section
            text: section
            color: "#66c0f4"
        }

        delegate: Rectangle {
            id: propRow
            required property string key
            required property string label
            required property string currentValue
            required property bool supported
            required property bool hasAlpha

            width: ListView.view.width
            height: 30
            color: "#16202d"

            onCurrentValueChanged: {
                if (propRow.key !== "tint")
                    return;
                root.tintDelegateUpdates += 1;
                root.tintDelegateValue = propRow.currentValue;
            }

            Text {
                text: propRow.label + " = " + propRow.currentValue
                      + (propRow.hasAlpha ? " (альфа)" : "")
                color: propRow.supported ? "#ffffff" : "#9fb4c6"
            }
        }
    }

    // ══ Выдача Discover ════════════════════════════════════════════════════

    RemoteListModel { id: remote }

    // Две страницы «как от источника»: первая с догрузкой, вторая
    // закрывающая. acquisitionState 1/2 — «подписаться»/«отписаться».
    readonly property var remotePageOne: [
        { itemId: "w1", sourceId: "workshop", title: "Ночной город",
          previewUrl: "https://example.invalid/w1.jpg", author: "",
          wpType: "scene", acquisitionState: 1 },
        { itemId: "w2", sourceId: "workshop", title: "Дождь по стеклу",
          previewUrl: "https://example.invalid/w2.jpg", author: "",
          wpType: "video", acquisitionState: 1 },
        { itemId: "w3", sourceId: "workshop", title: "Космический лифт",
          previewUrl: "https://example.invalid/w3.jpg", author: "",
          wpType: "web", acquisitionState: 1 },
        { itemId: "w4", sourceId: "workshop", title: "Северное сияние",
          previewUrl: "https://example.invalid/w4.jpg", author: "",
          wpType: "scene", acquisitionState: 2 }
    ]
    readonly property var remotePageTwo: [
        { itemId: "w5", sourceId: "workshop", title: "Тихий лес",
          previewUrl: "https://example.invalid/w5.jpg", author: "",
          wpType: "scene", acquisitionState: 1 },
        { itemId: "w6", sourceId: "workshop", title: "Маяк",
          previewUrl: "https://example.invalid/w6.jpg", author: "",
          wpType: "scene", acquisitionState: 1 }
    ]

    readonly property int acquisitionRole: remote.roleNumber("acquisitionState")
    readonly property int remoteAuthorRole: remote.roleNumber("author")
    // Журнал сигналов модели В ПОРЯДКЕ ПРИХОДА — включая структурные.
    property var remoteJournal: []
    property int remotePointUpdates: 0   // dataChanged по одной строке
    property int remoteWideUpdates: 0    // не одна ожидаемая роль — провал
    property int remoteFetchRequests: 0
    // true, если догрузку запросил САМ вид, а не сценарий.
    property bool remoteAutoFetch: false
    property int remoteStep: 0
    // Снято с ЖИВОГО делегата карточки «Космический лифт» (w3).
    property int remoteCardState: -1
    property int remoteCardStateUpdates: 0
    property string remoteCardAuthor: "(делегат не создан)"
    property int remoteCardAuthorUpdates: 0

    Connections {
        target: remote
        function onModelReset() {
            root.remoteJournal.push("modelReset");
        }
        function onRowsInserted(parent, first, last) {
            root.remoteJournal.push("rowsInserted(" + first + "," + last + ")");
        }
        function onCountChanged() {
            root.remoteJournal.push("countChanged");
        }
        function onHasMoreChanged(value) {
            root.remoteJournal.push("hasMoreChanged(" + value + ")");
        }
        function onReqFetchMore(row) {
            root.remoteJournal.push("reqFetchMore(" + row + ")");
            root.remoteFetchRequests += 1;
            root.remoteAutoFetch = true;
            // Ровно то, что делает RemoteSearchQuery.fetchMore: подать
            // следующую страницу выдачи.
            remote.applyPageJson(JSON.stringify(root.remotePageTwo), false, true);
        }
        function onDataChanged(topLeft, bottomRight, roles) {
            var name = roles.length === 1
                       ? (roles[0] === root.acquisitionRole
                          ? "acquisitionState"
                          : (roles[0] === root.remoteAuthorRole ? "author" : "?"))
                       : "<широкий>";
            if (roles.length !== 1 || name === "?") {
                root.remoteWideUpdates += 1;
            } else if (topLeft.row === bottomRight.row) {
                root.remotePointUpdates += 1;
            }
            root.remoteJournal.push("dataChanged(" + topLeft.row + ","
                                    + bottomRight.row + "," + name + ")");
        }
    }

    Timer {
        id: remoteSteps
        interval: 150
        repeat: true
        running: false
        onTriggered: root.remotePageStep()
    }

    // Ожидаемый журнал после подачи ПЕРВОЙ страницы.
    //
    // Если вид сам попросил догрузку, порядок не тот, какого ждёшь от чтения
    // кода модели, и это НАСТОЯЩЕЕ поведение GridView: canFetchMore/fetchMore
    // он спрашивает прямо из обработчика modelReset, то есть РЕЕНТРАНТНО —
    // ещё внутри endResetModel(), до хвоста сброса (countChanged,
    // hasMoreChanged). Вторая страница уезжает в модель раньше, чем первая
    // договорит, а хвостовой hasMoreChanged приходит уже с ЛОЖЬЮ.
    function expectedFirstPageJournal() {
        if (!root.remoteAutoFetch)
            return ["modelReset", "countChanged", "hasMoreChanged(true)"];
        var first = root.remotePageOne.length;
        var last = first + root.remotePageTwo.length - 1;
        return ["modelReset", "hasMoreChanged(false)", "reqFetchMore(" + first + ")",
                "rowsInserted(" + first + "," + last + ")", "countChanged",
                "countChanged", "hasMoreChanged(false)"];
    }

    function remotePageStep() {
        root.remoteStep += 1;
        if (root.remoteStep === 1) {
            // Структурный сигнал полной замены обязан быть modelReset: строк
            // может стать любое число.
            root.checkEqual(JSON.stringify(root.remoteJournal),
                            JSON.stringify(root.expectedFirstPageJournal()),
                            "выдача: сигналы первой страницы");
            var loaded = root.remotePageOne.length
                         + (root.remoteAutoFetch ? root.remotePageTwo.length : 0);
            root.checkCount(remote.count, loaded, "выдача: число строк первой страницы");
            root.checkEqual(remote.hasMore, !root.remoteAutoFetch,
                            "выдача: hasMore первой страницы");
            root.checkCount(remoteGrid.count, remote.count, "выдача: делегатов меньше строк");
            root.checkEqual(remote.roleValue(2, "itemId"), "w3", "выдача: роль itemId");
            root.checkEqual(remote.roleValue(2, "title"), "Космический лифт",
                            "выдача: роль title");
            root.checkEqual(remote.roleValue(2, "wpType"), "web", "выдача: роль wpType");
            root.checkEqual(remote.roleValue(2, "acquisitionState"), 1,
                            "выдача: роль acquisitionState");
            root.checkEqual(remote.roleNumber("такойРолиНет"), -1,
                            "выдача: несуществующая роль");
            // Две пустоты сравнивать бессмысленно: сперва строка вообще отдана.
            root.check(Object.keys(remote.item(2)).length > 0,
                       "выдача: item() вернул пустую строку — сравнивать нечего");
            root.checkEqual(JSON.stringify(remote.item(2)), JSON.stringify(remote.get(2)),
                            "выдача: item() и get() расходятся");
            root.checkEqual(remote.item(2).title, "Космический лифт",
                            "выдача: item() отдаёт строку целиком");
        } else if (root.remoteStep === 2) {
            // Догрузку у оригинала запрашивает САМ вид (canFetchMore/
            // fetchMore). Если вид этого не сделал — сценарий подаёт её сам,
            // а в отчёт уходит autoFetchMore=false.
            if (root.remoteFetchRequests === 0)
                remote.applyPageJson(JSON.stringify(root.remotePageTwo), false, true);
        } else if (root.remoteStep === 3) {
            var total = root.remotePageOne.length + root.remotePageTwo.length;
            root.checkCount(remote.count, total, "выдача: число строк после догрузки");
            root.checkEqual(remote.hasMore, false, "выдача: hasMore после догрузки");
            root.checkCount(remoteGrid.count, total,
                            "выдача: делегатов после догрузки меньше строк");
            // Догрузка обязана прийти ВСТАВКОЙ строк, а не сбросом модели:
            // сброс уронил бы прокрутку Discover в начало списка. Сброс за
            // весь сценарий ровно один — тот, которым подана первая страница.
            var inserted = "rowsInserted(" + root.remotePageOne.length + "," + (total - 1) + ")";
            root.checkEqual(root.remoteJournal.filter(function (e) {
                                return e === inserted;
                            }).length, 1, "выдача: вставки строк догрузки");
            root.checkEqual(root.remoteJournal.filter(function (e) {
                                return e === "modelReset";
                            }).length, 1, "выдача: сбросов модели за сценарий");
            root.checkEqual(remote.roleValue(4, "itemId"), "w5",
                            "выдача: первая строка второй страницы");
            // Подписка через ОБЩИЙ склад — так же, как её шлёт запрос
            // подписки: не в конкретную модель, а во все живые.
            root.checkEqual(remote.publishAcquisitionState("workshop", "w3", 2), 1,
                            "склад: рассылка не дошла ни до одной модели");
        } else if (root.remoteStep === 4) {
            root.checkEqual(root.remotePointUpdates, 1, "подписка: точечных dataChanged");
            root.checkEqual(remote.roleValue(2, "acquisitionState"), 2,
                            "подписка: состояние строки не изменилось");
            // Ради чего склад общий: карточка перерисовалась сразу, без
            // перезагрузки страницы.
            root.checkEqual(root.remoteCardState, 2,
                            "подписка: делегат карточки не увидел новое состояние");
            root.check(root.remoteCardStateUpdates >= 1,
                       "подписка: делегат карточки не перепривязался");
            remote.setAuthors({ "w3": "Автор третьей", "w1": "Автор первой",
                                "нет такого": "мимо" });
        } else if (root.remoteStep === 5) {
            remoteSteps.running = false;
            root.checkEqual(root.remotePointUpdates, 3, "авторы: точечных dataChanged всего");
            root.checkEqual(remote.roleValue(0, "author"), "Автор первой",
                            "авторы: первая строка");
            root.checkEqual(root.remoteCardAuthor, "Автор третьей",
                            "авторы: делегат карточки не получил имя");
            root.check(root.remoteCardAuthorUpdates >= 1,
                       "авторы: делегат карточки не перепривязался");
            root.check(root.remoteWideUpdates === 0,
                       "dataChanged выдачи не с одной ожидаемой ролью: "
                       + root.remoteWideUpdates);
            root.checkPopulations([
                { what: "строк модели", size: function () { return remote.count; } },
                { what: "делегатов", size: function () { return remoteGrid.count; } },
                { what: "записей в журнале сигналов",
                  size: function () { return root.remoteJournal.length; } }
            ]);
            console.log("REXON-LIVE-REMOTE rows=" + remote.count
                        + " delegates=" + remoteGrid.count
                        + " hasMore=" + remote.hasMore
                        + " pointUpdates=" + root.remotePointUpdates
                        + " wideUpdates=" + root.remoteWideUpdates
                        + " fetchRequests=" + root.remoteFetchRequests
                        + " autoFetchMore=" + root.remoteAutoFetch
                        + " cardState=" + root.remoteCardState
                        + " cardAuthor=" + root.remoteCardAuthor
                        + " journal=" + JSON.stringify(root.remoteJournal));
            root.verdict();
        }
    }

    // Сетка карточек — та же форма вида, что у вендорной страницы
    // (MD.VerticalGridView поверх searchQuery.model). Карточка читает роли
    // как required property и перерисовывается ТОЛЬКО от точечного
    // dataChanged.
    GridView {
        id: remoteGrid
        x: 450
        y: 10
        width: 440
        height: root.height - 20
        clip: true
        cellWidth: Math.floor(width / 2)
        cellHeight: 60
        model: root.scenario === "remote" ? remote : null
        cacheBuffer: 100000

        delegate: Rectangle {
            id: remoteCard
            required property string itemId
            required property string title
            required property string author
            required property int acquisitionState

            width: remoteGrid.cellWidth - 6
            height: remoteGrid.cellHeight - 6
            color: "#16202d"

            onAcquisitionStateChanged: {
                if (remoteCard.itemId !== "w3")
                    return;
                root.remoteCardStateUpdates += 1;
                root.remoteCardState = remoteCard.acquisitionState;
            }
            onAuthorChanged: {
                if (remoteCard.itemId !== "w3" || remoteCard.author === "")
                    return;
                root.remoteCardAuthorUpdates += 1;
                root.remoteCardAuthor = remoteCard.author;
            }

            Text {
                text: remoteCard.title + " · " + remoteCard.author
                      + (remoteCard.acquisitionState === 2 ? " · подписка" : "")
                color: "#ffffff"
            }
        }
    }

    // ══ Плашка состояния гаснет сама ═══════════════════════════════════════
    //
    // Числа СВОИ и мелкие: проверяется механизм гашения, а не продуктовый
    // срок (тот берётся из ядра и сторожится в Rust, `toast::timeout_ms`).

    /// Срок жизни пробной плашки: заметно больше затухания (150 мс), иначе
    /// «ушла сама» и «ещё затухает» неразличимы.
    readonly property int toastProbeTimeout: 450

    /// Ящик за краем окна, а не `visible: false`: видимость наследуется, и в
    /// спрятанном ящике плашка была бы невидимой ВСЕГДА — проверка «погасла
    /// и больше не ловит щелчки» зеленела бы на сломанном коде.
    Item {
        id: toastProbeHost
        x: -5000
        width: 800
        height: 200

        QtObject {
            id: toastProbeMetrics
            function toastWidth(pageWidth, textWidth) {
                return Math.min(pageWidth, textWidth + 40);
            }
        }

        RexonToast {
            id: toastProbe
            skin: ({
                "background": "#202020",
                "border": "#404040",
                "textColor": "#ffffff",
                "borderWidth": 1,
                "radius": 8,
                "paddingH": 14,
                "paddingV": 10,
                "margin": 12
            })
            timeoutMs: root.toastProbeTimeout
            metrics: toastProbeMetrics
            availableWidth: toastProbeHost.width
        }
    }

    property int toastStep: 0

    Timer {
        id: toastSteps
        // Шаг длиннее затухания: приговор выносится, когда движение уже
        // закончилось, а не посреди него.
        interval: 150
        repeat: true
        running: false
        onTriggered: root.toastStep += 1
    }

    onToastStepChanged: {
        switch (root.toastStep) {
        case 1:
            toastProbe.show("проба");
            break;
        case 2:
            root.check(toastProbe.shown, "плашка: не показалась по команде");
            root.check(toastProbe.visible, "плашка: показалась, но невидима");
            root.checkEqual(toastProbe.message, "проба", "плашка: чужой текст");
            break;
        case 3:
            // Свой срок ещё не вышел — уходить рано.
            root.check(toastProbe.shown, "плашка: ушла раньше своего срока");
            break;
        case 7:
            // ⭐ Ради этого сторож и заведён.
            root.check(!toastProbe.shown, "плашка: не погасла сама");
            root.check(!toastProbe.visible,
                       "плашка: погасла, но осталась видимой и ловит щелчки");
            break;
        case 8:
            toastProbe.show("ещё раз");
            break;
        case 9:
            root.check(toastProbe.shown, "плашка: не поднялась второй раз");
            break;
        case 10:
            // Пустое сообщение — не «показать пустоту», а «сказать нечего».
            toastProbe.show("");
            break;
        case 12:
            toastSteps.running = false;
            root.check(!toastProbe.shown, "плашка: пустое сообщение её не погасило");
            root.checkPopulations([
                { what: "шагов сценария", size: function () { return root.toastStep; } }
            ]);
            root.verdict();
            break;
        default:
            break;
        }
    }

    // ══ Выбор файла у товара магазина ══════════════════════════════════════

    property int storeFilesStep: 0

    StoreFileChoice {
        id: storeFilesProbe
        x: 10
        y: 10
        width: 600
        filesJson: root.scenario === "storefiles" ? root.storeFilesJson : "[]"
        title: "Файлы товара"
        hint: "пояснение"
        willInstallText: "будет установлен"
    }

    function storeFileRow(i) {
        for (let k = 0; k < storeFilesProbe.children.length; k++) {
            if (storeFilesProbe.children[k].objectName === "storeFile" + i)
                return storeFilesProbe.children[k];
        }
        return null;
    }

    function storeFileMarked(i) {
        const row = root.storeFileRow(i);
        if (!row)
            return false;
        function walk(item) {
            if (item.objectName === "storeFileMark")
                return item.visible;
            for (let k = 0; k < item.children.length; k++) {
                if (walk(item.children[k]))
                    return true;
            }
            return false;
        }
        return walk(row);
    }

    Timer {
        id: storeFilesSteps
        interval: 120
        repeat: true
        running: false
        onTriggered: root.storeFilesStep += 1
    }

    onStoreFilesStepChanged: {
        switch (root.storeFilesStep) {
        case 1:
            root.viewerEvents = Qt.createQmlObject("import QtTest\nTestEvent {}", root);
            root.check(root.viewerEvents !== null, "файлы: модуль QtTest не поднялся");
            root.checkCount(storeFilesProbe.files.length, 2, "файлы: строк от ядра");
            root.check(storeFilesProbe.visible, "файлы: два файла, а блока нет");
            root.check(root.storeFileRow(0) !== null && root.storeFileRow(1) !== null,
                       "файлы: строки не построены");
            // Автоматический выбор ядра — тема, а не «Extra»; он же выбран.
            root.checkEqual(storeFilesProbe.autoName, "McOS-MJV-Gnome-3.30-1.1.tar.xz",
                            "файлы: автоматический выбор");
            root.checkEqual(storeFilesProbe.selectedName, storeFilesProbe.autoName,
                            "файлы: по умолчанию выбран не автоматический");
            root.check(!storeFilesProbe.differs, "файлы: «выбран другой» без выбора");
            root.check(root.storeFileMarked(1) && !root.storeFileMarked(0),
                       "файлы: пометка «будет установлен» не у автоматического");
            break;
        case 2: {
            const row = root.storeFileRow(0);
            if (row)
                root.viewerEvents.mouseClick(row, row.width / 2, row.height / 2,
                                             Qt.LeftButton, Qt.NoModifier, -1);
            break;
        }
        case 3:
            root.checkEqual(storeFilesProbe.selectedName, "Extra 1.4.tar.xz",
                            "файлы: щелчок не выбрал строку");
            root.check(storeFilesProbe.differs, "файлы: выбран другой, а «differs» ложно");
            root.check(root.storeFileMarked(0) && !root.storeFileMarked(1),
                       "файлы: пометка не переехала на выбранный");
            // Один файл — выбирать не из чего.
            storeFilesProbe.filesJson = root.storeFilesSingleJson;
            break;
        case 4:
            root.check(!storeFilesProbe.visible, "файлы: у одного файла виден блок выбора");
            // Снова два файла (другой товар) — выбор опять автоматический.
            storeFilesProbe.filesJson = root.storeFilesJson;
            break;
        case 5:
            storeFilesSteps.running = false;
            root.checkEqual(storeFilesProbe.selectedName, storeFilesProbe.autoName,
                            "файлы: новый список не вернул автоматический выбор");
            root.check(!storeFilesProbe.differs, "файлы: после смены списка «выбран другой»");
            root.checkPopulations([
                { what: "строк выбора", size: function () { return storeFilesProbe.files.length; } }
            ]);
            root.verdict();
            break;
        default:
            break;
        }
    }

    // ══ Крупный просмотр картинки ══════════════════════════════════════════
    //
    // Щелчки и клавиши НАСТОЯЩИЕ: их шлёт в окно `TestEvent` из модуля
    // QtTest. Модуль поднимается только в этом сценарии и только здесь —
    // продукту он не нужен, поэтому в импортах документа его нет.

    readonly property var viewerProbeItems: [
        { "key": "a", "title": "Первая", "subtitle": "Игра А" },
        { "key": "b", "title": "Вторая", "subtitle": "Игра Б", "badge": "отметка" },
        { "key": "c", "title": "Третья", "subtitle": "Игра В" }
    ]
    /// Сколько щелчков дошло до самих карточек (мимо лупы и миниатюры).
    property int viewerCardTaps: 0
    property var viewerEvents: null
    property int viewerStep: 0

    Row {
        id: viewerProbeRow
        visible: root.scenario === "viewer"
        x: 20
        y: 20
        spacing: 10

        Repeater {
            id: viewerCards
            model: root.scenario === "viewer" ? root.viewerProbeItems : []

            Rectangle {
                id: viewerCard
                required property int index
                property alias zoomArea: viewerCardZoom
                width: 160
                height: 130
                color: "#16202d"

                TapHandler { onTapped: root.viewerCardTaps += 1 }

                Rectangle {
                    x: 10
                    y: 10
                    width: 140
                    height: 80
                    color: "#2a4055"

                    RexonZoomArea {
                        id: viewerCardZoom
                        anchors.fill: parent
                        // У третьей карточки щелчок «занят»: открывает
                        // только лупа.
                        badgeOnly: viewerCard.index === 2
                        onActivated: viewerProbe.show(root.viewerProbeItems, viewerCard.index)
                    }
                }
            }
        }
    }

    RexonImageViewer {
        id: viewerProbe
    }

    function viewerChild(name) {
        function walk(item) {
            if (!item)
                return null;
            if (item.objectName === name)
                return item;
            for (let i = 0; i < item.children.length; i++) {
                const hit = walk(item.children[i]);
                if (hit)
                    return hit;
            }
            return null;
        }
        return walk(viewerProbe.contentItem);
    }

    function viewerBadge(card) {
        const area = viewerCards.itemAt(card).zoomArea;
        for (let i = 0; i < area.children.length; i++) {
            if (area.children[i].objectName === "zoomBadge")
                return area.children[i];
        }
        return null;
    }

    function viewerClick(item, x, y) {
        root.viewerEvents.mouseClick(item, x, y, Qt.LeftButton, Qt.NoModifier, -1);
    }

    function viewerKey(key) {
        root.viewerEvents.keyClick(key, Qt.NoModifier, -1);
    }

    Timer {
        id: viewerSteps
        // Шаг длиннее защиты от двойного щелчка (450 мс) быть не обязан:
        // где она важна, сценарий пережидает её несколькими шагами.
        interval: 120
        repeat: true
        running: false
        onTriggered: root.viewerStep += 1
    }

    onViewerStepChanged: {
        const area0 = viewerCards.itemAt(0) ? viewerCards.itemAt(0).zoomArea : null;
        const area2 = viewerCards.itemAt(2) ? viewerCards.itemAt(2).zoomArea : null;
        switch (root.viewerStep) {
        case 1:
            root.viewerEvents = Qt.createQmlObject("import QtTest\nTestEvent {}", root);
            root.check(root.viewerEvents !== null, "просмотр: модуль QtTest не поднялся");
            root.check(area0 !== null && area2 !== null, "просмотр: карточки не построены");
            root.check(!root.viewerBadge(0).visible, "просмотр: лупа видна без наведения");
            root.viewerEvents.mouseMove(area0, 70, 40, -1, Qt.NoButton, Qt.NoModifier);
            break;
        case 2:
            root.check(root.viewerBadge(0).visible, "просмотр: лупа не появилась под курсором");
            root.viewerClick(area0, 70, 40);
            break;
        case 3:
            root.check(viewerProbe.opened, "просмотр: щелчок по миниатюре его не открыл");
            root.checkEqual(viewerProbe.index, 0, "просмотр: открыт не на той карточке");
            root.checkEqual(root.viewerCardTaps, 0,
                            "просмотр: щелчок по миниатюре дошёл и до карточки");
            root.checkEqual(root.viewerChild("imageViewerTitle").text, "Первая",
                            "просмотр: чужое название в подписи");
            root.viewerKey(Qt.Key_Right);
            break;
        case 4:
            root.checkEqual(viewerProbe.index, 1, "просмотр: → не листает вперёд");
            root.checkEqual(root.viewerChild("imageViewerSubtitle").text, "отметка  ·  Игра Б",
                            "просмотр: отметка и вторая строка подписи");
            root.checkEqual(root.viewerChild("imageViewerCounter").text, "2 / 3",
                            "просмотр: счётчик");
            root.viewerKey(Qt.Key_Left);
            root.viewerKey(Qt.Key_Left);
            break;
        case 5:
            root.checkEqual(viewerProbe.index, 2, "просмотр: ← не листает по кругу");
            root.viewerClick(root.viewerChild("imageViewerNext"), 10, 10);
            break;
        case 6:
            root.checkEqual(viewerProbe.index, 0, "просмотр: кнопка «вперёд» не листает");
            root.viewerClick(root.viewerChild("imageViewerPrev"), 10, 10);
            break;
        case 7:
            root.checkEqual(viewerProbe.index, 2, "просмотр: кнопка «назад» не листает");
            root.viewerKey(Qt.Key_Escape);
            break;
        case 8:
            root.check(!viewerProbe.opened, "просмотр: Esc его не закрыл");
            // Карточка с занятым щелчком: по картинке — мимо просмотра.
            root.viewerClick(area2, 40, 60);
            break;
        case 9:
            root.check(!viewerProbe.opened,
                       "просмотр: у карточки «только лупа» открылся щелчком по картинке");
            root.checkEqual(root.viewerCardTaps, 1,
                            "просмотр: щелчок по картинке не дошёл до карточки «только лупа»");
            root.viewerEvents.mouseMove(area2, 70, 40, -1, Qt.NoButton, Qt.NoModifier);
            break;
        case 10:
            root.viewerClick(root.viewerBadge(2), 14, 14);
            break;
        case 11:
            root.check(viewerProbe.opened, "просмотр: лупа его не открыла");
            root.checkEqual(viewerProbe.index, 2, "просмотр: лупа открыла не ту карточку");
            root.checkEqual(root.viewerCardTaps, 1, "просмотр: щелчок по лупе дошёл до карточки");
            // Сразу после открытия щелчок мимо картинки НЕ закрывает: это
            // вторая половина двойного щелчка.
            root.viewerClick(viewerProbe.contentItem, 3, 3);
            break;
        case 12:
            root.check(viewerProbe.opened,
                       "просмотр: закрылся второй половиной двойного щелчка");
            break;
        case 17:
            // Щелчок по самой картинке не закрывает…
            root.viewerClick(viewerProbe.contentItem, viewerProbe.width / 2,
                             viewerProbe.height / 2);
            break;
        case 18:
            root.check(viewerProbe.opened, "просмотр: закрылся щелчком по картинке");
            // …а мимо неё — закрывает.
            root.viewerClick(viewerProbe.contentItem, 3, 3);
            break;
        case 19:
            root.check(!viewerProbe.opened, "просмотр: щелчок мимо картинки его не закрыл");
            area0.forceActiveFocus();
            root.viewerKey(Qt.Key_Return);
            break;
        case 20:
            root.check(viewerProbe.opened, "просмотр: Enter на миниатюре в фокусе его не открыл");
            root.viewerClick(root.viewerChild("imageViewerClose"), 10, 10);
            break;
        case 21:
            viewerSteps.running = false;
            root.check(!viewerProbe.opened, "просмотр: кнопка закрытия его не закрыла");
            root.checkPopulations([
                { what: "карточек", size: function () { return viewerCards.count; } },
                { what: "шагов сценария", size: function () { return root.viewerStep; } }
            ]);
            root.verdict();
            break;
        default:
            break;
        }
    }

    // ══ Вкладка Steam: сетка фонов ════════════════════════════════════════
    //
    // Настоящая страница на подставном Steam (его собирает бегун до старта
    // окна: каталог Steam с учётной записью и кэш списка из пяти фонов, два
    // из них анимированные). Проверяется то, что видит человек: три кнопки
    // вида со счётчиками, отметка на миниатюре, отбор настоящим щелчком,
    // отметка в крупном просмотре, строка «анимированные не показаны» с
    // причиной и кнопка «Повторить».

    property var steamBgModel: null
    /// Что модель успела показать после «Повторить»: снятую причину и
    /// подпись «обновляю» — оба состояния живут доли секунды.
    property bool steamBgSawNoNote: false
    property bool steamBgSawUpdating: false

    Connections {
        target: root.steamBgModel
        function onBackgroundsChanged() {
            if (root.steamBgModel.bgAnimatedNote === "")
                root.steamBgSawNoNote = true;
            if (root.steamBgModel.bgStatusText.indexOf("Updating") >= 0)
                root.steamBgSawUpdating = true;
        }
    }

    // Бегун сценариев страницы Steam: шаги плана по одному на тик. Шаг,
    // ответивший `false`, повторяется — так сценарий ЖДЁТ итог щелчка или
    // фоновой работы, а не угадывает срок; дольше 80 тиков не ждёт никто.
    property var steamPlan: []
    property string steamPlanName: ""
    property int steamPlanAt: 0
    property int steamPlanWait: 0

    Timer {
        id: steamPlanTimer
        interval: 100
        repeat: true
        running: false
        onTriggered: {
            if (root.steamPlanAt >= root.steamPlan.length) {
                steamPlanTimer.running = false;
                root.verdict();
                return;
            }
            const done = root.steamPlan[root.steamPlanAt]();
            if (done === false && root.steamPlanWait < 80) {
                root.steamPlanWait += 1;
                return;
            }
            root.check(done !== false, root.steamPlanName + ": шаг " + root.steamPlanAt
                       + " не дождался своего");
            root.steamPlanWait = 0;
            root.steamPlanAt += 1;
        }
    }

    function steamPlanStart(name, plan) {
        root.steamPlanName = name;
        root.steamPlan = plan;
        steamPlanTimer.running = true;
    }

    Loader {
        id: steamBgLoader
        anchors.fill: parent
        active: root.scenario === "steambg" || root.scenario === "steamwiz"
        sourceComponent: SteamPage {
            actions: SteamActions {}
        }
    }

    /// Все объекты с данным именем под страницей: зримые потомки и
    /// невизуальные (модель, всплывающий просмотр).
    function steamBgAll(name) {
        const out = [];
        const seen = [];
        function walk(item) {
            if (!item || seen.indexOf(item) >= 0)
                return;
            seen.push(item);
            if (item.objectName === name)
                out.push(item);
            const kids = item.children || [];
            for (let i = 0; i < kids.length; i++)
                walk(kids[i]);
            const rest = item.resources || [];
            for (let i = 0; i < rest.length; i++)
                walk(rest[i]);
            if (item.contentItem)
                walk(item.contentItem);
        }
        walk(steamBgLoader.item);
        return out;
    }

    /// Первый ВИДИМЫЙ объект с этим именем (у карточек шагов помощника
    /// одноимённые кнопки есть в каждой, видна одна); видимого нет — любой.
    function steamBgOne(name) {
        const all = root.steamBgAll(name);
        for (let i = 0; i < all.length; i++) {
            if (all[i].visible === true)
                return all[i];
        }
        return all.length > 0 ? all[0] : null;
    }

    function steamBgMarks() {
        return root.steamBgAll("bgAnimatedMark").filter(m => m.visible).length;
    }

    function steamBgChip(key) {
        return root.steamBgOne("bgKindChip_" + key);
    }

    /// Подкрутить страницу так, чтобы элемент попал в окно. Страница сама
    /// подкручивается к сфокусированному контролу, поэтому где элемент
    /// окажется к моменту щелчка, заранее не известно, а щелчок мимо окна
    /// до него не доходит.
    function steamReveal(item) {
        // Прокручиваемых предков может быть несколько (раздел в своей
        // области, та — в области страницы): подкручиваем каждую.
        for (let p = item.parent; p; p = p.parent) {
            if (p.contentY === undefined || !p.contentItem || p.contentHeight === undefined)
                continue;
            const at = item.mapToItem(p.contentItem, 0, 0);
            if (at.y < p.contentY + 20 || at.y + item.height > p.contentY + p.height - 20)
                p.contentY = Math.max(0, Math.min(at.y - 80, p.contentHeight - p.height));
            // И по ширине: страница не уже своей наименьшей ширины, и в
            // узком окне правый край уходит за него.
            if (at.x < p.contentX || at.x + item.width > p.contentX + p.width)
                p.contentX = Math.max(0, Math.min(at.x + item.width + 20 - p.width,
                                                  p.contentWidth - p.width));
        }
    }

    /// Настоящий щелчок мышью по середине элемента.
    function steamBgClick(item) {
        root.steamReveal(item);
        // Куда щелчок пришёлся — в журнал прогона: по нему видно, если
        // элемент оказался за краем окна или под чужим слоем.
        const at = item.mapToItem(null, item.width / 2, item.height / 2);
        console.log("REXON-LIVE-CLICK " + item.objectName + " @" + Math.round(at.x) + ","
                    + Math.round(at.y) + " окно " + root.width + "×" + root.height
                    + " виден=" + item.visible + " доступен=" + item.enabled);
        root.viewerEvents.mouseClick(item, item.width / 2, item.height / 2,
                                     Qt.LeftButton, Qt.NoModifier, -1);
    }

    function steamBgSubtitle() {
        const sub = root.steamBgOne("imageViewerSubtitle");
        return sub ? sub.text : "";
    }

    function steamBgViewerOpen() {
        const viewer = root.steamBgOne("imageViewer");
        return viewer !== null && viewer.opened === true;
    }

    readonly property var steamBgPlan: [
        () => {
            root.viewerEvents = Qt.createQmlObject("import QtTest\nTestEvent {}", root);
            root.steamBgModel = root.steamBgOne("steamModel");
            if (!root.steamBgModel)
                return false;
            root.steamBgModel.setSection("bg");
        },
        // Ответ о владеемых фонах приходит из фоновой задачи.
        () => root.steamBgModel.bgAnimatedNote !== "" && root.steamBgChip("animated") !== null,
        // Узкое окно (живая проверка r1434, 16.10.2026): длинный путь к
        // Steam в шапке делал страницу шире окна — «Повторить» уезжала за
        // правый край, внизу висела горизонтальная прокрутка. Путь прогона
        // длинный сам (каталог прогона в HOME), при 640 точках он обязан
        // сократиться, а страница — уложиться в окно.
        () => { root.width = 640; },
        () => root.width === 640 && root.steamBgOne("headerFoundPath") !== null
              && root.steamBgOne("headerFoundPath").width < 640,
        () => {
            const scroll = root.steamBgOne("pageScroll");
            const path = root.steamBgOne("headerFoundPath");
            const retry = root.steamBgOne("bgAnimatedRetry");
            root.check(scroll.contentWidth <= scroll.availableWidth + 1,
                       "узкое окно: страница шире окна (" + Math.round(scroll.contentWidth)
                       + " при видимых " + Math.round(scroll.availableWidth) + ")");
            root.check(path.visible && path.truncated,
                       "узкое окно: путь к Steam в шапке не сокращён (ширина "
                       + Math.round(path.width) + ")");
            const right = retry.mapToItem(null, retry.width, 0).x;
            root.check(retry.visible && right <= root.width,
                       "узкое окно: «Повторить» за правым краем (" + Math.round(right)
                       + " при окне " + root.width + ")");
        },
        // Длинная сетка фонов прокручивается (живьём r1439, 16.10.2026: 37
        // анимированных фонов уходили за нижний край окна без ползунка, колесо
        // не крутило). У единственного ребёнка области не было
        // `implicitHeight`, и `contentHeight` оставался -1 — область считала
        // содержимое равным себе. В узком окне пять карточек встают столбиком
        // и в полосу не влезают: этим и проверяется.
        () => {
            const scroll = root.steamBgOne("bgScroll");
            const flick = scroll.contentItem;
            const group = scroll.contentChildren[0];
            root.check(group.height > scroll.availableHeight + 1,
                       "прокрутка фонов: сетка влезла в полосу (" + Math.round(group.height)
                       + " при видимых " + Math.round(scroll.availableHeight)
                       + ") — проверять нечего");
            root.check(scroll.contentHeight > scroll.availableHeight + 1,
                       "прокрутка фонов: высота содержимого " + Math.round(scroll.contentHeight)
                       + " при видимых " + Math.round(scroll.availableHeight)
                       + " и группе " + Math.round(group.height) + " — прокручивать нечего");
            // Присоединённый `ScrollBar` извне не читается; ту же долю
            // видимого, что задаёт размер ползунка, даёт сама полоса.
            root.check(flick.visibleArea.heightRatio < 0.99,
                       "прокрутка фонов: видна вся высота — ползунку нечего показывать (доля "
                       + flick.visibleArea.heightRatio + ")");
            flick.contentY = 0;
            const at = scroll.mapToItem(null, scroll.width / 2, scroll.height / 2);
            console.log("REXON-LIVE-WHEEL bgScroll @" + Math.round(at.x) + "," + Math.round(at.y)
                        + " содержимое " + Math.round(scroll.contentHeight)
                        + " видно " + Math.round(scroll.availableHeight));
            for (let i = 0; i < 3; i++)
                root.viewerEvents.mouseWheel(scroll, scroll.width / 2, scroll.height / 2,
                                             Qt.NoButton, Qt.NoModifier, 0, -120, -1);
        },
        // Колесо прокручивает плавно — ждём, пока сдвинется и встанет: иначе
        // дальнейшие щелчки попадут в едущую полосу.
        () => {
            const flick = root.steamBgOne("bgScroll").contentItem;
            return flick.contentY > 0 && !flick.moving;
        },
        () => {
            root.steamBgOne("bgScroll").contentItem.contentY = 0;
            root.width = 900;
        },
        () => root.width === 900,
        () => {
            const model = root.steamBgModel;
            root.checkPopulations([
                { what: "фоны в списке", size: () => model.bgCards.length },
                { what: "кнопки вида", size: () => model.bgKindChips.length },
                { what: "отметки анимированных", size: () => root.steamBgMarks() },
            ]);
            root.checkEqual(model.bgCards.length, 5, "фоны: карточек из кэша");
            root.checkEqual(root.steamBgChip("").text, "All (5)", "фоны: кнопка «все»");
            root.checkEqual(root.steamBgChip("static").text, "Still (3)", "фоны: кнопка «статичные»");
            root.checkEqual(root.steamBgChip("animated").text, "Animated (2)",
                            "фоны: кнопка «анимированные»");
            root.check(root.steamBgChip("").checked, "фоны: «все» не выбрана при открытии");
            root.checkEqual(root.steamBgMarks(), 2, "фоны: отметок анимированного на миниатюрах");
            root.check(model.bgAnimatedNote.indexOf("sign in to Steam on this computer") >= 0,
                       "фоны: строка об анимированных не называет причину: «"
                       + model.bgAnimatedNote + "»");
            const note = root.steamBgOne("bgAnimatedNote");
            root.check(note !== null && note.visible, "фоны: строка об анимированных не видна");
            root.steamBgClick(root.steamBgChip("animated"));
        },
        () => root.steamBgModel.bgCards.length === 2,
        () => {
            const model = root.steamBgModel;
            root.checkEqual(root.steamBgMarks(), 2, "фоны: под «анимированными» отметка не у всех");
            root.check(root.steamBgChip("animated").checked, "фоны: кнопка «анимированные» не нажата");
            root.checkEqual(model.bgStatusText, "Showing 2 of 5", "фоны: подпись при отборе");
            root.steamBgClick(root.steamBgChip("static"));
        },
        () => root.steamBgModel.bgCards.length === 3,
        () => {
            root.checkEqual(root.steamBgMarks(), 0, "фоны: отметка у статичного фона");
            // Поиск и вид вместе: счётчики считаются по найденному.
            root.steamBgModel.setBgSearch("half-life");
        },
        () => {
            root.checkEqual(root.steamBgChip("").text, "All (3)", "фоны: «все» при поиске");
            root.checkEqual(root.steamBgChip("static").text, "Still (2)", "фоны: «статичные» при поиске");
            root.checkEqual(root.steamBgChip("animated").text, "Animated (1)",
                            "фоны: «анимированные» при поиске");
            root.checkEqual(root.steamBgModel.bgCards.length, 2, "фоны: статичные из найденного");
            root.steamBgModel.setBgSearch("");
        },
        // Кнопки после смены поиска построены заново — щелчок отдельным
        // шагом, когда они уже разложены.
        () => { root.steamBgClick(root.steamBgChip("")); },
        () => root.steamBgModel.bgCards.length === 5,
        () => { steamBgLoader.item.openBackgroundViewer("2"); },
        () => root.steamBgViewerOpen() && root.steamBgSubtitle() !== "",
        () => {
            root.check(root.steamBgSubtitle().indexOf("🎞️ Animated background") >= 0,
                       "фоны: в просмотре нет отметки анимированного: «" + root.steamBgSubtitle() + "»");
            root.viewerEvents.keyClick(Qt.Key_Right, Qt.NoModifier, -1);
        },
        () => root.steamBgSubtitle().indexOf("Animated background") < 0,
        () => { root.viewerEvents.keyClick(Qt.Key_Escape, Qt.NoModifier, -1); },
        () => !root.steamBgViewerOpen(),
        // Просмотр уходит с экрана не мгновенно — щелчок по странице под
        // ним отдельным шагом. «Повторить»: прежний приговор снимается,
        // список из кэша остаётся, причина возвращается с новым ответом.
        () => {
            root.steamBgSawNoNote = false;
            root.steamBgSawUpdating = false;
        },
        () => { root.steamBgClick(root.steamBgOne("bgAnimatedRetry")); },
        () => root.steamBgSawNoNote && root.steamBgSawUpdating,
        () => root.steamBgModel.bgAnimatedNote !== "",
        () => {
            root.checkEqual(root.steamBgModel.bgCards.length, 5, "фоны: повтор стёр список из кэша");
        },
    ]

    // ══ Вкладка Steam: помощник загрузки витрин ═══════════════════════════
    //
    // Тот же подставной Steam, в «Изображениях» лежит набор из двух частей.
    // Проверяется путь человека: вернуться к набору → «Скопировать
    // название» (текст действительно в буфере обмена) → отметить части
    // загруженными → «Готово» → «Проверить мой профиль» при глухой сети:
    // помощник обязан сказать словами, что не вышло, и не закрыть шаг.
    // Кнопки «Открыть страницу загрузки» и «Открыть редактор профиля» не
    // нажимаются: они запускают чужие программы.

    property var steamWizActions: null
    property string steamWizDir: ""

    TextEdit { id: steamWizClip; visible: false }

    function steamWizPasted() {
        steamWizClip.clear();
        steamWizClip.paste();
        return steamWizClip.text;
    }

    function steamWizState() {
        return JSON.parse(root.steamWizActions.wizard(root.steamWizDir));
    }

    function steamWizReveal(name) {
        const item = root.steamBgOne(name);
        if (item)
            root.steamReveal(item);
    }

    function steamWizClick(name) {
        const item = root.steamBgOne(name);
        root.check(item !== null && item.visible, "помощник: нет на экране «" + name + "»");
        if (item)
            root.steamBgClick(item);
    }

    readonly property var steamWizPlan: [
        () => {
            root.viewerEvents = Qt.createQmlObject("import QtTest\nTestEvent {}", root);
            root.steamBgModel = root.steamBgOne("steamModel");
            if (!root.steamBgModel)
                return false;
            root.steamBgModel.setSection("showcase");
        },
        () => {
            root.steamWizActions = root.steamBgOne("steamShowcaseActions");
            if (!root.steamWizActions || !root.steamBgOne("showcaseRecent_0"))
                return false;
            const recent = JSON.parse(root.steamWizActions.recentSets());
            root.checkCount(recent.length, 1, "помощник: недавних наборов");
            root.steamWizDir = recent.length > 0 ? recent[0].dir : "";
            root.checkEqual(recent.length > 0 ? recent[0].name : "", "probe-set", "помощник: имя набора");
            const wizard = root.steamBgOne("showcaseWizard");
            root.check(wizard !== null && !wizard.visible, "помощник: виден до выбора набора");
            root.steamWizReveal("showcaseRecent_0");
        },
        () => { root.steamWizClick("showcaseRecent_0"); },
        () => root.steamBgOne("showcaseWizard").visible === true
              && root.steamBgOne("showcaseCopyTitle_0") !== null,
        () => {
            root.checkPopulations([
                { what: "шаги помощника", size: () => root.steamBgAll("showcaseStep_upload").length
                                                      + root.steamBgAll("showcaseStep_place").length
                                                      + root.steamBgAll("showcaseStep_check").length },
                { what: "части набора", size: () => root.steamWizState().parts.length },
            ]);
            root.checkEqual(root.steamWizState().step, "upload", "помощник: начальный шаг");
            root.steamWizReveal("showcaseCopyTitle_0");
        },
        () => { root.steamWizClick("showcaseCopyTitle_0"); },
        // «Скопировать название»: текст действительно в буфере обмена.
        () => root.steamWizPasted() === "probe-set — main",
        () => { root.steamWizClick("showcaseCopyBlank"); },
        () => root.steamWizPasted() === "\u2800",
        () => {
            const placed = root.steamBgOne("showcasePlaced");
            root.check(placed !== null && !placed.enabled,
                       "помощник: «Готово» доступна до загрузки частей");
            root.steamWizClick("showcaseUploaded_0");
        },
        () => root.steamWizState().parts[0].uploaded === true,
        () => {
            root.checkEqual(root.steamWizState().step, "upload", "помощник: шаг после одной части из двух");
            root.steamWizReveal("showcaseUploaded_1");
        },
        () => { root.steamWizClick("showcaseUploaded_1"); },
        () => root.steamWizState().step === "place",
        () => { root.steamWizReveal("showcasePlaced"); },
        () => { root.steamWizClick("showcasePlaced"); },
        () => root.steamWizState().step === "check",
        () => { root.steamWizReveal("showcaseCheck"); },
        () => { root.steamWizClick("showcaseCheck"); },
        // Сеть глухая: проверка обязана кончиться и назвать причину.
        () => {
            const check = JSON.parse(root.steamWizActions.poll()).check;
            return check.done === true && root.steamBgOne("showcaseCheckText").text !== "";
        },
        () => {
            const check = JSON.parse(root.steamWizActions.poll()).check;
            root.check(!check.ok && check.error !== "",
                       "помощник: проверка при глухой сети не назвала причину");
            const state = root.steamWizState();
            root.checkEqual(state.step, "check", "помощник: неудачная проверка сдвинула шаг");
            root.check(state.placed && !state.verified, "помощник: отметки после неудачной проверки");
        },
    ]
}
