// Текст со ссылками из чужого текста: описания и комментарии товаров,
// сведения о пакете, описания расширений.
//
// ЗАЧЕМ. 16.11.2026 ссылка автора обоев молча увела человека в браузер на
// платную тему. Теперь любая такая ссылка идёт ОДНИМ путём — этим:
//   * ссылка на товар магазина, который Rexon показывает карточкой, —
//     сигнал `cardWanted` (страница открывает карточку у себя);
//   * внешняя ссылка помечена ↗ и перед открытием спрашивает
//     (`RexonLinkDialog`), если сайт не в доверенных;
//   * `rexon:…` — сигнал `pageLink` (переключатель перевода);
//   * опасная схема (`javascript:`…) — сигнал `status` с отказом.
// Решение и пометка — ядро (`link_policy`) через синглтон `LinkGuard`.
// Страница без своих карточек (`cardsSupported: false`) получает о ссылке
// на товар тот же вопрос, что о любом внешнем сайте.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import org.rexon.gui 1.0

RexonText {
    id: lt

    /// Разметка (или простой текст при `plain`).
    property string html: ""
    /// Простой текст без разметки: ссылок в нём нет.
    property bool plain: false
    property var pal: null
    /// Страница умеет открыть карточку товара по `cardWanted`.
    property bool cardsSupported: false

    signal pageLink(string link)
    signal cardWanted(var card, string url)
    signal status(string text)

    // Текст собирается из `html` присваиванием, а не привязкой: окна, что
    // кладут готовый `text` сами (диалоги меню), остаются при своём, а
    // ссылки у них всё равно идут общим путём ниже.
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
