#!/bin/bash
# Вызывается из run_de.sh (INNER=inner_look.sh BIN=…/examples/look_stand)
# внутри своей шины: «Анимация» и «Кнопки заголовка окна» — действие стенда
# → чтение родным читателем окружения → повтор (ждём «без изменений») →
# возврат к прежнему значению. Отдельно не запускать.
C=$XDG_CONFIG_HOME
st() { echo; echo "### $*"; timeout -k 2 60 "$BIN" "$@" 2>&1 | sed 's/^/    /'; }
gs() { printf '  gsettings %-42s %-28s = %s\n' "$1" "$2" "$(timeout 5 gsettings get "$1" "$2" 2>&1)"; }
kr() { printf '  kreadconfig6 %-10s [%s] %-24s = %s\n' "$1" "$2" "$3" "$(timeout 5 kreadconfig6 --file "$1" --group "$2" --key "$3" 2>&1)"; }
xq() { printf '  xfconf-query -c %-10s -p %-24s = %s\n' "$1" "$2" "$(timeout 5 xfconf-query -c "$1" -p "$2" 2>&1)"; }
ini() { local v; v=$(awk -v s="[$2]" -v k="$3" '$0==s{f=1;next} /^\[/{f=0} f&&index($0,k"=")==1{sub(/^[^=]*=/,"");print}' "$1" 2>/dev/null); printf '  ini %-30s [%s] %-24s = %s\n' "${1#$H/}" "$2" "$3" "${v:-<нет>}"; }
fl() { printf '  file %s:\n' "${1#$H/}"; if [ -f "$1" ]; then sed 's/^/      | /' "$1"; else echo "      <нет файла>"; fi; }
snap() { (cd $H && find . -path ./run -prune -o -path ./.cache -prune -o -type f -print0 | sort -z | xargs -0 md5sum 2>/dev/null; timeout 5 dconf dump / 2>/dev/null | md5sum; timeout 5 xfconf-query -c xfwm4 -l -v 2>/dev/null | md5sum; timeout 5 xfconf-query -c xsettings -l -v 2>/dev/null | md5sum) | md5sum | cut -c1-12; }

# Конфиг композитора — как у человека после первого запуска.
case $DE in
  hyprland) mkdir -p $C/hypr && cp /usr/share/hypr/hyprland.lua $C/hypr/hyprland.lua ;;
  niri) mkdir -p $C/niri && cp /usr/share/doc/niri/default-config.kdl $C/niri/config.kdl ;;
esac

native() {
  sleep 1  # dconf-service пишет базу на диск не мгновенно
  case $DE in
    kde)
      kr kdeglobals KDE AnimationDurationFactor
      for k in scaleEnabled glideEnabled fadeEnabled squashEnabled magiclampEnabled slideEnabled fadedesktopEnabled loginEnabled slidingpopupsEnabled; do kr kwinrc Plugins $k; done
      kr kwinrc org.kde.kdecoration2 ButtonsOnLeft
      kr kwinrc org.kde.kdecoration2 ButtonsOnRight
      kr kwinrc org.kde.kdecoration2 BorderSize
      kr kwinrc org.kde.kdecoration2 BorderSizeAuto
      ini $C/gtk-3.0/settings.ini Settings gtk-enable-animations
      ini $C/gtk-3.0/settings.ini Settings gtk-decoration-layout
      ini $C/gtk-4.0/settings.ini Settings gtk-decoration-layout ;;
    gnome|pantheon) gs org.gnome.desktop.interface enable-animations; gs org.gnome.desktop.wm.preferences button-layout ;;
    budgie) gs org.gnome.desktop.interface enable-animations; gs com.solus-project.budgie-wm button-layout; gs org.gnome.desktop.wm.preferences button-layout ;;
    cinnamon)
      for k in desktop-effects-workspace window-effect-speed desktop-effects-map desktop-effects-minimize desktop-effects-change-size startup-animation; do gs org.cinnamon $k; done
      gs org.cinnamon.desktop.interface enable-animations
      gs org.cinnamon.desktop.wm.preferences button-layout ;;
    mate) gs org.mate.interface enable-animations; gs org.mate.interface gtk-enable-animations; gs org.mate.Marco.general button-layout; gs org.mate.interface gtk-decoration-layout ;;
    xfce) xq xsettings /Gtk/EnableAnimations; xq xfwm4 /general/button_layout ;;
    cosmic) fl $C/cosmic/com.system76.CosmicTk/v1/show_minimize; fl $C/cosmic/com.system76.CosmicTk/v1/show_maximize; ini $C/gtk-3.0/settings.ini Settings gtk-enable-animations ;;
    hyprland)
      fl $C/hypr/rexon-animations.lua; fl $C/hypr/rexon-borders.lua
      echo "  блок в hyprland.lua:"; grep -n -A1 'Rexon' $C/hypr/hyprland.lua | sed 's/^/      | /'
      printf '  Hyprland --verify-config = %s\n' "$(timeout 30 Hyprland --verify-config --config $C/hypr/hyprland.lua 2>&1 | sed -n '/parsing result/,$p' | tail -1)"
      ini $C/gtk-3.0/settings.ini Settings gtk-decoration-layout ;;
    niri)
      fl $C/niri/rexon-animations.kdl; fl $C/niri/rexon-borders.kdl
      echo "  блок в config.kdl:"; grep -n -A1 'Rexon' $C/niri/config.kdl | sed 's/^/      | /'
      printf '  niri validate = %s\n' "$(timeout 20 niri validate --config $C/niri/config.kdl 2>&1 | grep -o 'config is valid\|error.*' | head -1)"
      ini $C/gtk-3.0/settings.ini Settings gtk-decoration-layout ;;
    *) ini $C/gtk-3.0/settings.ini Settings gtk-enable-animations; ini $C/gtk-3.0/settings.ini Settings gtk-decoration-layout ;;
  esac
}

echo "### ДО"; native; S0=$(snap); echo "  снимок=$S0"
st anim-state
st buttons-state

# Изменения — как нажатия.
case $DE in
  kde) st anim-speed 0.5; st anim-row openClose=glide; st anim-row minimize=magiclamp; st anim-row login=false; st border Large ;;
  cinnamon) st anim-speed 2; st anim-row minimize=fade; st anim-row resize=false ;;
  hyprland) st anim-speed 2; st anim-row openClose=slide; st anim-row desktopSwitch=slidevert; st border 3 ;;
  niri) st anim-speed 2; st anim-row desktopSwitch=false; st border 3 ;;
  *) st anim-enabled false ;;
esac
st buttons left:close,minimize,maximize,menu
echo; echo "### ПОСЛЕ"; native; S1=$(snap); echo "  снимок=$S1"

# Повтор тех же значений — без изменений.
case $DE in
  kde) st anim-speed 0.5; st anim-row openClose=glide; st border Large ;;
  cinnamon) st anim-speed 2; st anim-row minimize=fade ;;
  hyprland) st anim-speed 2; st anim-row openClose=slide; st border 3 ;;
  niri) st anim-speed 2; st border 3 ;;
  *) st anim-enabled false ;;
esac
st buttons left:close,minimize,maximize,menu
S2=$(snap); echo "  ПОВТОР: снимок=$S2 $( [ "$S1" = "$S2" ] && echo 'совпал с ПОСЛЕ — без изменений' || echo 'ИЗМЕНИЛСЯ')"

# Откат к прежнему: «По умолчанию» в обоих разделах (чистый профиль = умолчания).
st anim-defaults
st anim-enabled true
st buttons-defaults
echo; echo "### ОТКАТ"; native
st anim-state
st buttons-state
