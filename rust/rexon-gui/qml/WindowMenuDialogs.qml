// Диалоги за пунктами меню главного окна: справка о программе, вопрос перед
// сбросом темы KDE и окно удаления Rexon.
//
// В продукте на их месте стоят `QMessageBox` (`window.py`: `_on_about`,
// `_on_recover_kde`, `_on_uninstall`). Виджетов в этом процессе нет, поэтому
// здесь `Dialog` из QtQuick Controls в скине продукта (`MenuDialog` ниже) —
// но текст в них ТОТ ЖЕ и приходит целиком от `WindowMenuModel`, то есть из
// ядра. Ни одной
// человеческой строки в этом файле не набрано: вторая копия разошлась бы с
// продуктом молча (правило #12).
//
// ⚠️ ПОРЯДОК КНОПОК УДАЛЕНИЯ И КНОПКА ПО УМОЛЧАНИЮ — НЕ КОСМЕТИКА. По
// умолчанию нажата «Отмена»: случайный Enter не должен снести программу с
// машины покупателя. Так в продукте, так и здесь, и решение это принимает
// ядро (`defaultId`), а не разметка.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    /// Объект меню на Rust — источник всех текстов и исполнитель действий.
    required property var menuModel

    /// Палитра скина.
    required property var pal

    /// Человек выбрал «Выход» либо удаление закончилось успехом (в продукте
    /// после удаления окно закрывается само).
    signal quitRequested()

    /// Какой диалог открыт сейчас — этим полем прогон окна проверяет, что
    /// пункт меню куда-то ведёт. Пусто — открытого диалога нет.
    ///
    /// ⚠️ СТАВИТСЯ В `onAboutToShow`, А НЕ В `onOpened`, И ФОКУС — ТАМ ЖЕ.
    /// `opened` приходит ПОСЛЕ анимации появления, а она есть не у всякого
    /// стиля: у Basic и Fusion её нет, у `org.kde.desktop` (стиль, который
    /// Qt берёт в сеансе Plasma) — есть. Замер 15.09.2026, прогон
    /// `--selftest` под xvfb: со стилем `org.kde.desktop` все три пункта
    /// меню «никуда не вели», а фокус в окне удаления стоял НЕ на «Отмене»;
    /// с Basic и в offscreen — зелёно. Для покупателя на KDE это значило,
    /// что всё время анимации Enter нажимал не «Отмену».
    property string openDialog: ""

    // ── Дверь из меню ─────────────────────────────────────────────────────
    //
    // Разбор идёт по идентификатору пункта, а не по подписи: подпись
    // переводится и меняется, идентификатор — нет.
    function activate(id) {
        if (id === "about") {
            aboutDialog.open();
        } else if (id === "recover_kde") {
            // Сначала — пакеты, подменяющие части Plasma: сброс темы их не
            // уберёт, и человек должен узнать это ДО сброса, а не из потока
            // вывода, которого он не видит.
            const found = root.menuModel.recoverForks();
            if (found.forks.length > 0) {
                console.info("найдены пакеты-замены Plasma: " + found.forks.join(", "));
                forksDialog.title = found.title;
                forksBody.text = found.text;
                forksRemoveButton.text = found.removeLabel;
                forksThemeButton.text = found.themeLabel;
                forksCancelButton.text = found.cancelLabel;
                forksDialog.open();
            } else {
                root.askRecover();
            }
        } else if (id === "uninstall") {
            const prompt = root.menuModel.uninstallPrompt();
            uninstallDialog.title = prompt.title;
            uninstallHead.text = prompt.text;
            uninstallBody.text = prompt.informative;
            keepButton.text = prompt.keepLabel;
            deleteButton.text = prompt.deleteLabel;
            cancelButton.text = prompt.cancelLabel;
            uninstallDialog.open();
        } else if (id === "quit") {
            root.quitRequested();
        } else {
            // Пункт, за которым ничего не стоит, — это обещание, которого
            // никто не выполнит. Пусть будет видно в журнале.
            console.warn("пункт меню «" + id + "» никуда не ведёт");
        }
    }

    /// Закрыть тот диалог, что открыт сейчас. Нужно прогону `--selftest`:
    /// он проверяет ПО ОЧЕРЕДИ, что каждый пункт меню куда-то ведёт, а
    /// незакрытый модальный диалог накрыл бы собой следующий.
    function closeOpenDialog() {
        aboutDialog.close();
        forksDialog.close();
        forksConfirmDialog.close();
        recoverDialog.close();
        uninstallDialog.close();
        noticeDialog.close();
    }

    /// Диалог закрылся. Сбрасывать «открыто» можно, только если не виден
    /// НИ ОДИН другой: `onAccepted` срабатывает ДО закрытия, и сообщение,
    /// открытое из него, иначе числилось бы закрытым (поймано стендом
    /// 11.09.2026 — «Да» → «Удаление идёт» давало `openDialog === ""`). То же
    /// в стилях с анимацией закрытия, где `closed` приходит позже `opened`
    /// следующего окна.
    function dialogClosed() {
        for (const d of [aboutDialog, forksDialog, forksConfirmDialog,
                         recoverDialog, uninstallDialog, noticeDialog])
            if (d.visible)
                return;
        root.openDialog = "";
    }

    /// Вопрос перед сбросом темы. На чужом рабочем столе вместо вопроса —
    /// объяснение: маршрут там откажет, и обещать «запущено» нельзя.
    function askRecover() {
        const prompt = root.menuModel.recoverPrompt();
        if (!prompt.allowed) {
            root.say(prompt.title, prompt.refusal);
            return;
        }
        recoverDialog.title = prompt.title;
        recoverBody.text = prompt.text;
        recoverDialog.open();
    }

    /// Кнопка окна про пакеты-замены нажата. Что делать дальше, решает ядро
    /// (`forksNext`) — то же решение, что у Python-окна.
    function forksChoice(buttonId) {
        forksDialog.close();
        const next = root.menuModel.forksNext(buttonId);
        if (next === "confirm_removal")
            root.askForksRemoval();
        else if (next === "theme_question")
            root.askRecover();
    }

    /// Последний вопрос перед снятием замен: ровно та команда, что
    /// выполнится. Вернуть стандартные пакеты нечем — вместо вопроса
    /// объяснение, и ничего не запускается.
    function askForksRemoval() {
        const preview = root.menuModel.forksPreview();
        if (!preview.ok) {
            root.say(preview.title, preview.text);
            return;
        }
        forksConfirmDialog.title = preview.title;
        forksConfirmBody.text = preview.text;
        forksConfirmDialog.runningTitle = preview.runningTitle;
        forksConfirmDialog.runningText = preview.runningText;
        forksConfirmDialog.open();
    }

    /// Держит ли клавиатурный фокус кнопка по умолчанию открытого окна
    /// удаления. Мерило защиты от дурака: случайный Enter не должен снести
    /// программу с машины покупателя, а проверить это нажатием нельзя —
    /// ошибись, и программа снесётся по-настоящему.
    function uninstallDefaultHasFocus() {
        return cancelButton.activeFocus;
    }

    /// Подписи трёх кнопок окна удаления — так, как они стоят СЕЙЧАС.
    /// Подписываются они в момент открытия окна, и пустая подпись здесь
    /// означала бы безымянную кнопку, за которой стоит снос программы.
    function uninstallButtonTexts() {
        return [keepButton.text, deleteButton.text, cancelButton.text];
    }

    /// Подписи кнопок окна удаления, которые НЕ влезли в свою кнопку и
    /// показаны обрезанными. Подпись «сохранить» длинная намеренно (на ней
    /// сказано, что сохранится), и «Удалить, но сохр…» прятало бы ровно
    /// то, ради чего она такая.
    function uninstallClippedLabels() {
        const out = [];
        for (const b of [keepButton, deleteButton, cancelButton])
            if (b.implicitContentWidth > b.availableWidth + 0.5)
                out.push(b.text + " (" + Math.round(b.availableWidth) + " из "
                         + Math.ceil(b.implicitContentWidth) + ")");
        return out;
    }

    /// Показать простое сообщение (итог сброса темы либо итог удаления).
    function say(title, text) {
        noticeDialog.title = title;
        noticeBody.text = text;
        noticeDialog.open();
    }

    // ── Общий вид ─────────────────────────────────────────────────────────
    //
    // ⚠️ ВСЕ ОКНА ЭТОГО ФАЙЛА СОБРАНЫ ИЗ ТРЁХ ДЕТАЛЕЙ НИЖЕ, И ЧИНИТЬ ИХ ВИД —
    // ЗДЕСЬ, А НЕ В КАЖДОМ ОКНЕ (правило #12). Поймано 30.09.2026 на окне
    // удаления: штатный `Dialog` стиля Basic рисовал белую шапку и светлые
    // кнопки поверх тёмного приложения (скин steam_dark, правило #17), а
    // пояснение, положенное в `ScrollView` без ширины, не переносилось
    // вовсе — строки обрезались справа («…and the des»), и человек не мог
    // дочитать, что удалится.

    /// Текст окна. `Text.Wrap` — по словам, а слово длиннее строки (путь
    /// без пробелов) — где придётся: иначе путь уходил бы за край.
    component Body: Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        // Тексты продукта — rich text (`<b>`, `<br>`, `<pre>`, `<h2>`), и
        // показывать их простым текстом значило бы показать разметку.
        textFormat: Text.RichText
        color: root.pal.text_primary
        // Ссылки — цветом акцента скина: по умолчанию Qt красит их #0000ff,
        // и на тёмном скине («Благодарности» в «О программе») их не видно.
        linkColor: root.pal.accent
        onLinkActivated: (link) => Qt.openUrlExternally(link)
    }

    /// Тело окна, которое ПЕРЕНОСИТСЯ по ширине окна и прокручивается,
    /// когда не влезает по высоте: окно вместе с кнопками обязано
    /// помещаться на экран 1280×800 (Steam Deck), иначе «Отмену» не видно.
    ///
    /// ⚠️ ШИРИНА ТЕКСТУ ЗАДАЁТСЯ ЯВНО. Содержимое `ScrollView` — это
    /// содержимое `Flickable`, раскладка на него не действует, и
    /// `Layout.fillWidth` там не значит ничего: без `width` текст брал
    /// ширину самой длинной строки и не переносился.
    component ScrollBody: ScrollView {
        id: scrollBody

        property alias text: scrollText.text
        property alias textFormat: scrollText.textFormat

        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(scrollText.implicitHeight,
                                         Math.max(120, root.Window.height - 320))
        contentWidth: availableWidth
        clip: true
        // Полоса видна всё время, пока есть что прокручивать: иначе стиль
        // Basic показывает её только во время прокрутки, и человек не знает,
        // что под последней видимой строкой есть ещё текст.
        ScrollBar.vertical.policy: contentHeight > height ? ScrollBar.AlwaysOn
                                                          : ScrollBar.AsNeeded

        Body {
            id: scrollText
            // Место под полосой прокрутки держится всегда: полоса стиля
            // Basic рисуется поверх содержимого и закрывала бы концы строк,
            // а ширина, зависящая от её видимости, замкнула бы круг
            // «ширина → высота → полоса → ширина».
            width: scrollBody.availableWidth - 14
        }
    }

    /// Ряд кнопок окна. Каждой кнопке — своя ширина: без выравнивания стили
    /// Basic и Fusion делят ряд на равные доли, и длинная подпись обрезалась
    /// бы, хотя место рядом есть. Стандартные кнопки (`standardButtons`)
    /// строятся делегатом — той же `RexonButton`, что во всём продукте.
    component Buttons: DialogButtonBox {
        alignment: Qt.AlignRight
        spacing: 8
        padding: 20
        topPadding: 12
        background: Item {}
        delegate: RexonButton { pal: root.pal }
    }

    /// Окно в скине продукта — того же вида, что окно операции над столом
    /// (`DeOperationDialog`): подложка карточки, затемнение под окном,
    /// заголовок крупным шрифтом, кнопки скина.
    component MenuDialog: Dialog {
        id: menuDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        padding: 20
        topPadding: 10

        background: Rectangle {
            color: root.pal.bg_card
            border.color: root.pal.border
            border.width: 1
            radius: 8
        }

        // Затемнение под модальным окном — общее решение продукта
        // (`ui_base::MODAL_DIM_RGBA`, ключ `modal_dim`). Стиль Basic фон не
        // гасит, и окно выходило модальным на деле, но не на вид.
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        header: Label {
            text: menuDialog.title
            color: root.pal.text_primary
            font.pointSize: 14
            font.weight: Font.DemiBold
            wrapMode: Text.WordWrap
            padding: 20
            bottomPadding: 0
        }

        footer: Buttons {}
    }

    // ── «О программе» ─────────────────────────────────────────────────────

    MenuDialog {
        id: aboutDialog
        // Заголовок и тело — из ядра; тело считается один раз при открытии.
        title: root.menuModel.aboutTitle()
        width: Math.min(root.Window.width - 80, 620)
        standardButtons: Dialog.Ok
        onClosed: root.dialogClosed()

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody {
                id: aboutBody
                text: root.menuModel.aboutHtml()
            }

            // Журнал Rexon: где лежит, открыть, скопировать конец — чтобы
            // отправить разработчику без поиска файла по диску. Тексты и
            // путь — из ядра (`window_menu::log_*`, `applog`).
            Body {
                id: logLocation
                Layout.fillWidth: true
                text: aboutDialog.logInfo.location || ""
                font.pointSize: 9
            }

            RowLayout {
                spacing: 8

                RexonButton {
                    pal: root.pal
                    text: aboutDialog.logInfo.openLabel || ""
                    onClicked: Qt.openUrlExternally(aboutDialog.logInfo.url)
                }

                RexonButton {
                    pal: root.pal
                    text: aboutDialog.logInfo.copyLabel || ""
                    onClicked: {
                        logClipboard.text = root.menuModel.logTail();
                        logClipboard.selectAll();
                        logClipboard.copy();
                        logClipboard.text = "";
                        logLocation.text = aboutDialog.logInfo.copied;
                    }
                }
            }

            // Буфер обмена из QML — через невидимое поле: у QML нет своего
            // API буфера, а `TextEdit.copy()` кладёт выделенное туда.
            TextEdit {
                id: logClipboard
                visible: false
                Layout.preferredWidth: 0
                Layout.preferredHeight: 0
            }
        }

        /// Сведения о журнале — один раз при открытии справки.
        property var logInfo: ({})
        onAboutToShow: {
            root.openDialog = "about";
            logInfo = root.menuModel.logInfo();
            logLocation.text = logInfo.location;
        }
    }

    // ── «Восстановить тему KDE»: пакеты-замены частей Plasma ──────────────
    //
    // Порт `_maybe_handle_plasma_forks`: три выбора — снять замены, только
    // сбросить тему, отмена. Для прогона окна это та же дверь пункта
    // «recover_kde», что и вопрос о сбросе темы.

    MenuDialog {
        id: forksDialog
        width: Math.min(root.Window.width - 80, 640)
        onClosed: root.dialogClosed()

        onAboutToShow: {
            root.openDialog = "recover_kde";
            // Кнопка по умолчанию — снятие замен, как в продукте: за ней
            // стоит ещё один вопрос, и там по умолчанию «Нет».
            forksRemoveButton.forceActiveFocus();
        }
        // Второй раз — после анимации: `DialogButtonBox` раздаёт фокус сам,
        // когда окно показано (см. окно удаления).
        onOpened: forksRemoveButton.forceActiveFocus()

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody { id: forksBody }
        }

        footer: Buttons {
            RexonButton {
                id: forksRemoveButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
                onClicked: root.forksChoice("remove")
            }

            RexonButton {
                id: forksThemeButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.ActionRole
                onClicked: root.forksChoice("theme")
            }

            RexonButton {
                id: forksCancelButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
                onClicked: root.forksChoice("cancel")
            }
        }
    }

    MenuDialog {
        id: forksConfirmDialog

        /// Что сказать, пока идёт снятие; приходит вместе с вопросом.
        property string runningTitle: ""
        property string runningText: ""

        width: Math.min(root.Window.width - 80, 640)
        standardButtons: Dialog.Yes | Dialog.No
        onClosed: root.dialogClosed()

        // «Нет» по умолчанию: за «Да» стоит снятие системных пакетов.
        onAboutToShow: {
            root.openDialog = "recover_kde";
            const no = forksConfirmDialog.standardButton(Dialog.No);
            if (no !== null)
                no.forceActiveFocus();
        }

        onAccepted: {
            if (root.menuModel.forksRemoveRun())
                root.say(forksConfirmDialog.runningTitle, forksConfirmDialog.runningText);
            else
                console.warn("снятие замен Plasma уже идёт");
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody { id: forksConfirmBody }
        }
    }

    // ── «Восстановить тему KDE» ───────────────────────────────────────────

    MenuDialog {
        id: recoverDialog
        width: Math.min(root.Window.width - 80, 620)
        standardButtons: Dialog.Yes | Dialog.No
        onClosed: root.dialogClosed()

        // «Нет» по умолчанию: сброс темы останавливает живые обои, и
        // случайный Enter не должен этого делать.
        onAboutToShow: {
            root.openDialog = "recover_kde";
            const no = recoverDialog.standardButton(Dialog.No);
            if (no !== null)
                no.forceActiveFocus();
        }

        onAccepted: {
            const done = root.menuModel.recoverApply();
            root.say(done.title, done.text);
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody { id: recoverBody }
        }
    }

    // ── «Удалить Rexon» ───────────────────────────────────────────────────

    MenuDialog {
        id: uninstallDialog
        // Шире обычного, если того требуют кнопки: подпись «сохранить»
        // называет, что сохранится, и обрезать её нельзя.
        width: Math.min(root.Window.width - 80,
                        Math.max(680, uninstallButtons.implicitWidth + leftPadding + rightPadding))
        onClosed: root.dialogClosed()

        onAboutToShow: {
            root.openDialog = "uninstall";
            // До анимации появления (см. `openDialog`): Enter, нажатый пока
            // окно выезжает, обязан попасть в «Отмену».
            cancelButton.forceActiveFocus();
        }

        onOpened: {
            // ⚠️ ФОКУС СТАВИТСЯ ЯВНО, И КАЖДОЕ ОТКРЫТИЕ ЗАНОВО. Одного
            // `focus: true` на кнопке не хватает: `DialogButtonBox` —
            // контейнер, и раздаёт фокус сам, когда окно показывается.
            // Поймано прогоном `--selftest` 09.09.2026 (строка
            // `REXON-MAINWINDOW-MENU-UNINSTALL-FOCUS`): фокус доставался НЕ
            // «Отмене», то есть защита от дурака, которую в продукте держит
            // `setDefaultButton(Cancel)`, здесь не работала вовсе.
            cancelButton.forceActiveFocus();
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            Body {
                id: uninstallHead
                font.bold: true
            }

            // Перечень удаляемого может быть длинным, а окно вопроса обязано
            // помещаться на экран вместе с кнопками: иначе «Отмену» не видно.
            ScrollBody { id: uninstallBody }
        }

        footer: Buttons {
            id: uninstallButtons

            RexonButton {
                id: keepButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
                onClicked: root.startUninstall(true)
            }

            // Необратимое — красным, как «Удалить» окна операции над столом.
            RexonButton {
                id: deleteButton
                pal: root.pal
                tone: root.pal.destructive_tone !== undefined ? root.pal.destructive_tone : null
                DialogButtonBox.buttonRole: DialogButtonBox.DestructiveRole
                onClicked: root.startUninstall(false)
            }

            RexonButton {
                id: cancelButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
                // Кнопка по умолчанию: защита от дурака.
                focus: true
                onClicked: uninstallDialog.close()
            }
        }
    }

    /// Взяться за удаление и закрыть вопрос. Итог придёт сигналом — за
    /// `pkexec` стоит запрос пароля, ждать его в потоке интерфейса нельзя.
    /// `keepProfile` — «сохранить настройки и резервные копии».
    function startUninstall(keepProfile) {
        uninstallDialog.close();
        if (!root.menuModel.uninstallRun(keepProfile))
            console.warn("удаление уже идёт");
    }

    Connections {
        target: root.menuModel

        function onUninstallFinished(ok, title, message) {
            root.say(title, message);
            if (ok)
                root.quitRequested();
        }

        // Итог снятия замен. Окно не закрывается: дальше нужен повторный
        // вход в сеанс, и об этом говорит сам итог.
        function onForksRemoveFinished(ok, title, message) {
            root.say(title, message);
        }
    }

    // ── Простое сообщение ─────────────────────────────────────────────────

    MenuDialog {
        id: noticeDialog
        width: Math.min(root.Window.width - 80, 560)
        standardButtons: Dialog.Ok
        onAboutToShow: root.openDialog = "notice"
        onClosed: root.dialogClosed()

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody {
                id: noticeBody
                // Итоги приходят обычным текстом с переводами строк, а не
                // разметкой: `\n` в rich text склеился бы в одну строку.
                textFormat: Text.PlainText
            }
        }
    }
}
