//! Auto-translation of theme descriptions — port of the PURE logic from
//! `rexon/appearance/translator.py`.
//!
//! The DETERMINISTIC part is ported: mapping the user's locale to the target
//! language ([`user_lang_from_locale`]), the translation cache key `src_hash`
//! ([`translation_cache_key`] — SHA-1 of `version:src:tgt:text`), the HTML
//! markup check ([`looks_like_html`]), segmenting HTML into text nodes while
//! keeping tags/entities verbatim ([`segment_html`] — a byte-for-byte port of
//! `_HTMLTextTranslator`/`html.parser.HTMLParser` with
//! `convert_charrefs=False`), a cheap language heuristic
//! ([`detect_lang_heuristic`]), extracting visible text for detection
//! ([`probe_text`]) and the DECISION «translate or return the original»
//! ([`plan_translate_text`]).
//!
//! Side effects are NOT moved HERE and stay in Python: the `argostranslate`
//! call itself (translating a text segment), the SQLite translation cache
//! (`get_cache`/`get_translation`/`put_translation` in `appearance/cache.py`),
//! the optional `langdetect` (non-deterministic without seed=0, but the seed
//! only fixes its internal PRNG — we do not drag the engine itself here) and
//! model loading (`_ensure_pair`, network). The core hands out SEGMENTS, the
//! KEY and the DECISION; observations (whether argos is available, what the
//! language detector found, whether the src→tgt pair can be assembled) are
//! passed as parameters.
//!
//! ⚠️ BEYOND THE PORT (Python does not have this): the «prose or technical»
//! filter — [`is_prose_segment`], [`CodeEnvStack`], [`translatable_flags`] and
//! [`render_segments_filtered`]. Python called the engine on every text node,
//! and a measurement on live workshop descriptions showed that this way any
//! engine breaks technical text (`Gnome` → «Гном», `sudo apt-get install …` →
//! «sudo apt-get установить …»). The filter decides BEFORE the call what to
//! give to the engine; the check is `translator_prose.rs`, it does not affect
//! parity behaviour. A technology name INSIDE a translatable phrase cannot be
//! saved by the filter (`Sweet Kvantum theme` is prose), so a substitution
//! lives next to it: [`mask_tech_names`] moves the name under the placeholder
//! `@0@`, [`unmask_tech_names`] returns it verbatim, and if the placeholders do
//! not match, the segment's translation is not taken at all. The same trick
//! hides names that cannot be in our list — [`mask_names`] takes them from the
//! caller (the name of the open item, the author's name).
//!
//! ⚠️ BEYOND THAT (acceptance 24.09): the «prose or technical» decision can be
//! computed BY SENTENCE ([`translatable_flags_by_sentence`]) — a node where
//! prose stands next to a link is no longer rejected entirely — and next to
//! the placeholder sieves a third one appeared: [`invented_latin_word`] catches
//! a Latin word that was not in the source («tooltips» → «toolbep»), i.e.
//! corruption without any names.
//!
//! Comparison with Python — golden `translator_parity.rs`: the reference was
//! taken from live `translator.py` (segmentation via `translate_html` with a
//! monkeypatched deterministic `translate`, `src_hash` from `hashlib.sha1`,
//! `_user_lang`, `_looks_like_html`, `detect_lang` without langdetect,
//! `translate_text` with monkeypatched observations).
//!
//! FACTS of Python behaviour reproduced verbatim (these are NOT bugs — the
//! standard normalization of `html.parser`, reproduced for parity, see the
//! report):
//!   * closing tags are lowercased (`</DIV>` → `</div>`), whereas the opening
//!     tag is returned VERBATIM (`get_starttag_text`);
//!   * charrefs/entityrefs without the terminating `;` get a `;` appended
//!     (`&#169 ` → `&#169;` + space);
//!   * declaration nodes the `_HTMLTextTranslator` subclass does NOT handle
//!     (`<!DOCTYPE …>`, `<?pi?>`, `<![CDATA[…]]>`) DROP OUT of the output — as
//!     in the original (it does not override those handlers). Such nodes do
//!     not occur in OCS/theme descriptions.

use serde::Serialize;

/// Version of the translation logic — mixed into the `src_hash` cache key. A
/// bump by 1 invalidates all old records (port of `TRANSLATOR_VERSION`).
///
/// `4` — ENGINE CHANGE (argos → fxtranslate). The bump here is not cosmetic:
/// texts translated by the previous engine lie in the `translations` table
/// under version `3` keys, and reading them as our own is NOT allowed — the
/// quality differs. The new version separates the keys, old records are
/// simply not found and leave by TTL ([`crate::appearance_cache::TRANSLATION_TTL`]).
///
/// `5` — TECHNICAL TEXT INSIDE PROSE (03.10): a phrase with a path, a link or
/// a file name is no longer rejected entirely but translated with the
/// technical part under a placeholder. Without the bump, a segment half
/// translated by the previous rule (an English phrase with a path in the
/// middle of a Russian paragraph) would live in the cache until the TTL ended.
///
/// `6` — TERM FIX DICTIONARY (03.10, [`TERM_FIXES`]): «расстегните файл» →
/// «распакуйте файл», «скачать скачать» → «скачать». Without the bump the old
/// «расстегните» would live in the cache until the TTL ended.
///
/// `7` — THE DESCRIPTION OF THEME 2380824 (06.11, KDE Store «Windows XP
/// Professional - Luna Blue and Classic Suite»): guarding names from the
/// core's tables (programs, desktops, distributions), picture characters and
/// phrase edges bypass the engine, a second pass hides only lost names,
/// invented repeats («и и») and the fixes «проводник»/«вход в
/// систему»/«системный лоток». Without the bump «Базцитовый Linux» and
/// «разногласия» would live in the cache until the TTL ended.
///
/// `8` — THE «Candy icons» DESCRIPTION (08.11, KDE Store): a name link is
/// glued to the phrase and not translated ([`fold_name_links`]), the
/// dictionary of stable terms — «тема значков», «тема курсоров»,
/// «Примечание», «заявка», «запрос на слияние», «виджет Plasma». Without the
/// bump «моему Сладкий тема темы» would live in the cache until the TTL ended.
pub const TRANSLATOR_VERSION: u32 = 8;

// ── src_hash (translation cache key) ───────────────────────────────────────

/// Translation cache key — `sha1(f"{VERSION}:{src}:{tgt}:{text}").hexdigest()`.
/// A bit-exact port of computing `h` in `translate()`/the `translate_html` tail.
pub fn translation_cache_key(src: &str, tgt: &str, text: &str) -> String {
    translation_cache_key_versioned(TRANSLATOR_VERSION, src, tgt, text)
}

/// The same key, but with an EXPLICIT engine version.
///
/// ⚠️ Needed by the check: the golden reference was taken at version `3`, and
/// it must check the MECHANISM (field order, encoding, SHA-1), not the
/// current value of the constant — otherwise any version bump would require
/// retaking the reference. Live code calls [`translation_cache_key`], which
/// fills in [`TRANSLATOR_VERSION`] itself.
pub fn translation_cache_key_versioned(version: u32, src: &str, tgt: &str, text: &str) -> String {
    let payload = format!("{version}:{src}:{tgt}:{text}");
    sha1_hex(payload.as_bytes())
}

/// SHA-1 → a hex string (lowercase), like `hashlib.sha1(...).hexdigest()`.
///
/// Visible inside the crate: `ocs::preview_cache_name` names preview cache
/// files with the same hash — there must not be a second SHA-1
/// implementation in the core.
pub(crate) fn sha1_hex(msg: &[u8]) -> String {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let ml = (msg.len() as u64).wrapping_mul(8);

    // Padding: 0x80, then 0x00 up to a length ≡ 56 (mod 64), then 8 bytes of BE length.
    let mut data = msg.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&ml.to_be_bytes());

    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let b = i * 4;
            *word = u32::from_be_bytes([chunk[b], chunk[b + 1], chunk[b + 2], chunk[b + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = String::with_capacity(40);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

// ── interface language (target) ────────────────────────────────────────────

/// Target language from a locale string — port of
/// `(_loc.split("_")[0] or "en").lower()` from `_user_lang`. Getting the
/// locale itself (`locale.getlocale`) is a side effect and stays in Python; a
/// ready string (possibly empty) comes here.
pub fn user_lang_from_locale(locale_str: &str) -> String {
    let head = locale_str.split('_').next().unwrap_or("");
    let base = if head.is_empty() { "en" } else { head };
    base.to_lowercase()
}

/// The language theme descriptions and comments are translated INTO — the
/// Rexon interface language (`i18n::current_language`: the person's setting,
/// otherwise the system locale, otherwise English).
///
/// The interface one, not the system locale: if a person chose English Rexon
/// on a Russian system, descriptions next to English buttons must be English
/// too, not Russian. Returns a code like `ru` or `pt_BR`;
/// [`user_lang_from_locale`] reduces it to the model language.
pub fn interface_locale() -> String {
    crate::i18n::current_language()
}

/// The customer's locale string from the environment — in the order the
/// locale system itself reads it: `LC_ALL`, `LC_MESSAGES`, `LANG`. Empty — no
/// locale.
///
/// ⚠️ NOT suitable for translating descriptions — that uses the interface
/// language ([`interface_locale`]); this is the system language.
///
/// The very `locale.getlocale` that in Python stayed a side effect of the
/// caller. Lives here, next to the parsing: a loop over these three variables
/// already lay as a copy in [`crate::distro_compat::Lang::from_env`], and a
/// third copy (description translation) would make divergence a matter of
/// time (rule #12).
pub fn locale_from_env() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    String::new()
}

// ── HTML check and probe text ──────────────────────────────────────────────

// `_HTML_TAG_RE = <[a-zA-Z/!][^>]*>|&[a-zA-Z#0-9]+;`.
// Returns the first occurrence (start, end) in a slice of chars, starting at `from`.
fn html_tag_find(data: &[char], from: usize) -> Option<(usize, usize)> {
    let n = data.len();
    let mut p = from;
    while p < n {
        let c = data[p];
        if c == '<' && p + 1 < n && (data[p + 1].is_ascii_alphabetic() || matches!(data[p + 1], '/' | '!')) {
            // <[a-zA-Z/!][^>]*>
            let mut q = p + 2;
            while q < n && data[q] != '>' {
                q += 1;
            }
            if q < n {
                return Some((p, q + 1));
            }
            // no closing '>' — this '<' is not a match, keep searching.
        } else if c == '&' {
            // &[a-zA-Z#0-9]+;
            let mut q = p + 1;
            while q < n && (data[q].is_ascii_alphanumeric() || data[q] == '#') {
                q += 1;
            }
            if q > p + 1 && q < n && data[q] == ';' {
                return Some((p, q + 1));
            }
        }
        p += 1;
    }
    None
}

/// Whether the text has HTML markup — port of `_looks_like_html`.
pub fn looks_like_html(text: &str) -> bool {
    let data: Vec<char> = text.chars().collect();
    html_tag_find(&data, 0).is_some()
}

/// Visible text for language detection — port of computing `probe`:
/// `re.sub(r"\s+", " ", html.unescape(_HTML_TAG_RE.sub(" ", text))).strip()`.
///
/// ⚠️ `html.unescape` is implemented fully for numeric references and for
/// COMMON named entities; since `_HTML_TAG_RE` already cut all valid `&…;`
/// into a space BEFORE unescape, only leftovers reach it (usually bare `&`).
/// The result is used ONLY as input of the language detector, it does not
/// affect the cache key or the kept markup.
pub fn probe_text(text: &str) -> String {
    let data: Vec<char> = text.chars().collect();
    // _HTML_TAG_RE.sub(" ", text): every occurrence → one space.
    let mut stripped_tags = String::new();
    let mut i = 0;
    while i < data.len() {
        if let Some((s, e)) = html_tag_find(&data, i) {
            if s == i {
                stripped_tags.push(' ');
                i = e;
                continue;
            }
            // up to the next match — as is.
            for &c in &data[i..s] {
                stripped_tags.push(c);
            }
            stripped_tags.push(' ');
            i = e;
        } else {
            for &c in &data[i..] {
                stripped_tags.push(c);
            }
            break;
        }
    }
    let unescaped = html_unescape(&stripped_tags);
    collapse_ws(&unescaped)
}

// `re.sub(r"\s+", " ", s).strip()` — collapse whitespace runs into one
// space, trim the edges. `\s` in Python = [ \t\n\r\f\v] (+ Unicode with
// re.UNICODE, the default for str). We use char::is_whitespace (Unicode WS).
fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws && !out.is_empty() {
                out.push(' ');
            }
            in_ws = false;
            out.push(c);
        }
    }
    out
}

// ── language detection (heuristic) ─────────────────────────────────────────

/// A cheap language heuristic by script — port of the `detect_lang` branch
/// WITHOUT langdetect (the lines with the emptiness check and counting
/// Cyrillic/Latin). When langdetect is installed, Python takes its result —
/// that is a non-deterministic side effect not ported into the core; the
/// caller passes the detection result as a parameter to [`plan_translate_text`].
pub fn detect_lang_heuristic(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    // _CYRILLIC = [Ѐ-ӿ] (U+0400..U+04FF); _LATIN = [A-Za-z].
    let cyr = text.chars().filter(|&c| ('\u{0400}'..='\u{04ff}').contains(&c)).count();
    let lat = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if cyr > 0 && cyr >= lat {
        return Some("ru".to_string());
    }
    if lat > 0 {
        return Some("en".to_string());
    }
    None
}

// ── the «translate or not» decision ────────────────────────────────────────

/// Text translation plan — the result of [`plan_translate_text`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TranslatePlan {
    /// Do not translate — return the original, `was_translated=false` (empty/spaces,
    /// already in the target language, argos unavailable).
    Skip,
    /// Translate: the source language and the path (HTML by segments or plain text).
    Translate { src: String, tgt: String, is_html: bool },
}

/// The `translate_text` decision BEFORE the actual translation — port of the
/// control logic (the lines «empty→original», «already in target→original»,
/// «no argos→original», choosing src with a fallback to `en`, choosing the
/// HTML/plain path). The translation itself and the final comparison
/// `result.strip() != text.strip()` (which determines `was_translated`) are
/// performed by the caller — that is a side effect.
///
/// Observations (parameters):
///   * `target` — the already computed target language (`target or _user_lang()`);
///   * `detected_src` — the result of `detect_lang(probe)` (None or a language code);
///   * `engine_available` — whether the translation engine is available
///     (`_argos_available()` in the previous engine; the parameter name is
///     neutral — the engine changes);
///   * `src_pair_ensurable` — `_ensure_pair(src, tgt)` for the src CANDIDATE
///     (`detected_src` or `"en"`), i.e. whether the translation path can be
///     assembled.
pub fn plan_translate_text(
    text: &str,
    target: &str,
    detected_src: Option<&str>,
    engine_available: bool,
    src_pair_ensurable: bool,
) -> TranslatePlan {
    // if not text or not text.strip(): return text, False
    if text.trim().is_empty() {
        return TranslatePlan::Skip;
    }
    // src = detect_lang(probe); if src and src == tgt: return text, False
    // (an empty string in Python is «falsy» → does not trigger).
    let src_detected = detected_src.filter(|s| !s.is_empty());
    if let Some(s) = src_detected {
        if s == target {
            return TranslatePlan::Skip;
        }
    }
    // if not _argos_available(): return text, False
    if !engine_available {
        return TranslatePlan::Skip;
    }
    // src = src or "en"; if not _ensure_pair(src, tgt): src = "en"
    let mut src = src_detected.unwrap_or("en").to_string();
    if !src_pair_ensurable {
        src = "en".to_string();
    }
    let is_html = looks_like_html(text);
    TranslatePlan::Translate { src, tgt: target.to_string(), is_html }
}

// ── HTML segmentation ──────────────────────────────────────────────────────

/// A segment of parsed HTML — port of the `_HTMLTextTranslator` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    /// A verbatim fragment (tag/entity/comment/whitespace node) — NOT
    /// translated, inserted as is.
    Literal { text: String },
    /// A translatable text node: leading/trailing spaces are moved outside
    /// (argos «eats» them), only `stripped` is translated. Reassembly:
    /// `lead + (translate(stripped) || stripped) + trail`.
    Text { lead: String, stripped: String, trail: String },
}

/// Split HTML into segments (port of `_HTMLTextTranslator.feed`+`close`).
/// Translatable text goes into [`Segment::Text`], everything else into
/// [`Segment::Literal`] verbatim. Translates nothing: translating text nodes
/// is the caller's business (argos, side effect). For assembling the string
/// see [`render_segments`].
pub fn segment_html(text: &str) -> Vec<Segment> {
    let mut seg = Segmenter::new(text);
    seg.goahead(false);
    seg.goahead(true);
    seg.out
}

/// Assemble a string from segments, translating text nodes with the
/// `translate` callback. Port of the `"".join(p.out)` reassembly together
/// with `_translate_segment`: `f"{lead}{res or stripped}{trail}"` (an empty
/// translation → the original is taken).
pub fn render_segments(segments: &[Segment], translate: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    for s in segments {
        match s {
            Segment::Literal { text } => out.push_str(text),
            Segment::Text { lead, stripped, trail } => {
                let res = translate(stripped);
                out.push_str(lead);
                if res.is_empty() {
                    out.push_str(stripped);
                } else {
                    out.push_str(&res);
                }
                out.push_str(trail);
            }
        }
    }
    out
}

// ── the «prose or technical» filter ────────────────────────────────────────
//
// ⚠️ PYTHON DOES NOT HAVE THIS — new behaviour, not parity. Python called
// the engine on EVERY text node, and a measurement on live workshop
// descriptions showed how this ends for ANY engine: `Gnome` → «Гном»,
// `Unity` → «Единство», `Cinnamon` → «Корица», `sudo apt-get install
// gtk2-engines-murrine` → «sudo apt-get установить gtk2-двигатели-муррин»
// (examples verbatim from `mt-bench/samples.md`, sample 10 «Qogir theme»).
// House rule: names of technologies and standards stay in Latin script, we
// translate only human prose.
//
// The filter is PURE functions and does NOT touch
// [`segment_html`]/[`render_segments`] (they are byte-compared with Python,
// the `translator_parity.rs` check). The decision «call the engine or not» is
// made BEFORE the call: a segment recognized as technical simply does not
// reach the engine, and an empty answer of the [`render_segments`] callback
// means «keep the original» — so refusing to translate is already expressed
// in the existing interface, nothing had to be added.
//
// ⚠️ This decides the fate of a WHOLE segment, not of words inside it:
// «Sweet Kvantum theme» is prose for the filter (three words, not all of
// them product names). A name INSIDE a phrase is protected by substitution —
// [`mask_tech_names`] and [`unmask_tech_names`] below, from the same name list.

