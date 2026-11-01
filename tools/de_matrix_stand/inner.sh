#!/bin/bash
# Вызывается из run_de.sh внутри своей шины: действие стенда → чтение родным
# читателем окружения. Отдельно не запускать.
C=$XDG_CONFIG_HOME
st() { echo; echo "### $*"; timeout -k 2 40 "$BIN" "$@" 2>&1 | sed 's/^/    /'; }
gs() { printf '  gsettings %-45s %-22s = %s\n' "$1" "$2" "$(timeout 5 gsettings get "$1" "$2" 2>&1)"; }
kr() { printf '  kreadconfig6 %-12s [%s] %-20s = %s\n' "$1" "$2" "$3" "$(timeout 5 kreadconfig6 --file "$1" --group "$2" --key "$3" 2>&1)"; }
xq() { printf '  xfconf-query -c %-12s -p %-28s = %s\n' "$1" "$2" "$(timeout 5 xfconf-query -c "$1" -p "$2" 2>&1)"; }
ini() { local v; v=$(awk -v s="[$2]" -v k="$3" '$0==s{f=1;next} /^\[/{f=0} f&&index($0,k"=")==1{sub(/^[^=]*=/,"");print}' "$1" 2>/dev/null); printf '  ini %-40s [%s] %-22s = %s\n' "${1#$H/}" "$2" "$3" "${v:-<нет>}"; }
fl() { printf '  file %s:\n' "${1#$H/}"; if [ -f "$1" ]; then sed 's/^/      | /' "$1"; else echo "      <нет файла>"; fi; }

# Hyprland: свой конфиг уже заведён (Hyprland кладёт его при первом входе) —
# иначе Rexon честно не пишет в него ни курсор, ни переменные сеанса.
if [ "$DE" = hyprland ] && [ -f /usr/share/hypr/hyprland.lua ]; then
  mkdir -p "$C/hypr" && cp /usr/share/hypr/hyprland.lua "$C/hypr/hyprland.lua"
fi

st de
st categories

# ── Pantheon: родные значения ДО Rexon (переопределения `:Pantheon`) ──
if [ "$DE" = pantheon ]; then
  echo; echo "### pantheon: умолчания до Rexon"
  gs org.gnome.desktop.interface gtk-theme
  gs org.gnome.desktop.interface icon-theme
  gs org.gnome.desktop.interface cursor-theme
  gs org.gnome.desktop.interface font-name
  gs org.gnome.desktop.sound theme-name
  gs org.gnome.desktop.wm.preferences button-layout
  gs io.elementary.settings-daemon.interface accent-color
  gs io.elementary.settings-daemon.prefers-color-scheme color-scheme
  gs io.elementary.settings-daemon.prefers-color-scheme prefer-dark-schedule
  gs net.launchpad.plank.dock.settings:/net/launchpad/plank/docks/dock1/ theme
fi

# ── Deepin: чтение ключей подставной службы (dde_shim.sh) ──
dc() { printf '  dde-dconfig %-42s %-24s = %s\n' "$1/$2" "$3" "$(timeout 5 dde-dconfig get -a "$1" -r "$2" -k "$3" 2>&1)"; }
deepin_keys() {
  [ "$DE" = deepin ] || return 0
  local A="org.deepin.dde.appearance org.deepin.dde.appearance" X="org.deepin.dde.daemon org.deepin.XSettings"
  for k in "$@"; do
    case $k in
      xs:*) dc $X "${k#xs:}" ;;
      se:*) dc org.deepin.dde.daemon org.deepin.dde.daemon.soundeffect "${k#se:}" ;;
      kw:*) dc org.kde.kwin org.kde.kwin.compositing "${k#kw:}" ;;
      *) dc $A "$k" ;;
    esac
  done
}

# ── Тема GTK ──
st gtk Materia-dark
deepin_keys Gtk_Theme xs:gtk-theme-name
gs org.gnome.desktop.interface gtk-theme
gs org.cinnamon.desktop.interface gtk-theme
gs org.mate.interface gtk-theme
xq xsettings /Net/ThemeName
ini $C/gtk-3.0/settings.ini Settings gtk-theme-name
ini $C/gtk-4.0/settings.ini Settings gtk-theme-name
fl $C/gtk-4.0/gtk.css
fl $H/.gtkrc-2.0

