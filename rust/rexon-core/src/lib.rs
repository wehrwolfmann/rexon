//! rexon-core — ядро Rexon на Rust (без Qt).
//!
//! Порт логики из прежнего Python-пакета `rexon/`. Каждый модуль
//! сверяется побайтово с оригиналом на Python (golden-тесты).

pub mod accent;
pub mod aeroshell;
pub mod animations;
pub mod app;
/// Значки в меню и на кнопках, подписи панелей инструментов — во всех окружениях.
pub mod app_widgets;
pub mod appdefaults;
pub mod appdefaults_exec;
pub mod appearance_cache;
pub mod appearance_installer;
pub mod appearance_page;
/// Журнал Rexon: файл с ротацией + копия в поток ошибок, уровни, `REXON_LOG`.
pub mod applog;
pub mod audio_capture;
pub mod aur;
pub mod aur_install;
pub mod aur_trust;
pub mod axon_extract;
pub mod axon_key;
pub mod axon_library;
pub mod axon_prepare;
pub mod backup;
pub mod categories;
pub mod cinnamon_spices;
// Точка входа: маршруты командной строки на живой машине, журнал сообщений
// Qt, каталоги переводов самой Qt.
pub mod cli;
pub mod color_scheme_edit;
pub mod compliance;
/// Автозапуск при входе в композиторах без XDG autostart (строка в их конфиге).
pub mod compositor_autostart;
pub mod constants;
/// Опасные директивы виджетов Conky: вопрос перед первым запуском, разрешение по отпечатку файлов.
pub mod conky_safety;
/// Автозапуск виджета Conky: задержка, окружение, строки композиторов.
pub mod conky_autostart;
pub mod conky_log;
/// Настройки виджета Conky поверх файла автора (накладка `rexon.conf`).
pub mod conky_overlay;
pub mod conky_preview;
pub mod conky_procs;
pub mod conky_scan;
pub mod conky_sets;
/// Светлое и тёмное оформление по расписанию.
pub mod day_night;
pub mod de_apply;
pub mod de_customization;
/// Deepin (DDE): служба оформления на шине сеанса и DConfig — куда Deepin кладёт настройки.
pub mod dde;
pub mod de_progress;
pub mod desktop_shell;
pub mod defaults;
/// Deepin на Treeland — единственный Deepin, который ставит Rexon: своя сборка deepin-session без deepin-kwin.
pub mod deepin_treeland;
pub mod distro_compat;
pub mod engine_update;
pub mod exit_guard;
pub mod ext_api;
pub mod ext_exec;
pub mod ext_manager;
pub mod ext_utils;
pub mod extension;
pub mod favorites;
pub mod fs_watch;
/// «Сохранить текущее оформление…» — своя глобальная тема из того, что стоит сейчас.
pub mod global_save;
pub mod gtk_sanitizer;
pub mod gtk4_theme;
pub mod i18n;
pub mod imp_live;
pub mod importers;
pub mod distro_looks;
pub mod install_journal;
pub mod install_plan;
pub mod installed_scan;
pub mod installer;
/// Встроенные рамки окон KDE (Breeze, Oxygen, темы QML Aurorae).
pub mod kdecoration;
/// Раскладка клавиатуры в конфигах тайловых столов и Wayfire.
pub mod keyboard_layout;
/// Исполнитель установки Rexon в систему (порт `install_to_system`).
pub mod installer_exec;
pub mod launch_feedback;
pub mod local_install;
pub mod local_preview;
pub mod login_rescue;
pub mod manifest_v4;
pub mod mate_panel;
pub mod models;
pub mod net;
pub mod ocs;
pub mod ocs_store;
pub mod onboarding;
pub mod plugin_journal;
pub mod plasma_login;
pub mod plugin_l10n;
pub mod privilege;
pub mod proto;
pub mod qml_modules;
pub mod qt_app;
pub mod qt_compare;
pub mod qt_desktops;
pub mod qt_extensions;
pub mod qt_main;
pub mod qt_settings;
pub mod qt_steam;
pub mod remote_l10n;
pub mod renderer_watch;
/// Пакет Arch, ждущий пересборки против новой библиотеки: признак и сборка из официальных исходников.
pub mod repo_rebuild;
pub mod rexon_bridge;
pub mod ruffle_manager;
pub mod rxn_build;
pub mod rxn_manifest;
pub mod rxn_v4_installer;
pub mod screen_effects;
pub mod screenshots;
/// Мост «не гасить экран»: org.freedesktop.ScreenSaver там, где его нет.
pub mod screensaver_bridge;
/// Сторож перед сеансом Plasma: вход без AeroThemePlasma, когда оно разошлось с Plasma.
pub mod session_guard;
/// Сторож сеанса Plasma: падения KWin и plasmashell из-за AeroThemePlasma сразу после входа.
pub mod session_watch;
pub mod session_splash;
pub mod settings;
/// Палитра в обе стороны между Rexon и чужим шеллом (Noctalia, DankMaterialShell).
pub mod shell_palette;
/// Связка «тайловый композитор + оболочка» (Noctalia, DankMaterialShell).
pub mod shell_bundle;
/// Один Rexon на сессию: замок второго запуска и слушатель «подними окно».
pub mod single_instance;
/// Прослушивание темы звуков: пять звуков, поиск по спецификации, Магазин до установки.
pub mod sound_preview;
pub mod splash_exec;
pub mod steam_profile;
pub mod steam_profile_items;
/// Витрины профиля Steam: нарезка своей картинки и макет предпросмотра (без записи в Steam).
pub mod steam_showcase;
pub mod steam_screensaver;
pub mod steam_token;
pub mod steamos_sysext;
pub mod system;
pub mod system_de_exec;
pub mod system_exec;
pub mod system_l10n;
pub mod system_plan;
pub mod theme_removal;
pub mod theme_watch;
pub mod themes;
pub mod translator;
/// Живой автоперевод: движок, поиск модели и склейка «текст → перевод».
pub mod translator_engine;
/// Значок Rexon в системном лотке и автозапуск при входе.
pub mod tray;
pub mod ui_base;
pub mod update_guard;
pub mod upscaler;
pub mod user_props;
pub mod utils;
pub mod wallpaper;
pub mod wallpaper_audio;
pub mod wallpaper_backend;
pub mod wallpaper_engine;
pub mod wallpaper_exec;
pub mod wallpaper_mmap;
pub mod waywallen_client;
pub mod we_client;
pub mod we_manage;
pub mod we_notify;
pub mod we_parity;
pub mod we_query;
pub mod we_settings;
pub mod we_thumbnail;
pub mod we_util;
pub mod we_wallpaper;
pub mod we_window;
pub mod waywallen_kde_plugin;
pub mod wincursor;
pub mod window_buttons;
pub mod window_menu;
// Снимок настоящих виджетов в стиле Qt и теме GTK для плиток «Оформления».
pub mod widget_shot;
pub mod youtube_wallpaper;