/// Command line heads: a segment starting with such a word is a command, not
/// prose. `yum` is in the list not for show: exactly `yum install
/// gtk-murrine-engine gtk2-engines` stands in the live Qogir description.
const COMMAND_HEADS: &[&str] = &[
    "sudo", "apt", "apt-get", "pacman", "yay", "paru", "dnf", "yum", "zypper", "flatpak", "snap",
    "git", "cd", "chmod", "chown", "systemctl", "makepkg", "cmake", "make",
    // Heads of file and install commands in look descriptions (`cp -r Foo
    // ~/.themes`, `tar -xf …`, `kpackagetool6 -i …`). While rule 4
    // rejected any line with a path, they were caught by it; now a path
    // inside prose is translated (see [`technical_spans`]), and a command
    // line must be recognized by its head by itself.
    "cp", "mv", "rm", "ln", "mkdir", "tar", "wget", "curl", "sh", "bash", "kpackagetool5",
    "kpackagetool6", "plasmapkg2", "lookandfeeltool", "gsettings", "dconf", "kwriteconfig5",
    "kwriteconfig6", "xdg-open", "sed", "meson", "ninja", "pip", "npm", "cargo", "dpkg", "rpm",
    "emerge", "xbps-install", "nix-env", "eopkg",
];

/// Names of technologies and products — ONE list for two jobs, and that is not
/// convenience but a condition of correctness: [`is_product_name_token`]
/// decides the fate of a WHOLE segment (`Gnome`, `2 Unity`, `6 Cinnamon`,
/// `9 Kde Plasma` — line labels from live descriptions, any engine turns them
/// into «Гном», «Единство», «Корицу»), while [`mask_tech_names`] hides the
/// same name INSIDE a phrase. If these two lists drifted apart, «Sweet
/// Kvantum theme» would be protected differently from «9 Kde Plasma»
/// (rule #12).
///
/// ⚠️ The spelling is CANONICAL (`GNOME`, not `gnome`): the case of a word
/// found in the text is checked against it ([`name_casing_matches`]). For the
/// segment filter the case still does not matter.
const PRODUCT_NAMES: &[&str] = &[
    "GNOME", "KDE", "Plasma", "Qt", "GTK", "Kvantum", "Papirus", "Dolphin", "Konsole", "Latte",
    "SDDM", "Plymouth", "XFCE", "Cinnamon", "Mate", "Unity",
    // The rest of the live «Supported desktops» list from the Qogir
    // description: any engine turns `3 Budgie` into «3 Баджи», and
    // `8 Lxde`/`4 Pantheon` of the same class are line labels, not prose.
    "Budgie", "LXDE", "Pantheon",
    // Names that occur only INSIDE a phrase of live descriptions — they
    // change nothing for the segment filter (there is always prose next to
    // them), but they must be hidden from the engine: «Firefox theme»,
    // «Nautilus headerbar», «Nordic Aurorae theme», «GTK2 engine Murrine
    // 0.98.1.1 or later».
    "Aurorae", "Firefox", "Nautilus", "Murrine",
    // The Nord palette: the «Nordic KDE» item description has «the awesome
    // Nord color palette», and the engine makes «Норд цветовая палитра» of it.
    "Nord",
    // Platforms and programs the live model translates as words (measured
    // 06.11, KDE Store, «Windows XP Professional - Luna Blue and Classic
    // Suite»): «Discord, Steam» → «разногласия, Steam». The core keeps no
    // program list anywhere else, so it is here, next to the other names.
    // The case is strict ([`name_casing_matches`]): «open windows» and «a
    // steam cloud» are still translated.
    "Windows", "Linux", "Steam", "Discord", "GitHub", "GitLab", "Wayland", "Proton", "Telegram",
    "Spotify", "Chromium", "Thunderbird", "LibreOffice", "macOS", "Android",
];

/// Desktop names from the core's table [`crate::screenshots::DE_DISPLAY_NAMES`]
/// that are at the same time ordinary English words: we do not take them as
/// a name («Mint green theme», «River of colors»). The other names of this
/// table (`Deepin`, `Hyprland`, `LXQt`, `Wayfire`, `COSMIC`…) are guarded on
/// a par with [`PRODUCT_NAMES`], and a new desktop in the table gets guarded
/// by itself.
const DE_NAME_WORDS_THAT_ARE_PLAIN_WORDS: &[&str] = &["Mint", "River"];

/// Distribution identifiers from [`crate::system::ID_MAP`] that are ordinary
/// English words (`arch`, `pop`, `crystal`, `void`…): «a crystal clear theme»
/// must be translated. The others (`bazzite`, `cachyos`, `fedora`,
/// `nobara`…) are names in ANY case: they are never words or part of prose,
/// and the live model turns «Bazzite Linux» into «Базцитовый Linux»
/// (measured 06.11).
const DISTRO_IDS_THAT_ARE_PLAIN_WORDS: &[&str] = &[
    "arch", "crystal", "pop", "elementary", "parrot", "rocky", "tuxedo", "peppermint", "void",
    "alpine", "clear", "simply", "boss", "calculate", "rosa", "astra",
];

/// The word is a desktop name from the core's table (strict case).
fn is_de_display_word(word: &str) -> bool {
    crate::screenshots::DE_DISPLAY_NAMES.iter().any(|(_, display)| {
        display.split_whitespace().any(|w| {
            w.chars().count() >= MIN_GUARDED_NAME_CHARS
                && !DE_NAME_WORDS_THAT_ARE_PLAIN_WORDS.contains(&w)
                && name_casing_matches(w, word)
        })
    })
}

/// The word is a distribution identifier from the core's table (any case).
fn is_distro_id_word(word: &str) -> bool {
    word.chars().count() >= 4
        && crate::system::ID_MAP.iter().any(|(id, _)| {
            !id.contains('-')
                && !DISTRO_IDS_THAT_ARE_PLAIN_WORDS.contains(id)
                && id.eq_ignore_ascii_case(word)
        })
}

/// Descriptive words of an ITEM NAME: such a word alone does not identify the name.
///
/// ⭐ WHY. An item name is recognized not only by the whole word chain but
/// also by separate meaningful words (see [`guarded_name_variants`]) —
/// otherwise the «Nordic KDE» item would not protect its own `Nordic Plasma
/// theme`. But taking ANY word of the name is not allowed: from «Sweet Mars
/// LAF Plasma 6» the word `laf` would guard every `LAF` in prose, and from
/// «Dark Glass Theme» — the word `theme`, and then the whole prose of the
/// description would end up under a placeholder.
///
/// The list lives next to [`PRODUCT_NAMES`] for exactly the same reason: one
/// place for all decisions about names (rule #12).
const NAME_FILLER_WORDS: &[&str] = &[
    "theme", "themes", "icons", "icon", "color", "colors", "colour", "colours", "scheme",
    "schemes", "global", "laf", "pack", "edition", "for", "and", "the", "with", "dark", "light",
];

/// «Code environment» tags: NOTHING inside them is translated, even if the
/// text looks like prose — the author explicitly said it is code.
const CODE_ENV_TAGS: &[&str] = &["code", "pre", "kbd", "samp", "tt"];

/// File extensions: a token ending in one of these is a path, not a word.
///
/// Archives and theme files (`*.tar.gz`, `index.theme`, `Nordic.colors`,
/// `Sweet.kvconfig`) were added 03.10 together with masking technical text
/// inside prose: such a file name no longer rejects the phrase but goes under
/// a placeholder and comes back verbatim.
const FILE_EXTENSIONS: &[&str] = &[
    ".sh", ".conf", ".desktop", ".py", ".qml", ".json", ".gz", ".xz", ".zst", ".bz2", ".zip",
    ".7z", ".tar", ".theme", ".colors", ".kvconfig", ".svg", ".png", ".jpg", ".ini", ".txt",
    ".css", ".xml", ".md",
];

/// Trim everything that is neither a letter nor a digit at the edges
/// (`«Features:»` → `Features`, `(Elementary` → `Elementary`). `+`/`#` go
/// too, so `GTK+` is recognized as the product name `gtk`.
fn trim_token_punct(tok: &str) -> &str {
    tok.trim_matches(|c: char| !c.is_alphanumeric())
}

/// A version or a screen resolution: `1.2.3`, `5.8`, `0.98.1.1`, `v0.1`,
/// `3840x2160`. Such a token is not a word: it does not hinder neighbouring
/// prose (`Murrine 0.98.1.1 or later` is translated), but is not prose by itself.
pub fn is_version_or_resolution_token(tok: &str) -> bool {
    let t = trim_token_punct(tok);
    if t.is_empty() {
        return false;
    }
    // Resolution: <digits>x<digits> (or `×`).
    let mut split = t.splitn(2, ['x', 'X', '×']);
    if let (Some(a), Some(b)) = (split.next(), split.next()) {
        if !a.is_empty()
            && !b.is_empty()
            && a.chars().all(|c| c.is_ascii_digit())
            && b.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    // Version: an optional `v`, then at least two numeric parts.
    let body = t.strip_prefix('v').or_else(|| t.strip_prefix('V')).unwrap_or(t);
    let parts: Vec<&str> = body.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// A «technical by form» token: it has `/`, `\`, `::`, `--`, `~/`, `$`, `@`
/// or it ends with a file extension.
///
/// ⚠️ Checked only on MEANINGFUL tokens (those with a letter) — on purpose.
/// `-->` in the live phrase «browsing the related collection --> Aritim-Light
/// Wallpapers Collection» (sample 12) contains `--`, but it is an arrow in
/// prose, not a command line switch; a switch is always attached to letters
/// (`--user`).
pub fn is_technical_token(tok: &str) -> bool {
    if tok.contains('/')
        || tok.contains('\\')
        || tok.contains("::")
        || tok.contains("--")
        || tok.contains('$')
        || tok.contains('@')
    {
        return true;
    }
    let lower = tok.trim_end_matches(|c: char| ",;:!?)\"'»".contains(c)).to_lowercase();
    let base = lower.trim_end_matches('.');
    FILE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext) || base.ends_with(ext))
}

/// A whole product name (case-insensitive, punctuation trimmed at the edges).
///
/// Case does not matter here on purpose: the fate of a WHOLE segment is
/// decided, and the line label `9 kde plasma` is the same label as
/// `9 Kde Plasma`. Case matters only where a name is extracted FROM a phrase
/// ([`name_casing_matches`]).
pub fn is_product_name_token(tok: &str) -> bool {
    let t = trim_token_punct(tok);
    !t.is_empty()
        && (PRODUCT_NAMES.iter().any(|n| n.eq_ignore_ascii_case(t))
            || is_de_display_word(t)
            || is_distro_id_word(t))
}

/// A «human word»: letters only (after trimming edge punctuation) and at least
/// three of them. Needed for description HEADINGS — `Features:`,
/// `Installation:`, `Note:`, `Folders`, `WALLPAPERS` are one-word prose, and
/// cutting them off with the «only one token» rule is not allowed.
fn is_plain_word(tok: &str) -> bool {
    let t = trim_token_punct(tok);
    t.chars().count() >= 3 && t.chars().all(|c| c.is_alphabetic())
}

/// A command line switch: `-r`, `-S`, `--user` (one or two hyphens, then a
/// letter or digit). A lone dash and the arrow `-->` do not count as a switch.
///
/// The non-breaking `‑` (U+2011) and its kin count as a hyphen too: the live
/// description «Sugar Candy for SDDM» writes the command `sddm‑greeter
/// ‑‑test‑mode` with exactly that.
fn is_command_flag(tok: &str) -> bool {
    let is_dash = |c: char| matches!(c, '-' | '\u{2010}' | '\u{2011}' | '\u{2212}');
    let body = tok.trim_start_matches(is_dash);
    let dashes = tok[..tok.len() - body.len()].chars().count();
    (1..=2).contains(&dashes) && body.chars().next().is_some_and(char::is_alphanumeric)
}

/// A command line INSIDE a phrase: a lowercase command head followed by a
/// switch (`Copy: cp -r bigsur ~/.local/share/sounds/`, `Use the following
/// command: kpackagetool6 -i Alba.P6.tar.xz` — both lines are live, KDE
/// Store). Such a phrase is not given to the engine whole: showing it
/// `cp -r bigsur` without a placeholder means risking a «бигсур».
fn has_inline_command(tokens: &[&str]) -> bool {
    tokens.windows(2).any(|pair| {
        let head = pair[0].trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        COMMAND_HEADS.contains(&head) && is_command_flag(pair[1])
    })
}

/// A pair of ordinary words through a slash — `and/or`, `light/dark`,
/// `on/off`. This is prose, not a path: both parts are lowercase letters,
/// without dots or digits.
/// ⚠️ Parts with a capital (`Fedora/RedHat`, `Plasma/KDE`) do NOT get here —
/// these are names, and they belong under a placeholder, not in the engine's
/// hands.
fn is_plain_slash_pair(core: &str) -> bool {
    let parts: Vec<&str> = core.split('/').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphabetic() && c.is_lowercase()))
}

/// Technical pieces INSIDE a line — what must not be shown to the engine, but
/// is no reason to reject the whole phrase either: (start, end) in bytes, in
/// order, without overlaps.
///
/// ⭐ WHY (owner's defect 03.10, KDE Store, «ZorinOS 6 Sound Theme»): «Just
/// unzip the file and put in the /usr/share/sounds folder to add the to your
/// desktop.» stayed entirely English, because the former rule 4 of
/// [`is_prose_segment`] rejected the line for ONE path in it. Now the path
/// goes under a placeholder by the same trick as names ([`mask_names`]), the
/// rest is translated, and the path comes back verbatim.
///
/// What counts as a technical piece:
///   * everything between a pair of backticks together with them
///     (`` `make install` ``);
///   * a Markdown link whose text is a name ([`markdown_name_links`]);
///   * a token between spaces that is technical by form
///     ([`is_technical_token`]: a path, a link, `$HOME`, `--user`,
///     `index.theme`, `*.tar.gz`). The phrase's edge punctuation («(», «.»,
///     «,», «:», quotes) is NOT part of the piece: «… /usr/share/sounds.»
///     hides the path without the dot;
///   * the token must have a letter (the arrow `-->` is not technical) and
///     must not have `@` (an email rejects the line entirely, see
///     [`NAME_PLACEHOLDER_MARK`]);
///   * a pair of ordinary words through a slash (`and/or`) is prose, not a piece.
pub fn technical_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    // Backticks — in pairs.
    let ticks: Vec<usize> = text.match_indices('`').map(|(i, _)| i).collect();
    for pair in ticks.chunks_exact(2) {
        spans.push((pair[0], pair[1] + 1));
    }
    // A Markdown link with name text (`[Sweet](https://…/p/1253385/)`) —
    // whole, together with the text: a neighbouring item's name is not translated.
    for (a, b) in markdown_name_links(text) {
        if !spans.iter().any(|(s, e)| a < *e && b > *s) {
            spans.push((a, b));
        }
    }
    let in_ticks = |s: usize, e: usize| spans.iter().any(|(a, b)| s < *b && e > *a);
    let mut tokens: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                tokens.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push((s, text.len()));
    }
    let mut found: Vec<(usize, usize)> = Vec::new();
    for (s, e) in tokens {
        if in_ticks(s, e) {
            continue;
        }
        let tok = &text[s..e];
        let lead = tok.len() - tok.trim_start_matches(|c: char| "([{\"'«“‘<".contains(c)).len();
        let core_end = tok.trim_end_matches(|c: char| ".,;:!?)]}\"'»”’>".contains(c)).len();
        if core_end <= lead {
            continue;
        }
        let core = &tok[lead..core_end];
        if core.contains(NAME_PLACEHOLDER_MARK)
            || !core.chars().any(char::is_alphabetic)
            || is_version_or_resolution_token(core)
            || is_plain_slash_pair(core)
            || !is_technical_token(core)
        {
            continue;
        }
        found.push((s + lead, s + core_end));
    }
    spans.extend(found);
    spans.sort();
    spans
}

/// Whether a segment is prose, i.e. whether to call the translation engine on it.
///
/// The order of rules matters, so it is pinned by tests
/// (`translator_prose.rs`, lines from live workshop descriptions):
///  1. not a single letter (`____`, `-----`, `1.2.3`) — not prose;
///  2. the segment starts with a LOWERCASE command head and is shorter than
///     nine tokens (`sudo apt-get install …`, `pacman -S …`) — not prose.
///     Case and length are not nitpicking here but protection of prose: «Make
///     sure to …» starts with a capital `Make` and is not a command;
///  3. no meaningful words left (there were only numbers/punctuation) — not prose;
///  4. technical text INSIDE the line ([`technical_spans`]: a path, a link, a
///     file name, a variable, a switch, a backtick piece). Before 03.10 any such
///     token rejected the line entirely, and «Just unzip the file and put in
///     the /usr/share/sounds folder…» stayed English (owner's defect, KDE
///     Store, «ZorinOS 6 Sound Theme»). Now the technical part goes under a
///     placeholder ([`mask_names`]) and comes back verbatim, and the fate of
///     the line is decided by what is LEFT:
///     * `@` in a meaningful word (email) — not prose, as before: the check
///       for a lost placeholder relies on `@`;
///     * the line STARTS with technical text (`~/.config/Kvantum`,
///       `./install.sh`, `` `make install` ``) — not prose; except a list of
///       names through a slash (`Fedora/RedHat distros:`);
///     * the first word is lowercase and a switch is next to it
///       (`kvantummanager --set Nordic`, `cp -r Foo ~/.themes`), or a command
///       head with a switch stands inside (`Copy: cp -r bigsur
///       ~/.local/share/sounds/`) — a command line, not prose;
///     * otherwise the technical part is cut out, and the rest is checked by
///       rules 1-8 again («Copy the folder to and pick it in Settings.» — prose);
///  5. ALL meaningful words are product names (`Gnome`, `2 Unity`, `9 Kde
///     Plasma`) — not prose;
///  6. one meaningful word: prose only if it is a human word (the point about
///     headings above);
///  7. non-letters are more than a third of non-whitespace characters — not prose;
///  8. fewer than two words of two or more letters — not prose.
pub fn is_prose_segment(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    // Meaningful tokens — those that have a letter and are not a version/resolution.
    let words: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| t.chars().any(|c| c.is_alphabetic()) && !is_version_or_resolution_token(t))
        .collect();
    // (2) command head.
    if tokens.len() <= 8 {
        if let Some(first) = words.first() {
            let head = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if head.chars().all(|c| !c.is_uppercase()) && COMMAND_HEADS.contains(&head) {
                return false;
            }
        }
    }
    // (3) only numbers and punctuation.
    if words.is_empty() {
        return false;
    }
    // (4) technical text inside the line — under a placeholder, the rest decides.
    if words.iter().any(|w| w.contains(NAME_PLACEHOLDER_MARK)) {
        return false;
    }
    let spans = technical_spans(trimmed);
    if !spans.is_empty() {
        // Technical start. The exception is a list of names through a slash
        // (`Fedora/RedHat distros:`, a live heading of the Qogir description):
        // it is a label, not a path, and its rest is translated («Дистрибутивы …:»).
        let first_letter = trimmed.find(|c: char| c.is_alphabetic()).unwrap_or(0);
        let (lead_start, lead_end) = spans[0];
        let lead_is_names = trimmed[lead_start..lead_end]
            .split('/')
            .all(|p| !p.is_empty() && p.chars().all(char::is_alphabetic));
        if lead_start <= first_letter && !lead_is_names {
            return false;
        }
        let first_is_lowercase = words.first().is_some_and(|w| !w.chars().any(char::is_uppercase));
        if (first_is_lowercase && tokens.iter().any(|t| is_command_flag(t)))
            || has_inline_command(&tokens)
        {
            return false;
        }
        let mut rest = String::with_capacity(trimmed.len());
        let mut copied = 0usize;
        for (start, end) in &spans {
            rest.push_str(&trimmed[copied..*start]);
            rest.push(' ');
            copied = *end;
        }
        rest.push_str(&trimmed[copied..]);
        return is_prose_segment(&rest);
    }
    // (5) the whole segment is product names.
    if words.iter().all(|w| is_product_name_token(w)) {
        return false;
    }
    // (6) one meaningful word — only a human word counts as prose.
    if words.len() == 1 {
        return is_plain_word(words[0]);
    }
    // (7) share of non-letters.
    let dense: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    let letters = dense.iter().filter(|c| c.is_alphabetic()).count();
    if (dense.len() - letters) * 3 > dense.len() {
        return false;
    }
    // (8) fewer than two words of two or more letters.
    let long_words = words
        .iter()
        .filter(|w| trim_token_punct(w).chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .count();
    long_words >= 2
}

