// Главное окно Rexon целиком на QML: хозяин — Rust, виджетов нет вовсе.
//
// Разница с продуктом — в том, КТО кому принадлежит. Живое окно строит
// Python: `QMainWindow` с `QTabWidget`, а QML-страницы лежат ВНУТРИ
// `QQuickWidget`. Здесь наоборот: окно — `ApplicationWindow`, страницы
// вкладываются в него напрямую, а данные приходят от Rust-объекта
// `MainWindowModel` (rust/rexon-gui/src/main_window.rs), который сам ничего
// не решает и спрашивает ядро (`rexon_core::qt_main`).
//
// ⚠️ Это СКЕЛЕТ отдельного входа для разработчика
// (`rust/target/release/rexon-main-window`), а не замена продукта. Живой
// Rexon запускается по-прежнему и этого файла не видит.
//
// Своих цветов окно не заводит: палитра приходит свойством `palette` от
// того же объекта и родом из `ui_base::active_qml_palette` — скина по
// умолчанию всего продукта (правило #17).
//
// Делегаты Repeater обращаются к id внешних элементов (root, shell, bar).
// Без этой прагмы проверка QML справедливо ругается на неквалифицированный
// доступ, а Qt разрешает такие имена лишь через устаревший контекст
// делегата.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

ApplicationWindow {
    id: root

    // ── Ядро окна ─────────────────────────────────────────────────────────

    MainWindowModel {
        id: shell
        // Имя нужно проверкам: снаружи объект достают
        // `findChild(QObject, "mainWindowModel")` и спрашивают его без
        // единой правки в этом файле.
        objectName: "mainWindowModel"
    }

    // Объект побочных действий вкладки «Рабочие столы». В продукте на его
    // месте стоит Python-класс `DesktopsActions`; здесь — Rust-объект с теми
    // же девятью методами и тем же сигналом `versionReady`. Все девять он
    // исполняет сам: установку, проверку, смену и удаление стола — через
    // исполнитель ядра (`rexon_core::system_de_exec`), с одним окном хода
    // внутри `DesktopsPage.qml` (карта `operation`, сигналы `operationLog`
    // и `desktopsChanged`; у Python-хозяина этих свойств нет, и окно там
    // просто не показывается). Подробности — в шапке
    // src/desktops_actions.rs.
    DesktopsActions {
        id: desktopsActions
        objectName: "desktopsActions"
    }

    // Объект побочных действий вкладки «Steam». В продукте на его месте
    // стоит Python-класс `SteamActions` (`rexon/qt_steam/qml_window.py`);
    // здесь — Rust-объект с теми же восемью методами, и `SteamPage.qml` от
    // этого не меняется ни на строку.
    //
    // ⚠️ Заводится ЗДЕСЬ, а не внутри `Component` страницы, по той же
    // причине, что и два соседа: до него должен дотянуться прогон
    // `--selftest`. Заведи его в компоненте — и проверить, что кнопки
    // вкладки кому-то отвечают, стало бы нечем.
    SteamActions {
        id: steamActions
        objectName: "steamActions"
    }

    // Меню окна («☰»). В продукте его строит `RexonMainWindow._build_menubar`
    // — четыре пункта, за которыми стоят сброс темы KDE, справка, удаление
    // Rexon и выход. Объект заводится ЗДЕСЬ, а не внутри разметки меню, по
    // той же причине, что и три соседа: до него должен дотянуться прогон
    // `--selftest`.
    WindowMenuModel {
        id: menuCore
        objectName: "windowMenuModel"
    }

    // ── Окно ──────────────────────────────────────────────────────────────

    title: shell.windowTitle
    width: shell.windowMinWidth
    height: shell.windowMinHeight
    minimumWidth: shell.windowMinWidth
    minimumHeight: shell.windowMinHeight
    visible: true
    color: root.colBg

    // Подсказки (`ToolTip.text` у кнопок всех вкладок) стиль Basic рисует
    // ролями `toolTipBase`/`toolTipText` палитры окна, а они у него светлые:
    // жёлто-белая плашка поверх тёмного окна — та же беда, что была у меню
    // «☰» (снимок 30.09.2026). Подсказок полсотни на шести вкладках, поэтому
    // краска ставится один раз здесь, на окне, а вкладки её наследуют.
    palette.toolTipBase: root.pal.bg_card
    palette.toolTipText: root.pal.text_primary

    readonly property var pal: shell.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colCard: root.pal.surface_card
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // Прогон без глаз: ставится начальным свойством из Rust
    // (`--selftest`). Смотри `runSelftest` внизу файла.
    property bool selftest: false

    // Раздел, открытый при запуске (`--section=<id>`). Пусто — первый.
    // Незнакомый идентификатор не молчит и не роняет окно: он остаётся на
    // первом разделе, а причина уходит в журнал.
    property string startSection: ""

    // Подраздел, открытый сразу при старте: `compare` — «Сравнение DE»
    // внутри «Рабочих столов» (`--page=compare` из ярлыка). Раздел гасит
    // просьбу, исполнив её: повторный заход в «Рабочие столы» обязан
    // показать сами столы, а не снова сравнение.
    property string startSubpage: ""

    // Каталог вендорного интерфейса вкладки обоев (`qml_ui/` пакета
    // `rexon/`). Приходит начальным свойством от `bin/main_window.rs`.
    property string weVendorQmlDir: ""

    // API всплывающих страниц вендора, который ставит на ОКНО шелл
    // `PageHost.qml` (`_installWindowApi`): вендорные страницы зовут
    // `Window.window.presentPopup(...)` и `Window.window.showChangelog()`.
    // У питоновского хозяина окно — чужой C++-объект, и JS дописывает эти
    // свойства в его обёртку сам. Здесь окно — этот корень, тип, объявленный
    // в QML, и новое свойство в него не дописать («Cannot assign to
    // non-existent property»): без объявления мертвы все всплывающие
    // страницы вкладки — фильтры, плейлисты, метки, «Библиотеки».
    property var popupPresenter: null
    property var presentPopup: null
    property var showChangelog: null

    /// Открыть раздел `id` (и подраздел `subpage`, `compare` — «Сравнение
    /// DE»). Одна дверь на стартовый раздел и на просьбу второго запуска
    /// (слушатель активации зовёт её через шим лотка). false — такого
    /// раздела нет, окно остаётся где было.
    function openSection(id, subpage) {
        const sections = shell.sections;
        let found = -1;
        for (let i = 0; i < sections.length; ++i)
            if (sections[i].id === id)
                found = i;
        if (found < 0)
            return false;
        // Просьба о подразделе ставится ДО переключения: только что
        // построенный раздел читает её при создании, уже построенный —
        // по сигналу смены (см. раздел «Рабочие столы»).
        root.startSubpage = subpage ? String(subpage) : "";
        bar.currentIndex = found;
        return true;
    }

    Component.onCompleted: {
        if (root.startSection !== ""
                && !root.openSection(root.startSection, root.startSubpage))
            console.warn("раздела «" + root.startSection + "» нет; "
                         + "открыт первый");
        if (root.selftest) {
            // Прогон обходит разделы по порядку и обязан начать с первого:
            // начни он с того, что назвали `--section=`, предыдущие разделы
            // остались бы непроверенными, а прогон — зелёным.
            bar.currentIndex = 0;
            selftestTimer.start();
        } else {
            // Самопроверка ни мастера установки, ни обучения не показывает:
            // вопрос накрыл бы окно, а ответ записался бы в настройки
            // человека.
            wizardTimer.start();
            // И сверку оформления тоже: её вопрос накрыл бы окно так же.
            aeroshellTimer.start();
        }
    }

    // ── Меню окна ─────────────────────────────────────────────────────────
    //
    // Именно `menuBar:`, а не `header:`: в шапке стоит полоса разделов, и
    // меню обязано жить над ней — там же, где его показывает продукт.

    menuBar: WindowMenu {
        id: menuBarItem
        objectName: "windowMenu"
        menuModel: menuCore
        pal: root.pal
        onItemActivated: (id) => menuDialogs.activate(id)
    }

    WindowMenuDialogs {
        id: menuDialogs
        objectName: "windowMenuDialogs"
        menuModel: menuCore
        pal: root.pal
        onQuitRequested: Qt.quit()
    }

    // ── Обучение при первом запуске ───────────────────────────────────────
    //
    // Сценарий, тексты и прогресс — у ядра; прогресс лежит в общем файле
    // настроек, поэтому прошедший обучение в Python-окне здесь его не увидит.

    OnboardingModel {
        id: onboardingCore
        objectName: "onboardingModel"
    }

    OnboardingOverlay {
        id: onboarding
        objectName: "onboardingOverlay"
        core: onboardingCore
        locate: (kind, tabId) => root.onboardingTarget(kind, tabId)
    }

    /// Обучение ждёт, пока окно покажут (запуск свёрнутым в лоток).
    property bool onboardingPending: false

    // Как у Python-окна: ПОСЛЕ первой вкладки. Вопрос поверх пустого окна
    // выглядит так, будто программа не загрузилась.
    //
    // ⚠️ Запускает этот таймер не показ окна, а КОНЕЦ МАСТЕРА УСТАНОВКИ
    // (`firstRun.onFinished`). Прежде обучение начиналось через 700 мс
    // после показа окна, и при первом запуске неустановленного Rexon два
    // вопроса встали бы друг на друга: карточка обучения поверх окна
    // установки. Мастер сам отвечает `finished()` сразу, когда предлагать
    // нечего, — тогда обучение начинается ровно как раньше.
    Timer {
        id: onboardingTimer
        interval: 700
        onTriggered: root.maybeStartOnboarding()
    }

    // ── Мастер первого запуска ────────────────────────────────────────────
    //
    // Решение «предлагать ли установку» и все подписи — у ядра; накладка
    // только спрашивает мост (`FirstRunWizardOverlay.begin()`). Обучение
    // ждёт её конца, а сверка оформления — конца их обоих.

    FirstRunWizardOverlay {
        id: firstRun
        objectName: "firstRunWizardOverlay"
        anchors.fill: parent
        pal: root.pal
        onFinished: root.maybeStartOnboarding()
    }

    /// Мастер ждёт, пока окно покажут (запуск свёрнутым в лоток).
    property bool wizardPending: false

    // Та же задержка, что у обучения, и по той же причине: вопрос поверх
    // пустого окна выглядит так, будто программа не загрузилась.
    Timer {
        id: wizardTimer
        interval: 700
        onTriggered: root.maybeStartWizard()
    }

    function maybeStartWizard() {
        if (!root.visible) {
            root.wizardPending = true;
            return;
        }
        root.wizardPending = false;
        firstRun.begin();
    }

    function maybeStartOnboarding() {
        if (!root.visible) {
            root.onboardingPending = true;
            return;
        }
        root.onboardingPending = false;
        const decision = onboardingCore.start();
        // В журнал: по нему видно, почему обучение показалось или нет.
        console.info("обучение при запуске: " + decision.kind
                     + (decision.step ? " " + decision.step : ""));
        onboarding.begin(decision);
    }

    onVisibleChanged: {
        if (root.visible && root.wizardPending)
            wizardTimer.restart();
        if (root.visible && root.onboardingPending)
            onboardingTimer.restart();
        if (root.visible && root.aeroshellPending)
            aeroshellTimer.restart();
    }

    // ── Оформление AeroShell отстало от системы ───────────────────────────
    //
    // Как у Python-окна (`rexon/qt_main/window.py`): сверка ПОСЛЕ обучения —
    // два окна разом при первом запуске перебор. Здесь только сверка и
    // вопрос; пересборка — исключительно нажатием «Восстановить» в окне.
    // Объект действий и окно — те же, что у кнопки «Проверить сейчас» в
    // «Настройках» (`settings_actions.rs`, исполнитель ядра `system_de_exec`).

    SettingsActions {
        id: aeroshellActions
        objectName: "aeroshellActions"
    }

    DeOperationDialog {
        anchors.fill: parent
        actions: aeroshellActions
        pal: root.pal
    }

    /// Сверка ждёт, пока окно покажут (запуск свёрнутым в лоток).
    property bool aeroshellPending: false

    Timer {
        id: aeroshellTimer
        interval: 1500
        onTriggered: root.maybeCheckAeroshell()
    }

    function maybeCheckAeroshell() {
        if (!root.visible) {
            root.aeroshellPending = true;
            return;
        }
        // Мастер установки или обучение ещё на экране — дождаться их конца:
        // вопрос поверх чужого окна читался бы как его продолжение.
        if (firstRun.opened || onboarding.opened) {
            aeroshellTimer.restart();
            return;
        }
        root.aeroshellPending = false;
        aeroshellActions.aeroshellStartupCheck();
    }

    /// Прямоугольник цели обучения в координатах окна либо null.
    /// "tab" — заголовок вкладки, "tab_content" — её содержимое (окно
    /// переключается на вкладку, как у Python-окна).
    function onboardingTarget(kind, tabId) {
        const sections = shell.sections;
        let index = -1;
        for (let i = 0; i < sections.length; ++i)
            if (sections[i].id === tabId)
                index = i;
        if (index < 0)
            return null;
        let item = null;
        if (kind === "tab") {
            item = bar.itemAt(index);
        } else if (kind === "tab_content") {
            if (bar.currentIndex !== index)
                bar.currentIndex = index;
            item = root.sectionHolder(index);
        }
        if (!item || !item.visible || item.width <= 0 || item.height <= 0)
            return null;
        const p = item.mapToItem(null, 0, 0);
        return { x: p.x, y: p.y, width: item.width, height: item.height };
    }

    // ── Полоса разделов ───────────────────────────────────────────────────

    header: TabBar {
        id: bar

        background: Rectangle {
            color: root.colCard
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: root.colBorder
            }
        }

        Repeater {
            model: shell.sections

            TabButton {
                id: tabButton
                required property int index
                required property var modelData

                text: tabButton.modelData.label
                // Высота ≥ 40 px — требование Big Picture (архитектурное
                // правило #16), а не вкусовое число.
                implicitHeight: 44
                font.pointSize: 11

                contentItem: Text {
                    text: tabButton.text
                    font: tabButton.font
                    color: bar.currentIndex === tabButton.index
                           ? root.colAccent : root.colText
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }

                // Плашка-«чип» у КАЖДОЙ вкладки, как в виджетном окне.
                //
                // ⚠️ Неактивная вкладка НЕ прозрачна. Прозрачной она была до
                // 18.09.2026, и полоса разделов выходила сплошной: у соседей
                // не было ни заливки, ни зазора, и не было видно, где
                // кончается одна вкладка и начинается следующая (замечание
                // Wehrwolfmann). Краски и отступы — из ядра
                // (`qt_main::TAB_*` через `shell.tabStyle`), общие с
                // виджетным окном; своих чисел разметка не заводит.
                background: Rectangle {
                    color: "transparent"

                    Rectangle {
                        id: tabChip
                        anchors.fill: parent
                        anchors.leftMargin: shell.tabStyle.marginH
                        anchors.rightMargin: shell.tabStyle.marginH
                        anchors.topMargin: shell.tabStyle.marginV
                        anchors.bottomMargin: shell.tabStyle.marginV
                        radius: shell.tabStyle.radius
                        color: bar.currentIndex === tabButton.index
                               ? root.colBg
                               : (tabButton.hovered ? shell.tabStyle.hoverFill
                                                    : shell.tabStyle.idleFill)
                        // Рамка сфокусированной кнопки: цепочка фокуса
                        // закладывается с самого начала, а не «когда дойдём до
                        // контроллера».
                        border.width: tabButton.activeFocus ? 2 : 0
                        border.color: root.colAccent

                        Rectangle {
                            anchors.bottom: parent.bottom
                            anchors.left: parent.left
                            anchors.right: parent.right
                            height: 2
                            color: bar.currentIndex === tabButton.index
                                   ? root.colAccent : "transparent"
                        }
                    }
                }
            }
        }
    }

    // ── Карта страниц: идентификатор → разметка ───────────────────────────
    //
    // Раньше на этом месте был гвоздь: `Loader.sourceComponent` смотрел
    // прямо в `comparePage`, а `wake()` сравнивал идентификатор с литералом
    // «compare» — открыться мог ровно один раздел из шести, остальные пять
    // показывали объяснение вместо страницы.
    //
    // Карта — единственное место, где окно знает, какая разметка стоит за
    // идентификатором страницы. КАКОЙ раздел её получает, окно не решает
    // вовсе: это таблица `SECTION_PAGES` в src/main_window.rs.
    //
    // Перечислены ВСЕ страницы, которые уже есть на QML, включая ту, что
    // таблица разделов не выбирает («compare»: сравнение — не раздел, а
    // вторая страница «Рабочих столов», и её кладёт держатель раздела ниже).
    // Карта — каталог доступного, а не вторая копия таблицы. Обратный
    // разъезд — страницу таблица назвала, а карта её не знает — ловит сторож
    // `every_page_is_in_the_qml_map`.
    readonly property var pageComponents: ({
        "compare": comparePage,
        "desktops": desktopsPage,
        "appearance": appearancePage,
        "extensions": extensionsPage,
        "wallpaper_engine": wallpaperEnginePage,
        "steam": steamPage,
        "settings": settingsPage
    })

    // ── Содержимое разделов ───────────────────────────────────────────────

    StackLayout {
        id: stack
        anchors.fill: parent
        currentIndex: bar.currentIndex

        Repeater {
            model: shell.sections

            Item {
                id: section
                required property int index
                required property var modelData

                /// Разметка этого раздела по карте окна либо null, если
                /// страницы у него нет. Здесь и кончается всё знание
                /// раздела о том, что он показывает.
                readonly property Component pageComponent:
                    root.pageComponents[section.modelData.page] ?? null

                /// Построенная страница раздела либо null — этим полем её
                /// достают проверки, не разбирая дерево наощупь.
                readonly property Item page: pageLoader.item as Item

                // Названная, но неизвестная карте страница не должна молча
                // превратиться в «раздел ещё не перенесён»: текст соврал бы
                // о причине, а причина — разъезд таблицы с картой.
                Component.onCompleted: {
                    if (section.modelData.page !== ""
                            && section.pageComponent === null)
                        console.warn("страница «" + section.modelData.page
                                     + "» раздела «" + section.modelData.id
                                     + "» не значится в карте pageComponents");
                }

                Loader {
                    id: pageLoader
                    anchors.fill: parent
                    // Страницы грузятся асинхронно и лениво — как в
                    // продукте: синхронная компиляция всех шести разделов
                    // легла бы целиком в старт окна.
                    asynchronous: true
                    active: false
                    visible: section.pageComponent !== null
                    sourceComponent: section.pageComponent

                    // Страница строится, когда её раздел открыли ПЕРВЫЙ раз,
                    // и дальше остаётся построенной: платить компиляцией за
                    // каждый возврат на вкладку покупатель не должен.
                    function wake() {
                        if (section.pageComponent !== null
                                && bar.currentIndex === section.index)
                            pageLoader.active = true;
                    }

                    Component.onCompleted: pageLoader.wake()

                    Connections {
                        target: bar
                        function onCurrentIndexChanged() { pageLoader.wake(); }
                    }
                }

                // Раздел, у которого Rust-хозяина ещё нет. Пустая страница
                // читается как поломка (правило #13), поэтому объяснение
                // приходит из ядра, а не рисуется пустотой.
                Label {
                    anchors.centerIn: parent
                    width: Math.min(parent.width - 80, 560)
                    visible: section.pageComponent === null
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    font.pointSize: 14
                    color: root.colTextSecondary
                    text: shell.notPortedNotice(section.modelData.id)
                }
            }
        }
    }

    // Страницы — ТЕ ЖЕ, что открывает живой Rexon, и ни одна из них не
    // знает, кто её хозяин. `Component` сам по себе ничего не строит, пока
    // его не попросили, поэтому семь объявлений стоят старту окна ровно
    // ноль: строится только та страница, чей раздел открыли.
    //
    // Объект побочных действий есть теперь у КАЖДОЙ страницы, которой он
    // нужен, и приходит к ней двумя разными путями — это не небрежность, а
    // разница в договоре самой страницы:
    //   * «Сравнение DE», «Рабочие столы» и «Steam» ждут его свойством
    //     `actions`, и подставляет его хозяин (объекты объявлены выше);
    //   * «Расширения» и «Wallpaper Engine» заводят его СЕБЕ САМИ
    //     (`ExtensionsActions { }`, `WeActions { }` прямо в .qml), поэтому
    //     здесь про них ничего и не сказано.
    // Пустой `actions` страницы по-прежнему переносят: каждая проверяет его
    // на null сама, и ни одна из-за него не падает.
    Component {
        id: comparePage

        ComparePage {
            // Тот же объект, что у «Рабочих столов»: установка и смена
            // стола идут одним исполнителем и одним окном хода.
            actions: desktopsActions
        }
    }

    // Раздел «Рабочие столы» — это ДВЕ страницы, ровно как в продукте:
    // витрина столов и встроенное «Сравнение DE» (отдельной вкладкой оно
    // перестало быть 08.07.2026). Виджетная вкладка держала их
    // `QStackedWidget`, здесь то же делает `StackLayout`.
    //
    // Вставить сравнение ВНУТРЬ сцены столов нельзя: это отдельная страница
    // со своей моделью. Поэтому переключает страницы хозяин, а сама вкладка
    // только просит об этом — сигналом `compareRequested` объекта действий.
    // Подпись кнопки возврата приходит из ядра (`compareBackText`), второй
    // копии буквы здесь нет.
    Component {
        id: desktopsPage

        Item {
            id: desktopsHolder

            // Заголовок раздела спрашивает прогон `--selftest`, и отдать его
            // обязана САМА страница столов: подставь сюда свою строку — и
            // проверка «хозяин окна → страница → модель» стала бы зелёной,
            // ничего не проверив.
            readonly property string windowTitle: desktopsView.windowTitle

            /// Какая из двух страниц раздела показана: 0 — столы, 1 —
            /// сравнение. Этим полем прогон проверяет, что просьба показать
            /// «Сравнение DE» куда-то ведёт.
            readonly property int subPage: desktopsStack.currentIndex

            /// Какой стол выбран и в каком сеанс: прогон требует, чтобы
            /// вкладка открывалась на текущем столе.
            readonly property string selectedDe: desktopsView.selectedDe
            readonly property string currentDe: desktopsView.currentDe

            StackLayout {
                id: desktopsStack
                anchors.fill: parent
                currentIndex: 0

                DesktopsPage {
                    id: desktopsView
                    actions: desktopsActions
                }

                ColumnLayout {
                    spacing: 0

                    RexonButton {
                        Layout.leftMargin: 12
                        Layout.topMargin: 10
                        Layout.bottomMargin: 4
                        pal: root.pal
                        text: desktopsView.compareBackText
                        onClicked: desktopsStack.currentIndex = 0
                    }

                    // Сравнение берётся из той же карты `pageComponents` и
                    // строится, только когда его попросили: пока человек не
                    // нажал «Сравнить», платить за компиляцию второй
                    // страницы раздел не должен.
                    Loader {
                        id: compareLoader
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        asynchronous: true
                        active: false
                        sourceComponent: root.pageComponents["compare"]
                    }
                }
            }

            function showCompare() {
                compareLoader.active = true;
                desktopsStack.currentIndex = 1;
            }

            Connections {
                target: desktopsActions

                function onCompareRequested() { desktopsHolder.showCompare(); }
            }

            // `--page=compare` (при старте либо от второго запуска): та же
            // дверь, что у кнопки «Сравнить». Просьба гасится исполненной.
            function takeSubpage() {
                if (root.startSubpage === "compare") {
                    root.startSubpage = "";
                    desktopsHolder.showCompare();
                }
            }

            Component.onCompleted: desktopsHolder.takeSubpage()

            Connections {
                target: root

                function onStartSubpageChanged() { desktopsHolder.takeSubpage(); }
            }
        }
    }

    Component {
        id: appearancePage

        AppearancePage {}
    }

    Component {
        id: extensionsPage

        ExtensionsPage {}
    }

    Component {
        id: wallpaperEnginePage

        // Каталог вендорного интерфейса waywallen называет хозяин процесса
        // (`we_env::vendor_qml_dir`, начальное свойство `weVendorQmlDir`).
        // Пусто — пакета `rexon/` рядом нет, и страница честно открывается
        // без вендорных страниц, как она и написана.
        WallpaperEnginePage {
            vendorQmlDir: root.weVendorQmlDir
        }
    }

    Component {
        id: steamPage

        SteamPage {
            actions: steamActions
        }
    }

    Component {
        id: settingsPage

        SettingsPage {}
    }

    // ── Подвал: что ответило побочное действие ────────────────────────────

    // Ответ ПОСЛЕДНЕГО нажатия, а не одного избранного объекта: объектов
    // побочных действий стало два, и «показываем только первого» означало бы,
    // что половина кнопок отвечает в пустоту — то есть выглядит поломкой
    // (правило #13).
    property string footerStatus: ""

    Connections {
        target: desktopsActions
        function onStatusChanged() { root.footerStatus = desktopsActions.status; }
    }

    footer: Rectangle {
        implicitHeight: root.footerStatus === "" ? 0 : 34
        visible: root.footerStatus !== ""
        color: root.colCard

        Rectangle {
            anchors.top: parent.top
            width: parent.width
            height: 1
            color: root.colBorder
        }

        Label {
            anchors.fill: parent
            anchors.leftMargin: 14
            anchors.rightMargin: 14
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            color: root.colTextSecondary
            font.pointSize: 10
            text: root.footerStatus
        }
    }

    // ── Самопроверка ──────────────────────────────────────────────────────

    // Стенд, который зеленеет всегда, ничего не проверяет: каждая проверка
    // названа поимённо, а провал печатается отдельной строкой
    // REXON-MAINWINDOW-FAIL и уносит код возврата в единицу.
    //
    // Прогон обходит ВСЕ разделы по очереди, а не только открытый. Пока
    // страница была одна, «текущий раздел построился» и «окно работает»
    // значили одно и то же; с шестью страницами такая проверка пропустила
    // бы пять — и разъезд карты с таблицей остался бы зелёным.
    //
    // Страница текущего раздела грузится АСИНХРОННО, и в первом же кадре её
    // ещё нет. Проверять сразу значило бы мерить не готовность страницы, а
    // скорость планировщика: первый прогон так и покраснел («страница
    // текущего раздела не построена»), хотя окно было исправно. Поэтому у
    // каждого раздела свой цикл ожидания.

    /// Накопленные провалы обхода; приговор по ним выносит `runSelftest`.
    property var selftestFailures: []

    Timer {
        id: selftestTimer
        interval: 100
        repeat: true
        property int ticks: 0
        onTriggered: {
            const holder = root.sectionHolder(bar.currentIndex);
            // Раздела без страницы ждать нечего — он готов сразу.
            const waiting = holder !== null && holder.pageComponent !== null
                            && !holder.page;
            // 40 попыток по 100 мс = 4 с на раздел, шесть разделов — 24 с.
            // Число подобрано под сторож бинаря (30 с): пересиди прогон
            // сторожа, и провал стал бы безымянным «окно не отчиталось»
            // вместо «страница раздела такого-то не построена». Замер обхода
            // на этой машине — 1,06 с на все шесть, запас двадцатикратный.
            // Не дождались — это уже провал, а не «подождём ещё»: страница
            // обязана строиться.
            if (waiting && selftestTimer.ticks++ < 40)
                return;
            selftestTimer.stop();
            selftestTimer.ticks = 0;
            root.checkSection();
        }
    }

    /// Держатель раздела по его номеру.
    ///
    /// Поиском, а не `stack.children[i]`: в детях StackLayout лежит ещё и
    /// сам Repeater, и номер ребёнка не обязан совпадать с номером раздела.
    /// Пока раздел был один, разницы не было видно — с шестью она стала бы
    /// молчаливой сверкой не той страницы.
    function sectionHolder(index) {
        for (let i = 0; i < stack.children.length; ++i)
            if (stack.children[i].index === index)
                return stack.children[i];
        return null;
    }

    /// Приговор по ОДНОМУ разделу; дальше прогон переходит к следующему.
    function checkSection() {
        const sections = shell.sections;
        const index = bar.currentIndex;
        const id = sections[index].id;
        const holder = root.sectionHolder(index);
        const page = holder ? holder.page : null;
        const wanted = holder !== null && holder.pageComponent !== null;

        if (holder === null)
            root.selftestFailures.push("раздел «" + id + "» не построен вовсе");
        else if (sections[index].page !== "" && !wanted)
            // Таблица разделов назвала страницу, а карта окна её не знает.
            // Без этой проверки прогон остался бы зелёным: раздел покажет
            // объяснение «ещё не перенесён», а оно выглядит как штатная
            // заглушка — то есть разъезд спрятался бы за исправным видом.
            root.selftestFailures.push("страницы «" + sections[index].page
                                       + "» раздела «" + id
                                       + "» нет в карте pageComponents");
        else if (wanted && !page)
            root.selftestFailures.push("страница раздела «" + id
                                       + "» не построена");

        // Заголовок от Rust-модели — сквозная проверка «хозяин окна →
        // страница → модель». Отдают его не все: у страницы Wallpaper Engine
        // своего заголовка нет вовсе, и требовать его значило бы выдумать
        // провал на ровном месте.
        const hasTitle = page !== null && page.windowTitle !== undefined;
        console.log("REXON-MAINWINDOW-PAGE " + id
                    + " | " + (page ? "построена" : (wanted ? "НЕТ" : "заглушка"))
                    + " | " + (hasTitle ? page.windowTitle : "—"));
        if (hasTitle && page.windowTitle === "")
            root.selftestFailures.push("страница раздела «" + id
                                       + "» не отдала заголовок от Rust-модели");

        if (index + 1 < sections.length) {
            bar.currentIndex = index + 1;
            selftestTimer.start();
        } else {
            // Разделы пройдены — очередь меню окна. Оно ОТКРЫВАЕТСЯ: пока
            // меню закрыто, у пунктов нет ширины, и подпись, которая не
            // влезла, ничем не отличается от влезшей.
            menuBarItem.openForCheck();
            menuLayoutTimer.start();
        }
    }

    // Меню разложится не в том же кадре, в каком открылось: ширину пунктам
    // даёт очередной проход раскладки. Мерить сразу значило бы мерить нули
    // и звать это «влезло».
    Timer {
        id: menuLayoutTimer
        interval: 50
        repeat: true
        property int ticks: 0
        onTriggered: {
            // 40 попыток по 50 мс = 2 с. Не разложилось за это время — это
            // уже провал, и он назовётся своим именем в `runSelftest`.
            if (!menuBarItem.laidOut() && menuLayoutTimer.ticks++ < 40)
                return;
            menuLayoutTimer.stop();
            menuLayoutTimer.ticks = 0;
            root.runSelftest();
        }
    }

    function runSelftest() {
        let failures = root.selftestFailures;
        const sections = shell.sections;
        console.log("REXON-MAINWINDOW-TITLE " + shell.windowTitle);
        console.log("REXON-MAINWINDOW-SECTIONS " + sections.length);
        let withPage = 0;
        for (let i = 0; i < sections.length; ++i) {
            console.log("REXON-MAINWINDOW-SECTION " + sections[i].id
                        + " | " + sections[i].label
                        + " | " + sections[i].page);
            if (sections[i].page !== "")
                ++withPage;
        }
        console.log("REXON-MAINWINDOW-PAGES " + withPage + "/" + sections.length);
        if (sections.length === 0)
            failures.push("состав разделов пуст");
        if (shell.windowTitle === "")
            failures.push("заголовок окна пуст");
        // Ради этого прогон и затевался: своя страница обязана быть у
        // КАЖДОГО раздела. Пока хоть один открывается заглушкой, окно на
        // Rust не может заменить окно на Python.
        if (withPage !== sections.length)
            failures.push("разделов без своей страницы: "
                          + (sections.length - withPage));

        // Побочное действие обязано ОТВЕТИТЬ, а не промолчать. Объект
        // действий «Рабочих столов» — он же у «Сравнения DE». Кнопка установки
        // ничего не запускает сама: она открывает окно вопроса (либо отказа
        // дистрибутива) и пишет в подвал, что ждёт ответа. Пустой подвал
        // значил бы, что кнопка промолчала, — покупатель принял бы это за
        // поломку. Ответа «Да» прогон не даёт, а открытое окно сразу
        // закрывает: ни пароля, ни пакетного менеджера здесь не бывает.
        desktopsActions.install("kde");
        console.log("REXON-MAINWINDOW-DESKTOPS-STATUS " + desktopsActions.status);
        if (desktopsActions.status === "")
            failures.push("побочное действие «Рабочих столов» ничего не сказало");
        desktopsActions.dismissOperation();

        // Тот же вопрос объекту действий «Steam». Спрашиваем ДВЕ вещи, и
        // обе — без единого следа на машине:
        //   * снимок витрины обязан быть картой со всеми шестью ключами,
        //     которые читает страница; отдай он `undefined` — витрина
        //     осталась бы пустой, а переключатели сами встали бы в «выкл»;
        //   * показ БЕЗ выбранного ролика обязан ответить словами. Пустой
        //     ответ страница понимает как «пошло» и пишет «▶ Проигрываю…»,
        //     то есть безмолвный отказ выглядел бы успехом.
        const snapshot = steamActions.scan();
        const missing = [];
        for (const key of ["movies", "current", "favorites",
                           "enabled", "rotate", "sound"])
            if (!snapshot || snapshot[key] === undefined)
                missing.push(key);
        console.log("REXON-MAINWINDOW-STEAM-SNAPSHOT "
                    + (snapshot ? Object.keys(snapshot).length : -1)
                    + " | пропущено: " + (missing.length ? missing.join(",") : "—"));
        if (missing.length)
            failures.push("снимок вкладки «Steam» без ключей: " + missing.join(","));

        // Третий вопрос — про корень Steam, и он про молчаливую пропажу.
        //
        // Страница берётся за фоновые задачи сама, в `Component.onCompleted`.
        // Пока корень приходил снаружи, от Python-хозяина, под этим хозяином
        // сказать его было НЕКОМУ — и секция «Steam-фоны профиля» оставалась
        // пустой навсегда, притом что вкладка выглядела рабочей. Теперь
        // страница ищет корень сама, а сверка требует, чтобы она знала ровно
        // то же, что находит объект действий: расходятся — значит снова
        // осталась без корня.
        const actionsRoot = steamActions.steamRoot();
        const pageHolder = root.sectionHolder(4);
        const pageRoot = pageHolder ? pageHolder.page : null;
        const knownRoot = pageRoot ? pageRoot.steamFoundPath : "";
        console.log("REXON-MAINWINDOW-STEAM-ROOT "
                    + (knownRoot === "" ? "—" : knownRoot)
                    + " | у объекта действий: "
                    + (actionsRoot === "" ? "—" : actionsRoot));
        if (knownRoot !== actionsRoot)
            failures.push("страница «Steam» не знает корня Steam: у неё «"
                          + knownRoot + "», а найден «" + actionsRoot + "»");

        const steamRefusal = steamActions.play("", false);
        console.log("REXON-MAINWINDOW-STEAM-PLAY " + steamRefusal);
        if (steamRefusal === "")
            failures.push("вкладка «Steam» промолчала об отказе показать заставку");

        // «Сравнение DE» больше не отдельный раздел, а вторая страница
        // «Рабочих столов». Не приведи просьба никуда — сравнение просто
        // исчезло бы из окна, и по виду вкладки этого не заметить.
        const desktopsHolder = root.sectionHolder(0);
        const desktopsRoot = desktopsHolder ? desktopsHolder.page : null;
        // Вкладка открывается на столе СЕАНСА (приёмка 10.10.2026 и снова
        // 20.10.2026: в MATE выбирался Cinnamon — первый в списке). Сеанс не
        // опознан — сверять не с чем.
        if (desktopsRoot) {
            console.log("REXON-MAINWINDOW-DESKTOPS-SELECTED " + desktopsRoot.selectedDe
                        + " | сеанс: " + (desktopsRoot.currentDe || "—"));
            if (desktopsRoot.currentDe !== ""
                    && desktopsRoot.selectedDe !== desktopsRoot.currentDe)
                failures.push("«Рабочие столы» открылись на «" + desktopsRoot.selectedDe
                              + "», а не на столе сеанса «" + desktopsRoot.currentDe + "»");
        }
        desktopsActions.openCompare();
        console.log("REXON-MAINWINDOW-DESKTOPS-SUBPAGE "
                    + (desktopsRoot ? desktopsRoot.subPage : -1));
        if (!desktopsRoot || desktopsRoot.subPage !== 1)
            failures.push("просьба показать «Сравнение DE» никуда не привела");

        // ── Меню окна ────────────────────────────────────────────────
        //
        // Полный список пунктов НИЧЕГО не доказывает: модель может быть
        // полна, а делегаты не построиться — и меню окажется пустым при
        // исправном ядре. Поэтому мерило здесь двойное: что сказало ядро и
        // сколько пунктов РЕАЛЬНО создано в `Menu`.
        const menuItems = menuCore.items;
        const wantShortcuts = menuItems.filter(e => e.shortcut !== "").length;
        console.log("REXON-MAINWINDOW-MENU " + menuItems.length
                    + " | построено: " + menuBarItem.builtItems
                    + " | разделителей: " + menuBarItem.builtSeparators
                    + " | клавиш: " + menuBarItem.builtShortcuts
                    + " из " + wantShortcuts
                    + " | кнопка: " + menuCore.buttonText);
        for (const e of menuItems)
            console.log("REXON-MAINWINDOW-MENU-ITEM " + e.id + " | " + e.label
                        + " | " + (e.shortcut === "" ? "—" : e.shortcut)
                        + " | " + (e.separatorBefore ? "черта выше" : "—")
                        + " | " + (e.enabled ? "доступен" : "НЕДОСТУПЕН"));
        if (menuItems.length === 0)
            failures.push("меню окна пусто");
        if (menuBarItem.builtItems !== menuItems.length)
            failures.push("пунктов меню построено " + menuBarItem.builtItems
                          + " из " + menuItems.length);
        if (menuBarItem.builtSeparators !== 1)
            failures.push("разделителей в меню построено "
                          + menuBarItem.builtSeparators + ", а нужен один");
        // Клавиша живёт отдельным объектом от пункта: не создайся он, меню
        // осталось бы целым на вид, а «Ctrl+Q» не работал бы вовсе.
        if (menuBarItem.builtShortcuts !== wantShortcuts)
            failures.push("горячих клавиш построено "
                          + menuBarItem.builtShortcuts + " из " + wantShortcuts);
        if (menuCore.buttonText === "")
            failures.push("кнопка меню без подписи — её не увидеть");

        // ⚠️ ШИРИНА ПУНКТА — ОТДЕЛЬНАЯ МЕРА, И ВОТ ПОЧЕМУ ОНА ЗДЕСЬ.
        // Счёт пунктов выше зелён и тогда, когда все четыре построены, а
        // читается вместо «🛟 Восстановить тему KDE» — «🛟 Восстановить
        // те…». Ровно это и прошло мимо прогона 18.09.2026: пункты были на
        // месте, а подпись — нет. Теперь меню открывается и меряется
        // ширина, и обрезанная подпись валит прогон.
        if (!menuBarItem.laidOut())
            failures.push("меню окна не разложилось — ширину подписей "
                          + "измерить нечем");
        console.log("REXON-MAINWINDOW-MENU-WIDTH " + menuBarItem.widthReport());
        const menuClipped = menuBarItem.clippedLabels();
        console.log("REXON-MAINWINDOW-MENU-CLIPPED "
                    + (menuClipped.length ? menuClipped.join(" | ") : "нет"));
        for (const c of menuClipped)
            failures.push("подпись пункта меню обрезана: " + c);
        // Меню обязано быть в скине окна: белое меню стиля Basic поверх
        // тёмного окна проходило все счёты выше (снимок 30.09.2026).
        const menuOffSkin = menuBarItem.offSkinParts();
        console.log("REXON-MAINWINDOW-MENU-SKIN "
                    + (menuOffSkin.length ? menuOffSkin.join(" | ") : "в скине"));
        for (const c of menuOffSkin)
            failures.push("меню окна не в скине: " + c);
        // Дальше идут окна пунктов, и открытое меню им только мешало бы:
        // фокус в окне удаления прогон проверяет нажатием клавиш.
        menuBarItem.closeAfterCheck();
        for (const e of menuItems) {
            if (e.label === "")
                failures.push("пункт меню «" + e.id + "» без подписи");
            if (!e.enabled)
                failures.push("пункт меню «" + e.id + "» недоступен");
        }

        // Тексты диалогов обязаны быть у ядра, а не собираться разметкой.
        // Пустой текст покупатель принял бы за поломку (правило #13).
        const about = menuCore.aboutHtml();
        console.log("REXON-MAINWINDOW-MENU-ABOUT " + menuCore.aboutTitle()
                    + " | " + about.length + " знаков");
        if (menuCore.aboutTitle() === "" || about.length === 0)
            failures.push("справка «О программе» пуста");

        const recover = menuCore.recoverPrompt();
        console.log("REXON-MAINWINDOW-MENU-RECOVER " + recover.title
                    + " | тема: " + recover.target);
        if (!recover.title || !recover.text || !recover.target)
            failures.push("вопрос о сбросе темы KDE неполон");

        // Окно удаления спрашивается ЦЕЛИКОМ, но ничего не удаляет: это
        // чтение файловой системы и опрос пакетного менеджера. Проверять
        // здесь само удаление нельзя — оно снесло бы Rexon с живой машины.
        const uninstall = menuCore.uninstallPrompt();
        const missingKeys = [];
        for (const key of ["title", "text", "informative", "files",
                           "keepLabel", "deleteLabel", "cancelLabel",
                           "defaultId"])
            if (uninstall[key] === undefined)
                missingKeys.push(key);
        console.log("REXON-MAINWINDOW-MENU-UNINSTALL "
                    + (uninstall.files ? uninstall.files.length : -1)
                    + " путей | по умолчанию: " + uninstall.defaultId
                    + " | пропущено: "
                    + (missingKeys.length ? missingKeys.join(",") : "—"));
        if (missingKeys.length)
            failures.push("окно удаления без ключей: " + missingKeys.join(","));
        if (uninstall.defaultId !== "cancel")
            failures.push("кнопка по умолчанию окна удаления не «Отмена»: "
                          + uninstall.defaultId);
        if (menuCore.busy())
            failures.push("удаление считает себя уже запущенным");

        // Пункт меню обязан КУДА-ТО вести, и проверять это надо у КАЖДОГО:
        // модель может быть полна, делегаты построены, а дверь за пунктом не
        // открыться — меню при этом выглядит исправным. «Выход» здесь не
        // трогаем намеренно: он закрыл бы окно посреди прогона.
        //
        // ⚠️ Живая система этим не задевается: справка только показывает
        // текст, вопрос о сбросе темы ждёт «Да», окно удаления — нажатия
        // кнопки. Ни один из трёх сам по себе ничего не делает.
        for (const e of menuItems) {
            if (e.id === "quit")
                continue;
            menuDialogs.activate(e.id);
            console.log("REXON-MAINWINDOW-MENU-OPENED " + e.id + " → "
                        + (menuDialogs.openDialog === ""
                           ? "никуда" : menuDialogs.openDialog));
            if (menuDialogs.openDialog !== e.id)
                failures.push("пункт меню «" + e.id + "» никуда не привёл");
            // Кнопки окна удаления подписываются в момент открытия. Пустая
            // подпись здесь — безымянная кнопка, за которой стоит снос
            // программы с машины покупателя.
            if (e.id === "uninstall") {
                const labels = menuDialogs.uninstallButtonTexts();
                console.log("REXON-MAINWINDOW-MENU-UNINSTALL-BUTTONS "
                            + labels.join(" | "));
                for (const t of labels)
                    if (t === "")
                        failures.push("кнопка окна удаления без подписи");
                // Подпись «сохранить» называет, что сохранится; обрезанная,
                // она прячет ровно это.
                const clipped = menuDialogs.uninstallClippedLabels();
                console.log("REXON-MAINWINDOW-MENU-UNINSTALL-CLIPPED "
                            + (clipped.length ? clipped.join(" | ") : "нет"));
                for (const c of clipped)
                    failures.push("подпись кнопки окна удаления обрезана: " + c);
                // Фокус на «Отмене» — та же защита от дурака, что
                // `setDefaultButton(Cancel)` в продукте. Нажатием её не
                // проверить: ошибись — и программа снесётся по-настоящему.
                console.log("REXON-MAINWINDOW-MENU-UNINSTALL-FOCUS "
                            + (menuDialogs.uninstallDefaultHasFocus()
                               ? "на «Отмене»" : "НЕ на «Отмене»"));
                if (!menuDialogs.uninstallDefaultHasFocus())
                    failures.push("в открытом окне удаления фокус не на "
                                  + "«Отмене» — случайный Enter снёс бы Rexon");
            }
            menuDialogs.closeOpenDialog();
        }

        // ── Обучение ─────────────────────────────────────────────────
        //
        // Только чтение: решение при запуске и ответы пишут настройки
        // человека, поэтому здесь не зовутся.
        const obTexts = onboardingCore.texts();
        const obFirst = onboardingCore.card("welcome");
        console.log("REXON-MAINWINDOW-ONBOARDING " + obTexts.askTitle
                    + " | " + (obFirst.title ?? "—"));
        if (!obTexts.askTitle || !obTexts.askYes || !obTexts.askNo)
            failures.push("вопрос обучения без текста");
        if (!obFirst.title || !obFirst.nextText)
            failures.push("первый шаг обучения без карточки");

        // ── Перевод: QML и Rust говорят на одном языке ──────────────
        //
        // Подписи из Rust и подписи QML переводит один и тот же каталог
        // .mo: QML спрашивает его через переводчик окна. Одна и та же
        // строка обязана выйти одинаковой с обеих сторон: разошлись —
        // значит переводчик QML не встал, и в переведённом окне остались
        // бы английские подписи QML.
        const qmlNext = qsTranslate("rexon", "Next");
        console.log("REXON-MAINWINDOW-L10N qml «" + qmlNext + "» | rust «"
                    + obFirst.nextText + "»");
        if (qmlNext !== obFirst.nextText)
            failures.push("QML и Rust перевели «Next» по-разному: «" + qmlNext
                          + "» против «" + obFirst.nextText + "»");

        for (const f of failures)
            console.log("REXON-MAINWINDOW-FAIL " + f);
        console.log("REXON-MAINWINDOW-DONE " + failures.length);
        Qt.exit(failures.length === 0 ? 0 : 1);
    }
}
