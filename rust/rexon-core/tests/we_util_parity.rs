//! Check of the `we_util.rs` port against the original waywallen `ui/src/util.cpp`.
//!
//! The reference is the upstream source itself (MIT):
//! their C++ is not built here, so the expectations were written out of the
//! algorithm line by line, and disputed spots (auto-links) were additionally
//! verified by simulating their regexes on a PCRE-compatible engine.
//!
//! Separate tests pin the FIXES: (a) our former Python bridge was a stub
//! (BBCode as is, colour as a Qt name) — now upstream semantics; (b) a bug of
//! upstream itself: auto-links wrapped an already built `[url]` anchor into
//! nested garbage.

use rexon_core::we_util as u;

// ── desktop ─────────────────────────────────────────────────────

#[test]
fn desktop_detection() {
    assert_eq!(u::detect_desktop("KDE"), u::DESKTOP_KDE);
    assert_eq!(u::detect_desktop("KDE:wayland"), u::DESKTOP_KDE);
    assert_eq!(u::detect_desktop("wayland:Hyprland"), u::DESKTOP_HYPRLAND);
    assert_eq!(u::detect_desktop("sway"), u::DESKTOP_SWAY);
    assert_eq!(u::detect_desktop("niri"), u::DESKTOP_NIRI);
    assert_eq!(u::detect_desktop("ubuntu:GNOME"), u::DESKTOP_GNOME);
    assert_eq!(u::detect_desktop(""), u::DESKTOP_UNKNOWN);
    assert_eq!(u::detect_desktop("XFCE"), u::DESKTOP_UNKNOWN);
    // empty tokens are skipped, case does not matter, extra spaces neither
    assert_eq!(u::detect_desktop("::  KdE  :x"), u::DESKTOP_KDE);
}

#[test]
fn display_rename_support() {
    for de in [u::DESKTOP_HYPRLAND, u::DESKTOP_SWAY, u::DESKTOP_NIRI] {
        assert!(u::supports_display_rename(de), "{de}");
    }
    for de in [u::DESKTOP_UNKNOWN, u::DESKTOP_KDE, u::DESKTOP_GNOME] {
        assert!(!u::supports_display_rename(de), "{de}");
    }
}

// ── BBCode ───────────────────────────────────────────────────────────

#[test]
fn bbcode_empty_and_plain() {
    assert_eq!(u::bbcode_to_html(""), "");
    assert_eq!(u::bbcode_to_html("просто текст"), "просто текст");
}

#[test]
fn bbcode_escapes_raw_html() {
    assert_eq!(
        u::bbcode_to_html("<b>чужой</b> & <script>"),
        "&lt;b&gt;чужой&lt;/b&gt; &amp; &lt;script&gt;"
    );
}

#[test]
fn bbcode_inline_marks() {
    assert_eq!(u::bbcode_to_html("[b]жир[/b]"), "<b>жир</b>");
    assert_eq!(u::bbcode_to_html("[I]кур[/i]"), "<i>кур</i>"); // case
    assert_eq!(u::bbcode_to_html("[u]под[/u]"), "<u>под</u>");
    assert_eq!(u::bbcode_to_html("[s]зач[/s]"), "<s>зач</s>");
    assert_eq!(u::bbcode_to_html("[strike]зач[/strike]"), "<s>зач</s>");
    assert_eq!(u::bbcode_to_html("[code]код[/code]"), "<tt>код</tt>");
    assert_eq!(
        u::bbcode_to_html("[spoiler]тс[/spoiler]"),
        "<font color=\"#888888\">тс</font>"
    );
    // non-greedy: the nearest closing tag
    assert_eq!(u::bbcode_to_html("[b]a[/b]x[b]b[/b]"), "<b>a</b>x<b>b</b>");
    // an unclosed tag stays text
    assert_eq!(u::bbcode_to_html("[b]висит"), "[b]висит");
}

