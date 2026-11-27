// Text field for Rexon QML tabs in the application's standard skin.
// The reason for a separate type is the same as for RexonButton: the Basic
// style does not follow the application palette.

import QtQuick
import QtQuick.Controls
import org.rexon.gui 1.0

TextField {
    id: control

    // ── Action journal (`ActionJournal`, core — `ui_watch`) ──────────────
    //
    // Button presses are caught by the application-wide input filter, but it
    // cannot see what is typed into a field — so the field reports it itself,
    // in ONE shared control for all tabs. Typing is only a mark for the window
    // watchdog (no line); the end of input (Enter, focus loss) is one line if
    // the value changed. Never: the text of a password field does not go to
    // the journal at all — an empty string is passed here, and the core
    // writes no line.
    readonly property bool journalSecret: control.echoMode !== TextInput.Normal
    readonly property string journalField: control.placeholderText !== ""
                                           ? control.placeholderText : control.objectName
    onTextEdited: ActionJournal.typing(control, control.journalField,
                                       control.journalSecret ? "" : control.text,
                                       control.journalSecret)
    onEditingFinished: {
        const journalId = ActionJournal.entered(control, control.journalField,
                                                control.journalSecret ? "" : control.text,
                                                control.journalSecret);
        if (journalId > 0)
            Qt.callLater(function() { ActionJournal.finish(journalId); });
    }

    /// Skin palette.
    property var pal: null

    // The field is painted by Qt ITSELF with the `Base` palette role, not by
    // the tab's style sheet — so the colour comes ready from the core
    // (`surface_field`, `ui_base::QML_SURFACES`). Previously a token that
    // looked similar, `bg_card`, was used here, and on the standard skin the
    // field came out (30,45,61) instead of (22,32,45) — one step lighter than
    // the original.
    readonly property color colField: control.pal ? control.pal.surface_field
                                                  : "#16202d"
    /// Border from the TAB's style sheet when the tab has one. Empty means the
    /// Qt style outline. Warning: both cases are real: in "Extensions",
    /// "Settings" and "Steam" the field is not overridden at all and its border
    /// is drawn by the style itself, while in "Appearance" the `#SearchBox` rule
    /// puts the skin's `border` token there and the style takes no part at all.
    property var borderTone: null

    // The core hands over the style outline as a ready colour
    // (`ui_base::CONTROL_OUTLINE_*`, surface `surface_field_outline`): a
    // measurement on 05.09 (the "Extensions" search field, steam_dark skin)
    // gave (73,83,96) — a white 0.20 overlay on top of the page background,
    // while the `border` token gives (42,64,85), 31 levels darker.
    readonly property color colBorder: control.borderTone !== null
                                       ? control.borderTone
                                       : (control.pal
                                          && control.pal.surface_field_outline !== undefined
                                          ? control.pal.surface_field_outline
                                          : "#495360")
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colText: control.pal ? control.pal.text_primary : "#ffffff"
    readonly property color colHint: control.pal ? control.pal.text_secondary : "#8f98a0"

    // ── Start of the line for right-to-left languages ────────────────────
    //
    // Without explicit alignment the field takes it from the text ITSELF, and
    // an empty field from the input direction, but not from window mirroring.
    // The placeholder (`placeholderText`) in Qt Quick Controls is a separate
    // `Text` that repeats the field's alignment only if it is set explicitly;
    // otherwise it too aligns by its own string. The result with `-reverse`:
    // the Russian placeholder sat on the left. Mirroring flips an explicit
    // `AlignLeft` (`effectiveHorizontalAlignment` becomes right) — both for the
    // typed text and for the placeholder; for left-to-right languages
    // everything stays as it was. The left and right paddings are equal, so
    // the text position does not need to be moved.
    horizontalAlignment: TextInput.AlignLeft

    // Accessibility: Qt publishes the placeholder only as the DESCRIPTION, so
    // a screen reader announced an unnamed "text" (found by
    // `rexon-a11y-robot`). The placeholder is the field's label in every
    // section — it becomes the name; a field may still set its own.
    Accessible.name: control.placeholderText

    implicitHeight: 40
    leftPadding: 10
    rightPadding: 10
    color: control.colText
    placeholderTextColor: control.colHint
    selectByMouse: true

    // ── Enter does not leave the page for the window frame ───────────────
    //
    // Warning: the Qt text field HANDLES the confirm key (the `accepted`
    // signal) but does NOT ACCEPT it: this is intended for widget dialogs,
    // where Enter must trigger the default button. Here, however, the QML
    // scene lives inside a `QQuickWidget`, and an unaccepted key goes up the
    // widget tree from it — through the page host, the tab stack and all the
    // way to the product's main window.
    //
    // Measurement on 08.09.2026 (a live Wayland session and offscreen gave the
    // same): Enter in the "Appearance" theme search reached `RexonMainWindow`
    // as a `ShortcutOverride`+`KeyPress` pair with code 0x1000004, even though
    // the search itself worked. This is exactly the path through which the
    // finding "Enter hides the main window" could happen at all: what the
    // window frame does with a foreign key is no longer up to the page.
    //
    // Fixed in the SHARED field, not in the "Appearance" search: there are
    // several text fields in the QML tabs (search in "Extensions", "Settings",
    // "Steam", "Wallpaper Engine"), and they all share one leak.
    //
    // Warning: it must be `Keys.AfterItem`: with the normal order the handler
    // would sit BEFORE the field, eat the key first — and the pages'
    // `onAccepted` would never fire. After the field the signal has already
    // been emitted, and the key goes no further. Other keys (arrows, Escape)
    // are not accepted by the field itself, so the page handlers keep
    // receiving them.
    Keys.priority: Keys.AfterItem
    Keys.onReturnPressed: (event) => { event.accepted = true; }
    Keys.onEnterPressed: (event) => { event.accepted = true; }

    background: Rectangle {
        radius: 4
        color: control.colField
        border.color: control.activeFocus ? control.colAccent : control.colBorder
        border.width: control.activeFocus ? 2 : 1
    }
}
