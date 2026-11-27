// Combo box for Rexon QML tabs in the application's standard skin.
// The reason for a separate type is the same as for RexonButton and
// RexonCheckBox: the Basic style does not follow the application palette.

// The list delegate refers to the id of the control itself. Without this
// pragma, QML lint rightly complains about unqualified access, and Qt
// resolves such names only through the deprecated delegate context.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

ComboBox {
    id: control

    /// Skin palette.
    property var pal: null

    /// Whether to show the combo box tooltip: under the cursor and ONLY while
    /// the list is closed. Callers write `ToolTip.visible: hintVisible`, not a
    /// bare `hovered`: after a click the cursor stays over the control, and the
    /// tooltip landed on top of the opened rows — the same class of bug as in
    /// the "Choose files / folder" menu (acceptance 10.10.2026) and the Axon
    /// tile menu.
    readonly property bool hintVisible: control.hovered && !control.popup.visible

    /// What should be selected — a VALUE from `valueRole`, not a row number.
    ///
    /// Warning: setting the selection with a `currentIndex: indexOfValue(...)`
    /// binding is NOT ALLOWED, and this is not a matter of taste. The binding is
    /// evaluated while the page is being built, when the combo box's internal
    /// delegate model is STILL EMPTY: at that moment `indexOfValue` returns −1
    /// for any model, and nothing re-evaluates the binding later — it has a
    /// single dependency, the value itself, and that no longer changes. The
    /// combo box stays at −1: the label is empty, the width collapses to a
    /// single arrow, and the popup inherits it and elides ALL rows.
    /// Measurement on 31.08 on the live "Appearance" tab: 9 rows, including the
    /// wanted `kde`, currentIndex −1, displayText empty, implicitWidth 38
    /// instead of 105; the same `indexOfValue("kde")` called a second later
    /// returns 4. Here the selection is set AFTER the model has been filled.
    property var selectedValue: undefined

    /// Width of the longest model row, without padding. Computed in place;
    /// there is nothing to assign from outside.
    ///
    /// The yardstick is the widget original: QComboBox sizes to its contents
    /// (`AdjustToContentsOnFirstShow`) by default, so both the control and the
    /// popup show names in full there. A QML combo box has no such sizing: it
    /// measures itself by the CURRENT label and the popup by the control, and
    /// long rows ("Tiling and Wayfire (sway/Hyprland/niri/River)") would be cut
    /// even with the right row selected.
    property real widestTextWidth: 0

    /// Colours from the core when the control is overridden by its tab's STYLE
    /// SHEET. The keys are the same as for a button's `tone`: `background`,
    /// `border`, `color`, `radius`. Empty means an ordinary skin combo box
    /// painted with a palette role.
    ///
    /// WHY. The palette role says what Qt paints the combo box with ITSELF.
    /// That holds while the page has no style sheet of its own; "Appearance"
    /// has one, and the combo box there is painted with the `bg_secondary`
    /// token, not the `Button` role.
    property var tone: null

    // Warning: Qt paints a non-editable combo box with the `Button` role, NOT
    // `Base`: a measurement on 04.09 on live Qt gave it (42,64,85) of the
    // `Button` role over its whole height, while the text field next to it is
    // (22,32,45) of the `Base` role. The item list below is already `Base`
    // (`surface_field`); these are different surfaces. Both colours come ready
    // from the core.
    readonly property color colField: (control.tone
                                       && control.tone.background !== undefined)
                                      ? control.tone.background
                                      : (control.pal ? control.pal.surface_button
                                                     : "#2a4055")
    readonly property color colBorder: (control.tone
                                        && control.tone.border !== undefined)
                                       ? control.tone.border
                                       : (control.pal ? control.pal.border : "#2a4055")
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colText: (control.tone
                                      && control.tone.color !== undefined)
                                     ? control.tone.color
                                     : (control.pal ? control.pal.text_primary : "#ffffff")
    readonly property color colPopup: control.pal ? control.pal.surface_field : "#16202d"
    readonly property color colHover: control.pal ? control.pal.bg_card_hover : "#263d52"

    // Never: the mouse wheel does NOT change the value. This is neither a
    // matter of taste nor a UX trifle.
    //
    // The `org.kde.desktop` style (it is active on the work machine; we do not
    // control the process style) sets `wheelEnabled: true` on combo boxes
    // (`/usr/lib/qt6/qml/org/kde/desktop/ComboBox.qml:31`), whereas Qt's own
    // default is `false`. Because of that the combo box catches the PAGE wheel
    // passing over it: it silently changes the selection and emits
    // `activated`, and the handler applies the setting for real. Measurement
    // on 06.09 in "Settings": while scrolling the page with the wheel, without
    // a single click on the combo box, the KDE system sound theme changed three
    // times (oxygen → zorinos2 → ocean) and was written to `kdeglobals` via
    // `kwriteconfig6` — on a customer's machine this would rewrite the LIVE
    // system theme. It also brings scrolling back: while the combo box ate the
    // event, the page under the pointer did not move.
    wheelEnabled: false

    implicitHeight: 40
    // The wide padding is for the arrow, the narrow one is on the other side.
    // For right-to-left languages the arrow is on the left, and the paddings
    // swap places.
    leftPadding: control.mirrored ? 28 : 10
    rightPadding: control.mirrored ? 10 : 28

    // Width follows the longest row, not the current one (see widestTextWidth).
    implicitWidth: Math.max(control.implicitBackgroundWidth + control.leftInset
                            + control.rightInset,
                            control.widestTextWidth + control.leftPadding
                            + control.rightPadding)

    // Ruler for measuring rows. Invisible and outside the scene: only its
    // measurement is needed.
    TextMetrics {
        id: textRuler
        font: control.font
    }

    /// Recompute the width and restore the selection from `selectedValue`.
    ///
    /// In one pass, because both have the same trigger: the model has been
    /// filled or replaced.
    function refreshFromModel() {
        var widest = 0;
        for (var i = 0; i < control.count; ++i) {
            textRuler.text = control.textAt(i);
            widest = Math.max(widest, Math.ceil(textRuler.advanceWidth));
        }
        control.widestTextWidth = widest;

        if (control.selectedValue === undefined || control.valueRole === "")
            return;
        var idx = control.indexOfValue(control.selectedValue);
        // We do not set −1: the value may arrive before the rows, and resetting an
        // already shown selection is worse than waiting for the next recompute.
        if (idx >= 0 && idx !== control.currentIndex)
            control.currentIndex = idx;
    }

    // The recompute is deferred to the end of the current event loop
    // iteration: by then both the rows and the internal delegate model are in
    // place (without the delay the count runs over nothing — the same trap as
    // described for selectedValue).
    Component.onCompleted: Qt.callLater(control.refreshFromModel)
    onCountChanged: Qt.callLater(control.refreshFromModel)
    onModelChanged: Qt.callLater(control.refreshFromModel)
    onSelectedValueChanged: Qt.callLater(control.refreshFromModel)

    background: Rectangle {
        radius: (control.tone && control.tone.radius !== undefined)
                ? control.tone.radius : 4
        color: control.colField
        border.color: control.activeFocus || control.hovered
                      ? control.colAccent : control.colBorder
        border.width: control.activeFocus ? 2 : 1
    }

    contentItem: RexonText {
        text: control.displayText
        color: control.colText
        font: control.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight

        // Warning: a stub for a foreign style, not something we need. The
        // `org.kde.desktop` style (it is active on the work machine; we do not
        // control the process style — the vendored QML of the Wallpaper Engine tab
        // lives next door) unconditionally sets up a touch input cursor on combo
        // boxes and asks the content for `positionToRectangle`. That method exists
        // on text INPUT fields; our content is a label, and every combo box
        // spammed the log with "TypeError: positionToRectangle is not a function"
        // (four entries every time "Settings" was opened). Our combo boxes are not
        // editable and there is no cursor to show, so we return an empty
        // rectangle.
        function positionToRectangle(position) {
            return Qt.rect(0, 0, 0, 0);
        }
    }

    indicator: RexonText {
        x: control.mirrored ? 10 : control.width - width - 10
        y: (control.height - height) / 2
        text: "▾"
        color: control.colText
    }

    delegate: ItemDelegate {
        id: item
        required property int index
        required property var modelData
        // A row as wide as the LIST, not as the control: the list can be wider
        // when the control did not get enough width (the layout set its own).
        width: item.ListView.view ? item.ListView.view.width : control.width
        highlighted: control.highlightedIndex === item.index

        background: Rectangle {
            color: item.highlighted ? control.colHover : "transparent"
        }
        contentItem: RexonText {
            text: control.textRole
                  ? (item.modelData[control.textRole] !== undefined
                     ? item.modelData[control.textRole] : "")
                  : item.modelData
            color: control.colText
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
    }

    popup: Popup {
        // A popup wider than the control grows towards the end of the line: for
        // right-to-left languages to the left, with its right edge flush with the
        // control.
        x: control.mirrored ? control.width - width : 0
        y: control.height
        // No narrower than the control, and no narrower than the longest row
        // either: the layout may squeeze the control (Layout.preferredWidth), but
        // the names in the list must be readable in full — just as in the widget
        // original.
        width: Math.max(control.width, control.widestTextWidth
                        + control.leftPadding + control.rightPadding)
        implicitHeight: Math.min(contentItem.implicitHeight + 2, 320)
        padding: 1

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.popup.visible ? control.delegateModel : null
            currentIndex: control.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator {}
        }

        background: Rectangle {
            color: control.colPopup
            border.color: control.colBorder
            border.width: 1
            radius: 4
        }
    }
}