// ── Protecting technology names INSIDE a phrase ────────────────────────────
//
// The filter above decides the fate of a WHOLE segment, and that is not
// enough: «Sweet Kvantum theme» is prose for it (three words, and not all of
// them names), and the live engine answers «сладкий Kvantum тема» to it and
// «квантская тема» to «A Kvantum theme heavily based on Microsoft's Fluent
// Design elements» — it inflects and translates a name inside a phrase. The
// house requires the opposite: names of technologies and standards stay in
// Latin script.
//
// The cure is substitution: before the engine the name goes under a
// placeholder, after the engine it returns to its place verbatim. The
// functions are PURE ([`mask_tech_names`] / [`unmask_tech_names`]), they do
// not look into the engine's text, and they do not change the
// [`crate::translator_engine::TranslateEngine`] contract at all — the
// substitution lives OUTSIDE the call.

/// Placeholder marker character.
///
/// `@` was not chosen by taste: it is the only character that CANNOT come in
/// the engine's translation by itself. First, a segment with a meaningful
/// token containing `@` has already been recognized as technical by the
/// filter ([`is_technical_token`], rule 4) — such text does not reach the
/// engine; second, `@` in the source turns substitution off entirely anyway
/// (see [`mask_tech_names`]). So an `@` left in the answer after names are
/// substituted is ALWAYS a trace of corruption, and that makes the loss check
/// reliable rather than approximate.
pub const NAME_PLACEHOLDER_MARK: char = '@';

/// The placeholder in place of the name with number `index`: `@0@`, `@1@`, …
///
/// The form was chosen by MEASURING on the live Firefox model `en→ru`, not by
/// guessing (the `zz_probe` bench, 21.09; its result remains in the tree — the
/// check `live_names_survive_the_real_engine` in `translator_engine.rs`).
/// Twenty forms were compared on seven live phrases; the result:
///   * `@0@` — intact in all of them; the trailing marker also prevents
///     numbers from gluing (`@1@` is not the start of `@10@`);
///   * `@@0@@` — breaks in 6 phrases of 7 («сладкая @@@@@ тема»): the engine
///     multiplies and swallows the repeated character;
///   * `【0】`, `⟦0⟧` — fall apart into bytes `<0xE2><0x9F><0xA6>` (the
///     character is not in the model's vocabulary), `$0` becomes «1 доллар»,
///     `<0>`/`{{0}}`/`Vv0vv` get duplicated and extra letters appended.
/// A form without a trailing marker (`@0`, `@x0`) also holds, but then the
/// numbers glue to neighbouring digits of the live text («@1 3» ↔ «@13»).
pub fn name_placeholder(index: usize) -> String {
    format!("{NAME_PLACEHOLDER_MARK}{index}{NAME_PLACEHOLDER_MARK}")
}

/// Text with placeholders instead of names, and the names themselves in
/// number order: `names[0]` is what stood in `@0@`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MaskedNames {
    pub text: String,
    pub names: Vec<String>,
}

/// Whether a word found in the text matches a canonical name from
/// [`PRODUCT_NAMES`] BY CASE.
///
/// Three spellings of a name count as its own: as in the list (`Kvantum`,
/// `GNOME`), ALL CAPS (`KVANTUM`) and Capitalized (`Gnome`, `Kde`, `Xfce` —
/// exactly how names are written in live workshop descriptions). An all
/// lowercase spelling does NOT count as a name, and that is the main point
/// here: the list holds `Mate`, `Unity`, `Cinnamon`, `Plasma` — ordinary
/// English words, and in the phrase «works with your mate» there is nothing
/// to substitute `mate` with.
pub fn name_casing_matches(canonical: &str, found: &str) -> bool {
    if canonical == found || found == canonical.to_uppercase() {
        return true;
    }
    let lower = canonical.to_lowercase();
    let mut capitalized = String::with_capacity(lower.len());
    for (i, c) in lower.chars().enumerate() {
        if i == 0 {
            capitalized.extend(c.to_uppercase());
        } else {
            capitalized.push(c);
        }
    }
    found == capitalized
}

/// An ACRONYM name in [`PRODUCT_NAMES`]: written in all CAPITALS (`KDE`,
/// `GTK`, `SDDM`, `XFCE`, `LXDE`, `GNOME`).
///
/// ⭐ WHY. Such a name stays a name in ANY case, and that cures the WORD LOSS
/// captured live on 23.09 on the Firefox `en→ru` model: «A colorful theme
/// inspired by sweet kde theme.» (item «Iridescent») it returns as
/// «красочная тема, вдохновленная сладкой темой.» — the word `kde` is not in
/// the translation at all. The same measurement shows that the lowercase
/// spelling is to blame: `sweet KDE theme` and `sweet Kde theme` arrive
/// intact, and so does the placeholder («вдохновленная сладкой @0@ темой»),
/// while a lone `kde is nice.` the engine even translates as «Кейд».
///
/// ⚠️ The relaxation is only for OUR list and only for acronyms: `kde`, `gtk`,
/// `sddm`, `xfce`, `lxde` are never English words in any case. Names that ARE
/// words (`Mate`, `Unity`, `Plasma`, `Cinnamon`) are written Capitalized in
/// the list, do not fall under this rule and live by the strict
/// [`name_casing_matches`] — «works with your mate» is still translated. ITEM
/// names from the caller get no relaxation at all: a «SWEET» label in caps
/// must not eat `sweet` in prose.
///
/// The rule has exactly one price and it is small: `GNOME` in the list is
/// also in caps, while lowercase `gnome` is an English word («garden gnome»).
/// Now such a phrase keeps the Latin `gnome` instead of «гнома». In a look
/// store a lowercase `gnome` is almost always the desktop, and a Latin word
/// in prose is more honest than a missing one.
fn is_acronym_name(canonical: &str) -> bool {
    canonical.chars().count() >= MIN_GUARDED_NAME_CHARS
        && canonical.chars().any(|c| c.is_alphabetic())
        && canonical.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
}

/// The whole word is a technology name (case-sensitive, and for the list's
/// acronyms — any case, see [`is_acronym_name`]).
fn is_masked_name(word: &str) -> bool {
    PRODUCT_NAMES.iter().any(|n| {
        name_casing_matches(n, word) || (is_acronym_name(n) && word.eq_ignore_ascii_case(n))
    }) || is_de_display_word(word)
        || is_distro_id_word(word)
}

/// Word boundaries: maximal runs of letters and digits. They are what give
/// the «word boundary» that makes `kde` inside `kdeglobals` not a name: the
/// run there is the whole `kdeglobals`, and it matches nothing.
fn word_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, i));
        }
    }
    if let Some(s) = start {
        runs.push((s, text.len()));
    }
    runs
}

/// Glue between two names: they hide under ONE placeholder if only spaces, a
/// comma or a slash are between them.
///
/// This is not decoration but survivability, and it was measured: «Nordic
/// Kvantum theme» with two placeholders in a row the engine breaks («Nordic @
/// Kvantum тема» — the character stayed, the number was eaten), and the list
/// «Gnome, Unity, Budgie, Pantheon, XFCE, Cinnamon and Mate» of seven
/// placeholders it multiplies (Budgie and XFCE come twice). With one
/// placeholder for the whole chain both phrases pass intact — and the chain's
/// contents return to the text verbatim, so the commas inside it stay the
/// author's.
fn is_name_glue(sep: &str) -> bool {
    matches!(sep.trim(), "" | "," | "/")
}

/// Glue INSIDE a name passed by the caller: `Aritim-Light` is written with a
/// hyphen in the description, `NT Legacy` — with a space, and `Sweet_Mars` —
/// with an underscore, and it is one and the same name. So the inner seam of
/// a name is wider than the glue between TWO different names
/// ([`is_name_glue`]): that one decides the fate of words foreign to each
/// other, while this one is the seam of a name known in advance.
fn is_guarded_gap(sep: &str) -> bool {
    matches!(sep.trim(), "" | "," | "/" | "-" | "–" | "—" | "_")
}

/// How many characters a caller's name must have to be hidden at all.
///
/// ⛔ A name of one or two characters is never hidden. An item «A» or «UI»
/// would give a placeholder for every such word of the description, and the
/// cost of error here is asymmetric: an extra placeholder means either a
/// corrupted phrase or (by the second sieve) refusing to translate the WHOLE
/// segment. An empty name is the same case.
const MIN_GUARDED_NAME_CHARS: usize = 3;

/// Words of a guarded name in order, or `None` — the name need not be hidden
/// (empty, only punctuation, or shorter than [`MIN_GUARDED_NAME_CHARS`]).
fn guarded_name_words(name: &str) -> Option<Vec<String>> {
    let runs = word_runs(name);
    let letters: usize = runs.iter().map(|(s, e)| name[*s..*e].chars().count()).sum();
    if runs.is_empty() || letters < MIN_GUARDED_NAME_CHARS {
        return None;
    }
    Some(runs.iter().map(|(s, e)| name[*s..*e].to_string()).collect())
}

/// Whether a word of the name is meaningful: whether it is fit to identify the
/// item ON ITS OWN.
///
/// Not fit: descriptive words ([`NAME_FILLER_WORDS`]), numbers («6» from
/// «Sweet Mars LAF Plasma 6») and pieces shorter than
/// [`MIN_GUARDED_NAME_CHARS`] («NT» from «NT Legacy» — the item is not
/// recognized by it alone, but the whole chain «NT Legacy» is caught as
/// before).
fn is_meaningful_name_word(word: &str) -> bool {
    if word.chars().count() < MIN_GUARDED_NAME_CHARS {
        return false;
    }
    if word.chars().all(|c| c.is_numeric()) {
        return false;
    }
    let lower = word.to_lowercase();
    !NAME_FILLER_WORDS.contains(&lower.as_str())
}

/// What an item name turns into for guarding: word chains that count as this
/// item, from long to short.
///
/// ⭐ WHY. Previously ONLY the whole word chain of the name was guarded, and
/// that was enough exactly for one-word names. Live acceptance on 23.09 showed
/// the price: the item «Nordic KDE» (chain `["Nordic","KDE"]`) had «Nordic
/// Plasma theme» in its own description — the chain did not match, and the
/// engine returned «Скандинавская Plasma тема»; the item «Sweet Mars LAF
/// Plasma 6» similarly lost `Sweet-Mars Kvantum theme` («Сладкий марс Kvantum
/// тема»).
///
/// So the following is taken from the name:
///   1. the whole chain of all words — as before, it is tried first;
///   2. groups of CONSECUTIVE meaningful words («Sweet Mars» from «Sweet Mars
///      LAF Plasma 6») — they catch a name written with a seam
///      (`Sweet-Mars`) with one placeholder, not two in a row, which the live
///      engine breaks;
///   3. each meaningful word separately («Nordic», «Sweet», «Mars»).
///
/// ⚠️ Case is not relaxed here: a chain of ONE word (and any word of point 3
/// is such a chain) requires a case match by [`name_casing_matches`],
/// otherwise the item «Dark» would eat the word `dark` in prose. The rule
/// lives in [`guarded_name_at`] and is shared by both lists.
fn guarded_name_variants(name: &str) -> Vec<Vec<String>> {
    let Some(words) = guarded_name_words(name) else {
        return Vec::new();
    };
    let mut out: Vec<Vec<String>> = vec![words.clone()];
    let mut group: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for word in &words {
        if is_meaningful_name_word(word) {
            group.push(word.clone());
        } else if !group.is_empty() {
            groups.push(std::mem::take(&mut group));
        }
    }
    if !group.is_empty() {
        groups.push(group);
    }
    for group in groups {
        if group.len() > 1 {
            out.push(group.clone());
        }
        out.extend(group.into_iter().map(|w| vec![w]));
    }
    let mut seen: Vec<Vec<String>> = Vec::with_capacity(out.len());
    out.retain(|words| {
        let fresh = !seen.contains(words);
        if fresh {
            seen.push(words.clone());
        }
        fresh
    });
    out
}

/// A guarded name starting at RUN `k`: how many runs it takes (`None` — no
/// name starts at this place).
///
/// The caller's names are tried before [`PRODUCT_NAMES`] and from the longest,
/// otherwise «NT Legacy» would lose to its own first word.
///
/// ⚠️ The case rule differs, and that is the main decision here:
///   * a name of SEVERAL words is compared case-insensitively. Matching a whole
///     word chain by accident is impossible, so «sweet kvantum» in the text is
///     the same item as «Sweet Kvantum» in its name;
///   * a name of ONE word requires the case to match by
///     [`name_casing_matches`] (as in the name, ALL CAPS or Capitalized; all
///     lowercase is not a name). Names are very often ordinary words —
///     «Dark», «Sweet», «Nordic», «Layan» — and «a dark theme» must be
///     translated («тёмная тема»), while «Dark» as an item name must not.
///     This is exactly the rule `Mate`, `Unity`, `Cinnamon` from the shared
///     list already live by (rule #12: one rule for both lists).
fn guarded_name_at(
    text: &str,
    runs: &[(usize, usize)],
    k: usize,
    guarded: &[Vec<String>],
) -> Option<usize> {
    for words in guarded {
        if k + words.len() > runs.len() {
            continue;
        }
        let matched = words.iter().enumerate().all(|(i, word)| {
            let (s, e) = runs[k + i];
            let found = &text[s..e];
            let same = if words.len() == 1 {
                name_casing_matches(word, found)
            } else {
                found.to_lowercase() == word.to_lowercase()
            };
            same && (i == 0 || is_guarded_gap(&text[runs[k + i - 1].1..s]))
        });
        if matched {
            return Some(words.len());
        }
    }
    let (start, end) = runs[k];
    is_masked_name(&text[start..end]).then_some(1)
}

/// Hide technology names under placeholders before calling the engine.
///
/// Since 03.10 TECHNICAL text inside a phrase goes under a placeholder too — a
/// path, a link, a file name, a variable, `code` ([`technical_spans`]); it
/// comes back through the same [`unmask_tech_names`] verbatim, byte for byte.
///
/// ⚠️ If the source ALREADY has `@`, no substitution is done at all (names
/// stay as they are): otherwise a placeholder could not be told apart from
/// the author's character, and the loss check would stop proving anything.
/// Such text is translated as before this change.
pub fn mask_tech_names(text: &str) -> MaskedNames {
    mask_names(text, &[])
}

