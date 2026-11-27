#!/usr/bin/env bash
# Rexon translation catalog: extract strings, update translations, build.
#
#   scripts/i18n-update.sh              — extract strings into po/rexon.pot and
#                                         merge them into all po/<code>.po
#   scripts/i18n-update.sh --check      — change nothing; exit code 1 if
#                                         po/rexon.pot is behind the sources
#   scripts/i18n-update.sh --new pl     — create po/pl.po and add pl to LINGUAS
#   scripts/i18n-update.sh --build DIR  — build DIR/<code>/LC_MESSAGES/rexon.mo
#                                         for all languages from LINGUAS
#   scripts/i18n-update.sh --verify DIR — exit code 1 if DIR/<code>/LC_MESSAGES/
#                                         rexon.mo is missing or was built not from
#                                         the current po/<code>.po (stale)
#
# One .po per language — one .mo, and it serves BOTH sides: Rust reads it
# with the `rexon_core::i18n` module, and QML gets strings from the same .mo through
# the window translator (`rexon_gui::install_app_translator`). Rexon has no second
# catalog (.qm) — the translation is stored in one place.
# The build is called by the build.rs of the rexon-gui crate during `cargo build-app`, so
# `--build` by hand is needed only for checking.
#
# Extraction keywords are the names of the `rexon_core::i18n` functions
# (tr, trf, trc, ntr, n_) and `qsTranslate("rexon", …)` in QML. If you rename a
# function — fix RUST_KEYWORDS below.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PO="$ROOT/po"
DOMAIN=rexon
POT="$PO/$DOMAIN.pot"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

RUST_SOURCES=("$ROOT/rust/rexon-core/src" "$ROOT/rust/rexon-gui/src")
QML_SOURCES=("$ROOT/rust/rexon-gui/qml")
RUST_KEYWORDS=(-k -kn_ -ktr -ktrf -ktrc:1c,2 -kntr:1,2)
# qsTranslate(context, text[, disambiguation[, number]]): the context is always
# "rexon" and is not written into the .po (see `i18n::QT_CONTEXT`); the disambiguation becomes
# msgctxt — like `trc` in Rust.
QML_KEYWORDS=(-k -kqsTranslate:2,2t -kqsTranslate:2,3c,3t -kqsTranslate:2,3c,4t)

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "i18n-update: program «$1» is missing (${2})" >&2
    exit 2
  }
}

languages() {
  [[ -f "$PO/LINGUAS" ]] || return 0
  sed -e 's/#.*//' "$PO/LINGUAS" | tr -s ' \t' '\n' | sed '/^$/d'
}

quiet_bugs_address() {
  grep -v -e 'msgid-bugs-address' -e 'MSGID_BUGS_ADDRESS' -e 'Makevars' || true
}

# Extract the template into file $1. The file order is fixed (sort) so that
# a repeated run without edits gives the same template; inside a file the strings go
# in source order — that way it is clearer to the translator what follows what on screen.
extract() {
  local out="$1" tmp="$WORK/extract"
  need xgettext "gettext package"
  need msguniq "gettext package"
  rm -rf "$tmp" && mkdir -p "$tmp" "$(dirname "$out")"
  (cd "$ROOT" && find "${RUST_SOURCES[@]#"$ROOT"/}" -name '*.rs' | LC_ALL=C sort) >"$tmp/rs.list"
  (cd "$ROOT" && find "${QML_SOURCES[@]#"$ROOT"/}" -name '*.qml' | LC_ALL=C sort) >"$tmp/qml.list"
  # There is no address for string remarks yet (the repository is not published yet); xgettext
  # warns about the empty one on every run — that warning is silenced,
  # the others get through.
  # --add-location=file: references without line numbers — otherwise any edit above
  # a translatable string would change the template and fail --check for no reason.
  local common=(--from-code=UTF-8 --add-comments=TRANSLATORS:
                --add-location=file
                --package-name=Rexon --msgid-bugs-address=
                --copyright-holder=Wehrwolfmann)
  # One template for both sides: QML is appended to the Rust template with -j
  # (join-existing) — that way references to one string from .rs and .qml merge into
  # one entry. (`msgcat --use-first` does not work for this: it loses
  # the references of the second file.)
  (cd "$ROOT" && xgettext -L Rust "${RUST_KEYWORDS[@]}" "${common[@]}" \
      -f "$tmp/rs.list" -o "$tmp/all.pot" 2>&1 | quiet_bugs_address >&2)
  # There may be no strings in Rust — then there is no template yet, and -j has nothing
  # to append to.
  local join=()
  [[ -f "$tmp/all.pot" ]] && join=(-j)
  (cd "$ROOT" && xgettext -L JavaScript "${QML_KEYWORDS[@]}" "${common[@]}" "${join[@]}" \
      -f "$tmp/qml.list" -o "$tmp/all.pot" 2>&1 | quiet_bugs_address >&2)
  if [[ ! -f "$tmp/all.pot" ]]; then
    echo "i18n-update: no translatable strings found" >&2
    exit 1
  fi
  # `qsTranslate("rexon", "%n file", "", n)` gives an empty msgctxt, and an empty
  # context for gettext is NOT the absence of a context: the string would diverge from
  # the same string from Rust. We remove it and merge the matching entries.
  sed -i '/^msgctxt ""$/d' "$tmp/all.pot"
  msguniq "$tmp/all.pot" -o "$out"
  # The creation date changes on every run — without it the template in the tree
  # changes only when the strings have changed.
  sed -i -e '/^"POT-Creation-Date: /d' \
         -e '1s/^# SOME DESCRIPTIVE TITLE\.$/# Rexon user interface strings./' "$out"
}

