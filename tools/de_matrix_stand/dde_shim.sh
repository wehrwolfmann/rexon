#!/bin/bash
# A stand-in Deepin service for the «setting × desktop» stand (deepin).
# run_de.sh puts this file into the stand's PATH under the names `busctl` and
# `dde-dconfig`. The stand does NOT start live DDE services (neither dde-daemon, nor
# deepin-service-manager, nor dde-dconfig-daemon): instead of them there is this model,
# written from the linuxdeepin sources (dde-appearance `doSetByType` and the
# `Appearance1` properties, dde-daemon `soundeffect1`), and the DConfig storage is
# files $H/.dde-shim/<appid>/<resource>/<key>. All calls go to $H/dde-calls.log.
#
# This checks «the Rexon command reached the right Deepin key», not
# Deepin itself: what the service does with the key afterwards, the stand does not see.
# DDE_SHIM_SERVICE=0 — «no appearance service on the bus» (busctl refuses).
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
