#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Синхронизация .proto контрол-плейна waywallen → Rexon (одна команда).

Назначение
==========
Вкладка «Wallpaper Engine» — фронтенд на Rust/QML к демону **waywallen**
(C++/Qt). Контрол-плейн между ними — protobuf поверх
WebSocket; схемы лежат в `proto/control.proto` + `proto/filter.proto`.

Когда апстрим-автор waywallen меняет протокол (новое поле/сообщение/тип
свойства обои), наши схемы устаревают. Этот скрипт за один запуск:

  1. берёт апстрим `*.proto` (из локального клона ИЛИ git-апстрима),
  2. копирует их в `data/qt_wallpaper_engine/proto/`, СОХРАНЯЯ наш
     лицензионный заголовок (MIT-атрибуция hypengw поверх каждого файла),
  3. записывает `proto/UPSTREAM_TAG` — выпуск, с которого снят снимок.

Структуры из схем печатает `rexon-core/build.rs` (prost) на сборке.
Питоновский протобуф (`*_pb2.py`) с 02.10.2026 не генерируется: Python в
проекте — только архив (`_archived/`), читать его некому.

⚠️ Шаг с меткой появился 09.09.2026, и до него метка выпуска правилась РУКАМИ.
Цена ручной правки видна на приёме v0.3.9: схема уехала на новый тег, а
`proto/UPSTREAM_TAG` остался на `v0.3.8` — и `parity.daemon_matches_snapshot()`
сравнивала живого демона со ВЧЕРАШНИМ снимком, о чём никто бы не узнал
(её собственный докстринг всё это время утверждал, что файл пишет этот
скрипт). Метку берём из `--tag`, а при локальном клоне выводим сами.

При ошибке записи оригиналы откатываются из
бэкапа (.bak), exit-код != 0. То есть «или синхронизировано целиком, или
ничего не сломано».

Использование
=============
    # из локального клона (по умолчанию ~/Загрузки/owe-ref/waywallen):
    scripts/sync-waywallen-proto.py
    scripts/sync-waywallen-proto.py --source /path/to/waywallen

    # напрямую из git-апстрима (клонирует во временную папку):
    scripts/sync-waywallen-proto.py --git https://github.com/hypengw/waywallen
    scripts/sync-waywallen-proto.py --git <url> --ref main

    # только проверить расхождение, ничего не писать (для CI/cron):
    scripts/sync-waywallen-proto.py --check

