// Slider for Rexon QML tabs in the application's standard skin.
// The reason for a separate type is the same as for RexonButton: the Basic
// style does not follow the application palette.

// The tick delegate refers to the id of the slider itself; without the
// pragma, QML lint rightly complains about unqualified access.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

Slider {
    id: control

    /// Skin palette.
    property var pal: null

    /// Tick step under the groove (in value units); 0 means no ticks.
    /// Counterpart of `QSlider.TicksBelow` in the widget version: the blur
    /// strength slider has a tick at every step.
    property real tickStep: 0

    readonly property color colTrack: control.pal ? control.pal.track
                                                  : "rgba(127,127,127,0.18)"
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colBorder: control.pal ? control.pal.border : "#2a4055"

    implicitHeight: 40

    background: Rectangle {
        x: control.leftPadding
        y: control.topPadding + control.availableHeight / 2 - height / 2
        implicitWidth: 160
        implicitHeight: 6
        width: control.availableWidth
        height: implicitHeight
        radius: 3
        color: control.colTrack

        // The fill goes from the START of the scale to the handle: from the left
        // edge for left to right, from the right edge for right to left. The width
        // follows `position`, not `visualPosition`: in a mirrored slider the latter
        // equals 1 − position, and the wrong side would be filled from the left edge.
        Rectangle {
            x: control.mirrored ? parent.width - width : 0
            width: control.position * parent.width
            height: parent.height
            radius: 3
            color: control.colAccent
        }
    }

    // Ticks: short strokes under the groove, one per step, in the border colour.
    Repeater {
        model: control.tickStep > 0
               ? Math.floor((control.to - control.from) / control.tickStep) + 1
               : 0

        delegate: Rectangle {
            required property int index
            readonly property real ratio: control.tickStep * index
                                          / (control.to - control.from)
            // On-screen position of a tick: a mirrored slider counts the scale from
            // the right edge, just like the handle (`visualPosition`).
            readonly property real visualRatio: control.mirrored ? 1 - ratio : ratio
            x: control.leftPadding + control.handle.width / 2
               + visualRatio * (control.availableWidth - control.handle.width) - width / 2
            y: control.topPadding + control.availableHeight / 2
               + control.background.height / 2 + 3
            width: 1
            height: 5
            color: control.colBorder
        }
    }

    handle: Rectangle {
        x: control.leftPadding + control.visualPosition
           * (control.availableWidth - width)
        y: control.topPadding + control.availableHeight / 2 - height / 2
        implicitWidth: 18
        implicitHeight: 18
        radius: 9
        color: control.pressed ? control.colAccent : control.colBorder
        border.color: control.activeFocus || control.hovered
                      ? control.colAccent : control.colBorder
        border.width: 2
    }
}