/// The same as [`mask_tech_names`], plus names the CALLER asked to hide: the
/// name of the item itself, the author's name — everything that cannot be in
/// the [`PRODUCT_NAMES`] list.
///
/// ⭐ WHY. There is no list of theme names and there cannot be one (the
/// workshop grows every day), but the window knows the name of the open item —
/// it arrives next to the description. Without this the live engine answered
/// «Скандинавская Kvantum тема» to «Nordic Kvantum theme» and «Сладкая Kvantum
/// тема» to «Sweet Kvantum theme»: we had already protected the technology
/// name, but it translated the ITEM NAME as prose.
///
/// The name is recognized not only by the whole chain of its words but also
/// by meaningful words separately ([`guarded_name_variants`]): for the item
/// «Nordic KDE» the description line «Nordic Plasma theme» does not contain
/// the whole chain, but it must be protected.
///
/// The trick is the same and so are the sieves: the name goes under a
/// placeholder, [`unmask_tech_names`] returns it VERBATIM (exactly the
/// spelling that stood in the source text, not the one from the caller), and
/// if the placeholders do not match, the segment is returned as the original.
pub fn mask_names(text: &str, guarded: &[&str]) -> MaskedNames {
    if text.contains(NAME_PLACEHOLDER_MARK) {
        return MaskedNames { text: text.to_string(), names: Vec::new() };
    }
    // A long name is tried before a short one also inside ONE item name:
    // the whole chain «Sweet Mars LAF Plasma 6» before the group «Sweet
    // Mars», and that before a lone «Sweet». The sort is stable, so the
    // [`guarded_name_variants`] order within one length is kept.
    let mut extra: Vec<Vec<String>> =
        guarded.iter().flat_map(|n| guarded_name_variants(n)).collect();
    extra.sort_by(|a, b| b.len().cmp(&a.len()));

    // Technical text inside a phrase (path, link, file, variable, `code`) —
    // under a placeholder on a par with names ([`technical_spans`]). A name
    // INSIDE a technical piece (`kde` in `~/.local/share/kde`) is not hidden
    // separately: the piece goes whole.
    // Picture characters (emoji, ✨, ™, arrows) — the same: the model does
    // not know them and substitutes its own («### ✨ Features:» →
    // «### 👍 Особенности:», measured 06.11), while the author's character
    // must arrive byte for byte.
    let mut tech = technical_spans(text);
    let pictures: Vec<(usize, usize)> = pictograph_spans(text)
        .into_iter()
        .filter(|(s, e)| !tech.iter().any(|(a, b)| s < b && e > a))
        .collect();
    tech.extend(pictures);
    tech.sort();
    let overlaps_tech = |s: usize, e: usize| tech.iter().any(|(a, b)| s < *b && e > *a);
    let mut spans: Vec<(usize, usize)> = tech.clone();

    let runs = word_runs(text);
    let mut k = 0usize;
    while k < runs.len() {
        if overlaps_tech(runs[k].0, runs[k].1) {
            k += 1;
            continue;
        }
        let Some(taken) = guarded_name_at(text, &runs, k, &extra) else {
            k += 1;
            continue;
        };
        let (start, end) = (runs[k].0, runs[k + taken - 1].1);
        if overlaps_tech(start, end) {
            k += 1;
            continue;
        }
        spans.push((start, end));
        k += taken;
    }
    spans.sort();

    // A consecutive chain (names and technical text through a space, comma
    // or slash) — under ONE placeholder: the live engine breaks two
    // placeholders in a row.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        if let Some(last) = merged.last_mut() {
            if start >= last.1 && is_name_glue(&text[last.1..start]) {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut out = String::with_capacity(text.len());
    let mut names: Vec<String> = Vec::new();
    let mut copied = 0usize;
    for (start, end) in merged {
        out.push_str(&text[copied..start]);
        out.push_str(&name_placeholder(names.len()));
        names.push(text[start..end].to_string());
        copied = end;
    }
    out.push_str(&text[copied..]);
    MaskedNames { text: out, names }
}

/// Put the names back into the translation. `None` — the placeholders did not
/// match, the translation must NOT be taken.
///
/// Loss is caught by two sieves, and together they cover all kinds of
/// corruption seen on the live model:
///   1. each placeholder must occur EXACTLY ONCE — that covers both loss (the
///      engine ate `@2@`) and multiplication (`Budgie` twice);
///   2. after substitution not a single `@` may remain in the line — those are
///      leftovers («Nordic @ Kvantum тема», `@@1@@@`) and invented numbers we
///      never gave.
/// There were no names — nothing to check, the translation is taken as is
/// (an `@` in it could only come from the source, see [`mask_tech_names`]).
///
/// ⭐ BEFORE the sieves ONE kind of multiplication is cured — a GLUED repeat
/// ([`collapse_adjacent_repeat`]): `@1@ @1@` in a row. Acceptance 24.09, a
/// comment on the item «Nordic KDE»: «Can we expect soon the @0@ Bluish theme
/// of @1@ for @2@ too?» the live model returned as «…@0@ Bluish тему @1@ @1@
/// для @2@ тоже?», sieve 1 rejected the translation, and the customer saw the
/// whole comment in English (together with «Very nice.», which translated
/// perfectly). A glued repeat is unambiguous: the name stood in the source
/// ONCE, so the second copy right after it is decoder stutter, and it can be
/// removed without guessing. A repeat in DIFFERENT places of the line is
/// still a refusal: there the second copy may be a genuine second mention,
/// and we have no right to choose between them.
pub fn unmask_tech_names(translated: &str, names: &[String]) -> Option<String> {
    if names.is_empty() {
        return Some(translated.to_string());
    }
    let mut out = translated.to_string();
    for (index, name) in names.iter().enumerate() {
        let placeholder = name_placeholder(index);
        out = collapse_adjacent_repeat(&out, &placeholder);
        if out.matches(&placeholder).count() != 1 {
            return None;
        }
        out = out.replace(&placeholder, name);
    }
    if out.contains(NAME_PLACEHOLDER_MARK) {
        return None;
    }
    Some(out)
}

/// Collapse GLUED placeholder repeats: `@1@ @1@` → `@1@`.
///
/// Copies may be separated only by spaces — nothing else appeared between
/// them during decoder stutter, and any letter or character between
/// placeholders means these are two DIFFERENT places of the translation, and
/// they must not be collapsed.
fn collapse_adjacent_repeat(text: &str, placeholder: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(placeholder) {
        let after = at + placeholder.len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        loop {
            let gap = rest.len() - rest.trim_start_matches([' ', '\u{a0}', '\t']).len();
            match rest[gap..].strip_prefix(placeholder) {
                Some(tail) => rest = tail,
                None => break,
            }
        }
    }
    out.push_str(rest);
    out
}

// ── Picture characters, sentence edges, soft names, invented repeats ──────

/// A picture character: emoji and kin (✨ 🚀 ★ ™ © ➜), as well as invisible
/// emoji parts (variation selector, joiner, skin tone modifiers and flags).
///
/// ⭐ WHY. The Firefox model vocabulary barely knows these characters, and the
/// decoder puts something «similar» in their place: «### ✨ Features:» →
/// «### 👍 Особенности:» (measured 06.11, KDE Store, theme 2380824). The
/// author's character is not text, there is nothing to translate in it, and
/// it must not reach the engine at all.
///
/// The ranges are Unicode blocks where pictograms live (arrows, technical
/// symbols, geometric shapes, «miscellaneous symbols», dingbats, the emoji
/// block U+1F000–1FAFF), plus ©, ®, ™. Letters, digits and ordinary
/// punctuation do not get here.
pub fn is_pictograph(c: char) -> bool {
    matches!(c,
        '\u{00A9}' | '\u{00AE}' | '\u{203C}' | '\u{2049}' | '\u{2122}' | '\u{2139}'
        | '\u{2190}'..='\u{21FF}'
        | '\u{2300}'..='\u{23FF}'
        | '\u{24C2}'
        | '\u{25A0}'..='\u{27BF}'
        | '\u{2900}'..='\u{297F}'
        | '\u{2B00}'..='\u{2BFF}'
        | '\u{3030}' | '\u{303D}' | '\u{3297}' | '\u{3299}'
        | '\u{FE0E}' | '\u{FE0F}' | '\u{200D}' | '\u{20E3}'
        | '\u{1F000}'..='\u{1FAFF}'
        | '\u{E0020}'..='\u{E007F}')
}

/// Runs of picture characters in a line: (start, end) in bytes. Neighbouring
/// characters (a flag of two regional letters, an emoji with a joiner) are
/// one run.
pub fn pictograph_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if is_pictograph(c) {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            spans.push((s, i));
        }
    }
    if let Some(s) = start {
        spans.push((s, text.len()));
    }
    spans
}

/// Characters the phrase itself may start with — they are NOT cut off from
/// the edge: a bracket and a quote are paired (the second half is inside the
/// phrase), while `~ . / $ \` ` and `@` start technical text that
/// [`technical_spans`] recognizes.
const SENTENCE_OPENERS: &[char] =
    &['(', '[', '{', '"', '\'', '«', '“', '‘', '„', '<', '¿', '¡', '`', '~', '.', '/', '$', '\\', '@'];

/// Cut a sentence into (left edge, middle, right edge). Only the middle goes to
/// the engine; the edges return to the output byte for byte.
///
/// ⭐ WHY (measured 06.11 on the live model, theme 2380824):
///   * the node after `</strong>` starts with «: Full Windows XP …» — the
///     engine changes the colon into a dash («- Светло-голубая область…») or
///     eats it («</strong>Выбор между…»);
///   * «### ✨ Features:» — the engine replaces the author's character with its
///     own (✨ → 👍).
/// These edges are text markup (list markers, heading hashes, the colon after
/// a bold label, emoji), not language: there is nothing to translate in them.
///
/// The left edge is punctuation and picture characters UP TO and including
/// the first space (except [`SENTENCE_OPENERS`]); without a space after it the
/// edge is not cut (`-r`, `:Note` are part of a word). The right edge is only
/// spaces and picture characters («Enjoy! 🎉»): the engine puts the period,
/// question mark and colon at the end of a phrase itself, and correctly for
/// its language.
pub fn split_sentence_edges(sentence: &str) -> (&str, &str, &str) {
    let mut lead_end = 0usize;
    for (i, c) in sentence.char_indices() {
        if c.is_whitespace() {
            lead_end = i + c.len_utf8();
            continue;
        }
        // The first letter, digit or «opening» character — from there on it is
        // the phrase. Characters before it become the edge only together
        // with the space after them.
        if c.is_alphanumeric() || SENTENCE_OPENERS.contains(&c) {
            break;
        }
    }
    // The left edge is only what was followed by a space: «### ✨ » is cut
    // whole, «-r» is not cut at all.
    let lead = &sentence[..lead_end];
    let rest = &sentence[lead_end..];
    let trail_start = rest
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_whitespace() || is_pictograph(*c))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let (core, trail) = rest.split_at(trail_start);
    // On the right we cut only if there really was a picture (spaces alone
    // do not bother the engine and do not get into the output anyway).
    if !trail.chars().any(is_pictograph) {
        return (lead, rest, "");
    }
    (lead, core, trail)
}

/// Hide again under placeholders ONLY the chosen names: `keep[i] == true` —
/// `names[i]` stays under a placeholder, the others return to the text.
/// Placeholder numbers go consecutively from zero over the remaining ones.
///
/// ⭐ WHY. The second pass hid ALL names of the phrase, although the engine
/// lost one in the first. Placeholders themselves confuse the decoder, and
/// with four at once it breaks: «: Full Windows XP Explorer replica with …
/// Classic View …» (item «… Luna Blue and Classic Suite», measured 06.11)
/// came back with `@0@` twice — the segment was thrown away, and the
/// paragraph stayed entirely English, although only one `Classic` got lost in
/// the first pass.
pub fn mask_only(masked: &MaskedNames, keep: &[bool]) -> MaskedNames {
    let mut text = masked.text.clone();
    let mut names: Vec<String> = Vec::new();
    // First give ALL of them their own numbers via a temporary marker, so
    // that `@1@` is not confused with an already rewritten `@1@`.
    for (i, name) in masked.names.iter().enumerate() {
        let from = name_placeholder(i);
        let to = if keep.get(i).copied().unwrap_or(false) {
            let t = format!("\u{0}{}\u{0}", names.len());
            names.push(name.clone());
            t
        } else {
            name.clone()
        };
        text = text.replacen(&from, &to, 1);
    }
    for i in 0..names.len() {
        text = text.replacen(&format!("\u{0}{i}\u{0}"), &name_placeholder(i), 1);
    }
    MaskedNames { text, names }
}

/// A «soft» name: one word from the item name, not the name itself, not a
/// name from the core's lists and not technical text. Losing such a name is
/// not corruption but translation of the word: from «… Luna Blue and Classic
/// Suite» `Classic` is guarded too, and in the prose «Classic View» the
/// engine honestly writes «классическим представлением».
///
/// If such a name did not hold even under a placeholder, the sentence is
/// taken as translated without it, and NOT thrown away together with the
/// whole paragraph: an English paragraph is worse than a translated word from
/// the name.
pub fn is_soft_guarded_name(name: &str, guarded: &[&str]) -> bool {
    let runs = word_runs(name);
    if runs.len() != 1 || runs[0] != (0, name.len()) {
        return false;
    }
    if is_masked_name(name) || !technical_spans(name).is_empty() {
        return false;
    }
    !guarded.iter().any(|g| g.trim().eq_ignore_ascii_case(name))
}

/// Beginnings at which the engine itself adds the conjunction «и» at the
/// start of the phrase: «Full documentation and source code available on
/// GitHub:» → «и полная документация…» (measured 06.11). The conjunction is
/// legitimate only if it was in the source: `and`, `both … and` («И X, и
/// Y»), `plus`, `also`, `&`.
const SOURCE_LEADING_CONJUNCTIONS: &[&str] = &["and", "both", "plus", "also"];

/// The conjunction the engine invents at the start of a phrase — in the target language.
fn invented_leading_word(tgt: &str) -> Option<&'static str> {
    match tgt {
        "ru" => Some("и"),
        "de" => Some("und"),
        _ => None,
    }
}

/// Remove from the translation what was not in the source and what the engine
/// ADDED by itself:
///   * a repeat of adjacent identical words («для KDE Plasma 6 и и Bazzite»:
///     the live model answers a lone «and» with «и и»). Removed only if the
///     source itself has no consecutive repeat — otherwise the repeat is the
///     author's («very very nice» → «очень очень мило»);
///   * the conjunction «и» at the start of a phrase that did not start with a
///     conjunction in the source («и полная документация…» for «Full
///     documentation…»).
pub fn drop_invented_repeats(src: &str, tgt: &str, source: &str, translated: &str) -> String {
    let mut out = translated.to_string();
    if !has_adjacent_repeat(source) {
        out = collapse_word_repeats(&out);
    }
    // A dash or colon at the start that was not in the source: «Lock
    // Screen» → «- экран замка» (measured 06.11). The source starts with a
    // letter — so the character is invented.
    if source.trim_start().chars().next().is_some_and(char::is_alphanumeric) {
        let trimmed = out.trim_start();
        let mut chars = trimmed.chars();
        if let (Some(mark), Some(gap)) = (chars.next(), chars.next()) {
            if matches!(mark, '-' | '–' | '—' | ':') && gap.is_whitespace() {
                out = chars.as_str().trim_start().to_string();
            }
        }
    }
    let first_source = source
        .split_whitespace()
        .next()
        .map(|w| trim_token_punct(w).to_lowercase())
        .unwrap_or_default();
    let word = if src == "en" { invented_leading_word(tgt) } else { None };
    if let Some(word) = word {
        let source_starts_with_conjunction = SOURCE_LEADING_CONJUNCTIONS
            .contains(&first_source.as_str())
            || source.trim_start().starts_with('&');
        if !source_starts_with_conjunction {
            let trimmed = out.trim_start();
            let lead = &out[..out.len() - trimmed.len()];
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let head = parts.next().unwrap_or_default();
            if let Some(tail) = parts.next() {
                if head.to_lowercase() == word && tail.trim_start().chars().next().is_some_and(char::is_alphabetic) {
                    out = format!("{lead}{}", tail.trim_start());
                }
            }
        }
    }
    out
}

/// Whether the line has two identical words in a row (case-insensitive).
fn has_adjacent_repeat(text: &str) -> bool {
    let words: Vec<String> = text.split_whitespace().map(|w| w.to_lowercase()).collect();
    words.windows(2).any(|p| p[0] == p[1] && p[0].chars().any(char::is_alphabetic))
}

/// Collapse repeats of adjacent identical words: «и и» → «и». The whole word
/// (with punctuation) is compared: «да, да» is not «да да» but the author's
/// intonation.
fn collapse_word_repeats(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last: Option<String> = None;
    let mut rest = text;
    loop {
        let ws = rest.len() - rest.trim_start().len();
        let (gap, after) = rest.split_at(ws);
        if after.is_empty() {
            out.push_str(gap);
            break;
        }
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        let (word, tail) = after.split_at(end);
        let key = word.to_lowercase();
        let repeat = last.as_deref() == Some(key.as_str()) && key.chars().any(char::is_alphabetic);
        if !repeat {
            out.push_str(gap);
            out.push_str(word);
        }
        last = Some(key);
        rest = tail;
    }
    out
}

/// A Latin word in the translation that was NOT in the source — a trace of
/// engine corruption. `None` — all the Latin in the translation is its own.
///
/// ⭐ WHY. Acceptance 24.09, item «Awesome Widgets» (arcanis): the line
/// `graphical item support - tooltips, bars` the live model returned as
/// «служба графических элементов - toolbep, bars». `toolbep` is NOT a mix of
/// alphabets (checked by code points: all seven letters are ASCII, `U+0074
/// U+006F U+006F U+006C U+0062 U+0065 U+0070`), but a word INVENTED by the
/// subword decoder from `tooltips`. Neither the prose filter nor the
/// placeholder sieves see this: the line has no names at all.
///
/// It can be caught in exactly one way — by comparing with the source. Latin
/// in an `en→ru` translation comes ONLY from the source (names, versions,
/// switches, paths); a Russian translation has no Latin of its own. So a
/// Latin word not found in the source is corruption, and a sentence with it
/// must not be taken.
///
/// Comparison is case-insensitive and BY SUBSTRING: `Kvantum` from
/// `Kvantum's` is its own, `GTK` from `GTK2` is its own, `bars` from `bars`
/// is its own, but `toolbep` is foreign. Words shorter than two letters are
/// not judged (a lone `x` in «3 x 4» and the like).
pub fn invented_latin_word(source: &str, translated: &str) -> Option<String> {
    let lower_source = source.to_lowercase();
    let mut word = String::new();
    for c in translated.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphabetic() {
            word.push(c);
            continue;
        }
        if word.len() >= 2 && !lower_source.contains(&word.to_lowercase()) {
            return Some(word);
        }
        word.clear();
    }
    None
}

// ── Term fix dictionary (after the engine) ────────────────────────────────

/// One fix: the engine's WRONG word form → the RIGHT one in the very same
/// grammatical form. Comparison ignores case and `ё`/`е`; the case of the
/// found word's first letter is carried over to the replacement.
#[derive(Debug, Clone, Copy)]
pub enum TermForm {
    /// The whole word — anywhere in the phrase.
    Word(&'static str, &'static str),
    /// An imperative that is spelled like the future tense (`загрузите`:
    /// «загрузите архив» and «если вы загрузите архив»). Replaced only if
    /// there is no «вы» among the three words before it — otherwise it is the
    /// future, and the replacement would have another form («скачаете», not
    /// «скачайте»).
    Imperative(&'static str, &'static str),
    /// A noun whose replacement has a different gender (`извлечения` neuter →
    /// `распаковки` feminine). Replaced ONLY right after a preposition: then
    /// there is neither an adjective nor a predicate with it that would have to
    /// be agreed anew («после извлечения» → «после распаковки»), while
    /// «извлечение не сработало» stays as is.
    AfterPreposition(&'static str, &'static str),
    /// A participle stem: stem + full ending (`ый`, `ая`, `ого`, …,
    /// [`PARTICIPLE_ENDINGS`]) → new stem + the same ending («извлечённого» →
    /// «распакованного»). The ending is carried over as is, so gender, number
    /// and case are kept without any inflection.
    ParticipleStem(&'static str, &'static str),
    /// Several words in a row (separated by spaces) → a ready replacement.
    Phrase(&'static str, &'static str),
}

/// Full endings of participles and adjectives — all that `-нный` can have.
/// Short forms (`извлечён`, `извлечена`) are written as separate words in the
/// dictionary.
const PARTICIPLE_ENDINGS: &[&str] = &[
    "ый", "ий", "ая", "ое", "ые", "ого", "ой", "ую", "ых", "ым", "ыми", "ом", "ому",
];

/// Prepositions after which a [`TermForm::AfterPreposition`] noun stands
/// without an agreeing word.
const PREPOSITIONS: &[&str] = &[
    "после", "для", "до", "без", "при", "к", "ко", "перед", "от", "с", "со", "по", "из", "в",
    "во", "о", "об", "на", "через",
];

/// A group of fixes for one term.
///
/// ⭐ FIXES ONLY BY PAIR: the group fires if the SOURCE sentence has one of
/// its English terms ([`TermFix::source_words`], compared by word start:
/// `unzip` also catches `unzipping`) and passes [`TermFix::gate`]. A
/// «Расстегните» that did not come from `unzip` is not touched by the
/// dictionary — maybe it really is about a fastener.
#[derive(Debug, Clone, Copy)]
pub struct TermFix {
    /// Source and translation language (`en` → `ru`). An `en` → `de`
    /// dictionary fits here with its own groups, the mechanism is shared.
    pub src: &'static str,
    pub tgt: &'static str,
    /// Beginnings of English words that switch the group on.
    pub source_words: &'static [&'static str],
    /// An additional condition on the source sentence (lowercase).
    pub gate: fn(&str) -> bool,
    /// «Wrong → right» pairs.
    pub forms: &'static [TermForm],
    /// Collapse a glued repeat of the RIGHT form: «скачать скачать» →
    /// «скачать» (the live model on «Download bigsur-sound-theme.tar.gz from
    /// Files»).
    pub collapse_repeats: bool,
}

fn always(_: &str) -> bool {
    true
}

