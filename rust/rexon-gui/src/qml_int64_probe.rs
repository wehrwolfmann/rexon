// Проба «64-битное целое в QML»: живой движок QML против мостов cxx-qt.
//
// ЧТО ИМЕННО ЛОМАЕТСЯ. cxx-qt печатает `i64`/`u64` как `::std::int64_t` /
// `::std::uint64_t`, а на 64-битном Linux это `long` / `unsigned long` —
// ОТДЕЛЬНЫЕ метатипы Qt, не те, что `qlonglong`/`qulonglong`. Движок QML их
// не знает, и ЛИТЕРАЛЬНАЯ декларативная привязка в такое свойство
// отказывает: «Cannot assign value 7 to property displayId». Отказ уносит
// ВЕСЬ документ — страница не создаётся вовсе.
//
// ⚠️ Ни компилятор, ни `cargo test`, ни гейт типов
// `scripts/test_we_rust_types.py` этого не видят: и сборка, и сигнатуры
// сходятся. Ломается только на экране. Отсюда и проба: замер живым
// движком, а не разбор исходников.
//
// Что замерено 31.08.2026 на Qt 6 (CachyOS), до починки:
//
//   * `Type { prop: 7 }` в свойство `long`/`unsigned long` — ОТКАЗ,
//     документ мёртв;
//   * `prop: root.someInt` и `prop: root.someReal` — работают;
//   * чтение такого свойства — приезжает числом;
//   * инвокабл и обработчик сигнала с таким параметром — работают;
//   * 64-битное целое ВНУТРИ `QVariant` — читается числом (`QVariant` несёт
//     метатип отдельно, и JS-обёртка Qt его разворачивает).
//
// Работает сегодня — не значит «можно оставить»: разница между литералом и
// привязкой держится на внутренностях движка, а не на договоре. Поэтому
// наружу 64-битное целое не выпускается ВООБЩЕ (`src/qvalue.rs`), а сторож
// `qml_int64` в `src/lib.rs` валит `cargo test`, если такое объявление
// заведётся снова. Проба — эмпирическая половина: она доказывает, что
// нынешний мост живой движок принимает.
//
// Запуск:
//     QT_QPA_PLATFORM=offscreen \
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --qml-int64-probe
//
// Возврат: 0 — все случаи прошли, 1 — есть провалы, 2 — проба не
// отчиталась (движок не собрал корневой документ либо не уложился в срок).

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

pub const ARG: &str = "--qml-int64-probe";

/// Один случай пробы: имя и тело QML-документа.
///
/// ⚠️ Каждый случай — ОТДЕЛЬНЫЙ файл, который бегун грузит
/// `Qt.createComponent`. Это не украшательство: отказ привязки уносит весь
/// документ, и в одном файле первый же провал спрятал бы все остальные
/// случаи. Отдельными файлами провал ловится по имени.
///
/// Тело обязано объявить `probeOk` (приговор) и `probeWhy` (что увидели).
struct Case {
    name: &'static str,
    body: &'static str,
}

