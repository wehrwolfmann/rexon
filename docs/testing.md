# How to test Rexon

Building, tests, test stands and debugging: what to run and what to look at.

## ⚠️ Rust build: a bare `cargo build --release` does NOT build the tabs

The first thing to know before any UI measurement.

`rust/Cargo.toml` sets `default-members = ["rexon-core"]` — on purpose,
so that a bare `cargo test` of the core does not pull in Qt6 (rexon-gui, rexon-qt-filter on
cxx-qt) and the Python interpreter (rexon-py). The flip side: **a bare
`cargo build --release` builds the core alone**, reports "Finished" within
seconds and leaves the `rexon-main-window` window with its QML tabs stale.
You end up measuring the day before yesterday while being sure you built today's code. This bit us on 2026-08-29: the build "succeeded",
and the .so was two days old.

Measurement after editing a tab's source: `cargo build --release` — "Finished
in 0.05s", the tab build untouched; `cargo build-app` — 1 min 45 s, fresh.

Build with the aliases from `rust/.cargo/config.toml` (`cargo --list`
prints them together with their expansion):

```bash
cd rust
cargo build-app          # core + rexon-main-window + librexon_qt_filter.so — THIS is "build the project"
cargo build-app-debug    # the same in debug (the release build stays stale!)
cargo build-splash       # the login splash binary, as a separate command (why — see below)
cargo test-core          # core tests: no Qt and no Python
cd rexon-py && maturin build --release   # the Python bridge: maturin builds it, not cargo
```

Check with your own eyes, not by the word "Finished":

```bash
ls -la --time-style=+%H:%M rust/target/release/rexon-main-window
```

⚠️ The bridge wheel: `maturin failed … libssl-566862b9.so.3 could not be located`.
The hash in the library name is the mark of a file ALREADY repaired by the wheel tooling: maturin
reuses `rust/target/maturin/librexon_py.so` left over from the previous
build and repairs what was already repaired. The cure is deleting the intermediate directory, not
digging into openssl:

```bash
rm -rf rust/target/maturin && (cd rust/rexon-py && maturin build --release)
```

Installing the built wheel (PEP 668 refuses otherwise):

```bash
pip install --user --force-reinstall --no-deps --break-system-packages \
    rust/target/wheels/rexon_py-*-manylinux*.whl
```

Why the splash is a separate command. `rexon-splash-overlay` pulls in its own stack
(wayland/EGL). When it ends up in ONE cargo run together with `rexon-gui`,
the feature unification of shared dependencies (libc, rustix, bitflags, log…) changes
and `rexon-gui` is rebuilt from scratch: alternating "everything in one run" and
`build-app` burned 1 min 40 s for nothing. In separate runs — 0.1 s.

⚠️ The aliases are read from the current directory, not from `--manifest-path`:
they work from `rust/` or from inside a crate. From the repository root
(`cargo build --manifest-path rust/Cargo.toml`) cargo does not see that file
— there the package set is written out by hand with `-p`.

Full installation of the tabs on the machine (the QML module `waywallen.control` +
both .so files where the application looks for them) — `tools/build_qml_control.sh`;
it calls cargo for `rexon-gui` and `rexon-qt-filter` itself.

## Testing