# ── Значки ──
st icons Papirus
deepin_keys Icon_Theme xs:icon-theme-name
gs org.gnome.desktop.interface icon-theme
gs org.cinnamon.desktop.interface icon-theme
gs org.mate.interface icon-theme
xq xsettings /Net/IconThemeName
kr kdeglobals Icons Theme
ini $C/gtk-3.0/settings.ini Settings gtk-icon-theme-name
ini $C/lxqt/lxqt.conf General icon_theme
fl $C/cosmic/com.system76.CosmicTk/v1/icon_theme

# ── Курсор ──
st cursor capitaine-cursors
st cursor-size 48
deepin_keys Cursor_Theme cursorSize xs:gtk-cursor-theme-name
gs org.gnome.desktop.interface cursor-theme
gs org.gnome.desktop.interface cursor-size
gs org.cinnamon.desktop.interface cursor-theme
gs org.cinnamon.desktop.interface cursor-size
gs org.mate.peripherals-mouse cursor-theme
gs org.mate.peripherals-mouse cursor-size
xq xsettings /Gtk/CursorThemeName
xq xsettings /Gtk/CursorThemeSize
kr kcminputrc Mouse cursorTheme
kr kcminputrc Mouse cursorSize
ini $C/gtk-3.0/settings.ini Settings gtk-cursor-theme-name
ini $C/gtk-3.0/settings.ini Settings gtk-cursor-theme-size
ini $C/lxqt/session.conf Mouse cursor_theme
ini $C/lxqt/session.conf Mouse cursor_size
fl $C/environment.d/rexon-cursor.conf
fl $H/.icons/default/index.theme
for f in $C/sway/config $C/sway/rexon-cursor.conf $C/niri/config.kdl $C/niri/rexon-cursor.kdl $C/hypr/hyprland.conf $C/hypr/hyprland.lua $C/hypr/rexon-cursor.conf $C/hypr/rexon-cursor.lua; do
  [ -f "$f" ] && { echo "  file ${f#$H/}: $(wc -l < "$f") строк; блок Rexon:"; grep -n -i -A3 "rexon" "$f" | sed 's/^/      | /'; }
done

# ── Стиль приложений и Kvantum ──
st widgetstyle Fusion
kr kdeglobals KDE widgetStyle
ini $C/lxqt/lxqt.conf Qt style
ini $C/qt6ct/qt6ct.conf Appearance style
fl $C/environment.d/rexon-qt-style.conf
st kvantum KvArc
fl $C/Kvantum/kvantum.kvconfig
kr kdeglobals KDE widgetStyle
ini $C/lxqt/lxqt.conf Qt style
ini $C/qt6ct/qt6ct.conf Appearance style
fl $C/environment.d/rexon-qt-style.conf

# Переменные сеанса там, где environment.d не читают (Д1).
fl $H/.xprofile
fl $C/xfce4/labwc/environment
fl $C/budgie-desktop/labwc/environment
for f in $C/hypr/hyprland.conf $C/hypr/hyprland.lua; do
  [ -f "$f" ] && { echo "  file ${f#$H/}: блок переменных:"; grep -n -A4 "rexon:session-env" "$f" | sed 's/^/      | /'; }
done

# ── Цветовая схема (раздел KDE) ──
st colors BreezeDark
kr kdeglobals General ColorScheme

# ── Звуки: «Оформление» ──
st sounds oxygen
deepin_keys Sound_Theme xs:sound-theme-name
gs org.gnome.desktop.sound theme-name
gs org.cinnamon.desktop.sound theme-name
gs org.mate.sound theme-name
xq xsettings /Net/SoundThemeName
kr kdeglobals Sounds Theme
# ── Звуки: «Настройки» ──
st sound-theme ocean
deepin_keys Sound_Theme xs:sound-theme-name
gs org.gnome.desktop.sound theme-name
gs org.cinnamon.desktop.sound theme-name
gs org.mate.sound theme-name
xq xsettings /Net/SoundThemeName
kr kdeglobals Sounds Theme
st event-sounds false
deepin_keys se:enabled
gs org.gnome.desktop.sound event-sounds
gs org.cinnamon.desktop.sound event-sounds
gs org.mate.sound event-sounds
xq xsettings /Net/EnableEventSounds
kr kdeglobals Sounds Enable
kr kdeglobals Sounds EnableNotificationSounds