Полная автоматизация
====================
`--check` даёт exit 0 (синхронны) / 3 (есть расхождение) — это всё, что
нужно для CI-гейта или cron-вотчера: периодически дёргать `--git <url>
--check`; при коде 3 — открыть PR / прислать уведомление. См. отчёт.
"""
from __future__ import annotations

import argparse
import filecmp
import os
import shutil
import subprocess
import sys
import tempfile

# proto-файлы, которые мы зеркалим (имя = относительно proto-каталога).
PROTO_FILES = ("control.proto", "filter.proto")

# Метка выпуска рядом со схемами: «v0.3.9 (c62441e)». Её читает
# `rexon-core::we_parity` (`read_proto_upstream_tag`), чтобы
# сказать, тот ли снимок протокола у нас против ЖИВОГО демона.
TAG_NAME = "UPSTREAM_TAG"

# Каталог наших proto (целевой). Вычисляется от расположения скрипта, чтобы
# работать и из симлинка /usr/local/lib/rexon, и из рабочей папки.
_HERE = os.path.dirname(os.path.abspath(__file__))
_REPO = os.path.dirname(_HERE)
DEST_DIR = os.path.join(_REPO, "data", "qt_wallpaper_engine", "proto")

DEFAULT_SOURCE = os.path.expanduser("~/Загрузки/owe-ref/waywallen")


def _eprint(*a) -> None:
    print(*a, file=sys.stderr)


def _our_header(dest_path: str) -> str:
    """Вернуть наш лицензионный заголовок (всё ДО строки `syntax =`).

    Так регенерация не затирает MIT-атрибуцию hypengw, которую мы держим
    поверх каждого файла. Если целевого файла ещё нет — заголовок пустой.
    """
    if not os.path.isfile(dest_path):
        return ""
    head: list[str] = []
    with open(dest_path, encoding="utf-8") as fh:
        for line in fh:
            if line.lstrip().startswith("syntax"):
                break
            head.append(line)
    return "".join(head)


def _upstream_proto_dir(args) -> tuple[str, tempfile.TemporaryDirectory | None]:
    """Каталог с апстрим .proto. Для --git клонирует во временную папку.

    Возвращает (dir, tmp) — tmp держим живым, пока не закончим (его cleanup
    удалит клон). Для локального источника tmp=None.
    """
    if args.git:
        tmp = tempfile.TemporaryDirectory(prefix="waywallen-proto-")
        cmd = ["git", "clone", "--depth", "1"]
        if args.ref:
            cmd += ["--branch", args.ref]
        cmd += [args.git, tmp.name]
        _eprint(f"[git] клонирую {args.git} (ref={args.ref or 'default'})…")
        res = subprocess.run(cmd, capture_output=True, text=True)
        if res.returncode != 0:
            tmp.cleanup()
            _eprint("[git] клонирование не удалось:\n"
                    + (res.stderr or res.stdout).strip())
            raise SystemExit(2)
        return os.path.join(tmp.name, "proto"), tmp
    src_proto = os.path.join(args.source, "proto")
    return src_proto, None


def _git_out(args: list[str]) -> str:
    """stdout команды git одной строкой; пусто — не получилось."""
    try:
        res = subprocess.run(["git", *args], capture_output=True, text=True,
                             timeout=60)
    except (OSError, subprocess.SubprocessError):
        return ""
    return res.stdout.strip() if res.returncode == 0 else ""


def _resolve_tag(args, upstream_dir: str) -> str:
    """Метка выпуска для `proto/UPSTREAM_TAG` — «v0.3.9 (c62441e)».

    Три источника по старшинству: явный `--tag` (его даёт
    `scripts/rexon-vendor-update`, который материализует срез тега в
    обычный каталог и потому единственный знает выпуск), затем клон —
    локальный (`--source`) или временный (`--git`). Ничего не вышло —
    пустая строка, и вызывающий скажет об этом ВСЛУХ.
    """
    if args.tag:
        return args.tag.strip()
    # Для --git клон лежит на уровень выше каталога proto.
    root = os.path.dirname(os.path.abspath(upstream_dir)) if args.git \
        else args.source
    if not os.path.isdir(os.path.join(root, ".git")):
        return ""
    ref = "HEAD" if args.git else (args.ref or "HEAD")
    sha = _git_out(["-C", root, "rev-parse", "--short", ref])
    if not sha:
        return ""
    name = args.ref or _git_out(
        ["-C", root, "describe", "--tags", "--abbrev=0", ref])
    return f"{name} ({sha})" if name else sha


def _tag_in_tree() -> str:
    """Что записано в нашем `proto/UPSTREAM_TAG` сейчас (пусто — файла нет)."""
    path = os.path.join(DEST_DIR, TAG_NAME)
    try:
        return _read(path).strip()
    except OSError:
        return ""


def _check_only(upstream_dir: str, tag: str = "") -> int:
    """Сравнить ТЕЛА (без заголовка) апстрима и наших. 0=синхронны, 3=расхождение."""
    drift = False
    for name in PROTO_FILES:
        up = os.path.join(upstream_dir, name)
        ours = os.path.join(DEST_DIR, name)
        if not os.path.isfile(up):
            _eprint(f"[check] НЕТ в апстриме: {up}")
            drift = True
            continue
        up_body = _strip_header(_read(up))
        our_body = _strip_header(_read(ours)) if os.path.isfile(ours) else None
        if up_body != our_body:
            _eprint(f"[check] РАСХОЖДЕНИЕ: {name}")
            drift = True
        else:
            print(f"[check] OK: {name}")
    # Метку сверяем ровно тогда, когда знаем, с чем сверять: без `--tag` и
    # без клона сравнивать нечем, и молчание тут честнее выдумки.
    if tag:
        have = _tag_in_tree()
        if have != tag:
            _eprint(f"[check] РАСХОЖДЕНИЕ: {TAG_NAME} — в дереве "
                    f"{have or '(нет файла)'}, у апстрима {tag}")
            drift = True
        else:
            print(f"[check] OK: {TAG_NAME} ({tag})")
    return 3 if drift else 0


def _read(path: str) -> str:
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def _strip_header(text: str) -> str:
    """Отрезать всё до `syntax =` — сравниваем только тело протокола."""
    for i, line in enumerate(text.splitlines(keepends=True)):
        if line.lstrip().startswith("syntax"):
            return "".join(text.splitlines(keepends=True)[i:])
    return text


def _sync(upstream_dir: str, tag: str = "") -> int:
    """Скопировать тела апстрим .proto под наш заголовок и записать метку.

    Делаем атомарно через бэкап: при любой ошибке откатываем схемы и метку.
    """
    backups: dict[str, str] = {}
    touched: list[str] = []

    def _backup(path: str) -> None:
        if os.path.isfile(path) and path not in backups:
            bak = path + ".bak"
            shutil.copy2(path, bak)
            backups[path] = bak

    try:
        # 1. proto: тело апстрима + наш заголовок
        for name in PROTO_FILES:
            up = os.path.join(upstream_dir, name)
            if not os.path.isfile(up):
                _eprint(f"[sync] НЕТ в апстриме: {up}")
                return 2
            dest = os.path.join(DEST_DIR, name)
            _backup(dest)
            touched.append(dest)
            header = _our_header(dest)
            body = _strip_header(_read(up))
            new = header + body
            if os.path.isfile(dest) and _read(dest) == new:
                print(f"[sync] без изменений: {name}")
            else:
                with open(dest, "w", encoding="utf-8") as fh:
                    fh.write(new)
                print(f"[sync] обновлён: {name}")

        # 2. метка выпуска. Пишется В ТОЙ ЖЕ транзакции, что схемы: снимок
        #    и его номер обязаны уезжать и откатываться вместе.
        tag_path = os.path.join(DEST_DIR, TAG_NAME)
        if tag:
            _backup(tag_path)
            touched.append(tag_path)
            new_tag = tag + "\n"
            if os.path.isfile(tag_path) and _read(tag_path) == new_tag:
                print(f"[sync] без изменений: {TAG_NAME}")
            else:
                with open(tag_path, "w", encoding="utf-8") as fh:
                    fh.write(new_tag)
                print(f"[sync] обновлён: {TAG_NAME} → {tag}")
        else:
            _eprint(f"[sync] ⚠ выпуск не назван (нет --tag и не клон) — "
                    f"{TAG_NAME} оставлен как был: "
                    f"{_tag_in_tree() or '(нет файла)'}")

        # успех — убрать .bak
        for bak in backups.values():
            try:
                os.remove(bak)
            except OSError:
                pass
        print("[sync] ГОТОВО — proto синхронизированы.")
        return 0
    except Exception as exc:  # откат
        _eprint(f"[sync] ОТКАТ из-за ошибки: {exc}")
        for path, bak in backups.items():
            try:
                shutil.copy2(bak, path)
                os.remove(bak)
            except OSError as e:
                _eprint(f"[sync] не удалось откатить {path}: {e}")
        return 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Синхронизация waywallen .proto → Rexon.")
    ap.add_argument("--source", default=DEFAULT_SOURCE,
                    help=f"корень локального клона waywallen "
                         f"(по умолчанию {DEFAULT_SOURCE})")
    ap.add_argument("--git", default="",
                    help="git-URL апстрима (клонируется во временную папку; "
                         "имеет приоритет над --source)")
    ap.add_argument("--ref", default="",
                    help="ветка/тег для --git (по умолчанию default branch)")
    ap.add_argument("--tag", default="",
                    help="метка выпуска для proto/UPSTREAM_TAG "
                         "(«v0.3.9 (c62441e)»); пусто — вывести из клона")
    ap.add_argument("--check", action="store_true",
                    help="только сравнить (ничего не писать); "
                         "exit 0=синхронны, 3=расхождение")
    args = ap.parse_args(argv)

    upstream_dir, tmp = _upstream_proto_dir(args)
    try:
        if not os.path.isdir(upstream_dir):
            _eprint(f"[err] нет каталога апстрим-proto: {upstream_dir}")
            return 2
        tag = _resolve_tag(args, upstream_dir)
        if args.check:
            return _check_only(upstream_dir, tag)
        return _sync(upstream_dir, tag)
    finally:
        if tmp is not None:
            tmp.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
