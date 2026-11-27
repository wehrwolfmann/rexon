// The "64-bit integer in QML" probe: a live QML engine against the cxx-qt
// bridges.
//
// WHAT EXACTLY BREAKS. cxx-qt emits `i64`/`u64` as `::std::int64_t` /
// `::std::uint64_t`, and on 64-bit Linux these are `long` / `unsigned long`
// — SEPARATE Qt metatypes, not the same as `qlonglong`/`qulonglong`. The
// QML engine does not know them, and a LITERAL declarative binding to such
// a property fails: "Cannot assign value 7 to property displayId". The
// failure takes the WHOLE document down — the page is not created at all.
//
// Warning: neither the compiler, nor `cargo test`, nor the type gate
// `scripts/test_we_rust_types.py` sees this: both the build and the
// signatures agree. It breaks only on screen. Hence the probe: a
// measurement with a live engine, not source analysis.
//
// What was measured on 31.08.2026 on Qt 6 (CachyOS), before the fix:
//
//   * `Type { prop: 7 }` into a `long`/`unsigned long` property — FAILS,
//     the document is dead;
//   * `prop: root.someInt` and `prop: root.someReal` — work;
//   * reading such a property — arrives as a number;
//   * an invokable and a signal handler with such a parameter — work;
//   * a 64-bit integer INSIDE a `QVariant` — reads as a number (`QVariant`
//     carries the metatype separately, and Qt's JS wrapper unwraps it).
//
// Working today does not mean "can be left as is": the difference between
// a literal and a binding rests on the engine's internals, not on a
// contract. So a 64-bit integer is NOT exposed at ALL (`src/qvalue.rs`),
// and the `qml_int64` guard in `src/lib.rs` fails `cargo test` if such a
// declaration appears again. The probe is the empirical half: it proves
// that the live engine accepts the current bridge.
//
// Run:
//     QT_QPA_PLATFORM=offscreen \
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --qml-int64-probe
//
// Return: 0 — all cases passed, 1 — there are failures, 2 — the probe did
// not report (the engine did not build the root document or did not make
// the deadline).

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

pub const ARG: &str = "--qml-int64-probe";

/// One probe case: the name and the body of a QML document.
///
/// Warning: each case is a SEPARATE file that the runner loads with
/// `Qt.createComponent`. This is not embellishment: a binding failure takes
/// the whole document down, and in one file the very first failure would
/// hide all the other cases. With separate files a failure is caught by
/// name.
///
/// The body must declare `probeOk` (the verdict) and `probeWhy` (what was
/// seen).
struct Case {
    name: &'static str,
    body: &'static str,
}