#[test]
fn bbcode_quote_headings_lists() {
    assert_eq!(u::bbcode_to_html("[quote]цит[/quote]"), "<i>“цит”</i>");
    assert_eq!(
        u::bbcode_to_html("[quote=Вася]цит[/quote]"),
        "<i>“цит”</i>"
    );
    assert_eq!(u::bbcode_to_html("[h1]З[/h1]"), "<h2>З</h2>");
    assert_eq!(u::bbcode_to_html("[h2]З[/h2]"), "<h3>З</h3>");
    assert_eq!(u::bbcode_to_html("[h3]З[/h3]"), "<h4>З</h4>");
    assert_eq!(u::bbcode_to_html("[hr]"), "<hr>");
    assert_eq!(
        u::bbcode_to_html("[list][*] раз[*] два[/list]"),
        "<ul><li>раз<li>два</ul>"
    );
}

#[test]
fn bbcode_links_and_images() {
    assert_eq!(
        u::bbcode_to_html("[url=http://x]Ссылка[/url]"),
        "<a href=\"http://x\">Ссылка</a>"
    );
    assert_eq!(
        u::bbcode_to_html("[url=\"http://x\"]Ссылка[/url]"),
        "<a href=\"http://x\">Ссылка</a>"
    );
    assert_eq!(
        u::bbcode_to_html("[url]http://x[/url]"),
        "<a href=\"http://x\">http://x</a>"
    );
    assert_eq!(
        u::bbcode_to_html("[img]http://x/p.png[/img]"),
        "<img src=\"http://x/p.png\">"
    );
}

#[test]
fn bbcode_autolink_bare_url() {
    assert_eq!(
        u::bbcode_to_html("смотри http://x/1 дальше"),
        "смотри <a href=\"http://x/1\">http://x/1</a> дальше"
    );
    assert_eq!(
        u::bbcode_to_html("https://a.b/c?d=1&e=2"),
        // & is escaped before the auto-link — as in the original
        "<a href=\"https://a.b/c?d=1&amp;e=2\">https://a.b/c?d=1&amp;e=2</a>"
    );
}

/// ⚠️ Fix of an UPSTREAM bug: their last pass went over the whole text and
/// wrapped the address inside an already built `<a href="…">…</a>` again —
/// the output was `<a href="<a href="http://x">http://x">…</a></a>`
/// (verified by simulating their regex). Here the anchor stays intact.
#[test]
fn fix_autolink_does_not_touch_existing_anchor() {
    assert_eq!(
        u::bbcode_to_html("[url]http://x[/url]"),
        "<a href=\"http://x\">http://x</a>"
    );
    assert_eq!(
        u::bbcode_to_html("[url=http://x]http://x[/url]"),
        "<a href=\"http://x\">http://x</a>"
    );
    // an image with the address inside an attribute — not wrapped either
    assert_eq!(
        u::bbcode_to_html("[img]https://y/p.png[/img]"),
        "<img src=\"https://y/p.png\">"
    );
    // a neighbouring bare address must still become a link
    assert_eq!(
        u::bbcode_to_html("[url]http://x[/url] и http://y"),
        "<a href=\"http://x\">http://x</a> и <a href=\"http://y\">http://y</a>"
    );
}

#[test]
fn bbcode_noparse_and_newlines() {
    // the [noparse] body is not parsed, but is escaped
    assert_eq!(
        u::bbcode_to_html("[noparse][b]x[/b] & <i>[/noparse]"),
        "[b]x[/b] &amp; &lt;i&gt;"
    );
    // line breaks last, a tag body with its own \n survives
    assert_eq!(u::bbcode_to_html("a\r\nb\nc"), "a<br>b<br>c");
    assert_eq!(u::bbcode_to_html("[b]a\nb[/b]"), "<b>a<br>b</b>");
}

// ── wire colour ───────────────────────────────────────────────────

