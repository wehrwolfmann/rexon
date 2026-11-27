# Reference map: what updates what

There are more than two hundred references in the tree, and until 07.09.2026 they were updated in **four
different ways**. What updates one particular file had to be
found out in a separate pass each time — and that was one of the main cost items when
updating the vendor: accepting waywallen 0.3.8 took ~13 hours, of which **four
commits out of six were fixing our own scaffolding** rather than accepting someone else's changes.

> ⛔ **02.10.2026: Python is an archive only** (`_archived/`, git tag
> `python-final`). The Python generators (`*_cases.py`, `frozen_*.py`), the
> `rexon_py` bridge comparisons and capturing daemon frames with the Python client went there too.
> Their references stayed in the tree FROZEN and are listed in section 3 with that
> reason. The recapture mechanism remained for two groups: `de_matrix` and
> `waywallen_qm`.

Now there is one entry point:

```
scripts/rexon-golden-update --check     # writes nothing, says what diverged
scripts/rexon-golden-update             # updates everything and prints the delta
scripts/rexon-golden-update --list      # this same table, but from the run's registry
```

> ⚠️ The "with a recapture mechanism" table below is a human-edited copy of the REGISTRY inside
> `scripts/rexon-golden-update`. They have nowhere to diverge: `--list` prints the registry,
> and the "without a mechanism" section is **computed by the run itself** — a new file in
> `tests/golden/` lands there automatically and cannot
> pretend to be checked.

---

## 1. Three kinds of references — and why this is the main distinction

| Kind (`--kind`) | What it means | What a run without prerequisites must do |
|---|---|---|
| **code** | captured from the tree, reproducible on any machine | nothing — it needs no prerequisites |
| **live** | a system tool (`lrelease`) is needed | say **"NOT CHECKED"** and name the reason |
| **manual** | there is no recapture mechanism at all | list them by name with a reason |

⛔ **A silent skip is worse than no check.** On 06.09.2026 the comparison was red
twice over, and the second defect was not visible behind the first. That is why the summary prints all
three buckets ALWAYS, even when the first one is already red, and the exit codes distinguish
"diverged" (1) from "not checked" (3).

---

## 2. References with a recapture mechanism

| Reference | Kind | What updated it BEFORE | What updates it NOW |
|---|---|---|---|
| `docs/de_customization_matrix.md` | code | `REXON_UPDATE_DOC=1 cargo test -p rexon-core --test de_customization` — the command was known only from text inside the document itself | `scripts/rexon-golden-update de_matrix` |
| `data/qt_wallpaper_engine/qml_ui/waywallen_{ru,de,zh_CN,fr}.qm` (one for each vendored `.ts`) | **live** | the `lrelease …` line from `docs/testing.md`, by hand | `… waywallen_qm` |

### What left this table on 02.10.2026 and why

* `qt_main`, `qt_settings`, `qt_compare`, `qt_extensions` — Python generators
  `golden/*_cases.py` (they captured the frozen source of the tabs by
  blob hash via `golden/frozen_source.py`). The generators are in
  `_archived/golden/rexon-core/`, the `*_cases.json` files themselves are frozen. Change them
  only deliberately and with an explanation of WHAT changed in the behaviour.
* `bridge_refs` — references of the bridge comparisons `rexon-py/tests/fixtures/*_reference.json`.
  The bridge went into `_archived/rexon-py/` as a whole; the two references read by
  Rust comparisons were moved next to them:
  `rexon-core/tests/golden/audio_capture_reference.json` and
  `rexon-gui/tests/golden/engine_update_reference.json` (frozen).
* `we_live_frames` — frames of the live daemon were captured by the Python client
  (`rexon.qt_wallpaper_engine.client` + `tools/we_proto`). The snapshot
  `golden/we_live_frames.json` is frozen; it can be recaptured once
  a capturer in Rust exists.

---

## 3. References WITHOUT a recapture mechanism (edited by hand)

There are 104 of them (as of 09.10.2026). The full list with reasons is printed by `scripts/rexon-golden-update --list`;
here are the classes and why there is nothing to automate.

| How many | Class | Why it cannot be recaptured |
|---|---|---|
| 61 | `rexon-core/tests/golden/*_cases.json` | captured by a Python generator; there is no live Python copy, the generators are in the archive (`_archived/golden`), the reference is frozen |
| 24 | other `golden/*.json` | neither a generator nor a formula: the exact capture method is described only in the header of the comparison that reads it |
| 7 | `*_raw.json`, `*_raw.html`, `fixtures/aur/*` | recorded network responses (`aur.archlinux.org/rpc/v5`, Wikimedia imageinfo, Steam Web API) |
| 9 | `*.xml` in `golden/` and `fixtures/ocs/` | recorded OCS API responses (`api.opendesktop.org`) |
| 7 | `*.rxn` | handmade INPUT samples (`zipslip`, `truncated`, `dotdot_middle`, `absolute_member`, `manifest_cp1251`, `no_manifest`, `sample`) — not snapshots |
| 4 | `rexon-gui/tests/golden/*_cases.json` | **FROZEN**: the Python mirrors of the WE tab were deleted together with the recording half of the generators, there is nothing to capture from. Change them only deliberately and with an explanation |
| 1 | `golden/we_live_frames.json` | was captured from the live daemon by the Python client; the client is in the archive — the snapshot is frozen |
| 1 | `golden/audio_capture_reference.json` | a recorded reference of the live Python (formerly `rexon-py/tests/fixtures`), frozen |

