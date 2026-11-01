#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Синхронизация вендорной разметки waywallen → Rexon (одна команда).

Назначение
==========
Вкладка «Wallpaper Engine» показывает QML-страницы проекта **waywallen**
(MIT, © hypengw) как есть: каталог
`data/qt_wallpaper_engine/qml_ui/waywallen/ui` — их `ui/qml`, `ui/i18n` и
две картинки из `ui/assets`, вендоренные ДОСЛОВНО с тега из `ui/UPSTREAM_TAG`.
Типы, которые у них печатает C++, у нас печатают крейты `rexon-gui` и
`rexon-qt-filter` под тем же именем модуля `waywallen.ui`.

Раньше выпуск апстрима переносился руками, и руками же делались три вещи,
каждая из которых умеет молча испортить вкладку:

  * `ui/qmldir` дописывался построчно, хотя состав модуля лежит у апстрима
    готовым — списками `qml_files` и `singletons` в `ui/build.lua`;
  * наши правки жили ВНУТРИ вендорных файлов с пометкой «при обновлении
    перенеси сюда же» — работает, пока помнишь;
  * новое имя типа `W.Foo`, появившееся у апстрима, обнаруживалось только
    пустой страницей на живом запуске.

Этот скрипт за один запуск:

  1. забирает `ui/qml`, каталоги перевода `ui/i18n/waywallen_{ru,de}.ts` и
     вендоримые `ui/assets` с заданного тега апстрима (локальный клон ИЛИ
     git) — побайтово;
  2. ГЕНЕРИРУЕТ `ui/qmldir` из `ui/build.lua` того же тега (рукописных
     строк в нём не остаётся ни одной) — того же вида, что пишет генератор
     модуля апстрима (lito-qt `qt.qml_module`, `prefer_resources = false`):
     `module`, `typeinfo module.qmltypes`, компоненты с версией модуля
     апстрима, `depends` по списку `imports`. Рядом лежит `module.qmltypes`
     того же тега — опись C++-типов из сборки апстрима (в исходниках её
     нет); при смене тега её нужно положить заново, иначе код 4;
  3. накладывает нашу патч-серию `patches/NNNN-*.patch` (`git apply`);
     патч, чью правку апстрим уже принял сам (бэкпорт), распознаётся по
     тому, что ложится задом наперёд, — он пропускается и называется
     лишним, а не обрывает синхронизацию;
  4. печатает имена типов `W.*`, которые вендорный QML зовёт, а наш модуль
     не регистрирует — то есть ровно те строчки, которые «забыли»;
  5. записывает `ui/UPSTREAM_TAG` и `ui/QML_MATERIAL_PIN` — ревизию
     библиотеки `Qcm.Material` из `lito.lock` того же выпуска, чтобы
     разметка и библиотека под ней не разъезжались (её берёт
     `tools/build_qml_material.sh`);
  6. выписывает `tools/qextra_plugin/lito.lock` — срез того же `lito.lock`:
     ревизию QExtra (`QE.AnimatedImage`, `QE.ImageCache`) со всеми её
     зависимостями, с которой `tools/build_qml_qextra.sh` собирает
     настоящий плагин; и называет типы `QE.*`, которых нет у нашей
     QML-подмены `qml_ui/QExtra/`.

Собирается всё в отдельном каталоге и переносится в дерево ТОЛЬКО когда
прошли все шаги. Не наложился патч — дерево не тронуто, код возврата != 0.
То есть «или синхронизировано целиком, или ничего не сломано».

Использование
=============
    # из локального клона (по умолчанию ~/Загрузки/owe-ref/waywallen),
    # тег берётся из ui/UPSTREAM_TAG:
    scripts/sync-waywallen-qml.py
    scripts/sync-waywallen-qml.py --source /path/to/waywallen --ref v0.3.9

    # напрямую из git-апстрима (клонирует во временную папку):
    scripts/sync-waywallen-qml.py --git https://github.com/hypengw/waywallen \\
        --ref v0.3.9

    # только проверить расхождение, ничего не писать (для CI/cron):
    scripts/sync-waywallen-qml.py --check

