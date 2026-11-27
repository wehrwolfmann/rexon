#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Sync of the vendored waywallen markup → Rexon (one command).

Purpose
==========
The «Wallpaper Engine» tab shows the QML pages of the **waywallen** project
(MIT, © hypengw) as is: the directory
`data/qt_wallpaper_engine/qml_ui/waywallen/ui` — their `ui/qml`, `ui/i18n` and
two pictures from `ui/assets`, vendored VERBATIM from the tag in `ui/UPSTREAM_TAG`.
The types their C++ provides are provided here by the `rexon-gui` and
`rexon-qt-filter` crates under the same module name `waywallen.ui`.

Previously an upstream release was carried over by hand, and three things were done by hand too,
each of which can silently break the tab:

  * `ui/qmldir` was appended line by line, although upstream has the module contents
    ready — as the `qml_files` and `singletons` lists in `ui/build.lua`;
  * our patches lived INSIDE the vendored files with a note "on update
    carry this over too" — works as long as you remember;
  * a new type name `W.Foo` that appeared upstream was discovered only
    as an empty page on a live run.

This script in one run:

  1. takes `ui/qml`, the translation catalogs `ui/i18n/waywallen_{ru,de}.ts` and
     the vendored `ui/assets` from the given upstream tag (a local clone OR
     git) — byte for byte;
  2. GENERATES `ui/qmldir` from `ui/build.lua` of the same tag (not a single handwritten
     line is left in it) — of the same form the upstream module generator writes
     (lito-qt `qt.qml_module`, `prefer_resources = false`):
     `module`, `typeinfo module.qmltypes`, components with the upstream module
     version, `depends` from the `imports` list. Next to it lies `module.qmltypes`
     of the same tag — the inventory of C++ types from the upstream build (it is not in the
     sources); when the tag changes it must be put there again, otherwise exit code 4;
  3. applies our patch series `patches/NNNN-*.patch` (`git apply`);
     a patch whose change upstream has already accepted itself (a backport) is recognized by
     the fact that it applies in reverse — it is skipped and called
     redundant rather than aborting the sync;
  4. prints the names of `W.*` types that the vendored QML calls but our module
     does not register — i.e. exactly the lines that were "forgotten";
  5. writes `ui/UPSTREAM_TAG` and `ui/QML_MATERIAL_PIN` — the revision of the
     `Qcm.Material` library from `lito.lock` of the same release, so that
     the markup and the library under it do not drift apart (it is taken by
     `tools/build_qml_material.sh`);
  6. writes out `tools/qextra_plugin/lito.lock` — a slice of the same `lito.lock`:
     the QExtra revision (`QE.AnimatedImage`, `QE.ImageCache`) with all its
     dependencies, with which `tools/build_qml_qextra.sh` builds
     the real plugin; and names the `QE.*` types that our
     QML stand-in `qml_ui/QExtra/` does not have.

Everything is assembled in a separate directory and moved into the tree ONLY when
all steps have passed. A patch did not apply — the tree is untouched, exit code != 0.
That is, "either synced entirely, or nothing is broken".

Usage
=============
    # from a local clone (default ~/Загрузки/owe-ref/waywallen),
    # the tag is taken from ui/UPSTREAM_TAG:
    scripts/sync-waywallen-qml.py
    scripts/sync-waywallen-qml.py --source /path/to/waywallen --ref v0.3.9

    # directly from the git upstream (clones into a temporary directory):
    scripts/sync-waywallen-qml.py --git https://github.com/hypengw/waywallen \\
        --ref v0.3.9

    # only check for divergence, write nothing (for CI/cron):
    scripts/sync-waywallen-qml.py --check

Full automation
====================
`--check` gives exit 0 (the tree matches "upstream + our patches") /
3 (divergence) / 1 (a patch did not apply) / 2 (no source) /
4 (matched, but upstream has an unregistered type `W.*` or
`QE.*`) — this
is enough both for a CI gate and a cron watcher: run `--git <url> --ref
<new tag> --check`, on code 3 open a task to update the vendor.
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
# Layout
# ---------------------------------------------------------------------------

_HERE = os.path.dirname(os.path.abspath(__file__))
_REPO = os.path.dirname(_HERE)

