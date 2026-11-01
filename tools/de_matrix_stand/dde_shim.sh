#!/bin/bash
# Подставная служба Deepin для стенда «настройка × окружение» (deepin).
# run_de.sh кладёт этот файл в PATH стенда под именами `busctl` и
# `dde-dconfig`. Живых служб DDE стенд НЕ запускает (ни dde-daemon, ни
# deepin-service-manager, ни dde-dconfig-daemon): вместо них — эта модель,
# написанная по исходникам linuxdeepin (dde-appearance `doSetByType` и
# свойства `Appearance1`, dde-daemon `soundeffect1`), а хранилище DConfig —
# файлы $H/.dde-shim/<appid>/<resource>/<key>. Все вызовы — в $H/dde-calls.log.
#
# Это проверка «команда Rexon дошла до нужного ключа Deepin», а не проверка
# самого Deepin: что служба делает с ключом дальше, стенд не видит.
# DDE_SHIM_SERVICE=0 — «службы оформления на шине нет» (busctl отказывает).
set -u
me=$(basename "$0")
store=${H:-$HOME}/.dde-shim
echo "$me $*" >> "${H:-$HOME}/dde-calls.log"

put() { mkdir -p "$store/$1/$2"; printf '%s' "$4" > "$store/$1/$2/$3"; }
get() { cat "$store/$1/$2/$3" 2>/dev/null; }
APP="org.deepin.dde.appearance org.deepin.dde.appearance"
XS="org.deepin.dde.daemon org.deepin.XSettings"

if [ "$me" = dde-dconfig ]; then
  cmd=$1; shift
  a= r= k= v=
  while [ $# -gt 0 ]; do
    case $1 in -a) a=$2; shift ;; -r) r=$2; shift ;; -k) k=$2; shift ;; -v) v=$2; shift ;; esac
    shift
  done
  case $cmd in
    set) put "$a" "$r" "$k" "$v" ;;
    get) val=$(get "$a" "$r" "$k") || exit 1
         case $val in ''|*[!0-9.-]*) [ "$val" = true ] || [ "$val" = false ] && echo "$val" || echo "\"$val\"" ;; *) echo "$val" ;; esac ;;
    *) exit 1 ;;
  esac
  exit 0
fi

# busctl --user --auto-start=no <call|set-property|get-property> DEST PATH IFACE ...
[ "${DDE_SHIM_SERVICE:-1}" = 0 ] && { echo "Unit/name not activatable" >&2; exit 1; }
shift 2  # --user --auto-start=no
verb=$1 dest=$2; shift 4
case "$verb $dest" in
  "call org.deepin.dde.Appearance1")
    method=$1; shift 2   # метод и подпись
    case $method in
      Set)
        kind=$1 value=$2
        case $kind in
          gtk) put $APP Gtk_Theme "$value"; put $XS gtk-theme-name "$value" ;;
          icon) put $APP Icon_Theme "$value"; put $XS icon-theme-name "$value" ;;
          cursor) put $APP Cursor_Theme "$value"; put $XS gtk-cursor-theme-name "$value" ;;
          standardfont) put $APP Font_Standard "$value" ;;
          monospacefont) put $APP Font_Monospace "$value" ;;
          fontsize) put $APP Font_Size "$value" ;;
          background) put $APP Wallpaper_Uris "$value" ;;
          greeterbackground) put $APP isCustomLockBackground true; put accounts1 user GreeterBackground "$value" ;;
          *) exit 1 ;;
        esac ;;
      SetActiveColors) put $APP Active_Colors "$1" ;;
      *) exit 1 ;;
    esac ;;
  "set-property org.deepin.dde.Appearance1")
    prop=$1 value=$3
    case $prop in
      CursorSize) put $APP cursorSize "$value" ;;
      WindowRadius) put $XS dtk-window-radius "$value" ;;
      Opacity) put $APP Opacity "$value" ;;
      *) exit 1 ;;
    esac ;;
  "get-property org.deepin.dde.Appearance1")
    case $1 in
      WindowRadius) v=$(get $XS dtk-window-radius) && echo "i $v" ;;
      Opacity) v=$(get $APP Opacity) && echo "d $v" ;;
      *) exit 1 ;;
    esac ;;
  "set-property org.deepin.dde.SoundEffect1")
    put org.deepin.dde.daemon org.deepin.dde.daemon.soundeffect enabled "$3" ;;
  *) exit 1 ;;
esac
exit 0
