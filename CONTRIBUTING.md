# Contributing to Rexon

Thank you for helping. Bug reports, fixes, translations and testing on desktops and
distributions other than Arch / CachyOS are all welcome.

## Layout

| Path | What it is |
|---|---|
| `rust/rexon-core` | The core, without Qt: desktop tables, package plans, parsers, store clients, settings, privileged helpers. Almost all decisions live here, and it is where most tests are. |
| `rust/rexon-gui` | The window: QML pages (`qml/`) and their Rust objects (cxx-qt), the wallpaper host. |
| `rust/rexon-qt-filter`, `rust/rexon-splash-overlay`, `rust/rexon-swf` | The wallpaper filter model, the login startup-movie player, the Flash player. |
| `rust/vendor/fxtranslate` | A vendored crate with our patch series (see its `ATTRIBUTION.md`). |
| `data/` | Product data: vendored waywallen QML, protocol schemas, shell extensions, `themes.json`, "Like a distribution" looks. |
| `po/` | Translations (gettext). |
| `packaging/` | `PKGBUILD`, AppStream metadata. |
| `tools/`, `scripts/` | Build scripts for the QML plugins, translation and vendor-update scripts. |

Keep logic in `rexon-core` and keep the QML pages thin: a decision made in the core
can be tested without Qt.

## Building

Install the dependencies listed in the [README](README.md#building-from-source), then,
from `rust/`:

```sh
cargo build-app          # core + window + QML libraries, release
cargo build-app-debug    # the same in debug (release is not rebuilt!)
cargo build-splash       # startup-movie player, a separate command on purpose
```

Plain `cargo build` builds only the core (`default-members = ["rexon-core"]`), so it
will not rebuild the window. The aliases only work when cargo runs from `rust/`; see
`rust/.cargo/config.toml` for what each one does and why.

After editing a `.qml` file, run `scripts/qmllint-check <file.qml>`.

## Tests

```sh
cd rust
cargo test-core            # all core tests
cargo test-core-isolated   # without checks that need a real machine
```

A few dozen checks test work with the real system: `msgfmt`, `bash`, `bsdtar`, a
live `PATH`, the git object database. `test-core-isolated` builds with the
`isolated-checks` feature, and those checks are reported as `ignored` with a reason
instead of failing. CI runs this variant.

Many core tests compare output with reference files ("golden" files). If you
intentionally change behaviour, update only the affected group with
`scripts/rexon-golden-update <group>`, and say so in the pull request;
`scripts/rexon-golden-update --list` shows the groups.

Please do not write tests that change your real desktop: use the fake I/O traits of
the core (`SystemIo` and friends) instead.

## Commits and pull requests

- One logical change per commit.
- Subject line: `Area: what changed`, for example
  `Desktops: refuse removing a desktop that shares packages with the current one`.
  Use the body to explain why.
- Commit messages in English or Russian are both fine.
- Code identifiers are always English words, never transliteration. Comments may
  be in English or Russian.
- Before opening a pull request: `cargo test-core-isolated` passes, the window
  builds with `cargo build-app`, and for interface strings
  `scripts/i18n-update.sh --check` passes.
- Changes to the privileged helpers (`rust/rexon-core/src/privilege/`,
  `system_plan.rs`, `installer.rs`) get extra review: describe what the helper is
  now allowed to do that it could not do before.

## Translations

Rexon uses GNU gettext with a single catalog for Rust and QML: the template is
`po/rexon.pot`, and each language is `po/<code>.po`. Russian is Rexon's original
text; please translate from `po/ru.po` when you can, and use the English `msgid` as
a hint.

To add a language:

```sh
scripts/i18n-update.sh --new pl     # creates po/pl.po and adds pl to po/LINGUAS
# translate po/pl.po in Poedit, Lokalize or a text editor
cd rust && cargo build-app
LANGUAGE=pl target/release/rexon-main-window
```

No code has to change. If you add or change interface strings in code, run
`scripts/i18n-update.sh` to update the template and all `.po` files. The full guide,
including rules for placeholders and plural forms, is in
[`po/README.md`](po/README.md).

## Reporting bugs

Please open an issue with the bug report form and include:

- distribution and version, desktop and session type (Wayland or X11), Rexon version
  or commit;
- what you did, what you expected, what happened instead;
- **the log**: `~/.local/state/rexon/rexon.log` (or `$XDG_STATE_HOME/rexon/rexon.log`;
  older entries are in `rexon.log.1` … `rexon.log.3`). To get a detailed log,
  start Rexon with `REXON_LOG=debug rexon`;
- if Rexon crashed: the report from `~/.config/rexon/crashes/`;
- if a system change failed (installing, removing, themes):
  `journalctl -t rexon-priv-helper -b`;
- if the login screen does not come up: the output of `rexon doctor` from a text
  console.

Check the log for anything you don't want to share (user names, paths) before you
attach it.

Security problems: please do not open a public issue; see [SECURITY.md](SECURITY.md).

## License and Contributor License Agreement

Rexon is licensed under the [Apache License 2.0](LICENSE), and your contribution is
licensed under it as well.

Before your first pull request can be merged, please accept the
[Contributor License Agreement](CLA.md). It is based on the Apache Individual CLA:
you keep the copyright in your work and grant the project a permanent license to use
it. This applies to code, translations, artwork and documentation alike. When you
open a pull request, a bot posts a comment with the sentence to reply with; you only
do this once, and it covers all your future contributions.
