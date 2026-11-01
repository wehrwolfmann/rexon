#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Синхронизация вендорного крейта fxtranslate → Rexon (одна команда).

Назначение
==========
Автоперевод описаний оформления делает движок **fxtranslate** (MPL-2.0,
реализация движка переводов Firefox на Rust) — каталог
`rust/vendor/fxtranslate/crate` — это опубликованный на crates.io крейт
версии из `crate/UPSTREAM_CRATE`, распакованный ПОБАЙТОВО из `.crate`
с прибитой контрольной суммой, плюс наша серия `patches/`.

Вендорить его в дерево, а не тянуть зависимостью с crates.io, приходится
потому, что мы его ПРАВИМ, и правки нельзя терять: без выбора набора
инструкций по CPUID продукт падает с SIGILL на любом x86-64 без AVX2 —
причём уже после загрузки модели, на первом переводе (разбор и замеры —
в шапке `patches/0001-cpuid-multiarch-and-first-token-fix.patch`).

Этот скрипт за один запуск:

  1. находит архив `fxtranslate-<версия>.crate` (кеш cargo, `--crate-file`
     или `--download` с static.crates.io) и сверяет его sha256 с прибитой
     в этом файле суммой — не сошлась, синхронизация не начинается;
  2. распаковывает архив во временный каталог и снимает с него реестровую
     шелуху: `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.lock` (замок у
     нашей мастерской свой) и `Cargo.toml`, который cargo сгенерировал при
     публикации, — вместо него кладётся авторский `Cargo.toml.orig`
     (тот же состав, но с объяснениями фич, которые нам и читать);
  3. накладывает нашу серию `patches/NNNN-*.patch` (`git apply -p1`);
     патч, чью правку апстрим уже принял сам, распознаётся по тому, что
     ложится задом наперёд, — он пропускается и называется лишним, а не
     обрывает синхронизацию;
  4. записывает `crate/UPSTREAM_CRATE` — версия и sha256 архива;
  5. переносит готовое дерево в `rust/vendor/fxtranslate/crate`.

Собирается всё в отдельном каталоге и переносится в дерево ТОЛЬКО когда
прошли все шаги. Не наложился патч — дерево не тронуто, код возврата != 0.

Использование
=============
    # из кеша cargo (обычный случай):
    scripts/sync-fxtranslate.py

    # из своего архива / скачав его:
    scripts/sync-fxtranslate.py --crate-file ~/fxtranslate-0.4.2.crate
    scripts/sync-fxtranslate.py --download

    # только проверить расхождение, ничего не писать (годится в CI):
    scripts/sync-fxtranslate.py --check

Коды возврата: 0 — синхронизировано (или при `--check` дерево совпадает с
«крейт + наши патчи» побайтово), 1 — патч не наложился, 2 — нет архива или
сумма не сошлась, 3 — при `--check` есть расхождение.

