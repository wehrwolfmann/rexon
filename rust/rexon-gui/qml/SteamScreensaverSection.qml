// The "Screensaver" sub-tab of the "Steam" tab: a Rexon screensaver for
// the Steam Big Picture idle screen (Steam Deck, Steam Machine, a TV, an
// ordinary computer). Art of recent games and screenshots as a slide show,
// or a bouncing logo.
//
// Markup only. Options, limits, all status phrases and the installation
// itself — the core `rexon_core::steam_screensaver`; the executor is
// `SteamScreensaverActions` (rust/rexon-gui/src/steam_screensaver_actions.rs).
//
// Never: Rexon does not touch the screensaver choice in Steam: the user
// picks it themselves in the Big Picture settings, and the hint at the
// bottom says where. There is no Steam logo among the options — it is a
// Valve trademark.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section
    RexonBidi { id: bidi }

    property var pal: ({})
    /// Tab styling (`SteamModel.styles`): group, inset, buttons.
    property var sty: ({})
    /// Error colour — ready from the tab core (`SteamModel.goldColor`).
    property color warnColor: section.pal.text_primary

    signal message(string text)

    // ── State ──────────────────────────────────────────────────────────

    /// Answer of `actions.choices()`: options, limits, hints.
    property var ch: ({ modes: [], logos: [], intervalMin: 4, intervalMax: 60, howTo: "", restartHint: "" })
    /// Answer of `actions.status()`.
    property var st: ({ steamFound: false, installed: false, foreign: false, text: "", options: {}, updated: 0, roots: [] })

    property string mode: "slideshow"
    property string logo: "rexon"
    property bool gameArt: true
    property bool screenshots: true
    property bool clock: true
    property int interval: 9

    property bool busy: false
    property string jobError: ""
    property bool scanBusy: false
    property string foundText: ""
    /// The first press of "Delete" only arms it: a second press within a few
    /// seconds deletes. There is no question window — it is awkward in Big
    /// Picture.
    property bool confirmRemove: false

    function options() {
        return { mode: section.mode, logo: section.logo, gameArt: section.gameArt,
                 screenshots: section.screenshots, clock: section.clock, interval: section.interval };
    }

    function takeOptions(o) {
        if (!o)
            return;
        section.mode = o.mode || "slideshow";
        section.logo = o.logo || "rexon";
        section.gameArt = o.gameArt !== false;
        section.screenshots = o.screenshots !== false;
        section.clock = o.clock !== false;
        section.interval = o.interval || 9;
    }

    /// The settings differ from the installed screensaver — "Update" is needed.
    readonly property bool dirty: section.st.installed
                                  && JSON.stringify(section.options()) !== JSON.stringify(section.st.options)

    function rescan() {
        actions.startScan(JSON.stringify(section.options()));
        section.scanBusy = true;
        poll.start();
    }

    function install() {
        const err = actions.startInstall(JSON.stringify(section.options()));
        if (err !== "") {
            section.jobError = err;
            return;
        }
        section.jobError = "";
        section.busy = true;
        poll.start();
    }

    function remove() {
        if (!section.confirmRemove) {
            section.confirmRemove = true;
            disarm.restart();
            return;
        }
        section.confirmRemove = false;
        const err = actions.startRemove();
        if (err !== "") {
            section.jobError = err;
            return;
        }
        section.jobError = "";
        section.busy = true;
        poll.start();
    }

    // What goes into the slide show depends on two check boxes — recompute.
    onGameArtChanged: rescanSoon.restart()
    onScreenshotsChanged: rescanSoon.restart()

    SteamScreensaverActions {
        id: actions
        objectName: "steamScreensaverActions"
    }

    Timer {
        id: rescanSoon
        interval: 300
        onTriggered: section.rescan()
    }

    Timer {
        id: disarm
        interval: 4000
        onTriggered: section.confirmRemove = false
    }

    Timer {
        id: poll
        interval: 150
        repeat: true
        onTriggered: {
            const p = JSON.parse(actions.poll());
            if (section.scanBusy && p.scan.done) {
                section.scanBusy = false;
                section.foundText = p.scan.text;
            }
            if (section.busy && p.job.done) {
                section.busy = false;
                section.st = p.job.status;
                if (p.job.ok)
                    section.message(p.job.message);
                else
                    section.jobError = p.job.message;
            }
            if (!section.scanBusy && !section.busy)
                poll.stop();
        }
    }

    Component.onCompleted: {
        section.ch = JSON.parse(actions.choices());
        section.st = JSON.parse(actions.status());
        section.takeOptions(section.st.options);
        section.rescan();
    }

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    readonly property real labelWidth: 220
    readonly property var modeInfo: section.ch.modes.find(m => m.key === section.mode) || {}
    readonly property var logoInfo: section.ch.logos.find(m => m.key === section.logo) || {}

    ColumnLayout {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "🌙 Screensaver")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
        }
        RexonText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: qsTranslate("rexon", "Rexon makes a screensaver for Steam Big Picture: Steam shows it when you leave your Steam Deck, Steam Machine, TV or computer idle for a while. It shows the artwork of your recently played games and your screenshots, or a bouncing logo.")
        }

        // ── State ─────────────────────────────────────────────────────
        RexonText {
            objectName: "screensaverStatus"
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_primary
            font.bold: true
            text: section.st.text
        }
        RexonText {
            Layout.fillWidth: true
            visible: section.st.installed && section.st.updated > 0
            color: section.pal.text_secondary
            text: bidi.arg(qsTranslate("rexon", "Updated: %1"), new Date(section.st.updated * 1000).toLocaleDateString(Qt.locale(), Locale.ShortFormat))
        }

        // ── What to show ──────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            enabled: section.st.steamFound && !section.busy
            RexonText {
                Layout.preferredWidth: section.labelWidth
                text: qsTranslate("rexon", "Show:")
                color: section.pal.text_primary
            }
            RexonComboBox {
                id: modeCombo
                objectName: "screensaverMode"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "key"
                model: section.ch.modes
                selectedValue: section.mode
                onActivated: section.mode = modeCombo.currentValue
            }
        }
        RexonText {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: section.modeInfo.hint || ""
        }

        // Slide show: what to build it from and how often to change the frame.
        ColumnLayout {
            Layout.fillWidth: true
            visible: section.mode === "slideshow"
            enabled: section.st.steamFound && !section.busy
            spacing: section.sty.group.spacing

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: section.labelWidth + 8
                spacing: 16
                RexonCheckBox {
                    objectName: "screensaverGameArt"
                    pal: section.pal
                    text: qsTranslate("rexon", "Game artwork")
                    checked: section.gameArt
                    onToggled: section.gameArt = checked
                }
                RexonCheckBox {
                    objectName: "screensaverShots"
                    pal: section.pal
                    text: qsTranslate("rexon", "My screenshots")
                    checked: section.screenshots
                    onToggled: section.screenshots = checked
                }
                Item { Layout.fillWidth: true }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                RexonText {
                    Layout.preferredWidth: section.labelWidth
                    text: qsTranslate("rexon", "Next picture every:")
                    color: section.pal.text_primary
                }
                RexonSlider {
                    objectName: "screensaverInterval"
                    Layout.fillWidth: true
                    pal: section.pal
                    from: section.ch.intervalMin
                    to: section.ch.intervalMax
                    stepSize: 1
                    value: section.interval
                    onMoved: section.interval = Math.round(value)
                }
                RexonText {
                    Layout.preferredWidth: 60
                    text: bidi.arg(qsTranslate("rexon", "%1 s"), section.interval)
                    color: section.pal.text_secondary
                }
            }
            RexonText {
                Layout.fillWidth: true
                Layout.leftMargin: section.labelWidth + 8
                visible: text !== ""
                wrapMode: Text.WordWrap
                color: section.pal.text_secondary
                text: section.scanBusy ? qsTranslate("rexon", "Looking for pictures in Steam…") : section.foundText
            }
        }

        // Bouncing logo: whose.
        RowLayout {
            Layout.fillWidth: true
            visible: section.mode === "bounce"
            enabled: section.st.steamFound && !section.busy
            spacing: 8
            RexonText {
                Layout.preferredWidth: section.labelWidth
                text: qsTranslate("rexon", "Logo:")
                color: section.pal.text_primary
            }
            RexonComboBox {
                id: logoCombo
                objectName: "screensaverLogo"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "key"
                model: section.ch.logos
                selectedValue: section.logo
                onActivated: section.logo = logoCombo.currentValue
            }
        }
        RexonText {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            visible: section.mode === "bounce"
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: section.logoInfo.hint || ""
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            enabled: section.st.steamFound && !section.busy
            RexonCheckBox {
                objectName: "screensaverClock"
                pal: section.pal
                text: qsTranslate("rexon", "Show the clock")
                checked: section.clock
                onToggled: section.clock = checked
            }
            Item { Layout.fillWidth: true }
        }

        // ── Actions ───────────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            RexonButton {
                objectName: "screensaverInstall"
                pal: section.pal
                tone: section.sty.primaryButton
                enabled: section.st.steamFound && !section.busy
                text: section.st.installed ? qsTranslate("rexon", "Update Screensaver")
                                           : qsTranslate("rexon", "Install Screensaver")
                ToolTip.visible: hovered && section.st.installed
                ToolTip.text: qsTranslate("rexon", "Collects fresh artwork and screenshots and applies the settings above.")
                onClicked: section.install()
            }
            RexonButton {
                objectName: "screensaverPreview"
                visible: section.st.installed
                enabled: !section.busy
                pal: section.pal
                text: qsTranslate("rexon", "👁️ Preview")
                ToolTip.visible: hovered
                ToolTip.text: qsTranslate("rexon", "Opens the screensaver in your web browser.")
                onClicked: {
                    const err = actions.preview();
                    if (err !== "")
                        section.message(err);
                }
            }
            RexonDangerButton {
                objectName: "screensaverRemove"
                visible: section.st.installed
                enabled: !section.busy
                pal: section.pal
                text: section.confirmRemove ? qsTranslate("rexon", "Click again to remove")
                                            : qsTranslate("rexon", "Remove")
                onClicked: section.remove()
            }
            RexonText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                visible: text !== ""
                color: section.jobError !== "" ? section.warnColor : section.pal.text_secondary
                text: section.busy ? qsTranslate("rexon", "Preparing the pictures…")
                      : section.jobError !== "" ? section.jobError
                      : section.dirty ? qsTranslate("rexon", "Press «Update Screensaver» to apply the changes.")
                      : ""
            }
        }

        // ── How to enable it in Steam ─────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            Layout.topMargin: 8
            implicitHeight: howCol.implicitHeight + section.sty.pitch.marginV * 2
            color: section.sty.pitch.background
            radius: section.sty.pitch.radius
            Rectangle {
                width: section.sty.pitch.barWidth
                height: parent.height
                color: section.sty.pitch.bar
            }
            ColumnLayout {
                id: howCol
                anchors.fill: parent
                anchors.margins: section.sty.pitch.marginV
                anchors.leftMargin: section.sty.pitch.marginH
                anchors.rightMargin: section.sty.pitch.marginH
                spacing: 4
                RexonText {
                    Layout.fillWidth: true
                    text: qsTranslate("rexon", "How to turn it on in Steam")
                    color: section.pal.text_primary
                    font.bold: true
                }
                RexonText {
                    objectName: "screensaverHowTo"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: section.pal.text_primary
                    text: section.ch.howTo
                }
                RexonText {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: section.pal.text_secondary
                    text: section.ch.restartHint
                }
            }
        }
    }
}