#: Vendor root: ui/, patches/, LICENSE and ATTRIBUTION.md live here.
VENDOR_DIR = os.path.join(
    _REPO, "data", "qt_wallpaper_engine", "qml_ui", "waywallen")
#: The vendored tree itself — the script rebuilds it entirely.
UI_DIR = os.path.join(VENDOR_DIR, "ui")
#: Our patch series on top of the vendor (applied in ascending name order).
PATCH_DIR = os.path.join(VENDOR_DIR, "patches")

#: What we take from upstream byte for byte. The whole `ui/qml` directory plus: the
#: `ui/i18n` translation directory AS A WHOLE (each `.ts` is compiled into its own
#: `waywallen_<lang>.qm`) and exactly the two `assets` files that upstream
#: puts into its QML module (`resources` in build.lua) — the rest of its
#: `assets` is read by the package, not by QML. Translations — as a directory, not a list
#: of languages: a list with ru and de lost Chinese on 08.11.2026, which
#: upstream had since 0.5.0 (the guard is `vendor_has_every_upstream_catalog` in
#: rust/rexon-core/tests/remote_l10n_parity.rs).
UPSTREAM_TREE = "ui/qml"
UPSTREAM_FILES = (
    "ui/i18n",
    "ui/assets/waywallen-ui.svg",
    "ui/assets/waywallen-ui.releases.xml",
)
#: Description of the upstream build — the source of the QML module contents. Not vendored itself.
BUILD_LUA = "ui/build.lua"
#: The upstream dependency lock — the source of the QmlMaterial (`Qcm.Material`) revision
#: that `tools/build_qml_material.sh` builds. Not vendored itself.
LOCK_FILE = "lito.lock"
#: Where this revision is written. The pin must move TOGETHER with the vendor tag:
#: once they drift apart, it gives the nastiest class of defects — the vendored QML
#: is new, the library under it is old, and the divergence is visible only by eye on
#: a live page (a caught case — the title case of Russian labels,
#: fixed by hypengw in 043c09e and not reaching our build for a month).
MATERIAL_PIN_NAME = "QML_MATERIAL_PIN"

#: The QExtra plugin wrapper: its `lito.lock` is a slice of the upstream lock, with it
#: `tools/build_qml_qextra.sh` builds the real `QExtra` (`--locked`).
#: The slice moves TOGETHER with the vendor tag for the same reason as the pin above:
#: the markup calls `QE.*` of the revision the release was built with.
QEXTRA_WRAPPER_DIR = os.path.join(_REPO, "tools", "qextra_plugin")
QEXTRA_LOCK_PATH = os.path.join(QEXTRA_WRAPPER_DIR, LOCK_FILE)
QEXTRA_LOCK_REL = os.path.relpath(QEXTRA_LOCK_PATH, _REPO)
#: Name of the slice in the candidate directory (next to `ui/`, does not go into the vendor tree).
QEXTRA_STAGE_NAME = "qextra.lito.lock"
#: Our QML stand-in for the `QExtra` module — the core takes it when the real
#: plugin is missing or unusable (`qml_modules::choose_qextra`), and always
#: under `offscreen`. So every vendor `QE.*` must be in it too.
QEXTRA_SHIM_QMLDIR = os.path.join(
    _REPO, "data", "qt_wallpaper_engine", "qml_ui", "QExtra", "qmldir")

#: QML module name. The qmldir component version is taken from `version` in
#: `ui/build.lua` (as lito-qt does), while Rust registers the C++ types as 1.0:
#: the vendored QML imports the module without a version (`import waywallen.ui as W`)
#: and sees both halves — this is kept by the case "the vendored I18n calls our
#: PluginTranslations" in `rust/rexon-gui/src/qml_int64_probe.rs`.
QML_MODULE = "waywallen.ui"

#: The inventory of the module's C++ types (`module.qmltypes`) from the upstream build. It is not in the
#: sources: it is printed by qmltyperegistrar during `lito build -p waywallen-ui`
#: (`build/<profile>/generated/waywallen-ui/lito-qml/waywallen/ui/`). Comparison
#: with our registrations is done by `rust/rexon-gui/tests/waywallen_qmltypes_contract.rs`.
QMLTYPES_NAME = "module.qmltypes"

DEFAULT_SOURCE = os.path.expanduser("~/Загрузки/owe-ref/waywallen")

