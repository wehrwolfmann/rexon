#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Sync of the waywallen control-plane .proto → Rexon (one command).

Purpose
==========
The «Wallpaper Engine» tab is a Rust/QML frontend to the **waywallen** daemon
(C++/Qt). The control plane between them is protobuf over
WebSocket; the schemas live in `proto/control.proto` + `proto/filter.proto`.

When the waywallen upstream author changes the protocol (a new field/message/wallpaper
property type), our schemas become outdated. This script in one run:

  1. takes the upstream `*.proto` (from a local clone OR the git upstream),
  2. copies them into `data/qt_wallpaper_engine/proto/`, KEEPING our
     license header (hypengw's MIT attribution on top of each file),
  3. writes `proto/UPSTREAM_TAG` — the release the snapshot was taken from.

The structs from the schemas are printed by `rexon-core/build.rs` (prost) at build time.
Python protobuf (`*_pb2.py`) is not generated since 02.10.2026: Python in
the project is only an archive (`_archived/`), nobody reads it.

⚠️ The tag step appeared on 09.09.2026, and before it the release tag was edited BY HAND.
The price of the manual edit showed on accepting v0.3.9: the schema moved to the new tag, while
`proto/UPSTREAM_TAG` stayed at `v0.3.8` — and `parity.daemon_matches_snapshot()`
compared the live daemon with YESTERDAY's snapshot, which nobody would have noticed
(its own docstring claimed all along that the file is written by this
script). The tag is taken from `--tag`, and with a local clone we derive it ourselves.

On a write error the originals are rolled back from
the backup (.bak), exit code != 0. That is, "either synced entirely, or
nothing is broken".

Usage
=============
    # from a local clone (default ~/Загрузки/owe-ref/waywallen):
    scripts/sync-waywallen-proto.py
    scripts/sync-waywallen-proto.py --source /path/to/waywallen

    # directly from the git upstream (clones into a temporary directory):
    scripts/sync-waywallen-proto.py --git https://github.com/hypengw/waywallen
    scripts/sync-waywallen-proto.py --git <url> --ref main

    # only check for divergence, write nothing (for CI/cron):
    scripts/sync-waywallen-proto.py --check

Full automation
====================
`--check` gives exit 0 (in sync) / 3 (divergence) — that is all that is
needed for a CI gate or a cron watcher: periodically run `--git <url>
--check`; on code 3 — open a PR / send a notification. See the report.
"""
from __future__ import annotations

import argparse
import filecmp
import os
import shutil
import subprocess
import sys
import tempfile

# proto files we mirror (name = relative to the proto directory).
PROTO_FILES = ("control.proto", "filter.proto")

# Release tag next to the schemas: "v0.3.9 (c62441e)". It is read by
# `rexon-core::we_parity` (`read_proto_upstream_tag`) to
# tell whether our protocol snapshot matches the LIVE daemon.
TAG_NAME = "UPSTREAM_TAG"

# Our proto directory (the target). Computed from the script location so that
# it works both from the /usr/local/lib/rexon symlink and from the working directory.
_HERE = os.path.dirname(os.path.abspath(__file__))
_REPO = os.path.dirname(_HERE)
DEST_DIR = os.path.join(_REPO, "data", "qt_wallpaper_engine", "proto")

DEFAULT_SOURCE = os.path.expanduser("~/Загрузки/owe-ref/waywallen")


def _eprint(*a) -> None:
    print(*a, file=sys.stderr)


def _our_header(dest_path: str) -> str:
    """Return our license header (everything BEFORE the `syntax =` line).

    That way regeneration does not overwrite hypengw's MIT attribution that we keep
    on top of each file. If the target file does not exist yet — the header is empty.
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
    """Directory with the upstream .proto. For --git clones into a temporary directory.

    Returns (dir, tmp) — tmp is kept alive until we finish (its cleanup
    removes the clone). For a local source tmp=None.
    """
    if args.git:
        tmp = tempfile.TemporaryDirectory(prefix="waywallen-proto-")
        cmd = ["git", "clone", "--depth", "1"]
        if args.ref:
            cmd += ["--branch", args.ref]
        cmd += [args.git, tmp.name]
        _eprint(f"[git] cloning {args.git} (ref={args.ref or 'default'})…")
        res = subprocess.run(cmd, capture_output=True, text=True)
        if res.returncode != 0:
            tmp.cleanup()
            _eprint("[git] cloning failed:\n"
                    + (res.stderr or res.stdout).strip())
            raise SystemExit(2)
        return os.path.join(tmp.name, "proto"), tmp
    src_proto = os.path.join(args.source, "proto")
    return src_proto, None


def _git_out(args: list[str]) -> str:
    """stdout of a git command as one line; empty — it did not work."""
    try:
        res = subprocess.run(["git", *args], capture_output=True, text=True,
                             timeout=60)
    except (OSError, subprocess.SubprocessError):
        return ""
    return res.stdout.strip() if res.returncode == 0 else ""


def _resolve_tag(args, upstream_dir: str) -> str:
    """Release tag for `proto/UPSTREAM_TAG` — "v0.3.9 (c62441e)".

    Three sources by priority: an explicit `--tag` (given by
    `scripts/rexon-vendor-update`, which materializes the tag slice into
    a plain directory and so is the only one that knows the release), then the clone —
    local (`--source`) or temporary (`--git`). Nothing worked —
    an empty string, and the caller will say so OUT LOUD.
    """
    if args.tag:
        return args.tag.strip()
    # For --git the clone lies one level above the proto directory.
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
    """What is written in our `proto/UPSTREAM_TAG` now (empty — no file)."""
    path = os.path.join(DEST_DIR, TAG_NAME)
    try:
        return _read(path).strip()
    except OSError:
        return ""


def _check_only(upstream_dir: str, tag: str = "") -> int:
    """Compare the BODIES (without the header) of upstream and ours. 0=in sync, 3=divergence."""
    drift = False
    for name in PROTO_FILES:
        up = os.path.join(upstream_dir, name)
        ours = os.path.join(DEST_DIR, name)
        if not os.path.isfile(up):
            _eprint(f"[check] MISSING upstream: {up}")
            drift = True
            continue
        up_body = _strip_header(_read(up))
        our_body = _strip_header(_read(ours)) if os.path.isfile(ours) else None
        if up_body != our_body:
            _eprint(f"[check] DIVERGENCE: {name}")
            drift = True
        else:
            print(f"[check] OK: {name}")
    # The tag is compared exactly when we know what to compare with: without `--tag` and
    # without a clone there is nothing to compare, and silence here is more honest than making things up.
    if tag:
        have = _tag_in_tree()
        if have != tag:
            _eprint(f"[check] DIVERGENCE: {TAG_NAME} — in the tree "
                    f"{have or '(no file)'}, upstream {tag}")
            drift = True
        else:
            print(f"[check] OK: {TAG_NAME} ({tag})")
    return 3 if drift else 0


def _read(path: str) -> str:
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def _strip_header(text: str) -> str:
    """Cut everything before `syntax =` — only the protocol body is compared."""
    for i, line in enumerate(text.splitlines(keepends=True)):
        if line.lstrip().startswith("syntax"):
            return "".join(text.splitlines(keepends=True)[i:])
    return text


def _sync(upstream_dir: str, tag: str = "") -> int:
    """Copy the bodies of the upstream .proto under our header and write the tag.

    Done atomically via a backup: on any error the schemas and the tag are rolled back.
    """
    backups: dict[str, str] = {}
    touched: list[str] = []

    def _backup(path: str) -> None:
        if os.path.isfile(path) and path not in backups:
            bak = path + ".bak"
            shutil.copy2(path, bak)
            backups[path] = bak

    try:
        # 1. proto: upstream body + our header
        for name in PROTO_FILES:
            up = os.path.join(upstream_dir, name)
            if not os.path.isfile(up):
                _eprint(f"[sync] MISSING upstream: {up}")
                return 2
            dest = os.path.join(DEST_DIR, name)
            _backup(dest)
            touched.append(dest)
            header = _our_header(dest)
            body = _strip_header(_read(up))
            new = header + body
            if os.path.isfile(dest) and _read(dest) == new:
                print(f"[sync] unchanged: {name}")
            else:
                with open(dest, "w", encoding="utf-8") as fh:
                    fh.write(new)
                print(f"[sync] updated: {name}")

        # 2. release tag. Written IN THE SAME transaction as the schemas: the snapshot
        #    and its number must move and roll back together.
        tag_path = os.path.join(DEST_DIR, TAG_NAME)
        if tag:
            _backup(tag_path)
            touched.append(tag_path)
            new_tag = tag + "\n"
            if os.path.isfile(tag_path) and _read(tag_path) == new_tag:
                print(f"[sync] unchanged: {TAG_NAME}")
            else:
                with open(tag_path, "w", encoding="utf-8") as fh:
                    fh.write(new_tag)
                print(f"[sync] updated: {TAG_NAME} → {tag}")
        else:
            _eprint(f"[sync] ⚠ release not named (no --tag and not a clone) — "
                    f"{TAG_NAME} left as it was: "
                    f"{_tag_in_tree() or '(no file)'}")

        # success — remove the .bak
        for bak in backups.values():
            try:
                os.remove(bak)
            except OSError:
                pass
        print("[sync] DONE — proto synced.")
        return 0
    except Exception as exc:  # rollback
        _eprint(f"[sync] ROLLBACK because of an error: {exc}")
        for path, bak in backups.items():
            try:
                shutil.copy2(bak, path)
                os.remove(bak)
            except OSError as e:
                _eprint(f"[sync] failed to roll back {path}: {e}")
        return 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Sync of waywallen .proto → Rexon.")
    ap.add_argument("--source", default=DEFAULT_SOURCE,
                    help=f"root of the local waywallen clone "
                         f"(default {DEFAULT_SOURCE})")
    ap.add_argument("--git", default="",
                    help="upstream git URL (cloned into a temporary directory; "
                         "takes priority over --source)")
    ap.add_argument("--ref", default="",
                    help="branch/tag for --git (default: the default branch)")
    ap.add_argument("--tag", default="",
                    help="release tag for proto/UPSTREAM_TAG "
                         "(\"v0.3.9 (c62441e)\"); empty — derive from the clone")
    ap.add_argument("--check", action="store_true",
                    help="only compare (write nothing); "
                         "exit 0=in sync, 3=divergence")
    args = ap.parse_args(argv)

    upstream_dir, tmp = _upstream_proto_dir(args)
    try:
        if not os.path.isdir(upstream_dir):
            _eprint(f"[err] no upstream proto directory: {upstream_dir}")
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