### Separately: machine-dependent references

On another machine they would capture SOMETHING ELSE, so recapturing them is more dangerous than not
recapturing them: `gtk_sanitizer_cases.json`, `installed_scan_tree.json`,
`favorites_real_names.json`, `local_preview_cases.json` (51 schemes from
`/usr/share/color-schemes`), `scheme_look_cases.json` (the colours of those same schemes
and the machine's Plasma styles, captured by a program on `KColorScheme`), `utils_cases.json`.
The cautionary example is `qt_settings_parity.rs`: a snapshot of a live machine broke four
sections on a recapture in September 2026 without a single line of code changing; the cure is
a substitute tree inside the generator, and that is what it has now.

### Separately: an ORPHAN REFERENCE

A file sits in `golden/`, looks like a reference and **is not read by any comparison** —
when the vendor is updated, this creates a false sense of coverage. The run finds such files
itself and prints them in the summary. As of 16.10.2026 there are none.

Deleted: `rust_render_cases.json` — a leftover of the removed in-house renderer
(09.10.2026); `remote_l10n_vendor_frozen.json` — a frozen tree of
waywallen v0.2.6+9 for parity of building the `.ts` from our own dictionary with Python;
the dictionary and the comparison were removed on 18.08.2026 (54ebb148), when the labels of the vendored
pages started being translated by waywallen's own catalog (16.10.2026).

---

## 4. What to do when updating the waywallen vendor

None of the above by hand — **there is a single command**:

```
scripts/rexon-vendor-update v0.3.9           # accept a new tag
scripts/rexon-vendor-update --check v0.3.9   # only say what diverged
```

It runs nine steps in a row — versions (vendor/upstream), protocol, markup
(+`qmldir`, +our patch series), QML modules, model notifications, the `.qm` catalog,
`cargo build-app`, `cargo test-core` and `rexon-golden-update --check` — and prints
ONE report: what changed, what turned red, what failed to build, what was NOT
CHECKED. ⭐It must run to the end: a red step does not cancel the following ones,
otherwise the second breakage hides behind the first again.

⚠️ The last step COMPARES the references, it does not recapture them. Recapturing is a separate,
deliberate action following this map: `scripts/rexon-golden-update <group>`,
and it should be done only when you understand WHAT changed in the behaviour and why that is
correct.

What remains as manual steps (the order, if you run the steps one by one):

1. vendor `ui/qml` + `ui/i18n` (`scripts/sync-waywallen-*.py`);
2. `scripts/rexon-golden-update --check` — see AT ONCE the whole list of what
   diverged, including the daemon version and new kinds of events;
3. `scripts/rexon-golden-update` — update and read the delta for each file;
4. `cargo test-core` — accept the result.

Step 2 is the main saving within this map: previously divergences surfaced
one at a time, each in a separate "and what updates this one" pass.

---

## 5. The yardstick is the bite (checked 07.09.2026)

> History: both bites below were applied to groups that went into the
> archive together with Python on 02.10.2026 (`qt_main`, `we_live_frames`). The principle still holds for
> the remaining groups. The output is quoted with today's English wording of the script;
> back then it printed the same messages in Russian.

A run that never turns red checks nothing. So for one reference of
EACH kind, the reference was corrupted at a single point, and it was checked that the divergence is named
by name.

**Kind "code" — `golden/qt_main_cases.json`.** One literal was corrupted
(`window.title`: «рабочих столов» → «рабочих СТОЛОВ», i.e. "desktops" → "DESKTOPS").
`--check` → `DIVERGED rust/rexon-core/tests/golden/qt_main_cases.json`,
delta `fields: +0 −0 changed 1 / ~ window.title`, exit code **1**.
The update returned the file byte for byte to the original, `--check` → **0**.

**Kind "live" — `golden/we_live_frames.json`.** Three places were corrupted at once:
`_daemon_version` 0.3.8 → 0.3.7, the payload kind of `tag_list` was swapped for
`source_list`, an erroneous `autostart_get` was passed off as a success.
`--check` named **all three** separately plus a fourth, real
divergence (the daemon sends a `playlist_changed` event that is not in the snapshot),
exit code **1**. What matters is that the three bites did not hide behind each other — that is
exactly the requirement "the second defect is visible behind the first".

**The "NOT CHECKED" branch.** The same run with the session bus unavailable
(`DBUS_SESSION_BUS_ADDRESS` pointing to a nonexistent socket) does not stay silent and does not turn green:
`NOT CHECKED: the live waywallen daemon does not answer … To bring it up: systemctl --user
start rexon-wallpaper-engine`, exit code **3**.

Along the way, the very first real recapture of frames increased coverage: the snapshot
stopped holding `display_list` and `renderer_list` from version 0.3.5 (there they
had been captured while no wallpaper was shown) and got a sixth kind of push event —
`playlist_changed`. `cargo test-core` on the updated snapshot is green.