#: Where to look for the module's type registrations: the C++ shims of our crates call
#: `qmlRegisterType<T>("waywallen.ui", 1, 0, "T")` with a string literal.
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
# Upstream tag
# ---------------------------------------------------------------------------

def _tag_file() -> str:
    return os.path.join(UI_DIR, "UPSTREAM_TAG")


def _current_ref() -> str:
    """The tag the vendor is on now: the first word of `ui/UPSTREAM_TAG`."""
    path = _tag_file()
    if not os.path.isfile(path):
        return ""
    first = _read(path).strip().split()
    return first[0] if first else ""


# ---------------------------------------------------------------------------
# Upstream: laying out the needed subtree into a temporary directory
# ---------------------------------------------------------------------------

def _git(args: list[str], cwd: str | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd,
                          capture_output=True, text=True)


def _upstream_tree(args) -> tuple[str, str, tempfile.TemporaryDirectory]:
    """Lay out the needed upstream paths into a temporary directory.

    Returns (directory, the resolved ref as `<ref> (<short sha>)`,
    the temporary object — its cleanup is called by the caller).
    """
    tmp = tempfile.TemporaryDirectory(prefix="waywallen-qml-")
    ref = args.ref or _current_ref()
    if not ref:
        tmp.cleanup()
        _eprint("[err] the tag is not given and cannot be read from ui/UPSTREAM_TAG — "
                "pass --ref")
        raise SystemExit(2)

    if args.git:
        repo = os.path.join(tmp.name, "clone")
        cmd = ["clone", "--depth", "1", "--branch", ref, args.git, repo]
        _eprint(f"[git] cloning {args.git} (ref={ref})…")
        res = _git(cmd)
        if res.returncode != 0:
            tmp.cleanup()
            _eprint("[git] cloning failed:\n"
                    + (res.stderr or res.stdout).strip())
            raise SystemExit(2)
        source, archive_ref = repo, "HEAD"
    else:
        source, archive_ref = args.source, ref
        if not os.path.isdir(os.path.join(source, ".git")):
            tmp.cleanup()
            _eprint(f"[err] not a waywallen clone: {source}")
            raise SystemExit(2)

    sha = _git(["rev-parse", "--short", archive_ref], cwd=source)
    if sha.returncode != 0:
        tmp.cleanup()
        _eprint(f"[err] no such ref in {source}: {ref}\n"
                + (sha.stderr or "").strip())
        raise SystemExit(2)

    out = os.path.join(tmp.name, "upstream")
    os.makedirs(out, exist_ok=True)
    paths = [UPSTREAM_TREE, *UPSTREAM_FILES, BUILD_LUA, LOCK_FILE]
    tar = subprocess.run(["git", "archive", archive_ref, *paths],
                         cwd=source, capture_output=True)
    if tar.returncode != 0:
        tmp.cleanup()
        _eprint("[git] archive failed:\n"
                + tar.stderr.decode("utf-8", "replace").strip())
        raise SystemExit(2)
    untar = subprocess.run(["tar", "-x", "-C", out], input=tar.stdout,
                           capture_output=True)
    if untar.returncode != 0:
        tmp.cleanup()
        _eprint("[tar] unpacking failed:\n"
                + untar.stderr.decode("utf-8", "replace").strip())
        raise SystemExit(2)
    return out, f"{ref} ({sha.stdout.strip()})", tmp


# ---------------------------------------------------------------------------
# qmldir from build.lua
# ---------------------------------------------------------------------------

_LUA_LIST_RE = r"{}\s*=\s*\{{(.*?)\}}"


def _lua_list(text: str, key: str) -> list[str]:
    """Strings from a lua list `key = { "a", "b" }` in declaration order."""
    m = re.search(_LUA_LIST_RE.format(re.escape(key)), text, re.S)
    if not m:
        return []
    return re.findall(r'"([^"]+)"', m.group(1))