```bash
# Syntax check of all modules
find rexon -name "*.py" | xargs -I{} python3 -m py_compile {}

# Running the application
rexon                       # Single Qt window (default)
rexon --tray                # straight to the system tray, no window (start at login)
rexon --desktops-qt         # standalone Qt tab "Desktops"
rexon --compare-qt          # "DE comparison"
rexon --appearance-qt       # "Appearance"
rexon --extensions-qt       # "Extensions"
rexon --settings-qt         # "Settings"
rexon --recover-kde         # emergency rollback of the KDE theme

# The main window whose OWNER is Rust (a separate entry point for developers).
# There are no widgets and no Python in the process: QGuiApplication + ApplicationWindow in
# QML. ⚠️ The product is still started as `rexon`, and it does not know this binary.
rust/target/release/rexon-main-window                    # the whole window
rust/target/release/rexon-main-window --section=steam    # straight to the given section
rust/target/release/rexon-main-window --section=appearance/screeneffects  # an "Appearance" section
QT_QPA_PLATFORM=offscreen rust/target/release/rexon-main-window --selftest
#   0 — checks passed, 1 — failure (prints REXON-MAINWINDOW-FAIL lines),
#   2 — the window did not report within 30 s. Bite-check of the stand: the same run with
#   `--section=steam` MUST turn red — that section has no page yet.

# Wallpaper host (standalone; the other types are drawn by the waywallen engine).
# The Python host is gone: wallpapers are shown by a binary of the rexon-gui crate.
# One host for video and web: the mode is `--mode video|web` (without the flag — video).
rust/target/release/rexon-wallpaper-host --mode video /path/to/video.mp4 [--windowed]
rust/target/release/rexon-wallpaper-host --mode video /path/to/wallpaper_dir  # WE wallpapers with project.json
rust/target/release/rexon-wallpaper-host --mode video /path/to/video.mp4 --selftest
rust/target/release/rexon-wallpaper-host --mode web /path/to/index.html --selftest  # code 5 — the page did not load

# Forced X11 mode (for debugging the fallback on a Wayland machine)
env -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE QT_QPA_PLATFORM=xcb \
    rust/target/release/rexon-wallpaper-host --mode video /path/video.mp4 --windowed -v

# Import checks
python3 -c "from rexon.extensions import Extension, ExtensionManager, VALID_TYPES; print('OK')"
python3 -c "from rexon.qt_main.window import RexonMainWindow; print('Qt OK')"
python3 -c "import rexon_py; print('helpers OK', rexon_py.wpm_is_wayland_session('wayland',''))"

# System installation (will ask for the pkexec password)
python3 -c "from rexon.installer import RexonInstaller; ok,m=RexonInstaller().install_to_system(); print(ok,m)"

# Dump all threads on a hang
kill -USR1 $(pgrep -f "python3 /usr/local/bin/rexon")

# Line count
find rexon -name "*.py" | xargs wc -l | tail -1
```

### Window tour: `--tour`

The window walks through its sections and stores by itself, INVISIBLY (`QT_QPA_PLATFORM=offscreen`
is set by the mode itself — no window appears on the live desktop), takes a screenshot of every screen and writes
a report on anything suspicious:

```bash
rust/target/release/rexon-main-window --tour=appearance/wallpapers --tour-out ~/tour
rust/target/release/rexon-main-window --tour                 # the whole window
#   --tour-query <word>    the search word (default: girl)
#   --tour-timeout <s>     overall limit (default: 1200 s)
#   without --tour-out — ~/.cache/rexon/tour
```

Section names are the same as for `--section` (only `appearance` has
subsections). In every "Appearance" section: every store of the switcher
(KDE Store, AUR, All, Wallhaven, Razer Axon…) → a screenshot; then the search word in
the first store and again every store — checking that the word stayed in
the field. Screenshots are `NN-<section>-<store>-<step>.png` (`browse`/`search`), next to them
`report.txt` and `journal.log` (the process error stream: Qt/QML messages and
the Rexon log). The report lists: tiles without a preview (k of n), empty results,
loading longer than 5 s or never finished (20 s), lost search text, a different
header between stores of the same section (whether there is a search field and where), a status
line with an error, network errors and QML warnings from the log — attached to the
screenshot during which they appeared.

Codes: 0 — clean, 1 — something suspicious, 2 — invalid flags, 3 — overall
timeout (the report is written then too — what got done and where it stopped).