/// Ожидаемое значение во всех случаях — 7 (`root.probeInt` / `root.probeReal`).
const CASES: &[Case] = &[
    // ── 1. Литеральная привязка: ТОТ САМЫЙ случай класса ─────────────────
    Case {
        name: "DisplayLayoutSetQuery.displayId ← литерал",
        body: r#"
    property QtObject target: W.DisplayLayoutSetQuery { displayId: 7 }
    readonly property bool probeOk: Number(target.displayId) === 7
    readonly property string probeWhy: "" + target.displayId + " / " + typeof target.displayId
"#,
    },
    Case {
        name: "DisplayRenameQuery.displayId ← литерал",
        body: r#"
    property QtObject target: W.DisplayRenameQuery { displayId: 7 }
    readonly property bool probeOk: Number(target.displayId) === 7
    readonly property string probeWhy: "" + target.displayId + " / " + typeof target.displayId
"#,
    },
    Case {
        name: "LibraryRemoveQuery.libraryId ← литерал",
        body: r#"
    property QtObject target: W.LibraryRemoveQuery { libraryId: 7 }
    readonly property bool probeOk: Number(target.libraryId) === 7
    readonly property string probeWhy: "" + target.libraryId + " / " + typeof target.libraryId
"#,
    },
    Case {
        name: "LibraryRenameQuery.libraryId ← литерал (waywallen 0.5.0)",
        body: r#"
    property QtObject target: W.LibraryRenameQuery { libraryId: 7; name: "Обои" }
    readonly property bool probeOk: Number(target.libraryId) === 7 && target.name === "Обои"
        && !target.querying
    readonly property string probeWhy: "" + target.libraryId + " / " + typeof target.libraryId
        + " / " + target.name
"#,
    },
    Case {
        name: "LibraryAddQuery.name — запись и чтение (waywallen 0.5.0)",
        body: r#"
    property QtObject target: W.LibraryAddQuery { path: "/p"; pluginName: "we"; name: "Мастерская" }
    readonly property bool probeOk: target.name === "Мастерская" && target.pluginName === "we"
    readonly property string probeWhy: target.name + " / " + target.pluginName
"#,
    },
    Case {
        name: "PlaylistDetailQuery.playlistId ← литерал",
        body: r#"
    property QtObject target: W.PlaylistDetailQuery { playlistId: 7 }
    readonly property bool probeOk: Number(target.playlistId) === 7
    readonly property string probeWhy: "" + target.playlistId + " / " + typeof target.playlistId
"#,
    },
    // ── 2. Привязка от свойства: int и double как источник ────────────────
    Case {
        name: "DisplayLayoutSetQuery.displayId ← int",
        body: r#"
    property int src: 7
    property QtObject target: W.DisplayLayoutSetQuery { displayId: root.src }
    readonly property bool probeOk: Number(target.displayId) === 7
    readonly property string probeWhy: "" + target.displayId + " / " + typeof target.displayId
"#,
    },
    Case {
        name: "LibraryRemoveQuery.libraryId ← double",
        body: r#"
    property double src: 7
    property QtObject target: W.LibraryRemoveQuery { libraryId: root.src }
    readonly property bool probeOk: Number(target.libraryId) === 7
    readonly property string probeWhy: "" + target.libraryId + " / " + typeof target.libraryId
"#,
    },
    // ── 3. Чтение свойств только для чтения ───────────────────────────────
    Case {
        name: "WallpaperSelectStorage.playlistEditTargetId — чтение",
        body: r#"
    property QtObject target: W.WallpaperSelectStorage { }
    readonly property bool probeOk: typeof target.playlistEditTargetId === "number"
    readonly property string probeWhy: typeof target.playlistEditTargetId
"#,
    },
    Case {
        name: "PlaylistItemSelectStorage.playlistId/revision — чтение",
        body: r#"
    property QtObject target: W.PlaylistItemSelectStorage { }
    readonly property bool probeOk: typeof target.playlistId === "number"
        && typeof target.revision === "number"
    readonly property string probeWhy: typeof target.playlistId + " / " + typeof target.revision
"#,
    },
    Case {
        name: "PlaylistMutationQuery.createdId — чтение",
        body: r#"
    property QtObject target: W.PlaylistMutationQuery { }
    readonly property bool probeOk: typeof target.createdId === "number"
    readonly property string probeWhy: typeof target.createdId
"#,
    },
    Case {
        name: "PlaylistDetailQuery.revision — чтение",
        body: r#"
    property QtObject target: W.PlaylistDetailQuery { }
    readonly property bool probeOk: typeof target.revision === "number"
    readonly property string probeWhy: typeof target.revision
"#,
    },
    Case {
        name: "App.networkCacheSize/MaximumSize — чтение",
        body: r#"
    readonly property bool probeOk: typeof W.App.networkCacheSize === "number"
        && typeof W.App.networkCacheMaximumSize === "number"
    readonly property string probeWhy: typeof W.App.networkCacheSize
        + " / " + typeof W.App.networkCacheMaximumSize
"#,
    },
    Case {
        name: "PluginTranslations.revision — чтение",
        body: r#"
    readonly property bool probeOk: typeof W.PluginTranslations.revision === "number"
    readonly property string probeWhy: typeof W.PluginTranslations.revision
"#,
    },
    // ── 4. Инвокаблы с числовым параметром ────────────────────────────────
    Case {
        name: "DisplayManager.get(id) — вызов",
        body: r#"
    property string why: "не звали"
    readonly property bool probeOk: run()
    readonly property string probeWhy: why
    function run() {
        try { W.App.displayManager.get(1); why = "вызов прошёл"; return true; }
        catch (e) { why = "" + e; return false; }
    }
"#,
    },
    Case {
        name: "App.setNetworkCacheMaximumSize(size) — вызов и возврат",
        body: r#"
    property string why: "не звали"
    readonly property bool probeOk: run()
    readonly property string probeWhy: why
    function run() {
        try {
            W.App.setNetworkCacheMaximumSize(64 * 1024 * 1024);
            const back = W.App.networkCacheMaximumSize;
            why = "" + back + " / " + typeof back;
            return Number(back) === 64 * 1024 * 1024;
        } catch (e) { why = "" + e; return false; }
    }
"#,
    },
    Case {
        name: "PlaylistItemSelectStorage.beginPlaylistItems(id, revision) — вызов и возврат",
        body: r#"
    property QtObject target: W.PlaylistItemSelectStorage { }
    property string why: "не звали"
    readonly property bool probeOk: run()
    readonly property string probeWhy: why
    function run() {
        try {
            target.beginPlaylistItems(3, 4, []);
            why = "" + target.playlistId + " / " + target.revision;
            return Number(target.playlistId) === 3 && Number(target.revision) === 4;
        } catch (e) { why = "" + e; return false; }
    }
"#,
    },
    Case {
        name: "PlaylistMutationQuery.setItems(id, items, revision) — вызов",
        body: r#"
    property QtObject target: W.PlaylistMutationQuery { }
    property string why: "не звали"
    readonly property bool probeOk: run()
    readonly property string probeWhy: why
    function run() {
        try { target.setItems(5, [], 6); why = "вызов прошёл"; return true; }
        catch (e) { why = "" + e; return false; }
    }
"#,
    },
    Case {
        name: "CanvasMutationQuery.updateCanvas/removeCanvas(expectedRevision) — вызов",
        body: r#"
    property QtObject target: W.CanvasMutationQuery { }
    property string why: "не звали"
    readonly property bool probeOk: run()
    readonly property string probeWhy: why
    function run() {
        try {
            target.updateCanvas("c", 1, "n", []);
            target.removeCanvas("c", 2);
            why = "оба вызова прошли";
            return true;
        } catch (e) { why = "" + e; return false; }
    }
"#,
    },
    // ── 5. Обработчик сигнала с числовым параметром ───────────────────────
    Case {
        name: "CanvasMutationQuery.onCanvasUpdated — обработчик",
        body: r#"
    property QtObject target: W.CanvasMutationQuery {
        onCanvasUpdated: function (canvasId, revision) { }
    }
    readonly property bool probeOk: target !== null
    readonly property string probeWhy: "обработчик объявлен"
"#,
    },
    // ── 6. Оборотная сторона починки: `%1` от `double` ────────────────────
    //
    // ⚠️ Замена `i64` на `f64` чинит привязку, но заводит ВТОРОЙ видимый
    // покупателю риск. `QString::arg()` для `double` печатает формат 'g' с
    // шестью значащими цифрами: с 1 000 000 число уходит в экспоненту, и
    // вендорный QML показал бы «Playlist #1.23457e+06» вместо номера.
    //
    // Замерено 31.08.2026 живым движком (`qml6`, Qt 6):
    //
    //     .arg(double 999999)    → «999999»      — цел
    //     .arg(double 1000000)   → «1e+06»       — ОБРЫВ
    //     .arg(double 1234567)   → «1.23457e+06»
    //     .arg(int 1234567)      → «1234567»     — цел
    //     "" + double            → «1234567»     — цел
    //     .arg(String(double))   → «1234567»     — цел
    //
    // ⚠️ «Двойственность» ЛИПКАЯ: `Number(x)`, параметр JS-функции,
    // свойство `var`, поле объектного литерала — всё сохраняет её, и
    // `.arg()` всё равно уходит в экспоненту. Спасают только явная строка
    // (`String(x)`, склейка через `+`) и целочисленное приведение (`x | 0`).
    //
    // ПОЧЕМУ ВЕНДОРНЫЙ QML НЕ ПРАВИМ. Все шесть мест, где наше `f64`
    // встречается с `.arg()`, несут номера дисплеев и плейлистов — строки
    // базы демона, счёт идёт на единицы и десятки. До рубежа 1e6 им шесть
    // порядков. Единственное значение, которое рубеж заведомо переходит, —
    // размер сетевого кэша в байтах (64 МиБ = 67 108 864), но вендорный
    // `SettingsPage.qml:1124` пропускает его через `formatBytes()`, а тот
    // возвращает СТРОКУ (`toFixed`) — безопасный путь. Править чужой QML
    // ради недостижимого случая значило бы ломать паритет и ссориться с
    // каждым обновлением waywallen.
    //
    // Поэтому не правка, а ЗАМОК: случаи ниже держат оба конца — что при
    // рабочих величинах номер печатается цифрами, и что рубеж лежит именно
    // там, где замерено. Уедет любой конец — проба скажет.
    Case {
        name: "%1 от f64-свойства при рабочем номере — цифры, не экспонента",
        body: r##"
    property QtObject target: W.LibraryRemoveQuery { libraryId: 42 }
    readonly property string shown: qsTr("#%1").arg(target.libraryId)
    readonly property bool probeOk: shown === "#42"
    readonly property string probeWhy: shown
"##,
    },
    Case {
        name: "%1 от f64-свойства у потолка номеров (999999) — ещё цифры",
        body: r##"
    property QtObject target: W.LibraryRemoveQuery { libraryId: 999999 }
    readonly property string shown: qsTr("#%1").arg(target.libraryId)
    readonly property bool probeOk: shown === "#999999"
    readonly property string probeWhy: shown
"##,
    },
    Case {
        name: "рубеж %1: с 1e6 arg(double) уходит в экспоненту (замер, не пожелание)",
        body: r##"
    property QtObject target: W.LibraryRemoveQuery { libraryId: 1000000 }
    readonly property string shown: qsTr("#%1").arg(target.libraryId)
    // ⚠️ Ждём ИМЕННО экспоненту: это замер поведения Qt, а не пожелание.
    // Сойдёт с места — значит Qt переменился, и запас можно пересчитать.
    readonly property bool probeOk: shown === "#1e+06"
    readonly property string probeWhy: shown
"##,
    },
    Case {
        name: "большое f64 через String() — безопасный путь (как formatBytes)",
        body: r##"
    property QtObject target: W.LibraryRemoveQuery { libraryId: 67108864 }
    readonly property string shown: qsTr("#%1").arg(String(target.libraryId))
    readonly property bool probeOk: shown === "#67108864"
    readonly property string probeWhy: shown
"##,
    },
    Case {
        name: "networkCacheMaximumSize через formatBytes — без экспоненты",
        body: r#"
    function formatBytes(bytes) {
        let value = Math.max(0, Number(bytes ?? 0));
        const units = ["B", "KiB", "MiB", "GiB", "TiB"];
        let index = 0;
        while (value >= 1024 && index < units.length - 1) { value /= 1024; ++index; }
        return value.toFixed(index === 0 ? 0 : 1) + " " + units[index];
    }
    property string why: "не звали"
    readonly property bool probeOk: run()
    readonly property string probeWhy: why
    function run() {
        W.App.setNetworkCacheMaximumSize(64 * 1024 * 1024);
        const shown = qsTr("%1").arg(formatBytes(W.App.networkCacheMaximumSize));
        why = shown;
        return shown.indexOf("e+") < 0 && shown === "64.0 MiB";
    }
"#,
    },
    // ── Не про 64 бита: договор с разметкой waywallen ────────────────────
    // Стенд один на все проверки типов `W.*` в живом движке, поэтому эти
    // случаи живут здесь, а не в отдельной пробе.
    //
    // Проверка обновлений waywallen (`W.UpdateChecker`, у апстрима с 0.4.4,
    // поле `supported` — с #328). Движок едет внутри пакета Rexon и
    // обновляется вместе с ним, поэтому Rexon даёт тот же синглтон, что
    // сборка апстрима без `update-check`: не поддерживается, выключен,
    // запись `enabled` и `check()` ничего не делают. Разметка новее 0.4.4
    // сама прячет по `supported` кнопки и выключатель.
    Case {
        name: "UpdateChecker — заглушка: не поддерживается, выключена, запись не проходит",
        body: r#"
    readonly property bool before: W.UpdateChecker.supported === false
        && W.UpdateChecker.enabled === false
        && W.UpdateChecker.checking === false
        && W.UpdateChecker.updateAvailable === false
        && W.UpdateChecker.latestVersion === ""
        && W.UpdateChecker.releaseUrl.toString() === ""
    property bool after: false
    Component.onCompleted: {
        W.UpdateChecker.enabled = true;
        W.UpdateChecker.check();
        after = W.UpdateChecker.enabled === false && W.UpdateChecker.checking === false;
    }
    readonly property bool probeOk: before && after
    readonly property string probeWhy: "supported=" + W.UpdateChecker.supported
        + " enabled=" + W.UpdateChecker.enabled + " checking=" + W.UpdateChecker.checking
"#,
    },
    // Логотип и журнал выпусков: разметка апстрима ищет их по абсолютному
    // адресу `qrc:/waywallen/ui/assets/…` — в ресурсах СВОЕГО бинаря. Rexon
    // кладёт те же файлы по тому же адресу в свои ресурсы, и QML не правится.
    Case {
        name: "логотип waywallen по адресу qrc: апстрима",
        body: r#"
    property Image logo: Image { source: "qrc:/waywallen/ui/assets/waywallen-ui.svg" }
    readonly property bool probeOk: logo.status === Image.Ready
    readonly property string probeWhy: "status=" + logo.status
"#,
    },
    Case {
        name: "журнал выпусков waywallen по адресу qrc: апстрима",
        body: r#"
    readonly property string text: read("qrc:/waywallen/ui/assets/waywallen-ui.releases.xml")
    readonly property bool probeOk: text.indexOf("<release") >= 0
    readonly property string probeWhy: "прочитано " + text.length + " символов"
    function read(url) {
        const xhr = new XMLHttpRequest();
        xhr.open("GET", url, false);
        xhr.send();
        return xhr.responseText || "";
    }
"#,
    },
    // Один модуль из двух половин: QML-синглтон `I18n` объявлен вендорным
    // qmldir (версия модуля апстрима, 0.3), а C++-синглтон
    // `PluginTranslations`, которого он зовёт, регистрирует Rust (1.0).
    // Разметка импортирует модуль без версии и обязана видеть обе.
    Case {
        name: "вендорный I18n (qmldir 0.3) зовёт наш PluginTranslations (1.0)",
        body: r#"
    // `I18n.revision` — привязка к `PluginTranslations.revision`, `tr()`
    // зовёт `PluginTranslations.translate()`: обе живы — модуль один.
    readonly property bool probeOk: typeof W.I18n.revision === "number"
        && W.I18n.revision === W.PluginTranslations.revision
        && typeof W.I18n.tr("Plain text") === "string"
    readonly property string probeWhy: "I18n.revision=" + W.I18n.revision
        + " PluginTranslations.revision=" + W.PluginTranslations.revision
"#,
    },    // waywallen 0.5.0: скрытые библиотеки у `WallpaperListQuery`. Номера —
    // список чисел-double (64-битное целое в QML не выставляется): страница
    // зовёт `Array.from(hiddenLibraryIds)`, `.includes(library.id)` и
    // `.length`, а глазик — `toggleLibraryHidden(id)`.
    Case {
        name: "WallpaperListQuery.hiddenLibraryIds: [3,1,3] → [1,3], toggle(1) → [3]",
        body: r#"
    property QtObject query: W.WallpaperListQuery {}
    // Расчёт в onCompleted, а не привязкой: он сам пишет свойства запроса,
    // и привязка к ним дала бы петлю.
    property string why: "не звали"
    property bool probeOk: false
    readonly property string probeWhy: why
    Component.onCompleted: probeOk = run()
    function run() {
        const fresh = !query.hasActiveFilters && !query.hasOtherFilters
            && Array.from(query.hiddenLibraryIds).length === 0;
        let ids = 0, active = 0;
        query.hiddenLibraryIdsChanged.connect(() => ++ids);
        query.hasActiveFiltersChanged.connect(() => ++active);
        query.hiddenLibraryIds = [3, 1, 3];
        const normalized = JSON.stringify(Array.from(query.hiddenLibraryIds));
        const flags = query.hasActiveFilters && !query.hasOtherFilters;
        query.hiddenLibraryIds = [1, 3];
        query.toggleLibraryHidden(1);
        const toggled = JSON.stringify(Array.from(query.hiddenLibraryIds));
        const includes = Array.from(query.hiddenLibraryIds).includes(3);
        let other = 0;
        query.hasOtherFiltersChanged.connect(() => ++other);
        query.skipTypes = ["video"];
        const otherOn = query.hasOtherFilters && other === 1;
        why = "fresh=" + fresh + " normalized=" + normalized + " flags=" + flags
            + " toggled=" + toggled + " includes=" + includes + " signals=" + ids
            + "/" + active + " other=" + otherOn;
        return fresh && normalized === "[1,3]" && flags && toggled === "[3]"
            && includes && ids === 2 && active === 1 && otherOn;
    }
"#,
    },
    // waywallen 0.5.0: `DisplaysPage.qml` объявляет свойства ТИПОВ `W.Display`
    // и `W.Canvas` (`selected as W.Display`). Неизвестный тип роняет
    // страницу «Мониторы» целиком — типы обязаны быть в модуле.
    Case {
        name: "W.Display и W.Canvas — типы модуля waywallen.ui",
        body: r#"
    property var selected: null
    readonly property W.Display selectedDisplay: selected as W.Display
    readonly property W.Canvas selectedCanvas: selected as W.Canvas
    readonly property bool probeOk: selectedDisplay === null && selectedCanvas === null
    readonly property string probeWhy: "display=" + selectedDisplay + " canvas=" + selectedCanvas
"#,
    },
    // waywallen 0.5.0: аватар автора в панели деталей Discover
    // (`details?.authorAvatarUrl`) — строка, пустая до ответа демона.
    Case {
        name: "RemoteDetailsQuery.authorAvatarUrl — пустая строка до ответа",
        body: r#"
    property QtObject details: W.RemoteDetailsQuery {}
    readonly property bool probeOk: details.authorAvatarUrl === ""
    readonly property string probeWhy: "authorAvatarUrl=" + details.authorAvatarUrl
"#,
    },
    // waywallen 0.5.0: выборка и страницы `WallpaperListQuery` так, как их
    // зовёт `PlaylistEditPage.qml`: `baselineSelection = previewQuery.selection`
    // (непрозрачное значение, в JS — ArrayBuffer), `restoreSelection`,
    // `onSelectionChanged → matchesSelection(baseline)`, `limit: 50` и
    // `fetchMore(50)` в конце прокрутки.
    Case {
        name: "WallpaperListQuery.selection: restore/matches, selectionChanged на каждый сеттер, fetchMore",
        body: r#"
    property QtObject query: W.WallpaperListQuery {}
    property string why: "не звали"
    property bool probeOk: false
    readonly property string probeWhy: why
    Component.onCompleted: probeOk = run()
    function run() {
        const base = query.selection;
        const opaque = base instanceof ArrayBuffer;
        let sel = 0;
        query.selectionChanged.connect(() => ++sel);
        // Восстановление текущей выборки — молчание (и без перезагрузки).
        query.restoreSelection(base);
        const silent = sel === 0 && query.matchesSelection(base);
        const notSelection = !query.matchesSelection(undefined)
            && !query.matchesSelection(null) && !query.matchesSelection("x");
        // `limit` до первого запуска: selectionChanged, но не запрос.
        query.limit = 50;
        const limitOk = sel === 1 && query.status === 0;
        sel = 0;
        query.wpType = "scene";
        query.searchText = "закат";
        query.sorts = [{key: 3, direction: 2}];
        query.filters = [{type: 1, group: 0, stringFilter: {value: "море", condition: 2}}];
        query.filterLogics = [{op: 1, groupA: 0, groupB: 1}];
        query.skipTypes = ["video"];
        query.filterTags = ["природа"];
        query.skipContentRatings = ["mature"];
        query.hiddenFilter = 2;
        query.hiddenLibraryIds = [4];
        const each = sel === 10;
        const changed = !query.matchesSelection(base);
        const full = query.selection;
        query.restoreSelection(base);
        const back = query.matchesSelection(base) && query.wpType === ""
            && query.filters.length === 0 && Array.from(query.hiddenLibraryIds).length === 0;
        query.restoreSelection(full);
        const again = query.matchesSelection(full) && query.searchText === "закат"
            && query.hiddenFilter === 2 && query.filters.length === 1;
        const paging = typeof query.fetchMore === "function" && query.offset === 0
            && query.noMore === false && query.limit === 50;
        // Без демона догрузка уходит в «нет соединения», но вызов обязан
        // существовать и принимать число (иначе TypeError в журнале).
        query.fetchMore(50);
        why = "opaque=" + opaque + " silent=" + silent + " notSelection=" + notSelection
            + " limit=" + limitOk + " each=" + each + "(" + sel + ") changed=" + changed
            + " back=" + back + " again=" + again + " paging=" + paging;
        return opaque && silent && notSelection && limitOk && each && changed && back
            && again && paging;
    }
"#,
    },
    // waywallen 0.5.0: умный плейлист так, как его сохраняет
    // `PlaylistEditPage.qml`: `saveSmart(playlistId, …, previewQuery.selection,
    // draftRevision)` с числами-`double`, а страница создания держит
    // `PlaylistDetailQuery { playlistId: 0 }` — тот обязан молчать.
    Case {
        name: "PlaylistMutationQuery.saveSmart и PlaylistDetailQuery с id 0 (waywallen 0.5.0)",
        body: r#"
    property QtObject preview: W.WallpaperListQuery { limit: 50 }
    property QtObject detail: W.PlaylistDetailQuery { playlistId: 0; forwardError: false }
    property QtObject mutation: W.PlaylistMutationQuery { forwardError: false }
    property string why: "не звали"
    property bool probeOk: false
    readonly property string probeWhy: why
    Component.onCompleted: probeOk = run()
    function run() {
        let done = 0;
        mutation.done.connect(() => ++done);
        detail.reload();
        const quiet = detail.status === 0 && detail.error === "";
        try {
            // Кадры и ответы сверяет стенд страницы «Обои»
            // (`run_v050_playlists`); здесь — что вызов с числами-`double`
            // и выборкой-`ArrayBuffer` движок принимает.
            mutation.saveSmart(0, "Новый", 1, 300, true, preview.selection, 0);
            mutation.saveSmart(7, "Море", 2, 600, false, preview.selection, 3);
        } catch (e) { why = "" + e; return false; }
        const sent = true;
        // Не выборка: кадра нет, сразу ошибка и ровно один `done()`.
        const before = done;
        mutation.saveSmart(7, "Море", 2, 600, false, undefined, 3);
        const invalid = done === before + 1 && mutation.status === 3 && mutation.error !== "";
        why = "quiet=" + quiet + "(" + detail.status + "/" + detail.error + ") sent=" + sent
            + " invalid=" + invalid + " done=" + done;
        return quiet && sent && invalid;
    }
"#,
    },
    // waywallen 0.5.0, патч 0002: строка «Версия %1» на «О программе» берёт
    // версию вендорного интерфейса, а не `Qt.application.version` (у Rexon
    // это его собственная версия). Вид — как у версии демона по D-Bus.
    Case {
        name: "App.uiVersion — версия вендорного интерфейса из ui/UPSTREAM_TAG",
        body: r#"
    readonly property string version: W.App.uiVersion
    readonly property bool probeOk: /^[0-9]+(\.[0-9]+)+$/.test(version)
    readonly property string probeWhy: "uiVersion=" + version
"#,
    },
    // Наш QML и вендорная разметка против СОБРАННОГО `Qcm.Material` пина
    // (`ui/QML_MATERIAL_PIN`) и наших типов `waywallen.ui`. Ловит то, чего не
    // видят ни qmllint (свойств модуля он без описи не знает), ни
    // контракт-тест (он сверяет имена «по слову»): присваивание
    // несуществующему свойству и обработчик несуществующего сигнала —
    // ошибка КОМПИЛЯЦИИ документа, и страница не открывается целиком.
    // Пойманный класс: переход QmlMaterial на InputHost (0.5.0) — у
    // `StandardIconButton` нет `checked`, у пунктов рейки `onClicked`.
    // Список файлов подставляет `run()` (`@QML_FILES@`).
    Case {
        name: "PageHost и вендорные страницы компилируются против Qcm.Material пина",
        body: r#"
    readonly property var files: @QML_FILES@
    readonly property var broken: files.filter(f => {
        const c = Qt.createComponent(f, Component.PreferSynchronous);
        const ok = c.status === Component.Ready;
        if (!ok)
            console.log("REXON-I64 не компилируется: " + c.errorString().trim());
        c.destroy();
        return !ok;
    })
    readonly property bool probeOk: files.length > 1 && broken.length === 0
    readonly property string probeWhy: "файлов " + files.length + ", не компилируются: "
        + broken.map(f => f.split("/qml_ui/")[1]).join(", ")
"#,
    },
];

