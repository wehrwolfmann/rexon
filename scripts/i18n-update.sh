#!/usr/bin/env bash
# Каталог перевода Rexon: извлечь строки, обновить переводы, собрать.
#
#   scripts/i18n-update.sh              — извлечь строки в po/rexon.pot и
#                                         подтянуть их во все po/<код>.po
#   scripts/i18n-update.sh --check      — ничего не менять; код 1, если
#                                         po/rexon.pot отстал от исходников
#   scripts/i18n-update.sh --new pl     — завести po/pl.po и вписать pl в LINGUAS
#   scripts/i18n-update.sh --build DIR  — собрать DIR/<код>/LC_MESSAGES/rexon.mo
#                                         для всех языков из LINGUAS
#
# Один .po на язык — один .mo, и он обслуживает ОБЕ стороны: Rust читает его
# модулем `rexon_core::i18n`, а QML получает строки из того же .mo через
# переводчик окна (`rexon_gui::install_app_translator`). Второго каталога
# (.qm) у Rexon нет — перевод хранится в одном месте.
# Сборку зовёт build.rs крейта rexon-gui при `cargo build-app`, так что
# руками `--build` нужен только для проверки.
#
# Ключевые слова извлечения — имена функций `rexon_core::i18n`
# (tr, trf, trc, ntr, n_) и `qsTranslate("rexon", …)` в QML. Переименовали
# функцию — поправьте RUST_KEYWORDS ниже.
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
# qsTranslate(контекст, текст[, уточнение[, число]]): контекст всегда
# "rexon" и в .po не пишется (см. `i18n::QT_CONTEXT`); уточнение становится
# msgctxt — как у `trc` в Rust.
QML_KEYWORDS=(-k -kqsTranslate:2,2t -kqsTranslate:2,3c,3t -kqsTranslate:2,3c,4t)

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "i18n-update: нет программы «$1» (${2})" >&2
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

# Извлечь шаблон в файл $1. Порядок файлов фиксирован (sort), чтобы
# повторный прогон без правок давал тот же шаблон; внутри файла строки идут
# в порядке исходника — переводчику так понятнее, что за чем на экране.
extract() {
  local out="$1" tmp="$WORK/extract"
  need xgettext "пакет gettext"
  need msguniq "пакет gettext"
  rm -rf "$tmp" && mkdir -p "$tmp" "$(dirname "$out")"
  (cd "$ROOT" && find "${RUST_SOURCES[@]#"$ROOT"/}" -name '*.rs' | LC_ALL=C sort) >"$tmp/rs.list"
  (cd "$ROOT" && find "${QML_SOURCES[@]#"$ROOT"/}" -name '*.qml' | LC_ALL=C sort) >"$tmp/qml.list"
  # Адреса для замечаний к строкам пока нет (репозиторий ещё не опубликован); xgettext
  # предупреждает о пустом каждый прогон — это предупреждение и глушим,
  # остальные доходят.
  # --add-location=file: ссылки без номеров строк — иначе любая правка выше
  # переводимой строки меняла бы шаблон и роняла --check без повода.
  local common=(--from-code=UTF-8 --add-comments=TRANSLATORS:
                --add-location=file
                --package-name=Rexon --msgid-bugs-address=
                --copyright-holder=Wehrwolfmann)
  # Один шаблон на обе стороны: QML дописывается к шаблону Rust ключом -j
  # (join-existing) — так ссылки на одну строку из .rs и .qml сливаются в
  # одну запись. (`msgcat --use-first` для этого не годится: он теряет
  # ссылки второго файла.)
  (cd "$ROOT" && xgettext -L Rust "${RUST_KEYWORDS[@]}" "${common[@]}" \
      -f "$tmp/rs.list" -o "$tmp/all.pot" 2>&1 | quiet_bugs_address >&2)
  # Строк в Rust может не оказаться — тогда шаблона ещё нет, и -j не к чему
  # дописывать.
  local join=()
  [[ -f "$tmp/all.pot" ]] && join=(-j)
  (cd "$ROOT" && xgettext -L JavaScript "${QML_KEYWORDS[@]}" "${common[@]}" "${join[@]}" \
      -f "$tmp/qml.list" -o "$tmp/all.pot" 2>&1 | quiet_bugs_address >&2)
  if [[ ! -f "$tmp/all.pot" ]]; then
    echo "i18n-update: переводимых строк не найдено" >&2
    exit 1
  fi
  # `qsTranslate("rexon", "%n file", "", n)` даёт пустой msgctxt, а пустой
  # контекст для gettext — НЕ отсутствие контекста: строка разошлась бы с
  # той же строкой из Rust. Снимаем и сливаем совпавшие записи.
  sed -i '/^msgctxt ""$/d' "$tmp/all.pot"
  msguniq "$tmp/all.pot" -o "$out"
  # Дата создания меняется на каждом прогоне — без неё шаблон в дереве
  # меняется только тогда, когда поменялись строки.
  sed -i -e '/^"POT-Creation-Date: /d' \
         -e '1s/^# SOME DESCRIPTIVE TITLE\.$/# Rexon user interface strings./' "$out"
}