Полная автоматизация
====================
`--check` даёт exit 0 (дерево совпадает с «апстрим + наши патчи») /
3 (есть расхождение) / 1 (патч не наложился) / 2 (нет источника) /
4 (сошлось, но у апстрима появился незарегистрированный тип `W.*` или
`QE.*`) — этого
хватает и для CI-гейта, и для cron-вотчера: дёргать `--git <url> --ref
<новый тег> --check`, при коде 3 открывать задачу на обновление вендора.
"""
from __future__ import annotations

import argparse
import filecmp
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

# ---------------------------------------------------------------------------
# Раскладка
# ---------------------------------------------------------------------------

_HERE = os.path.dirname(os.path.abspath(__file__))
_REPO = os.path.dirname(_HERE)

#: Корень вендора: тут лежат ui/, patches/, LICENSE и ATTRIBUTION.md.
VENDOR_DIR = os.path.join(
    _REPO, "data", "qt_wallpaper_engine", "qml_ui", "waywallen")
#: Само вендорное дерево — его скрипт целиком пересобирает.
UI_DIR = os.path.join(VENDOR_DIR, "ui")
#: Наша патч-серия поверх вендора (накладывается по возрастанию имени).
PATCH_DIR = os.path.join(VENDOR_DIR, "patches")

#: Что берём с апстрима побайтово. Каталог `ui/qml` целиком плюс отдельные
#: файлы: каталоги переводов (каждый компилируется в свой
#: `waywallen_<язык>.qm`; немецкий у апстрима с 0.5.0) и ровно те два
#: файла `assets`, которые апстрим кладёт в свой QML-модуль (`resources` в
#: build.lua) — остальное его `assets` читает пакет, а не QML.
UPSTREAM_TREE = "ui/qml"
UPSTREAM_FILES = (
    "ui/i18n/waywallen_de.ts",
    "ui/i18n/waywallen_ru.ts",
    "ui/assets/waywallen-ui.svg",
    "ui/assets/waywallen-ui.releases.xml",
)
#: Описание сборки апстрима — источник состава QML-модуля. Сам не вендорится.
BUILD_LUA = "ui/build.lua"
#: Замок зависимостей апстрима — источник ревизии QmlMaterial (`Qcm.Material`),
#: которую собирает `tools/build_qml_material.sh`. Сам не вендорится.
LOCK_FILE = "lito.lock"
#: Куда записывается эта ревизия. Пин обязан ехать ВМЕСТЕ с тегом вендора:
#: разъехавшись, он даёт самый неприятный класс дефектов — вендорный QML
#: новый, а библиотека под ним старая, и расхождение видно только глазами на
#: живой странице (пойманный случай — заголовочный регистр русских подписей,
#: починенный у hypengw 043c09e и месяц не доезжавший до нашей сборки).
MATERIAL_PIN_NAME = "QML_MATERIAL_PIN"

#: Обёртка плагина QExtra: её `lito.lock` — срез замка апстрима, с ним
#: `tools/build_qml_qextra.sh` собирает настоящий `QExtra` (`--locked`).
#: Срез едет ВМЕСТЕ с тегом вендора по той же причине, что и пин выше:
#: разметка зовёт `QE.*` той ревизии, с которой собран выпуск.
QEXTRA_WRAPPER_DIR = os.path.join(_REPO, "tools", "qextra_plugin")
QEXTRA_LOCK_PATH = os.path.join(QEXTRA_WRAPPER_DIR, LOCK_FILE)
QEXTRA_LOCK_REL = os.path.relpath(QEXTRA_LOCK_PATH, _REPO)
#: Имя среза в каталоге кандидата (рядом с `ui/`, в дерево вендора не идёт).
QEXTRA_STAGE_NAME = "qextra.lito.lock"
#: Наша QML-подмена модуля `QExtra` — её берёт ядро, когда настоящего
#: плагина нет или он не годится (`qml_modules::choose_qextra`), и всегда
#: под `offscreen`. Поэтому каждый `QE.*` вендора обязан быть и в ней.
QEXTRA_SHIM_QMLDIR = os.path.join(
    _REPO, "data", "qt_wallpaper_engine", "qml_ui", "QExtra", "qmldir")

#: Имя QML-модуля. Версию компонентов qmldir берёт из `version` в
#: `ui/build.lua` (как lito-qt), а C++-типы Rust регистрирует как 1.0:
#: вендорный QML импортирует модуль без версии (`import waywallen.ui as W`)
#: и видит обе половины — это держит случай «вендорный I18n зовёт наш
#: PluginTranslations» в `rust/rexon-gui/src/qml_int64_probe.rs`.
QML_MODULE = "waywallen.ui"

#: Опись C++-типов модуля (`module.qmltypes`) из сборки апстрима. В исходниках
#: её нет: её печатает qmltyperegistrar при `lito build -p waywallen-ui`
#: (`build/<профиль>/generated/waywallen-ui/lito-qml/waywallen/ui/`). Сверку
#: с нашими регистрациями делает `rust/rexon-gui/tests/waywallen_qmltypes_contract.rs`.
QMLTYPES_NAME = "module.qmltypes"

DEFAULT_SOURCE = os.path.expanduser("~/Загрузки/owe-ref/waywallen")

#: Где искать регистрации типов модуля: C++-шимы наших крейтов зовут
#: `qmlRegisterType<T>("waywallen.ui", 1, 0, "T")` строковым литералом.
REGISTRAR_DIRS = (
    os.path.join(_REPO, "rust", "rexon-gui", "cpp"),
    os.path.join(_REPO, "rust", "rexon-qt-filter", "cpp"),
)


def _eprint(*a) -> None:
    print(*a, file=sys.stderr)


def _read(path: str) -> str:
    with open(path, encoding="utf-8") as fh:
        return fh.read()


# ---------------------------------------------------------------------------
# Тег апстрима
# ---------------------------------------------------------------------------

def _tag_file() -> str:
    return os.path.join(UI_DIR, "UPSTREAM_TAG")


def _current_ref() -> str:
    """Тег, на котором сейчас вендор: первое слово `ui/UPSTREAM_TAG`."""
    path = _tag_file()
    if not os.path.isfile(path):
        return ""
    first = _read(path).strip().split()
    return first[0] if first else ""


# ---------------------------------------------------------------------------
# Апстрим: выкладка нужного поддерева во временный каталог
# ---------------------------------------------------------------------------

def _git(args: list[str], cwd: str | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd,
                          capture_output=True, text=True)


def _upstream_tree(args) -> tuple[str, str, tempfile.TemporaryDirectory]:
    """Выложить нужные пути апстрима во временный каталог.

    Возвращает (каталог, разрешённый ref в виде `<ref> (<короткий sha>)`,
    временный объект — его cleanup зовёт вызывающий).
    """
    tmp = tempfile.TemporaryDirectory(prefix="waywallen-qml-")
    ref = args.ref or _current_ref()
    if not ref:
        tmp.cleanup()
        _eprint("[err] тег не задан и не читается из ui/UPSTREAM_TAG — "
                "укажите --ref")
        raise SystemExit(2)

    if args.git:
        repo = os.path.join(tmp.name, "clone")
        cmd = ["clone", "--depth", "1", "--branch", ref, args.git, repo]
        _eprint(f"[git] клонирую {args.git} (ref={ref})…")
        res = _git(cmd)
        if res.returncode != 0:
            tmp.cleanup()
            _eprint("[git] клонирование не удалось:\n"
                    + (res.stderr or res.stdout).strip())
            raise SystemExit(2)
        source, archive_ref = repo, "HEAD"
    else:
        source, archive_ref = args.source, ref
        if not os.path.isdir(os.path.join(source, ".git")):
            tmp.cleanup()
            _eprint(f"[err] не клон waywallen: {source}")
            raise SystemExit(2)

    sha = _git(["rev-parse", "--short", archive_ref], cwd=source)
    if sha.returncode != 0:
        tmp.cleanup()
        _eprint(f"[err] нет такого ref в {source}: {ref}\n"
                + (sha.stderr or "").strip())
        raise SystemExit(2)

    out = os.path.join(tmp.name, "upstream")
    os.makedirs(out, exist_ok=True)
    paths = [UPSTREAM_TREE, *UPSTREAM_FILES, BUILD_LUA, LOCK_FILE]
    tar = subprocess.run(["git", "archive", archive_ref, *paths],
                         cwd=source, capture_output=True)
    if tar.returncode != 0:
        tmp.cleanup()
        _eprint("[git] archive не удался:\n"
                + tar.stderr.decode("utf-8", "replace").strip())
        raise SystemExit(2)
    untar = subprocess.run(["tar", "-x", "-C", out], input=tar.stdout,
                           capture_output=True)
    if untar.returncode != 0:
        tmp.cleanup()
        _eprint("[tar] распаковка не удалась:\n"
                + untar.stderr.decode("utf-8", "replace").strip())
        raise SystemExit(2)
    return out, f"{ref} ({sha.stdout.strip()})", tmp


# ---------------------------------------------------------------------------
# qmldir из build.lua
# ---------------------------------------------------------------------------

_LUA_LIST_RE = r"{}\s*=\s*\{{(.*?)\}}"


def _lua_list(text: str, key: str) -> list[str]:
    """Строки из lua-списка `key = { "a", "b" }` в порядке объявления."""
    m = re.search(_LUA_LIST_RE.format(re.escape(key)), text, re.S)
    if not m:
        return []
    return re.findall(r'"([^"]+)"', m.group(1))


def _qmldir_text(build_lua: str, ref: str) -> str:
    """Собрать `ui/qmldir` из `qt.qml_module` апстримного `ui/build.lua`.

    Тот же текст, что пишет lito-qt (`generate_qmldir` в `qt/qml.lua`) для
    разложенной копии без `prefer`: тип называется по имени файла,
    синглтоны помечаются словом `singleton`, версия — `version` модуля,
    `depends` — его `imports`. Комментариев lito-qt не пишет, и мы тоже:
    тогда вендорная копия и установленный модуль взаимозаменяемы.
    """
    del ref  # тег записан в UPSTREAM_TAG; в qmldir его у апстрима нет
    files = _lua_list(build_lua, "qml_files")
    singletons = set(_lua_list(build_lua, "singletons"))
    imports = _lua_list(build_lua, "imports")
    version = re.search(r'qt\.qml_module\(\{.*?\bversion\s*=\s*"([^"]+)"',
                        build_lua, re.S)
    if not version:
        raise RuntimeError("в ui/build.lua не нашлось version у qt.qml_module")
    if not files:
        raise RuntimeError("в ui/build.lua не нашлось списка qml_files")
    unknown = singletons - set(files)
    if unknown:
        raise RuntimeError("синглтоны апстрима вне списка qml_files: "
                           + ", ".join(sorted(unknown)))

    lines = [f"module {QML_MODULE}", f"typeinfo {QMLTYPES_NAME}"]
    for rel in files:
        name = os.path.basename(rel)[: -len(".qml")]
        prefix = "singleton " if rel in singletons else ""
        lines.append(f"{prefix}{name} {version.group(1)} {rel}")
    lines += [f"depends {name}" for name in imports]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# Ревизия QmlMaterial из замка апстрима
# ---------------------------------------------------------------------------

_MATERIAL_RE = re.compile(
    r'name\s*=\s*"qml-material"\s*\n\s*source\s*=\s*"[^"#]*#([0-9a-f]{7,40})"')


def _material_pin(lock_text: str) -> str:
    """Коммит QmlMaterial, на котором собран этот выпуск waywallen."""
    m = _MATERIAL_RE.search(lock_text)
    if not m:
        raise RuntimeError("в lito.lock апстрима не нашлась ревизия "
                           "qml-material")
    return m.group(1)


# ---------------------------------------------------------------------------
# Срез замка апстрима для плагина QExtra
# ---------------------------------------------------------------------------

_LOCK_BLOCK_RE = re.compile(r"(?m)^(?=\[\[packages\]\]\n)")
_QEXTRA_PIN_RE = re.compile(r'QExtra\.git#([0-9a-f]{7,40})')


def _qextra_pin(lock_text: str) -> str:
    """Коммит QExtra из замка (апстрима или нашего среза); '' — не найден."""
    m = _QEXTRA_PIN_RE.search(lock_text)
    return m.group(1) if m else ""


def _qextra_lock(lock_text: str) -> str:
    """Срез замка апстрима: всё, от чего зависит обёртка плагина QExtra.

    Целиком чужой замок обёртке не годится: lito отвергает пакеты, которых
    нет в её графе. Берётся замыкание зависимостей обёртки (они записаны в
    её `lito.toml`) плюс запись самой обёртки; блоки — по алфавиту, как их
    пишет lito, иначе `--locked` сочтёт файл изменённым.
    """
    with open(os.path.join(QEXTRA_WRAPPER_DIR, "lito.toml"), "rb") as fh:
        manifest = tomllib.load(fh)
    roots = sorted(manifest.get("dependencies", {}))
    packages = {p["name"]: p for p in tomllib.loads(lock_text)["packages"]}
    need: set[str] = set()
    todo = list(roots)
    while todo:
        name = todo.pop()
        if name in need:
            continue
        if name not in packages:
            raise RuntimeError(f"в lito.lock апстрима нет пакета «{name}», "
                               f"нужного плагину QExtra ({QEXTRA_LOCK_REL})")
        need.add(name)
        todo += packages[name].get("dependencies", [])
    head, *raw = _LOCK_BLOCK_RE.split(lock_text)
    blocks: dict[str, str] = {}
    for block in raw:
        name = re.search(r'name = "([^"]+)"', block).group(1)
        if name in need:
            blocks[name] = block.strip("\n") + "\n"
    pkg = manifest["package"]
    deps = "".join(f'  "{r}",\n' for r in roots)
    blocks[pkg["name"]] = (f'[[packages]]\nname = "{pkg["name"]}"\n'
                           f'version = "{pkg["version"]}"\n'
                           f'dependencies = [\n{deps}]\n')
    return head.strip("\n") + "\n\n" + "\n".join(
        blocks[k] for k in sorted(blocks))


def _qextra_lock_drift(stage_lock: str) -> str:
    """'' — срез в дереве совпадает с кандидатом; иначе — чем разошёлся."""
    new = _read(stage_lock)
    try:
        old = _read(QEXTRA_LOCK_PATH)
    except OSError:
        return "файла в дереве нет"
    if old == new:
        return ""
    was, now = _qextra_pin(old), _qextra_pin(new)
    if was != now:
        return f"QExtra сменился: {was[:7] or '?'} → {now[:7] or '?'}"
    return "ревизия QExtra та же, сменились его зависимости"


def _check_qextra_lock(stage_lock: str) -> int:
    drift = _qextra_lock_drift(stage_lock)
    if not drift:
        print(f"[check] OK: {QEXTRA_LOCK_REL} совпадает со срезом lito.lock "
              f"апстрима")
        return 0
    _eprint(f"[check] РАСХОЖДЕНИЕ: {QEXTRA_LOCK_REL} ({drift})")
    return 3


def _install_qextra_lock(stage_lock: str) -> None:
    drift = _qextra_lock_drift(stage_lock)
    if not drift:
        return
    shutil.copyfile(stage_lock, QEXTRA_LOCK_PATH)
    print(f"[sync] обновлён: {QEXTRA_LOCK_REL} ({drift})")
    print("[sync] после этого: пересобрать плагин tools/build_qml_qextra.sh "
          "и обновить списки _qe_git/_qe_reg в packaging/aur/PKGBUILD "
          "(его prepare() сверяет их с этим файлом)")


# ---------------------------------------------------------------------------
# Патч-серия
# ---------------------------------------------------------------------------

def _patches() -> list[str]:
    if not os.path.isdir(PATCH_DIR):
        return []
    return sorted(f for f in os.listdir(PATCH_DIR) if f.endswith(".patch"))


def _apply_patches(root: str) -> list[str]:
    """Наложить серию на выложенное дерево. Не лёг патч — RuntimeError.

    `root` — каталог, внутри которого лежит `ui/`: пути в патчах записаны
    от него (`a/ui/qml/...`), так что серия ложится `git apply -p1`.

    ⭐ Не наложившийся патч — ещё не конфликт. В серии живут БЭКПОРТЫ: мы
    берём правку, которую апстрим уже принял, но которая приехала позже
    тега вендора, и ждать её бумпа не можем. Такой патч обязан перестать
    накладываться ровно тогда, когда вендор дорастёт до выпуска с этой
    правкой, — и прежнее правило «первый непришедшийся патч обрывает ВСЮ
    синхронизацию» превращало собственную починку в замок на бумпе
    (пойманный случай 19.09.2026). Поэтому перед приговором проверяем
    обратное наложение: легло задом наперёд — правка УЖЕ в дереве
    апстрима, патч пропускается и называется лишним. Не легло ни так, ни
    так — вот это настоящий конфликт.

    Возвращает имена патчей, которые апстрим уже вобрал в себя: их место
    в сводке, а не в тишине, иначе серия обрастёт мёртвыми файлами.
    """
    series = _patches()
    if not series:
        print("[patch] серия пуста — накладывать нечего")
        return []
    obsolete: list[str] = []
    for name in series:
        path = os.path.join(PATCH_DIR, name)
        res = _git(["apply", "--whitespace=nowarn", path], cwd=root)
        if res.returncode == 0:
            print(f"[patch] наложен: {name}")
            continue
        back = _git(["apply", "--reverse", "--check", "--whitespace=nowarn",
                     path], cwd=root)
        if back.returncode == 0:
            obsolete.append(name)
            print(f"[patch] УЖЕ У АПСТРИМА, пропущен: {name}")
            continue
        raise RuntimeError(
            f"патч {name} НЕ НАЛОЖИЛСЯ на апстрим:\n"
            + (res.stderr or res.stdout).strip()
            + "\n\nЭто настоящий конфликт: апстрим тронул те же строки "
              "(правки апстрима в этом месте у нас НЕТ — иначе патч лёг бы "
              "задом наперёд и был бы пропущен).\nПеренесите правку руками "
              "и пересоберите патч (`git diff` по вендорному файлу → "
            + f"{os.path.relpath(path, _REPO)}).")
    if obsolete:
        print("[patch] ⚠️ апстрим вобрал наши правки — эти файлы серии "
              "больше не нужны, удалите их вместе с бумпом вендора:")
        for name in obsolete:
            print(f"[patch]   patches/{name}")
    return obsolete


# ---------------------------------------------------------------------------
# Новые имена типов W.*
# ---------------------------------------------------------------------------

_REGISTER_RE = re.compile(
    r'"' + re.escape(QML_MODULE) + r'"\s*,\s*\d+\s*,\s*\d+\s*,\s*"(\w+)"')
_USE_RE = re.compile(r"\bW\.([A-Z]\w*)")
_USE_QE_RE = re.compile(r"\bQE\.([A-Z]\w*)")


def _registered_types() -> set[str]:
    """Имена, которые наши крейты регистрируют в модуле `waywallen.ui`."""
    names: set[str] = set()
    for d in REGISTRAR_DIRS:
        if not os.path.isdir(d):
            continue
        for entry in sorted(os.listdir(d)):
            if entry.endswith((".cpp", ".h", ".hpp")):
                names |= set(_REGISTER_RE.findall(
                    _read(os.path.join(d, entry))))
    return names


def _qmldir_types(text: str) -> set[str]:
    names: set[str] = set()
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.split()[0] in (
                "module", "typeinfo", "depends", "prefer"):
            continue
        parts = line.split()
        if parts[0] == "singleton":
            parts = parts[1:]
        if parts:
            names.add(parts[0])
    return names


def _used_types(qml_root: str,
                use_re: re.Pattern = _USE_RE) -> dict[str, list[str]]:
    """Имена `W.Foo`, встреченные в вендорном QML → файлы, где встречены."""
    used: dict[str, list[str]] = {}
    for base, _dirs, files in os.walk(qml_root):
        for f in sorted(files):
            if not f.endswith(".qml"):
                continue
            path = os.path.join(base, f)
            rel = os.path.relpath(path, os.path.dirname(qml_root))
            for name in sorted(set(use_re.findall(_read(path)))):
                used.setdefault(name, []).append(rel)
    return used


def _report_new_types(stage_ui: str, qmldir_text: str) -> list[str]:
    """Напечатать типы `W.*`, которых наш модуль не даёт. Вернуть их список."""
    known = _registered_types() | _qmldir_types(qmldir_text)
    used = _used_types(os.path.join(stage_ui, "qml"))
    missing = sorted(n for n in used if n not in known)
    if not missing:
        print(f"[types] новых имён W.* нет "
              f"(вендор зовёт {len(used)}, модуль даёт {len(known)})")
        return []
    _eprint("[types] ⚠️ вендорный QML зовёт типы, которых наш модуль НЕ даёт "
            "— страница молча останется пустой:")
    for name in missing:
        where = ", ".join(used[name][:3])
        _eprint(f"[types]   W.{name}  ← {where}")
    _eprint("[types] лечится регистрацией в rust/rexon-gui/cpp/"
            "rexon_register_we.cpp (и типом в rexon-gui).")
    return missing


def _report_new_qextra_types(stage_ui: str) -> list[str]:
    """Напечатать типы `QE.*`, которых нет у нашей QML-подмены `QExtra`.

    Сверяется именно подмена: она лежит в дереве и работает всякий раз,
    когда настоящего плагина нет. Что даёт сам плагин, сверяет шаг
    «QML-модули» `rexon-vendor-update` — по собранному `module.qmltypes`.
    """
    try:
        known = _qmldir_types(_read(QEXTRA_SHIM_QMLDIR))
    except OSError:
        known = set()
    used = _used_types(os.path.join(stage_ui, "qml"), _USE_QE_RE)
    missing = sorted(n for n in used if n not in known)
    if not missing:
        print(f"[types] новых имён QE.* нет "
              f"(вендор зовёт {len(used)}, подмена даёт {len(known)})")
        return []
    _eprint("[types] ⚠️ вендорный QML зовёт типы QExtra, которых нет у нашей "
            "QML-подмены — без настоящего плагина вкладка останется пустой:")
    for name in missing:
        where = ", ".join(used[name][:3])
        _eprint(f"[types]   QE.{name}  ← {where}")
    _eprint("[types] лечится типом в data/qt_wallpaper_engine/qml_ui/QExtra/ "
            "(и строкой в его qmldir).")
    return ["QE." + n for n in missing]


# ---------------------------------------------------------------------------
# Сборка кандидата и сверка с деревом
# ---------------------------------------------------------------------------

def _stage(upstream: str, ref: str, workdir: str) -> tuple[str, list[str]]:
    """Собрать кандидат вендора: апстрим + qmldir + патчи + UPSTREAM_TAG."""
    root = os.path.join(workdir, "stage")
    os.makedirs(root, exist_ok=True)
    shutil.copytree(os.path.join(upstream, "ui"), os.path.join(root, "ui"))

    src_lua = os.path.join(root, BUILD_LUA)
    lua_text = _read(src_lua)
    os.remove(src_lua)                       # сам build.lua мы не вендорим
    # Замок лежит в КОРНЕ апстрима, а не в ui/ — читаем оттуда и не
    # вендорим: из него нужна одна строка, ревизия QmlMaterial.
    lock_text = _read(os.path.join(upstream, LOCK_FILE))

    stage_ui = os.path.join(root, "ui")
    qmldir = _qmldir_text(lua_text, ref)
    with open(os.path.join(stage_ui, "qmldir"), "w", encoding="utf-8") as fh:
        fh.write(qmldir)
    print(f"[qmldir] сгенерирован из ui/build.lua: "
          f"{len(qmldir.splitlines())} строк, рукописных 0")

    _apply_patches(root)

    with open(os.path.join(stage_ui, "UPSTREAM_TAG"), "w",
              encoding="utf-8") as fh:
        fh.write(ref + "\n")

    pin = _material_pin(lock_text)
    with open(os.path.join(stage_ui, MATERIAL_PIN_NAME), "w",
              encoding="utf-8") as fh:
        fh.write(pin + "\n")
    print(f"[material] ревизия QmlMaterial этого выпуска: {pin[:7]} "
          f"(её берёт tools/build_qml_material.sh)")

    with open(os.path.join(root, QEXTRA_STAGE_NAME), "w",
              encoding="utf-8") as fh:
        fh.write(_qextra_lock(lock_text))
    print(f"[qextra] ревизия QExtra этого выпуска: "
          f"{_qextra_pin(lock_text)[:7]} (её берёт tools/build_qml_qextra.sh)")

    missing = _carry_qmltypes(stage_ui, ref)
    missing += _report_new_types(stage_ui, qmldir)
    missing += _report_new_qextra_types(stage_ui)
    return stage_ui, missing


def _carry_qmltypes(stage_ui: str, ref: str) -> list[str]:
    """Перенести `module.qmltypes` из дерева, если он того же тега.

    Описи в исходниках апстрима нет, поэтому кандидат берёт её из живого
    дерева — но только для того же тега: опись прежнего выпуска рядом с
    новым QML врала бы сверке типов. Для нового тега её нет, и это
    называется вслух (код 4), а дерево остаётся без неё — Rust-сверка
    `waywallen_qmltypes_contract` тогда краснеет.
    """
    live = os.path.join(UI_DIR, QMLTYPES_NAME)
    tag = ref.split()[0] if ref else ""
    if os.path.isfile(live) and tag and tag == _current_ref():
        shutil.copy2(live, os.path.join(stage_ui, QMLTYPES_NAME))
        print(f"[qmltypes] опись C++-типов тега {tag} — из дерева")
        return []
    _eprint(f"[qmltypes] НЕТ описи C++-типов для {tag or ref}: собрать "
            f"waywallen-ui этого тега (lito build -p waywallen-ui) и положить "
            f"build/<профиль>/generated/waywallen-ui/lito-qml/waywallen/ui/"
            f"{QMLTYPES_NAME} в {os.path.relpath(UI_DIR, _REPO)}/")
    return [QMLTYPES_NAME]


def _walk_rel(root: str) -> set[str]:
    out: set[str] = set()
    for base, _dirs, files in os.walk(root):
        for f in files:
            out.add(os.path.relpath(os.path.join(base, f), root))
    return out


def _diff_trees(stage_ui: str, live_ui: str) -> tuple[list[str], list[str],
                                                      list[str]]:
    """(различаются, только в кандидате, только в дереве) — по содержимому."""
    a, b = _walk_rel(stage_ui), _walk_rel(live_ui)
    differ = [r for r in sorted(a & b)
              if not filecmp.cmp(os.path.join(stage_ui, r),
                                 os.path.join(live_ui, r), shallow=False)]
    return differ, sorted(a - b), sorted(b - a)


def _check(stage_ui: str) -> int:
    if not os.path.isdir(UI_DIR):
        _eprint(f"[check] вендора в дереве нет: {UI_DIR}")
        return 3
    differ, only_new, only_old = _diff_trees(stage_ui, UI_DIR)
    if not (differ or only_new or only_old):
        print("[check] OK: дерево совпадает с «апстрим + наши патчи» "
              "побайтово")
        return 0
    for r in differ:
        _eprint(f"[check] РАСХОЖДЕНИЕ: {r}")
    for r in only_new:
        _eprint(f"[check] НЕТ в дереве: {r}")
    for r in only_old:
        _eprint(f"[check] НЕТ у апстрима (лишний файл в дереве): {r}")
    return 3


def _install(stage_ui: str) -> int:
    """Перенести готовый кандидат в дерево. При ошибке вернуть как было."""
    differ, only_new, only_old = ([], [], [])
    if os.path.isdir(UI_DIR):
        differ, only_new, only_old = _diff_trees(stage_ui, UI_DIR)
        if not (differ or only_new or only_old):
            print("[sync] без изменений: дерево уже совпадает")
            return 0
    backup = UI_DIR + ".bak"
    if os.path.exists(backup):
        shutil.rmtree(backup)
    try:
        if os.path.isdir(UI_DIR):
            os.rename(UI_DIR, backup)
        shutil.copytree(stage_ui, UI_DIR)
    except Exception as exc:
        _eprint(f"[sync] ОТКАТ из-за ошибки: {exc}")
        if os.path.isdir(backup):
            if os.path.isdir(UI_DIR):
                shutil.rmtree(UI_DIR)
            os.rename(backup, UI_DIR)
        return 1
    shutil.rmtree(backup, ignore_errors=True)
    for r in differ:
        print(f"[sync] обновлён: {r}")
    for r in only_new:
        print(f"[sync] добавлен: {r}")
    for r in only_old:
        print(f"[sync] удалён:   {r}")
    print("[sync] ГОТОВО — вендорная разметка синхронизирована.")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Синхронизация вендорного QML waywallen → Rexon "
                    "(ui/qml + qmldir из build.lua + наша патч-серия).")
    ap.add_argument("--source", default=DEFAULT_SOURCE,
                    help=f"корень локального клона waywallen "
                         f"(по умолчанию {DEFAULT_SOURCE})")
    ap.add_argument("--git", default="",
                    help="git-URL апстрима (клонируется во временную папку; "
                         "имеет приоритет над --source)")
    ap.add_argument("--ref", default="",
                    help="тег/ветка апстрима (по умолчанию — тег из "
                         "ui/UPSTREAM_TAG)")
    ap.add_argument("--check", action="store_true",
                    help="только сравнить (ничего не писать); "
                         "exit 0=совпадает, 3=расхождение")
    args = ap.parse_args(argv)

    upstream, ref, tmp = _upstream_tree(args)
    try:
        with tempfile.TemporaryDirectory(prefix="waywallen-stage-") as work:
            try:
                stage_ui, missing = _stage(upstream, ref, work)
            except RuntimeError as exc:
                _eprint(f"[sync] {exc}")
                return 1
            stage_lock = os.path.join(os.path.dirname(stage_ui),
                                      QEXTRA_STAGE_NAME)
            if args.check:
                # Обе сверки до конца: расхождение разметки не должно
                # заслонить расхождение среза.
                rc = _check(stage_ui)
                lock_rc = _check_qextra_lock(stage_lock)
                rc = rc or lock_rc
            else:
                rc = _install(stage_ui)
                if rc == 0:
                    _install_qextra_lock(stage_lock)
            # Незарегистрированный тип — не повод рушить синхронизацию
            # (страница у апстрима уже новая, наш крейт догоняет), но
            # молчать о нём нельзя: код возврата 4 виден и в CI.
            if rc == 0 and missing:
                return 4
            return rc
    finally:
        tmp.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