def _qmldir_text(build_lua: str, ref: str) -> str:
    """Build `ui/qmldir` from `qt.qml_module` of the upstream `ui/build.lua`.

    The same text lito-qt writes (`generate_qmldir` in `qt/qml.lua`) for
    a laid-out copy without `prefer`: a type is named after the file,
    singletons are marked with the word `singleton`, the version is the module `version`,
    `depends` are its `imports`. lito-qt writes no comments, and neither do we:
    then the vendored copy and the installed module are interchangeable.
    """
    del ref  # the tag is written into UPSTREAM_TAG; upstream does not have it in qmldir
    files = _lua_list(build_lua, "qml_files")
    singletons = set(_lua_list(build_lua, "singletons"))
    imports = _lua_list(build_lua, "imports")
    version = re.search(r'qt\.qml_module\(\{.*?\bversion\s*=\s*"([^"]+)"',
                        build_lua, re.S)
    if not version:
        raise RuntimeError("no version found for qt.qml_module in ui/build.lua")
    if not files:
        raise RuntimeError("no qml_files list found in ui/build.lua")
    unknown = singletons - set(files)
    if unknown:
        raise RuntimeError("upstream singletons outside the qml_files list: "
                           + ", ".join(sorted(unknown)))

    lines = [f"module {QML_MODULE}", f"typeinfo {QMLTYPES_NAME}"]
    for rel in files:
        name = os.path.basename(rel)[: -len(".qml")]
        prefix = "singleton " if rel in singletons else ""
        lines.append(f"{prefix}{name} {version.group(1)} {rel}")
    lines += [f"depends {name}" for name in imports]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# QmlMaterial revision from the upstream lock
# ---------------------------------------------------------------------------

_MATERIAL_RE = re.compile(
    r'name\s*=\s*"qml-material"\s*\n\s*source\s*=\s*"[^"#]*#([0-9a-f]{7,40})"')


def _material_pin(lock_text: str) -> str:
    """The QmlMaterial commit this waywallen release was built on."""
    m = _MATERIAL_RE.search(lock_text)
    if not m:
        raise RuntimeError("the upstream lito.lock has no revision "
                           "qml-material")
    return m.group(1)


# ---------------------------------------------------------------------------
# Slice of the upstream lock for the QExtra plugin
# ---------------------------------------------------------------------------

_LOCK_BLOCK_RE = re.compile(r"(?m)^(?=\[\[packages\]\]\n)")
_QEXTRA_PIN_RE = re.compile(r'QExtra\.git#([0-9a-f]{7,40})')


def _qextra_pin(lock_text: str) -> str:
    """QExtra commit from the lock (upstream or our slice); '' — not found."""
    m = _QEXTRA_PIN_RE.search(lock_text)
    return m.group(1) if m else ""


def _qextra_lock(lock_text: str) -> str:
    """Slice of the upstream lock: everything the QExtra plugin wrapper depends on.

    The whole foreign lock does not suit the wrapper: lito rejects packages that
    are not in its graph. The dependency closure of the wrapper is taken (they are written in
    its `lito.toml`) plus the entry of the wrapper itself; blocks — alphabetically, as
    lito writes them, otherwise `--locked` would consider the file changed.
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
            raise RuntimeError(f"the upstream lito.lock has no package «{name}» "
                               f"needed by the QExtra plugin ({QEXTRA_LOCK_REL})")
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
    """'' — the slice in the tree matches the candidate; otherwise — how it diverged."""
    new = _read(stage_lock)
    try:
        old = _read(QEXTRA_LOCK_PATH)
    except OSError:
        return "no file in the tree"
    if old == new:
        return ""
    was, now = _qextra_pin(old), _qextra_pin(new)
    if was != now:
        return f"QExtra changed: {was[:7] or '?'} → {now[:7] or '?'}"
    return "the QExtra revision is the same, its dependencies changed"


def _check_qextra_lock(stage_lock: str) -> int:
    drift = _qextra_lock_drift(stage_lock)
    if not drift:
        print(f"[check] OK: {QEXTRA_LOCK_REL} matches the slice of the upstream "
              f"lito.lock")
        return 0
    _eprint(f"[check] DIVERGENCE: {QEXTRA_LOCK_REL} ({drift})")
    return 3


def _install_qextra_lock(stage_lock: str) -> None:
    drift = _qextra_lock_drift(stage_lock)
    if not drift:
        return
    shutil.copyfile(stage_lock, QEXTRA_LOCK_PATH)
    print(f"[sync] updated: {QEXTRA_LOCK_REL} ({drift})")
    print("[sync] after this: rebuild the plugin with tools/build_qml_qextra.sh "
          "and update the _qe_git/_qe_reg lists in packaging/aur/PKGBUILD "
          "(its prepare() checks them against this file)")


# ---------------------------------------------------------------------------
# Patch series
# ---------------------------------------------------------------------------

def _patches() -> list[str]:
    if not os.path.isdir(PATCH_DIR):
        return []
    return sorted(f for f in os.listdir(PATCH_DIR) if f.endswith(".patch"))


#: A patch header line: «Удалить-при: <text>» ("remove when"). Our own change that
#: upstream made UNNECESSARY in a way other than absorbing it verbatim (reverse
#: application does not recognize that): when the marker text appeared in the file the
#: patch edits, a patch that did not apply is obsolete rather than conflicting.
OBSOLETE_MARKER = "Удалить-при:"


def _obsolete_marker_hit(path: str, root: str) -> str | None:
    """The marker from «Удалить-при:» is found in the patch's target file → the marker itself."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    head = text.split("\n---\n", 1)[0]
    markers = [ln.split(OBSOLETE_MARKER, 1)[1].strip()
               for ln in head.splitlines() if ln.startswith(OBSOLETE_MARKER)]
    if not markers:
        return None
    targets = [ln[len("+++ b/"):].strip() for ln in text.splitlines()
               if ln.startswith("+++ b/")]
    for target in targets:
        try:
            with open(os.path.join(root, target), encoding="utf-8") as f:
                body = f.read()
        except OSError:
            continue
        for marker in markers:
            if marker and marker in body:
                return marker
    return None


