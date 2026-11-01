// Вкладка «Оформление» на QML. Данные и решения — Rust-объект
// AppearanceModel (rust/rexon-gui/src/appearance_page.rs), который сам
// ничего не решает, а спрашивает ядро (`rexon_core::appearance_page`).
// Здесь только РАЗМЕТКА.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из скина по умолчанию всего продукта
// (правило #17). Своих цветов страница не заводит — даже краски кнопки
// плитки приезжают из ядра (`page.buttonColors(state)`).
//
// ── Побочные действия ────────────────────────────────────────────────────
//
// Сеть и диск живут снаружи: хозяин страницы кладёт в `root.actions`
// объект с методами
//   environment()             -> строка JSON: {qtStyles, widgetStyle,
//                                aurHelper, detectedDe, savedDe, seenDe}
//   loadCatalog(requestJson)  -> void (работа долгая, ответ приходит
//                                сигналом catalogReady; поле `fresh`
//                                значит «обойди свой кеш сканов диска»)
//   loadPreview(job, itemId, url, low)
//                             -> void (одна картинка; ответ сигналами
//                                previewDone/previewFailed с тем же `job`.
//                                Писать её надо туда же, где страница ищет
//                                попадания в кеш, — путь считает ядро,
//                                `appearance_page::preview_cache_path`)
//   cancelPreviews()          -> void (оборвать всё летящее за картинками)
//   loadPrefetch(requestJson) -> void (фоновый прогрев кеша: показывать
//                                нечего, ответ сигналом prefetchReady)
//   remember(deId, seen)      -> bool (запомнить выбор рабочего стола;
//                                пустой `seen` — «какой стол запущен
//                                сейчас, спроси у машины сам»)
//   loadDetail(itemId)        -> void (полные детали товара, ответ сигналом)
//   installById(itemId, apply, fileName)-> void (установка, ход дела
//                             сигналами; fileName — файл, выбранный руками,
//                             пусто — автоматический)
//   installAur(pkgName)       -> void (сборка пакета помощником)
//   actOnStoreItem(itemId)    -> void (кнопка товара магазина: не стоит —
//                                поставить, стоит — применить)
//   applyLocalById(itemId)    -> void (применить тему с диска; у дополнения
//                                окружения — включить/выключить, у Conky —
//                                запустить/остановить)
//   globalApplyQuestion(itemId)-> строка JSON окна «Применить «X»?»
//                                глобальной темы Plasma; пусто — окна нет,
//                                кнопка делает то же, что прежде
//   applyGlobalById(itemId, appearance, layout, parts) — parts: маска частей
//                                    подробного вида, -1 — простой вид
//                             -> void (применить глобальную тему с выбором
//                                окна; итог сигналом actionDone)
//   conkyAutostart(itemId)    -> bool (автозапуск виджета Conky включён?)
//   setConkyAutostart(itemId, on) -> bool (переключить; итог сигналом actionDone)
//   cursorSize()              -> int (текущий размер курсора, 0 — не задан)
//   applyCursorSize(size)     -> void (размер курсора; итог сигналом actionDone)
//   uninstall(itemJson)       -> void (товар магазина — по журналу; тема
//                                с диска `local:…` — как removeLocalById)
//   removeLocalById(itemId)   -> void (тему с диска пометить к удалению —
//                                сигнал removalPending; отказ — actionDone)
//   commitRemovals()          -> void (снять помеченное; итог removalDone)
//   undoRemovals()            -> int  (снять пометки; сколько вернулось)
//   toggleFavorite(itemId)    -> строка JSON: состав избранного после
//   openUrl(url)              -> bool (страница товара в браузере)
//   purgeTranslations()       -> void (разовая чистка протухших переводов)
// и сигналами
//   catalogReady(payload)     — ответ на запрос каталога (строка JSON)
//   detailReady(payload)      — полные детали товара (строка JSON)
//   descriptionReady(payload) — переведённое описание ВДОГОНКУ уже показанной
//                               карточке (строка JSON: `itemId`,
//                               `translated`). Приходит только когда перевод
//                               случился: модели нет — сигнала нет, на панели
//                               остаётся оригинал
//   previewDone(job, path)    — картинка заявки записана в кеш
//   previewFailed(job, status)— заявка не удалась: `status` это ответ
//                               сервера числом строкой, пусто (ответа не
//                               было) либо «canceled» (оборвали мы сами)
//   prefetchReady(payload)    — ответ на фоновый прогрев (строка JSON: те же
//                               поля запроса плюс `items`)
//   installProgress(pct)      — доля выполненного установки, 0–100
//   installLog(text)          — очередная строка журнала установки
//   statusMessage(text)       — сообщение в строку состояния
//   actionDone(text, ok)      — итог установки/применения
//   removalPending(text)      — тема с диска помечена к удалению: текст
//                               строки «Вернуть»
//   removalDone(text)         — помеченное снято: итог для строки состояния
//   galleryReady(index, path) — готовый кадр карусели
//   galleryFailed(index)      — кадр не доехал
//   galleryFinished(count)    — обход ссылок закончен
//   commentsReady(payload)    — страница комментариев (строка JSON)
//   commentTranslationsReady(payload)
//                             — переводы комментариев ВДОГОНКУ уже показанному
//                               списку (строка JSON: `itemId`, `page` и `rows`
//                               с парами «оригинал → перевод»). В ответе только
//                               то, что действительно перевелось: русские
//                               комментарии в нём не упоминаются, и ссылки
//                               «показать оригинал» у них не будет
//   suggestReady(payload)     — подсказки поиска (строка JSON)
//
// Синхронный ответ у всего, что быстро; сигналы — только у долгого
// (грабля 18 схемы переноса). Пока эти методы на Python
// (rexon/appearance/ocs_client.py и rexon/appearance/installed_scan.py);
// когда живой исполнитель магазина приедет на Rust, на их место встанет он
// с теми же именами — страница не изменится.
//
// ⚠️ Объект действий приходит ДВУМЯ разными путями, и страница обязана
// работать при обоих: хозяин вкладки ставит его начальным свойством
// (`setInitialProperties`, как у «Steam»), а хозяин-стенд — свойством уже
// ПОСЛЕ `setSource` (как у «Настроек», грабля 20). Поэтому пуск собран в
// одну функцию `bootstrap()` со сторожем, и её зовут оба события —
// `Component.onCompleted` и `onActionsChanged`. Кто придёт первым, тот и
// поднимет страницу; второй ничего не сделает.

