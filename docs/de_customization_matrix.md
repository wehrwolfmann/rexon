# Кастомизация рабочих столов Linux: разбор по окружениям

Полная карта того, чем оформляется каждый рабочий стол, куда это кладётся, чем включается и что нужно перезапустить. Таблица — рабочая основа универсального `.rxn`: типы из колонки «Тип `.rxn`» Rexon ставит и включает сам.

> Файл СОБИРАЕТСЯ из `rust/rexon-core/src/de_customization.rs`
> (тест `matrix_doc_is_up_to_date`). Править нужно матрицу, а не этот
> текст: обновление — `REXON_UPDATE_DOC=1 cargo test -p rexon-core
> --test de_customization`.

## Общий каталог (кладётся одинаково везде)

| Тип `.rxn` | Что это | Куда положить | Где работает | Чем включить |
|---|---|---|---|---|
| `theme-gtk` | Тема GTK (вид окон и виджетов GTK-приложений) | `~/.themes` | во всех окружениях (в Pantheon — с оговоркой) | Rexon применяет сам (кнопка «Применить») |
| `theme-icons` | Набор значков | `~/.local/share/icons` | во всех окружениях | Rexon применяет сам (кнопка «Применить») |
| `theme-cursors` | Указатели мыши | `~/.local/share/icons` | во всех окружениях | Rexon применяет сам (кнопка «Применить») |
| `theme-sounds` | Звуковая тема (звуки событий системы) | `~/.local/share/sounds` | во всех окружениях (в Тайловые и Wayfire (sway/Hyprland/niri/River) — с оговоркой) | Rexon применяет сам (кнопка «Применить») |
| `theme-kvantum` | Тема Kvantum (вид Qt-приложений) | `~/.config/Kvantum` | KDE Plasma, GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, LXQt, COSMIC, Тайловые и Wayfire (sway/Hyprland/niri/River) | Rexon применяет сам (кнопка «Применить») |
| `theme-login-screen` | Оформление экрана входа в систему | `~/.local/share/rexon/login-themes` | во всех окружениях | Rexon применяет сам (кнопка «Применить») |
| `theme-plymouth` | Заставка загрузки компьютера (Plymouth) | `~/.local/share/rexon/plymouth-themes` | во всех окружениях | Rexon применяет сам (кнопка «Применить») |
| `theme-conky` | Виджет Conky (часы, датчики на столе) | `~/.config/conky` | во всех окружениях | вручную: запуск conky с этим файлом настроек |
| `theme-bundle` | Комплект оформления (несколько тем одним пакетом) | `~/.local/share/rexon/bundles` | во всех окружениях | Rexon применяет сам (кнопка «Применить») |
| `wallpaper-pack` | Набор обоев | — (применяется на месте) | во всех окружениях | Rexon применяет сам (кнопка «Применить») |
| `wallpaper-video-adaptive` | Живые обои (видео под разрешение и язык) | `~/.local/share/rexon/wallpapers` | во всех окружениях (в Pantheon — с оговоркой) | Rexon применяет сам (кнопка «Применить») |
| `font-pack` | Набор шрифтов | `~/.local/share/fonts` | во всех окружениях | подхватывается автоматически |

Подводные камни:

* **Тема GTK** — Нужна ВЕЗДЕ, включая KDE: под Plasma живут Firefox, GIMP, Inkscape, а в системных настройках есть модуль «Оформление приложений GNOME/GTK» (kde-gtk-config). Оформление Budgie задаётся именно темой GTK. ⚠️ Pantheon: служба io.elementary.settings-daemon (AccentColorManager) при каждом старте, смене акцента и — при акценте «авто» — смене обоев пишет в org.gnome.desktop.interface gtk-theme свой io.elementary.stylesheet.<цвет>; выключателя у неё нет.
* **Иконки** — Каталог общий с курсорами: и то, и другое — «темы значков» по спецификации freedesktop.
* **Курсоры** — Кроме выбора в настройках окружения пишется ~/.icons/default — иначе приложения на чистом X11 и часть Wayland-клиентов остаются со старым курсором.
* **Звуки** — Каталог общий по спецификации freedesktop, оболочке звуковая тема не принадлежит: её берут демоны уведомлений и программы на libcanberra — ровно так же, как иконки и курсоры берут GTK- и Qt-приложения.
* **Kvantum** — Kvantum — движок стилей Qt (libkvantum.so в каталоге плагинов Qt), он красит ЛЮБОЕ Qt-приложение независимо от оболочки: под GNOME это VLC, qBittorrent, Krita, OBS. Разница между окружениями не в том, работает ли он, а в том, ЧЕМ выбирается стиль: KDE — widgetStyle в kdeglobals, LXQt — [Qt] style в lxqt.conf, остальные — QT_STYLE_OVERRIDE в ~/.config/environment.d. В Deepin раздела нет: переменная на весь сеанс отняла бы у программ DTK их стиль Chameleon.
* **Экран входа** — Экран входа рисует менеджер входа, а не рабочий стол: Rexon сам определяет, что стоит (SDDM, GDM или LightDM), и просит пароль только на запись в системный каталог.
* **Загрузочный экран** — Показывается до входа в систему и от рабочего стола не зависит вообще; установка требует пароля и пересборки образа ядра.
* **Виджет Conky** — Conky — отдельная программа, а не часть рабочего стола, поэтому работает и там, где своих виджетов нет.
* **Полная тема** — Внутри — обычные типы этой же таблицы; каждый ставится своим путём, а негодные для текущего окружения пропускаются.
* **Обои** — Своего каталога установки нет: обои применяются прямо из пакета — их ставит движок обоев Rexon, одинаково во всех окружениях.
* **Adaptive обои (v4)** — Рисует движок обоев Rexon, а не рабочий стол, поэтому работает и в GNOME с Cinnamon (через наше расширение оболочки), и в тайловых (слой layer-shell). Pantheon на Wayland — нет: Gala знает только свой протокол pantheon-desktop-shell, wlr-layer-shell у неё нет, а модулей-расширений она не грузит (`wallpaper_backend::pick` → Unsupported).

Работает, но с оговоркой:

* **Тема GTK** (Pantheon) — Собственная служба настроек Pantheon возвращает стиль elementary поверх темы GTK: при каждом входе в систему, при смене акцентного цвета и — если акцент выбран автоматически — при каждой смене обоев. До тех пор ваша тема остаётся.
* **Звуки** (Тайловые и Wayfire (sway/Hyprland/niri/River)) — Звуки событий играет не оболочка, а демон уведомлений (swaync, mako, dunst) или сама программа. В тайловых композиторах такой демон запускают вручную — пока его нет, тема будет молчать.
* **Kvantum** (GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, COSMIC, Тайловые и Wayfire (sway/Hyprland/niri/River)) — Тему увидят все Qt-приложения (VLC, qBittorrent, Krita, OBS). Своего переключателя стиля Qt в этом рабочем столе нет, Rexon настраивает его сам — но изменения появятся только после следующего входа в систему.
* **Adaptive обои (v4)** (Pantheon) — Живые обои работают в «Классическом сеансе» Pantheon (X11). В «Защищённом сеансе» (Wayland) оконный менеджер Gala не даёт другим программам рисовать фон рабочего стола, поэтому там можно поставить только неподвижную картинку.
* **KDE Global Theme** (GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, Deepin, LXQt, COSMIC, Тайловые и Wayfire (sway/Hyprland/niri/River)) — Глобальная тема — пакет Plasma. Здесь Rexon применяет её по частям — цвета, оформление приложений, значки, курсоры, шрифты и кнопки заголовка; окно перед применением говорит, что переносится.
* **KDE Цвета** (GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, LXQt, COSMIC, Тайловые и Wayfire (sway/Hyprland/niri/River)) — Вне Plasma Rexon передаёт схему программам KDE (Dolphin, Kate, Okular) и остальным программам Qt — часть из них подхватит её только после следующего входа в систему. Программы GTK (Firefox, GIMP) и большинство собственных программ рабочего стола цветовые схемы KDE не читают и остаются со своими цветами — для программ GTK подберите подходящую тему в разделе «GTK-темы».