⛔ The tour installs and applies nothing: only navigation, search and
browsing. The order, the waiting, the analysis and the ban on calls live in the core
(`rexon-core/src/main_window_tour.rs`, tests right there); the window calls a page
through a single door, `tourInvoke`, and only the methods allowed by
`main_window_tour::call_allowed`. Like
`--selftest`, the tour does not take the lock, the tray or the profile log. The other window sections (not "Appearance") get
one screenshot per section for now.

### Accessibility robot: `rexon-a11y-robot`

The second path next to `--tour`. The tour walks the window from inside; the robot
drives it from OUTSIDE, through the accessibility tree (AT-SPI2 over D-Bus) — the
same tree a screen reader (Orca) sees. It finds elements only by role and name,
acts like a person (switches section tabs, types into the search field, presses
Enter and search buttons, walks the focus with Tab) and writes down what it
could not find. So every run is also an accessibility audit: controls without a
name, controls that cannot take keyboard focus, clickable things without a role.

```bash
cd rust && cargo build-app                     # builds the robot next to the window
rust/target/release/rexon-a11y-robot           # all sections, report in ~/.cache/rexon/a11y
rust/target/release/rexon-a11y-robot --sections=оформ,steam --out ~/a11y
#   --window <path>    the window binary (default: rexon-main-window next to the robot)
#   --query <word>     the search word (default: girl)
#   --settle <s>       wait after every action (default: 2.5)
#   --tabs <n>         Tab presses for the keyboard walk (default: 30, 0 = off)
```

`--sections` matches substrings of the TAB LABELS as the window shows them (in the
interface language), not section ids: the robot knows only what is on screen.
Output folder: `report.txt` (summary at the end), `tree-NN.txt` — the whole tree of
every section (`[role] «name» (description) {actions} ✎=editable <S showing,
s sensitive, F focusable, * focused>`), `tree-NN-search.txt` — after the search,
`window.log`, `dbus.log`, `xvfb.log`. Exit codes: 0 — the run finished (findings do
not fail it), 1 — the stand could not start or the window never appeared on the
accessibility bus, 2 — bad arguments.

Isolation — nothing on the live desktop is touched, nothing is installed:

* its own X server: `Xvfb` on a free display number (`:140` and up), the window is
  invisible, synthetic key presses (XTest through the AT-SPI registry) go only there;
* its own session bus (`dbus-daemon --session`); the AT-SPI bus launcher and the
  registry from at-spi2-core are D-Bus activated on it — not the live Plasma, tray
  or notification daemons;
* its own profile: `HOME`, `XDG_{CONFIG,DATA,CACHE,STATE}_HOME`, `XDG_RUNTIME_DIR` in
  `$TMPDIR/rexon-a11y-<pid>`, removed at the end; the first-run wizard
  (`REXON_FIRST_RUN=never`) and the tutorial (`onboarding_enabled=false`) are off;
* Qt: `QT_QPA_PLATFORM=xcb`, `QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1` (the offscreen
  platform has no accessibility bridge, so the robot cannot use it), the AT-SPI
  bus address is exported as `AT_SPI_BUS_ADDRESS` too.

⛔ Every action goes through one door, `action_allowed` (tests in the same file,
`cargo test-core`): section tabs; focus; text only into a field whose name says
"search"/"поиск"; Press only on a button whose name says "search"/"найти". A name
with install/remove/apply/reset/… (`DENY_WORDS`) is refused even if it also says
"search". The report ends with the list of every action performed.

For QML: Qt Quick Controls (`Button`, `TabButton`, `TextField`, `CheckBox`,
`ItemDelegate`…) publish role and name by themselves (name = `text`). Three gaps
the robot found are closed in the shared components, so every section gets them:

* `RexonTextField` — Qt publishes the placeholder only as the description; the
  placeholder is now also the accessible name;
* `RexonButton`, `RexonDangerButton` — a label made only of symbols ("🔍", "✕",
  "🗑️") takes the (already translated) tooltip as its accessible name: its own,
  or that of the wrapper item (a disabled button does not catch hover, so the
  tooltip often hangs on the wrapper);
