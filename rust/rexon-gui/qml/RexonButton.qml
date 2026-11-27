// Button for Rexon QML tabs in the application's standard skin.
//
// Why a separate type: the default QtQuick Controls style (Basic) does NOT
// follow the application palette — its colours are hard-coded in the style
// itself. The widget part of Rexon is painted with a QPalette from
// `ui_base` and therefore looks like the product, while a bare `Button` on
// the same page would stay light on a dark background. Switching the whole
// process to the Fusion style is not possible: the Wallpaper Engine tab
// with vendored QML lives in the same process, and changing the style on
// the fly would affect its look.
//
// Colours come from outside through the `pal` property — a map from
// `ui_base::skin_colors("steam_dark")` handed over by the page's Rust
// object. The button defines no palette of its own: one skin for the whole
// product (rule #17).
//
// Height 40 is the control minimum from the canon (Big Picture / Steam
// Deck): tabs must be operable not only with the mouse.

import QtQuick
import QtQuick.Controls

Button {
    id: control

    /// Skin palette; without it the button falls back to safe steam_dark colours.
    property var pal: null

    /// Colours of a "special" button from the core — primary ("Apply", "Watch
    /// full screen"), green ("✓ Applied") or a toggle chip.
    /// A map with the keys `background`, `hover`, `pressed`, `disabled`,
    /// `disabledBorder`, `disabledColor`, `color`, `border`, `hoverBorder`,
    /// `radius`,
    /// `paddingH`, `paddingV`, `bold`; any of them may be missing. Empty means
    /// an ordinary skin button. A disabled button with explicit colours
    /// (`disabled`) is painted with them rather than dimmed with opacity: in the
    /// "Settings" widgets a disabled blue button is grey, not pale blue.
    ///
    /// WHY. In the widget part such buttons are painted by their own style
    /// sheet, and without this property the ported tab lost its accent: "Apply"
    /// came out grey just like "Cancel", although in the original it is blue,
    /// and "✓ Applied" is green. The button still defines no colours of its
    /// own: everything comes ready from the core.
    property var tone: null

    /// Colours of the CHECKED state (for `checkable` chips): the same keys.
    /// Empty means the checked state is drawn with the ordinary `highlighted`.
    property var checkedTone: null

    /// What paints the button right now: a checked chip wins over the ordinary tone.
    readonly property var activeTone: (control.checked && control.checkedTone)
                                      ? control.checkedTone : control.tone

    // Qt paints a button with the `Button` role — the colour comes ready from
    // the core (`surface_button`). The `border` token matched it only on the
    // standard skin: on "Light" the `Button` role is #e3e6e8 while the `border`
    // token is #cdd3da, and the button came out in the border colour.
    readonly property color colButton: control.pal ? control.pal.surface_button
                                                   : "#2a4055"
    readonly property color colButtonHover: control.pal ? control.pal.bg_card_hover
                                                        : "#263d52"
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    /// Outline of a button WITHOUT colours from the core. It is drawn by the Qt
    /// style itself, not by a palette role: a measurement on 05.09 on live Qt
    /// (steam_dark skin) gave the "Quick pairs" button of "Compare" a fill of
    /// (42,64,85) and a border of (85,102,119) — a white 0.20 overlay on top of
    /// the fill. The colour comes ready from the core
    /// (`ui_base::CONTROL_OUTLINE_*`, surface `surface_button_outline`), just
    /// like the colour of the splitter handle in "Desktops".
    readonly property color colOutline: control.pal
                                        && control.pal.surface_button_outline !== undefined
                                        ? control.pal.surface_button_outline
                                        : "#556677"
    readonly property color colTextBtn: control.pal ? control.pal.text_primary : "#ffffff"
    /// Colour of the DISABLED label — from the core palette (the `Disabled`
    /// group of the Qt palette). The button defines no share of its own.
    readonly property color colTextOff: control.pal ? control.pal.text_disabled
                                                    : "#8cffffff"

    // The label of a "special" button is set bolder: in the widget part this
    // is `font-weight: 600` in its style sheet.
    font.bold: control.activeTone !== null && control.activeTone.bold === true

    implicitHeight: 40
    // Warning: a disabled button is dimmed through its LABEL, not through the
    // opacity of the whole button. Opacity 0.45 used to be here in order "not
    // to define colours of our own", but it mixed in the page background and
    // CHANGED THE FILL COLOUR, whereas the widget original keeps the fill and
    // border as they are and mutes only the label
    // (`QPushButton:disabled { color: … }` in the "Install" block of
    // "Extensions", `#PageBtn:disabled` in "Appearance", and Qt gives the same
    // through the `Disabled` palette group without any style sheets).
    // Measurement on 05.09: 10 108 pixels on "Extensions". The colour of the
    // muted label comes from the core (`text_disabled`).
    padding: (control.activeTone && control.activeTone.paddingV !== undefined)
             ? control.activeTone.paddingV : 10
    leftPadding: (control.activeTone && control.activeTone.paddingH !== undefined)
                 ? control.activeTone.paddingH : 14
    rightPadding: leftPadding

    background: Rectangle {
        radius: (control.activeTone && control.activeTone.radius !== undefined)
                ? control.activeTone.radius : 4
        // Resolution order: colours from the core win over everything, then the
        // highlighted button, then the ordinary skin button.
        color: control.activeTone
               ? (!control.enabled && control.activeTone.disabled !== undefined
                  ? control.activeTone.disabled
                  : (control.down && control.activeTone.pressed !== undefined
                     ? control.activeTone.pressed
                     : ((control.hovered || control.down)
                        && control.activeTone.hover !== undefined
                        ? control.activeTone.hover
                        : (control.activeTone.background !== undefined
                           ? control.activeTone.background
                           : "transparent"))))
               : (control.highlighted
                  ? Qt.rgba(control.colAccent.r, control.colAccent.g,
                            control.colAccent.b, control.down ? 0.55 : 0.35)
                  : (control.hovered || control.down ? control.colButtonHover
                                                     : control.colButton))
        // The focus frame is always visible while the button has focus: it is the
        // focus chain, not a decoration. A chip's own border is visible only when
        // focus is elsewhere — otherwise it could not be told apart from the focus
        // frame. Under the cursor, some of the original's buttons change ONLY the
        // border while the fill stays put (`#RefreshBtn:hover`, `#DetailsBtn:hover`
        // in the "Appearance" style sheet).
        border.color: control.activeFocus
                      ? control.colAccent
                      : (!control.enabled && control.activeTone
                         && control.activeTone.disabledBorder !== undefined
                         ? control.activeTone.disabledBorder
                         : ((control.hovered || control.down)
                            && control.activeTone
                            && control.activeTone.hoverBorder !== undefined
                            ? control.activeTone.hoverBorder
                            : ((control.activeTone
                                && control.activeTone.border !== undefined)
                               ? control.activeTone.border
                               : control.colOutline)))
        // A button WITHOUT colours from the core always has an outline: that is
        // how the Qt style draws it in the original. For a button with colours the
        // core gives (or does not give) the border — there the tab's style sheet
        // overrides the style entirely.
        border.width: control.activeFocus
                      ? 2
                      : (!control.activeTone
                         || control.activeTone.border !== undefined ? 1 : 0)
    }

    // Warning: the label sits on a SHARED baseline computed from the button
    // font, not from the line height. An icon at the start of the label ("🗑",
    // "🔄") is drawn with a fallback font, and each such font has its own
    // ascent: the "🗑 Delete" label came out 3 pixels higher than its
    // neighbours "🔄 Switch" and "🔧 Check" (owner's screenshot,
    // 08.10). Now all buttons in a row share one baseline, whatever icon is in
    // front.
    //
    // Warning: an icon label (a single character: "🔍", "❮", "×") is NOT
    // elided: on a round button over a picture there is less room than the
    // icon's width, and instead of the magnifier Qt drew nothing — a dark
    // circle without an icon in the corner of the carousel. Such an icon is
    // drawn centred and whole.
    contentItem: Item {
        implicitWidth: label.implicitWidth
        implicitHeight: labelMetrics.height

        FontMetrics {
            id: labelMetrics
            font: control.font
        }

        RexonText {
            id: label
            width: parent.width
            y: Math.round((parent.height - labelMetrics.height) / 2
                          + labelMetrics.ascent - label.baselineOffset)
            text: control.text
            color: !control.enabled
                   ? ((control.activeTone
                       && control.activeTone.disabledColor !== undefined)
                      ? control.activeTone.disabledColor : control.colTextOff)
                   : ((control.activeTone && control.activeTone.color !== undefined)
                      ? control.activeTone.color : control.colTextBtn)
            font: control.font
            horizontalAlignment: Text.AlignHCenter
            elide: control.text.length <= 3 && control.text.indexOf(" ") < 0
                   ? Text.ElideNone : Text.ElideRight
        }
    }
}
