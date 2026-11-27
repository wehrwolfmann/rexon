//! rexon-core — the Rust core of Rexon (without Qt).
//!
//! A port of the logic of the former Python package `rexon/`. Each module is
//! checked byte for byte against the Python original (golden tests).

pub mod accent;
pub mod aeroshell;
pub mod animations;
pub mod app;
/// Icons in menus and on buttons, toolbar labels — in every environment.
pub mod app_widgets;
pub mod appdefaults;
pub mod appdefaults_exec;
pub mod appearance_cache;
pub mod appearance_installer;
pub mod appearance_page;
/// The Rexon log: a rotated file + a copy to stderr, levels, `REXON_LOG`.
pub mod applog;
pub mod audio_capture;
pub mod aur;
pub mod aur_install;
pub mod aur_trust;
pub mod axon_extract;
pub mod axon_key;
pub mod axon_library;
pub mod axon_prepare;
pub mod axon_window;
pub mod backup;
pub mod categories;
pub mod cinnamon_spices;
// Entry point: command-line routes on the live machine, the Qt message log,
// Qt's own translation catalogs.
pub mod cli;
pub mod color_scheme_edit;
pub mod compliance;
/// Autostart on login in compositors without XDG autostart (a line in their config).
pub mod compositor_autostart;
pub mod constants;
/// Dangerous Conky widget directives: a question before the first run, approval by file fingerprint.
pub mod conky_safety;
/// Conky widget autostart: delay, environment, compositor lines.
pub mod conky_autostart;
pub mod conky_layout;
/// Palette of colors found in a Conky widget: replacing them everywhere and reverting.
pub mod conky_colors;
pub mod conky_log;
/// Conky widget settings on top of the author's file (the `rexon.conf` overlay).
pub mod conky_overlay;
pub mod conky_preview;
pub mod conky_procs;
pub mod conky_scan;
pub mod conky_sets;
pub mod conky_theme;
/// Light and dark appearance on a schedule.
pub mod day_night;
pub mod de_apply;
pub mod de_customization;
/// Deepin (DDE): the appearance service on the session bus and DConfig — where Deepin keeps its settings.
pub mod dde;
pub mod de_progress;
pub mod desktop_shell;
pub mod defaults;
/// Deepin on Treeland — the only Deepin Rexon installs: its own deepin-session build without deepin-kwin.
pub mod deepin_treeland;
pub mod dm_autologin;
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
/// "Save current appearance…" — a custom global theme from what is set right now.
pub mod global_save;
/// The GNOME extensions site (extensions.gnome.org): storefront, releases, compatibility.
pub mod gnome_ego;
/// The "GNOME Extensions" section: what the shell has installed, tile actions, updates.
pub mod gnome_ext_section;
/// GNOME extensions through the shell itself: D-Bus org.gnome.Shell.Extensions and the zip fallback.
pub mod gnome_shell_ext;
pub mod gtk_sanitizer;
pub mod gtk4_theme;
pub mod i18n;
/// Idle screensaver on the desktop: per-environment route, inhibitors, lock timers.
pub mod idle_screensaver;
pub mod imp_live;
pub mod importers;
pub mod distro_looks;
pub mod icon_hygiene;
pub mod install_journal;
pub mod install_plan;
pub mod installed_scan;
pub mod installer;
/// Built-in KDE window decorations (Breeze, Oxygen, Aurorae QML themes).
pub mod kdecoration;
/// Keyboard layout in the configs of tiling desktops and Wayfire.
pub mod keyboard_layout;
/// The executor that installs Rexon into the system (port of `install_to_system`).
pub mod installer_exec;
pub mod launch_feedback;
/// Links from foreign text: our own card, a question before an external site, trusted sites.
pub mod link_policy;
pub mod local_install;
pub mod local_preview;
pub mod login_rescue;
pub mod manifest_v4;
pub mod mate_panel;
pub mod models;
pub mod net;
pub mod notifications_slot;
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
pub mod main_window_tour;
pub mod qt_settings;
pub mod qt_steam;
pub mod remote_l10n;
pub mod renderer_watch;
/// An Arch package waiting to be rebuilt against a new library: the sign and the build from official sources.
pub mod repo_rebuild;
pub mod rexon_bridge;
pub mod ruffle_manager;
pub mod rxn_build;
pub mod rxn_manifest;
pub mod rxn_v4_installer;
pub mod screen_effects;
pub mod screenshots;
/// "Keep the screen on" bridge: org.freedesktop.ScreenSaver where it is missing.
pub mod screensaver_bridge;
/// Guard before a Plasma session: logging in without AeroThemePlasma when it has drifted from Plasma.
pub mod session_guard;
/// Game-mode guard (gamescope-session-plus): the display map before login, a refusal instead of a black screen.
pub mod gamemode_guard;
/// Plasma session guard: KWin and plasmashell crashes caused by AeroThemePlasma right after login.
pub mod session_watch;
pub mod session_splash;
pub mod settings;
/// Palette both ways between Rexon and a foreign shell (Noctalia, DankMaterialShell).
pub mod shell_palette;
/// The "tiling compositor + shell" pairing (Noctalia, DankMaterialShell).
pub mod shell_bundle;
/// One Rexon per session: the second-launch lock and the "raise the window" listener.
pub mod single_instance;
/// Previewing a sound theme: five sounds, lookup per the specification, the Store before installing.
pub mod sound_preview;
pub mod splash_exec;
pub mod steam_profile;
pub mod steam_profile_items;
/// Steam profile showcases: slicing your own picture and the preview layout (without writing to Steam).
pub mod steam_showcase;
pub mod steam_screensaver;
pub mod steam_token;
pub mod steamos_sysext;
pub mod system;
pub mod system_de_exec;
pub mod system_exec;
pub mod system_l10n;
pub mod system_plan;
pub mod terminal_guard;
pub mod theme_removal;
pub mod theme_watch;
pub mod themes;
pub mod translator;
/// Live machine translation: the engine, model lookup and the "text → translation" pairing.
pub mod translator_engine;
/// The Rexon icon in the system tray and autostart on login.
pub mod tray;
pub mod ui_base;
/// Watchdog of the window's main thread and the log of the person's actions.
pub mod ui_watch;
pub mod update_guard;
pub mod upscaler;
pub mod user_props;
pub mod utils;
pub mod wallhaven;
pub mod wallpaper;
pub mod wallpaper_audio;
pub mod wallpaper_backend;
pub mod wallpaper_engine;
pub mod wallpaper_exec;
pub mod wallpaper_mmap;
/// Playlist of Rexon's own wallpapers: the model, the rotator's decisions, the timer.
pub mod wallpaper_playlist;
pub mod wallpaper_screens;
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
// A snapshot of real widgets in the Qt style and GTK theme for the "Appearance" tiles.
pub mod widget_shot;
pub mod youtube_wallpaper;
