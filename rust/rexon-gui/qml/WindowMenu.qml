// Меню главного окна Rexon: кнопка «☰» и пункты под ней.
//
// В продукте это `QMenuBar` с одним подменю (`window.py`, `_build_menubar`).
// Здесь то же самое штатным `MenuBar` из QtQuick Controls: виджетов в этом
// процессе нет вовсе, а `menuBar:` у `ApplicationWindow` — то самое место,
// где меню и стоит (не `header:`, там полоса разделов).
//
// СВОИХ РЕШЕНИЙ ЗДЕСЬ НЕТ. Ни одной подписи, ни порядка, ни разделителя, ни
// горячей клавиши в этом файле не набрано: всё приходит списком `items` от
// `WindowMenuModel` (rust/rexon-gui/src/window_menu.rs), который спрашивает
// ядро (`rexon_core::window_menu`). Файл только рисует и сообщает наверх
// идентификатор нажатого пункта — по идентификатору, а не по подписи:
// подпись переводится, решение — нет.
//
// ⚠️ ПУНКТЫ СОЗДАЮТСЯ ПОИМЁННО, А НЕ `Repeater`-ом. Причина не вкусовая:
// `Menu` складывает детей в том порядке, в каком они к нему приходят, а
// `Repeater` создаёт свои элементы ПОСЛЕ статических соседей — разделитель,
// записанный отдельным элементом, уехал бы в начало меню. Здесь порядок
// строится ровно тем, что сказало ядро, включая место разделителя.
//
// ⚠️ ФОКУС С КЛАВИАТУРЫ. По меню обязано быть можно ходить: `MenuBar` берёт
// фокус Tab'ом и открывается пробелом, `Menu` водит по пунктам стрелками и
// закрывается Esc — всё это даёт сам Controls, но только если пункты
// настоящие `MenuItem` внутри настоящего `Menu`. Своей подделки под меню
// здесь нет намеренно: подделка потеряла бы цепочку фокуса, а правило
// проекта — держать её с самого начала, а не доделывать перед контроллером.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

