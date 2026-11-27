#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Sync of the vendored fxtranslate crate → Rexon (one command).

Purpose
==========
Automatic translation of appearance descriptions is done by the **fxtranslate** engine (MPL-2.0,
a Rust implementation of the Firefox translation engine) — the directory
`rust/vendor/fxtranslate/crate` is the crate published on crates.io in the
version from `crate/UPSTREAM_CRATE`, unpacked BYTE FOR BYTE from the `.crate`
with a pinned checksum, plus our `patches/` series.

It has to be vendored into the tree rather than pulled as a dependency from crates.io
because we PATCH it, and the patches must not be lost: without choosing the instruction
set by CPUID the product crashes with SIGILL on any x86-64 without AVX2 —
and only after the model is loaded, on the first translation (analysis and measurements —
in the header of `patches/0001-cpuid-multiarch-and-first-token-fix.patch`).

This script in one run:

  1. finds the `fxtranslate-<version>.crate` archive (cargo cache, `--crate-file`
     or `--download` from static.crates.io) and checks its sha256 against the sum pinned
     in this file — if it does not match, the sync does not start;
  2. unpacks the archive into a temporary directory and strips the registry
     husk from it: `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.lock` (our
     workspace has its own lock) and the `Cargo.toml` that cargo generated on
     publishing — instead the author's `Cargo.toml.orig` is put in
     (the same contents, but with explanations of the features, which is what we read);
  3. applies our `patches/NNNN-*.patch` series (`git apply -p1`);
     a patch whose change upstream has already accepted itself is recognized by the fact
     that it applies in reverse — it is skipped and called redundant rather than
     aborting the sync;
  4. writes `crate/UPSTREAM_CRATE` — the version and sha256 of the archive;
  5. moves the finished tree into `rust/vendor/fxtranslate/crate`.

Everything is assembled in a separate directory and moved into the tree ONLY when
all steps have passed. A patch did not apply — the tree is untouched, exit code != 0.

Usage
=============
    # from the cargo cache (the usual case):
    scripts/sync-fxtranslate.py

    # from your own archive / by downloading it:
    scripts/sync-fxtranslate.py --crate-file ~/fxtranslate-0.4.2.crate
    scripts/sync-fxtranslate.py --download

    # only check for divergence, write nothing (suitable for CI):
    scripts/sync-fxtranslate.py --check

Exit codes: 0 — synced (or with `--check` the tree matches
"crate + our patches" byte for byte), 1 — a patch did not apply, 2 — no archive or
the sum did not match, 3 — with `--check` there is a divergence.

Bumping the crate version
==================
Change `CRATE_VERSION` and `CRATE_SHA256` in this file (take the sum from
`https://index.crates.io/fx/tr/fxtranslate`, not "on faith") and run
without arguments. A patch did not apply — upstream touched the same lines: port the
change by hand and rebuild the patch, without smearing anything over the vendored
tree (an MPL-2.0 condition, see the series header).
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
# What we vendor (nailed down)
# ---------------------------------------------------------------------------

CRATE_NAME = "fxtranslate"
CRATE_VERSION = "0.4.2"
#: sha256 of the `fxtranslate-0.4.2.crate` archive from crates.io (checked against the index
#: `https://index.crates.io/fx/tr/fxtranslate` on 20.09.2026). A nail, not a
#: convenience: the vendor must be exactly what is published, otherwise
#: publishing our patch under MPL-2.0 does not relate to anything.
CRATE_SHA256 = "e085babeb3347fe8296e1c4a30b0c5226a284428850746d8e281139909502eb2"

_HERE = os.path.dirname(os.path.abspath(__file__))
_REPO = os.path.dirname(_HERE)

#: Vendor root: crate/, patches/ and ATTRIBUTION.md live here.
VENDOR_DIR = os.path.join(_REPO, "rust", "vendor", "fxtranslate")
#: The vendored tree itself — the script rebuilds it entirely.
CRATE_DIR = os.path.join(VENDOR_DIR, "crate")
#: Our series on top of the vendor (applied in ascending name order).
PATCH_DIR = os.path.join(VENDOR_DIR, "patches")

