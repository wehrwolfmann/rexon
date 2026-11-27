# Customizing Linux desktops: a per-environment breakdown

The full map of what each desktop is customized with, where it is put, what enables it and what has to be restarted. The table is the working basis of the universal `.rxn`: Rexon installs and enables the types from the "`.rxn` type" column itself.

> This file is GENERATED from `rust/rexon-core/src/de_customization.rs`
> (test `matrix_doc_is_up_to_date`). Edit the matrix, not this
> text: to update — `REXON_UPDATE_DOC=1 cargo test -p rexon-core
> --test de_customization`.

## Common directory (installed the same way everywhere)

| `.rxn` type | What it is | Where to put it | Works in | How to enable |
|---|---|---|---|---|
| `theme-gtk` | GTK theme (the look of GTK app windows and widgets) | `~/.themes` | in every desktop (with a caveat in Pantheon) | Rexon applies it itself (the “Apply” button) |
| `theme-icons` | Icon set | `~/.local/share/icons` | in every desktop | Rexon applies it itself (the “Apply” button) |
| `theme-cursors` | Mouse pointers | `~/.local/share/icons` | in every desktop | Rexon applies it itself (the “Apply” button) |
| `theme-sounds` | Sound theme (system event sounds) | `~/.local/share/sounds` | in every desktop (with a caveat in Tiling and Wayfire (sway/Hyprland/niri/River)) | Rexon applies it itself (the “Apply” button) |
| `theme-kvantum` | Kvantum theme (the look of Qt apps) | `~/.config/Kvantum` | KDE Plasma, GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, LXQt, COSMIC, Tiling and Wayfire (sway/Hyprland/niri/River) | Rexon applies it itself (the “Apply” button) |
| `theme-login-screen` | Login screen theme | `~/.local/share/rexon/login-themes` | in every desktop | Rexon applies it itself (the “Apply” button) |
| `theme-plymouth` | Boot splash (Plymouth) | `~/.local/share/rexon/plymouth-themes` | in every desktop | Rexon applies it itself (the “Apply” button) |
| `theme-conky` | Conky widget (clock, sensors on the desktop) | `~/.config/conky` | in every desktop | manually: run conky with this config file |
| `theme-bundle` | Theme bundle (several themes in one package) | `~/.local/share/rexon/bundles` | in every desktop | Rexon applies it itself (the “Apply” button) |
| `wallpaper-pack` | Wallpaper pack | — (applied in place) | in every desktop | Rexon applies it itself (the “Apply” button) |
| `wallpaper-video-adaptive` | Live wallpapers (video per resolution and language) | `~/.local/share/rexon/wallpapers` | in every desktop (with a caveat in Pantheon) | Rexon applies it itself (the “Apply” button) |
| `font-pack` | Font pack | `~/.local/share/fonts` | in every desktop | picked up automatically |

Pitfalls:

* **GTK Theme** — Needed EVERYWHERE, including KDE: Firefox, GIMP and Inkscape live under Plasma, and its system settings have the “GNOME/GTK Application Style” module (kde-gtk-config). The look of Budgie is defined precisely by the GTK theme. ⚠️ Pantheon: the io.elementary.settings-daemon service (AccentColorManager) writes its own io.elementary.stylesheet.<color> to org.gnome.desktop.interface gtk-theme at every start, on every accent change and — with the “auto” accent — on every wallpaper change; it has no off switch.
* **Icons** — The directory is shared with cursors: both are “icon themes” per the freedesktop specification.
* **Cursors** — Besides the choice in the desktop settings, ~/.icons/default is written — otherwise apps on plain X11 and some Wayland clients keep the old cursor.
* **Sounds** — The directory is shared per the freedesktop specification, and the sound theme does not belong to the shell: notification daemons and libcanberra programs pick it up — exactly as GTK and Qt apps pick up icons and cursors.
* **Kvantum** — Kvantum is a Qt style engine (libkvantum.so in the Qt plugin directory); it paints ANY Qt app regardless of the shell: under GNOME that means VLC, qBittorrent, Krita, OBS. The difference between desktops is not whether it works but WHAT selects the style: KDE — widgetStyle in kdeglobals, LXQt — [Qt] style in lxqt.conf, the rest — QT_STYLE_OVERRIDE in ~/.config/environment.d. Deepin has no such section: a session-wide variable would take the Chameleon style away from DTK programs.
* **Login Screen** — The login screen is drawn by the display manager, not by the desktop: Rexon detects which one is installed (SDDM, GDM or LightDM) and asks for the password only to write to the system directory.
* **Boot Splash** — Shown before login and does not depend on the desktop at all; installing it requires the password and rebuilding the kernel image.
* **Conky Widget** — Conky is a separate program, not part of the desktop, so it also works where the desktop has no widgets of its own.
* **Full Theme** — Inside are ordinary types from this same table; each is installed by its own path, and those unfit for the current desktop are skipped.
* **Wallpapers** — There is no install directory of its own: the wallpapers are applied straight from the package — Rexon's wallpaper engine sets them, the same way on every desktop.
* **Adaptive Wallpapers (v4)** — Drawn by Rexon's wallpaper engine, not by the desktop, so it also works in GNOME and Cinnamon (through our shell extension) and on tiling compositors (the layer-shell layer). Not in Pantheon on Wayland: Gala knows only its own pantheon-desktop-shell protocol, has no wlr-layer-shell and does not load extension modules (`wallpaper_backend::pick` → Unsupported).