MenuBar {
    id: root

    /// Объект меню на Rust. Ставит хозяин окна — тот же, что держит палитру:
    /// заведи объект здесь, и прогон окна до него бы не дотянулся.
    required property var menuModel

    /// Палитра скина. Своих красок меню не заводит (правило #15).
    required property var pal

    /// Краски опасного пункта («Удалить Rexon») — тон опасной кнопки из
    /// палитры окна (`destructive_tone`), тот же, что у «Удалить всё» в окне
    /// удаления. Нет тона — опасный пункт подсвечивается как обычный.
    readonly property var dangerTone: root.pal.destructive_tone !== undefined
                                      ? root.pal.destructive_tone : null

    /// Нажат пункт с этим идентификатором.
    signal itemActivated(string id)

    /// Сколько пунктов РЕАЛЬНО построено. Полный список ещё ничего не
    /// доказывает: он может быть полон, а делегаты не построиться — и меню
    /// окажется пустым при исправной модели. Этим числом прогон и мерит.
    property int builtItems: 0

    /// Сколько разделителей построено.
    property int builtSeparators: 0

    /// Сколько горячих клавиш построено. Клавиша живёт отдельным объектом, и
    /// не создайся он — «Ctrl+Q» пропал бы молча: меню при этом выглядело бы
    /// целым, а клавиша не работала бы вовсе.
    property int builtShortcuts: 0

    /// Открыть меню для прогона и закрыть его обратно.
    ///
    /// Ширину подписи у закрытого меню не измерить вовсе: пункты получают
    /// её только когда меню разложено, а до первого открытия их ширина —
    /// ноль. Поэтому прогон меню ОТКРЫВАЕТ. Человеку от этого ничего не
    /// достаётся: окно прогона живёт без экрана.
    function openForCheck() { burger.open(); }
    function closeAfterCheck() { burger.close(); }

    /// Построенные пункты (без разделителей) — по ним прогон мерит ширину.
    /// Список свой, а не `burger.itemAt()`: в меню лежит ещё и разделитель,
    /// у которого подписи нет вовсе, а ссылка на пункт здесь заодно держит
    /// его живым.
    property var entryItems: []

    /// Запас на колонку значка и поля подписи, которые стиль отвёл бы
    /// пункту сам, будь у него его собственный `contentItem`. Без запаса
    /// меню выходит ровно в ширину букв — то есть на грани обреза.
    readonly property real labelReserve: 12

    /// Сколько места нужно самому широкому пункту — по нему и считается
    /// ширина меню (см. `implicitWidth` у `Menu` ниже). Пересчитывается,
    /// когда пункт меняет запрос: подпись переводится, шрифт системы
    /// меняется на ходу, и ширина, посчитанная один раз при сборке, к
    /// вечеру соврала бы.
    property real wantedWidth: 0

    function remeasure() {
        let w = 0;
        for (const it of root.entryItems)
            if (it)
                w = Math.max(w, it.wantedWidth);
        root.wantedWidth = w;
    }

    /// Разложено ли меню: у каждого пункта есть ширина. Пока её нет, мерить
    /// подписи бессмысленно — «влезло» вышло бы у всех и всегда.
    function laidOut() {
        if (root.entryItems.length === 0)
            return false;
        for (const it of root.entryItems)
            if (!it || it.width <= 0)
                return false;
        return true;
    }

    /// Подписи пунктов, которые НЕ поместились в свою ширину и показаны
    /// обрезанными.
    ///
    /// ⚠️ Счёт пунктов такого не ловит: их может быть ровно столько, сколько
    /// назвало ядро, и каждый — на своём месте, а человек прочтёт
    /// «🛟 Восстановить те…» и не узнает, что за дверь за этим пунктом.
    /// Поэтому меряется ШИРИНА, и меряет её сам Qt (`Text.truncated`).
    function clippedLabels() {
        const out = [];
        for (const it of root.entryItems)
            if (it && it.labelClipped)
                out.push(it.text + " (" + Math.round(it.labelHas) + " из "
                         + Math.ceil(it.labelNeeds) + ")");
        return out;
    }

    /// Ширина меню и самой длинной подписи — для журнала прогона: по этим
    /// числам видно, ТЕСНО ли меню ещё до того, как подпись обрежется.
    function widthReport() {
        let widest = 0;
        for (const it of root.entryItems)
            if (it)
                widest = Math.max(widest, it.labelNeeds);
        return "меню " + Math.round(burger.width)
             + " | пункту доступно " + Math.round(burger.availableWidth)
             + " | самой длинной подписи нужно " + Math.ceil(widest);
    }

    /// Части меню, покрашенные НЕ скином, — для прогона окна.
    ///
    /// ⚠️ Счёт и ширина пунктов этого не ловят: меню 30.09.2026 было полным,
    /// подписи целыми, а подложка — белой от стиля Basic поверх тёмного
    /// окна. Меряем саму краску: подложку меню и подпись каждого пункта.
    function offSkinParts() {
        const out = [];
        if (!Qt.colorEqual(menuSurface.color, root.pal.bg_card))
            out.push("подложка меню " + menuSurface.color
                     + " вместо " + root.pal.bg_card);
        for (const it of root.entryItems) {
            if (!it)
                continue;
            const want = it.enabled ? root.pal.text_primary : root.pal.text_disabled;
            if (!it.highlighted && !Qt.colorEqual(it.inkColor, want))
                out.push("подпись «" + it.text + "» " + it.inkColor
                         + " вместо " + want);
        }
        if (root.dangerTone === null
                && root.entryItems.some(it => it && it.destructive))
            out.push("у опасного пункта нет краски опасной кнопки "
                     + "(в палитре окна нет destructive_tone)");
        return out;
    }

    Menu {
        id: burger
        title: root.menuModel.buttonText

        // ⚠️ ШИРИНУ МЕНЮ СЧИТАЕМ САМИ, И ВОТ ПОЧЕМУ.
        //
        // Пункт со СВОИМ `contentItem` (он здесь свой — ради «Ctrl+Q»
        // справа) теряет то, что стиль отводит пункту сам: колонку значка
        // и поля по краям подписи. Меню после этого выходит ровно в ширину
        // букв, без единой точки запаса, и подпись живёт на грани: любое
        // округление ширины всплывающего окна съедает последний знак, и
        // вместо «🛟 Восстановить тему KDE» человек читает «🛟 Восстановить
        // те…» — не зная, что за дверь за этим пунктом (замер 18.09.2026:
        // подписи нужно 164 точки, меню давало 164).
        //
        // ⚠️ И это НЕ особенность одного стиля. У `Menu` из Fusion и Basic
        // ширина вообще не следует за пунктами: список внутри не сообщает
        // наверх свою ширину, меню остаётся 200 точек при любой подписи, и
        // на машине без стиля KDE (GNOME, XFCE, голый Qt) обрезалось бы
        // всё, что длиннее. Поэтому считаем не «поправку к стилю», а саму
        // ширину: по самому широкому пункту, как это делает `QMenu`
        // продукта. Нижняя граница от стиля остаётся — его фон уже,
        // пусть меню не съёживается.
        implicitWidth: Math.max(burger.implicitBackgroundWidth
                                + burger.leftInset + burger.rightInset,
                                root.wantedWidth
                                + burger.leftPadding + burger.rightPadding)

        // Подложка меню — карточка скина, как у окон этого же меню
        // (`MenuDialog` в WindowMenuDialogs.qml). Подложка стиля Basic белая
        // и палитре скина не следует.
        background: Rectangle {
            id: menuSurface
            implicitWidth: 200
            implicitHeight: 40
            color: root.pal.bg_card
            border.color: root.pal.border
            border.width: 1
            radius: 4
        }

        // Компоненты и не-Item'ы `Menu` в свои пункты не кладёт (они уходят
        // в `data`), поэтому объявлять их прямо здесь безопасно.
        Component {
            id: itemComponent

            MenuItem {
                id: entry

                property string entryId: ""
                /// За пунктом необратимое (решает ядро, `is_destructive`).
                property bool destructive: false
                /// Чем красить подсвеченный пункт: опасный — краской опасной
                /// кнопки, прочие — подсветкой строки списка скина.
                readonly property bool dangerLit: entry.destructive
                                                  && entry.highlighted
                                                  && root.dangerTone !== null
                /// Цвет подписи и клавиши. Выключенный пункт гасится той же
                /// краской, что выключенная кнопка (`text_disabled`).
                readonly property color inkColor: !entry.enabled
                                                  ? root.pal.text_disabled
                                                  : (entry.dangerLit
                                                     ? root.dangerTone.color
                                                     : root.pal.text_primary)
                /// Горячая клавиша ЭТОГО пункта в записи Qt — только чтобы
                /// её напечатать. Слушает клавишу отдельный `Shortcut` (см.
                /// ниже): у закрытого меню пункты клавиш не разбирают.
                property string keys: ""

                /// Сколько места пункту нужно, чтобы подпись читалась
                /// ЦЕЛИКОМ: вся подпись, клавиша, зазор между ними, поля
                /// пункта от стиля и запас на колонку значка (`labelReserve`
                /// у `MenuBar` выше). Меряется по самим надписям, а не по
                /// `implicitContentWidth`: раскладка сообщает свою ширину на
                /// следующем проходе, и меню успело бы открыться по старому
                /// числу.
                readonly property real wantedWidth:
                    entryLabel.implicitWidth
                    + (entry.keys !== ""
                       ? entryRow.spacing + entryKeys.implicitWidth : 0)
                    + entry.leftPadding + entry.rightPadding
                    + root.labelReserve
                onWantedWidthChanged: root.remeasure()

                /// Показана ли подпись обрезанной («🛟 Восстановить те…»).
                /// Ответ даёт сам Qt, а не наш пересчёт ширин: `truncated`
                /// говорит ровно то, что человек видит на экране.
                readonly property bool labelClipped: entryLabel.truncated
                /// Сколько подписи нужно и сколько ей досталось — чтобы в
                /// журнале прогона стояло не «обрезана», а насколько.
                readonly property real labelNeeds: entryLabel.implicitWidth
                readonly property real labelHas: entryLabel.width

                onTriggered: root.itemActivated(entry.entryId)

                // ⚠️ КЛАВИША ПЕЧАТАЕТСЯ В ПУНКТЕ. `QMenu` в продукте сам
                // приписывает «Ctrl+Q» справа и приглушённо, а `MenuItem`
                // из Controls не печатает ничего: клавиша работала бы, но
                // человек о ней не узнал бы вовсе (проверено снимком
                // 09.09.2026 — в продукте «Ctrl+Q» стоит, в Rust-окне его
                // не было).
                //
                // Краски — из палитры скина, как и подложка пункта ниже: роли
                // `palette` у стиля Basic светлые и скину не следуют (белое
                // меню поверх тёмного окна, снимок 30.09.2026).
                contentItem: RowLayout {
                    id: entryRow

                    spacing: 24

                    Text {
                        id: entryLabel

                        text: entry.text
                        font: entry.font
                        color: entry.inkColor
                        elide: Text.ElideRight
                        verticalAlignment: Text.AlignVCenter
                        Layout.fillWidth: true
                    }

                    Text {
                        id: entryKeys

                        visible: entry.keys !== ""
                        text: entry.keys
                        font: entry.font
                        color: entry.inkColor
                        opacity: 0.6
                        verticalAlignment: Text.AlignVCenter
                    }
                }

                // Подсветка строки — та же, что у пунктов выпадающих списков
                // скина (`bg_card_hover`); у опасного пункта — краска опасной
                // кнопки. Рамка — фокус с клавиатуры и геймпада: по меню ходят
                // стрелками, и невидимый фокус равен его отсутствию.
                background: Rectangle {
                    implicitWidth: 200
                    implicitHeight: 40
                    radius: 3
                    color: entry.dangerLit ? root.dangerTone.background
                           : (entry.highlighted ? root.pal.bg_card_hover
                                                : "transparent")
                    border.width: entry.visualFocus ? 2 : 0
                    border.color: root.pal.accent
                }
            }
        }

        Component {
            id: separatorComponent

            MenuSeparator {
                contentItem: Rectangle {
                    implicitWidth: 188
                    implicitHeight: 1
                    color: root.pal.border
                }
            }
        }

        Component {
            id: shortcutComponent

            Shortcut {
                property string entryId: ""
                onActivated: root.itemActivated(entryId)
            }
        }

        // ⚠️ РОДИТЕЛЬ СОЗДАВАЕМОГО ПУНКТА — `contentItem` МЕНЮ, А НЕ САМО
        // МЕНЮ. `Menu` — не элемент сцены (`Popup` наследует QObject), и
        // пункт, созданный с таким родителем, рождается ВНЕ графической
        // сцены: Qt на каждый такой вызов пишет в журнал «Created graphical
        // object was not placed in the graphics scene» — по строке на пункт
        // и на разделитель, пять строк на каждый запуск. Глушить вывод
        // нечем и незачем: у меню есть настоящий элемент-хозяин, в который
        // `addItem` пункт всё равно переложит, — это `contentItem`. Создаём
        // сразу в нём, и предупреждения нет, потому что нет и повода.
        //
        // Объект клавиши (`Shortcut`) элементом сцены не является вовсе, и
        // его родителем остаётся само меню: ему нужен хозяин по времени
        // жизни, а не место на экране.
        Component.onCompleted: {
            const entries = root.menuModel.items;
            for (let i = 0; i < entries.length; ++i) {
                const e = entries[i];
                if (e.separatorBefore) {
                    burger.addItem(separatorComponent.createObject(
                                       burger.contentItem));
                    ++root.builtSeparators;
                }
                const item = itemComponent.createObject(burger.contentItem, {
                    entryId: e.id,
                    text: e.label,
                    keys: e.shortcut,
                    enabled: e.enabled,
                    destructive: e.destructive === true
                });
                if (item === null) {
                    // Молчать нельзя: пропавший пункт — это пропавшая дверь
                    // к удалению программы или к сбросу сломанной темы.
                    console.warn("пункт меню «" + e.id + "» не построился");
                    continue;
                }
                burger.addItem(item);
                root.entryItems.push(item);
                ++root.builtItems;

                // Горячая клавиша живёт отдельным `Shortcut`, а не свойством
                // пункта: у `MenuItem` его нет вовсе, а `Action` внутри
                // закрытого меню Qt не слушает — то есть «Ctrl+Q» работал бы
                // только при открытом меню, чего в продукте нет.
                if (e.shortcut !== "") {
                    const key = shortcutComponent.createObject(burger, {
                        entryId: e.id,
                        sequence: e.shortcut
                    });
                    if (key === null) {
                        console.warn("горячая клавиша пункта «" + e.id
                                     + "» не построилась");
                    } else {
                        ++root.builtShortcuts;
                        // В пункте — подпись клавиши на языке системы
                        // («Strg+Q» по-немецки), как её печатает `QMenu`:
                        // `nativeText` переводит имена клавиш каталогом
                        // qtbase. Ядро хранит запись Qt («Ctrl+Q») — она
                        // для `sequence`, не для глаз.
                        if (key.nativeText !== "")
                            item.keys = key.nativeText;
                    }
                }
            }
            // Пункты сложены — теперь и ширина меню считается по ним. Сам
            // пункт о своей ширине рассказывает раньше, чем попадает в
            // список, и без этого вызова меню осталось бы шириной со свой
            // фон.
            root.remeasure();
        }
    }

    delegate: MenuBarItem {
        id: barItem

        contentItem: Text {
            text: barItem.text
            font: barItem.font
            // Краски — из палитры окна. В продукте на этом месте стоит QSS,
            // и стоит он не для красоты: оконный слой рабочего стола отдаёт
            // `QMenuBar` СВОЮ палитру, и на светлой системной теме знак «☰»
            // выходил тёмным по тёмному фону скина — то есть НЕВИДИМЫМ, а за
            // ним живут «Восстановить тему KDE», «О программе», «Удалить
            // Rexon» и «Выход». В QML этой беды нет вовсе: платформенной
            // палитры у Controls не бывает, цвет берётся здесь и только
            // здесь.
            color: root.pal.text_primary
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }

        background: Rectangle {
            implicitWidth: 34
            implicitHeight: 30
            radius: 4
            color: barItem.highlighted || barItem.activeFocus
                   ? root.pal.surface_card : "transparent"
            // Рамка сфокусированной кнопки: по меню ходят с клавиатуры, и
            // невидимый фокус равен его отсутствию.
            border.width: barItem.activeFocus ? 2 : 0
            border.color: root.pal.accent
        }
    }

    // ⚠️ КНОПКА «☰» СТОИТ СПРАВА, А НЕ СЛЕВА. В продукте это делает
    // `bar.setLayoutDirection(RightToLeft)`, и место у кнопки не вкусовое:
    // слева от неё, вплотную, начинается полоса разделов, и кнопка,
    // поставленная там же, читается как первый раздел. Проверено снимком
    // 09.09.2026 — в Rust-окне «☰» вышел в левом верхнем углу, в продукте он
    // в правом.
    //
    // Мерило — сам продукт, поэтому строка стоит именно здесь: `MenuBar`
    // раскладывает пункты своим `contentItem`, и обычный `LayoutMirroring`
    // на кнопке не помог бы — его пришлось бы пускать в детей, а вместе с
    // ними зеркально встало бы и само подменю. Подменю остаётся слева
    // направо (`menu.setLayoutDirection(LeftToRight)` в продукте): оно —
    // отдельное окно и раскладки полосы не наследует.
    contentItem: Row {
        spacing: root.spacing
        layoutDirection: Qt.RightToLeft

        Repeater {
            model: root.contentModel
        }
    }

    background: Rectangle {
        color: root.pal.bg_primary
    }
}