def _apply_patches(root: str) -> list[str]:
    """Apply the series to the laid-out tree. A patch did not apply — RuntimeError.

    `root` — the directory that contains `ui/`: paths in the patches are written
    relative to it (`a/ui/qml/...`), so the series applies with `git apply -p1`.

    ⭐ A patch that did not apply is not a conflict yet. The series holds BACKPORTS: we
    take a change that upstream has already accepted but that arrived after
    the vendor tag, and we cannot wait for its bump. Such a patch must stop
    applying exactly when the vendor grows up to a release with this
    change — and the former rule "the first patch that does not apply aborts the WHOLE
    sync" turned our own fix into a lock on the bump
    (a caught case on 19.09.2026). So before the verdict we check
    reverse application: it applied in reverse — the change is ALREADY in the upstream
    tree, the patch is skipped and called redundant. It applied neither way
    — that is a real conflict.

    Returns the names of patches that upstream has already absorbed: their place is
    in the summary, not in silence, otherwise the series would grow dead files.
    """
    series = _patches()
    if not series:
        print("[patch] the series is empty — nothing to apply")
        return []
    obsolete: list[str] = []
    for name in series:
        path = os.path.join(PATCH_DIR, name)
        res = _git(["apply", "--whitespace=nowarn", path], cwd=root)
        if res.returncode == 0:
            print(f"[patch] applied: {name}")
            continue
        back = _git(["apply", "--reverse", "--check", "--whitespace=nowarn",
                     path], cwd=root)
        if back.returncode == 0:
            obsolete.append(name)
            print(f"[patch] ALREADY UPSTREAM, skipped: {name}")
            continue
        marker = _obsolete_marker_hit(path, root)
        if marker is not None:
            obsolete.append(name)
            print(f"[patch] OBSOLETE, skipped: {name} — upstream already has "
                  f"«{marker}» (the «{OBSOLETE_MARKER}» line in the patch header)")
            continue
        raise RuntimeError(
            f"patch {name} did NOT APPLY to upstream:\n"
            + (res.stderr or res.stdout).strip()
            + "\n\nThis is a real conflict: upstream touched the same lines "
              "(we do NOT have the upstream change in this place — otherwise the patch would apply "
              "in reverse and be skipped).\nPort the change by hand "
              "and rebuild the patch (`git diff` of the vendored file → "
            + f"{os.path.relpath(path, _REPO)}).")
    if obsolete:
        print("[patch] ⚠️ upstream absorbed our changes or made them unnecessary — these series files "
              "are no longer needed, remove them together with the vendor bump:")
        for name in obsolete:
            print(f"[patch]   patches/{name}")
    return obsolete