## KDE Plasma

| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |
|---|---|---|---|---|
| `theme-kde-global` | Полная тема Plasma (Global Theme) | `~/.local/share/plasma/look-and-feel` | Rexon применяет сам (кнопка «Применить») | ничего, применяется сразу |
| `theme-kde-colors` | Цветовая схема | `~/.local/share/color-schemes` | Rexon применяет сам (кнопка «Применить») | ничего, применяется сразу |
| `theme-kde-plasma` | Стиль Plasma (панель, меню, всплывающие окна) | `~/.local/share/plasma/desktoptheme` | Rexon применяет сам (кнопка «Применить») | ничего, применяется сразу |
| `theme-window-decoration` | Оформление рамок окон (Aurorae) | `~/.local/share/aurorae/themes` | Rexon применяет сам (кнопка «Применить») | qdbus org.kde.KWin /KWin reconfigure |
| `theme-plasmoid` | Виджет Plasma (плазмоид) | `~/.local/share/plasma/plasmoids` | вручную: добавить виджет на панель или рабочий стол | ничего, применяется сразу |
| `theme-kde-splash` | Заставка загрузки сеанса (KSplash) | `~/.local/share/plasma/look-and-feel` | kwriteconfig6 --file ksplashrc --group KSplash --key Theme <имя> (+ Engine=KSplashQML) | следующий вход в систему |
| `theme-kde-lockscreen` | Экран блокировки | `~/.local/share/plasma/look-and-feel` | kwriteconfig6 --file kscreenlockerrc --group Greeter --key Theme <имя> | следующая блокировка экрана |
| `kde-kwin-script` | Скрипт KWin (поведение окон, тайлинг, жесты) | `~/.local/share/kwin/scripts` | kwriteconfig6 --file kwinrc --group Plugins --key <id>Enabled true | qdbus org.kde.KWin /KWin reconfigure |
| `kde-kwin-effect` | Эффект рабочего стола KWin | `~/.local/share/kwin/effects` | kwriteconfig6 --file kwinrc --group Plugins --key <id>Enabled true | qdbus org.kde.KWin /KWin reconfigure |
| `kde-window-switcher` | Переключатель окон (Alt+Tab) | `~/.local/share/kwin/tabbox` | kwriteconfig6 --file kwinrc --group TabBox --key LayoutName <имя> | qdbus org.kde.KWin /KWin reconfigure |
| `kde-wallpaper-plugin` | Плагин обоев Plasma (свой движок фона) | `~/.local/share/plasma/wallpapers` | вручную: правой кнопкой по столу → «Настроить рабочий стол» → тип обоев | plasmashell --replace |

Подводные камни:

* **KDE Global Theme** — Меняет всё сразу — цвета, стиль Plasma, значки, заставку и экран блокировки; поэтому перед применением Rexon делает снимок панели (правило #13).
* **KDE Цвета** — В пакете лежат отдельные файлы .colors, а не папки. Вне Plasma Rexon вливает цвета схемы в ~/.config/kdeglobals теми же ключами, что Plasma, плюс [UiSettings] ColorScheme (иначе программы KDE с меню «Цветовая схема» сами ставят Breeze), и палитру Qt: LXQt — [Palette] в lxqt.conf, остальные — qt6ct/qt5ct colors/*.conf. GTK-программы схему KDE не читают.
* **Декорации окон** — Aurorae — движок рамок именно KWin; в других окружениях рамки рисуются своими средствами (xfwm4, Marco, GTK).
* **Plasma-виджет** — Роль плазмоидов в GNOME играют расширения оболочки, в Cinnamon — апплеты и десклеты: это ДРУГИЕ типы, не совместимые между собой.
* **Заставка KDE** — Отдельного типа пакета у KDE нет: заставка — это look-and-feel с одним каталогом contents/splash.
* **Экран блокировки** — Своей категории у экрана блокировки в KDE Store тоже нет — он всегда часть look-and-feel (contents/lockscreen).
* **Скрипт KWin** — Имя ключа в kwinrc — не имя папки, а KPlugin.Id из metadata.json.
* **Эффект KWin** — Эффекты на QML грузятся без сборки; эффекты на C++ — нативные плагины, они через .rxn не раздаются.
* **Плагин обоев Plasma** — Выбор плагина хранится в конфигурации КАЖДОГО контейнера (plasma-org.kde.plasma.desktop-appletsrc), автоматически переключать его небезопасно — можно снести настройку стола.

## GNOME

| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |
|---|---|---|---|---|
| `gnome-shell-extension` | Расширение оболочки GNOME | `~/.local/share/gnome-shell/extensions` | gnome-extensions enable <uuid> | Alt+F2 → r на X11; на Wayland — перезаход |
| `theme-gnome-shell` | Тема оболочки GNOME (верхняя панель, меню) | `~/.themes` | gsettings set org.gnome.shell.extensions.user-theme name '<имя>' | ничего, применяется сразу |

Подводные камни:

* **Расширение GNOME** — Каталог обязан называться ровно как uuid. Схемы настроек нужно скомпилировать (glib-compile-schemas schemas/). Если версии оболочки нет в shell-version, расширение молча не грузится.
* **Тема оболочки GNOME** — Работает только при включённом расширении User Themes (user-theme@gnome-shell-extensions.gcampax.github.com) — без него ключ есть, а тема не применяется.

Через `.rxn` не раздаётся:

* **Тема экрана входа GDM** (`/usr/share/gnome-shell/gnome-shell-theme.gresource`) — тема GDM вшита в бинарный gresource: нужно распаковать системный файл, подменить стили, собрать обратно glib-compile-resources и положить в /usr/share от root. Обновление gnome-shell затирает подменённый файл — это ломает экран входа до следующей переустановки темы.

## Cinnamon

| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |
|---|---|---|---|---|
| `cinnamon-applet` | Апплет панели Cinnamon | `~/.local/share/cinnamon/applets` | добавить «panel1:<зона>:<порядок>:<uuid>:<номер>» в список org.cinnamon enabled-applets (+ счётчик next-applet-id) | cinnamon --replace (Wayland — перезаход) |
| `cinnamon-desklet` | Десклет (виджет на рабочем столе) | `~/.local/share/cinnamon/desklets` | добавить «<uuid>:<номер>:<x>:<y>» в список org.cinnamon enabled-desklets (+ счётчик next-desklet-id) | cinnamon --replace (Wayland — перезаход) |
| `cinnamon-extension` | Расширение Cinnamon | `~/.local/share/cinnamon/extensions` | добавить «<uuid>» в список org.cinnamon enabled-extensions | cinnamon --replace (Wayland — перезаход) |
| `theme-cinnamon` | Тема оболочки Cinnamon | `~/.themes` | gsettings set org.cinnamon.theme name '<имя>' | ничего, применяется сразу |
| `nemo-action` | Действие в контекстном меню файлового менеджера Nemo | `~/.local/share/nemo/actions` | подхватывается автоматически | ничего, применяется сразу |

Подводные камни:

* **Апплет Cinnamon** — У каждого экземпляра свой номер; счётчик next-applet-id нужно увеличить, иначе следующий апплет получит тот же номер и настройки двух апплетов слипнутся.
* **Десклет Cinnamon** — В строке хранятся координаты на столе — ставим в левый верхний угол со смещением, чтобы десклет не оказался под панелью.
* **Расширение Cinnamon** — Апплеты, десклеты и расширения Cinnamon — ТРИ разных типа с разными каталогами и разными списками включения.
* **Тема Cinnamon** — cinnamon.css несовместим с gnome-shell.css: одинаковых имён классов там почти нет, тема GNOME в Cinnamon даёт голый вид.
* **Действие Nemo** — Nemo следит за каталогом — действие появляется без перезапуска.

## XFCE

| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |
|---|---|---|---|---|
| `theme-xfwm4` | Тема оформления окон xfwm4 | `~/.themes` | xfconf-query -c xfwm4 -p /general/theme -s <имя> | ничего, применяется сразу |

Подводные камни:

* **Окна XFCE (xfwm4)** — Тема окон XFCE отдельна от темы GTK: их выбирают двумя разными настройками, и .rxn может нести обе.

Через `.rxn` не раздаётся:

* **Плагин панели XFCE** (`/usr/lib/xfce4/panel/plugins`) — плагин панели XFCE — нативная библиотека .so под конкретную версию xfce4-panel; её нужно компилировать и класть в системный каталог. Исключение — внешние плагины-скрипты (genmon, generic monitor): это не плагин, а настройка уже установленного плагина.

## MATE

| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |
|---|---|---|---|---|
| `theme-marco` | Тема оформления окон Marco | `~/.themes` | gsettings set org.mate.Marco.general theme '<имя>' | ничего, применяется сразу |

Подводные камни:

* **Окна MATE (Marco)** — Marco — форк Metacity, поэтому каталог темы называется metacity-1; те же темы работают в MATE и в старом GNOME 2. Cinnamon их с версии 5.4 не читает: рамки Muffin берёт из темы GTK.

Через `.rxn` не раздаётся:

* **Апплет панели MATE** (`/usr/share/mate-panel/applets`) — апплету MATE нужны описатель .mate-panel-applet и служба D-Bus в /usr/share/dbus-1/services — оба каталога системные, от root.

## Budgie

Через `.rxn` не раздаётся:

* **Апплет панели Budgie** (`/usr/lib/budgie-desktop/plugins`) — апплеты Budgie — плагины libpeas, собранные под версию budgie-desktop; каталог системный. Оформление Budgie задаётся темой GTK — она через .rxn ставится (тип «Тема GTK»).

## LXQt

| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |
|---|---|---|---|---|
| `theme-lxqt` | Тема LXQt (стили панели и оболочки) | `~/.local/share/lxqt/themes` | .config/lxqt/lxqt.conf: [General] theme=<имя> | перезапуск lxqt-panel |
| `theme-openbox` | Тема рамок окон Openbox и labwc | `~/.themes` | узел theme/name = <имя> в openbox/rc.xml или labwc/rc.xml + --reconfigure | ничего, применяется сразу |

Подводные камни:

* **Тема LXQt** — В каталоге темы обязателен lxqt-panel.qss — без него панель остаётся в старом стиле.
* **Окна Openbox и labwc** — Тема рамок Openbox — themerc и рисунки кнопок .xbm в openbox-3; labwc понимает тот же формат. Свой rc.xml заменяет системный целиком, поэтому без своего он заводится копией системного.

Через `.rxn` не раздаётся:

* **Плагин панели LXQt** (`/usr/lib/lxqt-panel`) — плагин панели LXQt — библиотека C++ плюс .desktop-описатель, собирается под версию lxqt-panel.

## COSMIC

Через `.rxn` не раздаётся:

* **Тема COSMIC** (`.config/cosmic`) — формат тем COSMIC (файлы RON в ~/.config/cosmic) ещё меняется от выпуска к выпуску — фиксировать его в .rxn рано. Обои и значки COSMIC мы ставим уже сейчас — родными ключами: картинку в com.system76.CosmicBackground/v1/all (её рисует cosmic-bg), набор значков в com.system76.CosmicTk/v1/icon_theme.

## Тайловые и Wayfire (sway/Hyprland/niri/River)

Через `.rxn` не раздаётся:

* **Конфигурация тайлового композитора** (`.config`) — у sway/Hyprland/niri оформление задаётся личным файлом настроек, а не устанавливаемым пакетом: подменять его пакетом — значит затирать чужие настройки и оставлять пользователя без рабочего стола. Обои и панель здесь ставятся отдельно (layer-shell), это работает независимо от конфига.