Works, but with a caveat:

* **GTK Theme** (Pantheon) — Pantheon's own settings service puts the elementary style back over the GTK theme: at every login, whenever you change the accent color and — with the automatic accent — whenever the wallpaper changes. Until then your theme stays.
* **Sounds** (Tiling and Wayfire (sway/Hyprland/niri/River)) — Event sounds are played by a notification daemon (swaync, mako, dunst) or by the app itself, not by the desktop shell. On tiling compositors you start that daemon yourself — until you do, the theme stays silent.
* **Kvantum** (GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, COSMIC, Tiling and Wayfire (sway/Hyprland/niri/River)) — Every Qt app (VLC, qBittorrent, Krita, OBS) will pick the theme up. This desktop has no Qt style switch of its own, so Rexon sets one up for you — but the change only shows up after your next login.
* **Adaptive Wallpapers (v4)** (Pantheon) — Live wallpapers work in the Pantheon “Classic Session” (X11). In the “Secure Session” (Wayland) the Gala window manager doesn't let other programs draw the desktop background, so only a still picture can be set there.
* **KDE Global Theme** (GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, Deepin, LXQt, COSMIC, Tiling and Wayfire (sway/Hyprland/niri/River)) — A global theme is a Plasma package. Here Rexon applies its parts one by one — colors, application style, icons, pointers, fonts and title bar buttons; the window that opens before applying says what carries over.
* **KDE Colors** (GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, LXQt, COSMIC, Tiling and Wayfire (sway/Hyprland/niri/River)) — Outside Plasma, Rexon passes the scheme to KDE apps (Dolphin, Kate, Okular) and to the other Qt apps — some of them only pick it up after your next login. GTK apps (Firefox, GIMP) and most of the desktop's own apps don't read KDE color schemes and keep their own colors — for GTK apps, pick a matching theme in “GTK Themes”.

## KDE Plasma

| `.rxn` type | What it is | Where to put it | How to enable | What to restart |
|---|---|---|---|---|
| `theme-kde-global` | Full Plasma theme (Global Theme) | `~/.local/share/plasma/look-and-feel` | Rexon applies it itself (the “Apply” button) | nothing, applied immediately |
| `theme-kde-colors` | Color scheme | `~/.local/share/color-schemes` | Rexon applies it itself (the “Apply” button) | nothing, applied immediately |
| `theme-kde-plasma` | Plasma style (panel, menus, popups) | `~/.local/share/plasma/desktoptheme` | Rexon applies it itself (the “Apply” button) | nothing, applied immediately |
| `theme-window-decoration` | Window decorations (Aurorae) | `~/.local/share/aurorae/themes` | Rexon applies it itself (the “Apply” button) | qdbus org.kde.KWin /KWin reconfigure |
| `theme-plasmoid` | Plasma widget (plasmoid) | `~/.local/share/plasma/plasmoids` | manually: add the widget to a panel or the desktop | nothing, applied immediately |
| `theme-kde-splash` | Session splash screen (KSplash) | `~/.local/share/plasma/look-and-feel` | kwriteconfig6 --file ksplashrc --group KSplash --key Theme <name> (+ Engine=KSplashQML) | next login |
| `theme-kde-lockscreen` | Lock screen | `~/.local/share/plasma/look-and-feel` | kwriteconfig6 --file kscreenlockerrc --group Greeter --key Theme <name> | next screen lock |
| `kde-kwin-script` | KWin script (window behavior, tiling, gestures) | `~/.local/share/kwin/scripts` | kwriteconfig6 --file kwinrc --group Plugins --key <id>Enabled true | qdbus org.kde.KWin /KWin reconfigure |
| `kde-kwin-effect` | KWin desktop effect | `~/.local/share/kwin/effects` | kwriteconfig6 --file kwinrc --group Plugins --key <id>Enabled true | qdbus org.kde.KWin /KWin reconfigure |
| `kde-window-switcher` | Window switcher (Alt+Tab) | `~/.local/share/kwin/tabbox` | kwriteconfig6 --file kwinrc --group TabBox --key LayoutName <name> | qdbus org.kde.KWin /KWin reconfigure |
| `kde-wallpaper-plugin` | Plasma wallpaper plugin (a custom background engine) | `~/.local/share/plasma/wallpapers` | manually: right-click the desktop → “Configure Desktop” → wallpaper type | plasmashell --replace |

