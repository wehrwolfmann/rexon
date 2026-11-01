#!/bin/bash
# Стенд «настройка × окружение» (проверка 02.10.2026).
#
#   run_de.sh <kde|gnome|cinnamon|mate|xfce|budgie|pantheon|deepin|lxqt|cosmic|hyprland|sway|niri>
#
# Применяет все настройки вкладок «Настройки» и «Оформление» тем же путём,
# что кнопки (пример `de_matrix_stand` в rexon-core), в ПРИВАТНОМ HOME под
# подставленным XDG_CURRENT_DESKTOP, и читает результат родными читателями
# окружения (gsettings, kreadconfig6, xfconf-query, qtxdg-mat,
# xdg-terminal-exec, файлы lxqt/cosmic/hypr/sway/niri) — см. inner.sh.
# Итог — $WORK/out-<de>.txt.
#
# Безопасность (всё обязательно):
# * HOME, XDG_CONFIG_HOME, XDG_RUNTIME_DIR — свои; WAYLAND_DISPLAY, SWAYSOCK,
#   HYPRLAND_INSTANCE_SIGNATURE, NIRI_SOCKET не передаются (env -i) — ни одна
#   команда не дотянется до живого сеанса;
# * своя шина, и на ней поднимаются ТОЛЬКО dconf и xfconfd (белый список
#   service-файлов). ⛔ Не открывать шину на все системные сервисы: kded6
#   активируется по шине, тянет автозапуск Plasma, и упавший без экрана
#   plasmashell через KCrash доходит до DrKonqi живого стола (02.10 так и было);
# * тихий обработчик краха (LD_PRELOAD) + ulimit -c 0. ⚠️ Он НЕ защищает
#   программы с file capabilities (sway: LD_PRELOAD игнорируется) и программы
#   с KCrash — поэтому `sway -C`, kded6, plasmashell стенд не запускает.
set -u
# Какой сценарий гнать внутри шины: inner.sh (все разделы) или inner_look.sh
# («Анимация» и «Кнопки заголовка окна», пример look_stand).
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
  # Схем elementary на машине нет: берём их из исходников (закреплённые
  # коммиты) вместе с переопределениями `:Pantheon` из default-settings и
  # собираем каталог схем стенда — см. pantheon_schemas ниже.
  pantheon) XCD=Pantheon; DS=pantheon; EXTRA="GSETTINGS_SCHEMA_DIR=$WORK/schemas-pantheon" ;;
  # Deepin: схемы deepin-desktop-schemas (закреплённый пакет Arch) — для
  # терминала; служба оформления и DConfig — подставная модель dde_shim.sh
  # первой в PATH (живых служб DDE стенд не запускает) — см. deepin_stand ниже.
  deepin) XCD=DDE; DS=deepin
          EXTRA="GSETTINGS_SCHEMA_DIR=$WORK/schemas-deepin PATH=$WORK/deepin-shims:/usr/bin:/bin" ;;
  lxqt) XCD=LXQt; DS=lxqt ;;
  # start-cosmic сам ставит этот DCONF_PROFILE — повторяем его.
  cosmic) XCD=COSMIC; DS=cosmic; EXTRA="DCONF_PROFILE=/usr/share/dconf/profile/cosmic" ;;
  hyprland) XCD=Hyprland; DS=hyprland ;;
  sway) XCD=sway; DS=sway ;;
  niri) XCD=niri; DS=niri ;;
  *) echo "незнакомое окружение $DE"; exit 2 ;;
esac

# Pantheon: каталог схем = системные схемы + схемы elementary + переопределения
# `:Pantheon` (без них gsettings не знает io.elementary.* и не видит родных
# значений по умолчанию Pantheon). Ничего не ставится в систему: каталог —
# в $WORK, подключается через GSETTINGS_SCHEMA_DIR только этому запуску.
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

# Deepin: каталог схем = системные + схемы deepin-desktop-schemas из пакета
# Arch (скачивается в $WORK, в систему ничего не ставится) и подставные
# `busctl`/`dde-dconfig` (dde_shim.sh) в своём каталоге PATH стенда.
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

# XFCE: монитор в xfce4-desktop, как у живого профиля (иначе обоям некуда).
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