/// `extract` means «unpack» only when it is about an archive.
///
/// Live KDE Store descriptions know another `extract` too: «automatically
/// extract and apply dominant icon color», «All sounds have been extracted
/// from MacOS Monterey», «icons extracted from the beta of Uos» — there
/// «извлечь» is right. So an archive sign is needed (archive, zip/tar, file,
/// folder, package, path, download), and `from` after `extract` must NOT be
/// there (one extracts FROM something, but unpacks an archive).
fn extract_means_unpack(source: &str) -> bool {
    let Some(at) = source.find("extract") else { return false };
    // «…directly from extracted source files» — extracted FROM something.
    if source[..at].trim_end().ends_with("from") {
        return false;
    }
    // `from` after `extract` — «extract FROM», unless an archive is between
    // them: «extracted from MacOS Monterey», «extract the chrome folder from
    // one of the IE themes» — «извлечь»; «extract "maliceandspitekv.zip"
    // from this page» — «распаковать» (the object is the archive itself).
    let after = &source[at..];
    let words: Vec<&str> = after.split_whitespace().collect();
    if let Some(from) = words.iter().position(|w| *w == "from") {
        const ARCHIVE: &[&str] = &[
            "archive", "zip", "tar", ".gz", ".xz", ".7z", ".rar", ".bz2", ".zst", "tgz",
            "compressed", "package",
        ];
        let object = &words[..from];
        if !object.iter().any(|w| ARCHIVE.iter().any(|a| w.contains(a))) {
            return false;
        }
    }
    if source.contains("color") || source.contains("colour") {
        return false;
    }
    const MARKERS: &[&str] = &[
        "archive", "zip", "tar", "compressed", "package", "file", "folder", "download", "~/",
        "/usr/", "/home", ".7z", ".rar",
    ];
    MARKERS.iter().any(|m| source.contains(m))
}

/// `download` → «скачать» only if the sentence has no OTHER `load` (`upload`,
/// `load`, `reload`): there «загрузить» may be the legitimate word for them.
/// And no KDE window name «Download New …» («The Download New Global Themes
/// program…»): KDE ru has «Загрузить новые …» for it, and turning someone
/// else's window name into «скачать» is not allowed.
fn download_alone(source: &str) -> bool {
    let other_load = source
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.contains("load") && !w.starts_with("download"));
    !other_load && !source.contains("download new")
}

/// «Drag and drop» in any spelling, not just any `drag…` (`dragon`).
fn drag_and_drop(source: &str) -> bool {
    ["drag and drop", "drag & drop", "drag-and-drop", "drag'n'drop", "drag n drop"]
        .iter()
        .any(|p| source.contains(p))
}

/// «Files» — the tab of a KDE Store item page («Download … from Files»,
/// «from the Files tab»), not the word «файлы».
fn files_tab(source: &str) -> bool {
    source.contains("from files") || source.contains("files tab") || source.contains("files(")
}

/// Forms of «распаковать» instead of what the live Firefox en→ru model makes
/// of `unzip`/`unpack`/`extract` («расстегните», «извлеките», «вытащить»,
/// «разбейте», «распечатайте» and the invented «извайте», «изящите»).
///
/// SOURCE OF THE TERM: KDE ru — Ark («Extract» → «Распаковать», «Extract
/// here» → «Распаковать в эту папку», «Extraction failed…» → «Не удалось
/// распаковать…»), kcm and Discover; our po/ru.po («Couldn't unpack the
/// archive: %1» → «Не удалось распаковать архив: %1», «unpacking the theme»
/// → «распаковки темы»).
const UNPACK_FORMS: &[TermForm] = &[
    // извлечь / извлекать (perfective → perfective, imperfective → imperfective)
    TermForm::Word("извлеките", "распакуйте"),
    TermForm::Word("извлеки", "распакуй"),
    TermForm::Word("извлечь", "распаковать"),
    TermForm::Word("извлечет", "распакует"),
    TermForm::Word("извлекут", "распакуют"),
    TermForm::Word("извлекайте", "распаковывайте"),
    TermForm::Word("извлекай", "распаковывай"),
    TermForm::Word("извлекать", "распаковывать"),
    TermForm::Word("извлекает", "распаковывает"),
    TermForm::Word("извлекают", "распаковывают"),
    TermForm::Word("извлекая", "распаковывая"),
    TermForm::Word("извлекши", "распаковав"),
    TermForm::Word("извлечен", "распакован"),
    TermForm::Word("извлечена", "распакована"),
    TermForm::Word("извлечено", "распаковано"),
    TermForm::Word("извлечены", "распакованы"),
    TermForm::ParticipleStem("извлеченн", "распакованн"),
    TermForm::AfterPreposition("извлечения", "распаковки"),
    TermForm::AfterPreposition("извлечению", "распаковке"),
    TermForm::AfterPreposition("извлечении", "распаковке"),
    TermForm::AfterPreposition("извлечением", "распаковкой"),
    // расстегнуть (unzip)
    TermForm::Word("расстегните", "распакуйте"),
    TermForm::Word("расстегни", "распакуй"),
    TermForm::Word("расстегнуть", "распаковать"),
    TermForm::Word("расстегнув", "распаковав"),
    TermForm::Word("расстегивайте", "распаковывайте"),
    TermForm::Word("расстегивать", "распаковывать"),
    TermForm::ParticipleStem("расстегнут", "распакованн"),
    TermForm::AfterPreposition("расстегивания", "распаковки"),
    // вытащить, выкрутить (extract)
    TermForm::Word("вытащите", "распакуйте"),
    TermForm::Word("вытащи", "распакуй"),
    TermForm::Word("вытащить", "распаковать"),
    TermForm::Word("выкрутите", "распакуйте"),
    TermForm::Word("выкрутить", "распаковать"),
    // разбить (unpack), распечатать (unzip)
    TermForm::Word("разбейте", "распакуйте"),
    TermForm::Word("распечатайте", "распакуйте"),
    TermForm::Word("распечатать", "распаковать"),
    // Words that do not exist in Russian — decoder inventions, all imperative.
    TermForm::Word("извайте", "распакуйте"),
    TermForm::Word("изящите", "распакуйте"),
    TermForm::Word("извахните", "распакуйте"),
    TermForm::Word("извлечайте", "распаковывайте"),
];

/// Forms of «скачать» instead of «загрузить» and the invented «скашляйте».
///
/// SOURCE OF THE TERM: our po/ru.po — the «Download» button → «Скачать»,
/// «Couldn't download the archive» → «Не удалось скачать архив», «📥 Download
/// Only» → «📥 Только скачать». (KDE writes «Загрузить новые …» in kcm — Rexon
/// has its own word, and the theme description talks to the person with the
/// same word as the button.) Nouns («загрузка», «загрузки») are not touched:
/// «скачивание» has a different gender.
const DOWNLOAD_FORMS: &[TermForm] = &[
    TermForm::Imperative("загрузите", "скачайте"),
    TermForm::Word("загрузи", "скачай"),
    TermForm::Word("загрузить", "скачать"),
    TermForm::Word("загружайте", "скачивайте"),
    TermForm::Word("загружай", "скачивай"),
    TermForm::Word("загружать", "скачивать"),
    TermForm::Word("загружает", "скачивает"),
    TermForm::Word("загружают", "скачивают"),
    TermForm::Word("загружая", "скачивая"),
    TermForm::Word("загрузив", "скачав"),
    TermForm::Word("загрузил", "скачал"),
    TermForm::Word("загрузила", "скачала"),
    TermForm::Word("загрузило", "скачало"),
    TermForm::Word("загрузили", "скачали"),
    TermForm::Word("загрузит", "скачает"),
    TermForm::Word("загрузят", "скачают"),
    TermForm::Word("загружен", "скачан"),
    TermForm::Word("загружена", "скачана"),
    TermForm::Word("загружено", "скачано"),
    TermForm::Word("загружены", "скачаны"),
    TermForm::ParticipleStem("загруженн", "скачанн"),
    TermForm::ParticipleStem("загружаем", "скачиваем"),
    // Decoder inventions («Download the archive and unpack it.» →
    // «скашляйте архив…»).
    TermForm::Word("скашляйте", "скачайте"),
    TermForm::Word("скашлять", "скачать"),
];

/// The term fix dictionary for description translation — ONE place.
///
/// ⭐ WHY (owner's defect 03.10, KDE Store): «Just unzip the file…» the live
/// Firefox en→ru model translates as «Просто расстегните файл…», «Download
/// bigsur-sound-theme.tar.gz from Files» — «Скачать скачать … с Файлы»,
/// «extract» — «извлеките», «вытащить», «изящите».
///
/// ⚠️ WHY AFTER THE ENGINE AND NOT BEFORE. Substitution before the engine was
/// tested on the same model and is unreliable: replacing `unzip` with `unpack`
/// in the source gives now «распаковать его», now «Разбейте в
/// /usr/share/icons/» (`Unpack into …`), and a placeholder in place of the
/// verb itself confuses the decoder (see the two passes in
/// `translator_engine`). A fix after the engine by the pair «source term →
/// wrong form → right one in THE SAME form» guesses neither aspect nor case:
/// the engine chose the form, the dictionary changes only the lexeme.
///
/// ⛔ DO NOT INVENT FORMS. Each pair is one grammatical form into another;
/// where the replacement has a different gender (nouns), the fix applies only
/// right after a preposition ([`TermForm::AfterPreposition`]); where the
/// imperative matches the future — only without «вы» nearby
/// ([`TermForm::Imperative`]). A form not listed here stays as is.
///
/// Adding a language (`de`): new groups with `tgt: "de"` — «entzippen» →
/// «entpacken» and so on — the source of terms is the same: KDE de (Ark:
/// «Extract» → «Entpacken») and our po/de.po.
pub const TERM_FIXES: &[TermFix] = &[
    // unzip/unpack/untar/decompress — always «распаковать».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["unzip", "unpack", "untar", "decompress", "uncompress", "unrar"],
        gate: always,
        forms: UNPACK_FORMS,
        collapse_repeats: false,
    },
    // extract — «распаковать» only about an archive ([`extract_means_unpack`]).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["extract"],
        gate: extract_means_unpack,
        forms: UNPACK_FORMS,
        collapse_repeats: false,
    },
    // download — «скачать» (the Rexon «Скачать» button).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["download"],
        gate: download_alone,
        forms: DOWNLOAD_FORMS,
        collapse_repeats: true,
    },
    // «from Files» — a KDE Store tab; the engine leaves «с Файлы»
    // (nominative after a preposition).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["files"],
        gate: files_tab,
        forms: &[
            TermForm::Phrase("с файлы", "со вкладки «Файлы»"),
            TermForm::Phrase("из файлы", "со вкладки «Файлы»"),
            TermForm::Phrase("со вкладки файлы", "со вкладки «Файлы»"),
        ],
        collapse_repeats: false,
    },
    // drag and drop — «перетащите» (KDE ru, kcm_kwindecoration: «Drag buttons
    // between here and the titlebar» → «Перетаскивайте кнопки…»). The engine
    // appends «и высадите» to it.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["drag"],
        gate: drag_and_drop,
        forms: &[
            TermForm::Phrase("перетащите и высадите", "перетащите"),
            TermForm::Phrase("перетащите и бросьте", "перетащите"),
            TermForm::Phrase("перетащите и отпустите", "перетащите"),
            TermForm::Phrase("перетащить и высадить", "перетащить"),
            TermForm::Phrase("перетащить и бросить", "перетащить"),
            TermForm::Phrase("перетащить и отпустить", "перетащить"),
            TermForm::Phrase("перетаскивайте и бросайте", "перетаскивайте"),
        ],
        collapse_repeats: false,
    },
    // icon pack — «набор значков» (KDE ru, kcm_icons: «Installing icon
    // themes…» → «Установка наборов значков...»). «Пакет» and «набор» have
    // the same gender and declension, the case is carried over as is.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["icon"],
        gate: |s| s.contains("icon pack") || s.contains("icons pack") || s.contains("iconpack"),
        forms: &[
            TermForm::Phrase("пакет значков", "набор значков"),
            TermForm::Phrase("пакета значков", "набора значков"),
            TermForm::Phrase("пакету значков", "набору значков"),
            TermForm::Phrase("пакетом значков", "набором значков"),
            TermForm::Phrase("пакете значков", "наборе значков"),
            TermForm::Phrase("пакеты значков", "наборы значков"),
            TermForm::Phrase("пакетов значков", "наборов значков"),
            TermForm::Phrase("пакет иконок", "набор значков"),
            TermForm::Phrase("пакета иконок", "набора значков"),
            TermForm::Phrase("пакеты иконок", "наборы значков"),
            TermForm::Phrase("пакетов иконок", "наборов значков"),
        ],
        collapse_repeats: false,
    },
    // Installation — «Установка» (KDE ru, Discover: «Install» → «Установка»,
    // kcm_icons «Installing…» → «Установка…»). The engine picks
    // «Инсталляция». Both are feminine first declension, but «инсталляции»
    // is ambiguous by case, so only unambiguous forms.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["install"],
        gate: always,
        forms: &[
            TermForm::Word("инсталляция", "установка"),
            TermForm::Word("инсталляцию", "установку"),
            TermForm::Word("инсталляцией", "установкой"),
        ],
        collapse_repeats: false,
    },
    // Explorer — «Проводник»: that is the file manager's name in Russian
    // Windows, and the live model translates it that way INSIDE a phrase
    // («Full Windows XP Explorer replica» → «Полная копия проводника Windows
    // XP», measured 06.11). A lone «Explorer» (a label in <strong>, without
    // context) it returns as «исследователь». Both words are masculine
    // second declension, the case is carried over as is. We do not leave it
    // in Latin: in a neighbouring phrase of the same description the model
    // already wrote «проводник», and two names of one program in one card
    // would confuse.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["explorer"],
        gate: always,
        forms: &[
            TermForm::Word("исследователь", "проводник"),
            TermForm::Word("исследователя", "проводника"),
            TermForm::Word("исследователю", "проводнику"),
            TermForm::Word("исследователем", "проводником"),
            TermForm::Word("исследователе", "проводнике"),
        ],
        collapse_repeats: false,
    },
    // Lock Screen — «экран блокировки» (KDE ru, kscreenlocker; the same in
    // Russian Windows). A lone «Lock Screen» the model returns as «экран
    // замка» (measured 06.11).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["lock"],
        gate: |s| s.contains("lock screen") || s.contains("lockscreen"),
        forms: &[
            TermForm::Phrase("экран замка", "экран блокировки"),
            TermForm::Phrase("экрана замка", "экрана блокировки"),
            TermForm::Phrase("экране замка", "экране блокировки"),
            TermForm::Phrase("экраном замка", "экраном блокировки"),
        ],
        collapse_repeats: false,
    },
    // Start Menu — «меню «Пуск»» (Russian Windows; the model translates the
    // «Start» button as «Пуск» anyway). The model writes «Начало Меню»
    // (measured 06.11).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["start"],
        gate: |s| s.contains("start menu"),
        forms: &[
            TermForm::Phrase("начало меню", "меню «Пуск»"),
            TermForm::Phrase("меню начала", "меню «Пуск»"),
            TermForm::Phrase("стартовое меню", "меню «Пуск»"),
        ],
        collapse_repeats: false,
    },
    // Logon — «вход в систему» (as in Russian Windows: «Экран входа в
    // систему»). The model does not know a lone «Logon» and invents «Лофон»
    // (measured 06.11) — it is the invention we replace.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["logon"],
        gate: always,
        forms: &[
            TermForm::Word("лофон", "вход в систему"),
            TermForm::Word("логон", "вход в систему"),
        ],
        collapse_repeats: false,
    },
    // System Tray — «системный лоток»: that is what KDE and Rexon itself call
    // the panel («🔔 Лоток: значок поставлен»). The live model reverses the
    // word order — «Лоток системы Windows XP» (measured 06.11); the word is
    // right, the order is not.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["tray"],
        gate: |s| s.contains("system tray"),
        forms: &[
            TermForm::Phrase("лоток системы", "системный лоток"),
            TermForm::Phrase("лотка системы", "системного лотка"),
            TermForm::Phrase("лотке системы", "системном лотке"),
            TermForm::Phrase("лотком системы", "системным лотком"),
        ],
        collapse_repeats: false,
    },
    // ── stable look and GitHub terms (live check 08.11, «Candy icons»:
    // «икона темы», «Записка», «открыть вопрос», «вытащить запрос»). The
    // source of terms is our po/ru.po and KDE ru: «тема значков» («Checking
    // the icon theme...» → «Проверка темы значков...»), «набор значков»,
    // «тема курсоров» («cursor themes» → «тем курсоров»).
    //
    // icon / icons — «значки». Plural forms only: they have no gender, and
    // agreeing words need not change («иконки» feminine → «значки» masculine
    // are the same in the plural). «Иконописная тема», «икона темы» — whole
    // phrases the engine produces for `icon theme`.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["icon"],
        gate: always,
        forms: &[
            TermForm::Phrase("икона темы", "тема значков"),
            TermForm::Phrase("тема иконы", "тема значков"),
            TermForm::Phrase("икона тема", "тема значков"),
            TermForm::Phrase("иконка темы", "тема значков"),
            TermForm::Phrase("иконка тема", "тема значков"),
            TermForm::Phrase("иконописная тема", "тема значков"),
            TermForm::Phrase("иконописной темы", "темы значков"),
            TermForm::Phrase("иконописную тему", "тему значков"),
            TermForm::Phrase("иконописной темой", "темой значков"),
            TermForm::Phrase("иконописные темы", "темы значков"),
            TermForm::Phrase("иконописных тем", "тем значков"),
            TermForm::Phrase("иконная тема", "тема значков"),
            TermForm::Phrase("иконной темы", "темы значков"),
            TermForm::Phrase("иконную тему", "тему значков"),
            TermForm::Word("иконок", "значков"),
            TermForm::Word("иконкам", "значкам"),
            TermForm::Word("иконками", "значками"),
            TermForm::Word("иконках", "значках"),
            TermForm::Word("икон", "значков"),
        ],
        collapse_repeats: false,
    },
    // «иконки» is both nominative plural and genitive singular («цвет
    // иконки»). So only where the source has a single `icons` with no
    // singular `icon`: «To request for icons please» → «запросить значки».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["icons"],
        gate: |s| !s.split(|c: char| !c.is_alphanumeric()).any(|w| w == "icon"),
        forms: &[TermForm::Word("иконки", "значки")],
        collapse_repeats: false,
    },
    // «theme» — the repeat «тема темы» (this is exactly how the live model
    // returns a lone `theme`, measured 08.11).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["theme"],
        gate: always,
        forms: &[
            TermForm::Phrase("тема темы", "тема"),
            TermForm::Phrase("темы темы", "темы"),
        ],
        collapse_repeats: false,
    },
    // cursor theme — «тема курсоров» (po/ru.po: «GitHub: %1 cursor themes…»
    // → «тем курсоров»). The model writes «курсорная тема».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["cursor"],
        gate: |s| s.contains("cursor theme") || s.contains("cursors theme"),
        forms: &[
            TermForm::Phrase("курсорная тема", "тема курсоров"),
            TermForm::Phrase("курсорной темы", "темы курсоров"),
            TermForm::Phrase("курсорную тему", "тему курсоров"),
            TermForm::Phrase("курсорной темой", "темой курсоров"),
            TermForm::Phrase("курсорной теме", "теме курсоров"),
            TermForm::Phrase("курсорные темы", "темы курсоров"),
            TermForm::Phrase("курсорных тем", "тем курсоров"),
        ],
        collapse_repeats: false,
    },
    // Note / NOTE at the start of a phrase — the label «Примечание» (KDE ru).
    // The model gives «записка», «заметка», and for `NOTE` — «НОТЕ
    // ПРИМЕЧАНИЕ» (measured 08.11).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["note"],
        gate: |s| {
            let t = s.trim_start_matches(|c: char| !c.is_alphanumeric());
            t.strip_prefix("note").is_some_and(|r| !r.starts_with(char::is_alphanumeric))
        },
        forms: &[
            TermForm::Word("записка", "примечание"),
            TermForm::Word("заметка", "примечание"),
            TermForm::Word("ноте", "примечание"),
        ],
        collapse_repeats: true,
    },
    // issue — «заявка» (a project tracker): «open an issue» the model returns
    // as «открыть вопрос»/«открывать проблему», «issue tracker» — «трекер
    // вопросов».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["issue"],
        gate: always,
        forms: &[
            TermForm::Phrase("открыть вопрос", "создать заявку"),
            TermForm::Phrase("открыть проблему", "создать заявку"),
            TermForm::Phrase("открыть выпуск", "создать заявку"),
            TermForm::Phrase("открывать вопрос", "создавать заявку"),
            TermForm::Phrase("открывать проблему", "создавать заявку"),
            TermForm::Phrase("откройте вопрос", "создайте заявку"),
            TermForm::Phrase("откройте проблему", "создайте заявку"),
            TermForm::Phrase("откройте выпуск", "создайте заявку"),
            TermForm::Phrase("трекер вопросов", "трекер заявок"),
            TermForm::Phrase("трекере вопросов", "трекере заявок"),
            TermForm::Phrase("трекер проблем", "трекер заявок"),
            TermForm::Phrase("трекере проблем", "трекере заявок"),
        ],
        collapse_repeats: false,
    },
    // pull request — «запрос на слияние» (that is what Russian GitLab and KDE
    // ru call it). The model: «вытащить запрос», «вытащитные запросы».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["pull"],
        gate: |s| s.contains("pull request") || s.contains("pull-request") || s.contains("pullrequest"),
        forms: &[
            TermForm::Phrase("вытащить запрос", "запрос на слияние"),
            TermForm::Phrase("вытащить запросы", "запросы на слияние"),
            TermForm::Phrase("вытащитные запросы", "запросы на слияние"),
            TermForm::Phrase("вытащитный запрос", "запрос на слияние"),
            TermForm::Phrase("запрос на вытягивание", "запрос на слияние"),
            TermForm::Phrase("запросы на вытягивание", "запросы на слияние"),
            TermForm::Phrase("запрос на извлечение", "запрос на слияние"),
            TermForm::Phrase("запросы на извлечение", "запросы на слияние"),
            TermForm::Phrase("тянуть запрос", "запрос на слияние"),
        ],
        collapse_repeats: false,
    },
    // star (verb) — «отметьте звёздочкой»: «Please star the repository» →
    // «отведайте репозиторий» (measured 08.11). The object's case with
    // «отметьте» is the same (accusative).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["star"],
        gate: |s| ["star the", "star this", "star it", "star my", "star our"].iter().any(|p| s.contains(p)),
        forms: &[
            TermForm::Word("отведайте", "отметьте звёздочкой"),
            TermForm::Word("отведай", "отметьте звёздочкой"),
            TermForm::Phrase("звездите", "отметьте звёздочкой"),
        ],
        collapse_repeats: false,
    },
    // plasma widget — «виджет Plasma» (po/ru.po: «Виджеты рабочего стола»,
    // the name Plasma in Latin). The model: «плазменный виджет».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["plasma", "widget"],
        gate: |s| s.contains("plasma widget") || s.contains("plasma 6 widget") || s.contains("plasma 5 widget"),
        forms: &[
            TermForm::Phrase("плазменный виджет", "виджет Plasma"),
            TermForm::Phrase("плазменного виджета", "виджета Plasma"),
            TermForm::Phrase("плазменному виджету", "виджету Plasma"),
            TermForm::Phrase("плазменным виджетом", "виджетом Plasma"),
            TermForm::Phrase("плазменном виджете", "виджете Plasma"),
            TermForm::Phrase("плазменные виджеты", "виджеты Plasma"),
            TermForm::Phrase("плазменных виджетов", "виджетов Plasma"),
            TermForm::Phrase("плазменными виджетами", "виджетами Plasma"),
        ],
        collapse_repeats: false,
    },
];