Pitfalls:

* **KDE Global Theme** — Changes everything at once — colors, the Plasma style, icons, the splash screen and the lock screen; that is why Rexon takes a panel snapshot before applying it (rule #13).
* **KDE Colors** — The package holds separate .colors files, not directories. Outside Plasma, Rexon merges the scheme's colors into ~/.config/kdeglobals with the same keys Plasma uses, plus [UiSettings] ColorScheme (otherwise KDE programs with a “Color Scheme” menu set Breeze on their own), and the Qt palette: LXQt — [Palette] in lxqt.conf, the rest — qt6ct/qt5ct colors/*.conf. GTK programs do not read KDE schemes.
* **Window Decorations** — Aurorae is specifically KWin's decoration engine; on other desktops frames are drawn by their own means (xfwm4, Marco, GTK).
* **Plasma Widget** — In GNOME the role of plasmoids is played by shell extensions, in Cinnamon — by applets and desklets: these are DIFFERENT types, not compatible with each other.
* **KDE Splash Screen** — KDE has no separate package type for it: a splash screen is a look-and-feel package with a single contents/splash directory.
* **Lock Screen** — The lock screen has no category of its own in the KDE Store either — it is always part of a look-and-feel package (contents/lockscreen).
* **KWin Script** — The key name in kwinrc is not the directory name but KPlugin.Id from metadata.json.
* **KWin Effect** — QML effects load without building; C++ effects are native plugins and are not distributed via .rxn.
* **Plasma Wallpaper Plugin** — The plugin choice is stored in the configuration of EACH containment (plasma-org.kde.plasma.desktop-appletsrc); switching it automatically is unsafe — it can wipe the desktop's settings.

## GNOME

| `.rxn` type | What it is | Where to put it | How to enable | What to restart |
|---|---|---|---|---|
| `gnome-shell-extension` | GNOME Shell extension | `~/.local/share/gnome-shell/extensions` | gnome-extensions enable <uuid> | Alt+F2 → r on X11; on Wayland — log out and back in |
| `theme-gnome-shell` | GNOME Shell theme (top bar, menus) | `~/.themes` | gsettings set org.gnome.shell.extensions.user-theme name '<name>' | nothing, applied immediately |

Pitfalls:

* **GNOME Extension** — The directory must be named exactly after the uuid. Settings schemas must be compiled (glib-compile-schemas schemas/). If the shell version is missing from shell-version, the extension silently fails to load.
* **GNOME Shell Theme** — Works only with the User Themes extension enabled (user-theme@gnome-shell-extensions.gcampax.github.com) — without it the key exists but the theme is not applied.

Not distributed via `.rxn`:

* **GDM login screen theme** (`/usr/share/gnome-shell/gnome-shell-theme.gresource`) — the GDM theme is baked into a binary gresource: the system file has to be unpacked, its styles replaced, rebuilt with glib-compile-resources and put into /usr/share as root. A gnome-shell update overwrites the replaced file — this breaks the login screen until the theme is reinstalled.

## Cinnamon

| `.rxn` type | What it is | Where to put it | How to enable | What to restart |
|---|---|---|---|---|
| `cinnamon-applet` | Cinnamon panel applet | `~/.local/share/cinnamon/applets` | add “panel1:<zone>:<order>:<uuid>:<number>” to the list org.cinnamon enabled-applets (+ counter next-applet-id) | cinnamon --replace (Wayland — log out and back in) |
| `cinnamon-desklet` | Desklet (a widget on the desktop) | `~/.local/share/cinnamon/desklets` | add “<uuid>:<number>:<x>:<y>” to the list org.cinnamon enabled-desklets (+ counter next-desklet-id) | cinnamon --replace (Wayland — log out and back in) |
| `cinnamon-extension` | Cinnamon extension | `~/.local/share/cinnamon/extensions` | add “<uuid>” to the list org.cinnamon enabled-extensions | cinnamon --replace (Wayland — log out and back in) |
| `theme-cinnamon` | Cinnamon Shell theme | `~/.themes` | gsettings set org.cinnamon.theme name '<name>' | nothing, applied immediately |
| `nemo-action` | Context menu action for the Nemo file manager | `~/.local/share/nemo/actions` | picked up automatically | nothing, applied immediately |

Pitfalls:

* **Cinnamon Applet** — Each instance has its own number; the next-applet-id counter must be incremented, otherwise the next applet gets the same number and the settings of the two applets get mixed up.
* **Cinnamon Desklet** — The entry stores coordinates on the desktop — we place it in the top-left corner with an offset so the desklet does not end up under the panel.
* **Cinnamon Extension** — Cinnamon applets, desklets and extensions are THREE different types with different directories and different enable lists.
* **Cinnamon Theme** — cinnamon.css is incompatible with gnome-shell.css: they share almost no class names, so a GNOME theme in Cinnamon gives a bare look.
* **Nemo Action** — Nemo watches the directory — the action appears without a restart.

## XFCE

| `.rxn` type | What it is | Where to put it | How to enable | What to restart |
|---|---|---|---|---|
| `theme-xfwm4` | xfwm4 window decoration theme | `~/.themes` | xfconf-query -c xfwm4 -p /general/theme -s <name> | nothing, applied immediately |

Pitfalls:

* **XFCE Windows (xfwm4)** — The XFCE window theme is separate from the GTK theme: they are chosen by two different settings, and a .rxn can carry both.

Not distributed via `.rxn`:

* **XFCE panel plugin** (`/usr/lib/xfce4/panel/plugins`) — an XFCE panel plugin is a native .so library built for a specific xfce4-panel version; it has to be compiled and put into a system directory. The exception is external script plugins (genmon, generic monitor): that is not a plugin but a setting of an already installed plugin.

## MATE

| `.rxn` type | What it is | Where to put it | How to enable | What to restart |
|---|---|---|---|---|
| `theme-marco` | Marco window decoration theme | `~/.themes` | gsettings set org.mate.Marco.general theme '<name>' | nothing, applied immediately |

Pitfalls:

* **MATE Windows (Marco)** — Marco is a fork of Metacity, which is why the theme directory is called metacity-1; the same themes work in MATE and in old GNOME 2. Cinnamon has not read them since version 5.4: Muffin takes its frames from the GTK theme.

Not distributed via `.rxn`:

* **MATE panel applet** (`/usr/share/mate-panel/applets`) — a MATE applet needs a .mate-panel-applet descriptor and a D-Bus service in /usr/share/dbus-1/services — both directories are system ones, root only.

## Budgie

Not distributed via `.rxn`:

* **Budgie panel applet** (`/usr/lib/budgie-desktop/plugins`) — Budgie applets are libpeas plugins built for a specific budgie-desktop version; the directory is a system one. The look of Budgie is defined by the GTK theme — and that one installs via .rxn (the “GTK theme” type).

## LXQt

| `.rxn` type | What it is | Where to put it | How to enable | What to restart |
|---|---|---|---|---|
| `theme-lxqt` | LXQt theme (panel and shell styles) | `~/.local/share/lxqt/themes` | .config/lxqt/lxqt.conf: [General] theme=<name> | restart lxqt-panel |
| `theme-openbox` | Openbox and labwc window frame theme | `~/.themes` | node theme/name = <name> in openbox/rc.xml or labwc/rc.xml + --reconfigure | nothing, applied immediately |

Pitfalls:

* **LXQt Theme** — lxqt-panel.qss is mandatory in the theme directory — without it the panel keeps the old style.
* **Openbox and labwc Windows** — An Openbox frame theme is themerc plus .xbm button images in openbox-3; labwc understands the same format. A user rc.xml replaces the system one entirely, so if there is none it is created as a copy of the system one.

Not distributed via `.rxn`:

* **LXQt panel plugin** (`/usr/lib/lxqt-panel`) — an LXQt panel plugin is a C++ library plus a .desktop descriptor, built for a specific lxqt-panel version.

## COSMIC

Not distributed via `.rxn`:

* **COSMIC theme** (`.config/cosmic`) — the COSMIC theme format (RON files in ~/.config/cosmic) still changes from release to release — too early to pin it down in .rxn. COSMIC wallpapers and icons are already set today — with native keys: the picture in com.system76.CosmicBackground/v1/all (drawn by cosmic-bg), the icon set in com.system76.CosmicTk/v1/icon_theme.

## Tiling and Wayfire (sway/Hyprland/niri/River)

Not distributed via `.rxn`:

* **Tiling compositor configuration** (`.config`) — on sway/Hyprland/niri the look is defined by a personal config file, not by an installable package: replacing it with a package would mean overwriting someone's settings and leaving the user without a desktop. Wallpapers and the panel are installed separately here (layer-shell); that works independently of the config.