// Делегаты обращаются к id внешних элементов (root, page). Без этой прагмы
// проверка QML справедливо ругается на неквалифицированный доступ.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    // Объект побочных действий. По умолчанию — СВОЙ, на Rust; хозяин страницы
    // может подставить другой (Python-вкладка так и делает начальным
    // свойством). Состав методов и сигналов описан в шапке файла выше.
    property var actions: ownActions

    AppearanceActions {
        id: ownActions
        // Имя нужно проверкам — тем же приёмом, что у модели ниже.
        objectName: "appearanceActions"
    }

    // Кнопки «послушать» тем звуков: поиск файла события, проигрыватель и
    // скачивание товара Магазина до установки — ядро
    // (`rexon_core::sound_preview`), объект только передаёт вызовы.
    SoundPreviewActions {
        id: soundActions
        objectName: "soundPreviewActions"
    }

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    AppearanceModel {
        id: page
        // Имя нужно проверкам: модель достают
        // `root.findChild(QObject, "appearanceModel")` и спрашивают её без
        // единой правки в самой странице.
        objectName: "appearanceModel"

        // Ядро решило, что состав каталога устарел, — идём к хозяину.
        onCatalogWanted: root.askCatalog()

        // Открытому товару нужны полные детали: список магазина отдаёт
        // меньше скриншотов, чем есть у товара, и описание в нём обрезано.
        onDetailWanted: function (itemId) {
            // Пакет AUR: паспорт RPC и картинки — свой вызов.
            if (page.detailSource === "aur") {
                if (root.actions && root.actions.loadAurDetail)
                    root.actions.loadAurDetail(itemId);
                return;
            }
            if (root.actions && root.actions.loadDetail)
                root.actions.loadDetail(itemId);
        }

        // Кадр карусели просит ядро — оно же знает, какой именно нужен.
        onGalleryWanted: function (index, url) {
            if (root.actions && root.actions.loadGalleryFrame)
                root.actions.loadGalleryFrame(index, url);
        }

        onCommentsWanted: function (itemId, commentsPage) {
            // У пакета AUR комментарии — со страницы пакета на сайте.
            if (page.detailSource === "aur") {
                if (root.actions && root.actions.loadAurComments)
                    root.actions.loadAurComments(itemId, commentsPage);
                return;
            }
            if (root.actions && root.actions.loadComments)
                root.actions.loadComments(itemId, commentsPage);
        }

        onSuggestWanted: function (request) {
            if (root.actions && root.actions.loadSuggestions)
                root.actions.loadSuggestions(request);
        }

        // ── Картинки ─────────────────────────────────────────────────────
        //
        // Кого качать, сколько разом и в каком порядке, решает очередь ядра.
        // Разметка лишь передаёт заявку хозяину и возвращает ответ обратно
        // тем же номером.
        onPreviewWanted: function (job, itemId, url, low) {
            if (root.actions && root.actions.loadPreview)
                root.actions.loadPreview(job, itemId, url, low);
        }

        // Сбой был временным — ядро назвало паузу до повтора.
        onPreviewRetryWanted: function (generation, job, delayMs) {
            root.schedulePreviewRetry(generation, job, delayMs);
        }

        // Раздел сменили: летящие картинки прежнего к делу больше не
        // относятся, а канал нужен новому.
        onPreviewsCanceled: {
            if (root.actions && root.actions.cancelPreviews)
                root.actions.cancelPreviews();
        }

        // Фоновый прогрев кеша: соседние разделы и следующая страница.
        // Показывать его нечему — он лишь наполняет кеш.
        onPrefetchWanted: function (request) {
            if (root.actions && root.actions.loadPrefetch)
                root.actions.loadPrefetch(request);
        }
    }

    // Прогрев остальных разделов начинается ПОЗЖЕ первой загрузки: спорить
    // за сеть с тем разделом, что открыт, он не должен.
    Timer {
        interval: page.prefetchStartMs
        repeat: false
        running: root.started
        onTriggered: page.startCategoryPrefetch()
    }

    // Пауза между прогревами — не долбить магазин. Этим же таймером очередь
    // будят после ответа каталога: пока открытый раздел грузился, она стояла.
    Timer {
        id: prefetchPause
        interval: page.prefetchPauseMs
        repeat: false
        onTriggered: page.pumpPrefetch()
    }

    // Разовая лёгкая чистка протухших переводов. Работа не горячая: записи
    // и так не отдаются по сроку годности при чтении, чистка лишь
    // освобождает место в кеше — потому и делается позже всего остального.
    Timer {
        interval: page.purgeTranslationsMs
        repeat: false
        running: root.started
        onTriggered: {
            if (root.actions && root.actions.purgeTranslations)
                root.actions.purgeTranslations();
        }
    }

    // Применение полной темы поднимает в НАШЕМ же процессе смену стиля Qt
    // (Plasma рассылает её по D-Bus). Пересобирать сетку прямо в этот момент
    // нельзя — ждём, пока шторм осядет, и только потом перечитываем раздел,
    // чтобы бейдж «✅ Активна» переехал на применённую тему.
    Timer {
        interval: page.styleStormMs
        repeat: false
        running: page.reloadPending
        onTriggered: page.reloadAfterStyleStorm()
    }

    // Вкладку закрывают: новых прогревов не выдаём, летящие картинки рвём.
    Component.onDestruction: page.closePage()

    // Готовые картинки ставим по одной за проход цикла событий: шесть
    // раскодированных JPEG подряд дают заметный рывок. Таймер живёт ровно
    // пока очередь ядра говорит, что отдавать есть что.
    //
    // ⚠️ Одна миллисекунда, а не ноль: у оригинала стоял QTimer с нулём, но
    // ПОВТОРЯЮЩИЙСЯ Timer из QtQuick с нулевым промежутком не срабатывает ни
    // разу — он построен на паузе-анимации, и пауза нулевой длины проходит
    // мимо. Замер 29.08.2026 (1,5 с): `interval: 0` с `repeat: true` — ноль
    // срабатываний, с `repeat: false` — одно. Здесь повтор нужен, значит
    // нулю тут не место: очередь готовых копилась, а на плитках навсегда
    // оставалась пустота — при полном кеше картинок на диске.
    Timer {
        interval: 1
        repeat: true
        running: page.previewReadyPending
        onTriggered: page.drainPreview()
    }

    // Повтор одной заявки: свой таймер на заявку, как одноразовый таймер
    // оригинала. Повторы редки — плодить их не страшно.
    Component {
        id: previewRetryTimer
        Timer {
            repeat: false
            property string generation: ""
            property string job: ""
        }
    }

    function schedulePreviewRetry(generation, job, delayMs) {
        const t = previewRetryTimer.createObject(root, {
            "interval": parseInt(delayMs, 10) || 0,
            "generation": generation,
            "job": job
        });
        if (!t)
            return;
        t.triggered.connect(function () {
            page.previewRetry(t.generation, t.job);
            t.destroy();
        });
        t.start();
    }

    // Карусель показанных плиток греется ПОСЛЕ них и с паузой: человек
    // часто пробегает разделы насквозь, и без паузы каждый мелькнувший
    // успевал бы заказать два десятка полноразмерных скриншотов.
    property string galleryWarmGeneration: ""

    Timer {
        id: galleryWarm
        interval: page.galleryWarmDelayMs
        repeat: false
        onTriggered: page.warmGallery(root.galleryWarmGeneration)
    }

    // ── Палитра стандартного скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colSecondary: root.pal.bg_secondary
    readonly property color colCard: root.pal.bg_card
    readonly property color colCardHover: root.pal.bg_card_hover
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary
    readonly property color colStar: root.pal.rating_star

    // ── Контролы, перекрытые таблицей стилей вкладки ──────────────────────
    //
    // ⚠️ Общие поверхности палитры (`surface_button`, `surface_field`)
    // отвечают, чем Qt красит контрол САМА по роли. Это верно, пока у
    // страницы нет своей таблицы стилей. У «Оформления» она есть, и роль в
    // ней не участвует вовсе: выпадающий список красится токеном
    // `bg_secondary`, полоса страниц — `bg_card`, второстепенная кнопка —
    // `bg_card_hover`. Замер 05.09: QML клал туда роль `Button` (42,64,85) и
    // расходился с оригиналом на 6 475 точек у списка, 2 339 у кнопки
    // обновления и 2 121 у «Избранного». Решение остаётся в ядре
    // (`appearance_page::control_colors`), разметка его только кладёт.
    readonly property var toneCombo: page.controlColors("combo")
    readonly property var toneNav: page.controlColors("nav")
    readonly property var toneSecondary: page.controlColors("secondary")

    // ── Чип-переключатель ─────────────────────────────────────────────────
    //
    // Отдельный вид кнопки, а не общий `RexonButton`: у чипов оригинала
    // (`#SrcChip` в таблице стилей) свой рисунок — тонкая рамка, невысокая
    // подложка, а ВЫБРАННЫЙ отличается рамкой и цветом подписи, не заливкой.
    // Общая кнопка красит выбранное полупрозрачным синим, и ряды «Магазин:»,
    // «Вид:» выглядели чужими против оригинала. Общий тип трогать нельзя —
    // он одинаков во всех вкладках; здесь заводим местный, файл-локальный.
    component RexonChip: Button {
        id: chip

        /// Палитра скина — та же, что у всей страницы.
        property var pal: null

        readonly property color colChipBg: chip.pal ? chip.pal.bg_secondary
                                                    : "#16202d"
        readonly property color colChipHover: chip.pal ? chip.pal.bg_card : "#1e2d3d"
        readonly property color colChipOn: chip.pal ? chip.pal.bg_card_hover
                                                    : "#263d52"
        readonly property color colChipBorder: chip.pal ? chip.pal.border : "#2a4055"
        readonly property color colChipAccent: chip.pal ? chip.pal.accent : "#66c0f4"
        readonly property color colChipText: chip.pal ? chip.pal.text_primary
                                                      : "#ffffff"
        readonly property color colChipTextOff: chip.pal ? chip.pal.text_secondary
                                                         : "#c7d5e0"

        // Поля из `#SrcChip { padding: 5px 10px }`.
        topPadding: 5
        bottomPadding: 5
        leftPadding: 10
        rightPadding: 10

        background: Rectangle {
            radius: 4
            color: chip.checked ? chip.colChipOn
                 : (chip.hovered || chip.down ? chip.colChipHover
                                              : chip.colChipBg)
            // Рамка фокуса важнее прочих: это цепочка управления с геймпада,
            // а не украшение.
            border.color: chip.activeFocus ? chip.colChipAccent
                        : (chip.checked ? chip.colChipAccent : chip.colChipBorder)
            border.width: chip.activeFocus ? 2 : 1
        }

        contentItem: Text {
            text: chip.text
            color: chip.checked ? chip.colChipAccent
                 : (chip.hovered ? chip.colChipText : chip.colChipTextOff)
            font.bold: chip.checked
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
    }

    // ── Ряд «Прослушать:» темы звуков ─────────────────────────────────────
    //
    // Паритет с модулем KDE «Системные звуки»: пять кнопок-значков (звонок,
    // предупреждение, сообщение, батарея, устройство) и «▶» — все пять по
    // очереди. Какие события стоят за кнопками и где их файлы, решает ядро
    // (`sound_preview::PREVIEW_SLOTS`); здесь только раскладка и очередь.
    // Установленная тема играется системой по событию (`theme`), тема из
    // Магазина до установки — файлами из распакованного архива (`sounds`).
    component SoundButtons: Row {
        id: soundRow

        // ⚠️ Встроенный компонент не видит id страницы (у него своя область
        // имён) — объект действий, модель с подписями и палитра приходят
        // свойствами, как у `RexonChip`.
        property var player: null
        property var texts: null
        property var pal: null

        /// Установленная тема — либо пусто.
        property string theme: ""
        /// Звуки темы из Магазина: [{key, event, file}].
        property var storeSounds: []
        property int buttonSize: 34
        /// Ошибка проигрывания для человека (нечем играть, звука нет).
        signal failed(string text)

        readonly property var sounds: {
            if (soundRow.theme === "")
                return soundRow.storeSounds;
            if (!soundRow.player)
                return [];
            try {
                return JSON.parse(soundRow.player.themeSounds(soundRow.theme)).sounds || [];
            } catch (e) {
                return [];
            }
        }
        /// Номер звука, который играет очередь «▶»; -1 — очередь стоит.
        property int queued: -1

        spacing: 4

        function glyph(key) {
            switch (key) {
            case "bell": return "🔔";
            case "warning": return "⚠️";
            case "message": return "💬";
            case "battery": return "🪫";
            case "device": return "🔌";
            }
            return "♪";
        }

        function play(i) {
            const one = soundRow.sounds[i];
            if (!one || one.file === "" || !soundRow.player)
                return;
            const err = soundRow.theme !== ""
                        ? soundRow.player.playThemeSound(soundRow.theme, one.event)
                        : soundRow.player.playFile(one.file);
            if (err !== "") {
                soundRow.queued = -1;
                soundRow.failed(err);
            }
        }

        function playNext(from) {
            for (let i = from; i < soundRow.sounds.length; ++i) {
                if (soundRow.sounds[i].file !== "") {
                    soundRow.queued = i;
                    soundRow.play(i);
                    if (soundRow.queued === i)
                        queueTimer.restart();
                    return;
                }
            }
            soundRow.queued = -1;
        }

        Timer {
            id: queueTimer
            interval: 1500
            repeat: false
            onTriggered: soundRow.playNext(soundRow.queued + 1)
        }

        Repeater {
            model: soundRow.sounds
            delegate: Item {
                id: soundCell
                required property var modelData
                required property int index
                width: soundRow.buttonSize
                height: soundRow.buttonSize

                RexonButton {
                    anchors.fill: parent
                    pal: soundRow.pal
                    leftPadding: 2
                    rightPadding: 2
                    padding: 2
                    enabled: soundCell.modelData.file !== ""
                    highlighted: soundRow.queued === soundCell.index
                    font.pointSize: 13
                    text: soundRow.glyph(soundCell.modelData.key)
                    onClicked: soundRow.play(soundCell.index)
                }

                // Подсказка — и у выключенной кнопки: «звука нет» тоже ответ.
                HoverHandler { id: soundHover }
                ToolTip.text: !soundRow.texts ? ""
                              : (soundCell.modelData.file !== ""
                                 ? soundRow.texts.soundTooltip(soundCell.modelData.event)
                                 : soundRow.texts.soundMissingTooltip)
                ToolTip.visible: soundHover.hovered
                ToolTip.delay: 500
            }
        }

        Item {
            width: soundRow.buttonSize
            height: soundRow.buttonSize

            RexonButton {
                anchors.fill: parent
                pal: soundRow.pal
                leftPadding: 2
                rightPadding: 2
                padding: 2
                enabled: soundRow.sounds.some(function (x) { return x.file !== ""; })
                font.pointSize: 13
                text: soundRow.queued >= 0 ? "■" : "▶"
                onClicked: {
                    if (soundRow.queued >= 0) {
                        queueTimer.stop();
                        soundRow.queued = -1;
                    } else {
                        soundRow.playNext(0);
                    }
                }
            }

            HoverHandler { id: playAllHover }
            ToolTip.text: soundRow.texts ? soundRow.texts.soundPlayAllTooltip : ""
            ToolTip.visible: playAllHover.hovered
            ToolTip.delay: 500
        }
    }

    // ── Пуск ──────────────────────────────────────────────────────────────

    property bool started: false

    // ── «Установить из файла…» ───────────────────────────────────────────
    //
    // Тема, которая уже лежит на диске (скачал сам, в том числе из Windows):
    // файлы или папка через стандартное окно выбора (на Wayland — портал
    // рабочего стола). Что это и куда ставить — ядро (`local_install`), здесь
    // только выбор и передача адресов.

    /// Раздел, который предложено открыть после установки (тема оказалась
    /// другого вида или видна в «Загруженных»), и подпись кнопки.
    property string fileOpenKey: ""
    property string fileOpenText: ""

    // Ушли в другой раздел — прежнее предложение больше не к месту.
    readonly property string fileSection: page.installFileSection
    onFileSectionChanged: root.fileOpenKey = ""

    function installPicked(urls) {
        if (!root.actions || !root.actions.installFiles)
            return;
        const list = [];
        for (let i = 0; i < urls.length; ++i)
            list.push(urls[i].toString());
        if (list.length === 0)
            return;
        const json = JSON.stringify(list);
        root.fileOpenKey = "";
        page.installFileBegin(json);
        root.actions.installFiles(json, page.installFileSection);
    }

    FileDialog {
        id: installFileDialog
        title: page.installFileTitle
        nameFilters: page.installFileFilters
        fileMode: FileDialog.OpenFiles
        onAccepted: root.installPicked(selectedFiles)
    }

    FolderDialog {
        id: installFolderDialog
        title: page.installFileTitle
        onAccepted: root.installPicked([selectedFolder])
    }

    Menu {
        id: installFileMenu
        // Подложка — карточка скина, как у меню окна (`WindowMenu.qml`):
        // подложка стиля Basic белая и палитре не следует.
        background: Rectangle {
            implicitWidth: 220
            color: root.colCard
            border.color: root.colBorder
            border.width: 1
            radius: 4
        }

        PickItem {
            text: page.installFileFilesText
            onTriggered: installFileDialog.open()
        }

        PickItem {
            text: page.installFileFolderText
            onTriggered: installFolderDialog.open()
        }
    }

    // Пункт меню выбора: подсветка и рамка фокуса — как у меню окна.
    component PickItem: MenuItem {
        id: pickEntry
        contentItem: Text {
            text: pickEntry.text
            color: root.colText
            verticalAlignment: Text.AlignVCenter
            leftPadding: 8
        }
        background: Rectangle {
            implicitWidth: 220
            implicitHeight: 40
            radius: 3
            color: pickEntry.highlighted ? root.colCardHover : "transparent"
            border.width: pickEntry.visualFocus ? 2 : 0
            border.color: root.colAccent
        }
    }

    function bootstrap() {
        if (root.started || !root.actions)
            return;
        root.started = true;
        if (root.actions.environment)
            page.setEnvironment(root.actions.environment());
        // Что рисует экран входа и не SteamOS ли это — фоном: проба может
        // звать `systemctl`. Ответ придёт `loginScreenReady`.
        if (root.actions.probeLoginScreen)
            root.actions.probeLoginScreen();
        // ⚠️ Звёздочки — тоже часть пуска. Раньше о них говорил хозяин окна
        // (`_tell_favorites` у Python), и странице с собственным объектом
        // действий сказать было бы некому: первый показ шёл бы с пустыми
        // звёздочками у тем, которые человек уже отметил.
        if (root.actions.favorites)
            page.setFavorites(root.actions.favorites());
        // Очередь установок AUR живёт дольше страницы: открытая заново
        // страница подхватывает идущую установку и итоги прошлых.
        if (root.actions.aurJobs)
            page.setAurJobs(root.actions.aurJobs());
        root.focusFirstControl();
        root.takeSubpage();
    }

    onActionsChanged: root.bootstrap()
    Component.onCompleted: root.bootstrap()

    // Кнопка плитки. Что именно она делает, решает вид плитки: товар
    // магазина ставится и применяется, местная тема — только применяется,
    // пакет AUR собирается помощником.
    // «🗑» на плитке темы с диска: что можно и как снимать — решает хозяин
    // (`removeLocalById`), итог — сигналами `removalPending`/`actionDone`.
    /// Плитка, у которой нажали «🗑» последней: по ней `onRemovalDone`
    /// узнаёт, что открытая карточка — той самой снятой темы.
    property string removingId: ""

    function removeLocal(itemId) {
        root.removingId = itemId;
        if (root.actions && root.actions.removeLocalById)
            root.actions.removeLocalById(itemId);
    }

    /// Глобальная тема Plasma: перед применением — окно «Применить «X»?»
    /// (оформление / расположение), как в Plasma. `true` — окно открыто,
    /// применит оно само.
    function askGlobal(itemId) {
        if (!root.actions || !root.actions.globalApplyQuestion)
            return false;
        const json = root.actions.globalApplyQuestion(itemId);
        if (!json)
            return false;
        globalApplyDialog.show(JSON.parse(json));
        return true;
    }

    function applyLocal(itemId) {
        if (root.askGlobal(itemId))
            return;
        page.actionBegin(itemId, "apply");
        if (root.actions && root.actions.applyLocalById)
            root.actions.applyLocalById(itemId);
    }

    function actOnCard(kind, itemId) {
        if (!root.actions)
            return;
        // Долгое действие начинается ЗДЕСЬ, а не по первому ответу хозяина:
        // иначе между нажатием и первой строкой журнала страница выглядит
        // так, будто кнопка не сработала. Вид действия решает, будет ли у
        // него полоса хода и какой подписью оно назовётся.
        if (kind === "local") {
            root.applyLocal(itemId);
            return;
        }
        // Пакет AUR встаёт в ОБЩУЮ очередь приложения (по одной установке за
        // раз): стадию на плитке и строку сверху рисует снимок очереди
        // (`aurJobsChanged`), а не общий «занят» страницы — пока один пакет
        // собирается, остальные плитки нажимаются и встают «В очереди».
        if (kind === "aur") {
            if (root.actions.installAur)
                root.actions.installAur(itemId);
            return;
        }
        // Как в KDE: «Установить» только ставит, у поставленной темы та же
        // кнопка — «Применить». Что именно сделать, решает ядро.
        // Поставленная глобальная тема — сперва окно выбора.
        if (root.askGlobal(itemId))
            return;
        page.actionBegin(itemId, "install");
        if (root.actions.actOnStoreItem)
            root.actions.actOnStoreItem(itemId);
    }

    // Запрос уходит не мгновенно: перещёлкивание разделов подряд не должно
    // слать в магазин по запросу на каждый щелчок. Паузы — числа ядра
    // (первый заход короче остальных: пустая витрина при открытии вкладки
    // выглядит поломкой).
    Timer {
        id: catalogDebounce
        interval: root.firstLoadDone ? page.catalogDebounceMs : page.firstLoadMs
        repeat: false
        onTriggered: {
            root.firstLoadDone = true;
            root.sendCatalogRequest();
        }
    }

    property bool firstLoadDone: false

    function askCatalog() {
        catalogDebounce.restart();
    }

    function sendCatalogRequest() {
        if (!root.actions || !root.actions.loadCatalog)
            return;
        const request = page.catalogRequest();
        if (request)
            root.actions.loadCatalog(request);
    }

    // Долгая работа отвечает сигналами — синхронного ответа у сети быть не
    // может, страница на нём замерла бы вместе с окном.
    Connections {
        target: root.actions
        ignoreUnknownSignals: true

        function onCatalogReady(payload) {
            page.catalogAnswer(payload);
            root.galleryWarmGeneration = page.catalogGeneration();
            galleryWarm.restart();
            prefetchPause.restart();
        }

        function onPrefetchReady(payload) {
            page.prefetchAnswer(payload);
            prefetchPause.restart();
        }

        function onPreviewDone(job, path) {
            page.previewDone(job, path);
        }

        function onPreviewFailed(job, status) {
            page.previewFailed(job, status);
        }

        function onStatusMessage(text) {
            page.setStatus(text);
        }

        function onDetailReady(payload) {
            page.detailAnswer(payload);
        }

        function onAurDetailReady(payload) {
            page.aurDetailAnswer(payload);
        }

        function onAurTrustReady(payload) {
            page.aurTrustAnswer(payload);
        }

        function onAurPkgbuildReady(payload) {
            pkgbuildPopup.answer(payload);
        }

        function onDescriptionReady(payload) {
            page.descriptionAnswer(payload);
        }

        function onInstallProgress(pct) {
            page.setInstallPercent(pct);
        }

        function onInstallLog(text) {
            page.installLog(text);
        }

        function onAurJobsChanged(payload) {
            page.setAurJobs(payload);
        }

        function onShellPaletteDone(text, ok) {
            shellPaletteBar.busy = false;
            shellPaletteBar.message = text;
            shellPaletteBar.failed = !ok;
            shellPaletteBar.refresh();
            // Кнопка полосы шелла могла уйти — у цветов появляется своя.
            appliedRevertBar.rev++;
            // Схема сменилась мимо плиток: бейдж «✅ Активна» в разделе с
            // диска переезжает только после перечитывания (кеш сканов
            // хозяин уже сбросил).
            if (ok)
                page.reloadLocalFresh();
        }

        // «Установить из файла…»: какой раздел предложить открыть и надо ли
        // перечитать открытый раздел с диска — решило ядро (`local_install`).
        function onLocalInstallDone(payload) {
            let r = {};
            try {
                r = JSON.parse(payload);
            } catch (e) {
                return;
            }
            root.fileOpenKey = r.openKey || "";
            root.fileOpenText = r.openText || "";
            // Раздел с установленным показывает поставленное сам: перечитать.
            if (r.reload)
                page.showInstalledFresh();
        }

        // Тема с диска помечена к удалению: строка «Вернуть» внизу, плитка
        // уходит из сетки (хозяин не отдаёт помеченные в ответах сканов).
        function onRemovalPending(text) {
            removalBar.show(text);
            if (page.screen === "detail")
                page.closeDetail();
            grid.refreshKeepingScroll();
        }

        // Помеченное снято (или системная тема снята помощником): итог — в
        // строку состояния, раздел — заново с диска.
        function onRemovalDone(text) {
            if (text)
                page.setStatus(text);
            // Карточка снятой темы показывала бы то, чего на диске уже нет,
            // с рабочими «Применить» и «Удалить».
            if (page.screen === "detail" && page.detailItemId === root.removingId)
                page.closeDetail();
            root.removingId = "";
            grid.refreshKeepingScroll();
        }

        function onActionDone(text, ok) {
            page.setDetailStatus(text, ok);
            // Применённая схема могла сразу уйти шеллу — «Вернуть как было»
            // должна это увидеть.
            shellPaletteBar.refresh();
            // Удачное «Применить» оставило снимок — кнопка «Вернуть как было».
            appliedRevertBar.rev++;
            // Размер курсора могли сменить и тема, и чип — отмечаем то, что
            // получилось на деле.
            if (cursorSizeRow.visible && root.actions.cursorSize)
                cursorSizeRow.current = root.actions.cursorSize();
        }

        function onGalleryReady(index, path) {
            page.setGalleryImage(index, path);
        }

        function onGalleryFailed(index) {
            page.setGalleryFailed(index);
        }

        function onGalleryFinished(count) {
            page.setGalleryFinished(count);
        }

        function onCommentsReady(payload) {
            page.commentsAnswer(payload);
        }

        function onCommentTranslationsReady(payload) {
            page.commentTranslationsAnswer(payload);
        }

        function onSuggestReady(payload) {
            page.suggestAnswer(payload);
        }

        function onLoginScreenReady(payload) {
            page.setLoginScreen(payload);
        }

        // Страница грузится асинхронно: ответ пробы мог прийти раньше, чем
        // этот `Connections` подключился. Подключившись, забираем готовый.
        Component.onCompleted: {
            if (root.actions && root.actions.loginScreen) {
                const last = root.actions.loginScreen();
                if (last !== "")
                    page.setLoginScreen(last);
            }
        }
    }


    // ── Прослушивание товара Магазина до установки ──────────────────────
    //
    // Состояние живёт у страницы, а не у панели: ответ приходит сигналом
    // через секунды, и панель к тому времени могли закрыть и открыть заново.
    // Ответ чужого товара (открыли другой, пока качался первый) не
    // показывается.
    property string storePreviewId: ""
    property bool storePreviewBusy: false
    property bool storePreviewFailed: false
    property string storePreviewStatus: ""
    property var storePreviewThemes: []
    property real storePreviewDone: 0
    property real storePreviewTotal: 0

    readonly property string openedItemId: page.detailItemId
    onOpenedItemIdChanged: {
        // Другой товар — прежний итог к нему не относится.
        if (root.openedItemId !== root.storePreviewId) {
            root.storePreviewId = "";
            root.storePreviewBusy = false;
            root.storePreviewFailed = false;
            root.storePreviewStatus = "";
            root.storePreviewThemes = [];
        }
    }

    function startStorePreview() {
        if (root.storePreviewBusy)
            return;
        root.storePreviewId = page.detailItemId;
        root.storePreviewBusy = true;
        root.storePreviewFailed = false;
        root.storePreviewThemes = [];
        root.storePreviewDone = 0;
        root.storePreviewTotal = 0;
        root.storePreviewStatus = page.soundStoreProgress(0, 0);
        soundActions.previewStoreTheme(page.detailItemId, page.detailDownloadUrl());
    }

    Connections {
        target: soundActions

        function onStorePreviewProgress(contentId, done, total) {
            if (contentId !== root.storePreviewId || !root.storePreviewBusy)
                return;
            root.storePreviewDone = done;
            root.storePreviewTotal = total;
            root.storePreviewStatus = page.soundStoreProgress(done, total);
        }

        function onStorePreviewReady(payload) {
            let answer = null;
            try {
                answer = JSON.parse(payload);
            } catch (e) {
                return;
            }
            if (answer.contentId !== root.storePreviewId)
                return;
            root.storePreviewBusy = false;
            root.storePreviewFailed = !answer.ok;
            root.storePreviewStatus = answer.ok ? "" : answer.error;
            root.storePreviewThemes = answer.ok ? answer.themes : [];
        }
    }

    // Фокус на первом кадре берёт ПЕРВЫЙ УПРАВЛЯЕМЫЙ ЭЛЕМЕНТ, а не корень
    // страницы: `forceActiveFocus()` на корне делает activeFocusItem
    // безымянным Item, и страница мертва и с клавиатуры, и с геймпада.
    function focusFirstControl() {
        // ⚠️ Первый фокус — СТРОКЕ ПОИСКА, как в оригинале: боковая панель у
        // него `NoFocus` и в обходе не участвует вовсе, а первым по порядку
        // стоит поиск. Здесь фокус отдавался первой строке панели — живой
        // запуск 01.09.2026 показал разницу сразу: у оригинала подсвечена
        // рамкой строка поиска, у нас — чужое место.
        if (searchBox.visible) {
            searchBox.forceActiveFocus();
            return;
        }
        // Поиска в разделе нет — тогда панель разделов: пустого фокуса быть
        // не должно, цепочка нужна для управления с геймпада.
        if (sidebar.count > 0 && sidebar.itemAtIndex(0)) {
            sidebar.itemAtIndex(0).forceActiveFocus();
            return;
        }
        Qt.callLater(root.focusFirstControl);
    }

    onVisibleChanged: {
        if (root.visible) {
            root.focusFirstControl();
            root.takeSubpage();
        }
    }

    // ── Раздел, открытый снаружи ──────────────────────────────────────────
    //
    // Ссылка «Оформление → …» вкладки «Настройки» открывает вкладку через
    // дверь окна (`openSection("appearance", ключ)`), ключ раздела едет
    // подразделом окна (`startSubpage`). Чужой подраздел («compare» у
    // «Рабочих столов») не трогаем: берём только ключ видимого раздела
    // боковой панели.
    function takeSubpage() {
        const w = root.Window.window;
        if (!w || w.startSubpage === undefined || w.startSubpage === "")
            return;
        const key = String(w.startSubpage);
        if (!(page.sidebarRows || []).some(r => r.key === key && r.available))
            return;
        w.startSubpage = "";
        page.selectCategory(key);
    }

    Connections {
        target: root.Window.window
        ignoreUnknownSignals: true
        function onStartSubpageChanged() { root.takeSubpage(); }
    }

    // ── Выпадашка подсказок поиска ────────────────────────────────────────
    //
    // Лежит поверх страницы, а не внутри шапки: список обязан накрывать
    // сетку, а не раздвигать её. Место и размер считает ядро
    // (`suggest_box`) — своих отступов у разметки нет.
    Rectangle {
        id: suggestBox
        visible: page.suggestVisible
        z: 100
        color: root.colSecondary
        border.color: root.colBorder
        border.width: 1
        radius: 4

        readonly property var box: page.suggestGeometry(
            String(Math.round(root.mapFromItem(searchBox, 0, 0).x)),
            String(Math.round(searchBox.width)),
            String(Math.round(root.width)))

        x: suggestBox.box.x
        y: root.mapFromItem(searchBox, 0, searchBox.height).y + 2
        width: suggestBox.box.width
        height: suggestBox.box.height

        ListView {
            id: suggestList
            anchors.fill: parent
            anchors.margins: 2
            clip: true
            model: page.suggestRows

            delegate: ItemDelegate {
                id: suggestRow
                required property var modelData
                width: suggestList.width
                height: page.suggestRowHeight

                background: Rectangle {
                    color: suggestRow.modelData.current || suggestRow.hovered
                           ? root.colCardHover : "transparent"
                }

                contentItem: RowLayout {
                    spacing: 8

                    Rectangle {
                        Layout.preferredWidth: page.suggestThumbWidth
                        Layout.preferredHeight: page.suggestThumbHeight
                        color: root.colCard
                        radius: 2

                        Image {
                            anchors.fill: parent
                            source: suggestRow.modelData.thumb
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            visible: source !== ""
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        Text {
                            Layout.fillWidth: true
                            text: suggestRow.modelData.name
                            color: root.colText
                            font.bold: true
                            elide: Text.ElideRight
                        }

                        Text {
                            Layout.fillWidth: true
                            text: suggestRow.modelData.author
                            color: root.colTextSecondary
                            font.pointSize: 9
                            elide: Text.ElideRight
                        }
                    }
                }

                onClicked: page.suggestPick(suggestRow.modelData.index)
            }
        }
    }

    // ── Крупный просмотр картинки ─────────────────────────────────────────
    //
    // Слой общий на весь продукт (`RexonImageViewer.qml`): тот же, что во
    // вкладках Steam, «Расширения» и «Настройки», — листание стрелками,
    // масштаб, полный экран, Esc. Своего просмотра у страницы нет.

    /// Снимки открытой карточки темы: листание по всем её загруженным
    /// кадрам, открывается на показанном.
    function openZoom() {
        const frames = page.zoomFrames();
        if (!frames || frames.length < 2)
            return;
        // Последним слотом ядро кладёт номер кадра, с которого открывать.
        const at = parseInt(frames[frames.length - 1], 10);
        const items = [];
        for (let i = 0; i < frames.length - 1; i++) {
            const path = String(frames[i]);
            items.push({
                "key": String(i),
                "title": page.detailName,
                "thumb": path,
                "source": path,
                "animatedImage": page.isAnimatedPreview(path)
            });
        }
        zoomPopup.show(items, at);
    }

    /// Картинка плитки витрины — одна, без листания: остальные снимки темы
    /// живут в её карточке.
    function openTileZoom(name, path) {
        if (!path)
            return;
        zoomPopup.show([{
            "key": path,
            "title": name,
            "thumb": path,
            "source": path,
            "animatedImage": page.isAnimatedPreview(path)
        }], 0);
    }

    // «Применить «X»?» глобальной темы Plasma — то же окно, что в Plasma:
    // простой вид — «Оформление» и «Расположение рабочего стола и виджетов»
    // (флажки, когда в теме есть оба; одна часть — строкой), подробный
    // («Выбрать, что применить…») — флажок на каждую часть темы. Тексты,
    // видимость и отметки по умолчанию решает ядро
    // (`themes::global_question_in`); вне Plasma окно сразу подробное, и
    // части, которые сюда не переносятся, выключены с причиной. Фокус при
    // открытии — на «Применить», как в Plasma, Escape — отмена.
    Popup {
        id: globalApplyDialog
        objectName: "globalApplyDialog"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min((parent ? parent.width : 600) - 40, 560)
        modal: true
        focus: true
        padding: 16
        closePolicy: Popup.CloseOnEscape

        property var q: ({})
        property bool appearance: false
        property bool layout: false
        /// Подробный вид и отмеченные в нём части (маска флагов Plasma).
        property bool detailed: false
        property int mask: 0
        readonly property var parts: q.parts || []
        readonly property var layoutParts: parts.filter(p => p.layout)
        readonly property var appearanceParts: parts.filter(p => !p.layout)

        function show(question) {
            q = question;
            detailed = question.detailed === true;
            reset();
            open();
        }

        /// Отметки по умолчанию — и при каждом переключении вида, как в
        /// Plasma (`resetCheckboxes`).
        function reset() {
            appearance = q.appearanceChecked === true;
            layout = q.layoutChecked === true;
            // Щелчок по флажку рвёт привязку `checked`, поэтому отметки
            // ставятся заново.
            globalApplyAppearanceBox.checked = appearance;
            globalApplyLayoutBox.checked = layout;
            let m = 0;
            for (const p of parts) {
                if (p.checked)
                    m |= p.mask;
            }
            mask = m;
        }

        function partOn(p) {
            return (mask & p.mask) !== 0;
        }

        function setPart(p, on) {
            mask = on ? (mask | p.mask) : (mask & ~p.mask);
        }

        /// Отмечено ли расположение рабочего стола (подробный вид).
        readonly property bool desktopLayoutOn: detailed
            && layoutParts.some(p => p.id === "desktopLayout" && partOn(p))
        readonly property bool anyPicked: detailed
            ? parts.some(p => p.enabled && partOn(p))
            : (appearance || layout)

        Overlay.modal: Rectangle { color: root.pal.modal_dim }
        background: Rectangle {
            color: root.pal.bg_secondary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        contentItem: ColumnLayout {
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: globalApplyDialog.q.title || ""
                color: root.colText
                font.bold: true
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                visible: (globalApplyDialog.q.outsideNote || "") !== ""
                text: globalApplyDialog.q.outsideNote || ""
                color: root.colTextSecondary
                wrapMode: Text.WordWrap
            }

            // ── Простой вид ──
            Text {
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && (globalApplyDialog.q.emptyText || "") === ""
                text: globalApplyDialog.q.heading || ""
                color: root.colText
                wrapMode: Text.WordWrap
            }

            RexonCheckBox {
                id: globalApplyAppearanceBox
                objectName: "globalApplyAppearance"
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.appearanceRow === "checkbox"
                pal: root.pal
                text: globalApplyDialog.q.appearanceText || ""
                onToggled: globalApplyDialog.appearance = checked
            }

            Text {
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.appearanceRow === "bullet"
                text: "• " + (globalApplyDialog.q.appearanceText || "")
                color: root.colText
                wrapMode: Text.WordWrap
            }

            RexonCheckBox {
                id: globalApplyLayoutBox
                objectName: "globalApplyLayout"
                // По ширине окна: подпись «(из Breeze — своего в теме нет)»
                // длинная и переносится, а не вылезает за край.
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.layoutRow === "checkbox"
                pal: root.pal
                text: globalApplyDialog.q.layoutText || ""
                onToggled: globalApplyDialog.layout = checked
            }

            Text {
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.layoutRow === "bullet"
                text: "• " + (globalApplyDialog.q.layoutText || "")
                color: root.colText
                wrapMode: Text.WordWrap
            }

            // ── Подробный вид: «Расположение:», затем «Оформление:» ──
            // У темы со всеми частями список длинный: на низком экране он
            // прокручивается, а кнопки окна остаются на месте.
            ScrollView {
                id: globalApplyPartsScroll
                Layout.fillWidth: true
                visible: globalApplyDialog.detailed
                Layout.preferredHeight: Math.min(globalApplyPartsCol.implicitHeight,
                    (globalApplyDialog.parent ? globalApplyDialog.parent.height : 600) - 260)
                clip: true
                contentWidth: availableWidth

                ColumnLayout {
                    id: globalApplyPartsCol
                    width: globalApplyPartsScroll.availableWidth
                    spacing: 10

                    Text {
                        Layout.fillWidth: true
                        visible: globalApplyDialog.detailed && globalApplyDialog.layoutParts.length > 0
                        text: globalApplyDialog.q.layoutHeading || ""
                        color: root.colText
                        font.bold: true
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        model: globalApplyDialog.detailed ? globalApplyDialog.layoutParts : []
                        delegate: globalApplyPartRow
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: globalApplyDialog.detailed && globalApplyDialog.appearanceParts.length > 0
                        text: globalApplyDialog.q.appearanceHeading || ""
                        color: root.colText
                        font.bold: true
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        model: globalApplyDialog.detailed ? globalApplyDialog.appearanceParts : []
                        delegate: globalApplyPartRow
                    }
                }
            }

            // Предупреждение — пока отмечено расположение: оно сносит
            // нынешние панели и виджеты.
            Text {
                Layout.fillWidth: true
                visible: globalApplyDialog.detailed
                         ? globalApplyDialog.desktopLayoutOn
                         : (globalApplyDialog.layout && globalApplyDialog.q.layoutRow === "checkbox")
                text: "⚠ " + (globalApplyDialog.q.layoutWarning || "")
                color: root.colStar
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                visible: (globalApplyDialog.q.emptyText || "") !== ""
                text: globalApplyDialog.q.emptyText || ""
                color: root.colText
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                // «Выбрать, что применить…» / «Меньше параметров…» — слева,
                // как в Plasma; отметки при переключении сбрасываются.
                RexonButton {
                    objectName: "globalApplyMore"
                    pal: root.pal
                    text: globalApplyDialog.detailed
                          ? (globalApplyDialog.q.fewerText || "")
                          : (globalApplyDialog.q.moreText || "")
                    enabled: globalApplyDialog.q.moreEnabled === true
                    onClicked: {
                        globalApplyDialog.detailed = !globalApplyDialog.detailed;
                        globalApplyDialog.reset();
                    }
                }

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: globalApplyYes
                    objectName: "globalApplyYes"
                    pal: root.pal
                    highlighted: true
                    text: globalApplyDialog.q.applyText || ""
                    enabled: globalApplyDialog.anyPicked
                    onClicked: {
                        const id = globalApplyDialog.q.itemId || "";
                        const appearance = globalApplyDialog.appearance;
                        const layout = globalApplyDialog.layout;
                        const parts = globalApplyDialog.detailed ? globalApplyDialog.mask : -1;
                        globalApplyDialog.close();
                        page.actionBegin(id, "apply");
                        if (root.actions && root.actions.applyGlobalById)
                            root.actions.applyGlobalById(id, appearance, layout, parts);
                    }
                }

                RexonButton {
                    objectName: "globalApplyNo"
                    pal: root.pal
                    text: globalApplyDialog.q.cancelText || ""
                    onClicked: globalApplyDialog.close()
                }
            }
        }

        onOpened: globalApplyYes.forceActiveFocus()
    }

    // Флажок части в подробном виде окна «Применить «X»?»; выключенный —
    // с причиной под ним (вне Plasma).
    Component {
        id: globalApplyPartRow

        ColumnLayout {
            id: partRow
            required property var modelData
            Layout.fillWidth: true
            Layout.leftMargin: 8
            spacing: 2

            RexonCheckBox {
                objectName: "globalApplyPart_" + partRow.modelData.id
                Layout.fillWidth: true
                pal: root.pal
                text: partRow.modelData.text
                enabled: partRow.modelData.enabled
                // Непереносимая часть — приглушённо, с причиной под ней.
                opacity: enabled ? 1 : 0.55
                checked: globalApplyDialog.partOn(partRow.modelData)
                onToggled: globalApplyDialog.setPart(partRow.modelData, checked)
            }

            Text {
                Layout.fillWidth: true
                Layout.leftMargin: 28
                visible: partRow.modelData.reason !== ""
                text: partRow.modelData.reason
                color: root.colTextSecondary
                wrapMode: Text.WordWrap
            }
        }
    }

    // Свой редактор цветовой схемы — когда редактора KDE
    // (kcolorschemeeditor) в системе нет.
    ColorSchemeEditor {
        id: schemeEditor
        objectName: "colorSchemeEditor"
        parent: root
        anchors.centerIn: parent
        pal: root.pal
        texts: JSON.parse(page.detailSchemeEdit || "{}")
        onSaveRequested: function(name, roles, base, overwrite) {
            const r = JSON.parse(page.saveSchemeCopy(name, roles, base, overwrite) || "{}");
            schemeEditor.done(r.ok === true, r.message || "");
            if (r.ok === true) {
                page.setStatus(r.message || "");
                // Новая схема — в сетке: туда и вернуться.
                page.closeDetail();
            }
        }

        function start(mode) {
            const r = JSON.parse(page.editDetailScheme(mode) || "{}");
            if (r.error) {
                page.setStatus(r.error);
                return;
            }
            if (r.mode === "own")
                schemeEditor.show(r);
        }
    }

    // PKGBUILD пакета AUR — внутри программы, а не в браузере: что будет
    // скачано и выполнено при сборке.
    Popup {
        id: pkgbuildPopup
        objectName: "aurPkgbuildPopup"
        parent: root
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 960)
        height: root.height - 40
        modal: true
        focus: true
        padding: 16
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        property string itemId: ""
        property string text: ""
        property string error: ""

        function show() {
            itemId = page.detailItemId;
            text = "";
            error = "";
            open();
            if (root.actions && root.actions.loadAurPkgbuild)
                root.actions.loadAurPkgbuild(page.detailItemId, page.aurPackageBase);
        }

        function answer(payload) {
            const v = JSON.parse(payload);
            if (v.itemId !== itemId)
                return;
            text = v.text || "";
            error = v.error || "";
        }

        Overlay.modal: Rectangle { color: root.pal.modal_dim }
        background: Rectangle {
            color: root.colBg
            border.color: root.colBorder
            radius: 4
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Text {
                    Layout.fillWidth: true
                    text: page.aurPkgbuildTitle
                    color: root.colText
                    font.bold: true
                    font.pointSize: 13
                    elide: Text.ElideRight
                }

                RexonButton {
                    pal: root.pal
                    tone: root.toneSecondary
                    text: page.aurOpenText
                    onClicked: {
                        const url = page.detailSiteUrl();
                        if (url && root.actions && root.actions.openUrl)
                            root.actions.openUrl(url);
                    }
                }

                RexonButton {
                    id: pkgbuildClose
                    pal: root.pal
                    text: "❌"
                    onClicked: pkgbuildPopup.close()
                }
            }

            Text {
                Layout.fillWidth: true
                visible: pkgbuildPopup.text === "" || pkgbuildPopup.error !== ""
                text: pkgbuildPopup.error !== "" ? pkgbuildPopup.error : page.aurPkgbuildLoading
                color: pkgbuildPopup.error !== "" ? root.colStar : root.colTextSecondary
                wrapMode: Text.WordWrap
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: pkgbuildPopup.text !== ""
                clip: true

                TextArea {
                    objectName: "aurPkgbuildText"
                    readOnly: true
                    selectByMouse: true
                    text: pkgbuildPopup.text
                    textFormat: TextEdit.PlainText
                    wrapMode: TextEdit.NoWrap
                    color: root.colText
                    font.family: "monospace"
                    font.pointSize: 10
                    background: Rectangle {
                        color: root.colSecondary
                        border.color: root.colBorder
                    }
                }
            }
        }

        onOpened: pkgbuildClose.forceActiveFocus()
    }

    RexonImageViewer {
        id: zoomPopup
        objectName: "imageViewer"
        pal: root.pal
    }

    // ── Подложка ──────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // ── Боковая панель разделов ───────────────────────────────────────
        Rectangle {
            Layout.preferredWidth: page.sidebarWidth
            Layout.fillHeight: true
            color: root.colSecondary

            ListView {
                id: sidebar
                anchors.fill: parent
                clip: true
                // Строки приходят списком целиком, и выбор раздела присылает
                // его заново: прямая связка `model: page.sidebarRows`
                // пересоздавала список и отбрасывала прокрутку в начало —
                // выбрав раздел внизу, человек терял его из виду (приёмка
                // 10.10.2026). Список подменяем сами и возвращаем прокрутку.
                property var rows: page.sidebarRows
                onRowsChanged: {
                    const y = sidebar.contentY;
                    sidebar.model = sidebar.rows;
                    Qt.callLater(function () {
                        const bottom = sidebar.contentHeight - sidebar.height;
                        sidebar.contentY = Math.max(0, Math.min(y, bottom));
                    });
                }
                // Строк два десятка: держим все созданными, чтобы высота
                // содержимого была точной сразу после подмены списка.
                cacheBuffer: 2000
                Component.onCompleted: sidebar.model = sidebar.rows
                // Разделы, которых в выбранном рабочем столе не
                // существует, из панели убраны: оформление, которое в твоём
                // столе не работает, покупателю не нужно ни при каких
                // обстоятельствах.
                //
                // Разделы собраны в группы («Обзор», «Рабочий стол «MATE»»,
                // «Для всех программ»…): заголовок группы приходит в первой
                // видимой строке группы (`groupTitle`), решение — ядра
                // (`categories::category_group`). Обёртка-FocusScope отдаёт
                // фокус самой строке: геймпад и Tab ходят по разделам, а не
                // по заголовкам.
                delegate: FocusScope {
                    id: sidebarBox
                    required property var modelData
                    required property int index
                    width: sidebar.width
                    height: sidebarRow.height + (sidebarHeader.visible ? sidebarHeader.height : 0)
                    visible: sidebarBox.modelData.available

                    Column {
                        id: sidebarHeader
                        visible: (sidebarBox.modelData.groupTitle || "") !== ""
                        width: parent.width
                        topPadding: sidebarBox.index > 0 ? 12 : 4
                        bottomPadding: 4
                        leftPadding: 10
                        rightPadding: 6
                        spacing: 1

                        Text {
                            width: parent.width - parent.leftPadding - parent.rightPadding
                            text: sidebarBox.modelData.groupTitle || ""
                            color: root.colTextSecondary
                            font.pointSize: 9
                            font.weight: Font.Bold
                            font.capitalization: Font.AllUppercase
                            wrapMode: Text.Wrap
                        }
                        Text {
                            width: parent.width - parent.leftPadding - parent.rightPadding
                            visible: text !== ""
                            text: sidebarBox.modelData.groupNote || ""
                            color: root.colTextSecondary
                            font.pointSize: 8
                            wrapMode: Text.Wrap
                        }
                    }

                ItemDelegate {
                    id: sidebarRow
                    readonly property var modelData: sidebarBox.modelData
                    focus: true
                    y: sidebarHeader.visible ? sidebarHeader.height : 0
                    width: sidebar.width
                    // Длинная подпись («Window Decorations (Aurorae)», «Экран
                    // входа (Plasma Login Manager)») переносится на вторую
                    // строку, а строка раздела подрастает под неё: обрезать
                    // название посреди слова — значит спрятать, что это за
                    // раздел.
                    height: sidebarRow.modelData.available
                            ? Math.max(page.sidebarRowHeight,
                                       sidebarLabel.implicitHeight + 12)
                            : 0
                    visible: sidebarRow.modelData.available
                    enabled: sidebarRow.modelData.available
                    padding: 0

                    background: Rectangle {
                        color: sidebarRow.modelData.current ? root.colCardHover
                             : (sidebarRow.hovered ? root.colCard : "transparent")
                        // Левая полоса выбранного раздела — тот же приём,
                        // что в таблице стилей виджетной вкладки.
                        Rectangle {
                            width: 3
                            height: parent.height
                            color: sidebarRow.modelData.current ? root.colAccent
                                                                : "transparent"
                        }
                        // ⚠️ `visualFocus`, а не `activeFocus`. Список
                        // отдаёт фокус первой строке сразу при показе, и по
                        // `activeFocus` на «✅ Активные» висела синяя рамка
                        // ВСЕГДА — даже когда открыт другой раздел (снимки
                        // 01.09.2026). Оригинал такой рамки не рисует вовсе:
                        // боковая панель у него `NoFocus`. Совсем убирать
                        // фокус нельзя — цепочка нужна для управления с
                        // геймпада; `visualFocus` взводится только при
                        // переходе с клавиатуры, а не при раздаче фокуса на
                        // старте.
                        border.color: sidebarRow.visualFocus ? root.colAccent
                                                             : "transparent"
                        border.width: 2
                    }

                    // Значок раздела и подпись — порознь. Перенесённая вторая
                    // строка длинной подписи («Экран входа (Plasma Login
                    // Manager)») обязана начинаться под первой буквой
                    // подписи, а не под значком у края панели (снимки
                    // 01.10.2026: перенос без отступа).
                    contentItem: Item {
                        id: sidebarContent

                        readonly property string full: sidebarRow.modelData.text.trim()
                        readonly property int gap: full.indexOf(" ")
                        // Значок — короткое первое слово без регистра и
                        // цифр (у разделов ядра это всегда эмодзи); обычное
                        // слово так не выглядит.
                        readonly property string head: gap > 0 ? full.substring(0, gap) : ""
                        readonly property bool hasIcon: gap > 0 && gap <= 4
                            && head.toUpperCase() === head.toLowerCase()
                            && !/[0-9]/.test(head)
                        readonly property color tone:
                            sidebarRow.modelData.current ? root.colAccent
                            : (sidebarRow.hovered ? root.colText
                                                  : root.colTextSecondary)

                        Text {
                            id: sidebarIcon
                            visible: sidebarContent.hasIcon
                            // Отступ ядра (два пробела перед значком) — полем.
                            x: 7
                            // Значок стоит на одной линии шрифта с ПЕРВОЙ
                            // строкой подписи, как стоял, пока был с ней
                            // одним текстом.
                            y: sidebarLabel.y + sidebarLabel.baselineOffset - baselineOffset
                            text: sidebarContent.head
                            color: sidebarContent.tone
                            font.pointSize: 11
                            font.weight: Font.DemiBold
                        }

                        Text {
                            id: sidebarLabel
                            x: sidebarContent.hasIcon
                               ? sidebarIcon.x + sidebarIcon.implicitWidth + 5 : 7
                            width: parent.width - x - 6
                            anchors.verticalCenter: parent.verticalCenter
                            text: sidebarContent.hasIcon
                                  ? sidebarContent.full.substring(sidebarContent.gap + 1).trim()
                                  : sidebarContent.full
                            wrapMode: Text.Wrap
                            maximumLineCount: 2
                            // Оригинал (`#Sidebar::item:hover`) при наведении
                            // высветляет и подпись, а не только подложку.
                            color: sidebarContent.tone
                            font.pointSize: 11
                            // `#Sidebar { font-weight: 600 }` — полужирный.
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                    }

                    onClicked: page.selectCategory(sidebarRow.modelData.key)
                }
                }
            }
        }

        // ── Правая часть ──────────────────────────────────────────────────
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // ── Шапка ─────────────────────────────────────────────────────
            //
            // Порядок и раскладка — как в оригинале: название раздела слева,
            // распорка, дальше вправо чипы источника, поиск, рабочий стол,
            // сортировка и «обновить».
            //
            // ⚠️ Здесь стоял `Flow` — ради переноса строки на наименьшей
            // ширине окна. Перенос он давал, но ценой вида: без распорки все
            // органы управления собирались в кучу слева от пустого поля, и
            // шапка переставала быть похожей на оригинал. Мерило вкладки —
            // паритет с оригиналом, а он на `QHBoxLayout` со стретчем и на
            // узком окне точно так же сжимает строку поиска.
            //
            // ⚠️ Узкое окно (1100 точек, снимки 01.10.2026): одна строка не
            // помещалась, её наименьшая ширина распирала всю правую часть, и
            // за правый край уезжали и шапка, и третий столбец плиток. Теперь
            // органы управления разбиты на две группы — «магазин + поиск» и
            // «рабочий стол + сортировка + обновить». Хватает ширины — всё в
            // одну строку, как в оригинале; не хватает — вторая группа
            // переносится на свою строку, прижатая вправо. Поиск при нехватке
            // места сжимается (оригинал на узком окне делает так же).
            GridLayout {
                id: headerRow

                /// Высота органов управления шапки. Оригинал: 34 точки
                /// (замер снимка), поля сверху и снизу — из ядра.
                readonly property int rowHeight: 34
                /// Всё помещается в одну строку. Мерило — желаемые ширины
                /// групп; от числа столбцов они не зависят, круга нет.
                readonly property bool oneRow: width >= headerTitle.implicitWidth
                                                        + searchGroup.implicitWidth
                                                        + toolsGroup.fullWidth
                                                        + 2 * columnSpacing

                Layout.fillWidth: true
                Layout.leftMargin: page.headerMarginH
                Layout.rightMargin: page.headerMarginH
                Layout.topMargin: page.headerMarginV
                Layout.bottomMargin: page.headerMarginV
                columns: oneRow ? 3 : 2
                columnSpacing: 8
                rowSpacing: 8

                Text {
                    id: headerTitle
                    // ⚠️ Именно `sectionTitle`, а не `headerTitle`. Второе —
                    // название вкладки, оно неизменно; оригинал же с первого
                    // мига показывает ВЫБРАННЫЙ раздел со значком («✅
                    // Активные», «📥 Загруженные»), и слово «Оформление»
                    // видно там ровно ноль секунд. Мы держали его всегда.
                    text: page.sectionTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                    Layout.preferredHeight: headerRow.rowHeight
                    verticalAlignment: Text.AlignVCenter
                }

                RowLayout {
                    id: searchGroup
                    Layout.fillWidth: true
                    spacing: 8

                    // Распорка: у оригинала за заголовком стоит
                    // `addStretch(1)`, и весь остальной ряд прижат к правому
                    // краю.
                    Item { Layout.fillWidth: true }

                    // Чипы «Магазин:».
                    Row {
                        spacing: 4
                        visible: page.sourceBarVisible
                        height: headerRow.rowHeight

                        Text {
                            text: page.sourceLabel
                            color: root.colTextSecondary
                            // Оригинал: `#SrcLabel { font-weight: 600 }` —
                            // полужирный, а не жирный. Разница видна глазом.
                            font.weight: Font.DemiBold
                            height: headerRow.rowHeight
                            verticalAlignment: Text.AlignVCenter
                        }

                        Repeater {
                            model: page.sourceChips
                            delegate: RexonChip {
                                required property var modelData
                                pal: root.pal
                                text: modelData.label
                                ToolTip.text: modelData.tooltip
                                ToolTip.visible: hovered && modelData.tooltip !== ""
                                ToolTip.delay: 600
                                checked: modelData.checked
                                onClicked: page.setSource(modelData.key)
                            }
                        }
                    }

                    RexonTextField {
                        id: searchBox
                        pal: root.pal
                        // Рамку полю поиска даёт таблица стилей самой вкладки
                        // (`#SearchBox { border: 1px solid {border} }`), а не
                        // стиль Qt: замер 05.09 — (42,64,85) у оригинала против
                        // (73,83,96) обводки стиля.
                        borderTone: root.pal.border
                        visible: page.searchVisible
                        // ⚠️ Именно `Layout.*`, а не `width`/`height`: размеры
                        // прямых детей раскладки назначает она, и голый `width`
                        // она молча перебивает — строка поиска выходила ýже
                        // заказанных ядром 240 точек.
                        Layout.preferredWidth: page.searchWidth
                        // Шире ядра не растёт; на узком окне сжимается, а не
                        // выталкивает шапку за край.
                        Layout.fillWidth: true
                        Layout.maximumWidth: page.searchWidth
                        Layout.minimumWidth: 140
                        Layout.preferredHeight: headerRow.rowHeight
                        placeholderText: page.searchPlaceholder
                        text: page.query
                        // Поиск идёт по мере набора: обзор диска отбирает сразу,
                        // магазин — после паузы ядра (`searchDelayMs`), Enter —
                        // сразу. Очистка поля сбрасывает запрос СРАЗУ — иначе
                        // запрос залипал на прошлом слове при визуально пустом
                        // поле (дефект оригинала, починен в ядре).
                        rightPadding: searchClear.visible ? searchClear.width + 6 : 10
                        onTextEdited: {
                            page.setQuery(text);
                            if (text.trim() !== "" && page.searchDelayMs > 0)
                                searchDebounce.restart();
                            else
                                searchDebounce.stop();
                            // Посимвольно магазин не дёргаем: до порога длины
                            // выпадашка просто закрыта, дальше — пауза из ядра.
                            suggestDebounce.restart();
                        }
                        onAccepted: {
                            // Выбранная подсказка ведёт прямо в карточку темы;
                            // если не выбрана ни одна — обычный поиск.
                            if (page.suggestActivate())
                                return;
                            searchDebounce.stop();
                            page.setQuery(text);
                            page.submitQuery();
                            // Поиск отправлен — выпадашка своё отработала:
                            // оставшись открытой, она закрывала кнопки шапки.
                            suggestDebounce.stop();
                            page.updateSuggestions("");
                        }
                        onActiveFocusChanged: page.setSearchFocused(activeFocus)

                        // ⚠️ Набор текста РВЁТ связку `text: page.query` — так
                        // устроено поле ввода Qt. Поэтому смену раздела (там
                        // запрос сбрасывается) поле само не заметит и осталось
                        // бы стоять с забытым словом при неотфильтрованной
                        // витрине. Своё свойство связку не теряет и возвращает
                        // поле к тому, что говорит ядро.
                        // Сравнение — по обрезанному тексту: ядро хранит слово
                        // без пробелов по краям, и набранный пробел иначе
                        // стирался бы на ходу.
                        property string coreQuery: page.query
                        onCoreQueryChanged: {
                            if (searchBox.text.trim() !== searchBox.coreQuery) {
                                searchDebounce.stop();
                                searchBox.text = searchBox.coreQuery;
                            }
                        }

                        Keys.onDownPressed: page.suggestMove("down")
                        Keys.onUpPressed: page.suggestMove("up")
                        Keys.onEscapePressed: page.setSearchFocused(false)

                        // Пауза набора перед поиском в магазине. Слово к этому
                        // моменту уже в ядре (`setQuery`), остаётся применить.
                        Timer {
                            id: searchDebounce
                            interval: Math.max(1, page.searchDelayMs)
                            repeat: false
                            onTriggered: {
                                if (searchBox.text.trim() === page.query)
                                    page.submitQuery();
                            }
                        }

                        // ✕ «Очистить»: заметная, пока в поле что-то есть, —
                        // забытое слово в поле не должно прятать раздел.
                        RexonButton {
                            id: searchClear
                            pal: root.pal
                            visible: searchBox.text !== ""
                            text: "✕"
                            width: height
                            height: searchBox.height - 8
                            anchors.right: parent.right
                            anchors.rightMargin: 4
                            anchors.verticalCenter: parent.verticalCenter
                            leftPadding: 0
                            rightPadding: 0
                            focusPolicy: Qt.NoFocus
                            ToolTip.text: page.searchClearTooltip
                            ToolTip.visible: hovered
                            ToolTip.delay: 600
                            onClicked: {
                                searchDebounce.stop();
                                suggestDebounce.stop();
                                searchBox.text = "";
                                page.setQuery("");
                                page.updateSuggestions("");
                            }
                        }

                        Timer {
                            id: suggestDebounce
                            interval: page.suggestDebounceMs
                            repeat: false
                            onTriggered: page.updateSuggestions(searchBox.text)
                        }
                    }
                }

                // Органы управления — потоком справа налево: не помещаются в
                // строку — переносятся, а не выталкивают шапку и сетку плиток
                // за правый край (окно 1100 точек: «Магазин:» и третий
                // столбец уезжали за край). Порядок детей
                // обратный порядку на экране — так у потока справа налево.
                Flow {
                    id: toolsGroup
                    /// Ширина всех органов в одну строку — мерило `oneRow`.
                    readonly property real fullWidth: {
                        let w = 0, n = 0;
                        for (let i = 0; i < children.length; ++i) {
                            const c = children[i];
                            if (!c.visible)
                                continue;
                            w += c.implicitWidth;
                            n += 1;
                        }
                        return w + Math.max(0, n - 1) * spacing;
                    }
                    // На своей строке группа занимает оба столбца.
                    Layout.columnSpan: headerRow.oneRow ? 1 : 2
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.preferredWidth: fullWidth
                    Layout.preferredHeight: implicitHeight
                    layoutDirection: Qt.RightToLeft
                    spacing: 8

                    RexonButton {
                        pal: root.pal
                        tone: root.toneNav
                        height: headerRow.rowHeight
                        text: page.refreshText
                        ToolTip.text: page.refreshTooltip
                        ToolTip.visible: hovered
                        ToolTip.delay: 600
                        onClicked: page.refresh()
                    }

                    RexonButton {
                        id: installFileButton
                        objectName: "installFileButton"
                        pal: root.pal
                        tone: root.toneNav
                        visible: page.installFileVisible
                        enabled: !page.actionBusy
                        height: headerRow.rowHeight
                        text: page.installFileText
                        ToolTip.text: page.installFileTooltip
                        // Пока открыто меню «Выбрать файлы / папку», подсказки
                        // нет: она ложилась поверх меню, и кнопка выглядела
                        // неработающей (приёмка 10.10.2026).
                        ToolTip.visible: hovered && !installFileMenu.opened
                        ToolTip.delay: 600
                        onClicked: {
                            installFileMenu.parent = installFileButton;
                            installFileMenu.x = 0;
                            installFileMenu.y = installFileButton.height;
                            installFileMenu.open();
                        }
                    }

                    RexonComboBox {
                        id: sortCombo
                        pal: root.pal
                        tone: root.toneCombo
                        visible: page.sortVisible
                        height: headerRow.rowHeight
                        model: page.sorts
                        textRole: "label"
                        valueRole: "key"
                        selectedValue: page.sortKey
                        onActivated: page.setSort(currentValue)
                    }

                    Row {
                        spacing: 6
                        height: headerRow.rowHeight
                        visible: deCombo.count > 0

                        Text {
                            text: page.deLabel
                            color: root.colTextSecondary
                            font.weight: Font.DemiBold   // `#SrcLabel`, вес 600
                            height: headerRow.rowHeight
                            verticalAlignment: Text.AlignVCenter
                        }

                        RexonComboBox {
                            id: deCombo
                            pal: root.pal
                            tone: root.toneCombo
                            height: headerRow.rowHeight
                            model: page.deChoices
                            textRole: "name"
                            valueRole: "id"
                            ToolTip.text: page.deTooltip
                            ToolTip.visible: hovered
                            ToolTip.delay: 600
                            selectedValue: page.de
                            onActivated: {
                                // Значение берётся ДО setDe: тот пересобирает
                                // список (подпись «выбрано вручную»).
                                const picked = currentValue;
                                page.setDe(picked);
                                // Выбор руками живёт между запусками: без этого
                                // каждый заход снова показывал стол, определённый
                                // по сеансу. «Определять автоматически» пишет
                                // пустой выбор — дальше каталог идёт за сеансом.
                                // Второй довод — пустой: какой стол запущен
                                // СЕЙЧАС, знает хозяин, он же и запишет.
                                if (root.actions && root.actions.remember)
                                    root.actions.remember(picked === "auto" ? "" : picked, "");
                            }
                        }
                    }
                }
            }

            // ── Под-чипы и строка состояния ───────────────────────────────
            Flow {
                Layout.fillWidth: true
                Layout.leftMargin: page.headerMarginH
                Layout.rightMargin: page.headerMarginH
                Layout.bottomMargin: 4
                spacing: 6

                Row {
                    id: submodeRow
                    spacing: 4
                    visible: page.styleBarVisible || page.lockBarVisible || page.toneBarVisible

                    Text {
                        text: page.submodeLabel
                        color: root.colTextSecondary
                        font.weight: Font.DemiBold   // `#SrcLabel`, вес 600
                        height: headerRow.rowHeight
                        verticalAlignment: Text.AlignVCenter
                    }

                    Repeater {
                        model: page.styleBarVisible ? page.styleChips : []
                        delegate: RexonChip {
                            required property var modelData
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setStyleSub(modelData.key)
                        }
                    }

                    Repeater {
                        model: page.lockBarVisible ? page.lockChips : []
                        delegate: RexonChip {
                            required property var modelData
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setLockMode(modelData.key)
                        }
                    }

                    // «Все / Светлые / Тёмные» у цветовых схем и стилей Plasma
                    // (у стилей ещё «С поддержкой цветовых схем»), как у KDE.
                    Repeater {
                        model: page.toneBarVisible ? page.toneChips : []
                        delegate: RexonChip {
                            required property var modelData
                            objectName: "toneChip_" + modelData.key
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setToneFilter(modelData.key)
                        }
                    }
                }

                // Размер курсора — в разделе «Курсоры». Ложится на текущую
                // тему; куда он доходит в каждом окружении, решает ядро
                // (`themes::apply_cursor_plan_sized`), здесь только выбор.
                Row {
                    id: cursorSizeRow
                    spacing: 4
                    visible: page.cursorSizeBarVisible

                    // Отмеченный размер — ответ хозяина при входе в раздел.
                    property int current: 0
                    onVisibleChanged: {
                        if (visible && root.actions && root.actions.cursorSize)
                            current = root.actions.cursorSize();
                    }

                    Text {
                        text: page.cursorSizeLabel
                        color: root.colTextSecondary
                        font.weight: Font.DemiBold   // `#SrcLabel`, вес 600
                        height: headerRow.rowHeight
                        verticalAlignment: Text.AlignVCenter
                    }

                    Repeater {
                        model: page.cursorSizeChips
                        delegate: RexonChip {
                            required property var modelData
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered
                            ToolTip.delay: 600
                            checked: parseInt(modelData.key, 10) === cursorSizeRow.current
                            onClicked: {
                                if (!root.actions || !root.actions.applyCursorSize)
                                    return;
                                cursorSizeRow.current = parseInt(modelData.key, 10);
                                root.actions.applyCursorSize(cursorSizeRow.current);
                            }
                        }
                    }
                }

                Text {
                    text: page.statusText
                    color: root.colTextSecondary
                    // Оригинал: `#StatusBar { font-size: 10pt;
                    // font-weight: 600 }`.
                    font.pointSize: page.cardMetaPointSize
                    font.weight: Font.DemiBold
                    // ⚠️ Высота — по тексту, пока рядом нет под-чипов. Здесь
                    // стояли безусловные 40 точек, и в разделах БЕЗ чипов
                    // (а их большинство) строка состояния распирала полосу
                    // вдвое против оригинала: вся сетка плиток съезжала вниз
                    // на 20 точек. Когда чипы есть, тянемся до их высоты —
                    // иначе подпись прилипнет к верху вместо середины, как в
                    // оригинале.
                    height: Math.max(implicitHeight,
                                     submodeRow.visible ? submodeRow.height
                                   : cursorSizeRow.visible ? cursorSizeRow.height
                                                           : 0)
                    verticalAlignment: Text.AlignVCenter
                    // Длинная причина отказа («X конфликтует с Y…», путь к
                    // журналу) переносится внутри полосы, а не уходит за
                    // правый край: ширина — не больше самой полосы.
                    width: Math.min(implicitWidth, parent.width)
                    wrapMode: Text.WordWrap
                }

                // После «Установить из файла…»: тема оказалась другого вида
                // («это курсоры») или видна в «Загруженных» — открыть тот
                // раздел одним нажатием.
                RexonButton {
                    objectName: "installFileOpenButton"
                    pal: root.pal
                    tone: root.toneNav
                    visible: root.fileOpenKey !== "" && root.fileOpenText !== ""
                    height: headerRow.rowHeight
                    text: root.fileOpenText
                    onClicked: {
                        const key = root.fileOpenKey;
                        root.fileOpenKey = "";
                        page.selectCategory(key);
                    }
                }
            }

            // ── Оговорка раздела: «работает, но при условии» ───────────────
            //
            // Раздел при этом остаётся на месте: прятать работающее вместо
            // того, чтобы назвать условие, — не наш способ (правило #13).
            Text {
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 6
                text: page.caveatText
                visible: text !== ""
                color: root.colTextSecondary
                font.pointSize: 9
                wrapMode: Text.WordWrap
            }

            // ── Ряд раздела «Виджеты Conky» ───────────────────────────────
            //
            // Чип «Запущенные» — в сетке только виджеты с процессом сейчас;
            // «Остановить все» — все conky на машине, и чужие тоже, после
            // вопроса со списком (ядро: `conky_procs`). Подписи — ядро.
            RowLayout {
                id: conkyToolsRow
                objectName: "conkyToolsRow"
                readonly property bool active: page.installFileSection === "conky"
                readonly property var texts: {
                    try {
                        return root.actions && root.actions.conkyTexts
                               ? JSON.parse(root.actions.conkyTexts()) : ({});
                    } catch (e) {
                        return ({});
                    }
                }
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: active && page.screen === "grid"
                spacing: 8

                onActiveChanged: {
                    runningChip.checked = false;
                    if (active && root.actions && root.actions.setConkyPreviewScreen)
                        root.actions.setConkyPreviewScreen(
                            Math.round(Screen.width * Screen.devicePixelRatio),
                            Math.round(Screen.height * Screen.devicePixelRatio),
                            Math.round(96 * Screen.devicePixelRatio));
                    if (active && root.actions && root.actions.watchConkyDisk)
                        root.actions.watchConkyDisk();
                }

                RexonChip {
                    id: runningChip
                    objectName: "conkyRunningChip"
                    pal: root.pal
                    checkable: true
                    text: conkyToolsRow.texts.running_chip || ""
                    ToolTip.text: conkyToolsRow.texts.running_chip_tooltip || ""
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onToggled: page.setRunningOnly(checked)
                }

                Item { Layout.fillWidth: true }

                RexonButton {
                    objectName: "conkyStopAllButton"
                    pal: root.pal
                    tone: root.toneSecondary
                    enabled: !page.actionBusy
                    text: conkyToolsRow.texts.stop_all || ""
                    ToolTip.text: conkyToolsRow.texts.stop_all_tooltip || ""
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onClicked: {
                        if (root.actions && root.actions.stopAllConky
                                && root.actions.stopAllConky())
                            grid.refreshKeepingScroll();
                    }
                }

                Connections {
                    target: root.actions
                    ignoreUnknownSignals: true
                    // Новый виджет распаковали в ~/.conky (или убрали) —
                    // плитки «Найдено на диске» обновляются сами.
                    function onConkyDiskChanged() {
                        if (conkyToolsRow.active && page.screen === "grid")
                            page.showInstalledFresh();
                    }
                    // Сняты новые снимки виджетов для плиток — перечитать,
                    // не сбивая прокрутку.
                    function onConkyPreviewsReady() {
                        if (conkyToolsRow.active && page.screen === "grid")
                            grid.refreshKeepingScroll();
                    }
                }
            }

            // ── Лента «На столе сейчас» ───────────────────────────────────
            //
            // Запущенные виджеты Conky миниатюрами, как гаджеты на столе
            // Windows: щелчок — карточка виджета (настройки, части,
            // журнал), крестик — убрать со стола (виджет остаётся
            // установленным). Список и подписи — ядро (`conky_procs::ribbon`).
            ColumnLayout {
                id: conkyRibbon
                objectName: "conkyRibbon"
                property var items: []
                function reload() {
                    try {
                        conkyRibbon.items = root.actions && root.actions.conkyRibbon
                                            ? JSON.parse(root.actions.conkyRibbon()) : [];
                    } catch (e) {
                        conkyRibbon.items = [];
                    }
                }
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: conkyToolsRow.visible && conkyRibbon.items.length > 0
                spacing: 4
                onVisibleChanged: if (conkyToolsRow.visible) conkyRibbon.reload()
                Component.onCompleted: conkyRibbon.reload()

                Text {
                    text: conkyToolsRow.texts.ribbon_title || ""
                    color: root.colText
                    font.bold: true
                }
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: conkyRibbon.items
                        delegate: Rectangle {
                            id: gadget
                            required property var modelData
                            objectName: "conkyGadget"
                            width: 132
                            height: 104
                            radius: 4
                            color: gadgetHover.hovered ? root.colCardHover : root.colCard
                            HoverHandler { id: gadgetHover }
                            ToolTip.text: conkyToolsRow.texts.ribbon_open_tooltip || ""
                            ToolTip.visible: gadgetHover.hovered
                            ToolTip.delay: 600
                            TapHandler { onTapped: page.openDetail(gadget.modelData.item_id) }
                            Image {
                                id: gadgetPicture
                                anchors.fill: parent
                                anchors.margins: 6
                                anchors.bottomMargin: 24
                                fillMode: Image.PreserveAspectFit
                                asynchronous: true
                                sourceSize.width: 240
                                source: gadget.modelData.picture !== ""
                                        ? "file://" + encodeURI(gadget.modelData.picture) : ""
                            }
                            Text {
                                anchors.centerIn: gadgetPicture
                                visible: gadgetPicture.status !== Image.Ready
                                text: "📊"
                                font.pixelSize: 32
                            }
                            Text {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.margins: 6
                                text: gadget.modelData.title
                                color: root.colText
                                elide: Text.ElideRight
                                horizontalAlignment: Text.AlignHCenter
                            }
                            RexonButton {
                                objectName: "conkyGadgetStop"
                                anchors.top: parent.top
                                anchors.right: parent.right
                                anchors.margins: 2
                                implicitWidth: 26
                                implicitHeight: 26
                                pal: root.pal
                                enabled: !page.actionBusy
                                text: "✕"
                                ToolTip.text: conkyToolsRow.texts.ribbon_stop_tooltip || ""
                                ToolTip.visible: hovered
                                ToolTip.delay: 400
                                onClicked: {
                                    if (root.actions && root.actions.stopConkyRibbonItem
                                            && root.actions.stopConkyRibbonItem(gadget.modelData.item_id))
                                        grid.refreshKeepingScroll();
                                }
                            }
                        }
                    }
                }
                Connections {
                    target: root.actions
                    ignoreUnknownSignals: true
                    function onActionDone(text, ok) {
                        if (conkyToolsRow.visible)
                            conkyRibbon.reload();
                    }
                }
            }

            // ── Наборы виджетов Conky ─────────────────────────────────────
            //
            // Несколько виджетов и обои одной кнопкой; «Сохранить запущенные
            // как набор». Наборы из файлов .cmtheme пакетов тем видны сразу.
            // Решения и подписи — ядро (`conky_sets`).
            ConkySets {
                id: conkySets
                objectName: "conkySets"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: conkyToolsRow.visible
                pal: root.pal
                actions: root.actions
                busy: page.actionBusy
                onVisibleChanged: if (visible) conkySets.reload()
                onChanged: grid.refreshKeepingScroll()
            }

            // ── Системные звуки выключены ─────────────────────────────────
            //
            // В разделе «Звуки» при выключенных звуках событий любая тема
            // немая — говорим это сразу и даём включить одной кнопкой.
            // Решение и подписи — ядро (`ap::event_sounds_banner`).
            Rectangle {
                id: eventSoundsBar
                objectName: "eventSoundsBar"

                property int rev: 0
                property string message: ""
                readonly property var info: {
                    // Пересчёт при смене раздела и рабочего стола.
                    page.sectionTitle; page.caveatText; eventSoundsBar.rev;
                    try {
                        return JSON.parse(page.eventSoundsBanner());
                    } catch (e) {
                        return ({});
                    }
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: eventSoundsBar.info.visible === true || eventSoundsBar.message !== ""
                implicitHeight: eventSoundsRow.implicitHeight + 20
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                RowLayout {
                    id: eventSoundsRow
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: 10
                    spacing: 10

                    Text {
                        Layout.fillWidth: true
                        text: eventSoundsBar.info.visible === true
                              ? "🔇 " + (eventSoundsBar.info.text || "")
                              : eventSoundsBar.message
                        color: root.colText
                        font.pointSize: 10
                        wrapMode: Text.WordWrap
                    }

                    RexonButton {
                        objectName: "eventSoundsOnButton"
                        pal: root.pal
                        visible: eventSoundsBar.info.visible === true
                        text: eventSoundsBar.info.button || ""
                        onClicked: {
                            eventSoundsBar.message = page.enableEventSounds();
                            eventSoundsBar.rev++;
                        }
                    }
                }

                Connections {
                    target: page
                    function onSidebarChanged() { eventSoundsBar.message = ""; }
                    function onDeChanged() { eventSoundsBar.rev++; }
                }
            }

            // ── Палитра оболочки ──────────────────────────────────────────
            //
            // Полоса видна ТОЛЬКО при живом чужом шелле (Noctalia,
            // DankMaterialShell): он сам ставит приложениям тёмный/светлый
            // режим, и спор с ним надо назвать, а не прятать (правило #13).
            // Решение, текст и подписи — ядро (`shell_palette::PaletteBanner`
            // через `actions.shellPalette()`); здесь только раскладка и три
            // кнопки. Шелл могут запустить и погасить при открытом окне —
            // отсюда редкий таймер.
            Rectangle {
                id: shellPaletteBar
                objectName: "shellPaletteBar"

                property var info: ({})
                property string message: ""
                property bool failed: false
                property bool busy: false

                function refresh() {
                    if (!root.actions || !root.actions.shellPalette)
                        return;
                    try {
                        shellPaletteBar.info = JSON.parse(root.actions.shellPalette());
                    } catch (e) {
                        shellPaletteBar.info = ({});
                    }
                }

                function act(name) {
                    if (!root.actions || !root.actions[name] || shellPaletteBar.busy)
                        return;
                    shellPaletteBar.busy = true;
                    shellPaletteBar.message = "";
                    root.actions[name]();
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: shellPaletteBar.info.visible === true
                implicitHeight: shellPaletteColumn.implicitHeight + 20
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                Timer {
                    interval: 15000
                    repeat: true
                    running: root.started && root.visible
                    triggeredOnStart: true
                    onTriggered: shellPaletteBar.refresh()
                }

                ColumnLayout {
                    id: shellPaletteColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 10
                    spacing: 8

                    Text {
                        Layout.fillWidth: true
                        text: "🎨 " + (shellPaletteBar.info.text || "")
                        color: root.colText
                        font.pointSize: 10
                        wrapMode: Text.WordWrap
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: 6

                        Text {
                            text: shellPaletteBar.info.lead_caption || ""
                            color: root.colTextSecondary
                            font.weight: Font.DemiBold
                            height: 32
                            verticalAlignment: Text.AlignVCenter
                        }

                        Repeater {
                            model: [
                                { key: "rexon", label: shellPaletteBar.info.lead_rexon_label || "" },
                                { key: "shell", label: shellPaletteBar.info.lead_shell_label || "" }
                            ]
                            delegate: RexonChip {
                                required property var modelData
                                pal: root.pal
                                text: modelData.label
                                checked: shellPaletteBar.info.lead === modelData.key
                                onClicked: {
                                    if (root.actions && root.actions.setPaletteLead
                                            && root.actions.setPaletteLead(modelData.key))
                                        shellPaletteBar.refresh();
                                }
                            }
                        }

                        Item { width: 12; height: 1 }

                        RexonButton {
                            pal: root.pal
                            text: shellPaletteBar.info.push_label || ""
                            enabled: shellPaletteBar.info.can_push === true && !shellPaletteBar.busy
                            onClicked: shellPaletteBar.act("pushShellPalette")
                        }

                        RexonButton {
                            pal: root.pal
                            text: shellPaletteBar.info.pull_label || ""
                            enabled: !shellPaletteBar.busy
                            onClicked: shellPaletteBar.act("pullShellPalette")
                        }

                        RexonButton {
                            pal: root.pal
                            text: shellPaletteBar.info.revert_label || ""
                            visible: shellPaletteBar.info.can_revert === true
                            enabled: !shellPaletteBar.busy
                            onClicked: shellPaletteBar.act("revertShellPalette")
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: shellPaletteBar.message
                        color: shellPaletteBar.failed
                               ? (root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                    : root.colAccent)
                               : root.colTextSecondary
                        font.pointSize: 9
                        wrapMode: Text.WordWrap
                    }
                }
            }

            // ── Вернуть как было ──────────────────────────────────────────
            //
            // Удачное «Применить» оставило снимок того, что было до него, —
            // в разделе одна кнопка вернуть это. У цветов при видимой кнопке
            // полосы «Палитра оболочки» своей нет (та возвращает и шелл, и
            // прежнюю схему). Решение и подписи — ядро
            // (`ap::applied_revert_banner`, возврат `revert_applied_section`).
            Rectangle {
                id: appliedRevertBar
                objectName: "appliedRevertBar"

                property int rev: 0
                property string message: ""
                property bool failed: false
                // Раздел, к которому относится итог: после удачного возврата
                // раздел перечитывается (тот же сигнал, что у смены раздела),
                // и итог должен остаться — уходит он только в другом разделе.
                property string messageSection: ""
                readonly property bool showMessage: appliedRevertBar.message !== ""
                        && appliedRevertBar.info.section === appliedRevertBar.messageSection
                readonly property var info: {
                    // Пересчёт при смене раздела и после нажатий.
                    page.sectionTitle; page.caveatText; appliedRevertBar.rev;
                    try {
                        return JSON.parse(page.appliedRevertBanner());
                    } catch (e) {
                        return ({});
                    }
                }

                function revert() {
                    if (!root.actions || !root.actions.revertApplied)
                        return;
                    let out = ({});
                    try {
                        out = JSON.parse(root.actions.revertApplied(appliedRevertBar.info.section || ""));
                    } catch (e) {
                        out = ({});
                    }
                    appliedRevertBar.messageSection = appliedRevertBar.info.section || "";
                    appliedRevertBar.message = out.text || "";
                    appliedRevertBar.failed = out.ok !== true;
                    appliedRevertBar.rev++;
                    if (out.ok === true)
                        page.reloadLocalFresh();
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: appliedRevertBar.info.visible === true || appliedRevertBar.showMessage
                implicitHeight: appliedRevertColumn.implicitHeight + 20
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                ColumnLayout {
                    id: appliedRevertColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 10
                    spacing: 8

                    RowLayout {
                        Layout.fillWidth: true
                        visible: appliedRevertBar.info.visible === true
                        spacing: 10

                        Text {
                            Layout.fillWidth: true
                            text: "↩️ " + (appliedRevertBar.info.text || "")
                            color: root.colText
                            font.pointSize: 10
                            wrapMode: Text.WordWrap
                        }

                        RexonButton {
                            objectName: "appliedRevertButton"
                            pal: root.pal
                            text: appliedRevertBar.info.button || ""
                            enabled: !page.actionBusy
                            onClicked: appliedRevertBar.revert()
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: appliedRevertBar.showMessage
                        text: appliedRevertBar.message
                        color: appliedRevertBar.failed
                               ? (root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                    : root.colAccent)
                               : root.colTextSecondary
                        font.pointSize: 9
                        wrapMode: Text.WordWrap
                    }
                }
            }

            // ── Менеджер входа ────────────────────────────────────────────
            //
            // В разделе экрана входа (полка тем SDDM и панель Plasma Login
            // Manager): установленные экраны входа, основной и «Сделать
            // основным». Данные — проба экрана входа, тексты — ядро.
            LoginManagerSection {
                id: loginManagerSection
                objectName: "loginManagerSection"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                // Здесь блок стоит только над объяснением (`note`). Над
                // сеткой тем он — шапка самой сетки (`grid.header`) и
                // прокручивается вместе с плитками, при Plasma Login
                // Manager — в общем потоке с объяснением (`plmView`).
                visible: page.loginSection && page.screen === "note"
                         && (loginManagerSection.managers.length > 0 || loginManagerSection.sessionOffer !== null)
                pal: root.pal
                sty: page.sectionStyles
                actions: root.actions
            }

            // ── Цвет акцента ──────────────────────────────────────────────
            //
            // Над сеткой «Цветовых схем», как в KDE «Цвета». Где он стоит и
            // доступен ли здесь — решает ядро (`accent`).
            AccentSection {
                id: accentSection
                objectName: "accentSection"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: page.installFileSection === (accentSection.st.section || "-")
                         && accentSection.st.available === true
                         && (page.screen === "grid" || (page.screen === "note" && accentSection.st.on_note === true))
                pal: root.pal
                sty: page.sectionStyles
            }

            // ── Светлое и тёмное ──────────────────────────────────────────
            //
            // Над «Глобальными темами» (Plasma) и «Цветовыми схемами»: пары
            // тем и смена по расписанию. Где стоит и доступен ли — ядро
            // (`day_night`).
            DayNightSection {
                id: dayNightSection
                objectName: "dayNightSection"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: (dayNightSection.st.sections || []).indexOf(page.installFileSection) >= 0
                         && dayNightSection.st.available === true
                         && page.screen === "grid"
                pal: root.pal
                sty: page.sectionStyles
            }

            // ── «Сохранить текущее оформление…» ───────────────────────────
            //
            // Над «Глобальными темами» во всех окружениях: то, что стоит
            // сейчас, — своей глобальной темой (ядро `global_save`).
            // Сохранили — сетка перечитывает диск, плитка новой темы сразу в ней.
            GlobalSaveSection {
                objectName: "globalSaveSection"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: page.installFileSection === "global" && page.screen === "grid"
                pal: root.pal
                sty: page.sectionStyles
                onSaved: function(message) {
                    page.setStatus(message);
                    page.refresh();
                }
            }

            // ── Сетка плиток либо объяснение ──────────────────────────────
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true

                // Экран входа рисует Plasma Login Manager: тем у него нет,
                // полки нет — объяснение и дорога в его настройки. Все
                // подписи — ядра (`plasma_login_panel`).
                //
                // Блок «Менеджер входа» и объяснение — один прокручиваемый
                // поток: в невысоком окне (1100×750) объяснение, отцентрованное
                // в остатке высоты, вылезало вверх поверх списка, а страница
                // не прокручивалась. Теперь при нехватке высоты растёт
                // прокрутка, при запасе объяснение по-прежнему по центру.
                ScrollView {
                    id: plmView
                    objectName: "plasmaLoginView"
                    anchors.fill: parent
                    visible: page.screen === "plasmalogin"
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: plmView.availableWidth
                        height: Math.max(implicitHeight, plmView.availableHeight)
                        spacing: 0

                        LoginManagerSection {
                            id: plmLoginManagerSection
                            objectName: "plasmaLoginManagerSection"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.bottomMargin: 8
                            visible: page.loginSection && (plmLoginManagerSection.managers.length > 0
                                                           || plmLoginManagerSection.sessionOffer !== null
                                                           || plmLoginManagerSection.greeter !== null)
                            pal: root.pal
                            sty: page.sectionStyles
                            actions: root.actions
                        }

                        Item {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Layout.minimumHeight: plmColumn.implicitHeight + 32
                            implicitHeight: plmColumn.implicitHeight + 32

                            ColumnLayout {
                                id: plmColumn
                                objectName: "plasmaLoginColumn"
                                anchors.centerIn: parent
                                width: Math.min(parent.width - 64, 720)
                                spacing: 14

                                Text {
                                    Layout.fillWidth: true
                                    text: page.plmTitle
                                    color: root.colText
                                    font.pointSize: 16
                                    font.bold: true
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.WordWrap
                                }

                                Text {
                                    Layout.fillWidth: true
                                    text: page.plmBody
                                    color: root.colTextSecondary
                                    font.pointSize: 14
                                    lineHeight: 1.4
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.WordWrap
                                }

                                RexonButton {
                                    id: plmOpenButton
                                    Layout.alignment: Qt.AlignHCenter
                                    Layout.topMargin: 6
                                    visible: page.plmOpenVisible
                                    pal: root.pal
                                    font.bold: true
                                    text: page.plmOpenLabel
                                    onClicked: {
                                        const ok = root.actions && root.actions.openLoginScreenSettings
                                                   ? root.actions.openLoginScreenSettings() : false;
                                        if (!ok)
                                            page.loginSettingsFailed();
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    Layout.topMargin: 6
                                    text: page.plmStoreHint
                                    color: root.colTextSecondary
                                    font.pointSize: 11
                                    font.italic: true
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.WordWrap
                                }
                            }
                        }
                    }
                }

                // «Панель MATE»: готовые раскладки mate-panel. Раздел
                // самостоятельный (`MatePanelSection`, своё окно
                // подтверждения); ответ на «Применить» — строкой под ним.
                ScrollView {
                    id: matePanelView
                    anchors.fill: parent
                    visible: page.screen === "matepanel"
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: matePanelView.availableWidth
                        spacing: 10

                        MatePanelSection {
                            Layout.fillWidth: true
                            Layout.margins: 16
                            pal: root.pal
                            sty: page.sectionStyles
                            onMessage: text => matePanelMessage.text = text
                        }

                        Text {
                            id: matePanelMessage
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: text !== ""
                            color: root.colTextSecondary
                            font.pointSize: 12
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                // Разделы настроек (анимация, шрифты, кнопки заголовка…) —
                // те же, что были во вкладке «Настройки», её страницей в
                // режиме одного раздела (`SettingsPage.panel`). Строится при
                // первом заходе и дальше живёт; ключ раздела запоминается,
                // пока экран не свой, чтобы скрытая страница не пересобралась
                // в полную вкладку.
                Loader {
                    id: settingsPanelLoader
                    objectName: "settingsPanelLoader"
                    anchors.fill: parent
                    visible: page.screen === "settings"
                    // Строится при первом заходе и дальше живёт. ⚠️ Не по
                    // `onVisibleChanged`: страница, открытая ссылкой из
                    // «Настроек», строится асинхронно уже на этом экране, и
                    // смены видимости загрузчик не видит — раздел оставался
                    // пустым.
                    active: page.screen === "settings" || settingsPanelLoader.built
                    asynchronous: true
                    property bool built: false
                    property string panel: ""
                    onLoaded: settingsPanelLoader.built = true

                    Binding {
                        target: settingsPanelLoader
                        property: "panel"
                        value: page.installFileSection
                        when: page.screen === "settings"
                        restoreMode: Binding.RestoreNone
                    }

                    sourceComponent: Component {
                        SettingsPage {
                            panel: settingsPanelLoader.panel
                        }
                    }
                }

                // Пустой экран без объяснения — худшее, что можно показать
                // человеку (правило #13). Текст собирает ядро по матрице
                // кастомизации.
                Text {
                    anchors.centerIn: parent
                    width: parent.width - 64
                    visible: page.screen === "note"
                    text: page.noteText
                    color: root.colTextSecondary
                    font.pointSize: 14
                    lineHeight: 1.5
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                }

                // ── Панель деталей ────────────────────────────────────
                //
                // Отдельный экран, а не окно: у витрины товар открывается
                // на месте, и возврат — одной кнопкой.
                ScrollView {
                    id: detailView
                    anchors.fill: parent
                    visible: page.screen === "detail"
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: detailView.availableWidth
                        // Раскладка внутри ОБЫЧНОГО элемента высоты не
                        // получает ни от кого и схлопывается в полоску —
                        // высоту ставим сами (грабля 7 схемы переноса).
                        height: implicitHeight
                        spacing: 10

                        RexonButton {
                            Layout.margins: 16
                            pal: root.pal
                            tone: root.toneNav
                            text: page.detailBackText
                            onClicked: page.closeDetail()
                        }

                        // Картинка товара. По ней открывается полноэкранный
                        // просмотр — накладные контролы галереи приезжают
                        // следующим ломтем.
                        Rectangle {
                            Layout.alignment: Qt.AlignHCenter
                            Layout.preferredWidth: page.detailPreviewWidth
                            Layout.preferredHeight: page.detailPreviewHeight
                            color: root.colSecondary
                            radius: 4

                            // GIF и анимированный WebP (у курсоров первая
                            // картинка часто такая) играют, остальное —
                            // обычной картинкой. Решает ядро по первым байтам
                            // файла: имя в кеше бывает и без расширения.
                            readonly property bool previewAnimated:
                                page.isAnimatedPreview(page.detailPreview)

                            Image {
                                anchors.fill: parent
                                source: parent.previewAnimated ? "" : page.detailPreview
                                fillMode: Image.PreserveAspectFit
                                asynchronous: true
                                visible: source !== ""
                            }

                            AnimatedImage {
                                anchors.fill: parent
                                source: parent.previewAnimated ? page.detailPreview : ""
                                fillMode: Image.PreserveAspectFit
                                visible: source !== ""
                                // Не крутим кадры, пока карточку не видно
                                // или поверх открыт полноэкранный просмотр.
                                // ⚠️ Своё `visible` здесь не читать: при
                                // создании страницы оно давало петлю привязки
                                // «playing» (журнал r1434). Видна картинка
                                // ровно тогда, когда у неё есть источник, —
                                // это и спрашиваем.
                                playing: parent.previewAnimated && detailView.visible
                                    && !zoomPopup.opened
                            }

                            Text {
                                anchors.centerIn: parent
                                width: parent.width - 32
                                visible: page.detailPreview === ""
                                text: page.detailPreviewText
                                color: root.colTextSecondary
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.WordWrap
                            }

                            // По картинке открывается крупный просмотр:
                            // лупа под курсором, щелчок, Enter — общий
                            // вход (`RexonZoomArea.qml`), как в остальных
                            // вкладках.
                            RexonZoomArea {
                                anchors.fill: parent
                                pal: root.pal
                                radius: parent.radius
                                tooltip: page.detailPreviewTooltip
                                enabled: page.detailPreview !== ""
                                visible: enabled
                                onActivated: root.openZoom()
                            }

                            // ── Накладные контролы карусели ────────────
                            // Место каждого считает ядро (overlay_*_pos):
                            // своих отступов у разметки нет.
                            RexonButton {
                                pal: root.pal
                                visible: page.galleryNavVisible
                                width: page.overlayButtonSize
                                height: page.overlayButtonSize
                                anchors.left: parent.left
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.margins: page.overlayMargin
                                text: page.overlayPrevText
                                ToolTip.text: page.overlayPrevTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: page.galleryPrev()
                            }

                            RexonButton {
                                pal: root.pal
                                visible: page.galleryNavVisible
                                width: page.overlayButtonSize
                                height: page.overlayButtonSize
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.margins: page.overlayMargin
                                text: page.overlayNextText
                                ToolTip.text: page.overlayNextTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: page.galleryNext()
                            }

                            // Счётчик «2/5»: виден только когда кадров
                            // больше одного (решение ядра).
                            Rectangle {
                                visible: page.galleryCounter !== ""
                                anchors.horizontalCenter: parent.horizontalCenter
                                anchors.bottom: parent.bottom
                                anchors.margins: page.overlayMargin
                                width: counterText.implicitWidth + 20
                                height: 24
                                radius: 12
                                color: root.colSecondary
                                opacity: 0.85

                                Text {
                                    id: counterText
                                    anchors.centerIn: parent
                                    text: page.galleryCounter
                                    color: root.colText
                                    font.bold: true
                                }
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            text: page.detailName
                            color: root.colText
                            font.pointSize: 15
                            font.bold: true
                            wrapMode: Text.WordWrap
                        }

                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            text: page.detailMeta
                            color: root.colTextSecondary
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                        }

                        RowLayout {
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            spacing: 0
                            visible: page.detailStars !== ""

                            Text {
                                text: page.detailStars
                                color: root.colStar
                                font.bold: true
                                textFormat: Text.PlainText
                            }

                            Text {
                                text: page.detailDownloads
                                color: root.colText
                                font.bold: true
                                textFormat: Text.PlainText
                            }
                        }

                        // Действия. Набор зависит от того, откуда товар:
                        // у местной темы магазина нет вовсе.
                        Flow {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            spacing: 8

                            Button {
                                id: installBtn
                                visible: !page.detailIsLocal
                                readonly property bool isAur: page.detailSource === "aur"
                                // Кнопка пакета AUR — ровно та, что на его
                                // плитке (очередь установок). Ревизия
                                // перечитывает её при каждом снимке очереди.
                                property int aurRev: 0
                                readonly property var aurBtn: (aurRev, page.count,
                                                               page.detailItemId, isAur)
                                                              ? page.detailAurButton() : ({})
                                Connections {
                                    target: root.actions
                                    ignoreUnknownSignals: true
                                    function onAurJobsChanged() { installBtn.aurRev++; }
                                }
                                // Пока действие идёт, запуск заперт: второе
                                // нажатие поверх первого ничего не ускоряет.
                                enabled: isAur ? aurBtn.enabled === true : !page.actionBusy
                                implicitHeight: 40
                                leftPadding: 14
                                rightPadding: 14
                                // Выбран руками не тот файл, что берётся сам:
                                // кнопка ставит выбранный, даже у стоящего товара.
                                readonly property bool otherFile: !isAur && detailFiles.visible
                                                                  && detailFiles.differs
                                text: isAur ? (aurBtn.text || "")
                                            : (otherFile ? page.detailFileInstallText
                                                         : page.detailInstallText)
                                readonly property string tip: isAur ? (aurBtn.tooltip || "")
                                                                    : (otherFile ? ""
                                                                                 : page.detailInstallTooltip)
                                ToolTip.text: tip
                                ToolTip.visible: hovered && tip !== ""
                                ToolTip.delay: 600
                                onClicked: {
                                    // Тот же общий вход, что у кнопки плитки.
                                    if (isAur) {
                                        root.actOnCard("aur", page.detailItemId);
                                        installBtn.aurRev++;
                                        return;
                                    }
                                    if (otherFile) {
                                        page.actionBegin(page.detailItemId, "install");
                                        if (root.actions && root.actions.installById)
                                            root.actions.installById(page.detailItemId, false,
                                                                     detailFiles.selectedName);
                                        return;
                                    }
                                    root.actOnCard("theme", page.detailItemId);
                                }

                                readonly property var colors:
                                    page.detailButtonColors(isAur ? (aurBtn.state || "")
                                                                  : (otherFile ? "false"
                                                                               : page.detailInstallState))

                                background: Rectangle {
                                    radius: 3
                                    color: installBtn.colors.bg === ""
                                           ? "transparent" : installBtn.colors.bg
                                    border.color: installBtn.activeFocus
                                                  ? root.colAccent
                                                  : (installBtn.colors.border === ""
                                                     ? "transparent"
                                                     : installBtn.colors.border)
                                    border.width: installBtn.activeFocus ? 2 : 1
                                }

                                contentItem: Text {
                                    text: installBtn.text
                                    color: installBtn.colors.fg
                                    font.bold: true
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                }
                            }

                            // Признаки риска пакета AUR — рядом с «Установить»,
                            // не вместо неё: установку это не запрещает.
                            // Список — в подсказке и в «Проверке источника».
                            Text {
                                objectName: "aurTrustBadge"
                                visible: page.detailSource === "aur" && page.aurTrustBadge !== ""
                                height: 40
                                verticalAlignment: Text.AlignVCenter
                                leftPadding: 4
                                rightPadding: 4
                                text: page.aurTrustBadge
                                color: page.aurTrustLevel === "danger" ? page.aurTrustDangerColor
                                                                       : root.colStar
                                font.bold: true
                                ToolTip.text: page.aurTrustTooltip
                                ToolTip.visible: trustBadgeHover.hovered
                                ToolTip.delay: 300
                                HoverHandler { id: trustBadgeHover }
                            }

                            // У темы с диска «Удалить» бывает недоступна
                            // (включённая, системная) — причина подсказкой;
                            // выключенная кнопка наведения не ловит, его
                            // ловит обёртка.
                            Item {
                                visible: page.detailUninstallVisible
                                implicitWidth: uninstallButton.implicitWidth
                                implicitHeight: uninstallButton.implicitHeight

                                RexonDangerButton {
                                    id: uninstallButton
                                    anchors.fill: parent
                                    pal: root.pal
                                    enabled: page.detailUninstallEnabled
                                    text: page.detailUninstallText
                                    onClicked: {
                                        if (root.actions && root.actions.uninstall)
                                            root.actions.uninstall(page.detailItemJson());
                                    }
                                }

                                HoverHandler { id: uninstallHover }
                                ToolTip.text: page.detailUninstallTooltip
                                ToolTip.visible: uninstallHover.hovered
                                                 && page.detailUninstallTooltip !== ""
                                ToolTip.delay: 400
                            }

                            RexonButton {
                                visible: !page.detailIsLocal
                                pal: root.pal
                                // Сайт — вторично: у пакета AUR вся карточка
                                // уже здесь.
                                tone: page.detailSource === "aur" ? root.toneSecondary
                                                                  : null
                                text: page.detailSource === "aur" ? page.aurOpenText
                                                                  : page.detailSiteText
                                ToolTip.text: page.detailSource === "aur" ? page.aurOpenTooltip
                                                                          : page.detailSiteTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: {
                                    const url = page.detailSiteUrl();
                                    if (url && root.actions && root.actions.openUrl)
                                        root.actions.openUrl(url);
                                }
                            }

                            RexonButton {
                                visible: !page.detailIsLocal
                                pal: root.pal
                                text: page.detailCopyLinkText
                                ToolTip.text: page.detailCopyLinkTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: page.copyDetailLink()
                            }

                            RexonButton {
                                visible: page.detailIsLocal
                                pal: root.pal
                                text: page.detailApplyLocalText
                                enabled: page.detailApplyLocalEnabled
                                         && !page.actionBusy
                                onClicked: root.applyLocal(page.detailItemId)
                            }

                            // «Настроить…» стиля приложений (Breeze, Oxygen,
                            // Kvantum…) — как в модуле KDE «Стиль
                            // приложений». Нечем открыть — кнопка выключена,
                            // причина в подсказке (её ловит обёртка:
                            // выключенная кнопка наведения не видит).
                            Item {
                                id: detailStyleConfig
                                objectName: "detailStyleConfig"
                                readonly property var st: JSON.parse(page.detailStyleConfig || "{}")
                                visible: page.detailIsLocal && st.visible === true
                                implicitWidth: styleConfigButton.implicitWidth
                                implicitHeight: styleConfigButton.implicitHeight

                                RexonButton {
                                    id: styleConfigButton
                                    anchors.fill: parent
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    enabled: detailStyleConfig.st.enabled === true
                                    text: detailStyleConfig.st.text || ""
                                    onClicked: {
                                        const err = page.configureDetailStyle();
                                        if (err !== "")
                                            page.setStatus(err);
                                    }
                                }

                                HoverHandler { id: styleConfigHover }
                                ToolTip.text: detailStyleConfig.st.tooltip || ""
                                ToolTip.visible: styleConfigHover.hovered
                                                 && (detailStyleConfig.st.tooltip || "") !== ""
                                ToolTip.delay: 400
                            }

                            // «Настроить…» рамки окна KDE (Breeze, Oxygen,
                            // Klassy, темы Aurorae) — как кнопка правки в
                            // окне KDE «Оформление окон». Нет своих настроек
                            // или нечем открыть — выключена, причина в
                            // подсказке. Окно закрыли — образец переснимается.
                            Item {
                                id: detailDecoConfig
                                objectName: "detailDecoConfig"
                                readonly property var st: JSON.parse(page.detailDecoConfig || "{}")
                                visible: page.detailIsLocal && st.visible === true
                                implicitWidth: decoConfigButton.implicitWidth
                                implicitHeight: decoConfigButton.implicitHeight

                                RexonButton {
                                    id: decoConfigButton
                                    anchors.fill: parent
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    enabled: detailDecoConfig.st.enabled === true
                                    text: detailDecoConfig.st.text || ""
                                    onClicked: {
                                        const err = page.configureDetailDecoration();
                                        if (err !== "")
                                            page.setStatus(err);
                                    }
                                }

                                HoverHandler { id: decoConfigHover }
                                ToolTip.text: detailDecoConfig.st.tooltip || ""
                                ToolTip.visible: decoConfigHover.hovered
                                                 && (detailDecoConfig.st.tooltip || "") !== ""
                                ToolTip.delay: 400
                            }

                            // «Изменить…» и «Создать на основе…» цветовой
                            // схемы — как в модуле KDE «Цвета»: редактор
                            // KDE, а без него — своё окно.
                            Repeater {
                                id: detailSchemeEdit
                                objectName: "detailSchemeEdit"
                                readonly property var st: JSON.parse(page.detailSchemeEdit || "{}")
                                model: page.detailIsLocal && st.visible === true
                                       ? [["edit", st.edit, st.edit_tooltip],
                                          ["new", st.new_from, st.new_from_tooltip]]
                                       : []
                                delegate: RexonButton {
                                    required property var modelData
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    text: modelData[1] || ""
                                    ToolTip.text: modelData[2] || ""
                                    ToolTip.visible: hovered && (modelData[2] || "") !== ""
                                    ToolTip.delay: 600
                                    onClicked: schemeEditor.start(modelData[0])
                                }
                            }

                            // «Запускать при входе» — только у виджетов
                            // Conky: запуск сейчас и автозапуск у них разные
                            // вещи (кнопка рядом — «Запустить»/«Остановить»).
                            RexonCheckBox {
                                id: detailAutostart
                                objectName: "detailAutostart"
                                visible: page.detailIsLocal && page.detailAutostartVisible
                                pal: root.pal
                                text: page.detailAutostartText
                                checked: visible && root.actions && root.actions.conkyAutostart
                                         ? root.actions.conkyAutostart(page.detailItemId)
                                         : false
                                onToggled: {
                                    if (root.actions && root.actions.setConkyAutostart)
                                        detailAutostart.checked = root.actions.setConkyAutostart(
                                            page.detailItemId, detailAutostart.checked);
                                }
                                // Включение проверяет программы виджета в фоне
                                // (`conky_safety::live_prepare_start`): итог
                                // приходит позже — перечитать состояние.
                                Connections {
                                    target: root.actions
                                    ignoreUnknownSignals: true
                                    function onActionDone(text, ok) {
                                        if (detailAutostart.visible && root.actions.conkyAutostart)
                                            detailAutostart.checked = root.actions.conkyAutostart(page.detailItemId);
                                    }
                                }
                            }

                            RexonButton {
                                visible: page.detailIsLocal
                                pal: root.pal
                                tone: root.toneSecondary
                                text: page.detailFavoriteText
                                onClicked: {
                                    if (root.actions && root.actions.toggleFavorite)
                                        page.setFavorites(
                                            root.actions.toggleFavorite(page.detailItemId));
                                }
                            }
                        }

                        // Файлы товара магазина, когда их несколько: по
                        // умолчанию выбран тот, что берётся сам (как с
                        // плитки); выбран другой — главная кнопка ставит его.
                        StoreFileChoice {
                            id: detailFiles
                            objectName: "detailFiles"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            pal: root.pal
                            filesJson: page.detailSource === "store" ? page.detailFiles : "[]"
                            title: page.detailFilesTitle
                            hint: page.detailFilesHint
                            willInstallText: page.detailFileWillInstall
                            enabled: !page.actionBusy
                        }

                        // Тема GTK в программах GTK 4/libadwaita: ссылки на
                        // её `gtk-4.0` в ~/.config/gtk-4.0. Переносить нечего —
                        // переключатель выключен, причина видна строкой.
                        ColumnLayout {
                            id: detailGtk4
                            objectName: "detailGtk4"
                            readonly property var st: JSON.parse(page.detailGtk4 || "{}")
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            visible: page.detailIsLocal && st.visible === true
                            spacing: 2

                            RexonCheckBox {
                                id: detailGtk4Box
                                pal: root.pal
                                enabled: detailGtk4.st.enabled === true
                                checked: detailGtk4.st.checked === true
                                text: detailGtk4.st.text || ""
                                ToolTip.text: detailGtk4.st.note || ""
                                ToolTip.visible: hovered && detailGtk4.st.enabled === true
                                ToolTip.delay: 600
                                onToggled: {
                                    const err = page.setGtk4Apply(detailGtk4Box.checked);
                                    if (err !== "")
                                        page.setStatus(err);
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                visible: detailGtk4.st.enabled !== true
                                text: detailGtk4.st.note || ""
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }
                        }

                        // ── Части виджета Conky и его журнал ─────────────
                        //
                        // Тема из нескольких виджетов (Seamod, наборы
                        // вариантов) — у каждой части своя кнопка. Журнал —
                        // что conky написал при последнем запуске из Rexon.
                        ColumnLayout {
                            id: detailConky
                            objectName: "detailConky"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            visible: page.detailIsLocal && page.detailAutostartVisible
                            spacing: 4

                            property int rev: 0
                            property var parts: []
                            property string log: ""
                            readonly property var texts: conkyToolsRow.texts

                            function reload() {
                                if (!detailConky.visible || !root.actions || !root.actions.conkyParts) {
                                    detailConky.parts = [];
                                    detailConky.log = "";
                                    return;
                                }
                                try {
                                    detailConky.parts = JSON.parse(root.actions.conkyParts(page.detailItemId));
                                } catch (e) {
                                    detailConky.parts = [];
                                }
                                detailConky.log = root.actions.conkyLog(page.detailItemId);
                            }

                            onVisibleChanged: reload()
                            Connections {
                                target: page
                                function onDetailChanged() { detailConky.reload(); }
                            }
                            Connections {
                                target: root.actions
                                ignoreUnknownSignals: true
                                function onActionDone(text, ok) { detailConky.reload(); }
                            }

                            Text {
                                visible: detailConky.parts.length > 1
                                text: detailConky.texts.parts_title || ""
                                color: root.colTextSecondary
                                font.bold: true
                            }

                            Repeater {
                                model: detailConky.parts.length > 1 ? detailConky.parts : []
                                delegate: RowLayout {
                                    id: partRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Text {
                                        Layout.fillWidth: true
                                        text: partRow.modelData.label
                                        color: root.colText
                                        elide: Text.ElideMiddle
                                        ToolTip.text: partRow.modelData.conf
                                        ToolTip.visible: partHover.hovered
                                        ToolTip.delay: 600
                                        HoverHandler { id: partHover }
                                    }

                                    RexonButton {
                                        objectName: "conkyPartButton"
                                        pal: root.pal
                                        enabled: !page.actionBusy
                                        text: partRow.modelData.running ? (detailConky.texts.part_stop || "")
                                                                        : (detailConky.texts.part_start || "")
                                        onClicked: {
                                            if (root.actions && root.actions.pressConkyPart)
                                                detailConky.parts = JSON.parse(root.actions.pressConkyPart(
                                                    page.detailItemId, partRow.modelData.conf));
                                            grid.refreshKeepingScroll();
                                        }
                                    }
                                }
                            }

                            // Настройки виджета — для выбранной части (у
                            // темы из одного файла — для него).
                            // Выбранная часть помнится сама: части
                            // перечитываются после каждой правки, и список,
                            // заведённый заново, иначе прыгал бы на первую.
                            property string chosenConf: ""
                            readonly property string settingsConf: {
                                const ps = detailConky.parts;
                                if (ps.length === 0)
                                    return "";
                                for (let i = 0; i < ps.length; i++)
                                    if (ps[i].conf === detailConky.chosenConf)
                                        return ps[i].conf;
                                return ps[0].conf;
                            }
                            Connections {
                                target: page
                                function onDetailChanged() { detailConky.chosenConf = ""; }
                            }

                            // «Запускать при входе»: задержка и «только в
                            // этом окружении» (ядро: conky_autostart).
                            RowLayout {
                                id: conkyAutostartRow
                                objectName: "conkyAutostartRow"
                                property var v: ({})
                                function reload() {
                                    if (!detailConky.visible || !root.actions || !root.actions.conkyAutostartOptions) {
                                        conkyAutostartRow.v = ({});
                                        return;
                                    }
                                    try {
                                        conkyAutostartRow.v = JSON.parse(root.actions.conkyAutostartOptions(page.detailItemId));
                                    } catch (e) {
                                        conkyAutostartRow.v = ({});
                                    }
                                }
                                function save(delay, onlyHere) {
                                    if (root.actions && root.actions.setConkyAutostartOptions)
                                        conkyAutostartRow.v = JSON.parse(root.actions.setConkyAutostartOptions(
                                            page.detailItemId, delay, onlyHere));
                                }
                                Connections {
                                    target: detailConky
                                    function onPartsChanged() { conkyAutostartRow.reload(); }
                                }
                                visible: conkyAutostartRow.v.on === true
                                spacing: 8
                                Text {
                                    text: (conkyAutostartRow.v.t || {}).delay || ""
                                    color: root.colText
                                    ToolTip.text: (conkyAutostartRow.v.t || {}).delay_tooltip || ""
                                    ToolTip.visible: delayHover.hovered
                                    ToolTip.delay: 600
                                    HoverHandler { id: delayHover }
                                }
                                RexonTextField {
                                    objectName: "conkyAutostartDelay"
                                    pal: root.pal
                                    Layout.preferredWidth: 60
                                    enabled: !page.actionBusy
                                    text: String(conkyAutostartRow.v.delay === undefined ? "" : conkyAutostartRow.v.delay)
                                    validator: IntValidator { bottom: 0; top: conkyAutostartRow.v.max_delay || 600 }
                                    onEditingFinished: {
                                        const n = parseInt(text);
                                        if (!isNaN(n) && n !== conkyAutostartRow.v.delay)
                                            conkyAutostartRow.save(n, conkyAutostartRow.v.only_here === true);
                                    }
                                }
                                RexonCheckBox {
                                    objectName: "conkyAutostartOnlyHere"
                                    visible: conkyAutostartRow.v.can_limit === true
                                    pal: root.pal
                                    enabled: !page.actionBusy
                                    text: (conkyAutostartRow.v.t || {}).only_here || ""
                                    checked: conkyAutostartRow.v.only_here === true
                                    ToolTip.text: (conkyAutostartRow.v.t || {}).only_here_tooltip || ""
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 600
                                    onToggled: conkyAutostartRow.save(conkyAutostartRow.v.delay || 0, checked)
                                }
                            }

                            RexonComboBox {
                                id: conkySettingsPart
                                objectName: "conkySettingsPart"
                                visible: detailConky.parts.length > 1
                                pal: root.pal
                                textRole: "label"
                                valueRole: "conf"
                                model: detailConky.parts
                                selectedValue: detailConky.settingsConf
                                onActivated: detailConky.chosenConf = currentValue
                            }

                            ConkySettings {
                                id: conkySettings
                                objectName: "conkySettings"
                                Layout.fillWidth: true
                                visible: detailConky.parts.length > 0
                                pal: root.pal
                                actions: root.actions
                                busy: page.actionBusy
                                itemId: page.detailItemId
                                conf: detailConky.settingsConf
                            }

                            Text {
                                Layout.topMargin: 4
                                visible: detailConky.log !== ""
                                text: detailConky.texts.log_title || ""
                                color: root.colTextSecondary
                                font.bold: true
                            }

                            Text {
                                objectName: "conkyLogText"
                                Layout.fillWidth: true
                                visible: detailConky.log !== ""
                                text: detailConky.log
                                color: root.colTextSecondary
                                font.family: "monospace"
                                font.pointSize: 9
                                textFormat: Text.PlainText
                                wrapMode: Text.WrapAnywhere
                            }
                        }

                        // ── Прослушать (тема звуков) ───────────────────
                        //
                        // Установленная тема — сразу пять кнопок, как в
                        // KDE; товар Магазина — сперва «Прослушать до
                        // установки», после скачивания те же кнопки.
                        ColumnLayout {
                            id: detailSounds
                            objectName: "detailSounds"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: page.detailSoundTheme !== "" || page.detailSoundStore
                            spacing: 6

                            property string playError: ""

                            Row {
                                visible: page.detailSoundTheme !== ""
                                spacing: 8

                                Text {
                                    text: page.soundPreviewLabel
                                    color: root.colTextSecondary
                                    font.weight: Font.DemiBold
                                    height: 40
                                    verticalAlignment: Text.AlignVCenter
                                }

                                SoundButtons {
                                    player: soundActions
                                    texts: page
                                    pal: root.pal
                                    theme: page.detailSoundTheme
                                    buttonSize: 40
                                    onFailed: function (text) { detailSounds.playError = text; }
                                }
                            }

                            RexonButton {
                                objectName: "storePreviewButton"
                                visible: page.detailSoundStore
                                         && root.storePreviewThemes.length === 0
                                enabled: !root.storePreviewBusy
                                pal: root.pal
                                text: page.soundStorePreviewText
                                ToolTip.text: page.soundStorePreviewTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: {
                                    detailSounds.playError = "";
                                    root.startStorePreview();
                                }
                            }

                            ProgressBar {
                                Layout.preferredWidth: 320
                                visible: page.detailSoundStore && root.storePreviewBusy
                                indeterminate: root.storePreviewTotal <= 0
                                from: 0
                                to: Math.max(1, root.storePreviewTotal)
                                value: root.storePreviewDone
                            }

                            Text {
                                Layout.fillWidth: true
                                visible: page.detailSoundStore && text !== ""
                                text: root.storePreviewStatus
                                color: root.storePreviewFailed
                                       ? (root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                            : root.colAccent)
                                       : root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            // В одном архиве бывает несколько тем — у каждой
                            // свой ряд, с именем, когда тем больше одной.
                            Repeater {
                                model: page.detailSoundStore ? root.storePreviewThemes : []
                                delegate: Column {
                                    id: storeTheme
                                    required property var modelData
                                    spacing: 4

                                    Text {
                                        visible: root.storePreviewThemes.length > 1
                                        text: storeTheme.modelData.theme
                                        color: root.colText
                                        font.bold: true
                                    }

                                    Row {
                                        spacing: 8

                                        Text {
                                            text: page.soundPreviewLabel
                                            color: root.colTextSecondary
                                            font.weight: Font.DemiBold
                                            height: 40
                                            verticalAlignment: Text.AlignVCenter
                                        }

                                        SoundButtons {
                                            player: soundActions
                                            texts: page
                                            pal: root.pal
                                            storeSounds: storeTheme.modelData.sounds
                                            buttonSize: 40
                                            onFailed: function (text) { detailSounds.playError = text; }
                                        }
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                visible: text !== ""
                                text: detailSounds.playError
                                color: root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                         : root.colAccent
                                wrapMode: Text.WordWrap
                            }
                        }

                        // Полоса хода установки. В оригинале виден именно
                        // ХОД, а не одна строка итога: сборка помощником AUR
                        // и большие темы идут десятками секунд, и без полосы
                        // страница выглядит замершей.
                        ProgressBar {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: page.progressVisible
                            from: 0
                            to: 100
                            value: page.installPercent
                        }

                        // Итог последнего действия — он же построчный журнал
                        // работы: установщик пишет сюда каждый свой шаг.
                        // Значок «✅»/«❌» ставит ядро — своей развилки у
                        // разметки нет.
                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: text !== ""
                            text: page.detailStatus
                            color: root.colText
                            wrapMode: Text.WordWrap
                        }

                        // Описание с переключателем «перевод ⇄ оригинал».
                        // Переключатель — ссылка внутри самого текста: его
                        // собирает ядро, здесь только обработка нажатия.
                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            text: page.detailDescription
                            color: root.colText
                            linkColor: root.colAccent
                            textFormat: Text.RichText
                            wrapMode: Text.WordWrap
                            onLinkActivated: function (link) {
                                if (link === "rexon:toggle")
                                    page.toggleDescription();
                                else if (root.actions && root.actions.openUrl)
                                    root.actions.openUrl(link);
                            }
                        }

                        // ── Сведения о пакете AUR ──────────────────────
                        //
                        // Паспорт RPC: сопровождающие, голоса, даты,
                        // лицензия, зависимости, конфликты. Подписи и порядок
                        // строк — ядро (`aur::info_facts`).
                        ColumnLayout {
                            objectName: "aurFacts"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            visible: page.detailSource === "aur"
                            spacing: 4

                            Repeater {
                                model: page.detailSource === "aur" ? page.aurFacts : []

                                delegate: RowLayout {
                                    id: factRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 14

                                    Text {
                                        Layout.preferredWidth: 190
                                        Layout.alignment: Qt.AlignTop
                                        visible: factRow.modelData.label !== ""
                                        text: factRow.modelData.label
                                        color: root.colTextSecondary
                                        font.bold: true
                                        textFormat: Text.PlainText
                                        wrapMode: Text.WordWrap
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: factRow.modelData.html !== ""
                                              ? factRow.modelData.html
                                              : factRow.modelData.value
                                        textFormat: factRow.modelData.html !== ""
                                                    ? Text.RichText : Text.PlainText
                                        color: factRow.modelData.warn
                                               ? (root.pal.state_warn !== undefined
                                                  ? root.pal.state_warn : root.colStar)
                                               : root.colText
                                        linkColor: root.colAccent
                                        wrapMode: Text.WordWrap
                                        onLinkActivated: function (link) {
                                            if (root.actions && root.actions.openUrl)
                                                root.actions.openUrl(link);
                                        }
                                    }
                                }
                            }
                        }

                        // ── Проверка источника пакета AUR ──────────────
                        //
                        // Признаки риска (`aur_trust`): что насторожило и
                        // почему это важно. Установку не запрещает.
                        ColumnLayout {
                            objectName: "aurTrust"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 10
                            visible: page.detailSource === "aur"
                            spacing: 6

                            Text {
                                text: page.aurTrustTitle
                                color: root.colText
                                font.bold: true
                                font.pointSize: 12
                            }

                            Text {
                                Layout.fillWidth: true
                                text: page.aurTrustNote
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            Repeater {
                                model: page.detailSource === "aur" ? page.aurTrustRows : []

                                delegate: RowLayout {
                                    id: trustRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 10

                                    readonly property color tone: trustRow.modelData.danger
                                                                  ? page.aurTrustDangerColor
                                                                  : root.colStar

                                    Text {
                                        Layout.alignment: Qt.AlignTop
                                        text: trustRow.modelData.danger ? "⛔" : "⚠️"
                                        color: trustRow.tone
                                        font.pointSize: 12
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 2

                                        Text {
                                            Layout.fillWidth: true
                                            text: trustRow.modelData.text
                                            color: trustRow.tone
                                            font.bold: true
                                            textFormat: Text.PlainText
                                            wrapMode: Text.WordWrap
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            text: trustRow.modelData.why
                                            color: root.colTextSecondary
                                            textFormat: Text.PlainText
                                            wrapMode: Text.WordWrap
                                        }
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                text: page.aurTrustStatus
                                color: root.colTextSecondary
                                textFormat: Text.PlainText
                                wrapMode: Text.WordWrap
                            }

                            RexonButton {
                                objectName: "aurPkgbuildButton"
                                pal: root.pal
                                tone: root.toneSecondary
                                text: page.aurPkgbuildBtn
                                ToolTip.text: page.aurPkgbuildBtnTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: pkgbuildPopup.show()
                            }
                        }

                        // ── Комментарии ────────────────────────────────
                        //
                        // У местной темы их нет вовсе: тема лежит на диске,
                        // страницы магазина у неё не бывает. У Cinnamon
                        // Spices комментариев для программ нет.
                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 10
                            visible: !page.detailIsLocal && page.detailSource !== "spices"
                            text: page.commentsHeader
                            color: root.colText
                            font.bold: true
                            font.pointSize: 12
                        }

                        Text {
                            Layout.leftMargin: 16
                            visible: !page.detailIsLocal && page.detailSource !== "spices"
                                     && page.commentsEmpty
                                     && !page.commentsBlocked
                            text: page.commentsEmptyText
                            color: root.colTextSecondary
                        }

                        // Страницу пакета AUR не дали (защита сайта от ботов,
                        // нет сети): комментарии — только на сайте.
                        ColumnLayout {
                            objectName: "aurCommentsBlocked"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: page.commentsBlocked
                            spacing: 6

                            Text {
                                Layout.fillWidth: true
                                text: page.commentsBlockedText
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            RexonButton {
                                pal: root.pal
                                tone: root.toneSecondary
                                text: page.aurOpenText
                                onClicked: {
                                    const url = page.detailSiteUrl();
                                    if (url && root.actions && root.actions.openUrl)
                                        root.actions.openUrl(url);
                                }
                            }
                        }

                        Repeater {
                            model: page.detailIsLocal ? [] : page.commentRows

                            delegate: ColumnLayout {
                                id: commentRow
                                required property var modelData
                                Layout.fillWidth: true
                                // Отбивка ответа в ветке — число из ядра.
                                Layout.leftMargin: 16 + commentRow.modelData.indent
                                Layout.rightMargin: 16
                                Layout.topMargin: 6
                                spacing: 2

                                Text {
                                    Layout.fillWidth: true
                                    text: commentRow.modelData.replyPrefix
                                          + commentRow.modelData.author
                                          + "  ·  " + commentRow.modelData.date
                                    // Автор товара выделен акцентом: его
                                    // ответ в ветке весит иначе, чем чужой.
                                    color: commentRow.modelData.isProductAuthor
                                           ? root.colAccent : root.colTextSecondary
                                    font.bold: true
                                    font.pointSize: 9
                                    elide: Text.ElideRight
                                }

                                Text {
                                    Layout.fillWidth: true
                                    text: commentRow.modelData.body
                                    color: root.colText
                                    linkColor: root.colAccent
                                    textFormat: Text.RichText
                                    wrapMode: Text.WordWrap
                                    onLinkActivated: function (link) {
                                        if (link === "rexon:toggle")
                                            page.toggleComment(commentRow.modelData.index);
                                        else if (root.actions && root.actions.openUrl)
                                            root.actions.openUrl(link);
                                    }
                                }
                            }
                        }

                        RexonButton {
                            Layout.leftMargin: 16
                            Layout.topMargin: 8
                            visible: page.commentsMoreVisible
                            pal: root.pal
                            tone: root.toneSecondary
                            text: page.commentsMoreText
                            onClicked: page.loadMoreComments()
                        }

                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 10
                            Layout.bottomMargin: 16
                            text: page.detailPath
                            color: root.colTextSecondary
                            font.pointSize: 9
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                GridView {
                    id: grid
                    anchors.fill: parent
                    // ⚠️ Поля СОДЕРЖИМОГО, а не самой области. Разница
                    // видна на полосе прокрутки: у оригинала полоса стоит у
                    // края окна, а поля принадлежат сетке внутри неё. Когда
                    // поля были у области (`anchors.*Margin`), полоса
                    // уезжала на 16 точек внутрь — замер живого запуска
                    // 01.09.2026.
                    //
                    // Поля разные по осям: по бокам ровно столько же,
                    // сколько у шапки и строки состояния (16), иначе левый
                    // край плиток не совпадает с левым краем подписи над
                    // ними.
                    leftMargin: page.gridMarginH
                    // Справа поле уже на зазор: зазор после последнего
                    // столбца сидит внутри ячейки, и край плиток встаёт на
                    // те же 16 точек, что и слева.
                    rightMargin: Math.max(0, page.gridMarginH - page.gridSpacing)
                    topMargin: page.gridMarginV
                    bottomMargin: page.gridMarginV
                    visible: page.screen === "grid"
                    clip: true
                    // Столбцы — по ширине: сколько плиток ядра (`cardWidth`
                    // — наименьшая ширина) влезает, столько и столбцов, а
                    // остаток ширины делится между ними. Раньше плитка была
                    // постоянной, и на окне 1100 точек справа от двух
                    // столбцов зияла пустая полоса шириной почти в плитку.
                    readonly property int usableWidth:
                        width - leftMargin - rightMargin
                    readonly property int columns: Math.max(1, Math.floor(
                        usableWidth / (page.cardWidth + page.gridSpacing)))
                    cellWidth: Math.max(page.cardWidth + page.gridSpacing,
                                        Math.floor(usableWidth / columns))
                    cellHeight: page.cardHeight + page.gridSpacing
                    model: page

                    ScrollBar.vertical: ScrollBar {}

                    // Блок «Менеджер входа» над темами экрана входа — шапка
                    // самой сетки, а не отдельная строка над ней: в окне
                    // 1100×750 блок в пять строк оставлял плиткам около
                    // 60 точек, и колесо их почти не листало. Теперь
                    // прокручивается весь раздел целиком, как у KDE.
                    // Шапка стоит в координатах содержимого с x = 0, то есть
                    // уже после левого поля сетки; ширина — до правого поля
                    // плиток (16, как у шапки страницы).
                    header: Item {
                        width: grid.width - grid.leftMargin - page.gridMarginH
                        height: gridLoginManager.visible ? gridLoginManager.implicitHeight + page.gridSpacing : 0

                        LoginManagerSection {
                            id: gridLoginManager
                            objectName: "gridLoginManagerSection"
                            width: parent.width
                            visible: page.loginSection
                                     && (gridLoginManager.managers.length > 0 || gridLoginManager.sessionOffer !== null)
                            pal: root.pal
                            sty: page.sectionStyles
                            actions: root.actions
                        }
                    }

                    // Перечитать раздел, не теряя места: после пометки к
                    // удалению, «Вернуть» и снятия сетка сбрасывается
                    // моделью и прыгала наверх, а в KDE позиция остаётся.
                    // Место запоминается здесь и возвращается, когда придёт
                    // новый состав (`countChanged` шлётся на каждый ответ).
                    property real keptY: NaN

                    function refreshKeepingScroll() {
                        if (isNaN(grid.keptY))
                            grid.keptY = grid.contentY;
                        page.refresh();
                    }

                    function restoreKeptScroll() {
                        if (isNaN(grid.keptY))
                            return;
                        const y = grid.keptY;
                        grid.keptY = NaN;
                        grid.forceLayout();
                        const top = grid.originY - grid.topMargin;
                        const bottom = grid.originY + grid.contentHeight
                                       + grid.bottomMargin - grid.height;
                        grid.contentY = Math.max(top, Math.min(y, Math.max(top, bottom)));
                    }

                    Connections {
                        target: page
                        function onCountChanged() {
                            Qt.callLater(grid.restoreKeptScroll);
                        }
                    }

                    delegate: Item {
                        id: cell
                        required property string kind
                        required property string itemId
                        required property string name
                        required property string author
                        required property string stats
                        required property string statsTooltip
                        required property string starGlyph
                        required property string preview
                        required property string previewText
                        required property string metaLine
                        required property string metaTooltip
                        required property bool metaWarn
                        required property string buttonText
                        required property string buttonState
                        required property bool buttonEnabled
                        required property string buttonTooltip
                        required property string favoriteText
                        required property string favoriteState
                        required property string soundTheme
                        required property string splashDemo
                        required property string aurNote
                        required property string aurNoteTooltip
                        required property string aurLog
                        required property string riskLevel
                        required property bool removeVisible
                        required property bool removeEnabled
                        required property string removeTooltip
                        required property bool current
                        // Текущая тема раздела выделяется, как в KDE: рамка
                        // цвета выделения и жирная подпись (у остальных
                        // плиток раздела подпись обычная).
                        readonly property bool marked: page.markCurrent && cell.current

                        width: grid.cellWidth
                        height: grid.cellHeight

                        Rectangle {
                            id: card
                            width: grid.cellWidth - page.gridSpacing
                            height: page.cardHeight
                            radius: 4
                            color: cardMouse.containsMouse ? root.colCardHover
                                                           : root.colCard
                            border.color: (cell.marked || cardMouse.containsMouse)
                                          ? root.colAccent : root.colBorder
                            border.width: 1

                            // Рамка текущей темы — поверх картинки: своя
                            // рамка плитки лежит под содержимым, и картинка
                            // во всю ширину её закрывает.
                            Rectangle {
                                objectName: "currentFrame"
                                anchors.fill: parent
                                z: 10
                                visible: cell.marked
                                color: "transparent"
                                radius: parent.radius
                                border.color: root.colAccent
                                border.width: 3
                            }

                            MouseArea {
                                id: cardMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                // Пакет AUR открывает свою карточку внутри
                                // программы, как товар магазина.
                                onClicked: page.openDetail(cell.itemId)
                            }

                            ColumnLayout {
                                anchors.fill: parent
                                spacing: 0

                                // Картинка товара.
                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: page.cardPreviewHeight
                                    id: previewBox
                                    color: root.colSecondary

                                    // Нарисованная картинка темы без своей
                                    // (каталог `local_placeholders`): образец
                                    // из её содержимого либо заглушка вида.
                                    // Обрезка по высоте, уместная у снимков,
                                    // срезала бы у неё края: вписываем
                                    // целиком, а поля по бокам закрывает тот же
                                    // переход цвета, что на самом кадре.
                                    readonly property bool placeholder:
                                        cell.preview.indexOf("/local_placeholders/") >= 0
                                    // Картинки нет вовсе (пакет AUR без
                                    // ссылки на GitHub): та же заглушка вида,
                                    // что у местных тем, — знак на той же
                                    // подложке.
                                    readonly property bool bare:
                                        cell.preview === "" && cell.previewText !== ""

                                    Rectangle {
                                        anchors.fill: parent
                                        visible: previewBox.placeholder || previewBox.bare
                                        gradient: Gradient {
                                            GradientStop { position: 0.0; color: "#263d52" }
                                            GradientStop { position: 1.0; color: "#16202d" }
                                        }
                                    }

                                    Image {
                                        anchors.fill: parent
                                        source: cell.preview
                                        fillMode: previewBox.placeholder
                                                  ? Image.PreserveAspectFit
                                                  : Image.PreserveAspectCrop
                                        asynchronous: true
                                        cache: true
                                        // У темы звуков на этом месте ряд
                                        // «Прослушать:» (ниже): картинка-
                                        // заглушка с колокольчиком его
                                        // перекрывала бы.
                                        visible: source !== "" && cell.soundTheme === ""
                                    }

                                    // Щелчок по плитке занят карточкой
                                    // темы, поэтому крупный просмотр её
                                    // картинки открывает лупа под курсором.
                                    // У нарисованной картинки (образец,
                                    // заглушка) крупнее показать нечего.
                                    RexonZoomArea {
                                        anchors.fill: parent
                                        pal: root.pal
                                        badgeOnly: true
                                        enabled: cell.preview !== "" && cell.soundTheme === ""
                                                 && !previewBox.placeholder
                                        visible: enabled
                                        onActivated: root.openTileZoom(cell.name, cell.preview)
                                    }

                                    // «Продемонстрировать» у заставки KDE,
                                    // как в модуле KDE: заставка во весь
                                    // экран на несколько секунд (ядро —
                                    // `splash_demo_argv`). Поверх лупы.
                                    RexonButton {
                                        anchors.right: parent.right
                                        anchors.bottom: parent.bottom
                                        anchors.margins: 8
                                        visible: cell.splashDemo !== ""
                                        pal: root.pal
                                        // У темы SDDM та же кнопка — «Предпросмотр»
                                        // экрана входа (ядро — `local_splash_demo`).
                                        text: "▶ " + (root.actions && root.actions.tileDemoLabel
                                                      ? root.actions.tileDemoLabel(cell.splashDemo)
                                                      : page.splashDemoLabel)
                                        onClicked: {
                                            if (!root.actions || !root.actions.demoSplash)
                                                return;
                                            const err = root.actions.demoSplash(cell.splashDemo);
                                            if (err !== "")
                                                page.setStatus(err);
                                        }
                                        HoverHandler { id: splashDemoHover }
                                        ToolTip.text: root.actions && root.actions.tileDemoTooltip
                                                      ? root.actions.tileDemoTooltip(cell.splashDemo)
                                                      : page.splashDemoTooltip
                                        ToolTip.visible: splashDemoHover.hovered
                                        ToolTip.delay: 500
                                    }

                                    // Картинки нет вовсе — на её месте знак
                                    // товара. Так делал оригинал: у пакета
                                    // AUR без ссылки на GitHub взять картинку
                                    // неоткуда, и пустой прямоугольник читался
                                    // как «плитка не догрузилась».
                                    Text {
                                        anchors.centerIn: parent
                                        visible: previewBox.bare
                                        text: cell.previewText
                                        color: "#ffffff"
                                        // Кегль знака — как на нарисованной
                                        // заглушке (48 при высоте кадра 170).
                                        font.pixelSize: Math.round(parent.height * 0.38)
                                    }

                                    // Тема звуков картинки не имеет: на её
                                    // месте ряд «Прослушать:», как у
                                    // карточки KDE «Системные звуки».
                                    Column {
                                        anchors.centerIn: parent
                                        visible: cell.soundTheme !== ""
                                        spacing: 8

                                        Text {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            text: "🔔"
                                            font.pointSize: 24
                                        }

                                        Text {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            text: page.soundPreviewLabel
                                            color: root.colTextSecondary
                                            font.weight: Font.DemiBold
                                        }

                                        Loader {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            active: cell.soundTheme !== ""
                                            sourceComponent: SoundButtons {
                                                player: soundActions
                                                texts: page
                                                pal: root.pal
                                                theme: cell.soundTheme
                                                onFailed: function (text) { page.setStatus(text); }
                                            }
                                        }
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    // Поля тела плитки у оригинала разные
                                    // сверху и снизу (14 против 10), а зазор
                                    // строк вдвое шире, чем стоял здесь.
                                    Layout.leftMargin: page.cardBodyMargin
                                    Layout.rightMargin: page.cardBodyMargin
                                    Layout.topMargin: page.cardBodyTopMargin
                                    Layout.bottomMargin: page.cardBodyBottomMargin
                                    spacing: page.cardBodySpacing

                                    Text {
                                        Layout.fillWidth: true
                                        text: cell.name
                                        color: root.colText
                                        // Название темы у оригинала КРУПНЕЕ
                                        // прочих строк плитки (11pt против
                                        // 10): по нему тему находят в сетке.
                                        // Здесь кегль не задавался вовсе, и
                                        // имя выходило мельче автора.
                                        font.pointSize: page.cardNamePointSize
                                        font.bold: !page.markCurrent || cell.marked
                                        elide: Text.ElideRight
                                    }

                                    // У пакета AUR место автора занимает
                                    // строка источника («🛠 AUR · версия»),
                                    // как в оригинале: лишняя строка не
                                    // влезала в плитку постоянной высоты, и
                                    // кнопка «Установить» обрезалась снизу
                                    // (снимки 01.10.2026).
                                    Text {
                                        Layout.fillWidth: true
                                        visible: cell.metaLine === ""
                                        text: cell.author
                                        color: root.colText
                                        font.pointSize: page.cardMetaPointSize
                                        // `#CardAuthor { font-weight: 600 }`
                                        // — полужирный, не жирный; у
                                        // текущей темы — жирный (KDE).
                                        font.weight: cell.marked ? Font.Bold : Font.DemiBold
                                        elide: Text.ElideRight
                                    }

                                    // Строка источника — только у пакетов AUR:
                                    // в режиме «Все» они идут вперемешку с
                                    // магазинными, и откуда возьмётся пакет,
                                    // человек обязан видеть.
                                    Text {
                                        Layout.fillWidth: true
                                        visible: cell.metaLine !== ""
                                        text: cell.metaLine
                                        color: cell.riskLevel === "danger"
                                               ? page.aurTrustDangerColor
                                               : (cell.metaWarn ? root.colStar
                                                                : root.colTextSecondary)
                                        font.pointSize: page.cardMetaPointSize
                                        elide: Text.ElideRight
                                        ToolTip.text: cell.metaTooltip
                                        ToolTip.visible: metaHover.hovered
                                                         && cell.metaTooltip !== ""
                                        ToolTip.delay: 600
                                        HoverHandler { id: metaHover }
                                    }

                                    // Отказ установки AUR — на месте
                                    // стат-строки (высота плитки постоянна:
                                    // лишняя строка обрезала бы кнопку):
                                    // причина словами и ссылка «Журнал».
                                    RowLayout {
                                        Layout.fillWidth: true
                                        visible: cell.aurNote !== ""
                                        spacing: 6

                                        Text {
                                            Layout.fillWidth: true
                                            text: cell.aurNote
                                            color: root.colStar
                                            font.pointSize: page.cardMetaPointSize
                                            font.bold: true
                                            elide: Text.ElideRight
                                            ToolTip.text: cell.aurNoteTooltip
                                            ToolTip.visible: noteHover.hovered
                                                             && cell.aurNoteTooltip !== ""
                                            ToolTip.delay: 300
                                            HoverHandler { id: noteHover }
                                        }

                                        Text {
                                            visible: cell.aurLog !== ""
                                            text: cell.aurLog
                                            color: root.colAccent
                                            font.pointSize: page.cardMetaPointSize
                                            font.underline: true
                                            MouseArea {
                                                anchors.fill: parent
                                                cursorShape: Qt.PointingHandCursor
                                                onClicked: {
                                                    if (root.actions && root.actions.openAurLog)
                                                        root.actions.openAurLog(cell.itemId);
                                                }
                                            }
                                        }
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true
                                        // Пустая стат-строка (тема с диска)
                                        // не показывается вовсе — и звезда тоже.
                                        visible: cell.aurNote === "" && cell.stats !== ""
                                        spacing: 0

                                        // Значок оценки — единственное
                                        // цветное пятно стат-строки; цвет
                                        // приходит токеном скина.
                                        Text {
                                            text: cell.starGlyph
                                            color: root.colStar
                                            font.pointSize: page.cardMetaPointSize
                                            font.bold: true
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            text: cell.stats
                                            color: root.colText
                                            font.pointSize: page.cardMetaPointSize
                                            font.bold: true
                                            elide: Text.ElideRight
                                            ToolTip.text: cell.statsTooltip
                                            ToolTip.visible: statsHover.hovered
                                                             && cell.statsTooltip !== ""
                                            ToolTip.delay: 600
                                            HoverHandler { id: statsHover }
                                        }
                                    }

                                    // Кнопка плитки. Цвета приходят из ядра:
                                    // три состояния различаются не только
                                    // оттенком, но и силуэтом (заливка
                                    // против контура).
                                    RowLayout {
                                        id: cardButtons
                                        Layout.fillWidth: true
                                        Layout.topMargin: page.cardButtonTopMargin
                                        spacing: 6

                                        Button {
                                            id: cardButton
                                            Layout.fillWidth: true
                                            implicitHeight: page.cardButtonHeight
                                            enabled: cell.buttonEnabled
                                            // ⚠️ Приглушения тут быть не
                                            // должно. Бейдж «✅ Активна»
                                            // выключен, но у оригинала он
                                            // полноцветный: своего правила
                                            // `:disabled` таблица стилей не
                                            // держит. Прозрачность подмешивала
                                            // фон плитки и меняла цвет кнопки.
                                            text: cell.buttonText
                                            // Исключение — действие, которое
                                            // сейчас не сработает (тема SDDM
                                            // при Plasma Login Manager): кнопка
                                            // приглушена, причина — в подсказке.
                                            opacity: (cell.kind !== "aur"
                                                      && cell.buttonTooltip !== "") ? 0.45 : 1.0
                                            ToolTip.text: cell.buttonTooltip
                                            ToolTip.visible: hovered
                                                             && cell.buttonTooltip !== ""
                                            ToolTip.delay: 600

                                            onClicked: root.actOnCard(cell.kind, cell.itemId)
                                            // Пробел кнопка ловит сама; Enter —
                                            // то же нажатие (у виджета Conky —
                                            // запустить/остановить).
                                            Keys.onReturnPressed: cardButton.clicked()
                                            Keys.onEnterPressed: cardButton.clicked()

                                            readonly property var colors:
                                                page.buttonColors(cell.buttonState)

                                            background: Rectangle {
                                                radius: 3
                                                color: cardButton.colors.bg === ""
                                                       ? "transparent"
                                                       : cardButton.colors.bg
                                                border.color: cardButton.activeFocus
                                                              ? root.colAccent
                                                              : (cardButton.colors.border === ""
                                                                 ? "transparent"
                                                                 : cardButton.colors.border)
                                                border.width: cardButton.activeFocus ? 2 : 1
                                            }

                                            contentItem: Text {
                                                text: cardButton.text
                                                color: cardButton.colors.fg
                                                font.bold: true
                                                horizontalAlignment: Text.AlignHCenter
                                                verticalAlignment: Text.AlignVCenter
                                                elide: Text.ElideRight
                                            }
                                        }

                                        // У пакета AUR вместо избранного —
                                        // страница пакета на сайте, как в
                                        // оригинале.
                                        RexonButton {
                                            Layout.fillWidth: true
                                            visible: cell.kind === "aur"
                                            pal: root.pal
                                            tone: root.toneSecondary
                                            implicitHeight: page.cardButtonHeight
                                            text: page.aurDetailsText
                                            ToolTip.text: page.aurDetailsTooltip
                                            ToolTip.visible: hovered
                                            ToolTip.delay: 600
                                            // Пакет AUR — своя карточка внутри
                                            // программы; сайт — из неё.
                                            onClicked: page.openDetail(cell.itemId)
                                        }

                                        // «★ Избранное» — рядом с действием, как
                                        // в оригинале; у пакета AUR избранного
                                        // не бывает, и подпись тогда пуста.
                                        RexonButton {
                                            Layout.fillWidth: true
                                            visible: cell.favoriteText !== ""
                                            pal: root.pal
                                            tone: root.toneSecondary
                                            implicitHeight: page.cardButtonHeight
                                            text: cell.favoriteText
                                            highlighted: cell.favoriteState === "on"
                                            onClicked: {
                                                if (root.actions
                                                        && root.actions.toggleFavorite)
                                                    page.setFavorites(
                                                        root.actions.toggleFavorite(
                                                            cell.itemId));
                                            }
                                        }

                                        // «🗑» — удалить тему с диска (решает ядро,
                                        // `theme_removal`). У включённой и системной
                                        // кнопка недоступна, причина — подсказкой;
                                        // выключенная кнопка наведения не ловит,
                                        // поэтому подсказку держит обёртка.
                                        Item {
                                            visible: cell.removeVisible
                                            Layout.preferredWidth: page.cardButtonHeight
                                            Layout.preferredHeight: page.cardButtonHeight

                                            RexonDangerButton {
                                                anchors.fill: parent
                                                pal: root.pal
                                                enabled: cell.removeEnabled
                                                text: "🗑️"
                                                onClicked: root.removeLocal(cell.itemId)
                                            }

                                            HoverHandler { id: removeHover }
                                            ToolTip.text: cell.removeTooltip !== ""
                                                          ? cell.removeTooltip
                                                          : page.detailUninstallText
                                            ToolTip.visible: removeHover.hovered
                                            ToolTip.delay: 400
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── Полоса страниц ────────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 10
                visible: page.paginationVisible && page.screen === "grid"
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    pal: root.pal
                    tone: root.toneNav
                    text: page.pagePrevText
                    enabled: page.prevEnabled
                    onClicked: page.prevPage()
                }

                Text {
                    text: page.pageLabel
                    color: root.colTextSecondary
                    font.bold: true
                    leftPadding: 12
                    rightPadding: 12
                    verticalAlignment: Text.AlignVCenter
                    Layout.alignment: Qt.AlignVCenter
                }

                RexonButton {
                    pal: root.pal
                    tone: root.toneNav
                    text: page.pageNextText
                    enabled: page.nextEnabled
                    onClicked: page.nextPage()
                }

                Item { Layout.fillWidth: true }
            }
        }
    }
    // ── Строка «Вернуть» отложенного удаления ────────────────────────────
    //
    // Как в KDE: тема с диска не снимается сразу, а помечается; плитка
    // уходит, внизу висит строка с «Вернуть». Снимает тему хозяин, когда
    // строка закрылась (✕), истекла или страница закрылась. Сколько висит —
    // число ядра (`theme_removal::UNDO_TIMEOUT_MS`).
    Rectangle {
        id: removalBar

        property string message: ""
        // Сколько осталось до снятия, мс. Отсчёт виден (полоса и секунды
        // на «Вернуть»), стоит на паузе, пока над строкой курсор или фокус
        // на её кнопках, и начинается заново при каждой новой пометке.
        property real remainingMs: page.removalUndoMs
        readonly property int remainingSeconds: Math.max(0, Math.ceil(remainingMs / 1000))
        readonly property bool paused: barHover.hovered || undoButton.activeFocus
                                       || closeButton.activeFocus

        // Ширина не скачет вместе с числом секунд: держится самая широкая
        // за показ.
        property real widest: 0

        function show(text) {
            removalBar.message = text;
            removalBar.widest = 0;
            removalBar.remainingMs = page.removalUndoMs;
            removalBar.visible = true;
            removalTimer.restart();
        }

        // Закрыть строку и снять помеченное.
        function finish() {
            if (!removalBar.visible)
                return;
            removalBar.visible = false;
            removalTimer.stop();
            if (root.actions && root.actions.commitRemovals)
                root.actions.commitRemovals();
        }

        function undo() {
            removalBar.visible = false;
            removalTimer.stop();
            if (root.actions && root.actions.undoRemovals)
                root.actions.undoRemovals();
            grid.refreshKeepingScroll();
        }

        objectName: "removalBar"
        visible: false
        z: 100
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        width: Math.min(parent.width - 32, Math.max(removalBar.widest, barRow.implicitWidth) + 24)
        height: barRow.implicitHeight + 16
        radius: 4
        color: root.colCard
        border.color: root.colAccent
        border.width: 1

        HoverHandler {
            id: barHover
        }

        RowLayout {
            id: barRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 10
            onImplicitWidthChanged: removalBar.widest = Math.max(removalBar.widest, implicitWidth)

            Text {
                objectName: "removalBarText"
                Layout.fillWidth: true
                // «Тема «X» будет удалена. Через 8 секунд, если не нажать
                // «Вернуть».» — подписи ядра (`theme_removal`).
                text: removalBar.message + " "
                      + page.removalCountdownText(removalBar.remainingSeconds)
                textFormat: Text.PlainText
                color: root.colText
                elide: Text.ElideRight
            }

            RexonButton {
                id: undoButton
                objectName: "removalUndoButton"
                pal: root.pal
                text: page.removalUndoText + " (" + removalBar.remainingSeconds + ")"
                onClicked: removalBar.undo()
            }

            RexonButton {
                id: closeButton
                pal: root.pal
                tone: root.toneSecondary
                text: "❌"
                onClicked: removalBar.finish()
            }
        }

        // Полоса отсчёта: убывает к нулю за срок ядра.
        Rectangle {
            objectName: "removalCountdownBar"
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            anchors.leftMargin: 1
            anchors.bottomMargin: 1
            height: 3
            radius: 1
            color: root.colAccent
            width: Math.max(0, (parent.width - 2) * removalBar.remainingMs
                                / Math.max(1, page.removalUndoMs))
        }

        Timer {
            id: removalTimer
            interval: 100
            repeat: true
            onTriggered: {
                if (removalBar.paused)
                    return;
                removalBar.remainingMs -= interval;
                if (removalBar.remainingMs <= 0)
                    removalBar.finish();
            }
        }

        Component.onDestruction: removalBar.finish()
    }
}