* the section tabs (`MainWindow.qml`) — a `TabButton` has only the focus action;
  `Accessible.onPressAction` adds the standard "Press".

A bare `MouseArea`/`TapHandler` publishes NOTHING — give the item
`Accessible.role`, `Accessible.name` and `Accessible.onPressAction` (as in
`DayNightSection.qml`, `ConkyScreenMap.qml`), the name from a string that is
already translated.

Inspecting by hand: Accerciser (GUI tree browser) shows the same tree; KDE's
selenium-webdriver-at-spi drives the same AT-SPI interfaces from Python tests.

---


## References: one entry point for everything

There are more than two hundred references in the tree, and they used to be updated in four different
ways. Now there is one entry point — `scripts/rexon-golden-update`:

```
scripts/rexon-golden-update --check   # writes nothing, says what diverged (code ≠ 0)
scripts/rexon-golden-update           # updates everything and prints the delta for each file
scripts/rexon-golden-update --list    # the table "reference → what updates it"
```

The run honestly splits references into three kinds: captured from the code, requiring a LIVE
waywallen daemon (or `lrelease`), and not recapturable at all. The second kind
without its prerequisites must say "NOT CHECKED" and name the reason — there is no silent
skip here. The full map with reasons is `docs/golden_reference_map.md`.

## Updating the vendored waywallen QML — one command

The directory `rexon/qt_wallpaper_engine/qml_ui/waywallen/ui` is not edited by hand:
`scripts/sync-waywallen-qml.py` rebuilds it as a whole.

```
scripts/sync-waywallen-qml.py --ref v0.3.9   # take a new upstream release
scripts/sync-waywallen-qml.py --check        # 0 — the tree matches, 3 — it diverged
scripts/sync-waywallen-qml.py --git https://github.com/hypengw/waywallen --ref v0.3.9
```

In one run it: puts `ui/qml`, `ui/i18n` and the vendored `ui/assets`
byte for byte from the tag; GENERATES `ui/qmldir` from their `ui/build.lua` (there are no handwritten
lines there); applies our series
`waywallen/patches/NNNN-*.patch`; writes out `ui/QML_MATERIAL_PIN` —
the `Qcm.Material` revision from `lito.lock` of the same release, which
`tools/build_qml_material.sh` picks up; prints the names of `W.*` types that the
vendored QML calls but our module does not register (exit code 4).

A patch that does not apply is a real conflict; the tree stays as it was, code 1.
To add your own change: edit the file in `ui/`, take a `git diff` of it
and put it into `patches/` under the next number.

⚠️ After a sync, rebuild `Qcm.Material` — the pin may have moved:
`tools/build_qml_material.sh`.

## After updating the vendored waywallen QML

The labels of the vendored pages are translated by the VENDORED catalogs
`data/qt_wallpaper_engine/qml_ui/waywallen/ui/i18n/waywallen_<language>.ts`
(waywallen 0.6.1: ru, de, zh_CN, fr, es, uk; the whole catalog is taken) — they are vendored together with `ui/qml`, from the same
commit as the one in `ui/UPSTREAM_TAG`. We have no label dictionary of our own and do not need one:
a translation of someone else's program is fixed IN THAT PROGRAM (otherwise nobody but us
gets it, and it drifts away from the original).

After updating the vendor, update **both** directories, `ui/qml` and `ui/i18n`, and rebuild
the catalog:

```
scripts/rexon-golden-update waywallen_qm   # each .ts → its own waywallen_<language>.qm
cargo test -p rexon-core --test remote_l10n_parity
```

The test turns red if the catalog and `ui/qml` diverged, and says in which direction:
vendored strings without a translation (the customer will see English) or catalog
entries that are no longer in `ui/qml`. If a vendor string is new and untranslated,
the translation is submitted to waywallen so that everyone gets it.

## Test stand for the privilege session helper ("one password per launch")

