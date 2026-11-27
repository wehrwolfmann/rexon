#!/bin/bash
# The «setting × desktop» stand (check of 02.10.2026).
#
#   run_de.sh <kde|gnome|cinnamon|mate|xfce|budgie|pantheon|deepin|lxqt|cosmic|hyprland|sway|niri>
#
# Applies all settings of the «Настройки» and «Оформление» tabs the same way
# as the buttons do (the `de_matrix_stand` example in rexon-core), in a PRIVATE HOME under
# a substituted XDG_CURRENT_DESKTOP, and reads the result with the desktop's native
# readers (gsettings, kreadconfig6, xfconf-query, qtxdg-mat,
# xdg-terminal-exec, the lxqt/cosmic/hypr/sway/niri files) — see inner.sh.
# The result is $WORK/out-<de>.txt.
#
# Safety (all of it mandatory):
# * HOME, XDG_CONFIG_HOME, XDG_RUNTIME_DIR are its own; WAYLAND_DISPLAY, SWAYSOCK,
#   HYPRLAND_INSTANCE_SIGNATURE, NIRI_SOCKET are not passed (env -i) — not a single
#   command reaches the live session;
# * its own bus, and ONLY dconf and xfconfd are started on it (an allowlist of
#   service files). ⛔ Do not open the bus to all system services: kded6
#   is activated over the bus, pulls in Plasma autostart, and a plasmashell that crashed
#   without a screen reaches the live desktop's DrKonqi through KCrash (that is what happened on 02.10);
# * a quiet crash handler (LD_PRELOAD) + ulimit -c 0. ⚠️ It does NOT protect
#   programs with file capabilities (sway: LD_PRELOAD is ignored) and programs
#   with KCrash — so the stand does not start `sway -C`, kded6, plasmashell.
set -u
# Which scenario to run inside the bus: inner.sh (all sections) or inner_look.sh
# («Анимация» and «Кнопки заголовка окна», the look_stand example).
INNER=${INNER:-inner.sh}
DE=$1
S=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-$HOME/Загрузки/Rexon/de-matrix-stand}
BIN=${BIN:-$HOME/Загрузки/Rexon/target-a6/debug/examples/de_matrix_stand}
mkdir -p "$WORK/services"
[ -f "$WORK/quietcrash.so" ] || gcc -shared -fPIC -O2 -o "$WORK/quietcrash.so" "$S/quietcrash.c"
for svc in ca.desrt.dconf.service org.xfce.Xfconf.service; do
  [ -f /usr/share/dbus-1/services/$svc ] && ln -sf /usr/share/dbus-1/services/$svc "$WORK/services/$svc"
done
cat > "$WORK/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <keep_umask/>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <servicedir>$WORK/services</servicedir>
  <policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy>
</busconfig>
EOF

H=$WORK/home-$DE
rm -rf "$H"; mkdir -p "$H/run" "$H/.config" "$H/.local/share/wallpapers"; chmod 700 "$H/run"
cp /usr/share/pixmaps/archlinux-logo.png "$H/.local/share/wallpapers/test.png" 2>/dev/null \
  || magick -size 64x64 xc:red "$H/.local/share/wallpapers/test.png"

XCD= ; DS= ; EXTRA=
case $DE in
  kde) XCD=KDE; DS=plasma ;;
  gnome) XCD=GNOME; DS=gnome ;;
  cinnamon) XCD=X-Cinnamon; DS=cinnamon ;;
  mate) XCD=MATE; DS=mate ;;
  xfce) XCD=XFCE; DS=xfce ;;
  budgie) XCD=Budgie:GNOME; DS=budgie-desktop ;;
  # There are no elementary schemas on the machine: we take them from the sources (pinned
  # commits) together with the `:Pantheon` overrides from default-settings and
  # build the stand's schema directory — see pantheon_schemas below.
  pantheon) XCD=Pantheon; DS=pantheon; EXTRA="GSETTINGS_SCHEMA_DIR=$WORK/schemas-pantheon" ;;
  # Deepin: the deepin-desktop-schemas schemas (a pinned Arch package) — for the
  # terminal; the appearance service and DConfig — the stand-in model dde_shim.sh
  # first in PATH (the stand does not start live DDE services) — see deepin_stand below.
  deepin) XCD=DDE; DS=deepin
          EXTRA="GSETTINGS_SCHEMA_DIR=$WORK/schemas-deepin PATH=$WORK/deepin-shims:/usr/bin:/bin" ;;
  lxqt) XCD=LXQt; DS=lxqt ;;
  # start-cosmic sets this DCONF_PROFILE itself — we repeat it.
  cosmic) XCD=COSMIC; DS=cosmic; EXTRA="DCONF_PROFILE=/usr/share/dconf/profile/cosmic" ;;
  hyprland) XCD=Hyprland; DS=hyprland ;;
  sway) XCD=sway; DS=sway ;;
  niri) XCD=niri; DS=niri ;;
  *) echo "незнакомое окружение $DE"; exit 2 ;;
esac

