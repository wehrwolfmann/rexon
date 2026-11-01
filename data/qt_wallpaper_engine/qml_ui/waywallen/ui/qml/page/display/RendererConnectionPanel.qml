pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Qcm.Material as MD
import waywallen.ui as W

ColumnLayout {
    id: control

    required property var target

    readonly property string connectedId: {
        if (!target)
            return "";
        const links = target.links || [];
        return links.length > 0 ? (links[0].rendererId || "") : "";
    }
    readonly property bool active: {
        if (!target)
            return false;
        const links = target.links || [];
        return links.length > 0 && !!links[0].active;
    }
    // Keep the manager dependency so late renderer events update this panel.
    readonly property var renderer: {
        const _ = W.App.rendererManager.renderers;
        return connectedId.length > 0 ? W.App.rendererManager.get(connectedId) : null;
    }
    readonly property int activePlaylistId: target ? Number(target.activePlaylistId || 0) : 0
    readonly property var playlistStatus: target ? (target.playlistStatus || ({})) : ({})
    readonly property bool hasPlaylist: activePlaylistId > 0
    readonly property string playlistDetail: {
        const status = playlistStatus || ({});
        const parts = [];
        const count = Number(status.count || 0);
        const position = Number(status.position || 0);
        const remaining = Number(status.remainingSecs || 0);
        if (count > 0)
            parts.push(Math.min(position + 1, count) + " / " + count);
        if (remaining > 0)
            parts.push(qsTr("%n min left", "", Math.ceil(remaining / 60)));
        return parts.join(" · ");
    }

    spacing: 8

    MD.Divider {
        Layout.fillWidth: true
        Layout.topMargin: 4
        Layout.bottomMargin: 4
    }

    MD.Text {
        text: control.active ? qsTr("Connected") : qsTr("Assigned")
        typescale: MD.Token.typescale.title_small
        color: MD.Token.color.on_surface
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 16

        RowLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            spacing: 8

            MD.Icon {
                readonly property string status: control.renderer ? control.renderer.status : ""
                name: {
                    if (!control.renderer || !control.active)
                        return MD.Token.icon.pause;
                    return status === "paused" ? MD.Token.icon.pause : MD.Token.icon.play_arrow;
                }
                size: 24
                color: !control.renderer || !control.active || status === "paused" ? MD.Token.color.on_surface_variant : MD.Token.color.primary
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                spacing: 0

                MD.Text {
                    Layout.fillWidth: true
                    text: {
                        const renderer = control.renderer;
                        if (renderer) {
                            const name = renderer.name && renderer.name.length ? renderer.name : "renderer";
                            return renderer.pid > 0 ? (name + "-" + renderer.pid) : name;
                        }
                        if (control.connectedId.length > 0)
                            return control.connectedId;
                        return qsTr("Idle");
                    }
                    typescale: MD.Token.typescale.body_medium
                    color: control.renderer ? MD.Token.color.on_surface : MD.Token.color.on_surface_variant
                    font.family: control.renderer ? "monospace" : ""
                    elide: Text.ElideMiddle
                }

                MD.Text {
                    Layout.fillWidth: true
                    visible: !!control.renderer
                    text: {
                        const renderer = control.renderer;
                        if (!renderer)
                            return "";
                        const parts = [(renderer.status || ""), (renderer.fps || 0) + " fps"];
                        const textureWidth = Number(renderer.textureWidth || 0);
                        const textureHeight = Number(renderer.textureHeight || 0);
                        if (textureWidth > 0 && textureHeight > 0)
                            parts.push(textureWidth + " × " + textureHeight);
                        return parts.join(" · ");
                    }
                    typescale: MD.Token.typescale.label_small
                    color: MD.Token.color.on_surface_variant
                    elide: Text.ElideRight
                }
            }
        }

        RowLayout {
            visible: control.hasPlaylist
            Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
            Layout.maximumWidth: Math.max(220, control.width * 0.4)
            spacing: 8

            MD.Icon {
                name: MD.Token.icon.playlist_play
                size: 24
                color: MD.Token.color.primary
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                spacing: 0

                MD.Text {
                    Layout.fillWidth: true
                    text: qsTr("Playlist #%1").arg(control.activePlaylistId)
                    typescale: MD.Token.typescale.body_medium
                    color: MD.Token.color.on_surface
                    elide: Text.ElideRight
                }

                MD.Text {
                    Layout.fillWidth: true
                    visible: control.playlistDetail.length > 0
                    text: control.playlistDetail
                    typescale: MD.Token.typescale.label_small
                    color: MD.Token.color.on_surface_variant
                    elide: Text.ElideRight
                }
            }
        }
    }
}
