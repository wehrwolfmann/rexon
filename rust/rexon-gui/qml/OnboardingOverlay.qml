// First-run tutorial — a "spotlight" overlay over the Rust window.
//
// A port of the widget overlay `rexon/onboarding.py`: dim the window, cut
// out a hole around the place of interest, show a card with "Next" and
// "Skip". The scenario, order, texts, the decision "what to show", saving
// progress, the cut-out margin, the card position and the skin colours
// belong to the core (`rexon_core::onboarding` through `OnboardingModel`).
// Here we only draw and find the target in the window (the `locate`
// function from the host).
//
// The first step is the question "Show a short tutorial?", as in the
// Python window (`Tour._ask`), only not as a separate window but as the
// same card: in Big Picture a chain of system windows is awkward
// (architecture rule #16).
//
// Control without a mouse: focus goes straight to the main button,
// Enter/Return presses the focused button, Esc declines ("No thanks" /
// "Skip"; on the last step — "Done"), the left/right arrows and Tab move
// between buttons. Steam Input maps the D-pad to arrows, A to Enter, B to
// Esc. Buttons are no smaller than 40×40 (rule #16).
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes

Popup {
    id: tour

    /// The Rust tutorial object.
    required property var core

    /// Find the target in window coordinates: `(kind, tabId)` → `{x, y, width,
    /// height}` or null. `kind` is "tab" (the tab header) or "tab_content" (the
    /// content; the host also switches to the tab).
    property var locate: null

    /// "ask" is the first-run question, "step" a scenario step, "" closed.
    property string mode: ""
    /// Identifier of the step being shown.
    property string stepId: ""
    /// Content of the step card from the core.
    property var content: ({})
    /// Cut-out around the target from the core.
    property var hole: ({ valid: false, x: 0, y: 0, width: 0, height: 0 })

    readonly property var st: tour.core.style
    readonly property var texts: tour.core.texts()
    readonly property var sizes: tour.core.pointSizes(
        tour.font.pointSize > 0 ? tour.font.pointSize : 10)

    parent: Overlay.overlay
    x: 0
    y: 0
    width: parent ? parent.width : 0
    height: parent ? parent.height : 0
    padding: 0
    modal: true
    // Our own dimming with a cut-out below; the stock one would paint over the
    // whole window.
    dim: false
    // Closed only with the buttons: an accidental click outside the card must
    // neither close the tutorial nor fall through into the window underneath.
    closePolicy: Popup.NoAutoClose
    focus: true
    enter: null
    exit: null

    // ── Tutorial flow ─────────────────────────────────────────────────────

    /// Execute the core decision `{kind, step}`.
    function begin(decision) {
        if (!decision || decision.kind === "nothing") {
            tour.finish();
        } else if (decision.kind === "ask") {
            tour.mode = "ask";
            tour.stepId = "";
            tour.hole = tour.core.hole(0, 0, 0, 0);
            tour.open();
            Qt.callLater(tour.settle);
        } else if (decision.kind === "show" && decision.step !== "") {
            tour.show(decision.step);
        }
    }

    function show(id) {
        const c = tour.core.card(id);
        if (c.title === undefined) {
            // The step is not in the scenario — an empty card is worse than nothing.
            tour.finish();
            return;
        }
        tour.content = c;
        tour.stepId = id;
        tour.mode = "step";
        tour.open();
        // The target is looked up after showing: the tab may have just switched.
        Qt.callLater(tour.settle);
    }

    /// Point the highlight, place the card and put focus on the main button.
    function settle() {
        tour.aim();
        primaryButton.forceActiveFocus();
    }

    function aim() {
        if (tour.mode !== "step") {
            tour.hole = tour.core.hole(0, 0, 0, 0);
            tour.place();
            return;
        }
        const a = tour.core.aim(tour.stepId);
        let rect = null;
        if (a.kind !== "window" && a.tab !== "" && tour.locate)
            rect = tour.locate(a.kind, a.tab);
        // The target was not found — the card goes to the centre, without an
        // empty frame.
        tour.hole = rect
            ? tour.core.hole(Math.round(rect.x), Math.round(rect.y),
                             Math.round(rect.width), Math.round(rect.height))
            : tour.core.hole(0, 0, 0, 0);
        tour.place();
    }

    function place() {
        const p = tour.core.placement(
            Math.round(tour.width), Math.round(tour.height),
            Math.round(card.width), Math.round(card.height),
            tour.hole.valid === true, tour.hole.x, tour.hole.y,
            tour.hole.width, tour.hole.height);
        card.x = p.x;
        card.y = p.y;
    }

    /// Main button: "Show" / "Next" / "Done".
    function primary() {
        if (tour.mode === "ask") {
            tour.begin(tour.core.answerAsk(true));
        } else if (tour.mode === "step") {
            const next = tour.core.advance(tour.stepId);
            if (next === "")
                tour.finish();
            else
                tour.show(next);
        }
    }

    /// Declining: "No thanks" (remembered) / "Skip" (the whole scenario).
    function secondary() {
        if (tour.mode === "ask")
            tour.core.answerAsk(false);
        else if (tour.mode === "step")
            tour.core.skipAll();
        tour.finish();
    }

    /// Esc. On the last step there is nothing to decline — it means "Done".
    function dismiss() {
        if (tour.mode === "step" && tour.content.skipVisible !== true)
            tour.primary();
        else
            tour.secondary();
    }

    function finish() {
        tour.mode = "";
        tour.stepId = "";
        tour.close();
    }

    onWidthChanged: if (tour.opened) tour.aim()
    onHeightChanged: if (tour.opened) tour.aim()

    Connections {
        target: tour.core

        // "Take the tutorial again" from Settings.
        function onReplayRequested() { tour.begin(tour.core.replay()); }
    }

    // ── Dimming with a cut-out ────────────────────────────────────────────

    background: Item {
        Shape {
            anchors.fill: parent

            ShapePath {
                fillColor: tour.st.dim
                strokeWidth: -1
                fillRule: ShapePath.OddEvenFill

                PathRectangle {
                    x: 0
                    y: 0
                    width: tour.width
                    height: tour.height
                }

                PathRectangle {
                    x: tour.hole.x
                    y: tour.hole.y
                    width: tour.hole.valid ? tour.hole.width : 0
                    height: tour.hole.valid ? tour.hole.height : 0
                    radius: tour.st.cornerRadius
                }
            }
        }

        // Neon frame around the highlighted place.
        Rectangle {
            visible: tour.hole.valid === true
            x: tour.hole.x
            y: tour.hole.y
            width: tour.hole.width
            height: tour.hole.height
            radius: tour.st.cornerRadius
            color: "transparent"
            border.width: tour.st.haloWidth
            border.color: tour.st.halo
        }
    }

    // ── Card ──────────────────────────────────────────────────────────────

    contentItem: FocusScope {
        id: scope
        focus: true

        Keys.onEscapePressed: (event) => { event.accepted = true; tour.dismiss(); }
        Keys.onReturnPressed: (event) => { event.accepted = true; scope.pressFocused(); }
        Keys.onEnterPressed: (event) => { event.accepted = true; scope.pressFocused(); }

        // Warning: focus stays on the card while the tutorial is open. The "Theme
        // catalogue" step switches the tab, and the page, finishing its build
        // asynchronously, takes focus for itself (the "Search themes…" field).
        // Caught by the Xvfb test bench on 12.09.2026: Esc and the arrows went to
        // the search field, and the tutorial could no longer be closed from the
        // keyboard or a gamepad.
        Connections {
            target: scope.Window.window
            enabled: tour.opened

            function onActiveFocusItemChanged() { Qt.callLater(scope.keepFocus); }
        }

        function keepFocus() {
            if (!tour.opened)
                return;
            for (let it = scope.Window.activeFocusItem; it; it = it.parent)
                if (it === scope)
                    return;
            primaryButton.forceActiveFocus();
        }

        function pressFocused() {
            if (secondaryButton.visible && secondaryButton.activeFocus)
                tour.secondary();
            else
                tour.primary();
        }

        Rectangle {
            id: card
            objectName: "onboardingCard"
            width: Math.min(tour.st.cardMaxWidth, tour.width - 2 * tour.st.cardGap)
            height: column.implicitHeight + tour.st.marginTop + tour.st.marginBottom
            color: tour.st.cardBackground
            border.width: tour.st.haloWidth
            border.color: tour.st.halo
            radius: tour.st.cardRadius

            onHeightChanged: tour.place()
            onWidthChanged: tour.place()

            ColumnLayout {
                id: column
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.leftMargin: tour.st.marginLeft
                anchors.rightMargin: tour.st.marginRight
                anchors.topMargin: tour.st.marginTop
                spacing: tour.st.spacing

                RexonText {
                    Layout.fillWidth: true
                    visible: tour.mode === "step"
                    text: tour.content.progress ?? ""
                    color: tour.st.cardMuted
                    font.pointSize: tour.sizes.progress
                }

                RexonText {
                    objectName: "onboardingTitle"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: tour.mode === "ask" ? tour.texts.askTitle
                                              : (tour.content.title ?? "")
                    color: tour.st.cardText
                    font.pointSize: tour.sizes.title
                    font.bold: true
                }

                RexonText {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: tour.mode === "ask" ? tour.texts.askText
                                              : (tour.content.text ?? "")
                    color: tour.st.cardText
                    font.weight: tour.st.bodyWeight
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    spacing: tour.st.buttonSpacing

                    // "No thanks" / "Skip" — a flat label.
                    Button {
                        id: secondaryButton
                        objectName: "onboardingSecondary"
                        visible: tour.mode === "ask" || tour.content.skipVisible === true
                        text: tour.mode === "ask" ? tour.texts.askNo
                                                  : (tour.content.skipText ?? "")
                        implicitHeight: Math.max(40, implicitContentHeight + topPadding + bottomPadding)
                        implicitWidth: Math.max(40, implicitContentWidth + leftPadding + rightPadding)
                        topPadding: tour.st.skipPadV
                        bottomPadding: tour.st.skipPadV
                        leftPadding: tour.st.skipPadH
                        rightPadding: tour.st.skipPadH
                        KeyNavigation.right: primaryButton
                        onClicked: tour.secondary()

                        contentItem: RexonText {
                            text: secondaryButton.text
                            font: secondaryButton.font
                            color: secondaryButton.hovered || secondaryButton.activeFocus
                                   ? tour.st.halo : tour.st.cardMuted
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            color: "transparent"
                            radius: tour.st.nextRadius
                            border.width: secondaryButton.activeFocus ? 2 : 0
                            border.color: tour.st.halo
                        }
                    }

                    Item { Layout.fillWidth: true }

                    // "Show" / "Next" / "Done" — filled with the accent.
                    Button {
                        id: primaryButton
                        objectName: "onboardingPrimary"
                        text: tour.mode === "ask" ? tour.texts.askYes
                                                  : (tour.content.nextText ?? "")
                        implicitHeight: Math.max(40, implicitContentHeight + topPadding + bottomPadding)
                        implicitWidth: Math.max(40, implicitContentWidth + leftPadding + rightPadding)
                        topPadding: tour.st.nextPadV
                        bottomPadding: tour.st.nextPadV
                        leftPadding: tour.st.nextPadH
                        rightPadding: tour.st.nextPadH
                        font.weight: tour.st.nextWeight
                        KeyNavigation.left: secondaryButton.visible ? secondaryButton : null
                        onClicked: tour.primary()

                        contentItem: RexonText {
                            text: primaryButton.text
                            font: primaryButton.font
                            color: tour.st.onAccent
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            color: primaryButton.hovered || primaryButton.down
                                   ? tour.st.haloHover : tour.st.halo
                            radius: tour.st.nextRadius
                            // Focus frame in the card's text colour: on an accent fill an accent
                            // frame would not be visible.
                            border.width: primaryButton.activeFocus ? 2 : 0
                            border.color: tour.st.cardText
                        }
                    }
                }
            }
        }
    }
}