# Pantheon: the schema directory = system schemas + elementary schemas + the
# `:Pantheon` overrides (without them gsettings does not know io.elementary.* and does not see the native
# Pantheon defaults). Nothing is installed into the system: the directory is
# in $WORK, connected through GSETTINGS_SCHEMA_DIR only for this run.
pantheon_schemas() {
  local src=$WORK/pantheon-src dst=$WORK/schemas-pantheon raw=https://raw.githubusercontent.com/elementary
  mkdir -p "$src"
  local f
  for f in \
    "settings-daemon/abe7f8bbfa5966416b362b78a36430ad0b2e602a/data/io.elementary.settings-daemon.gschema.xml" \
    "gala/f804092eca973f1a3dc3d2f069b7b9947a78eaa4/data/gala.gschema.xml" \
    "wingpanel/070d01ad1cbe2b2cec019367cc7910c8ff1f8cfa/schemas/io.elementary.desktop.wingpanel.gschema.xml" \
    "default-settings/4bd722fabbbc418b6f95ab181cc48f335547dd2c/overrides/default-settings.gschema.override.in"; do
    [ -s "$src/${f##*/}" ] || curl -fsSL -m 30 -o "$src/${f##*/}" "$raw/$f" || { echo "нет сети для $f"; exit 3; }
  done
  rm -rf "$dst"; mkdir -p "$dst"
  cp /usr/share/glib-2.0/schemas/*.xml "$dst/"
  cp "$src"/*.gschema.xml "$dst/"
  sed -e 's|@DEFAULT_WALLPAPER@|/usr/share/backgrounds/elementary/default.jpg|' \
      -e 's|@DEFAULT_WALLPAPER_DARK@|/usr/share/backgrounds/elementary/default.jpg|' \
      "$src/default-settings.gschema.override.in" > "$dst/90_pantheon-default-settings.gschema.override"
  glib-compile-schemas "$dst" 2>&1 | grep -v -E '^$|No such (schema|key)|ignoring' | head -5
}
[ "$DE" = pantheon ] && pantheon_schemas

# Deepin: the schema directory = system ones + the deepin-desktop-schemas schemas from the
# Arch package (downloaded into $WORK, nothing is installed into the system) and the stand-in
# `busctl`/`dde-dconfig` (dde_shim.sh) in their own PATH directory of the stand.
deepin_stand() {
  local src=$WORK/deepin-src dst=$WORK/schemas-deepin shims=$WORK/deepin-shims
  local pkg=deepin-desktop-schemas-6.0.13-1-any.pkg.tar.zst
  mkdir -p "$src"
  [ -s "$src/$pkg" ] || curl -fsSL -m 60 -o "$src/$pkg" "https://at.arch.mirror.kescher.at/extra/os/x86_64/$pkg" \
    || { echo "нет сети для $pkg"; exit 3; }
  rm -rf "$dst" "$src/x"; mkdir -p "$dst" "$src/x"
  tar -xf "$src/$pkg" -C "$src/x" usr/share/glib-2.0/schemas
  cp /usr/share/glib-2.0/schemas/*.xml "$dst/"
  cp "$src"/x/usr/share/glib-2.0/schemas/* "$dst/"
  glib-compile-schemas "$dst" 2>&1 | grep -v -E '^$|No such (schema|key)|ignoring' | head -5
  rm -rf "$shims"; mkdir -p "$shims"
  ln -s "$S/dde_shim.sh" "$shims/busctl"
  ln -s "$S/dde_shim.sh" "$shims/dde-dconfig"
}
[ "$DE" = deepin ] && deepin_stand

# XFCE: a monitor in xfce4-desktop, as in a live profile (otherwise the wallpaper has nowhere to go).
if [ "$DE" = xfce ]; then
  mkdir -p "$H/.config/xfce4/xfconf/xfce-perchannel-xml"
  cat > "$H/.config/xfce4/xfconf/xfce-perchannel-xml/xfce4-desktop.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<channel name="xfce4-desktop" version="1.0">
  <property name="backdrop" type="empty">
    <property name="screen0" type="empty">
      <property name="monitorVirtual-1" type="empty">
        <property name="workspace0" type="empty">
          <property name="last-image" type="string" value="/usr/share/backgrounds/xfce/xfce-shapes.svg"/>
        </property>
      </property>
    </property>
  </property>
</channel>
EOF
fi

ulimit -c 0
env -i PATH=/usr/bin:/bin HOME=$H XDG_RUNTIME_DIR=$H/run XDG_CONFIG_HOME=$H/.config \
  XDG_DATA_HOME=$H/.local/share XDG_CACHE_HOME=$H/.cache GTK_USE_PORTAL=0 GIO_USE_VFS=local \
  QT_QPA_PLATFORM=offscreen LANG=C.UTF-8 LANGUAGE=en \
  XDG_CURRENT_DESKTOP=$XCD DESKTOP_SESSION=$DS XDG_SESSION_TYPE=wayland $EXTRA \
  LD_PRELOAD=$WORK/quietcrash.so BIN=$BIN DE=$DE H=$H \
  timeout -k 5 300 dbus-run-session --config-file=$WORK/bus.conf -- bash $S/$INNER \
  > "$WORK/out-$DE${OUT_SUFFIX:-}.txt" 2>&1
echo "$DE → $WORK/out-$DE${OUT_SUFFIX:-}.txt"