# ---------------------------------------------------------------------------
# New W.* type names
# ---------------------------------------------------------------------------

_REGISTER_RE = re.compile(
    r'"' + re.escape(QML_MODULE) + r'"\s*,\s*\d+\s*,\s*\d+\s*,\s*"(\w+)"')
_USE_RE = re.compile(r"\bW\.([A-Z]\w*)")
_USE_QE_RE = re.compile(r"\bQE\.([A-Z]\w*)")


def _registered_types() -> set[str]:
    """Names our crates register in the `waywallen.ui` module."""
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
    """`W.Foo` names met in the vendored QML → the files where they were met."""
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
    """Print the `W.*` types our module does not provide. Return their list."""
    known = _registered_types() | _qmldir_types(qmldir_text)
    used = _used_types(os.path.join(stage_ui, "qml"))
    missing = sorted(n for n in used if n not in known)
    if not missing:
        print(f"[types] no new W.* names "
              f"(the vendor calls {len(used)}, the module provides {len(known)})")
        return []
    _eprint("[types] ⚠️ the vendored QML calls types our module does NOT provide "
            "— the page will silently stay empty:")
    for name in missing:
        where = ", ".join(used[name][:3])
        _eprint(f"[types]   W.{name}  ← {where}")
    _eprint("[types] fixed by a registration in rust/rexon-gui/cpp/"
            "rexon_register_we.cpp (and a type in rexon-gui).")
    return missing


def _report_new_qextra_types(stage_ui: str) -> list[str]:
    """Print the `QE.*` types that our QML stand-in for `QExtra` does not have.

    It is the stand-in that is checked: it lies in the tree and works every time
    there is no real plugin. What the plugin itself provides is checked by the
    «QML modules» step of `rexon-vendor-update` — against the built `module.qmltypes`.
    """
    try:
        known = _qmldir_types(_read(QEXTRA_SHIM_QMLDIR))
    except OSError:
        known = set()
    used = _used_types(os.path.join(stage_ui, "qml"), _USE_QE_RE)
    missing = sorted(n for n in used if n not in known)
    if not missing:
        print(f"[types] no new QE.* names "
              f"(the vendor calls {len(used)}, the stand-in provides {len(known)})")
        return []
    _eprint("[types] ⚠️ the vendored QML calls QExtra types that our "
            "QML stand-in does not have — without the real plugin the tab will stay empty:")
    for name in missing:
        where = ", ".join(used[name][:3])
        _eprint(f"[types]   QE.{name}  ← {where}")
    _eprint("[types] fixed by a type in data/qt_wallpaper_engine/qml_ui/QExtra/ "
            "(and a line in its qmldir).")
    return ["QE." + n for n in missing]


# ---------------------------------------------------------------------------
# Building the candidate and comparing with the tree
# ---------------------------------------------------------------------------

def _stage(upstream: str, ref: str, workdir: str) -> tuple[str, list[str]]:
    """Build the vendor candidate: upstream + qmldir + patches + UPSTREAM_TAG."""
    root = os.path.join(workdir, "stage")
    os.makedirs(root, exist_ok=True)
    shutil.copytree(os.path.join(upstream, "ui"), os.path.join(root, "ui"))

    src_lua = os.path.join(root, BUILD_LUA)
    lua_text = _read(src_lua)
    os.remove(src_lua)                       # we do not vendor build.lua itself
    # The lock lies in the upstream ROOT, not in ui/ — we read it from there and do not
    # vendor it: only one line is needed from it, the QmlMaterial revision.
    lock_text = _read(os.path.join(upstream, LOCK_FILE))

    stage_ui = os.path.join(root, "ui")
    qmldir = _qmldir_text(lua_text, ref)
    with open(os.path.join(stage_ui, "qmldir"), "w", encoding="utf-8") as fh:
        fh.write(qmldir)
    print(f"[qmldir] generated from ui/build.lua: "
          f"{len(qmldir.splitlines())} lines, handwritten 0")

    _apply_patches(root)

    with open(os.path.join(stage_ui, "UPSTREAM_TAG"), "w",
              encoding="utf-8") as fh:
        fh.write(ref + "\n")

    pin = _material_pin(lock_text)
    with open(os.path.join(stage_ui, MATERIAL_PIN_NAME), "w",
              encoding="utf-8") as fh:
        fh.write(pin + "\n")
    print(f"[material] QmlMaterial revision of this release: {pin[:7]} "
          f"(taken by tools/build_qml_material.sh)")

    with open(os.path.join(root, QEXTRA_STAGE_NAME), "w",
              encoding="utf-8") as fh:
        fh.write(_qextra_lock(lock_text))
    print(f"[qextra] QExtra revision of this release: "
          f"{_qextra_pin(lock_text)[:7]} (taken by tools/build_qml_qextra.sh)")

    missing = _carry_qmltypes(stage_ui, ref)
    missing += _report_new_types(stage_ui, qmldir)
    missing += _report_new_qextra_types(stage_ui)
    return stage_ui, missing


