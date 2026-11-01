// Поле ввода QML-вкладок Rexon в стандартном скине приложения.
// Причина отдельного типа — та же, что у RexonButton: стиль Basic не следует
// палитре приложения.

import QtQuick
import QtQuick.Controls

TextField {
    id: control

    /// Палитра скина.
    property var pal: null

    // Поле красит САМА Qt ролью палитры `Base`, а не таблица стилей
    // вкладки, — поэтому краска приходит готовой из ядра (`surface_field`,
    // `ui_base::QML_SURFACES`). Прежде здесь стоял похожий на глаз токен
    // `bg_card`, и на стандартном скине поле выходило (30,45,61) вместо
    // (22,32,45) — на ступень светлее оригинала.
    readonly property color colField: control.pal ? control.pal.surface_field
                                                  : "#16202d"
    /// Рамка от таблицы стилей ВКЛАДКИ, когда она у вкладки есть. Пусто —
    /// обводка стиля Qt. ⚠️ Оба случая настоящие: у «Расширений»,
    /// «Настроек» и «Steam» поле ничем не перекрыто и рамку ему рисует сам
    /// стиль, а у «Оформления» правило `#SearchBox` кладёт туда токен
    /// `border` скина, и стиль в этом не участвует вовсе.
    property var borderTone: null

    // Обводку стиля ядро отдаёт готовой краской (`ui_base::CONTROL_OUTLINE_*`,
    // поверхность `surface_field_outline`): замер 05.09 (поле поиска
    // «Расширений», скин steam_dark) дал (73,83,96) — накладку белого 0.20
    // поверх фона страницы, а токен `border` даёт (42,64,85), на 31 уровень
    // темнее.
    readonly property color colBorder: control.borderTone !== null
                                       ? control.borderTone
                                       : (control.pal
                                          && control.pal.surface_field_outline !== undefined
                                          ? control.pal.surface_field_outline
                                          : "#495360")
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colText: control.pal ? control.pal.text_primary : "#ffffff"
    readonly property color colHint: control.pal ? control.pal.text_secondary : "#8f98a0"

    implicitHeight: 40
    leftPadding: 10
    rightPadding: 10
    color: control.colText
    placeholderTextColor: control.colHint
    selectByMouse: true

    // ── Enter не уходит из страницы в каркас окна ────────────────────────
    //
    // ⚠️ Поле ввода Qt клавишу подтверждения ОБРАБАТЫВАЕТ (сигнал
    // `accepted`), но НЕ ПРИНИМАЕТ: так задумано ради виджетных диалогов, где
    // на Enter обязана срабатывать кнопка по умолчанию. У нас же сцена QML
    // лежит внутри `QQuickWidget`, и непринятая клавиша уходит из неё вверх
    // по дереву виджетов — через хозяина страницы, стопку вкладок и до самого
    // главного окна продукта.
    //
    // Замер 08.09.2026 (живой сеанс Wayland и offscreen дали одно и то же):
    // Enter в поиске тем «Оформления» доезжал до `RexonMainWindow` парой
    // `ShortcutOverride`+`KeyPress` с кодом 0x1000004, хотя поиск при этом
    // отрабатывал. Это и есть дорожка, по которой находка «Enter прячет
    // главное окно» вообще могла состояться: что сделает с чужой клавишей
    // каркас окна — уже не во власти страницы.
    //
    // Правится в ОБЩЕМ поле, а не в поиске «Оформления»: полей ввода в
    // QML-вкладках несколько (поиск «Расширений», «Настроек», «Steam»,
    // «Wallpaper Engine»), и утечка у них одна на всех.
    //
    // ⚠️ Именно `Keys.AfterItem`: при обычном порядке обработчик стоял бы
    // ПЕРЕД полем, съедал клавишу первым — и `onAccepted` у страниц не
    // сработал бы вовсе. После поля сигнал уже отправлен, а клавише дальше
    // хода нет. Прочие клавиши (стрелки, Escape) поле само не принимает,
    // поэтому обработчики страниц продолжают их получать.
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
