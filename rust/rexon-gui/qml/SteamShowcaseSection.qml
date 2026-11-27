// The "Profile showcases" sub-tab of the "Steam" tab: your own picture is
// cut to fit a Steam profile showcase, the preview shows the profile with
// the selected background, and the finished parts go into a folder.
//
// Markup only. Decisions (kinds, geometry, validation, cutting,
// compression, profile mock-up colours) — the core
// `rexon_core::steam_showcase`; the executor is `SteamShowcaseActions`
// (rust/rexon-gui/src/steam_showcase_actions.rs).
//
// Never: nothing is uploaded to or changed in Steam: the helper behind the
// "Save parts" button only opens Steam pages for the user, copies names to
// the clipboard, shows the folder and reads the public profile page.
// Everything on the Steam pages is clicked by the user.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section
    RexonBidi { id: bidi }

    property var pal: ({})
    /// Tab styling (`SteamModel.styles`): group, buttons.
    property var sty: ({})
    /// Profile backgrounds from the tab (`SteamModel.bgChoices`).
    property var backgrounds: []
    /// Colour for warnings and errors — ready from the tab core
    /// (`SteamModel.goldColor`).
    property color warnColor: section.pal.text_primary

    signal message(string text)
    /// Request to show pictures large (the shared viewer lives in the tab).
    signal zoomRequested(var items, int index)

    // ── State ──────────────────────────────────────────────────────────

    property var kinds: []
    property string kind: "artwork"

    property string srcPath: ""
    property string srcUrl: ""
    property var srcInfo: null
    property string srcError: ""
    property var srcNotes: []

    property real zoom: 1.0
    property real panX: 0.5
    property real panY: 0.5
    /// Downward offset of the showcase (showcases above it in the profile),
    /// Steam px.
    property int shiftY: 0

    property var lay: null
    property var layWarnings: []

    /// Selected background: a Steam background id, "custom" — own file,
    /// "" — no background.
    property string bgKey: ""
    property string bgCustomPath: ""
    property string bgUrl: ""
    property string bgError: ""
    property bool bgBusy: false
    property int bgSerial: 0

    property bool sliceBusy: false
    property real sliceProgress: 0
    property int sliceSerial: 0
    property var result: null
    property string sliceError: ""

    // ── Upload helper ──────────────────────────────────────────────────

    /// The set the helper is guiding (the `actions.wizard` answer), or null.
    property var wiz: null
    /// Recent sets — one can return to them.
    property var recent: []
    property bool checkBusy: false
    property var checkReport: null
    property string checkError: ""

    function loadWizard(dir) {
        section.setWizard(JSON.parse(actions.wizard(dir)));
        section.checkReport = null;
        section.checkError = "";
    }

    function setWizard(w) {
        section.wiz = w && w.ok ? w : null;
    }

    function say(text) {
        if (text)
            section.message(text);
    }

    function copy(text, note) {
        actions.copyText(text);
        section.message(note);
    }

    function startCheck() {
        if (!section.wiz)
            return;
        const err = actions.startCheck(section.wiz.dir);
        if (err) {
            section.checkError = err;
            return;
        }
        section.checkBusy = true;
        section.checkError = "";
        poll.start();
    }

    /// Part cards for the large view: "in the profile" and "computed".
    function partViewerItems(fromProfile) {
        const out = [];
        if (fromProfile) {
            for (const p of (section.checkReport ? section.checkReport.parts : [])) {
                if (p.found)
                    out.push({ key: "profile-" + p.role, title: p.label,
                               subtitle: bidi.arg(qsTranslate("rexon", "As on your profile: %1×%2 px"), p.shown_width, p.shown_height),
                               thumb: p.image_url, source: p.image_url });
            }
        } else {
            for (const p of (section.wiz ? section.wiz.parts : []))
                out.push({ key: "part-" + p.file, title: p.label, subtitle: p.size,
                           thumb: p.url, source: p.url, animatedImage: p.file.toLowerCase().endsWith(".gif") });
        }
        return out;
    }

    // ── Mock-up (without a picture — a placeholder with the same layout) ─

    readonly property var geo: section.lay ? section.lay : section.placeholderLayout
    readonly property var placeholderLayout: JSON.parse(actions.layout(JSON.stringify({
        kind: section.kind, width: section.kind === "featured" ? 630 : 615, height: 400,
        zoom: 1, panX: 0.5, panY: 0.5 })))
    readonly property var look: section.geo.look || ({})

    function relayout() {
        if (!section.srcInfo) {
            section.lay = null;
            section.layWarnings = [];
            return;
        }
        const r = JSON.parse(actions.layout(JSON.stringify({
            kind: section.kind, width: section.srcInfo.width, height: section.srcInfo.height,
            zoom: section.zoom, panX: section.panX, panY: section.panY })));
        section.lay = r.ok ? r : null;
        section.layWarnings = r.ok ? r.warnings : [r.error];
    }

    function chooseSource(path) {
        if (!path)
            return;
        const r = JSON.parse(actions.inspect(path));
        section.result = null;
        section.sliceError = "";
        if (!r.ok) {
            section.srcError = r.error;
            return;
        }
        section.srcError = "";
        section.srcPath = path;
        section.srcUrl = r.url;
        section.srcInfo = r.info;
        section.srcNotes = r.notes;
        section.zoom = 1.0;
        section.panX = 0.5;
        section.panY = 0.5;
        section.relayout();
    }

    function chooseBackground(key) {
        section.bgKey = key;
        section.bgError = "";
        if (key === "") {
            section.bgUrl = "";
            section.bgBusy = false;
            return;
        }
        let req = { id: "", url: "", path: "" };
        if (key === "custom") {
            req.path = section.bgCustomPath;
        } else {
            const b = section.backgrounds.find(x => x.id === key);
            if (!b)
                return;
            req.id = b.id;
            req.url = b.url;
        }
        section.bgBusy = true;
        actions.startBackground(JSON.stringify(req));
        poll.start();
    }

    function save() {
        const err = actions.startSlice(JSON.stringify({
            path: section.srcPath, kind: section.kind,
            zoom: section.zoom, panX: section.panX, panY: section.panY }));
        if (err !== "") {
            section.message(err);
            return;
        }
        section.result = null;
        section.sliceError = "";
        section.sliceBusy = true;
        section.sliceProgress = 0;
        poll.start();
    }

    onKindChanged: section.relayout()
    onZoomChanged: section.relayout()
    onPanXChanged: section.relayout()
    onPanYChanged: section.relayout()
    // Backgrounds arrive from the network later than the section: as soon as
    // the list arrives, we select the equipped background (if the user has
    // not chosen anything yet).
    onBackgroundsChanged: {
        if (section.bgKey !== "" || !section.backgrounds)
            return;
        const eq = section.backgrounds.find(x => x.equipped === true);
        if (eq)
            section.chooseBackground(eq.id);
    }

    SteamShowcaseActions {
        id: actions
        objectName: "steamShowcaseActions"
    }

    Timer {
        id: poll
        interval: 150
        repeat: true
        onTriggered: {
            const st = JSON.parse(actions.poll());
            const s = st.slice;
            section.sliceProgress = s.progress || 0;
            if (section.sliceBusy && s.done) {
                section.sliceBusy = false;
                if (s.ok) {
                    section.result = s;
                    section.loadWizard(s.report.dir);
                    section.message(qsTranslate("rexon", "Showcase parts saved."));
                } else {
                    section.sliceError = s.error;
                }
            }
            const b = st.bg;
            if (section.bgBusy && b.done) {
                section.bgBusy = false;
                if (b.ok)
                    section.bgUrl = b.url;
                else {
                    section.bgUrl = "";
                    section.bgError = b.error;
                }
            }
            const c = st.check;
            if (section.checkBusy && c.done) {
                section.checkBusy = false;
                if (c.ok) {
                    section.checkReport = c.report;
                    // It matches — the step is done; the state has already been written next
                    // to the parts, all that is left is to re-read it.
                    if (section.wiz)
                        section.setWizard(JSON.parse(actions.wizard(section.wiz.dir)));
                } else {
                    section.checkReport = null;
                    section.checkError = c.error;
                }
            }
            if (!section.sliceBusy && !section.bgBusy && !section.checkBusy)
                poll.stop();
        }
    }

    Component.onCompleted: {
        section.kinds = JSON.parse(actions.kinds());
        section.recent = JSON.parse(actions.recentSets());
        section.backgroundsChanged();
    }

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    readonly property real labelWidth: 220

    ColumnLayout {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "🖼️ Profile Showcases")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
        }
        RexonText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: qsTranslate("rexon", "Rexon cuts your picture to the sizes of a Steam profile showcase and shows how the profile will look. Then it walks you through the upload: opens the right Steam pages, copies the titles and checks the result. Rexon uploads nothing by itself — you press the buttons on the Steam pages.")
        }
        RexonText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: bidi.arg(qsTranslate("rexon", "Showcases are available from Steam level %1."), section.geo.minLevel || 10)
        }

        // ── Picture ───────────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            RexonButton {
                objectName: "showcasePick"
                pal: section.pal
                tone: section.sty.primaryButton
                text: qsTranslate("rexon", "Choose a Picture…")
                onClicked: section.chooseSource(actions.pickImage())
            }
            RexonText {
                Layout.fillWidth: true
                elide: Text.ElideMiddle
                color: section.srcError !== "" ? section.warnColor : section.pal.text_primary
                text: section.srcError !== "" ? "❌ " + section.srcError
                      : !section.srcInfo ? qsTranslate("rexon", "PNG, JPG or GIF (animated GIFs keep their animation).")
                      : bidi.arg(qsTranslate("rexon", "%1 — %2×%3 px, %4"), section.srcPath.split("/").pop(), section.srcInfo.width, section.srcInfo.height, section.srcInfo.format.toUpperCase())
            }
        }
        Repeater {
            model: section.srcNotes
            delegate: RexonText {
                required property var modelData
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: section.pal.text_secondary
                text: "ℹ️ " + modelData
            }
        }

        // ── Showcase kind ─────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            RexonText {
                Layout.preferredWidth: section.labelWidth
                text: qsTranslate("rexon", "Showcase type:")
                color: section.pal.text_primary
            }
            RexonComboBox {
                id: kindCombo
                objectName: "showcaseKind"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "key"
                model: section.kinds
                selectedValue: section.kind
                onActivated: section.kind = kindCombo.currentValue
            }
        }
        RexonText {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: (section.kinds.find(k => k.key === section.kind) || {}).hint || ""
        }

        // ── Profile background ────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            RexonText {
                Layout.preferredWidth: section.labelWidth
                text: qsTranslate("rexon", "Profile background:")
                color: section.pal.text_primary
            }
            RexonComboBox {
                id: bgCombo
                objectName: "showcaseBackground"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "key"
                model: {
                    let m = [{ key: "", label: qsTranslate("rexon", "No background") }];
                    if (section.bgCustomPath !== "")
                        m.push({ key: "custom", label: bidi.arg(qsTranslate("rexon", "Your file: %1"), section.bgCustomPath.split("/").pop()) });
                    for (const b of (section.backgrounds || []))
                        m.push({
                            key: b.id,
                            // An animated background gets the same badge as on its card in the
                            // background grid.
                            label: (b.mark ? b.mark + " " : "")
                                   + (b.equipped ? bidi.arg(qsTranslate("rexon", "%1 (on your profile)"), b.name) : b.name)
                        });
                    return m;
                }
                selectedValue: section.bgKey
                onActivated: section.chooseBackground(bgCombo.currentValue)
            }
            RexonButton {
                objectName: "showcasePickBackground"
                pal: section.pal
                text: qsTranslate("rexon", "Your Background…")
                onClicked: {
                    const p = actions.pickBackground();
                    if (p !== "") {
                        section.bgCustomPath = p;
                        section.chooseBackground("custom");
                    }
                }
            }
        }
        RexonText {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            visible: text !== ""
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: section.bgBusy ? qsTranslate("rexon", "Loading the background…") : section.bgError
        }

        // ── Picture position ──────────────────────────────────────────
        GridLayout {
            Layout.fillWidth: true
            columns: 3
            columnSpacing: 8
            rowSpacing: 2
            enabled: section.srcInfo !== null

            RexonText { Layout.preferredWidth: section.labelWidth; text: qsTranslate("rexon", "Zoom:"); color: section.pal.text_primary }
            RexonSlider {
                objectName: "showcaseZoom"
                Layout.fillWidth: true
                pal: section.pal
                from: 1; to: 4; stepSize: 0.05
                value: section.zoom
                onMoved: section.zoom = value
            }
            RexonText { Layout.preferredWidth: 60; text: "×" + section.zoom.toFixed(2); color: section.pal.text_secondary }

            RexonText { Layout.preferredWidth: section.labelWidth; text: qsTranslate("rexon", "Horizontal position:"); color: section.pal.text_primary }
            RexonSlider {
                objectName: "showcasePanX"
                Layout.fillWidth: true
                pal: section.pal
                from: 0; to: 1; stepSize: 0.01
                value: section.panX
                onMoved: section.panX = value
            }
            RexonText { Layout.preferredWidth: 60; text: Math.round(section.panX * 100) + " %"; color: section.pal.text_secondary }

            RexonText { Layout.preferredWidth: section.labelWidth; text: qsTranslate("rexon", "Vertical position:"); color: section.pal.text_primary }
            RexonSlider {
                objectName: "showcasePanY"
                Layout.fillWidth: true
                pal: section.pal
                from: 0; to: 1; stepSize: 0.01
                value: section.panY
                onMoved: section.panY = value
            }
            RexonText { Layout.preferredWidth: 60; text: Math.round(section.panY * 100) + " %"; color: section.pal.text_secondary }
        }
        RexonText {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            visible: section.srcInfo !== null && section.zoom <= 1.0
            text: qsTranslate("rexon", "The position sliders work when the picture is zoomed in.")
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            RexonText {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Showcase lower by:")
                color: section.pal.text_primary
            }
            RexonSlider {
                objectName: "showcaseShiftY"
                Layout.fillWidth: true
                pal: section.pal
                from: 0; to: 1200; stepSize: 2
                value: section.shiftY
                onMoved: section.shiftY = Math.round(value)
                ToolTip.visible: hovered
                ToolTip.text: qsTranslate("rexon", "Only for the preview: if other showcases stand above this one, it moves down the page.")
            }
            RexonText { Layout.preferredWidth: 60; text: section.shiftY + " px"; color: section.pal.text_secondary }
        }

        Repeater {
            model: section.layWarnings
            delegate: RexonText {
                required property var modelData
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: section.warnColor
                text: "⚠️ " + modelData
            }
        }

        // ── Profile preview ───────────────────────────────────────────
        RexonText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: qsTranslate("rexon", "Preview (approximate: the exact place of the showcase on the background is still to be checked against a live profile).")
        }
        Item {
            id: preview
            objectName: "showcasePreview"
            Layout.fillWidth: true
            Layout.preferredHeight: preview.viewHeight * preview.s
            clip: true

            // Physical zone: a snapshot of the Steam profile page. The showcase parts
            // are cut from one picture by its real coordinates and placed on the
            // profile background with manual `x` — they must not be mirrored,
            // otherwise the left part of the picture would move into the right cell.
            // For right-to-left languages the zone stays as it is.
            LayoutMirroring.enabled: false
            LayoutMirroring.childrenInherit: true

            readonly property var g: section.geo
            readonly property real s: width / (g.viewWidth || 1440)
            readonly property real vx: g.viewX || 240
            readonly property real showTop: (g.showcaseY || 256) + section.shiftY
            readonly property var lay: g.layout || ({ parts: [], frame_width: 615, frame_height: 400, crop: { x: 0, y: 0, w: 1, h: 1 } })
            readonly property real viewHeight: Math.max(1080, showTop + lay.frame_height + 60)
            // source px → preview px.
            readonly property real k: lay.frame_width * s / Math.max(lay.crop.w, 1)

            Rectangle {
                anchors.fill: parent
                color: section.look.page || "#1b2838"
            }
            Image {
                x: -preview.vx * preview.s
                y: 0
                width: (preview.g.profileWidth || 1920) * preview.s
                height: implicitWidth > 0 ? width * implicitHeight / implicitWidth : 0
                source: section.bgUrl
                fillMode: Image.Stretch
                asynchronous: true
                smooth: true
                visible: section.bgUrl !== ""
            }

            // Profile header — a placeholder.
            Rectangle {
                x: ((preview.g.columnX || 480) - preview.vx) * preview.s
                y: 24 * preview.s
                width: (preview.g.columnWidth || 960) * preview.s
                height: 176 * preview.s
                color: section.look.plate || "#66000000"
                Rectangle {
                    x: 16 * preview.s; y: 8 * preview.s
                    width: 160 * preview.s; height: 160 * preview.s
                    color: section.look.placeholder || "#3d4450"
                }
                Text {
                    x: 196 * preview.s; y: 20 * preview.s
                    text: qsTranslate("rexon", "Your profile")
                    color: section.look.text || "white"
                    font.pixelSize: Math.max(8, 24 * preview.s)
                }
                Rectangle {
                    x: (960 - 300) * preview.s; y: 24 * preview.s
                    width: 280 * preview.s; height: 40 * preview.s
                    color: "transparent"
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: bidi.arg(qsTranslate("rexon", "Level %1"), preview.g.minLevel || 10)
                        color: section.look.muted || "#acb2b8"
                        font.pixelSize: Math.max(8, 20 * preview.s)
                    }
                }
            }

            // Showcase backing with a title.
            Rectangle {
                x: ((preview.g.showcaseX || 495) - 11 - preview.vx) * preview.s
                y: (preview.showTop - 44) * preview.s
                width: (Math.max(preview.lay.frame_width, 630) + 20) * preview.s
                height: (preview.lay.frame_height + 54) * preview.s
                color: section.look.plate || "#66000000"
                Rectangle {
                    width: parent.width
                    height: 34 * preview.s
                    color: section.look.header || "#80000000"
                    Text {
                        x: 10 * preview.s
                        anchors.verticalCenter: parent.verticalCenter
                        text: ((section.kinds.find(k => k.key === section.kind) || {}).label || "").toUpperCase()
                        color: section.look.text || "white"
                        font.pixelSize: Math.max(7, 16 * preview.s)
                    }
                }
            }

            // Showcase parts: each one is a window into the shared picture. The two
            // GIF copies each run at their own pace — just as in Steam.
            Repeater {
                model: preview.lay.parts
                delegate: Item {
                    id: partBox
                    required property var modelData
                    x: ((preview.g.showcaseX || 495) + modelData.x - preview.vx) * preview.s
                    y: preview.showTop * preview.s
                    width: modelData.width * preview.s
                    height: modelData.height * preview.s
                    clip: true

                    Rectangle {
                        anchors.fill: parent
                        visible: section.srcUrl === ""
                        color: "transparent"
                        border.color: section.look.muted || "#acb2b8"
                        border.width: 1
                        Text {
                            anchors.centerIn: parent
                            width: parent.width - 4
                            horizontalAlignment: Text.AlignHCenter
                            wrapMode: Text.WordWrap
                            text: partBox.modelData.label
                            color: section.look.muted || "#acb2b8"
                            font.pixelSize: Math.max(7, 14 * preview.s)
                        }
                    }
                    AnimatedImage {
                        visible: section.srcUrl !== ""
                        source: section.srcUrl
                        playing: true
                        smooth: true
                        width: (section.srcInfo ? section.srcInfo.width : 0) * preview.k
                        height: (section.srcInfo ? section.srcInfo.height : 0) * preview.k
                        x: -preview.lay.crop.x * preview.k - partBox.modelData.x * preview.s
                        y: -preview.lay.crop.y * preview.k
                    }
                }
            }

            Text {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 6
                text: qsTranslate("rexon", "Preview")
                color: section.look.muted || "#acb2b8"
                font.pixelSize: 11
            }
        }

        // ── Save ──────────────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            RexonButton {
                objectName: "showcaseSave"
                pal: section.pal
                tone: section.sty.primaryButton
                enabled: section.srcInfo !== null && section.lay !== null && !section.sliceBusy
                text: qsTranslate("rexon", "Save Parts")
                onClicked: section.save()
            }
            RexonButton {
                objectName: "showcaseOpenFolder"
                pal: section.pal
                text: qsTranslate("rexon", "Open Folder")
                onClicked: actions.openFolder(section.result ? section.result.report.dir : actions.outputRoot())
            }
            RexonButton {
                visible: section.sliceBusy
                pal: section.pal
                text: qsTranslate("rexon", "Cancel")
                onClicked: actions.cancel()
            }
            RexonText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: section.sliceError !== "" ? (section.warnColor) : section.pal.text_secondary
                text: section.sliceBusy
                      ? bidi.arg(qsTranslate("rexon", "Saving… %1 %"), Math.round(section.sliceProgress * 100))
                      : section.sliceError !== "" ? section.sliceError
                      : section.result ? bidi.arg(qsTranslate("rexon", "Saved to %1"), section.result.report.dir)
                      : ""
            }
        }
        Repeater {
            model: section.result ? section.result.files : []
            delegate: RexonText {
                required property var modelData
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: section.pal.text_primary
                text: "✅ " + modelData.name + " — " + modelData.label + ", " + modelData.size + ", " + modelData.weight
                      + (modelData.frames > 1 ? ", " + bidi.arg(qsTranslate("rexon", "frames: %1"), modelData.frames) : "")
            }
        }
        Repeater {
            model: section.result ? section.result.report.warnings : []
            delegate: RexonText {
                required property var modelData
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: section.warnColor
                text: "⚠️ " + modelData
            }
        }
        // ── Return to a set started earlier ───────────────────────────
        Flow {
            Layout.fillWidth: true
            spacing: 8
            visible: section.wiz === null && section.recent.length > 0

            RexonText {
                height: 40
                verticalAlignment: Text.AlignVCenter
                color: section.pal.text_secondary
                text: qsTranslate("rexon", "Continue the upload of an earlier set:")
            }
            Repeater {
                model: section.recent
                delegate: RexonButton {
                    required property var modelData
                    required property int index
                    objectName: "showcaseRecent_" + index
                    pal: section.pal
                    text: modelData.name
                    onClicked: section.loadWizard(modelData.dir)
                }
            }
        }

        // ── Helper: upload and put into the showcase ──────────────────
        ColumnLayout {
            objectName: "showcaseWizard"
            Layout.fillWidth: true
            visible: section.wiz !== null
            spacing: section.sty.group.spacing

            RexonText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: section.pal.text_primary
                font.bold: true
                font.pointSize: 12
                text: section.wiz ? bidi.arg(qsTranslate("rexon", "Upload to Steam: %1 — %2"), section.wiz.kindLabel, section.wiz.name) : ""
            }

            Repeater {
                model: section.wiz ? section.wiz.steps : []

                delegate: Rectangle {
                    id: stepCard
                    required property var modelData
                    readonly property string key: stepCard.modelData.key

                    objectName: "showcaseStep_" + stepCard.key
                    Layout.fillWidth: true
                    implicitHeight: stepBody.implicitHeight + 2 * section.sty.card.margin
                    radius: section.sty.card.radius
                    color: section.sty.card.background
                    border.width: 1
                    // The step the user is on right now — with an accent frame.
                    border.color: stepCard.modelData.current ? section.pal.accent : section.sty.card.border

                    ColumnLayout {
                        id: stepBody
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: section.sty.card.margin
                        y: section.sty.card.margin
                        spacing: 6

                        RexonText {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            color: section.pal.text_primary
                            font.bold: true
                            text: (stepCard.modelData.done ? "✅ " : "") + stepCard.modelData.title
                        }
                        Repeater {
                            model: stepCard.modelData.lines
                            delegate: RexonText {
                                required property string modelData
                                required property int index
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                color: section.pal.text_secondary
                                text: (index + 1) + ". " + modelData
                            }
                        }

                        // ── Step 1: upload the parts ──────────────────
                        RexonButton {
                            objectName: "showcaseOpenUpload"
                            visible: stepCard.key === "upload"
                            pal: section.pal
                            tone: section.sty.primaryButton
                            text: qsTranslate("rexon", "Open the upload page")
                            ToolTip.visible: hovered
                            ToolTip.text: qsTranslate("rexon", "Opens Steam's artwork upload page and the folder with the parts")
                            onClicked: section.say(actions.openUploadPage(section.wiz.dir))
                        }
                        Repeater {
                            model: stepCard.key === "upload" && section.wiz ? section.wiz.parts : []
                            delegate: RowLayout {
                                id: partRow
                                required property var modelData
                                required property int index
                                Layout.fillWidth: true
                                spacing: 10

                                Rectangle {
                                    implicitWidth: 56
                                    implicitHeight: 56
                                    radius: section.sty.thumb.radius
                                    color: section.sty.thumb.bgBackground
                                    clip: true
                                    Image {
                                        anchors.fill: parent
                                        source: partRow.modelData.url
                                        sourceSize.width: 112
                                        fillMode: Image.PreserveAspectCrop
                                        verticalAlignment: Image.AlignTop
                                        asynchronous: true
                                    }
                                    RexonZoomArea {
                                        anchors.fill: parent
                                        pal: section.pal
                                        radius: section.sty.thumb.radius
                                        onActivated: section.zoomRequested(section.partViewerItems(false), partRow.index)
                                    }
                                }
                                RexonText {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    color: section.pal.text_primary
                                    text: partRow.modelData.file + " — " + partRow.modelData.label + ", " + partRow.modelData.size
                                }
                                RexonButton {
                                    objectName: "showcaseCopyTitle_" + partRow.index
                                    pal: section.pal
                                    text: qsTranslate("rexon", "Copy the title")
                                    ToolTip.visible: hovered
                                    ToolTip.text: bidi.arg(qsTranslate("rexon", "Copies \"%1\" — paste it into the Title field on the upload page"), partRow.modelData.title)
                                    onClicked: section.copy(partRow.modelData.title,
                                                            bidi.arg(qsTranslate("rexon", "Copied: %1"), partRow.modelData.title))
                                }
                                RexonCheckBox {
                                    objectName: "showcaseUploaded_" + partRow.index
                                    pal: section.pal
                                    text: qsTranslate("rexon", "Uploaded")
                                    checked: partRow.modelData.uploaded
                                    onToggled: section.setWizard(JSON.parse(actions.wizardSetUploaded(JSON.stringify({
                                        dir: section.wiz.dir, file: partRow.modelData.file, on: checked }))))
                                }
                            }
                        }
                        RowLayout {
                            visible: stepCard.key === "upload"
                            Layout.fillWidth: true
                            spacing: 10
                            RexonButton {
                                objectName: "showcaseCopyBlank"
                                pal: section.pal
                                text: qsTranslate("rexon", "Copy an empty title")
                                onClicked: section.copy(section.wiz.blankTitle,
                                                        qsTranslate("rexon", "An invisible character is copied — paste it into the Title field."))
                            }
                            RexonText {
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                color: section.pal.text_secondary
                                text: qsTranslate("rexon", "Optional: with an invisible character as the title there is no caption line under the picture in the showcase. The parts are then harder to tell apart in Steam's picker — the newest upload comes first there.")
                            }
                        }

                        // ── Step 2: put them into the showcase ────────
                        RowLayout {
                            visible: stepCard.key === "place"
                            Layout.fillWidth: true
                            spacing: 10
                            RexonButton {
                                objectName: "showcaseOpenEditor"
                                pal: section.pal
                                tone: section.sty.primaryButton
                                text: qsTranslate("rexon", "Open the profile editor")
                                onClicked: section.say(actions.openProfileEditor())
                            }
                            RexonCheckBox {
                                objectName: "showcasePlaced"
                                pal: section.pal
                                enabled: section.wiz !== null && section.wiz.step !== "upload"
                                text: qsTranslate("rexon", "Done — the parts are in the showcase")
                                checked: section.wiz !== null && section.wiz.placed
                                onToggled: section.setWizard(JSON.parse(actions.wizardSetPlaced(JSON.stringify({
                                    dir: section.wiz.dir, on: checked }))))
                            }
                            Item { Layout.fillWidth: true }
                        }

                        // ── Step 3: check ─────────────────────────────
                        RowLayout {
                            visible: stepCard.key === "check"
                            Layout.fillWidth: true
                            spacing: 10
                            RexonButton {
                                objectName: "showcaseCheck"
                                pal: section.pal
                                tone: section.sty.primaryButton
                                enabled: !section.checkBusy
                                text: section.checkBusy ? qsTranslate("rexon", "Checking…")
                                                        : qsTranslate("rexon", "Check my profile")
                                onClicked: section.startCheck()
                            }
                            RexonText {
                                objectName: "showcaseCheckText"
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                color: section.checkError !== "" || (section.checkReport && !section.checkReport.ok)
                                       ? section.warnColor : section.pal.text_primary
                                text: section.checkError !== "" ? section.checkError
                                      : section.checkReport ? section.checkReport.headline : ""
                            }
                        }
                        Repeater {
                            model: stepCard.key === "check" && section.checkReport ? section.checkReport.lines : []
                            delegate: RexonText {
                                required property string modelData
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                color: section.pal.text_secondary
                                text: "• " + modelData
                            }
                        }
                        // "As in your profile" next to the computed one: the same parts at the
                        // same scale, with the same gap.
                        RowLayout {
                            id: compareRow
                            objectName: "showcaseCompare"
                            visible: stepCard.key === "check" && section.checkReport !== null
                                     && section.checkReport.parts.length > 0
                                     && section.checkReport.status !== "private"
                            Layout.fillWidth: true
                            spacing: 24

                            readonly property real k: Math.min(0.5, (stepBody.width - 24) / 2 / 640)

                            Repeater {
                                model: [true, false]
                                delegate: ColumnLayout {
                                    id: side
                                    required property bool modelData
                                    required property int index
                                    Layout.alignment: Qt.AlignTop
                                    spacing: 4

                                    RexonText {
                                        color: section.pal.text_secondary
                                        text: side.modelData ? qsTranslate("rexon", "On your profile")
                                                             : qsTranslate("rexon", "Calculated")
                                    }
                                    // Physical zone: the showcase parts are pieces of one picture, placed
                                    // side by side as in the Steam profile (the large one on the left, the
                                    // side one on the right). Do not swap them for right to left, otherwise
                                    // the picture falls apart; the labels above the row are mirrored.
                                    Row {
                                        id: cmpParts
                                        LayoutMirroring.enabled: false
                                        LayoutMirroring.childrenInherit: true
                                        spacing: 9 * compareRow.k
                                        Repeater {
                                            model: section.checkReport ? section.checkReport.parts : []
                                            delegate: Rectangle {
                                                id: cmpPart
                                                required property var modelData
                                                required property int index
                                                readonly property bool fromProfile: side.modelData
                                                readonly property real w: cmpPart.fromProfile ? cmpPart.modelData.shown_width
                                                                                               : cmpPart.modelData.expected_width
                                                readonly property real h: cmpPart.fromProfile ? cmpPart.modelData.shown_height
                                                                                               : cmpPart.modelData.expected_height
                                                visible: !cmpPart.fromProfile || cmpPart.modelData.found
                                                width: Math.max(1, cmpPart.w * compareRow.k)
                                                height: Math.max(1, cmpPart.h * compareRow.k)
                                                color: section.sty.thumb.bgBackground
                                                border.width: cmpPart.modelData.ok ? 0 : 1
                                                border.color: section.warnColor
                                                Image {
                                                    anchors.fill: parent
                                                    asynchronous: true
                                                    fillMode: Image.Stretch
                                                    source: cmpPart.fromProfile ? cmpPart.modelData.image_url
                                                            : (section.wiz && section.wiz.parts[cmpPart.index]
                                                               ? section.wiz.parts[cmpPart.index].url : "")
                                                }
                                                RexonZoomArea {
                                                    anchors.fill: parent
                                                    pal: section.pal
                                                    onActivated: {
                                                        // Only the found parts go into the "In the profile" view: the place in
                                                        // the list is looked up by key, not by number.
                                                        const items = section.partViewerItems(cmpPart.fromProfile);
                                                        const key = "profile-" + cmpPart.modelData.role;
                                                        const at = cmpPart.fromProfile
                                                            ? Math.max(0, items.findIndex(it => it.key === key))
                                                            : cmpPart.index;
                                                        section.zoomRequested(items, at);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Item { Layout.fillWidth: true }
                        }
                    }
                }
            }
        }
    }
}