def _carry_qmltypes(stage_ui: str, ref: str) -> list[str]:
    """Carry `module.qmltypes` over from the tree if it is of the same tag.

    The upstream sources have no inventory, so the candidate takes it from the live
    tree — but only for the same tag: the inventory of a previous release next to
    the new QML would lie to the type check. For a new tag there is none, and this
    is said out loud (code 4), while the tree stays without it — the Rust check
    `waywallen_qmltypes_contract` then turns red.
    """
    live = os.path.join(UI_DIR, QMLTYPES_NAME)
    tag = ref.split()[0] if ref else ""
    if os.path.isfile(live) and tag and tag == _current_ref():
        shutil.copy2(live, os.path.join(stage_ui, QMLTYPES_NAME))
        print(f"[qmltypes] C++ type inventory of tag {tag} — from the tree")
        return []
    if tag and _qmltypes_from_appimage(tag, os.path.join(stage_ui, QMLTYPES_NAME)):
        return []
    _eprint(f"[qmltypes] NO C++ type inventory for {tag or ref}: build "
            f"waywallen-ui of this tag (lito build -p waywallen-ui) and put "
            f"build/<profile>/generated/waywallen-ui/lito-qml/waywallen/ui/"
            f"{QMLTYPES_NAME} into {os.path.relpath(UI_DIR, _REPO)}/")
    return [QMLTYPES_NAME]


#: Where the interface module inventory lies in the release AppImage (since waywallen 0.6.0
#: the `waywallen.ui` module is installed as files — waywallen#346).
APPIMAGE_QMLTYPES = "usr/share/qml/waywallen/ui/" + QMLTYPES_NAME


def _qmltypes_from_appimage(tag: str, dest: str) -> bool:
    """Get the tag's `module.qmltypes` from the official release AppImage.

    There is no need to build waywallen-ui for one inventory: the same file lies in
    the release image. The image is `$REXON_WAYWALLEN_APPIMAGE`, otherwise
    `~/.cache/rexon/waywallen-<version>-x86_64.AppImage` (missing — downloaded
    from the GitHub release). The file is extracted with `unsquashfs` at the image offset, the
    image itself is not run (except for its loader's `--appimage-offset`).
    """
    version = tag[1:] if tag.startswith("v") else tag
    image = os.environ.get("REXON_WAYWALLEN_APPIMAGE") or os.path.expanduser(
        f"~/.cache/rexon/waywallen-{version}-x86_64.AppImage")
    if not os.path.isfile(image):
        url = (f"https://github.com/waywallen/waywallen/releases/download/"
               f"{tag}/waywallen-{version}-x86_64.AppImage")
        try:
            import urllib.request
            os.makedirs(os.path.dirname(image), exist_ok=True)
            print(f"[qmltypes] downloading {url}")
            urllib.request.urlretrieve(url, image + ".part")
            os.replace(image + ".part", image)
            os.chmod(image, 0o755)
        except Exception as e:  # network, no release
            _eprint(f"[qmltypes] release image {tag} not obtained: {e}")
            return False
    try:
        offset = subprocess.run([image, "--appimage-offset"], capture_output=True,
                                text=True, timeout=30, check=True).stdout.strip()
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "x")
            subprocess.run(["unsquashfs", "-q", "-o", offset, "-d", out, image,
                            APPIMAGE_QMLTYPES], capture_output=True, timeout=120,
                           check=True)
            got = os.path.join(out, APPIMAGE_QMLTYPES)
            if not os.path.isfile(got):
                return False
            shutil.copy2(got, dest)
    except (OSError, subprocess.SubprocessError) as e:
        _eprint(f"[qmltypes] cannot extract from image {image}: {e}")
        return False
    print(f"[qmltypes] C++ type inventory of tag {tag} — from the release AppImage ({image})")
    return True