/// Файлы QML, которые случай компиляции грузит по одному: наш `PageHost.qml`
/// и каждый `.qml` вендорной разметки. JSON-массив адресов `file://`.
fn compile_targets() -> String {
    let base = crate::we_env::vendor_qml_dir();
    if base.is_empty() {
        return "[]".to_string();
    }
    let mut files = vec![rexon_core::qml_modules::qml_path(
        &base,
        rexon_core::qml_modules::PAGE_HOST_QML,
    )];
    let mut stack = vec![std::path::PathBuf::from(rexon_core::remote_l10n::default_qml_dir(&base))];
    let mut vendor = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "qml") {
                vendor.push(path.to_string_lossy().into_owned());
            }
        }
    }
    vendor.sort();
    files.extend(vendor);
    serde_json::to_string(
        &files
            .iter()
            .map(|f| format!("file://{f}"))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string())
}

/// Шапка документа случая — общая на все.
// QtQuick — ради `Image` в случае логотипа.
const CASE_HEADER: &str =
    "import QtQml\nimport QtQuick\nimport waywallen.ui as W\n\nQtObject {\n    id: root\n";

/// Сколько ждать приговора. Срок держит `Timer` ВНУТРИ цикла событий Qt, а
/// не сторонний поток: посторонний поток, живой в миг выхода, роняет разбор
/// статики Qt.
const GUARD_MS: i32 = 30_000;