#: Registry husk: it has nothing to do in the tree.
#:  * `.cargo-ok` / `.cargo_vcs_info.json` — cargo unpacking marks;
#:  * `Cargo.lock` — the crate author's lock; our workspace has its own
#:    (`rust/Cargo.lock`), and a second one would only confuse;
#:  * `Cargo.toml` — the one cargo generated on publishing; it is
#:    replaced by the author's `Cargo.toml.orig` (see below).
DROP = (".cargo-ok", ".cargo_vcs_info.json", "Cargo.lock")

#: The file with the version and the sum — a measure against "the vendor was updated, but with what — nobody remembers".
PIN_NAME = "UPSTREAM_CRATE"

CRATE_URL = (f"https://static.crates.io/crates/{CRATE_NAME}/"
             f"{CRATE_NAME}-{CRATE_VERSION}.crate")


def _eprint(*a) -> None:
    print(*a, file=sys.stderr)


# ---------------------------------------------------------------------------
# Crate archive
# ---------------------------------------------------------------------------

def _sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _find_crate(args, tmp: str) -> str:
    """Path to the `.crate`: argument → cargo cache → download. With a sum check."""
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
        print(f"[crate] downloading {CRATE_URL}…")
        import urllib.request
        try:
            urllib.request.urlretrieve(CRATE_URL, path)
        except Exception as exc:  # network, proxy, anything
            _eprint(f"[err] download failed: {exc}")
            raise SystemExit(2)

    if not path:
        _eprint(f"[err] the archive {CRATE_NAME}-{CRATE_VERSION}.crate is neither in "
                f"the cargo cache nor in --crate-file.\n"
                f"      Put it there (`cargo fetch` in any crate with "
                f"this dependency) or run with --download.")
        raise SystemExit(2)

    got = _sha256(path)
    if got != CRATE_SHA256:
        _eprint(f"[err] sha256 does NOT MATCH for {path}\n"
                f"      expected {CRATE_SHA256}\n"
                f"      got      {got}\n"
                f"      This is either the wrong file or the wrong version — "
                f"it must not be vendored.")
        raise SystemExit(2)
    print(f"[crate] {path}\n[crate] sha256 matches: {got}")
    return path


def _extract(crate: str, dest: str) -> str:
    """Unpack the archive into `dest`, return the crate directory inside."""
    with tarfile.open(crate, "r:gz") as tf:
        top = {m.name.split("/", 1)[0] for m in tf.getmembers()}
        if top != {f"{CRATE_NAME}-{CRATE_VERSION}"}:
            _eprint(f"[err] the archive has not a single directory "
                    f"{CRATE_NAME}-{CRATE_VERSION}, but {sorted(top)}")
            raise SystemExit(2)
        # filter="data" — python 3.12+; ignored on older ones.
        try:
            tf.extractall(dest, filter="data")
        except TypeError:
            tf.extractall(dest)
    return os.path.join(dest, f"{CRATE_NAME}-{CRATE_VERSION}")


def _normalize(root: str) -> None:
    """Strip the registry husk and put the author's manifest in place."""
    orig = os.path.join(root, "Cargo.toml.orig")
    if not os.path.isfile(orig):
        _eprint("[err] the archive has no Cargo.toml.orig — nothing to replace "
                "the generated manifest with")
        raise SystemExit(2)
    for name in DROP:
        path = os.path.join(root, name)
        if os.path.exists(path):
            os.remove(path)
    os.replace(orig, os.path.join(root, "Cargo.toml"))


# ---------------------------------------------------------------------------
# Our series
# ---------------------------------------------------------------------------

def _git(args: list[str], cwd: str | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True,
                          text=True)


def _patches() -> list[str]:
    if not os.path.isdir(PATCH_DIR):
        return []
    return sorted(f for f in os.listdir(PATCH_DIR) if f.endswith(".patch"))