The privileged part (`rust/rexon-core/src/privilege/`, the binary
`rexon-priv-helper`) is tested without root, without `pkexec`, without a real
`pacman` and without a GUI — on three levels:

1. **Unit tests** of the `privilege/*` modules — in the regular `cargo test-core`.
2. **The helper as an ordinary process** — `tests/priv_stand.rs` + the binary
   `rexon-priv-stand`. The window is a real `privilege::client::Client`; the test's "pkexec
   shim" starts the helper with the socket on fd 0/1 and `PKEXEC_UID`, like
   pkexec after the password. The helper is the same `helper::run_session`, but with a "dry"
   `RootIo`: programs are not started, their argv is written to the stand log, files
   land under the root prefix `<stand>/root`. logind, the clock and the sleep counter are
   stand files (`session`, `clock_ms`, `slept_ms`), read by the real
   `LogindWatch` and `Revoker`. Scenarios: several operations with one password;
   closing the window (EOF) — exit; EOF and revocation in the middle of an operation — it is
   carried through; lock, 15 minutes of idle, sleep, an inactive session — new
   commands do not go through on the old right (a new password is needed); refusal for the permanent ones
   (`linux-cachyos`, `mesa`, `glibc`, `systemd`) and for the packages of the current desktop;
   a foreign build before the password and in the handshake (`BuildMismatch`, sticky); a foreign
   logind session; a foreign peer on the socket and the socket being inherited by the window's children.
3. **User namespace** — `tests/priv_userns.rs`: the REAL
   `rexon-priv-helper` (live `LiveRootIo`) as uid 0 in `unshare -r --mount`.
   `packaging/test/priv-ns-enter.sh` bind-mounts substitute `/usr/bin`
   (the scripts `pacman`, `loginctl`, `systemctl`), `/etc`, `/usr/share`,
   `/var/tmp` — only the helper sees them, nothing changes on the host.
   Real files are checked: SDDM theme permissions (0644/0755), a symlink in place of
   the destination is replaced rather than written through, archives with a symlink, `..` and
   an absolute path are rejected (`openat2`), work directories are cleaned up;
   the environment and descriptors of the helper's programs (clean, without the window socket); EOF
   in the middle of a transaction; `PR_SET_PDEATHSIG` (the window thread died, the socket is open).
   No `unshare`, or the kernel does not allow the namespace — the test writes
   "ПРОПУЩЕНО (стенд уровня 3): <причина>" ("SKIPPED (level 3 stand): <reason>") and does not fail.

```
scripts/priv-stand                  # levels 2 and 3 (via build-queue)
scripts/priv-stand --release-check  # + the release rexon-priv-helper contains no stand strings
scripts/priv-stand -- screen_lock   # only tests with this word in the name
# the same by hand:
BUILD_QUEUE_SLOTS=1 scripts/build-queue test -p rexon-core --features stand --test priv_stand --test priv_userns
```

⛔ The `stand` feature is for the stand only: the `rexon-priv-stand` binary and the
`priv_stand` test do not build without it (`required-features`), the package and
`cargo build-app` do not enable it. The stand code lives ONLY in
`src/bin/rexon-priv-stand.rs`: the library and the real helper do not
change with the feature. The guard is `priv_userns::stand_code_stays_out_of_release` (it also runs in
the regular `cargo test-core`).

4. **VM** (real pkexec, polkitd, logind, PLM) and the final acceptance with the package —
   per the plan, outside this stand.

## Pre-flight checks (mandatory after an edit)

After every edit:
- `.py` → `python3 -m py_compile <file>` (+ if installed — `ruff check <file>`)
- `.qml` → `scripts/qmllint-check <file>`
- `.rs` → `cd rust && cargo build-app` (a bare `cargo build` will not rebuild the tabs — see the section on the trap above)
- `.cpp/.hpp` → diagnostics in the IDE (clang LSP); a full CMake build — by hand
- screenshot-based regression → `scripts/image-diff new.png reference.png`

---