/// Собрать бегуна: он проходит случаи по одному и печатает приговор.
fn runner_qml(dir: &std::path::Path, names: &[String]) -> String {
    let dir = dir.display();
    let list = names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"import QtQml

QtObject {{
    id: root

    readonly property var files: [{list}]
    readonly property string dir: "file://{dir}/"

    // Сторож срока — в ГЛАВНОМ потоке, внутри цикла событий.
    property Timer guard: Timer {{
        interval: {GUARD_MS}
        running: true
        onTriggered: {{
            console.log("REXON-I64 проба не отчиталась за {GUARD_MS} мс");
            Qt.exit(2);
        }}
    }}

    // Нулевой интервал: случаи проигрываются, когда цикл событий УЖЕ
    // работает, иначе Qt.exit() из onCompleted некому услышать.
    property Timer go: Timer {{
        interval: 0
        running: true
        onTriggered: root.runAll()
    }}

    function runAll() {{
        guard.running = false;
        let passed = 0;
        const failures = [];
        for (const file of files) {{
            const comp = Qt.createComponent(dir + file + ".qml");
            if (comp.status === Component.Error) {{
                failures.push(file + " — " + comp.errorString().trim());
                continue;
            }}
            const obj = comp.createObject(root);
            if (obj === null) {{
                failures.push(file + " — объект не создан");
                continue;
            }}
            if (obj.probeOk) {{
                console.log("REXON-I64 OK   " + file);
                ++passed;
            }} else {{
                failures.push(file + " — " + obj.probeWhy);
            }}
            obj.destroy();
        }}
        for (const line of failures)
            console.log("REXON-I64 FAIL " + line);
        console.log("REXON-I64 ИТОГ прошло " + passed + ", провалено " + failures.length);
        Qt.exit(failures.length === 0 ? 0 : 1);
    }}
}}
"#
    )
}

