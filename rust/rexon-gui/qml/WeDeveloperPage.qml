// The hidden "Developer" page of the "Wallpaper Engine" tab — MARKUP.
//
// A port of `DeveloperPageWidget` from `rexon/qt_wallpaper_engine/window.py`.
// Data, decisions and ALL labels come from the Rust object
// `WeDeveloperModel` (rust/rexon-gui/src/we_dev_page.rs) on top of
// `rexon_core::we_window`. There is not a single human-readable string and
// not a single layout number here: labels arrive as the `texts` map,
// numbers as the `metrics` map.
//
// Warning: the page is OURS, not vendored: it lies in the static resource
// of the `org.rexon.gui` module next to the tab shell, not in the
// waywallen directory. The "Dev" rail item is marked so in the core
// (`RailItem.vendor == false`), otherwise the shell would look for its
// file among the waywallen pages.
//
// Warning: in the original the section panels have NO background or
// border. Their `QFrame` carries `objectName="wePane"`, but there is not a
// single style sheet with such a selector in the product (checked with
// `grep` over the tree on 06.09) — so on screen these are just margins. We
// define no background of our own here: that would be an invention, and
// the page's yardstick is parity with the widget version.
//
// Warning: the page does three things NOT by itself:
//   * the version of the live daemon is named by the bus singleton
//     `DaemonDBusClient` — the Rust model does not reach into Qt and
//     cannot ask it;
//   * installing a plugin through the daemon is driven by
//     `PluginInstallQuery` — a shared request of the tab; there is no
//     reason to create a second way of talking to the daemon;
//   * the daemon answer guard is an ordinary `Timer`, and it carries the
//     attempt number with it: an old guard must not abort a NEW
//     installation (fix 2 in `rexon_core::we_window`).