def _walk_rel(root: str) -> set[str]:
    out: set[str] = set()
    for base, _dirs, files in os.walk(root):
        for f in files:
            out.add(os.path.relpath(os.path.join(base, f), root))
    return out


def _diff_trees(stage_ui: str, live_ui: str) -> tuple[list[str], list[str],
                                                      list[str]]:
    """(differ, only in the candidate, only in the tree) — by content."""
    a, b = _walk_rel(stage_ui), _walk_rel(live_ui)
    differ = [r for r in sorted(a & b)
              if not filecmp.cmp(os.path.join(stage_ui, r),
                                 os.path.join(live_ui, r), shallow=False)]
    return differ, sorted(a - b), sorted(b - a)


def _check(stage_ui: str) -> int:
    if not os.path.isdir(UI_DIR):
        _eprint(f"[check] the vendor is not in the tree: {UI_DIR}")
        return 3
    differ, only_new, only_old = _diff_trees(stage_ui, UI_DIR)
    if not (differ or only_new or only_old):
        print("[check] OK: the tree matches \"upstream + our patches\" "
              "byte for byte")
        return 0
    for r in differ:
        _eprint(f"[check] DIVERGENCE: {r}")
    for r in only_new:
        _eprint(f"[check] MISSING in the tree: {r}")
    for r in only_old:
        _eprint(f"[check] MISSING upstream (extra file in the tree): {r}")
    return 3


def _install(stage_ui: str) -> int:
    """Move the finished candidate into the tree. On error put things back as they were."""
    differ, only_new, only_old = ([], [], [])
    if os.path.isdir(UI_DIR):
        differ, only_new, only_old = _diff_trees(stage_ui, UI_DIR)
        if not (differ or only_new or only_old):
            print("[sync] unchanged: the tree already matches")
            return 0
    backup = UI_DIR + ".bak"
    if os.path.exists(backup):
        shutil.rmtree(backup)
    try:
        if os.path.isdir(UI_DIR):
            os.rename(UI_DIR, backup)
        shutil.copytree(stage_ui, UI_DIR)
    except Exception as exc:
        _eprint(f"[sync] ROLLBACK because of an error: {exc}")
        if os.path.isdir(backup):
            if os.path.isdir(UI_DIR):
                shutil.rmtree(UI_DIR)
            os.rename(backup, UI_DIR)
        return 1
    shutil.rmtree(backup, ignore_errors=True)
    for r in differ:
        print(f"[sync] updated: {r}")
    for r in only_new:
        print(f"[sync] added: {r}")
    for r in only_old:
        print(f"[sync] removed: {r}")
    print("[sync] DONE — the vendored markup is synced.")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Sync of the vendored waywallen QML → Rexon "
                    "(ui/qml + qmldir from build.lua + our patch series).")
    ap.add_argument("--source", default=DEFAULT_SOURCE,
                    help=f"root of the local waywallen clone "
                         f"(default {DEFAULT_SOURCE})")
    ap.add_argument("--git", default="",
                    help="upstream git URL (cloned into a temporary directory; "
                         "takes priority over --source)")
    ap.add_argument("--ref", default="",
                    help="upstream tag/branch (default — the tag from "
                         "ui/UPSTREAM_TAG)")
    ap.add_argument("--check", action="store_true",
                    help="only compare (write nothing); "
                         "exit 0=matches, 3=divergence")
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
                # Both checks to the end: a markup divergence must not
                # hide a divergence of the slice.
                rc = _check(stage_ui)
                lock_rc = _check_qextra_lock(stage_lock)
                rc = rc or lock_rc
            else:
                rc = _install(stage_ui)
                if rc == 0:
                    _install_qextra_lock(stage_lock)
            # An unregistered type is no reason to break the sync
            # (the upstream page is already new, our crate is catching up), but
            # it must not be kept silent: exit code 4 is visible in CI too.
            if rc == 0 and missing:
                return 4
            return rc
    finally:
        tmp.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