cmd_update() {
  need msgmerge "gettext package"
  mkdir -p "$PO"
  extract "$POT"
  local lang
  for lang in $(languages); do
    if [[ -f "$PO/$lang.po" ]]; then
      msgmerge --quiet --update --backup=none --previous --add-location=file \
        "$PO/$lang.po" "$POT"
    else
      echo "i18n-update: $lang is in LINGUAS, but there is no po/$lang.po — create it: $0 --new $lang" >&2
    fi
  done
  echo "template: $(grep -c '^msgid ' "$POT") strings (including the header) → $POT"
  for lang in $(languages); do
    [[ -f "$PO/$lang.po" ]] && printf '%s: ' "$lang" && msgfmt --statistics -o /dev/null "$PO/$lang.po"
  done
}

cmd_check() {
  local tmp="$WORK/check.pot"
  extract "$tmp"
  if ! diff -q "$tmp" "$POT" >/dev/null 2>&1; then
    echo "po/rexon.pot is behind the sources — run scripts/i18n-update.sh" >&2
    diff -u "$POT" "$tmp" | head -40 >&2 || true
    return 1
  fi
  echo "po/rexon.pot matches the sources"
}

cmd_new() {
  local lang="$1"
  need msginit "gettext package"
  [[ "$lang" =~ ^[a-z]{2,3}(_[A-Z]{2})?$ ]] || {
    echo "i18n-update: a language code like pl, pt_BR, cs — not «$lang»" >&2
    exit 2
  }
  [[ -f "$POT" ]] || extract "$POT"
  if [[ -f "$PO/$lang.po" ]]; then
    echo "po/$lang.po already exists" >&2
  else
    msginit --no-translator --locale="$lang.UTF-8" -i "$POT" -o "$PO/$lang.po"
  fi
  if ! languages | grep -qx "$lang"; then
    echo "$lang" >>"$PO/LINGUAS"
  fi
  echo "done: translate po/$lang.po"
}

cmd_build() {
  local dest="$1" lang
  need msgfmt "gettext package"
  for lang in $(languages); do
    local po="$PO/$lang.po" dir="$dest/$lang/LC_MESSAGES"
    [[ -f "$po" ]] || { echo "i18n-update: $po is missing" >&2; exit 1; }
    mkdir -p "$dir"
    # --check: header, plural forms and formats; msgfmt does not put fuzzy
    # translations into the .mo.
    msgfmt --check -o "$dir/$DOMAIN.mo" "$po"
    # .qm of earlier builds (before 09.10.2026 QML read its own catalog) — remove them,
    # so that an outdated second copy of the translation does not get into the package
    # from a reused build directory.
    rm -f "$dir/$DOMAIN.qm"
  done
}

# Guard against stale catalogs: the 15.11.2026 package shipped the .mo of the previous build
# (build.rs put the fresh ones in the wrong place, and cp took the old ones), and hints whose msgid
# had changed were shown in English. We compare not bytes but the catalog
# text (msgunfmt) — it does not depend on the msgfmt version that built the .mo.
cmd_verify() {
  local dir="$1" lang bad=0
  need msgfmt "gettext package"
  need msgunfmt "gettext package"
  for lang in $(languages); do
    local po="$PO/$lang.po" mo="$dir/$lang/LC_MESSAGES/$DOMAIN.mo"
    if [[ ! -f "$mo" ]]; then
      echo "i18n-update: $mo is missing" >&2; bad=1; continue
    fi
    msgfmt -o "$WORK/$lang.mo" "$po"
    msgunfmt "$WORK/$lang.mo" >"$WORK/$lang.want"
    msgunfmt "$mo" >"$WORK/$lang.have"
    if ! cmp -s "$WORK/$lang.want" "$WORK/$lang.have"; then
      local n
      n=$(diff "$WORK/$lang.want" "$WORK/$lang.have" | grep -c '^[<>] msgid ' || true)
      echo "i18n-update: $mo is stale — built not from the current po/$lang.po" \
        "(diverging msgid: $n; rebuild: --build $dir)" >&2
      bad=1
    fi
  done
  return "$bad"
}

case "${1:-}" in
  "") cmd_update ;;
  --check) cmd_check ;;
  --new) [[ -n "${2:-}" ]] || { echo "give a language code: --new pl" >&2; exit 2; }; cmd_new "$2" ;;
  --build) [[ -n "${2:-}" ]] || { echo "give a directory: --build DIR" >&2; exit 2; }; cmd_build "$2" ;;
  --verify) [[ -n "${2:-}" ]] || { echo "give a directory: --verify DIR" >&2; exit 2; }; cmd_verify "$2" ;;
  -h|--help) sed -n '2,23p' "$0" ;;
  *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
esac