// Repeater delegates refer to ids of outer elements.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── To the outside ────────────────────────────────────────────────────

    /// Parity divergence: the tab shell uses it to put a mark on the "Dev"
    /// button in the rail — as the `parity_drift_changed` signal of the widget
    /// page did.
    signal parityDrift(bool drift)

    // ── Page core ─────────────────────────────────────────────────────────

    WeDeveloperModel {
        id: page

        // The name is needed by checks: the object is fetched with
        // `root.findChild(QObject, "weDeveloperModel")` and queried without
        // touching the markup.
        objectName: "weDeveloperModel"

        // The plugin archive is verified and waits to be installed THROUGH the
        // daemon.
        onOweIpcRequested: (zipPath, attempt) => {
            oweIpc.zipPath = zipPath;
            oweIpc.reload();
            oweIpcGuard.attempt = attempt;
            oweIpcGuard.restart();
        }
    }

    readonly property var txt: page.texts
    readonly property var m: page.metrics

    // ── Palette of the active skin ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colText: root.pal.text_primary
    readonly property color colBorder: root.pal.border
    // Muted text: the same text role with a share of opacity from the core —
    // that is how the widget version paints it (`muted_qss` over the `Text`
    // role).
    readonly property color colTextMuted: Qt.rgba(root.colText.r, root.colText.g,
                                                  root.colText.b, page.variantAlpha)

    /// Status line colour: an empty colour from the core means "ordinary text".
    function statusColor(color) {
        return color === "" ? root.colText : color;
    }

    // ── Version of the live daemon ────────────────────────────────────────
    //
    // Only the Qt layer (D-Bus) knows it, so we ask the tab singleton. The
    // original called `self._daemon.probe()` on every parity reconciliation;
    // here the query runs once when the page opens, and after that the
    // singleton itself drives the property — it is subscribed to bus events.
    readonly property string daemonVersion: DaemonDBusClient.daemonVersion

    // ── UI parity reconciliation ──────────────────────────────────────────

    WeParityQuery {
        id: parity

        objectName: "weParityQuery"
        daemonVersion: root.daemonVersion
        onChecked: (drift) => root.parityDrift(drift)
    }

    // ── Installing the OWE plugin through the daemon (hot-install) ────────

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

    // Daemon answer guard. It remembers ITS OWN attempt: the answer to the
    // first installation arrives quickly, while the guard lives for up to a
    // minute — and the next installation started in this window would get
    // someone else's alarm.
    Timer {
        id: oweIpcGuard

        property int attempt: 0

        interval: root.m.oweIpcTimeoutMs
        repeat: false
        onTriggered: page.oweIpcTimeout(oweIpcGuard.attempt)
    }

    // ── Background task event queue ───────────────────────────────────────
    //
    // Tasks (network, unpacking, subprocesses) live in their own runtime and
    // put their outcomes into a queue; the page drains it from its own
    // thread. The same approach and the same period as in the "Steam" tab.
    Timer {
        interval: 120
        repeat: true
        running: true
        onTriggered: page.pump()
    }

    // Delayed start: the version label and, if the box is checked, the
    // automatic update check and automatic parity reconciliation. The delay
    // is named by the core — the same one the tab itself starts daemon
    // detection with.
    Timer {
        interval: root.m.startupDelayMs
        repeat: false
        running: true
        onTriggered: {
            // One-off bus query: without it the singleton returns an empty version
            // until something happens on the bus.
            DaemonDBusClient.refreshWsPort();
            page.refreshVersion(root.daemonVersion);
            if (page.autoUpstream)
                page.startCheck(root.daemonVersion);
            if (page.autoParity)
                parity.reloadIfConnected();
        }
    }

    Component.onDestruction: page.shutdown()

    // ── Markup ────────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    ScrollView {
        anchors.fill: parent
        anchors.margins: root.m.pageMargin
        contentWidth: availableWidth
        // The original has no horizontal bar at all (`ScrollBarAlwaysOff`).
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        ColumnLayout {
            width: parent.width
            spacing: root.m.columnSpacing

            // ── Page title ────────────────────────────────────────────────
            RexonText {
                Layout.fillWidth: true
                text: root.txt.title
                color: root.colText
                font.pixelSize: root.m.titleFontSize
                font.weight: root.m.titleFontWeight
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt.subtitle
                color: root.colTextMuted
                font.pixelSize: root.m.mutedFontSize
                wrapMode: Text.WordWrap
            }

            // ══ "Upstream updates" section ════════════════════════════════
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: root.m.panePadding
                Layout.rightMargin: root.m.panePadding
                Layout.topMargin: root.m.panePadding
                Layout.bottomMargin: root.m.panePadding
                spacing: root.m.paneSpacing

                RexonText {
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

                // The engine version label — one for the whole life of the field (core
                // fix 3: previously the field went by two different names).
                RexonText {
                    Layout.fillWidth: true
                    objectName: "devVersionLabel"
                    text: page.versionLabel
                    color: root.colText
                    font.pixelSize: root.m.statusFontSize
                    wrapMode: Text.WordWrap
                }

                // A status line per target: waywallen (we install it) and
                // open-wallpaper-engine (we report on it). The set of targets is named by
                // the core; the markup only draws them.
                Repeater {
                    model: page.targets

                    delegate: TextEdit {
                        required property var modelData

                        Layout.fillWidth: true
                        // The text is selectable with the mouse — as in the original
                        // (`TextSelectableByMouse`): the status line is copied into reports.
                        readOnly: true
                        selectByMouse: true
                        text: modelData.text
                        color: root.statusColor(modelData.color)
                        font.pixelSize: root.m.statusFontSize
                        font.bold: modelData.bold
                        wrapMode: Text.WordWrap
                        // Start of the line: without explicit alignment the field takes it from
                        // the text itself, and mirroring flips an explicit `AlignLeft`.
                        horizontalAlignment: TextEdit.AlignLeft
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

                    // OWE plugin: hot-install through the daemon (waywallen ≥ 0.2.3) or
                    // unpacking into the plugin directory if the daemon is not running. The
                    // core decides which way.
                    RexonButton {
                        objectName: "devOweButton"
                        pal: root.pal
                        text: root.txt.btnOwe
                        enabled: page.oweEnabled
                        onClicked: root.ask(root.txt.confirmOweTitle,
                                            page.confirmOweText(),
                                            () => page.startOweInstall(root.daemonVersion))
                    }

                    // "Reviewed" is for the case "I read the changelog and will not install":
                    // it fixes the base version of the next report.
                    RexonButton {
                        objectName: "devSeenButton"
                        pal: root.pal
                        text: root.txt.btnSeen
                        enabled: page.seenEnabled
                        onClicked: page.markOweSeen()
                    }

                    Item { Layout.fillWidth: true }
                }

                RexonText {
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

                    RexonText {
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

                // Changelog panel for the range "our version → the latest one".
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
                        // The change list — engine commit titles — reads left to right in any
                        // interface language, so the field is physically on the left: mirroring
                        // would flip an explicit `AlignLeft` to the right, while `AlignRight`
                        // under mirroring gives the left edge (like the PKGBUILD window in
                        // "Appearance").
                        horizontalAlignment: LayoutMirroring.enabled ? TextEdit.AlignRight
                                                                     : TextEdit.AlignLeft

                        // Warning: the panel HAS a backing, and it is darker than the page. In the
                        // widget version it is drawn by the `QPlainTextEdit` itself with the
                        // `Base` role, while a transparent QML panel showed the page background:
                        // test bench measurement 06.09 — a solid area of 232 910 pixels, (22,32,45)
                        // versus (27,40,56). The colour is taken from the skin (`bg_secondary`);
                        // there is none of our own here.
                        background: Rectangle {
                            color: root.pal.bg_secondary
                            border.color: root.colBorder
                            border.width: 1
                        }
                    }
                }
            }

            // ══ "UI parity report" section ════════════════════════════════
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: root.m.panePadding
                Layout.rightMargin: root.m.panePadding
                Layout.topMargin: root.m.panePadding
                Layout.bottomMargin: root.m.panePadding
                spacing: root.m.paneSpacing

                RexonText {
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
                        // While the engine is answering, the button is locked — as in the widget
                        // version for the duration of the request.
                        enabled: !parity.querying
                        onClicked: parity.reload()
                    }

                    RexonText {
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
                    // Start of the line: without explicit alignment the field takes it from
                    // the text itself, and mirroring flips an explicit `AlignLeft`.
                    horizontalAlignment: TextEdit.AlignLeft
                }
            }

            Item { Layout.fillHeight: true }
        }
    }

    // ── Question before installing ────────────────────────────────────────
    //
    // In the widget version this is `QMessageBox.question`; in Big Picture
    // multiple system windows are awkward (canon, item 16), so the question is
    // asked with the page's own modal window — as already done in "Settings".

    /// Ask and, if agreed, execute. An empty text means there is nothing to ask
    /// about (nothing to install), and then nothing is opened.
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

        // Dimming under a modal window is a product-wide decision
        // (`ui_base::modal_dim_argb_hex`, key `modal_dim`).
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            RexonText {
                Layout.fillWidth: true
                text: confirmDialog.heading
                color: root.colText
                font.bold: true
                font.pixelSize: root.m.sectionFontSize
                wrapMode: Text.WordWrap
            }

            RexonText {
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

        // Focus on opening is on the safe answer: an accidental Enter must not
        // start a ~200 MiB installation (foolproofing).
        onOpened: confirmNo.forceActiveFocus()
    }
}
