// Выпадающий список QML-вкладок Rexon в стандартном скине приложения.
// Причина отдельного типа — та же, что у RexonButton и RexonCheckBox: стиль
// Basic не следует палитре приложения.

// Делегат списка обращается к id самого контрола. Без этой прагмы проверка
// QML справедливо ругается на неквалифицированный доступ, а Qt разрешает
// такие имена лишь через устаревший контекст делегата.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

ComboBox {
    id: control

    /// Палитра скина.
    property var pal: null

    /// Что должно быть выбрано — ЗНАЧЕНИЕ из `valueRole`, а не номер строки.
    ///
    /// ⚠️ Заводить выбор привязкой `currentIndex: indexOfValue(...)` НЕЛЬЗЯ, и
    /// это не вкусовщина. Привязка считается при сборке страницы, когда
    /// внутренняя модель делегатов у списка ЕЩЁ ПУСТА: `indexOfValue` в этот
    /// миг отдаёт −1 при любой модели, а пересчитать привязку потом нечему —
    /// зависимость у неё одна, само значение, и оно уже не меняется. Список
    /// так и остаётся с −1: подпись пустая, ширина схлопывается до одной
    /// стрелки, а выпадашка наследует её и режет ВСЕ строки в многоточия.
    /// Замер 31.08 на живой вкладке «Оформление»: строк 9, среди них нужная
    /// `kde`, currentIndex −1, displayText пуст, implicitWidth 38 вместо 105;
    /// тот же `indexOfValue("kde")`, позванный секундой позже, отдаёт 4.
    /// Здесь выбор ставится ПОСЛЕ того, как модель наполнилась.
    property var selectedValue: undefined

    /// Ширина самой длинной строки модели, без отступов. Считается по месту,
    /// присваивать снаружи нечего.
    ///
    /// Мерило — оригинал на виджетах: у QComboBox отбор ширины по содержимому
    /// (`AdjustToContentsOnFirstShow`) стоит по умолчанию, поэтому и контрол,
    /// и выпадашка там показывают названия целиком. У QML-списка такого
    /// отбора нет: сам он меряется по ТЕКУЩЕЙ подписи, а выпадашка — по
    /// контролу, и длинные строки («Тайловые и Wayfire (sway/Hyprland/niri/River)») резались
    /// бы даже с правильно выбранной строкой.
    property real widestTextWidth: 0

    /// Краски от ядра, если контрол перекрыт ТАБЛИЦЕЙ СТИЛЕЙ своей вкладки.
    /// Ключи те же, что у `tone` кнопки: `background`, `border`, `color`,
    /// `radius`. Пусто — обычный список скина, крашенный ролью палитры.
    ///
    /// ЗАЧЕМ. Роль палитры отвечает, чем Qt красит список САМА. Это верно,
    /// пока у страницы нет своей таблицы стилей; у «Оформления» она есть, и
    /// список там красится токеном `bg_secondary`, а не ролью `Button`.
    property var tone: null

    // ⚠️Нередактируемый выпадающий список Qt красит ролью `Button`, а НЕ
    // `Base`: замер 04.09 на живой Qt дал ему (42,64,85) роли `Button` по
    // всей высоте, тогда как поле ввода рядом — (22,32,45) роли `Base`.
    // Список пунктов ниже — уже `Base` (`surface_field`), это разные
    // поверхности. Обе краски приходят готовыми из ядра.
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

    // ⛔Колесо мыши НЕ меняет значение. Это не вкусовщина и не мелочь UX.
    //
    // Стиль `org.kde.desktop` (он активен на рабочей машине; стилем процесса
    // мы не управляем) ставит выпадающему списку `wheelEnabled: true`
    // (`/usr/lib/qt6/qml/org/kde/desktop/ComboBox.qml:31`), тогда как у
    // самого Qt значение по умолчанию — `false`. Список от этого ловит
    // колесо СТРАНИЦЫ, проехавшее над ним: молча переставляет выбор и шлёт
    // `activated`, а обработчик применяет настройку по-настоящему. Замер
    // 06.09 в «Настройках»: за проходы колесом по странице, без единого
    // клика по списку, системная звуковая тема KDE сменилась трижды
    // (oxygen → zorinos2 → ocean) и легла в `kdeglobals` через
    // `kwriteconfig6` — у покупателя так перепишется ЖИВАЯ тема системы.
    // Заодно возвращается прокрутка: пока список ел событие, страница под
    // указателем стояла на месте.
    wheelEnabled: false

    implicitHeight: 40
    leftPadding: 10
    rightPadding: 28

    // Ширина — по самой длинной строке, а не по текущей (см. widestTextWidth).
    implicitWidth: Math.max(control.implicitBackgroundWidth + control.leftInset
                            + control.rightInset,
                            control.widestTextWidth + control.leftPadding
                            + control.rightPadding)

    // Линейка для замера строк. Невидимая и вне сцены: нужен только её счёт.
    TextMetrics {
        id: textRuler
        font: control.font
    }

    /// Пересчитать ширину и восстановить выбор по `selectedValue`.
    ///
    /// Одним проходом, потому что повод у обоих один и тот же: модель
    /// наполнилась или сменилась.
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
        // −1 не ставим: значение может прийти раньше строк, и обнулить уже
        // показанный выбор хуже, чем подождать следующего пересчёта.
        if (idx >= 0 && idx !== control.currentIndex)
            control.currentIndex = idx;
    }

    // Пересчёт откладывается на конец текущего круга событий: и строки, и
    // внутренняя модель делегатов к этому мигу уже на месте (без отсрочки
    // счёт идёт по пустоте — тот же капкан, что описан у selectedValue).
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

    contentItem: Text {
        text: control.displayText
        color: control.colText
        font: control.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight

        // ⚠️ Заглушка для чужого стиля, а не наша нужда. Стиль
        // `org.kde.desktop` (он активен на рабочей машине, стилем процесса
        // мы не управляем — рядом живёт вендорный QML вкладки Wallpaper
        // Engine) безусловно заводит у выпадающего списка курсор для
        // сенсорного ввода и спрашивает у содержимого `positionToRectangle`.
        // Такой метод есть у полей ВВОДА; у нас содержимое — надпись, и
        // каждый список сыпал в журнал «TypeError: positionToRectangle is
        // not a function» (по четыре записи на открытие «Настроек»).
        // Списки у нас не редактируются, курсору показывать нечего —
        // отдаём пустой прямоугольник.
        function positionToRectangle(position) {
            return Qt.rect(0, 0, 0, 0);
        }
    }

    indicator: Text {
        x: control.width - width - 10
        y: (control.height - height) / 2
        text: "▾"
        color: control.colText
    }

    delegate: ItemDelegate {
        id: item
        required property int index
        required property var modelData
        // Строка шириной со СПИСОК, а не с контролом: список бывает шире,
        // когда контролу ширины не досталось (раскладка задала свою).
        width: item.ListView.view ? item.ListView.view.width : control.width
        highlighted: control.highlightedIndex === item.index

        background: Rectangle {
            color: item.highlighted ? control.colHover : "transparent"
        }
        contentItem: Text {
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
        y: control.height
        // Не уже контрола, но и не уже самой длинной строки: раскладка вправе
        // прижать контрол (Layout.preferredWidth), а названия в списке должны
        // читаться целиком — так же, как в оригинале на виджетах.
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
