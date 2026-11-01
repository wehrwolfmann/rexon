#!/bin/bash
# Вызывается из run_de.sh (INNER=inner_sections.sh) внутри своей шины:
# кнопки разделов окружений «Оформления» (решение 04.10.2026) — «Применить»
# у тем, «Включить»/«Выключить» у дополнений, «Запустить»/«Остановить» у
# Conky. Действие стенда → чтение родным читателем окружения → правда
# `section-state`. Отдельно не запускать.
C=$XDG_CONFIG_HOME
S=$H/.local/share
export PATH="$(dirname "$H")/fakebin:$PATH"   # подставной conky (fake_conky.c)
st() { echo; echo "### $*"; timeout -k 2 40 "$BIN" "$@" 2>&1 | sed 's/^/    /'; }
gs() { printf '  gsettings %-42s %-22s = %s\n' "$1" "$2" "$(timeout 5 gsettings get "$1" "$2" 2>&1)"; }
kr() { printf '  kreadconfig6 %-8s [%s] %-18s = %s\n' "$1" "$2" "$3" "$(timeout 5 kreadconfig6 --file "$1" --group "$2" --key "$3" 2>&1)"; }
xq() { printf '  xfconf-query -c %-6s -p %-16s = %s\n' "$1" "$2" "$(timeout 5 xfconf-query -c "$1" -p "$2" 2>&1)"; }
ini() { local v; v=$(awk -v s="[$2]" -v k="$3" '$0==s{f=1;next} /^\[/{f=0} f&&index($0,k"=")==1{sub(/^[^=]*=/,"");print}' "$1" 2>/dev/null); printf '  ini %-26s [%s] %-8s = %s\n' "${1#$H/}" "$2" "$3" "${v:-<нет>}"; }
theme() { mkdir -p "$H/.themes/RexTest/$1"; echo "x" > "$H/.themes/RexTest/$1/themerc"; }
meta() { mkdir -p "$1"; echo "$2" > "$1/metadata.json"; }

st de
case $DE in
  xfce)
    theme xfwm4
    xq xfwm4 /general/theme; st section-state xfwm4
    st section xfwm4 RexTest
    xq xfwm4 /general/theme; st section-state xfwm4 ;;
  mate)
    theme metacity-1
    gs org.mate.Marco.general theme
    st section marco RexTest
    gs org.mate.Marco.general theme; st section-state marco ;;
  gnome)
    theme gnome-shell
    st section gnomeshell RexTest
    gs org.gnome.shell.extensions.user-theme name
    meta "$S/gnome-shell/extensions/rex@test" '{"uuid": "rex@test", "name": "Rex", "shell-version": ["48"]}'
    gs org.gnome.shell enabled-extensions
    st section gnomeext rex@test
    gs org.gnome.shell enabled-extensions; st section-state gnomeext
    st section gnomeext rex@test
    gs org.gnome.shell enabled-extensions; st section-state gnomeext ;;
  cinnamon)
    theme cinnamon
    gs org.cinnamon.theme name
    st section cinnamontheme RexTest
    gs org.cinnamon.theme name
    meta "$S/cinnamon/applets/rex@test" '{"uuid": "rex@test", "name": "Rex"}'
    st section cinnamonapplets rex@test
    gs org.cinnamon enabled-applets; gs org.cinnamon next-applet-id; st section-state cinnamonapplets
    st section cinnamonapplets rex@test
    gs org.cinnamon enabled-applets; st section-state cinnamonapplets
    meta "$S/cinnamon/desklets/rexd@test" '{"uuid": "rexd@test"}'
    st section cinnamondesklets rexd@test
    gs org.cinnamon enabled-desklets
    meta "$S/cinnamon/extensions/rexe@test" '{"uuid": "rexe@test"}'
    st section cinnamonext rexe@test
    gs org.cinnamon enabled-extensions
    mkdir -p "$S/nemo/actions"; printf '[Nemo Action]\nName=Rex\nExec=true\n' > "$S/nemo/actions/rex.nemo_action"
    st section-state nemoactions
    st section nemoactions rex.nemo_action
    gs org.nemo.plugins disabled-actions; st section-state nemoactions
    st section nemoactions rex.nemo_action
    gs org.nemo.plugins disabled-actions; st section-state nemoactions ;;
  lxqt)
    mkdir -p "$S/lxqt/themes/RexTest"; : > "$S/lxqt/themes/RexTest/lxqt-panel.qss"
    st section lxqttheme RexTest
    ini "$C/lxqt/lxqt.conf" General theme; st section-state lxqttheme ;;
  kde)
    meta "$S/kwin/tabbox/rex.switcher" '{"KPlugin": {"Id": "rex.switcher"}}'
    st section tabbox rex.switcher
    kr kwinrc TabBox LayoutName; st section-state tabbox
    meta "$S/kwin/scripts/rexscript" '{"KPlugin": {"Id": "rexscript"}}'
    st section kwinscripts rexscript
    kr kwinrc Plugins rexscriptEnabled; st section-state kwinscripts
    st section kwinscripts rexscript
    kr kwinrc Plugins rexscriptEnabled; st section-state kwinscripts
    meta "$S/kwin/effects/rexeffect" '{"KPlugin": {"Id": "rexeffect"}}'
    st section kwineffects rexeffect
    kr kwinrc Plugins rexeffectEnabled ;;
esac

# Conky — в любом окружении (отдельная программа).
mkdir -p "$C/conky/rexw"; echo 'conky.text = [[x]]' > "$C/conky/rexw/conky.conf"
st section-state conky
st section conky rexw
st section-state conky
st conky-autostart rexw on
ls "$C/autostart/"
st conky-autostart rexw off
ls "$C/autostart/" 2>&1
st section conky rexw
st section-state conky