def _apply_patches(root: str) -> list[str]:
    """Apply the series to the unpacked crate. A patch did not apply — RuntimeError.

    A patch that did not apply is not a conflict yet: a change that upstream accepted
    on its side shows by applying IN REVERSE (i.e. it is already in
    the tree). Such a patch is skipped and called redundant — otherwise our own
    fix accepted upstream would block the whole crate bump.
    """
    series = _patches()
    if not series:
        print("[patch] the series is empty — nothing to apply")
        return []
    obsolete: list[str] = []
    for name in series:
        path = os.path.join(PATCH_DIR, name)
        res = _git(["apply", "-p1", "--whitespace=nowarn", path], cwd=root)
        if res.returncode == 0:
            print(f"[patch] applied: {name}")
            continue
        back = _git(["apply", "-p1", "--reverse", "--check",
                     "--whitespace=nowarn", path], cwd=root)
        if back.returncode == 0:
            obsolete.append(name)
            print(f"[patch] ALREADY UPSTREAM, skipped: {name}")
            continue
        raise RuntimeError(
            f"patch {name} did NOT APPLY to crate {CRATE_VERSION}:\n"
            + (res.stderr or res.stdout).strip()
            + "\n\nThis is a real conflict: upstream touched the same lines "
              "(we do NOT have its change in this place — otherwise the patch would apply "
              "in reverse and be skipped).\nPort the change by hand "
              "and rebuild the patch (`diff -ru` of the reference and the edited tree "
            + f"→ {os.path.relpath(path, _REPO)}).")
    if obsolete:
        print("[patch] ⚠️ upstream absorbed our changes — these series files "
              "are no longer needed, remove them together with the vendor bump:")
        for name in obsolete:
            print(f"[patch]   patches/{name}")
    return obsolete


# ---------------------------------------------------------------------------
# Building the candidate, comparison, moving
# ---------------------------------------------------------------------------

def _stage(args, workdir: str) -> str:
    crate = _find_crate(args, workdir)
    root = _extract(crate, os.path.join(workdir, "unpacked"))
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
    """(differ, only in the candidate, only in the tree) — by content."""
    a, b = _walk_rel(stage), _walk_rel(live)
    differ = [r for r in sorted(a & b)
              if not filecmp.cmp(os.path.join(stage, r),
                                 os.path.join(live, r), shallow=False)]
    return differ, sorted(a - b), sorted(b - a)


def _check(stage: str) -> int:
    if not os.path.isdir(CRATE_DIR):
        _eprint(f"[check] the vendor is not in the tree: {CRATE_DIR}")
        return 3
    differ, only_new, only_old = _diff_trees(stage, CRATE_DIR)
    if not (differ or only_new or only_old):
        print("[check] OK: the tree matches \"crate + our patches\" byte for byte")
        return 0
    for r in differ:
        _eprint(f"[check] DIVERGENCE: {r}")
    for r in only_new:
        _eprint(f"[check] MISSING in the tree: {r}")
    for r in only_old:
        _eprint(f"[check] MISSING upstream (extra file in the tree): {r}")
    return 3


def _install(stage: str) -> int:
    """Move the finished candidate into the tree. On error put things back as they were."""
    differ, only_new, only_old = ([], [], [])
    if os.path.isdir(CRATE_DIR):
        differ, only_new, only_old = _diff_trees(stage, CRATE_DIR)
        if not (differ or only_new or only_old):
            print("[sync] unchanged: the tree already matches")
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
        _eprint(f"[sync] ROLLBACK because of an error: {exc}")
        if os.path.isdir(backup):
            if os.path.isdir(CRATE_DIR):
                shutil.rmtree(CRATE_DIR)
            os.rename(backup, CRATE_DIR)
        return 1
    shutil.rmtree(backup, ignore_errors=True)
    for r in differ:
        print(f"[sync] updated: {r}")
    for r in only_new:
        print(f"[sync] added: {r}")
    for r in only_old:
        print(f"[sync] removed: {r}")
    print(f"[sync] DONE — vendor {CRATE_NAME} {CRATE_VERSION} built "
          f"from the archive and our series.")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=f"Sync of the vendored crate {CRATE_NAME} "
                    f"{CRATE_VERSION} (crates.io + our patch series).")
    ap.add_argument("--crate-file", default="",
                    help="a .crate archive instead of searching the cargo cache")
    ap.add_argument("--download", action="store_true",
                    help="download the archive from static.crates.io if it is missing")
    ap.add_argument("--check", action="store_true",
                    help="only compare the tree with \"crate + our patches\" "
                         "(0 — matched, 3 — not)")
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