/// A word lowercased and with `ё` → `е` — that is how the dictionary compares.
fn term_key(word: &str) -> String {
    word.to_lowercase().replace('ё', "е")
}

/// Replacement with the case of the found word's first letter (`Извлеките` → `Распакуйте`).
fn with_case_of(found: &str, replacement: &str) -> String {
    let upper = found.chars().next().is_some_and(char::is_uppercase);
    if !upper {
        return replacement.to_string();
    }
    let mut chars = replacement.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Words of a line: (start, end) in bytes, consecutive letters.
fn letter_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_alphabetic() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, i));
        }
    }
    if let Some(s) = start {
        runs.push((s, text.len()));
    }
    runs
}

/// Only spaces between words — so these are consecutive words of one phrase.
fn only_spaces(gap: &str) -> bool {
    gap.chars().all(|c| c == ' ' || c == '\u{a0}')
}

/// Try one form starting at word `k`: how many words were consumed and the replacement.
fn match_term_form(
    text: &str,
    runs: &[(usize, usize)],
    k: usize,
    form: &TermForm,
) -> Option<(usize, String)> {
    let word = &text[runs[k].0..runs[k].1];
    let key = term_key(word);
    let prev_words = |n: usize| -> Vec<String> {
        runs[..k].iter().rev().take(n).map(|(s, e)| term_key(&text[*s..*e])).collect()
    };
    match *form {
        TermForm::Word(wrong, right) => {
            (key == term_key(wrong)).then(|| (1, with_case_of(word, right)))
        }
        TermForm::Imperative(wrong, right) => (key == term_key(wrong)
            && !prev_words(3).iter().any(|w| w == "вы"))
        .then(|| (1, with_case_of(word, right))),
        TermForm::AfterPreposition(wrong, right) => {
            let after_prep = k > 0
                && only_spaces(&text[runs[k - 1].1..runs[k].0])
                && PREPOSITIONS.contains(&prev_words(1)[0].as_str());
            (key == term_key(wrong) && after_prep).then(|| (1, with_case_of(word, right)))
        }
        TermForm::ParticipleStem(wrong, right) => {
            let ending = key.strip_prefix(&term_key(wrong))?;
            PARTICIPLE_ENDINGS
                .contains(&ending)
                .then(|| (1, with_case_of(word, &format!("{right}{ending}"))))
        }
        TermForm::Phrase(wrong, right) => {
            let parts: Vec<String> = wrong.split(' ').map(term_key).collect();
            if k + parts.len() > runs.len() {
                return None;
            }
            for (j, part) in parts.iter().enumerate() {
                let (s, e) = runs[k + j];
                if term_key(&text[s..e]) != *part {
                    return None;
                }
                if j > 0 && !only_spaces(&text[runs[k + j - 1].1..s]) {
                    return None;
                }
            }
            Some((parts.len(), with_case_of(word, right)))
        }
    }
}

/// The group's right forms — what is collapsed on a glued repeat.
fn right_forms(fix: &TermFix) -> Vec<String> {
    fix.forms
        .iter()
        .filter_map(|f| match *f {
            TermForm::Word(_, r) | TermForm::Imperative(_, r) => Some(term_key(r)),
            _ => None,
        })
        .collect()
}

/// Apply one group to the translation.
fn apply_term_fix(fix: &TermFix, translated: &str) -> String {
    let runs = letter_runs(translated);
    let mut out = String::with_capacity(translated.len());
    let mut copied = 0usize;
    let mut k = 0usize;
    while k < runs.len() {
        let found = fix.forms.iter().find_map(|f| match_term_form(translated, &runs, k, f));
        match found {
            Some((taken, replacement)) => {
                out.push_str(&translated[copied..runs[k].0]);
                out.push_str(&replacement);
                copied = runs[k + taken - 1].1;
                k += taken;
            }
            None => k += 1,
        }
    }
    out.push_str(&translated[copied..]);
    if !fix.collapse_repeats {
        return out;
    }
    // A glued repeat of the right form: «скачать скачать» → «скачать».
    let rights = right_forms(fix);
    let runs = letter_runs(&out);
    let mut collapsed = String::with_capacity(out.len());
    let mut copied = 0usize;
    for pair in runs.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let first = term_key(&out[a.0..a.1]);
        if b.0 >= copied
            && only_spaces(&out[a.1..b.0])
            && first == term_key(&out[b.0..b.1])
            && rights.contains(&first)
        {
            collapsed.push_str(&out[copied..a.1]);
            copied = b.1;
        }
    }
    collapsed.push_str(&out[copied..]);
    collapsed
}

/// Fix the translation's terms by the [`TERM_FIXES`] dictionary.
///
/// `source` — the source sentence (English), `translated` — the engine's
/// answer for it. A group fires only if its English term is in `source` and
/// it passes its condition; otherwise the translation is not touched. A
/// language pair without groups (for now everything except `en` → `ru`) is
/// returned as is.
pub fn apply_term_fixes(src: &str, tgt: &str, source: &str, translated: &str) -> String {
    let lower = source.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    let mut out = translated.to_string();
    for fix in TERM_FIXES.iter().filter(|f| f.src == src && f.tgt == tgt) {
        let has_term =
            fix.source_words.iter().any(|t| words.iter().any(|w| w.starts_with(t)));
        if has_term && (fix.gate)(&lower) {
            out = apply_term_fix(fix, &out);
        }
    }
    out
}

/// A code environment tag (`code`, `pre`, `kbd`, `samp`, `tt`), case-insensitive.
pub fn is_code_env_tag(name: &str) -> bool {
    let lower = name.to_lowercase();
    CODE_ENV_TAGS.contains(&lower.as_str())
}

/// Parse a literal as a tag: name, whether closing, whether self-closing.
/// [`segment_html`] returns not only tags as literals (also entities,
/// comments and whitespace nodes) — for those `None` is returned.
fn parse_literal_tag(lit: &str) -> Option<(String, bool, bool)> {
    let body = lit.strip_prefix('<')?;
    let (closing, rest) = match body.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, body),
    };
    if !rest.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    let name: String =
        rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let self_closing = lit.trim_end().trim_end_matches('>').trim_end().ends_with('/');
    Some((name, closing, self_closing))
}

/// Code environment stack: we feed it literals in order and ask whether we
/// are inside code or not. Nothing is translated inside
/// `code`/`pre`/`kbd`/`samp`/`tt`.
#[derive(Debug, Clone, Default)]
pub struct CodeEnvStack {
    stack: Vec<String>,
}

impl CodeEnvStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Account for a literal (a tag). Non-tags are ignored.
    pub fn feed_literal(&mut self, literal: &str) {
        let Some((name, closing, self_closing)) = parse_literal_tag(literal) else {
            return;
        };
        if !is_code_env_tag(&name) {
            return;
        }
        if closing {
            if let Some(pos) = self.stack.iter().rposition(|n| *n == name) {
                self.stack.truncate(pos);
            }
        } else if !self_closing {
            self.stack.push(name);
        }
    }

    /// Are we inside a code environment?
    pub fn in_code(&self) -> bool {
        !self.stack.is_empty()
    }
}

/// For each segment — whether to call the engine. `Literal` is always
/// `false`; `Text` — only if it is not inside a code environment and is
/// recognized as prose AS A WHOLE.
pub fn translatable_flags(segments: &[Segment]) -> Vec<bool> {
    flags_with(segments, &|text| is_prose_segment(text))
}

/// The same, but prose is also decided BY SENTENCE: a segment qualifies if it
/// is prose itself OR at least one of its sentences is.
///
/// ⭐ WHY. Acceptance 24.09, maxkaspar's comment on «Awesome Widgets»:
/// «hddtemp shows only the first digit. here is a screenshot http://…». The
/// first phrase is pure prose, the second contains a link, and rule 4 of
/// [`is_prose_segment`] («at least one meaningful word is technical by form»)
/// rejected the WHOLE node — the person saw an English comment without any
/// explanation. The rule itself is right: a link must not be broken. What was
/// wrong was the UNIT of decision — a whole text node, although the text goes
/// to the engine by sentences anyway ([`crate::translator_engine`]). Now the
/// node passes when it has prose, and the fate of each sentence is decided by
/// the same rule: links, paths and commands still do not reach the engine and
/// stay verbatim in the output. (Since 03.10 rule 4 no longer rejects a
/// phrase for a link inside — the link goes under a placeholder, see
/// [`technical_spans`]; the unit of decision is still the sentence.)
///
/// Sentence boundaries are given by the CALLER — with the same splitter that
/// later feeds the engine. No second splitter of its own is set up here: two
/// truths about sentence boundaries would silently diverge.
pub fn translatable_flags_by_sentence(
    segments: &[Segment],
    sentences: &dyn Fn(&str) -> Vec<String>,
) -> Vec<bool> {
    flags_with(segments, &|text| {
        is_prose_segment(text) || sentences(text).iter().any(|s| is_prose_segment(s))
    })
}

// The shared part of both flag sets: the code environment plus the given
// «is it prose» rule.
fn flags_with(segments: &[Segment], is_prose: &dyn Fn(&str) -> bool) -> Vec<bool> {
    let mut env = CodeEnvStack::new();
    let mut flags = Vec::with_capacity(segments.len());
    for s in segments {
        match s {
            Segment::Literal { text } => {
                env.feed_literal(text);
                flags.push(false);
            }
            Segment::Text { stripped, .. } => {
                flags.push(!env.in_code() && is_prose(stripped));
            }
        }
    }
    flags
}

/// [`render_segments`] with a filter: the engine is called only on prose
/// segments, other text nodes stay the original (the callback is given an
/// empty string — exactly what [`render_segments`] understands as «keep as
/// it was», so the reassembly itself did not have to be rewritten).
pub fn render_segments_filtered(
    segments: &[Segment],
    translate: impl Fn(&str) -> String,
) -> String {
    render_segments_with_flags(segments, &translatable_flags(segments), translate)
}

/// The same, but the flags are COMPUTED IN ADVANCE: the assembler must use
/// exactly the flags by which it selected text for the engine
/// ([`translatable_flags_by_sentence`]), otherwise a translated node would
/// stay the original in the output.
pub fn render_segments_with_flags(
    segments: &[Segment],
    flags: &[bool],
    translate: impl Fn(&str) -> String,
) -> String {
    let next = std::cell::Cell::new(0usize);
    render_segments(segments, |s| {
        // render_segments calls the callback exactly once per Segment::Text and
        // strictly in order — the text node index is recovered by a counter.
        let idx = next.get();
        next.set(idx + 1);
        let allow = segments
            .iter()
            .enumerate()
            .filter(|(_, seg)| matches!(seg, Segment::Text { .. }))
            .nth(idx)
            .map(|(i, _)| flags[i])
            .unwrap_or(false);
        if allow {
            translate(s)
        } else {
            String::new()
        }
    })
}

// ── name links ─────────────────────────────────────────────────────────────
//
// ⭐ WHY (live check 08.11, KDE Store, «Candy icons» by eliverlara): «Sweet
// gradient icon theme originally created to fit my <a
// href="https://www.gnome-look.org/p/1253385/">Sweet</a> theme» came out as
// «…чтобы соответствовать моему Сладкий тема темы». The link text is the
// name of a neighbouring item, and HTML cuts the phrase into three nodes, so
// the engine got «Sweet» and «theme» separately. So a link whose text is a
// name ([`is_name_link`]) is glued with neighbouring nodes into ONE phrase
// ([`fold_name_links`]): the engine sees the whole sentence, the name is
// guarded as an item name, and the link tags go back around the surviving
// name ([`NameLinkRun::relink`]). The same yardstick applies to a Markdown
// link `[Sweet](…)` inside prose ([`technical_spans`]).

/// Hosts of item pages of the Pling family (KDE Store and kin): an item page
/// there is always `/p/<number>`.
const STORE_HOSTS: &[&str] = &[
    "store.kde.org", "kde-look.org", "gnome-look.org", "xfce-look.org", "cinnamon-look.org",
    "opendesktop.org", "pling.com", "linux-apps.com", "appimagehub.com",
];

/// Code hosting hosts: a repository page is `/<owner>/<name>`.
const CODE_HOSTS: &[&str] =
    &["github.com", "gitlab.com", "codeberg.org", "invent.kde.org", "gitlab.gnome.org", "framagit.org"];

/// First path parts on code hosts that are never a repository
/// (`github.com/sponsors/eliverlara` is a sponsorship page).
const CODE_HOST_NON_REPO: &[&str] =
    &["sponsors", "orgs", "settings", "marketplace", "topics", "explore", "users", "search"];

/// Ordinary words that occur as the text of a link to an item page but are
/// not a name: «Download», «Here», «Source code»…
const LINK_WORDS_NOT_NAMES: &[&str] = &[
    "here", "download", "downloads", "link", "source", "code", "website", "site", "page",
    "homepage", "repo", "repository", "click", "project", "this", "theme", "themes", "icons",
    "readme", "wiki", "releases", "release", "issues", "issue", "changelog", "version",
];

/// The link leads to an item page (`…/p/1253385/`) or a code repository
/// (`github.com/EliverLara/Sweet`), not to a section, an issue or a payment.
pub fn is_product_page_url(href: &str) -> bool {
    let lower = href.trim().to_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.strip_prefix("www.").unwrap_or(host);
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if STORE_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
        return parts
            .windows(2)
            .any(|w| w[0] == "p" && w[1].chars().all(|c| c.is_ascii_digit()) && !w[1].is_empty());
    }
    if CODE_HOSTS.contains(&host) {
        return parts.len() == 2 && !CODE_HOST_NON_REPO.contains(&parts[0]);
    }
    false
}

/// The link text looks like a name: one to four words, each Capitalized (or a
/// digit), or a slug with hyphens/underscores (`sweet-folders`), and not a
/// single ordinary word from [`LINK_WORDS_NOT_NAMES`]. «open an issue»,
/// «Become a sponsor on github», «pull request» are prose, not names.
fn is_name_shaped_link_text(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || words.len() > 4 || !text.chars().any(char::is_alphabetic) {
        return false;
    }
    words.iter().all(|w| {
        let core = trim_token_punct(w);
        if core.is_empty() || LINK_WORDS_NOT_NAMES.contains(&core.to_lowercase().as_str()) {
            return false;
        }
        let first = core.chars().next().unwrap_or(' ');
        let slug = (core.contains('-') || core.contains('_'))
            && core.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        first.is_uppercase() || first.is_ascii_digit() || slug
    })
}

