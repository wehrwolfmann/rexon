# Translating Rexon

Rexon's interface is translated with GNU gettext. There is one catalog for the
whole program: every string you see — in Rust code and in QML pages — lives in
`po/rexon.pot`, and each language is one file, `po/<code>.po`.

The catalog keys (`msgid`) are English, but **Rexon's original text is Russian**
(`po/ru.po`). Translate from the Russian text, not from the English key: a
translation of a translation drifts, and Slavic languages in particular lose
their nuance when they go through English. Use the English `msgid` only as a
hint to the meaning. If the Russian and the English disagree, the Russian is
right — report it so the English gets fixed.

## Add a new language in 3 steps

1. **Create the file.** From the repository root:

   ```sh
   scripts/i18n-update.sh --new pl        # Polish; use pt_BR, cs, de, …
   ```

   This creates `po/pl.po` from the template and adds `pl` to `po/LINGUAS`.
   Use the plain language code (`pl`, `cs`) unless the language really differs
   by country — then use the country form (`pt_BR`). A user whose system is
   set to `pt_PT` falls back to `pt` if there is no `pt_PT.po`.

2. **Translate.** Open `po/pl.po` in any PO editor (Poedit, Lokalize, GTranslator)
   or a text editor and fill in every `msgstr`. Check that the `Plural-Forms`
   line in the header is right for your language (`--new` fills in the usual
   one).

3. **Check it.** Build and run in your language:

   ```sh
   cd rust && cargo build-app
   LANGUAGE=pl target/release/rexon-main-window
   ```

   `cargo build-app` compiles all catalogs from `po/LINGUAS` next to the
   binaries (`rust/target/release/locale/`), and the program picks them up
   from there. Send the `.po` file (and the `LINGUAS` line) back — nothing in
   the code has to change.

## Rules for translators

* **Placeholders stay as they are.** `%1`, `%2`, … are replaced with values
  (numbers, names); you may move them within the sentence, but every one of
  them must be present. `%n` is the number in plural strings.
* **Plural forms.** A string with `msgid_plural` has one `msgstr[N]` per
  plural form of your language, in the order defined by the `Plural-Forms`
  expression of your file.
* **Context.** Some strings carry a `msgctxt` line: the same English word
  used in two meanings ("Open" the button vs. "open" the state). Translate
  each one on its own.
* **Comments** starting with `#.` are notes from developers — read them, they
  say where the string is shown.
* **Fuzzy entries** (`#, fuzzy`) are guesses made by the tools after the
  English text changed. They are *not* used until you check them and remove
  the `fuzzy` flag.
* **Technical names stay in Latin script**: KDE, GNOME, Wallpaper Engine,
  Steam, Wayland and the like are names, not words.

## For developers

* Mark strings in Rust with the functions of `rexon_core::i18n`:
  `tr("Text")`, `trc("context", "Text")`, `trf("Step %1 of %2", &[&a, &b])`,
  `ntr("%n file", "%n files", n)`, and `n_("Text")` for tables and constants
  (translate them where they are shown: `tr(ITEM.title)`).
* In QML, use `qsTranslate("rexon", "Text")` — not `qsTr()`: the Wallpaper
  Engine pages loaded into the same process have their own contexts with the
  same page names. Plural strings go through Rust (`ntr`); a QML-only plural
  has no way to declare its English plural form for gettext.
* After changing strings run `scripts/i18n-update.sh`: it extracts the
  template and merges it into every `.po`. `scripts/i18n-update.sh --check`
  fails if the template is out of date.
* Put a `// TRANSLATORS: …` comment right above a string when its meaning is
  not obvious from the text alone; it ends up in the catalog for translators.

How it works: Rust reads the compiled `.mo` with its own reader (no libintl);
QML reads the very same `.mo` — Rexon installs a `QTranslator` that answers
`qsTranslate("rexon", …)` by asking the Rust catalog, so there is no separate
`.qm` and a translation lives in exactly one place. The language is the
user's setting (`ui_language` in `settings.json`), otherwise the system locale
(`LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`), otherwise English. Catalogs are
searched in `$REXON_LOCALEDIR`, then `locale/` next to the executable, then
`/usr/share/locale`, in the usual `<code>/LC_MESSAGES/rexon.mo` layout.
