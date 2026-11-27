// Pop-up status banner — one for all tabs of the product.
//
// Why a separate type instead of a banner inside every tab (rule #12).
// The same defect — "the message never goes away" — was introduced THREE
// times: in the widget "Settings" (28.06), in the widget "Steam" (08.07)
// and a third time in the QML "Steam" tab, where the live check on 18.09
// caught the banner hanging for 29 minutes straight. There is no point
// fixing it a fourth time in a fourth file: while the timer, dismissal and
// colours live HERE, there is nowhere for a new copy of the defect to
// appear, and a new tab gets a correct banner with one line of markup.
//
// The banner has NO numbers or colours of its own at all:
//   * colours and paddings arrive as the `styles.toast` map — from the core
//     (`ui_base::toast_colors` and the `TOAST_*` constants next to it), the
//     same ones that paint the widget banner;
//   * the width is computed by the core (`ui_base::toast_width`) through a
//     method of the tab model — the banner lies ON TOP of the content and
//     must fit into the window at any size;
//   * the lifetime is `ui_base::TOAST_TIMEOUT_MS`.
//
// Placement: bottom LEFT with the core's margin — the same place as the
// widget banner.

pragma ComponentBehavior: Bound

import QtQuick

Rectangle {
    id: plate

    // ── Contract with the tab ─────────────────────────────────────────────

    /// Colours and metrics from the core: the `styles.toast` map of the tab model.
    required property var skin

    /// After how many ms the banner hides itself (`ui_base::TOAST_TIMEOUT_MS`).
    /// Zero or less means never hide; that way the banner can be left hanging
    /// on purpose rather than because of a forgotten timer.
    required property int timeoutMs

    /// Tab model — the banner width is requested from it through the method
    /// `toastWidth(pageWidth, textWidth)`. The core does the computing anyway.
    required property var metrics

    /// The width the banner must fit into. Defaults to the parent's width,
    /// that is, the page itself.
    property int availableWidth: plate.parent ? plate.parent.width : 0

    /// How to parse the text. Markup by default — that is how the "Settings"
    /// banner is shown with `<b>` and `<br>` from the core. A tab whose messages
    /// carry file names and paths is better off with `Text.PlainText`.
    property int textFormat: Text.RichText

    /// Text on the banner. Exposed so that checks read it from the banner
    /// itself rather than guessing from a page property.
    property string message: ""

    /// Whether the banner is visible to the user right now. Separate from
    /// `visible`: in the middle of fading out `visible` is still true, while
    /// the message is already leaving.
    readonly property bool shown: plate.opacity > 0

    // ── Show and hide ─────────────────────────────────────────────────────

    /// Show a message and restart the hide timer. An empty text does not mean
    /// "show emptiness" but "nothing to say": the banner goes away.
    function show(text) {
        if (!text) {
            plate.dismiss();
            return;
        }
        plate.message = text;
        plate.opacity = 1;
        hideTimer.stop();
        if (plate.timeoutMs > 0)
            hideTimer.restart();
    }

    /// Hide immediately.
    // The name is not `hide`: Item already has `visible`, and a twin verb in
    // markup would read as its toggle.
    function dismiss() {
        hideTimer.stop();
        plate.opacity = 0;
    }

    // ── Look ──────────────────────────────────────────────────────────────

    /// Padding from the banner edge to the text: inner padding plus border —
    /// exactly what the widget label gets from its style sheet.
    readonly property int inset: plate.skin.paddingH + plate.skin.borderWidth

    anchors.left: parent ? parent.left : undefined
    anchors.bottom: parent ? parent.bottom : undefined
    anchors.leftMargin: plate.skin.margin
    anchors.bottomMargin: plate.skin.margin

    width: plate.metrics.toastWidth(plate.availableWidth, label.implicitWidth)
    height: label.implicitHeight + 2 * (plate.skin.paddingV + plate.skin.borderWidth)

    radius: plate.skin.radius
    color: plate.skin.background
    border.color: plate.skin.border
    border.width: plate.skin.borderWidth

    opacity: 0
    // A hidden banner must intercept neither clicks nor view checks.
    visible: opacity > 0

    Behavior on opacity {
        NumberAnimation { duration: 150 }
    }

    RexonText {
        id: label
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: plate.inset
        anchors.rightMargin: plate.inset
        text: plate.message
        textFormat: plate.textFormat
        wrapMode: Text.WordWrap
        color: plate.skin.textColor
    }

    Timer {
        id: hideTimer
        interval: plate.timeoutMs
        onTriggered: plate.opacity = 0
    }
}