/// The link is a proper name: the text looks like a name and leads to an item
/// or repository page. Such text is never translated.
pub fn is_name_link(text: &str, href: &str) -> bool {
    is_name_shaped_link_text(text) && is_product_page_url(href)
}

/// The address from the opening `<a …>` tag (attribute name case-insensitive).
pub fn link_href(open_tag: &str) -> Option<String> {
    let (name, closing, _) = parse_literal_tag(open_tag)?;
    if name != "a" || closing {
        return None;
    }
    let lower = open_tag.to_lowercase();
    let at = lower.find("href")?;
    let rest = open_tag[at + 4..].trim_start().strip_prefix('=')?.trim_start();
    let value = match rest.chars().next()? {
        q @ ('"' | '\'') => rest[1..].split(q).next()?,
        _ => rest.split(|c: char| c.is_whitespace() || c == '>').next()?,
    };
    Some(html_unescape(value))
}

/// Markdown links `[text](address)` with name text ([`is_name_link`]):
/// (start, end) in bytes — the whole piece together with the brackets.
pub fn markdown_name_links(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(open) = text[from..].find('[').map(|i| from + i) {
        let Some(close) = text[open..].find("](").map(|i| open + i) else { break };
        let label = &text[open + 1..close];
        let Some(end) = text[close + 2..].find(')').map(|i| close + 2 + i) else { break };
        let href = &text[close + 2..end];
        if !label.contains('[') && !href.contains(char::is_whitespace) && is_name_link(label, href) {
            out.push((open, end + 1));
            from = end + 1;
        } else {
            from = open + 1;
        }
    }
    out
}

/// A piece of a glued phrase: ordinary text or a name link with its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkPart {
    Text(String),
    Link { open: String, name: String, close: String },
}

/// A phrase glued through name links: its pieces in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameLinkRun {
    pub parts: Vec<LinkPart>,
}

impl NameLinkRun {
    /// What the engine sees: links as a bare name.
    pub fn plain(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                LinkPart::Text(t) => t.as_str(),
                LinkPart::Link { name, .. } => name.as_str(),
            })
            .collect()
    }

    /// The original markup verbatim — in case there is no translation.
    pub fn markup(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                LinkPart::Text(t) => t.clone(),
                LinkPart::Link { open, name, close } => format!("{open}{name}{close}"),
            })
            .collect()
    }

    /// Link names in order.
    pub fn names(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                LinkPart::Link { name, .. } => Some(name.trim()),
                LinkPart::Text(_) => None,
            })
            .collect()
    }

    /// Put the link tags back into the translation: each name (in order, as a
    /// separate word) is framed by its tags. A name not found — `None`.
    ///
    /// ⚠️ The link name may also stand in the phrase without a link: «Sweet
    /// gradient icon theme … to fit my <a>Sweet</a> theme» — the link is on the
    /// SECOND «Sweet». So the occurrence with the same number as the link's in
    /// the source phrase is taken; if the engine changed the number of
    /// occurrences — the nearest by position in the phrase.
    pub fn relink(&self, translated: &str) -> Option<String> {
        let plain = self.plain();
        let mut out = String::with_capacity(translated.len() + 64);
        let mut cursor = 0usize;
        let mut plain_at = 0usize;
        for part in &self.parts {
            let (open, name, close) = match part {
                LinkPart::Text(t) => {
                    plain_at += t.len();
                    continue;
                }
                LinkPart::Link { open, name, close } => (open, name, close),
            };
            let here = plain_at + (name.len() - name.trim_start().len());
            plain_at += name.len();
            let name = name.trim();
            let in_plain = free_word_positions(&plain, name);
            let in_tgt: Vec<usize> =
                free_word_positions(translated, name).into_iter().filter(|p| *p >= cursor).collect();
            let rank = in_plain.iter().position(|p| *p == here)?;
            let at = if in_plain.len() == free_word_positions(translated, name).len() {
                *free_word_positions(translated, name).get(rank).filter(|p| **p >= cursor)?
            } else {
                // The link's place in the source phrase — a fraction of the length; the nearest is taken.
                let want = here as f64 / plain.len().max(1) as f64;
                *in_tgt.iter().min_by(|a, b| {
                    let da = (**a as f64 / translated.len().max(1) as f64 - want).abs();
                    let db = (**b as f64 / translated.len().max(1) as f64 - want).abs();
                    da.total_cmp(&db)
                })?
            };
            out.push_str(&translated[cursor..at]);
            out.push_str(open);
            out.push_str(name);
            out.push_str(close);
            cursor = at + name.len();
        }
        out.push_str(&translated[cursor..]);
        Some(out)
    }
}

/// All occurrences of `word` as a separate word (boundaries — not a letter and not a digit).
fn free_word_positions(text: &str, word: &str) -> Vec<usize> {
    let mut found = Vec::new();
    if word.is_empty() {
        return found;
    }
    let mut from = 0usize;
    while let Some(i) = text[from..].find(word) {
        let start = from + i;
        let end = start + word.len();
        let before = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            found.push(start);
        }
        from = start + word.chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// Glue phrases through name links.
///
/// `text <a href=…>Name</a> text` (node, tag, name node, tag, node) becomes
/// ONE text node `text Name text`; next to it, at the same place of the
/// second list, lies a [`NameLinkRun`] — by it the assembler will put the
/// tags back ([`NameLinkRun::relink`]) or, without a translation, the
/// original markup ([`NameLinkRun::markup`]). A slug (`sweet-folders`) is not
/// glued. A name link without neighbouring nodes stays as it was:
/// its text is not translated anyway ([`translatable_flags_with_links`]).
/// Nothing is glued inside a code environment.
pub fn fold_name_links(segments: Vec<Segment>) -> (Vec<Segment>, Vec<Option<NameLinkRun>>) {
    // A name link starting at `i`: (opening tag, name node, closing tag).
    let name_link_at = |segs: &[Segment], i: usize| -> Option<(String, String, String)> {
        let (Segment::Literal { text: open }, Some(Segment::Text { lead, stripped, trail }), Some(Segment::Literal { text: close })) =
            (&segs[i], segs.get(i + 1), segs.get(i + 2))
        else {
            return None;
        };
        let href = link_href(open)?;
        let closes_a = matches!(parse_literal_tag(close), Some((n, true, _)) if n == "a");
        // A slug (`sweet-folders`) is not glued: under a placeholder the live
        // model chops the phrase at it («…представленных в sweet-folders
        // виде.» instead of «…как показано на втором скриншоте», measured
        // 08.11), and a lone slug is not translated anyway.
        let slug = stripped.contains('-') || stripped.contains('_');
        (closes_a && !slug && is_name_link(stripped, &href))
            .then(|| (open.clone(), format!("{lead}{stripped}{trail}"), close.clone()))
    };

    let mut out: Vec<Segment> = Vec::with_capacity(segments.len());
    let mut runs: Vec<Option<NameLinkRun>> = Vec::with_capacity(segments.len());
    let mut env = CodeEnvStack::new();
    let mut i = 0usize;
    while i < segments.len() {
        let link = if env.in_code() { None } else { name_link_at(&segments, i) };
        let Some((open, name, close)) = link else {
            if let Segment::Literal { text } = &segments[i] {
                env.feed_literal(text);
            }
            out.push(segments[i].clone());
            runs.push(None);
            i += 1;
            continue;
        };
        let before = matches!(out.last(), Some(Segment::Text { .. }));
        let after = matches!(segments.get(i + 3), Some(Segment::Text { .. }));
        if !before && !after {
            out.extend(segments[i..i + 3].iter().cloned());
            runs.extend([None, None, None]);
            i += 3;
            continue;
        }
        // Start of gluing: the previous node (or an already glued phrase) or nothing.
        let (lead, mut run) = match (before, out.last().cloned()) {
            (true, Some(Segment::Text { lead, stripped, trail })) => {
                out.pop();
                let prev = runs.pop().flatten();
                let run = match prev {
                    Some(mut run) => {
                        run.parts.push(LinkPart::Text(trail));
                        run
                    }
                    None => NameLinkRun { parts: vec![LinkPart::Text(format!("{stripped}{trail}"))] },
                };
                (lead, run)
            }
            _ => (String::new(), NameLinkRun::default()),
        };
        run.parts.push(LinkPart::Link { open, name, close });
        let mut trail = String::new();
        i += 3;
        if let Some(Segment::Text { lead: l, stripped: s, trail: t }) = segments.get(i) {
            run.parts.push(LinkPart::Text(format!("{l}{s}")));
            trail = t.clone();
            i += 1;
        }
        // Glued node: `stripped` — what the engine will see (without edge
        // spaces), the spaces themselves go to `lead`/`trail`, like an ordinary node.
        let plain = run.plain();
        let body = plain.trim();
        let inner_lead = &plain[..plain.len() - plain.trim_start().len()];
        let inner_trail = &plain[plain.trim_end().len()..];
        // The phrase's edge spaces are already inside the pieces: trim them there too.
        trim_run_edges(&mut run);
        out.push(Segment::Text {
            lead: format!("{lead}{inner_lead}"),
            stripped: body.to_string(),
            trail: format!("{inner_trail}{trail}"),
        });
        runs.push(Some(run));
    }
    (out, runs)
}

/// Trim the edge spaces of a glued phrase in its first and last piece — they
/// are moved to the node's `lead`/`trail`.
fn trim_run_edges(run: &mut NameLinkRun) {
    if let Some(LinkPart::Text(t)) = run.parts.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(LinkPart::Text(t)) = run.parts.last_mut() {
        *t = t.trim_end().to_string();
    }
}

/// «Call the engine» flags for glued nodes: like
/// [`translatable_flags_by_sentence`], plus the text of a name link left
/// without neighbours is never prose.
pub fn translatable_flags_with_links(
    segments: &[Segment],
    sentences: &dyn Fn(&str) -> Vec<String>,
) -> Vec<bool> {
    let mut flags = translatable_flags_by_sentence(segments, sentences);
    for i in 0..segments.len().saturating_sub(2) {
        if let (Segment::Literal { text: open }, Segment::Text { stripped, .. }) =
            (&segments[i], &segments[i + 1])
        {
            if link_href(open).is_some_and(|href| is_name_link(stripped, &href)) {
                flags[i + 1] = false;
            }
        }
    }
    flags
}

/// Assembling a string from glued nodes: the translation of a glued phrase
/// gets the link tags back, and without a translation (or if the name did not
/// survive) the original markup is put. The rest is like
/// [`render_segments_with_flags`].
pub fn render_folded(
    segments: &[Segment],
    flags: &[bool],
    runs: &[Option<NameLinkRun>],
    translate: impl Fn(&str) -> String,
) -> String {
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            Segment::Literal { text } => out.push_str(text),
            Segment::Text { lead, stripped, trail } => {
                let res = if flags[i] { translate(stripped) } else { String::new() };
                out.push_str(lead);
                match &runs[i] {
                    Some(run) => {
                        let linked = (!res.is_empty()).then(|| run.relink(&res)).flatten();
                        out.push_str(&linked.unwrap_or_else(|| run.markup()));
                    }
                    None if res.is_empty() => out.push_str(stripped),
                    None => out.push_str(&res),
                }
                out.push_str(trail);
            }
        }
    }
    out
}

// Whitespace characters `[\t\n\r\f ]` from html.parser regexes (the ASCII set).
fn is_html_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{0c}' | ' ')
}

/// Port of `html.parser.HTMLParser` with `convert_charrefs=False`, narrowed to
/// the handlers `_HTMLTextTranslator` overrides.
struct Segmenter {
    data: Vec<char>,
    n: usize,
    i: usize,
    out: Vec<Segment>,
    // RAWTEXT/RCDATA: if inside script/style/… — the element name and «escapable».
    cdata_elem: Option<String>,
    escapable: bool,
    support_cdata: bool,
}

const CDATA_CONTENT_ELEMENTS: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];
const RCDATA_CONTENT_ELEMENTS: &[&str] = &["textarea", "title"];

impl Segmenter {
    fn new(text: &str) -> Self {
        let data: Vec<char> = text.chars().collect();
        let n = data.len();
        Segmenter { data, n, i: 0, out: Vec::new(), cdata_elem: None, escapable: true, support_cdata: true }
    }

    fn slice(&self, a: usize, b: usize) -> String {
        self.data[a..b].iter().collect()
    }

    fn starts_with(&self, pat: &str, at: usize) -> bool {
        let pc: Vec<char> = pat.chars().collect();
        if at + pc.len() > self.n {
            return false;
        }
        self.data[at..at + pc.len()] == pc[..]
    }

    // rawdata[i:i+len].lower() == pat (pat in lowercase).
    fn starts_with_ci(&self, pat: &str, at: usize) -> bool {
        let pc: Vec<char> = pat.chars().collect();
        if at + pc.len() > self.n {
            return false;
        }
        self.data[at..at + pc.len()]
            .iter()
            .zip(&pc)
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
    }

    // ── handlers (matching the _HTMLTextTranslator overrides) ───────────────
    fn handle_data(&mut self, s: &str) {
        let chars: Vec<char> = s.chars().collect();
        // stripped = data.strip(); if not stripped: return data (Literal).
        let first = chars.iter().position(|c| !c.is_whitespace());
        match first {
            None => self.out.push(Segment::Literal { text: s.to_string() }),
            Some(f) => {
                let last = chars.iter().rposition(|c| !c.is_whitespace()).unwrap();
                let lead: String = chars[..f].iter().collect();
                let stripped: String = chars[f..=last].iter().collect();
                let trail: String = chars[last + 1..].iter().collect();
                self.out.push(Segment::Text { lead, stripped, trail });
            }
        }
    }

    fn handle_starttag_text(&mut self, start: usize, end: usize) {
        // get_starttag_text() — the verbatim tag text (for starttag and startendtag).
        self.out.push(Segment::Literal { text: self.slice(start, end) });
    }

    fn handle_endtag(&mut self, tag: &str) {
        self.out.push(Segment::Literal { text: format!("</{tag}>") });
    }

    fn handle_entityref(&mut self, name: &str) {
        self.out.push(Segment::Literal { text: format!("&{name};") });
    }

    fn handle_charref(&mut self, name: &str) {
        self.out.push(Segment::Literal { text: format!("&#{name};") });
    }

    fn handle_comment(&mut self, data: &str) {
        self.out.push(Segment::Literal { text: format!("<!--{data}-->") });
    }
    // handle_decl / handle_pi / unknown_decl — the subclass does NOT override them →
    // <!DOCTYPE>/<?..?>/<![CDATA[..]]> nodes drop out (we add nothing).

    fn set_cdata_mode(&mut self, elem: &str, escapable: bool) {
        self.cdata_elem = Some(elem.to_ascii_lowercase());
        self.escapable = escapable;
    }

    fn clear_cdata_mode(&mut self) {
        self.cdata_elem = None;
        self.escapable = true;
    }

    // Find the next «interesting» character from `from` (analogue of interesting.search).
    fn interesting_search(&self, from: usize) -> Option<usize> {
        match &self.cdata_elem {
            None => (from..self.n).find(|&p| matches!(self.data[p], '&' | '<')),
            Some(elem) => {
                if elem == "plaintext" {
                    return None; // interesting = \z
                }
                (from..self.n).find(|&p| {
                    (self.escapable && self.data[p] == '&') || self.matches_close_tag(p, elem)
                })
            }
        }
    }

    // </elem(?=[\t\n\r\f />]) case-insensitive.
    fn matches_close_tag(&self, p: usize, elem: &str) -> bool {
        if self.data[p] != '<' || p + 1 >= self.n || self.data[p + 1] != '/' {
            return false;
        }
        let ec: Vec<char> = elem.chars().collect();
        let start = p + 2;
        if start + ec.len() > self.n {
            return false;
        }
        for (k, e) in ec.iter().enumerate() {
            if self.data[start + k].to_ascii_lowercase() != *e {
                return false;
            }
        }
        let after = start + ec.len();
        after < self.n && (is_html_ws(self.data[after]) || matches!(self.data[after], '/' | '>'))
    }

    fn goahead(&mut self, end: bool) {
        while self.i < self.n {
            let i = self.i;
            let j = match self.interesting_search(i) {
                Some(m) => m,
                None => {
                    if self.cdata_elem.is_some() {
                        break;
                    }
                    self.n
                }
            };
            if i < j {
                let d = self.slice(i, j);
                self.handle_data(&d);
            }
            self.i = j;
            if self.i == self.n {
                break;
            }
            let i = self.i;
            if self.data[i] == '<' {
                let mut k: isize;
                if i + 1 < self.n && self.data[i + 1].is_ascii_alphabetic() {
                    k = self.parse_starttag(i);
                } else if self.starts_with("</", i) {
                    k = self.parse_endtag(i);
                } else if self.starts_with("<!--", i) {
                    k = self.parse_comment(i);
                } else if self.starts_with("<?", i) {
                    k = self.parse_pi(i);
                } else if self.starts_with("<!", i) {
                    k = self.parse_html_declaration(i);
                } else if i + 1 < self.n || end {
                    self.handle_data("<");
                    k = (i + 1) as isize;
                } else {
                    break;
                }
                if k < 0 {
                    if !end {
                        break;
                    }
                    self.recover_incomplete(i);
                    k = self.n as isize;
                }
                self.i = k as usize;
            } else if self.starts_with("&#", i) {
                if let Some(mend) = self.charref_match(i) {
                    let name = self.slice(i + 2, mend - 1);
                    self.handle_charref(&name);
                    let k = if self.data[mend - 1] != ';' { mend - 1 } else { mend };
                    self.i = k;
                    continue;
                }
                if self.incomplete_charref_match(i) {
                    if end {
                        let name = self.slice(i + 2, self.n);
                        self.handle_charref(&name);
                        self.i = self.n;
                        break;
                    }
                    break;
                } else if i + 3 < self.n {
                    self.handle_data("&#");
                    self.i = i + 2;
                } else {
                    break;
                }
            } else if self.data[i] == '&' {
                if let Some((g1_end, mend)) = self.entityref_match(i) {
                    let name = self.slice(i + 1, g1_end);
                    self.handle_entityref(&name);
                    let k = if self.data[mend - 1] != ';' { mend - 1 } else { mend };
                    self.i = k;
                    continue;
                }
                if self.incomplete_amp_match(i) {
                    if end {
                        let name = self.slice(i + 1, self.n);
                        self.handle_entityref(&name);
                        self.i = self.n;
                        break;
                    }
                    break;
                } else if i + 1 < self.n {
                    self.handle_data("&");
                    self.i = i + 1;
                } else {
                    break;
                }
            } else {
                // interesting.search() «lied» — unreachable.
                break;
            }
        }
        if end && self.i < self.n {
            let d = self.slice(self.i, self.n);
            self.handle_data(&d);
            self.i = self.n;
        }
    }