Бумп версии крейта
==================
Поменять `CRATE_VERSION` и `CRATE_SHA256` в этом файле (сумму брать из
`https://index.crates.io/fx/tr/fxtranslate`, а не «на слово») и запустить
без аргументов. Не лёг патч — апстрим тронул те же строки: перенести
правку руками и пересобрать патч, ничего не размазывая по вендорному
дереву (условие MPL-2.0, см. шапку серии).
"""
from __future__ import annotations

import argparse
import filecmp
import glob
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile

# ---------------------------------------------------------------------------
# Что вендорим (прибито гвоздями)
# ---------------------------------------------------------------------------

CRATE_NAME = "fxtranslate"
CRATE_VERSION = "0.4.2"
#: sha256 архива `fxtranslate-0.4.2.crate` с crates.io (сверено с индексом
#: `https://index.crates.io/fx/tr/fxtranslate` 20.09.2026). Гвоздь, а не
#: удобство: вендор обязан быть ровно тем, что опубликовано, иначе
#: публикация нашего патча по MPL-2.0 ни к чему не относится.
CRATE_SHA256 = "e085babeb3347fe8296e1c4a30b0c5226a284428850746d8e281139909502eb2"

_HERE = os.path.dirname(os.path.abspath(__file__))
_REPO = os.path.dirname(_HERE)

#: Корень вендора: тут лежат crate/, patches/ и ATTRIBUTION.md.
VENDOR_DIR = os.path.join(_REPO, "rust", "vendor", "fxtranslate")
#: Само вендорное дерево — его скрипт целиком пересобирает.
CRATE_DIR = os.path.join(VENDOR_DIR, "crate")
#: Наша серия поверх вендора (накладывается по возрастанию имени).
PATCH_DIR = os.path.join(VENDOR_DIR, "patches")

#: Реестровая шелуха: в дереве ей делать нечего.
#:  * `.cargo-ok` / `.cargo_vcs_info.json` — метки распаковки cargo;
#:  * `Cargo.lock` — замок автора крейта; у нашей мастерской свой
#:    (`rust/Cargo.lock`), и второй только путал бы;
#:  * `Cargo.toml` — тот, что cargo сгенерировал при публикации; его
#:    заменяет авторский `Cargo.toml.orig` (см. ниже).
DROP = (".cargo-ok", ".cargo_vcs_info.json", "Cargo.lock")

#: Файл с версией и суммой — мера от «вендор обновили, а чем — не помним».
PIN_NAME = "UPSTREAM_CRATE"

CRATE_URL = (f"https://static.crates.io/crates/{CRATE_NAME}/"
             f"{CRATE_NAME}-{CRATE_VERSION}.crate")


def _eprint(*a) -> None:
    print(*a, file=sys.stderr)


# ---------------------------------------------------------------------------
# Архив крейта
# ---------------------------------------------------------------------------

def _sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _find_crate(args, tmp: str) -> str:
    """Путь к `.crate`: аргумент → кеш cargo → скачивание. С проверкой суммы."""
    candidates: list[str] = []
    if args.crate_file:
        candidates.append(os.path.expanduser(args.crate_file))
    else:
        candidates += sorted(glob.glob(os.path.expanduser(
            f"~/.cargo/registry/cache/index.crates.io-*/"
            f"{CRATE_NAME}-{CRATE_VERSION}.crate")))
    path = next((p for p in candidates if os.path.isfile(p)), "")

    if not path and args.download:
        path = os.path.join(tmp, f"{CRATE_NAME}-{CRATE_VERSION}.crate")
        print(f"[crate] качаю {CRATE_URL}…")
        import urllib.request
        try:
            urllib.request.urlretrieve(CRATE_URL, path)
        except Exception as exc:  # сеть, прокси, что угодно
            _eprint(f"[err] скачать не удалось: {exc}")
            raise SystemExit(2)

    if not path:
        _eprint(f"[err] архива {CRATE_NAME}-{CRATE_VERSION}.crate нет ни в "
                f"кеше cargo, ни в --crate-file.\n"
                f"      Положить его туда (`cargo fetch` в любом крейте с "
                f"этой зависимостью) либо запустить с --download.")
        raise SystemExit(2)

    got = _sha256(path)
    if got != CRATE_SHA256:
        _eprint(f"[err] sha256 НЕ СОШЛАСЬ для {path}\n"
                f"      ожидалась {CRATE_SHA256}\n"
                f"      получена  {got}\n"
                f"      Это либо не тот файл, либо не та версия — "
                f"вендорить его нельзя.")
        raise SystemExit(2)
    print(f"[crate] {path}\n[crate] sha256 сошлась: {got}")
    return path


def _extract(crate: str, dest: str) -> str:
    """Распаковать архив в `dest`, вернуть каталог крейта внутри."""
    with tarfile.open(crate, "r:gz") as tf:
        top = {m.name.split("/", 1)[0] for m in tf.getmembers()}
        if top != {f"{CRATE_NAME}-{CRATE_VERSION}"}:
            _eprint(f"[err] в архиве не один каталог "
                    f"{CRATE_NAME}-{CRATE_VERSION}, а {sorted(top)}")
            raise SystemExit(2)
        # filter="data" — python 3.12+; на более старых игнорируется.
        try:
            tf.extractall(dest, filter="data")
        except TypeError:
            tf.extractall(dest)
    return os.path.join(dest, f"{CRATE_NAME}-{CRATE_VERSION}")


def _normalize(root: str) -> None:
    """Снять реестровую шелуху и поставить авторский манифест."""
    orig = os.path.join(root, "Cargo.toml.orig")
    if not os.path.isfile(orig):
        _eprint("[err] в архиве нет Cargo.toml.orig — нечем заменить "
                "сгенерированный манифест")
        raise SystemExit(2)
    for name in DROP:
        path = os.path.join(root, name)
        if os.path.exists(path):
            os.remove(path)
    os.replace(orig, os.path.join(root, "Cargo.toml"))


# ---------------------------------------------------------------------------
# Наша серия
# ---------------------------------------------------------------------------

def _git(args: list[str], cwd: str | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True,
                          text=True)


def _patches() -> list[str]:
    if not os.path.isdir(PATCH_DIR):
        return []
    return sorted(f for f in os.listdir(PATCH_DIR) if f.endswith(".patch"))


def _apply_patches(root: str) -> list[str]:
    """Наложить серию на распакованный крейт. Не лёг патч — RuntimeError.

    Не наложившийся патч — ещё не конфликт: правку, которую апстрим принял
    у себя, видно по тому, что она ложится ЗАДОМ НАПЕРЁД (то есть уже в
    дереве). Такой патч пропускается и называется лишним — иначе наша же
    принятая наверху починка заперла бы бумп крейта целиком.
    """
    series = _patches()
    if not series:
        print("[patch] серия пуста — накладывать нечего")
        return []
    obsolete: list[str] = []
    for name in series:
        path = os.path.join(PATCH_DIR, name)
        res = _git(["apply", "-p1", "--whitespace=nowarn", path], cwd=root)
        if res.returncode == 0:
            print(f"[patch] наложен: {name}")
            continue
        back = _git(["apply", "-p1", "--reverse", "--check",
                     "--whitespace=nowarn", path], cwd=root)
        if back.returncode == 0:
            obsolete.append(name)
            print(f"[patch] УЖЕ У АПСТРИМА, пропущен: {name}")
            continue
        raise RuntimeError(
            f"патч {name} НЕ НАЛОЖИЛСЯ на крейт {CRATE_VERSION}:\n"
            + (res.stderr or res.stdout).strip()
            + "\n\nЭто настоящий конфликт: апстрим тронул те же строки "
              "(его правки в этом месте у нас НЕТ — иначе патч лёг бы "
              "задом наперёд и был бы пропущен).\nПеренесите правку руками "
              "и пересоберите патч (`diff -ru` эталона и правленого дерева "
            + f"→ {os.path.relpath(path, _REPO)}).")
    if obsolete:
        print("[patch] ⚠️ апстрим вобрал наши правки — эти файлы серии "
              "больше не нужны, удалите их вместе с бумпом вендора:")
        for name in obsolete:
            print(f"[patch]   patches/{name}")
    return obsolete


# ---------------------------------------------------------------------------
# Сборка кандидата, сверка, перенос
# ---------------------------------------------------------------------------

def _stage(args, workdir: str) -> str:
    crate = _find_crate(args, workdir)
    root = _extract(crate, os.path.join(workdir, "распаковка"))
    _normalize(root)
    _apply_patches(root)
    with open(os.path.join(root, PIN_NAME), "w", encoding="utf-8") as fh:
        fh.write(f"{CRATE_NAME} {CRATE_VERSION}\n"
                 f"sha256 {CRATE_SHA256}\n"
                 f"источник https://crates.io/crates/{CRATE_NAME}\n"
                 f"лицензия MPL-2.0 (текст — LICENSE рядом)\n"
                 f"наши правки — ../patches/, пересборка вендора — "
                 f"scripts/sync-fxtranslate.py\n")
    return root


def _walk_rel(root: str) -> set[str]:
    out: set[str] = set()
    for base, _dirs, files in os.walk(root):
        for f in files:
            out.add(os.path.relpath(os.path.join(base, f), root))
    return out


def _diff_trees(stage: str, live: str) -> tuple[list[str], list[str], list[str]]:
    """(различаются, только в кандидате, только в дереве) — по содержимому."""
    a, b = _walk_rel(stage), _walk_rel(live)
    differ = [r for r in sorted(a & b)
              if not filecmp.cmp(os.path.join(stage, r),
                                 os.path.join(live, r), shallow=False)]
    return differ, sorted(a - b), sorted(b - a)


def _check(stage: str) -> int:
    if not os.path.isdir(CRATE_DIR):
        _eprint(f"[check] вендора в дереве нет: {CRATE_DIR}")
        return 3
    differ, only_new, only_old = _diff_trees(stage, CRATE_DIR)
    if not (differ or only_new or only_old):
        print("[check] OK: дерево совпадает с «крейт + наши патчи» побайтово")
        return 0
    for r in differ:
        _eprint(f"[check] РАСХОЖДЕНИЕ: {r}")
    for r in only_new:
        _eprint(f"[check] НЕТ в дереве: {r}")
    for r in only_old:
        _eprint(f"[check] НЕТ у апстрима (лишний файл в дереве): {r}")
    return 3


def _install(stage: str) -> int:
    """Перенести готовый кандидат в дерево. При ошибке вернуть как было."""
    differ, only_new, only_old = ([], [], [])
    if os.path.isdir(CRATE_DIR):
        differ, only_new, only_old = _diff_trees(stage, CRATE_DIR)
        if not (differ or only_new or only_old):
            print("[sync] без изменений: дерево уже совпадает")
            return 0
    os.makedirs(os.path.dirname(CRATE_DIR), exist_ok=True)
    backup = CRATE_DIR + ".bak"
    if os.path.exists(backup):
        shutil.rmtree(backup)
    try:
        if os.path.isdir(CRATE_DIR):
            os.rename(CRATE_DIR, backup)
        shutil.copytree(stage, CRATE_DIR)
    except Exception as exc:
        _eprint(f"[sync] ОТКАТ из-за ошибки: {exc}")
        if os.path.isdir(backup):
            if os.path.isdir(CRATE_DIR):
                shutil.rmtree(CRATE_DIR)
            os.rename(backup, CRATE_DIR)
        return 1
    shutil.rmtree(backup, ignore_errors=True)
    for r in differ:
        print(f"[sync] обновлён: {r}")
    for r in only_new:
        print(f"[sync] добавлен: {r}")
    for r in only_old:
        print(f"[sync] удалён:   {r}")
    print(f"[sync] ГОТОВО — вендор {CRATE_NAME} {CRATE_VERSION} собран "
          f"из архива и нашей серии.")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=f"Синхронизация вендорного крейта {CRATE_NAME} "
                    f"{CRATE_VERSION} (crates.io + наша серия патчей).")
    ap.add_argument("--crate-file", default="",
                    help="архив .crate вместо поиска в кеше cargo")
    ap.add_argument("--download", action="store_true",
                    help="скачать архив с static.crates.io, если его нет")
    ap.add_argument("--check", action="store_true",
                    help="только сверить дерево с «крейт + наши патчи» "
                         "(0 — совпало, 3 — нет)")
    args = ap.parse_args(argv)

    with tempfile.TemporaryDirectory(prefix="fxtranslate-vendor-") as workdir:
        try:
            stage = _stage(args, workdir)
        except RuntimeError as exc:
            _eprint(f"[err] {exc}")
            return 1
        if args.check:
            return _check(stage)
        return _install(stage)


if __name__ == "__main__":
    raise SystemExit(main())