/// Имя файла случая: без пробелов и точек, чтобы годиться в URL и в имя
/// типа QML (первая буква обязана быть заглавной).
fn file_name(index: usize) -> String {
    format!("Case{index:02}")
}

/// ⚠️ Возврата НЕТ намеренно: приговор уходит `std::process::exit` прямо
/// отсюда, как в соседних самопроверках `demo.rs`.
pub fn run() -> ! {
    unsafe { std::env::set_var("QT_FORCE_STDERR_LOGGING", "1") };
    // Случай журнала выпусков читает `qrc:` через XMLHttpRequest, а Qt
    // считает это чтением локального файла и без переменной отказывает.
    unsafe { std::env::set_var("QML_XHR_ALLOW_FILE_READ", "1") };
    // Модуль вкладок — ради статических архивов cxx-qt (та же причина, что
    // в стенде `demo.rs`).
    cxx_qt::init_qml_module!("org.rexon.gui");

    // ⚠️ Типы `waywallen.ui` регистрирует `prepare_process` ниже — ПОСЛЕ
    // поиска каталога пакета, как у живого окна. Прежде проба звала
    // регистрацию первой строкой, ещё до приложения: синглтон `App` (он
    // создаётся сразу при регистрации) рождался без каталога пакета — без
    // языков вкладки и без `uiVersion`, — и проба мерила не то, что видит
    // покупатель.
    let mut app = QGuiApplication::new();
    // Процесс — как у живого окна: каталог пакета (вендорный qmldir),
    // модель фильтров, прогрев `waywallen.control`. Звать при живом
    // приложении — и ДО записи случаев: список файлов для случая компиляции
    // берётся из найденного здесь каталога пакета.
    crate::we_env::prepare_process();
    let targets = compile_targets();

    let dir = std::env::temp_dir().join(format!("rexon-i64-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("каталог пробы");

    let mut names = Vec::new();
    for (index, case) in CASES.iter().enumerate() {
        let name = file_name(index);
        let body = format!(
            "{CASE_HEADER}{}}}\n",
            case.body.replace("@QML_FILES@", &targets)
        );
        std::fs::write(dir.join(format!("{name}.qml")), body).expect("файл случая");
        names.push(name);
    }
    // Имена случаев печатаем сразу: в выводе движка видно только имя файла.
    for (index, case) in CASES.iter().enumerate() {
        eprintln!("REXON-I64 {} = {}", file_name(index), case.name);
    }

    let runner = dir.join("Int64Probe.qml");
    std::fs::write(&runner, runner_qml(&dir, &names)).expect("файл бегуна");

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        // Пути импорта — как у живой вкладки: вендорный qmldir модуля
        // `waywallen.ui` и модули под ним (Qcm.Material, QExtra, …). Без
        // вендорного qmldir случаи видели бы только C++-половину модуля.
        let raw = unsafe { engine.as_mut().get_unchecked_mut() } as *mut QQmlApplicationEngine;
        unsafe { crate::we_env::prepare_engine(raw.cast()) };
        engine.as_mut().load(&QUrl::from(
            format!("file://{}", runner.display()).as_str(),
        ));
    }
    let code = match app.as_mut() {
        Some(app) => app.exec(),
        None => 2,
    };
    let _ = std::fs::remove_dir_all(&dir);
    // Ни `app`, ни `engine` не разбираются: то же, что делают соседние
    // самопроверки в `demo.rs`.
    std::process::exit(code);
}