    // Recovery on an incomplete «<…» at true EOF (goahead end=true).
    fn recover_incomplete(&mut self, i: usize) {
        let n = self.n;
        if i + 1 < n && self.data[i + 1].is_ascii_alphabetic() {
            // starttagopen — pass.
        } else if self.starts_with("</", i) {
            if i + 2 == n {
                self.handle_data("</");
            } else if i + 2 < n && self.data[i + 2].is_ascii_alphabetic() {
                // endtagopen — pass.
            } else {
                let d = self.slice(i + 2, n);
                self.handle_comment(&d);
            }
        } else if self.starts_with("<!--", i) {
            let mut j = n;
            for suffix in ["--!", "--", "-"] {
                let sc: Vec<char> = suffix.chars().collect();
                if n >= i + 4 + sc.len() && self.data[n - sc.len()..n] == sc[..] {
                    j -= sc.len();
                    break;
                }
            }
            let d = self.slice(i + 4, j);
            self.handle_comment(&d);
        } else if self.starts_with("<![CDATA[", i) && self.support_cdata {
            // unknown_decl — pass.
        } else if self.starts_with_ci("<!doctype", i) {
            // handle_decl — pass.
        } else if self.starts_with("<!", i) {
            let d = self.slice(i + 2, n);
            self.handle_comment(&d);
        } else if self.starts_with("<?", i) {
            // handle_pi — pass.
        }
    }

    // charref = &#(?:[0-9]+|[xX][0-9a-fA-F]+)[^0-9a-fA-F]. Returns the index
    // AFTER the terminating character (match.end()) or None.
    fn charref_match(&self, i: usize) -> Option<usize> {
        if !self.starts_with("&#", i) {
            return None;
        }
        let mut p = i + 2;
        let hex = p < self.n && matches!(self.data[p], 'x' | 'X');
        if hex {
            p += 1;
            let start = p;
            while p < self.n && self.data[p].is_ascii_hexdigit() {
                p += 1;
            }
            if p == start {
                return None; // at least one hex digit is needed
            }
        } else {
            let start = p;
            while p < self.n && self.data[p].is_ascii_digit() {
                p += 1;
            }
            if p == start {
                return None; // at least one digit is needed
            }
        }
        // terminating [^0-9a-fA-F]
        if p < self.n && !self.data[p].is_ascii_hexdigit() {
            Some(p + 1)
        } else {
            None
        }
    }

    // incomplete_charref = &#(?:[0-9]|[xX][0-9a-fA-F]).
    fn incomplete_charref_match(&self, i: usize) -> bool {
        if !self.starts_with("&#", i) {
            return false;
        }
        let p = i + 2;
        if p < self.n && self.data[p].is_ascii_digit() {
            return true;
        }
        if p < self.n && matches!(self.data[p], 'x' | 'X') {
            return p + 1 < self.n && self.data[p + 1].is_ascii_hexdigit();
        }
        false
    }

    // entityref = &([a-zA-Z][-.a-zA-Z0-9]*)[^a-zA-Z0-9]. Returns
    // (end of group1, match.end()) or None.
    fn entityref_match(&self, i: usize) -> Option<(usize, usize)> {
        if self.data.get(i) != Some(&'&') {
            return None;
        }
        let mut p = i + 1;
        if p >= self.n || !self.data[p].is_ascii_alphabetic() {
            return None;
        }
        p += 1;
        while p < self.n && (self.data[p].is_ascii_alphanumeric() || matches!(self.data[p], '-' | '.')) {
            p += 1;
        }
        let g1_end = p;
        // terminating [^a-zA-Z0-9]
        if p < self.n && !self.data[p].is_ascii_alphanumeric() {
            Some((g1_end, p + 1))
        } else {
            None
        }
    }

    // incomplete = &[a-zA-Z#].
    fn incomplete_amp_match(&self, i: usize) -> bool {
        self.data.get(i) == Some(&'&')
            && i + 1 < self.n
            && (self.data[i + 1].is_ascii_alphabetic() || self.data[i + 1] == '#')
    }

    // ── tag parsing ─────────────────────────────────────────────────────────
    fn parse_starttag(&mut self, i: usize) -> isize {
        let endpos = self.locatetagend(i + 1);
        if self.data[endpos - 1] != '>' {
            return -1;
        }
        // tagfind_tolerant: tag name + trailing ws/'/'(not before '>').
        let (tag, mut k) = self.tagfind(i + 1);
        while k < endpos {
            match self.attrfind(k) {
                Some(mend) => k = mend,
                None => break,
            }
        }
        // end = rawdata[k:endpos].strip()
        let end_str: String = self.slice(k, endpos);
        let end_str = end_str.trim();
        if end_str != ">" && end_str != "/>" {
            let d = self.slice(i, endpos);
            self.handle_data(&d);
            return endpos as isize;
        }
        if end_str == "/>" {
            self.handle_starttag_text(i, endpos); // handle_startendtag → the same verbatim
        } else {
            self.handle_starttag_text(i, endpos);
            if CDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) || tag == "plaintext" {
                self.set_cdata_mode(&tag, false);
            } else if RCDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) {
                self.set_cdata_mode(&tag, true);
            }
        }
        endpos as isize
    }

    fn parse_endtag(&mut self, i: usize) -> isize {
        // fast check: is there a '>' after i+2
        if (i + 2..self.n).all(|p| self.data[p] != '>') {
            return -1;
        }
        // endtagopen = </ + letter
        if !(i + 2 < self.n && self.data[i + 2].is_ascii_alphabetic()) {
            if i + 2 < self.n && self.data[i + 2] == '>' {
                return (i + 3) as isize; // </> is ignored
            }
            return self.parse_bogus_comment(i);
        }
        let endpos = self.locatetagend(i + 2);
        if self.data[endpos - 1] != '>' {
            return -1;
        }
        let (tag, _) = self.tagfind(i + 2);
        self.handle_endtag(&tag);
        self.clear_cdata_mode();
        endpos as isize
    }

    fn parse_bogus_comment(&mut self, i: usize) -> isize {
        match (i + 2..self.n).find(|&p| self.data[p] == '>') {
            None => -1,
            Some(pos) => {
                let d = self.slice(i + 2, pos);
                self.handle_comment(&d);
                (pos + 1) as isize
            }
        }
    }

    fn parse_comment(&mut self, i: usize) -> isize {
        // commentclose = --!?>  (search from i+4); otherwise commentabruptclose = -?> (match).
        let mut found: Option<(usize, usize)> = None;
        let mut p = i + 4;
        while p < self.n {
            if self.data[p] == '-' && p + 1 < self.n && self.data[p + 1] == '-' {
                if p + 2 < self.n && self.data[p + 2] == '>' {
                    found = Some((p, p + 3));
                    break;
                }
                if p + 3 < self.n && self.data[p + 2] == '!' && self.data[p + 3] == '>' {
                    found = Some((p, p + 4));
                    break;
                }
            }
            p += 1;
        }
        if found.is_none() {
            // commentabruptclose.match at i+4: -?>
            let q = i + 4;
            if q < self.n && self.data[q] == '>' {
                found = Some((q, q + 1));
            } else if q + 1 < self.n && self.data[q] == '-' && self.data[q + 1] == '>' {
                found = Some((q, q + 2));
            }
        }
        match found {
            None => -1,
            Some((start, endp)) => {
                let d = self.slice(i + 4, start);
                self.handle_comment(&d);
                endp as isize
            }
        }
    }

    fn parse_pi(&mut self, i: usize) -> isize {
        match (i + 2..self.n).find(|&p| self.data[p] == '>') {
            None => -1,
            // handle_pi — not overridden, we add nothing; return end.
            Some(pos) => (pos + 1) as isize,
        }
    }

    fn parse_html_declaration(&mut self, i: usize) -> isize {
        if self.starts_with("<!--", i) {
            return self.parse_comment(i);
        }
        if self.starts_with("<![CDATA[", i) && self.support_cdata {
            // find ]]> from i+9
            let target = [']', ']', '>'];
            let mut p = i + 9;
            while p + 3 <= self.n {
                if self.data[p..p + 3] == target {
                    // unknown_decl — not overridden, we add nothing.
                    return (p + 3) as isize;
                }
                p += 1;
            }
            return -1;
        }
        if self.starts_with_ci("<!doctype", i) {
            match (i + 9..self.n).find(|&p| self.data[p] == '>') {
                None => -1,
                // handle_decl — not overridden, we add nothing.
                Some(gtpos) => (gtpos + 1) as isize,
            }
        } else {
            self.parse_bogus_comment(i)
        }
    }

    // ── regex primitives of tag parsing ────────────────────────────────────
    // tagfind_tolerant = ([a-zA-Z][^\t\n\r\f />]*)(?:[\t\n\r\f ]|/(?!>))*
    // Returns (the name in LOWERCASE, end of match).
    fn tagfind(&self, from: usize) -> (String, usize) {
        let mut p = from;
        // [a-zA-Z] — guaranteed by the caller; then [^\t\n\r\f />]*.
        let name_start = p;
        p += 1;
        while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '>') {
            p += 1;
        }
        let name: String = self.data[name_start..p].iter().collect::<String>().to_lowercase();
        p = self.skip_tag_trailing(p);
        (name, p)
    }

    // (?:[\t\n\r\f ]|/(?!>))* — a space OR a '/' not before '>'.
    fn skip_tag_trailing(&self, mut p: usize) -> usize {
        while p < self.n {
            let c = self.data[p];
            // a space OR a '/' not immediately before '>'.
            if is_html_ws(c) || (c == '/' && !(p + 1 < self.n && self.data[p + 1] == '>')) {
                p += 1;
            } else {
                break;
            }
        }
        p
    }

    // attrfind_tolerant.match(k) → end of match (one attribute name[=value]) or None.
    fn attrfind(&self, k: usize) -> Option<usize> {
        // (?<=['"\t\n\r\f /]) — the character before the attribute name.
        if k == 0 {
            return None;
        }
        let prev = self.data[k - 1];
        if !(prev == '\'' || prev == '"' || is_html_ws(prev) || prev == '/') {
            return None;
        }
        // name: [^\t\n\r\f />] then [^\t\n\r\f /=>]*
        let mut p = k;
        if p >= self.n || is_html_ws(self.data[p]) || matches!(self.data[p], '/' | '>') {
            return None;
        }
        p += 1;
        while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '=' | '>') {
            p += 1;
        }
        // optional value indicator: [ws]*=[ws]*(value)
        let save = p;
        let mut q = p;
        while q < self.n && is_html_ws(self.data[q]) {
            q += 1;
        }
        if q < self.n && self.data[q] == '=' {
            q += 1;
            while q < self.n && is_html_ws(self.data[q]) {
                q += 1;
            }
            match self.match_attr_value(q) {
                Some(vend) => p = vend,
                None => p = save, // the value did not assemble → the indicator does not match
            }
        } else {
            p = save;
        }
        // trailing (?:[\t\n\r\f ]|/(?!>))*
        p = self.skip_tag_trailing(p);
        Some(p)
    }

    // '[^']*' | "[^"]*" | (?!['"])[^>\t\n\r\f ]*
    fn match_attr_value(&self, q: usize) -> Option<usize> {
        if q >= self.n {
            // (?!['"])[^…]* matches empty on an empty tail.
            return Some(q);
        }
        match self.data[q] {
            '\'' => {
                let mut p = q + 1;
                while p < self.n && self.data[p] != '\'' {
                    p += 1;
                }
                if p < self.n {
                    Some(p + 1)
                } else {
                    None // no closing quote — the alternative does not match
                }
            }
            '"' => {
                let mut p = q + 1;
                while p < self.n && self.data[p] != '"' {
                    p += 1;
                }
                if p < self.n {
                    Some(p + 1)
                } else {
                    None
                }
            }
            _ => {
                // (?!['"]) already checked (not a quote); [^>\t\n\r\f ]*
                let mut p = q;
                while p < self.n && !is_html_ws(self.data[p]) && self.data[p] != '>' {
                    p += 1;
                }
                Some(p)
            }
        }
    }

    // locatetagend.match(from) → end of match (greedy, always matches).
    fn locatetagend(&self, from: usize) -> usize {
        let mut p = from;
        // [a-zA-Z]
        if p < self.n && self.data[p].is_ascii_alphabetic() {
            p += 1;
        }
        // [^\t\n\r\f />]*
        while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '>') {
            p += 1;
        }
        // [\t\n\r\f /]*
        while p < self.n && (is_html_ws(self.data[p]) || self.data[p] == '/') {
            p += 1;
        }
        // (?: (?<=['"\t\n\r\f /]) attrname (=value)? [\t\n\r\f /]* )*
        loop {
            if p == 0 {
                break;
            }
            let prev = self.data[p - 1];
            if !(prev == '\'' || prev == '"' || is_html_ws(prev) || prev == '/') {
                break;
            }
            if p >= self.n || is_html_ws(self.data[p]) || matches!(self.data[p], '/' | '>') {
                break;
            }
            // attrname
            p += 1;
            while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '=' | '>') {
                p += 1;
            }
            // (=value)?
            let save = p;
            let mut q = p;
            while q < self.n && is_html_ws(self.data[q]) {
                q += 1;
            }
            if q < self.n && self.data[q] == '=' {
                q += 1;
                while q < self.n && is_html_ws(self.data[q]) {
                    q += 1;
                }
                match self.match_attr_value(q) {
                    Some(vend) => p = vend,
                    None => p = save,
                }
            } else {
                p = save;
            }
            // [\t\n\r\f /]*
            while p < self.n && (is_html_ws(self.data[p]) || self.data[p] == '/') {
                p += 1;
            }
        }
        // >?
        if p < self.n && self.data[p] == '>' {
            p += 1;
        }
        p
    }
}

// ── html.unescape (for probe_text) ─────────────────────────────────────────
//
// Numeric references — fully; named ones — a common set. Since _HTML_TAG_RE
// cuts valid `&…;` BEFORE unescape, the impact is limited.
///
/// Public: the «Appearance» tab parses the text of store comments with the
/// same function. A second entity parser must not appear in the core (rule
/// #12) — if they diverged, one and the same comment would read differently
/// in the item details and in the language detector.
pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // &(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[a-zA-Z][a-zA-Z0-9]*;?)
        if i + 1 < chars.len() && chars[i + 1] == '#' {
            let mut p = i + 2;
            let hex = p < chars.len() && matches!(chars[p], 'x' | 'X');
            if hex {
                p += 1;
            }
            let start = p;
            while p < chars.len()
                && ((hex && chars[p].is_ascii_hexdigit()) || (!hex && chars[p].is_ascii_digit()))
            {
                p += 1;
            }
            if p > start {
                let num: String = chars[start..p].iter().collect();
                let cp = if hex {
                    u32::from_str_radix(&num, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                if p < chars.len() && chars[p] == ';' {
                    p += 1;
                }
                match cp.and_then(char_from_html_codepoint) {
                    Some(c) => out.push(c),
                    None => out.push('\u{fffd}'),
                }
                i = p;
                continue;
            }
        } else if i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic() {
            let mut p = i + 1;
            while p < chars.len() && chars[p].is_ascii_alphanumeric() {
                p += 1;
            }
            let mut name: String = chars[i + 1..p].iter().collect();
            let mut consumed = p;
            if p < chars.len() && chars[p] == ';' {
                name.push(';');
                consumed = p + 1;
            }
            if let Some(rep) = named_entity(&name) {
                out.push_str(rep);
                i = consumed;
                continue;
            }
            // without ';' try the variant with ';' too
            if !name.ends_with(';') {
                if let Some(rep) = named_entity(&format!("{name};")) {
                    out.push_str(rep);
                    i = consumed;
                    continue;
                }
            }
        }
        out.push('&');
        i += 1;
    }
    out
}

/// Port of `html.escape(text)` (quote=True): `& < > " '` → entities.
///
/// ⚠️ Python returns the apostrophe as `&#x27;`, not `&#39;` — the notation
/// matters here, because the output is compared with the original byte for
/// byte.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

// Special HTML rules for C1 and forbidden code points (an html5 subset).
fn char_from_html_codepoint(cp: u32) -> Option<char> {
    // Windows-1252 map for 0x80..0x9F, as in html._invalid_charrefs.
    const C1: [(u32, char); 27] = [
        (0x80, '\u{20ac}'), (0x82, '\u{201a}'), (0x83, '\u{0192}'), (0x84, '\u{201e}'),
        (0x85, '\u{2026}'), (0x86, '\u{2020}'), (0x87, '\u{2021}'), (0x88, '\u{02c6}'),
        (0x89, '\u{2030}'), (0x8a, '\u{0160}'), (0x8b, '\u{2039}'), (0x8c, '\u{0152}'),
        (0x8e, '\u{017d}'), (0x91, '\u{2018}'), (0x92, '\u{2019}'), (0x93, '\u{201c}'),
        (0x94, '\u{201d}'), (0x95, '\u{2022}'), (0x96, '\u{2013}'), (0x97, '\u{2014}'),
        (0x98, '\u{02dc}'), (0x99, '\u{2122}'), (0x9a, '\u{0161}'), (0x9b, '\u{203a}'),
        (0x9c, '\u{0153}'), (0x9e, '\u{017e}'), (0x9f, '\u{0178}'),
    ];
    if cp == 0 {
        return Some('\u{fffd}');
    }
    if let Some(&(_, c)) = C1.iter().find(|&&(k, _)| k == cp) {
        return Some(c);
    }
    char::from_u32(cp)
}

// Common named entities (the key includes ';', plus a few «legacy» ones
// without ';', as in CPython's html5 dictionary).
fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp;" | "amp" => "&",
        "lt;" | "lt" => "<",
        "gt;" | "gt" => ">",
        "quot;" | "quot" => "\"",
        "apos;" => "'",
        "nbsp;" | "nbsp" => "\u{a0}",
        "copy;" | "copy" => "\u{a9}",
        "reg;" | "reg" => "\u{ae}",
        "trade;" => "\u{2122}",
        "deg;" | "deg" => "\u{b0}",
        "plusmn;" | "plusmn" => "\u{b1}",
        "middot;" | "middot" => "\u{b7}",
        "bull;" => "\u{2022}",
        "hellip;" => "\u{2026}",
        "mdash;" => "\u{2014}",
        "ndash;" => "\u{2013}",
        "lsquo;" => "\u{2018}",
        "rsquo;" => "\u{2019}",
        "ldquo;" => "\u{201c}",
        "rdquo;" => "\u{201d}",
        "laquo;" | "laquo" => "\u{ab}",
        "raquo;" | "raquo" => "\u{bb}",
        "times;" | "times" => "\u{d7}",
        "divide;" | "divide" => "\u{f7}",
        "euro;" => "\u{20ac}",
        "pound;" | "pound" => "\u{a3}",
        "cent;" | "cent" => "\u{a2}",
        "sect;" | "sect" => "\u{a7}",
        "para;" | "para" => "\u{b6}",
        "eacute;" | "eacute" => "\u{e9}",
        "egrave;" | "egrave" => "\u{e8}",
        "agrave;" | "agrave" => "\u{e0}",
        "uuml;" | "uuml" => "\u{fc}",
        "ouml;" | "ouml" => "\u{f6}",
        "auml;" | "auml" => "\u{e4}",
        "szlig;" | "szlig" => "\u{df}",
        "ntilde;" | "ntilde" => "\u{f1}",
        "ccedil;" | "ccedil" => "\u{e7}",
        _ => return None,
    })
}
