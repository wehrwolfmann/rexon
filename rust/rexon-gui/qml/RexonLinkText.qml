// Text with links taken from third-party text: item descriptions and
// comments, package details, extension descriptions.
//
// WHY. On 16.11.2026 a wallpaper author's link silently took the user to a
// paid theme in the browser. Now every such link goes through ONE path —
// this one:
//   * a link to a store item that Rexon shows as a card emits the
//     `cardWanted` signal (the page opens the card itself);
//   * an external link is marked ↗ and asks before opening
//     (`RexonLinkDialog`) unless the site is trusted;
//   * `rexon:…` emits the `pageLink` signal (the translation switch);
//   * a dangerous scheme (`javascript:`…) emits `status` with a refusal.
// The decision and the marking come from the core (`link_policy`) through
// the `LinkGuard` singleton. A page without its own cards
// (`cardsSupported: false`) gets the same question for a store item link
// as for any external site.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import org.rexon.gui 1.0

RexonText {
    id: lt

    /// Markup (or plain text when `plain` is set).
    property string html: ""
    /// Plain text without markup: it contains no links.
    property bool plain: false
    property var pal: null
    /// The page can open an item card on `cardWanted`.
    property bool cardsSupported: false

    signal pageLink(string link)
    signal cardWanted(var card, string url)
    signal status(string text)

    // The text is built from `html` by assignment, not by binding: windows
    // that set a ready `text` themselves (menu dialogs) keep their own, and
    // their links still go through the shared path below.
    textFormat: Text.RichText
    onHtmlChanged: lt.refresh()
    onPlainChanged: lt.refresh()
    Component.onCompleted: if (lt.html !== "") lt.refresh()
    onLinkActivated: function (link) { lt.activate(link); }

    function refresh() {
        lt.textFormat = lt.plain ? Text.PlainText : Text.RichText;
        lt.text = lt.plain ? lt.html : LinkGuard.mark(lt.html);
    }

    HoverHandler {
        cursorShape: lt.hoveredLink !== "" ? Qt.PointingHandCursor : Qt.ArrowCursor
    }

    function activate(link) {
        const d = JSON.parse(LinkGuard.decide(link));
        switch (d.action) {
        case "page":
            lt.pageLink(link);
            return;
        case "card":
            if (lt.cardsSupported) {
                lt.cardWanted(d.card, link);
                return;
            }
            break;
        case "open":
            LinkGuard.open(link, false);
            return;
        case "refuse":
            lt.status(d.refused);
            return;
        }
        const dlg = dialogComponent.createObject(lt.Overlay.overlay, { pal: lt.pal, destroyOnClose: true });
        if (dlg)
            dlg.ask(link, d);
    }

    Component {
        id: dialogComponent
        RexonLinkDialog {}
    }
}