/// The expected value in all cases is 7 (`root.probeInt` / `root.probeReal`).
const CASES: &[Case] = &[
    // ── 1. A literal binding: THE very case of the class ─────────────────
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
        name: "LibraryEditQuery.libraryId ← литерал (waywallen 0.6.0)",
        body: r#"
    property QtObject target: W.LibraryEditQuery { libraryId: 7; name: "Обои" }
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
        name: "LibraryAddQuery/LibraryEditQuery.settings — карта из QML (waywallen 0.6.0)",
        body: r#"
    property QtObject target: W.LibraryAddQuery { settings: ({ recursive: true, depth: 3 }) }
    property QtObject edit: W.LibraryEditQuery { libraryId: 7; settings: ({ recursive: false }) }
    readonly property bool probeOk: target.settings.recursive === true
        && Number(target.settings.depth) === 3 && edit.settings.recursive === false
    readonly property string probeWhy: JSON.stringify(target.settings) + " / "
        + JSON.stringify(edit.settings)
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
    // ── 2. Binding from a property: int and double as the source ──────────
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
    // ── 3. Reading read-only properties ───────────────────────────────────
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
    // ── 4. Invokables with a numeric parameter ────────────────────────────
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
    // ── 5. A signal handler with a numeric parameter ──────────────────────
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
    // ── 6. The flip side of the fix: `%1` from a `double` ─────────────────
    //
    // Warning: replacing `i64` with `f64` fixes the binding but introduces a
    // SECOND risk visible to the customer. `QString::arg()` for a `double`
    // prints the 'g' format with six significant digits: from 1 000 000 the
    // number goes into exponent notation, and the vendored QML would show
    // "Playlist #1.23457e+06" instead of the number.
    //
    // Measured on 31.08.2026 with a live engine (`qml6`, Qt 6):
    //
    //     .arg(double 999999)    → "999999"      — intact
    //     .arg(double 1000000)   → "1e+06"       — BROKEN
    //     .arg(double 1234567)   → "1.23457e+06"
    //     .arg(int 1234567)      → "1234567"     — intact
    //     "" + double            → "1234567"     — intact
    //     .arg(String(double))   → "1234567"     — intact
    //
    // Warning: the "doubleness" is STICKY: `Number(x)`, a JS function
    // parameter, a `var` property, an object literal field — all preserve it,
    // and `.arg()` still goes into exponent notation. Only an explicit string
    // (`String(x)`, concatenation with `+`) and an integer cast (`x | 0`) help.
    //
    // WHY WE DO NOT EDIT THE VENDORED QML. All six places where our `f64` meets
    // `.arg()` carry display and playlist numbers — rows of the daemon's
    // database, counted in ones and tens. They are six orders of magnitude
    // away from the 1e6 boundary. The only value that certainly crosses the
    // boundary is the network cache size in bytes (64 MiB = 67 108 864), but
    // the vendored `SettingsPage.qml:1124` passes it through `formatBytes()`,
    // which returns a STRING (`toFixed`) — a safe path. Editing someone else's
    // QML for an unreachable case would mean breaking parity and fighting
    // every waywallen update.
    //
    // So not an edit but a LOCK: the cases below hold both ends — that at
    // working values the number prints as digits, and that the boundary lies
    // exactly where it was measured. If either end moves, the probe will say
    // so.
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
    // ── Not about 64 bits: the contract with the waywallen markup ────────
    // The bench is one for all checks of the `W.*` types in a live engine, so
    // these cases live here rather than in a separate probe.
    //
    // The waywallen update check (`W.UpdateChecker`, upstream since 0.4.4, the
    // `supported` field since #328). The engine ships inside the Rexon package
    // and is updated together with it, so Rexon provides the same singleton as
    // an upstream build without `update-check`: not supported, disabled,
    // writing `enabled` and `check()` do nothing. Markup newer than 0.4.4 hides
    // the buttons and the switch by itself based on `supported`.
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
    // Logo and release log: the upstream markup looks for them by the absolute
    // address `qrc:/waywallen/ui/assets/…` — in the resources of ITS OWN binary.
    // Rexon puts the same files at the same address into its own resources,
    // and the QML is not edited.
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
    // One module of two halves: the QML singleton `I18n` is declared by the
    // vendored qmldir (the upstream module version, 0.3), while the C++
    // singleton `PluginTranslations` it calls is registered by Rust (1.0). The
    // markup imports the module without a version and must see both.
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
    },    // waywallen 0.5.0: hidden libraries of `WallpaperListQuery`. The numbers are
    // a list of double numbers (a 64-bit integer is not exposed to QML): the
    // page calls `Array.from(hiddenLibraryIds)`, `.includes(library.id)` and
    // `.length`, and the eye icon calls `toggleLibraryHidden(id)`.
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
    // waywallen 0.5.0: `DisplaysPage.qml` declares properties of the TYPES
    // `W.Display` and `W.Canvas` (`selected as W.Display`). An unknown type
    // crashes the whole "Monitors" page — the types must be in the module.
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
    // waywallen 0.5.0: the author's avatar in the Discover details panel
    // (`details?.authorAvatarUrl`) — a string, empty until the daemon answers.
    Case {
        name: "RemoteDetailsQuery.authorAvatarUrl — пустая строка до ответа",
        body: r#"
    property QtObject details: W.RemoteDetailsQuery {}
    readonly property bool probeOk: details.authorAvatarUrl === ""
    readonly property string probeWhy: "authorAvatarUrl=" + details.authorAvatarUrl
"#,
    },
    // waywallen 0.5.0: the selection and pages of `WallpaperListQuery` the way
    // `PlaylistEditPage.qml` calls them:
    // `baselineSelection = previewQuery.selection` (an opaque value, an
    // ArrayBuffer in JS), `restoreSelection`,
    // `onSelectionChanged → matchesSelection(baseline)`, `limit: 50` and
    // `fetchMore(50)` at the end of scrolling.
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
    // waywallen 0.5.0: a smart playlist the way `PlaylistEditPage.qml` saves
    // it: `saveSmart(playlistId, …, previewQuery.selection, draftRevision)` with
    // `double` numbers, while the creation page holds
    // `PlaylistDetailQuery { playlistId: 0 }` — that one must stay silent.
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
    // waywallen 0.5.0, patch 0002: the "Version %1" line on "About" takes the
    // version of the vendored interface, not `Qt.application.version` (for
    // Rexon that is its own version). The look is like the daemon version over
    // D-Bus.
    Case {
        name: "App.uiVersion — версия вендорного интерфейса из ui/UPSTREAM_TAG",
        body: r#"
    readonly property string version: W.App.uiVersion
    readonly property bool probeOk: /^[0-9]+(\.[0-9]+)+$/.test(version)
    readonly property string probeWhy: "uiVersion=" + version
"#,
    },
    // Our QML and the vendored markup against the BUILT `Qcm.Material` pin
    // (`ui/QML_MATERIAL_PIN`) and our `waywallen.ui` types. Catches what
    // neither qmllint (it does not know the module's properties without a
    // type description) nor the contract test (it compares names "by word")
    // sees: assigning to a non-existent property and a handler of a
    // non-existent signal are document COMPILATION errors, and the page does
    // not open at all. The class caught: the move of QmlMaterial to InputHost
    // (0.5.0) — `StandardIconButton` has no `checked`, rail items have no
    // `onClicked`. The file list is substituted by `run()` (`@QML_FILES@`).
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