cmd_update() {
  need msgmerge "пакет gettext"
  mkdir -p "$PO"
  extract "$POT"
  local lang
  for lang in $(languages); do
    if [[ -f "$PO/$lang.po" ]]; then
      msgmerge --quiet --update --backup=none --previous --add-location=file \
        "$PO/$lang.po" "$POT"
    else
      echo "i18n-update: $lang есть в LINGUAS, но нет po/$lang.po — заведите: $0 --new $lang" >&2
    fi
  done
  echo "шаблон: $(grep -c '^msgid ' "$POT") строк (включая заголовок) → $POT"
  for lang in $(languages); do
    [[ -f "$PO/$lang.po" ]] && printf '%s: ' "$lang" && msgfmt --statistics -o /dev/null "$PO/$lang.po"
  done
}

cmd_check() {
  local tmp="$WORK/check.pot"
  extract "$tmp"
  if ! diff -q "$tmp" "$POT" >/dev/null 2>&1; then
    echo "po/rexon.pot отстал от исходников — запустите scripts/i18n-update.sh" >&2
    diff -u "$POT" "$tmp" | head -40 >&2 || true
    return 1
  fi
  echo "po/rexon.pot совпадает с исходниками"
}

cmd_new() {
  local lang="$1"
  need msginit "пакет gettext"
  [[ "$lang" =~ ^[a-z]{2,3}(_[A-Z]{2})?$ ]] || {
    echo "i18n-update: код языка вида pl, pt_BR, cs — а не «$lang»" >&2
    exit 2
  }
  [[ -f "$POT" ]] || extract "$POT"
  if [[ -f "$PO/$lang.po" ]]; then
    echo "po/$lang.po уже есть" >&2
  else
    msginit --no-translator --locale="$lang.UTF-8" -i "$POT" -o "$PO/$lang.po"
  fi
  if ! languages | grep -qx "$lang"; then
    echo "$lang" >>"$PO/LINGUAS"
  fi
  echo "готово: переводите po/$lang.po"
}

cmd_build() {
  local dest="$1" lang
  need msgfmt "пакет gettext"
  for lang in $(languages); do
    local po="$PO/$lang.po" dir="$dest/$lang/LC_MESSAGES"
    [[ -f "$po" ]] || { echo "i18n-update: нет $po" >&2; exit 1; }
    mkdir -p "$dir"
    # --check: заголовок, формы по числу и форматы; нечёткие (fuzzy)
    # переводы msgfmt в .mo не кладёт.
    msgfmt --check -o "$dir/$DOMAIN.mo" "$po"
    # .qm прежних сборок (до 09.10.2026 QML читал свой каталог) — снять,
    # чтобы из переиспользованного каталога сборки в пакет не уехал
    # устаревший второй экземпляр перевода.
    rm -f "$dir/$DOMAIN.qm"
  done
}

case "${1:-}" in
  "") cmd_update ;;
  --check) cmd_check ;;
  --new) [[ -n "${2:-}" ]] || { echo "укажите код языка: --new pl" >&2; exit 2; }; cmd_new "$2" ;;
  --build) [[ -n "${2:-}" ]] || { echo "укажите каталог: --build DIR" >&2; exit 2; }; cmd_build "$2" ;;
  -h|--help) sed -n '2,20p' "$0" ;;
  *) echo "неизвестный ключ: $1 (см. --help)" >&2; exit 2 ;;
esac
