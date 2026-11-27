#!/bin/bash
# probe_apps.sh <de> <qt|gtk3|gtk4|adw> [with-env 0|1]
#
# What the PROGRAM really picked up from the profile built by run_de.sh:
# Qt (style, fonts, icon theme, PySide6 on the system Qt) or GTK3 / GTK4 /
# libadwaita (theme, icons, cursor, font + a window snapshot in $WORK/shot-<de>-<kind>.png).
# The window is on a separate X screen (Xvfb), with its own bus without services.
# with-env=1 — as after logging into a session again that reads
# ~/.config/environment.d (Plasma, GNOME, niri, Hyprland through uwsm).
set -u
DE=$1; KIND=$2; WITH=${3:-0}
S=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-$HOME/Загрузки/Rexon/de-matrix-stand}
H=$WORK/home-$DE; DN=${DN:-93}
[ -d "$H" ] || { echo "сначала run_de.sh $DE"; exit 2; }
XCD=$(grep -m1 -o 'XDG_CURRENT_DESKTOP="[^"]*"' "$WORK/out-$DE.txt" | cut -d'"' -f2)
EXTRA=()
if [ "$WITH" = 1 ]; then
  for f in "$H"/.config/environment.d/*.conf; do
    [ -f "$f" ] && while IFS= read -r l; do [ -n "$l" ] && EXTRA+=("$l"); done < "$f"
  done
fi
mkdir -p "$WORK/empty-services"
sed "s|<servicedir>.*</servicedir>|<servicedir>$WORK/empty-services</servicedir>|" "$WORK/bus.conf" > "$WORK/bus-empty.conf"
ulimit -c 0
timeout -k 3 30 Xvfb :$DN -screen 0 420x240x24 -nolisten tcp >/dev/null 2>&1 &
XP=$!
sleep 2
case $KIND in
  qt) CMD=(python3 "$S/qtprobe.py"); PLAT=(QT_QPA_PLATFORM=xcb) ;;
  *)  CMD=(python3 "$S/gtkprobe.py" "$KIND"); PLAT=(GDK_BACKEND=x11 GSETTINGS_BACKEND=memory) ;;
esac
env -i PATH=/usr/bin HOME=$H XDG_RUNTIME_DIR=$H/run XDG_CONFIG_HOME=$H/.config \
  XDG_DATA_HOME=$H/.local/share DISPLAY=:$DN XDG_CURRENT_DESKTOP=$XCD "${PLAT[@]}" \
  GTK_USE_PORTAL=0 GIO_USE_VFS=local LANG=C.UTF-8 LD_PRELOAD=$WORK/quietcrash.so "${EXTRA[@]}" \
  timeout -k 3 12 dbus-run-session --config-file=$WORK/bus-empty.conf -- "${CMD[@]}" 2>&1 \
  | grep -E '^(QT_QPA|style|font|fixed|iconTheme|window|gtk3|gtk4|adw)' &
AP=$!
sleep 4
[ "$KIND" != qt ] && import -display :$DN -window root "$WORK/shot-$DE-$KIND.png" 2>/dev/null
wait $AP
kill $XP 2>/dev/null; wait $XP 2>/dev/null