/// QML files the compilation case loads one by one: our `PageHost.qml` and
/// every `.qml` of the vendored markup. A JSON array of `file://` addresses.
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

/// Header of a case document — shared by all.
// QtQuick — for `Image` in the logo case.
const CASE_HEADER: &str =
    "import QtQml\nimport QtQuick\nimport waywallen.ui as W\n\nQtObject {\n    id: root\n";

/// How long to wait for the verdict. The deadline is held by a `Timer`
/// INSIDE the Qt event loop, not by a separate thread: a foreign thread
/// alive at the moment of exit crashes the teardown of Qt statics.
const GUARD_MS: i32 = 30_000;

/// Build the runner: it goes through the cases one by one and prints the
/// verdict.
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

/// Case file name: without spaces or dots, to fit into a URL and into a QML
/// type name (the first letter must be uppercase).
fn file_name(index: usize) -> String {
    format!("Case{index:02}")
}

/// Warning: there is NO return on purpose: the verdict goes out with
/// `std::process::exit` right from here, as in the neighbouring self-tests
/// of `demo.rs`.
pub fn run() -> ! {
    unsafe { std::env::set_var("QT_FORCE_STDERR_LOGGING", "1") };
    // The release log case reads `qrc:` through XMLHttpRequest, and Qt treats
    // that as reading a local file and refuses without the variable.
    unsafe { std::env::set_var("QML_XHR_ALLOW_FILE_READ", "1") };
    // The tabs module — for the sake of the cxx-qt static archives (the same
    // reason as in the `demo.rs` bench).
    cxx_qt::init_qml_module!("org.rexon.gui");

    // Warning: the `waywallen.ui` types are registered by `prepare_process`
    // below — AFTER looking up the package directory, as in the live window.
    // Previously the probe called registration on its first line, even before
    // the application: the `App` singleton (it is created right at
    // registration) was born without the package directory — without the
    // tab's languages and without `uiVersion` — and the probe measured
    // something other than what the customer sees.
    let mut app = QGuiApplication::new();
    // The process is as in the live window: the package directory (the
    // vendored qmldir), the filter model, warming up `waywallen.control`. Call
    // with a live application — and BEFORE recording the cases: the file list
    // for the compilation case is taken from the package directory found here.
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
    // We print the case names right away: the engine's output shows only the
    // file name.
    for (index, case) in CASES.iter().enumerate() {
        eprintln!("REXON-I64 {} = {}", file_name(index), case.name);
    }

    let runner = dir.join("Int64Probe.qml");
    std::fs::write(&runner, runner_qml(&dir, &names)).expect("файл бегуна");

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        // Import paths — as in the live tab: the vendored qmldir of the
        // `waywallen.ui` module and the modules under it (Qcm.Material, QExtra, …).
        // Without the vendored qmldir the cases would see only the C++ half of the
        // module.
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
    // Neither `app` nor `engine` is torn down: the same as the neighbouring
    // self-tests in `demo.rs` do.
    std::process::exit(code);
}