#[test]
fn wire_color_parse() {
    assert_eq!(u::parse_wire_color("1 0 0"), vec![1.0, 0.0, 0.0]);
    assert_eq!(
        u::parse_wire_color("  0.5   0.25 0.125 0.75 "),
        vec![0.5, 0.25, 0.125, 0.75]
    );
    assert!(u::parse_wire_color("#ff0000").is_empty());
    assert!(u::parse_wire_color("1 0 x").is_empty());
    assert!(u::parse_wire_color("").is_empty());
}

#[test]
fn wire_color_from() {
    assert_eq!(u::color_from_wire("1 0 0"), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(u::color_from_wire("0 0 0 0.5"), [0.0, 0.0, 0.0, 0.5]);
    // values outside 0..1 are clamped, garbage and missing numbers → black
    assert_eq!(u::color_from_wire("2 -1 0.5"), [1.0, 0.0, 0.5, 1.0]);
    assert_eq!(u::color_from_wire("1 0"), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(u::color_from_wire("#ff0000"), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn wire_color_to() {
    assert_eq!(u::color_to_wire(1.0, 0.0, 0.5, 1.0, false), "1.0000 0.0000 0.5000");
    assert_eq!(
        u::color_to_wire(1.0, 0.0, 0.5, 0.25, true),
        "1.0000 0.0000 0.5000 0.2500"
    );
    assert!(u::color_has_alpha("0 0 0 1"));
    assert!(!u::color_has_alpha("0 0 0"));
    assert!(!u::color_has_alpha("#ff000000")); // not hex, but numbers
}

/// Round trip of a daemon value: string → colour → string.
#[test]
fn wire_color_roundtrip() {
    let c = u::color_from_wire("0.2500 0.5000 0.7500 0.1250");
    assert_eq!(
        u::color_to_wire(c[0], c[1], c[2], c[3], true),
        "0.2500 0.5000 0.7500 0.1250"
    );
}

// ── display label ──────────────────────────────────────────────────

/// Port of `Display::displayLabel` (waywallen 0.4.4, `display.cppm`): the
/// display number is ALWAYS appended, not only for namesakes. Caught on the
/// «Monitors» page: the author has «HEADLESS-2 (#1)», we had «HEADLESS-2».
#[test]
fn display_label_always_carries_the_id() {
    assert_eq!(u::display_label("", "HEADLESS-2", 1), "HEADLESS-2 (#1)");
    // the alias beats the output name
    assert_eq!(u::display_label("Левый", "DP-1", 7), "Левый (#7)");
    // a single display — the number is still there
    assert_eq!(u::display_label("", "eDP-1", 0), "eDP-1 (#0)");
    // neither name nor alias: the author's string without translation
    assert_eq!(u::display_label("", "", 3), "Display #3");
}

/// Guard: the tab's `Display` object takes the label from the core and
/// notifies about its change when the name or alias changes; for `Canvas` the
/// label is the name itself (the author has
/// `Q_PROPERTY(QString displayLabel READ name NOTIFY changed)`).
#[test]
fn display_object_takes_its_label_from_core() {
    const OBJMODEL_RS: &str = include_str!("../../rexon-gui/src/we_objmodel.rs");
    let objmodel = OBJMODEL_RS.split_whitespace().collect::<Vec<_>>().join(" ");

    let getter = objmodel
        .split("fn display_label(&self) -> QString {")
        .nth(1)
        .and_then(|rest| rest.split("fn width(&self)").next())
        .expect("Display::display_label");
    assert!(
        getter.contains("rexon_core::we_util::display_label(&info.alias, &info.name, info.id)"),
        "Display.displayLabel обязан собираться в ядре (с номером дисплея)"
    );
    assert!(
        objmodel.contains(
            "if old.name != new.name || old.alias != new.alias { self.as_mut().display_label_changed(); }"
        ),
        "смена имени или псевдонима обязана слать displayLabelChanged"
    );
    assert!(
        objmodel.contains(
            "#[qproperty(QString, display_label, cxx_name = \"displayLabel\", READ = canvas_name, NOTIFY = changed)]"
        ),
        "Canvas.displayLabel — имя холста, без номера"
    );
}