# ── Шрифты ──
st font-system "Noto Serif,13"
st font-mono "Hack,11"
st font-title "Noto Sans,12"
deepin_keys Font_Standard Font_Monospace Font_Size
gs org.gnome.desktop.interface font-name
gs org.gnome.desktop.interface monospace-font-name
gs org.gnome.desktop.wm.preferences titlebar-font
gs org.gnome.desktop.wm.preferences titlebar-uses-system-font
gs org.cinnamon.desktop.interface font-name
gs org.cinnamon.desktop.wm.preferences titlebar-font
gs org.cinnamon.desktop.wm.preferences titlebar-uses-system-font
gs org.mate.interface font-name
gs org.mate.interface monospace-font-name
gs org.mate.Marco.general titlebar-font
xq xsettings /Gtk/FontName
xq xsettings /Gtk/MonospaceFontName
xq xfwm4 /general/title_font
kr kdeglobals General font
kr kdeglobals General fixed
kr kdeglobals WM activeFont
ini $C/lxqt/lxqt.conf Qt font
ini $C/lxqt/lxqt.conf Qt fixedFont
ini $C/gtk-3.0/settings.ini Settings gtk-font-name
fl $C/cosmic/com.system76.CosmicTk/v1/interface_font
fl $C/cosmic/com.system76.CosmicTk/v1/monospace_font
fl $C/qt6ct/qt6ct.conf

# ── Терминал ──
st terminal org.kde.konsole.desktop
[ "$DE" = deepin ] && { gs com.deepin.desktop.default-applications.terminal app-id; gs com.deepin.desktop.default-applications.terminal exec; gs com.deepin.desktop.default-applications.terminal exec-arg; }
fl $C/xdg-terminals.list
printf '  xdg-terminal-exec --print-id = %s\n' "$(XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP timeout 5 xdg-terminal-exec --print-id 2>&1)"
gs org.gnome.desktop.default-applications.terminal exec
gs org.cinnamon.desktop.default-applications.terminal exec
gs org.mate.applications-terminal exec
kr kdeglobals General TerminalApplication
kr kdeglobals General TerminalService
ini $C/xfce4/helpers.rc "" TerminalEmulator
fl $C/xfce4/helpers.rc
printf '  qtxdg-mat def-terminal = %s\n' "$(timeout 5 qtxdg-mat def-terminal 2>&1 | tr '\n' ' ')"
fl $C/environment.d/rexon-terminal.conf

# ── Файловый менеджер ──
st file-manager org.kde.dolphin.desktop
fl $C/dde-mimeapps.list
printf '  xdg-mime query default inode/directory = %s\n' "$(timeout 5 xdg-mime query default inode/directory 2>&1)"
ini $C/xfce4/helpers.rc "" FileManager
fl $H/.local/share/xfce4/helpers/custom-FileManager.desktop
fl $H/.local/share/xfce4/helpers/custom-TerminalEmulator.desktop
fl $C/xfce4/helpers.rc

# ── Обои (статичная картинка) ──
st wallpaper test.png
deepin_keys Wallpaper_Uris
gs org.gnome.desktop.background picture-uri
gs org.gnome.desktop.background picture-uri-dark
gs org.cinnamon.desktop.background picture-uri
gs org.mate.background picture-filename
[ "$DE" = xfce ] && xq xfce4-desktop /backdrop/screen0/monitorVirtual-1/workspace0/last-image
fl $C/cosmic/com.system76.CosmicBackground/v1/all
fl $C/pcmanfm-qt/lxqt/settings.conf

# ── Окна Deepin (раздел «Настройки») ──
if [ "$DE" = deepin ]; then
  st dde-look radius 6
  st dde-look opacity 60
  st dde-look effect performance
  st dde-look accent "#49B125,#459F29"
  deepin_keys Opacity Active_Colors xs:dtk-window-radius kw:user_type
  st dde-look-state
  echo; echo "### deepin: служба оформления не отвечает (DDE_SHIM_SERVICE=0)"
  DDE_SHIM_SERVICE=0 st gtk Adwaita
  DDE_SHIM_SERVICE=0 st wallpaper test.png
  DDE_SHIM_SERVICE=0 st dde-look radius 18
  deepin_keys Gtk_Theme xs:dtk-window-radius
  fl $H/dde-calls.log
fi

echo; echo "### всё дерево настроек:"; (cd $H && find . -path ./run -prune -o -type f -newer $H/run -print | sort | sed 's/^/    /')
