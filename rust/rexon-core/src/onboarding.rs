//! First-run onboarding — the scenario and its state.
//!
//! A person coming from Windows opens a window with six tabs and does not
//! understand where to start. Onboarding leads them along a chain: highlight
//! the right place, explain briefly, offer "Next" and "Skip".
//!
//! Here are only data and decisions: which steps the scenario consists of, what
//! a step is attached to, which one to show next and when onboarding is over.
//! The dimming itself, the "spotlight" and the buttons are drawn by the Qt side.
//!
//! Steps are described as DATA, not code: adding a lesson means adding an entry
//! to the table without touching any tab.

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Settings keys
// ============================================================================

/// Whether onboarding is enabled. A missing key means "not asked yet" — that is
/// exactly how the first run is told apart from a deliberate refusal.
pub const KEY_ENABLED: &str = "onboarding_enabled";

/// Identifiers of steps already shown. Each step is shown once.
pub const KEY_SEEN: &str = "onboarding_seen";

// ============================================================================
// Steps
// ============================================================================

/// What a hint is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// A tab title in the top bar — the "chip" itself is highlighted.
    Tab(&'static str),
    /// The whole content of a tab.
    TabContent(&'static str),
    /// Nothing: a card in the center of the window (greeting, finish).
    Window,
}

impl Anchor {
    /// Tab identifier, if the step is attached to one.
    pub fn tab_id(&self) -> Option<&'static str> {
        match self {
            Anchor::Tab(id) | Anchor::TabContent(id) => Some(id),
            Anchor::Window => None,
        }
    }

    /// The attachment kind as a string — easier for the Qt side.
    pub fn kind(&self) -> &'static str {
        match self {
            Anchor::Tab(_) => "tab",
            Anchor::TabContent(_) => "tab_content",
            Anchor::Window => "window",
        }
    }
}

/// One onboarding step.
///
/// `title` and `text` are translation catalog strings (`msgid`, in English);
/// show them via [`Step::title`]/[`Step::text`], not through the fields.
#[derive(Debug, Clone)]
pub struct Step {
    /// A stable identifier: "already shown" is remembered by it. It must not be
    /// changed — otherwise people would see the lesson again.
    pub id: &'static str,
    pub anchor: Anchor,
    pub title: &'static str,
    pub text: &'static str,
}

impl Step {
    /// Step title in the interface language.
    pub fn title(&self) -> String {
        tr(self.title)
    }

    /// Step explanation in the interface language.
    pub fn text(&self) -> String {
        tr(self.text)
    }
}

/// The onboarding scenario in display order.
///
/// The chain leads through what a person installs Rexon for: changing the look
/// of the desktop, setting a live wallpaper, protecting themselves from
/// breakage and learning about the Steam features.
pub const STEPS: &[Step] = &[
    Step {
        id: "welcome",
        anchor: Anchor::Window,
        title: n_("Welcome to Rexon"),
        text: n_(
            "Rexon changes how your desktop looks: themes, wallpapers, cursors, \
             fonts. I'll show you where everything is in a minute. You can skip \
             this and turn it on later in Settings."
        ),
    },
    Step {
        id: "tab_appearance",
        anchor: Anchor::Tab("appearance"),
        title: n_("Appearance"),
        text: n_(
            "The main tab. Here are the theme catalog, wallpapers, cursors and \
             fonts. A theme can be downloaded and applied in one click — no need \
             to search websites and unpack archives by hand."
        ),
    },
    Step {
        id: "appearance_catalog",
        anchor: Anchor::TabContent("appearance"),
        title: n_("Theme catalog"),
        text: n_(
            "Pick a section on the left; theme cards with previews are on the \
             right. The button on a card downloads the theme and applies it right \
             away. Rexon keeps your previous look, so you can always go back."
        ),
    },
    Step {
        id: "tab_wallpaper_engine",
        anchor: Anchor::Tab("wallpaper_engine"),
        title: n_("Live wallpapers"),
        text: n_(
            "Video and animated wallpapers — the same as Wallpaper Engine on \
             Windows. The Workshop is here too: wallpapers can be downloaded and \
             set right from it."
        ),
    },
    Step {
        id: "tab_desktops",
        anchor: Anchor::Tab("desktops"),
        title: n_("Desktops"),
        text: n_(
            "Install and switch desktops — KDE, GNOME, Cinnamon and others. The \
             current desktop can't be removed: otherwise there would be nothing \
             to log in to after a reboot."
        ),
    },
    Step {
        id: "settings_protection",
        anchor: Anchor::Tab("settings"),
        title: n_("App protection"),
        text: n_(
            "Switching desktops tends to reset your default apps — browser, \
             terminal, viewer. Protection in Settings remembers your choice and \
             brings it back. It is on from the start."
        ),
    },
    Step {
        id: "tab_steam",
        anchor: Anchor::Tab("steam"),
        title: n_("Steam features"),
        text: n_(
            "A startup movie when you log in — like on the Steam Deck — and \
             Steam profile backgrounds. You can pick a movie from the ones \
             already in your library."
        ),
    },
    Step {
        id: "finish",
        anchor: Anchor::Window,
        title: n_("Done"),
        text: n_(
            "That's all. You can take the tour again or turn it off in Settings. \
             If something goes wrong with the appearance, the Start menu has an \
             emergency recovery item."
        ),
    },
];

/// A step by identifier.
pub fn step(id: &str) -> Option<&'static Step> {
    STEPS.iter().find(|s| s.id == id)
}

/// Ordinal number of the step among all of them, starting from one — for the
/// "step N of M" caption.
pub fn step_position(id: &str) -> Option<usize> {
    STEPS.iter().position(|s| s.id == id).map(|i| i + 1)
}

/// Total number of steps in the scenario.
pub fn total_steps() -> usize {
    STEPS.len()
}

// ============================================================================
// State
// ============================================================================

/// What to do when the application starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// First run: ask whether to enable onboarding.
    Ask,
    /// Show this step.
    Show(&'static str),
    /// Do nothing: disabled or everything has been gone through.
    Nothing,
}

/// Decision at start-up.
///
/// `enabled` — the value of the settings key: `None` means "not asked yet".
/// Distinguishing this from an explicit `Some(false)` is important: otherwise a
/// person who refused once would get the question on every start.
pub fn on_start(enabled: Option<bool>, seen: &[String]) -> Start {
    match enabled {
        None => Start::Ask,
        Some(false) => Start::Nothing,
        Some(true) => match next_step(seen) {
            Some(step) => Start::Show(step.id),
            None => Start::Nothing,
        },
    }
}

/// The next step not yet shown, or `None` if the scenario has been completed.
pub fn next_step(seen: &[String]) -> Option<&'static Step> {
    STEPS.iter().find(|s| !seen.iter().any(|x| x == s.id))
}

/// The step following the given one — for the "Next" button.
pub fn step_after(current: &str, seen: &[String]) -> Option<&'static Step> {
    let Some(pos) = STEPS.iter().position(|s| s.id == current) else {
        return next_step(seen);
    };
    STEPS[pos + 1..]
        .iter()
        .find(|s| !seen.iter().any(|x| x == s.id))
}

/// Mark a step as shown. No duplicates appear in the list, the order is kept —
/// the list is readable if one opens the settings by hand.
pub fn mark_seen(seen: &mut Vec<String>, id: &str) {
    if !seen.iter().any(|x| x == id) {
        seen.push(id.to_string());
    }
}

/// Whether the whole scenario has been completed.
pub fn is_finished(seen: &[String]) -> bool {
    next_step(seen).is_none()
}

/// The list of shown steps after a refusal in the middle of onboarding: mark
/// everything, so that "Skip" does not turn into a question on the next start.
pub fn skip_all() -> Vec<String> {
    STEPS.iter().map(|s| s.id.to_string()).collect()
}

/// Reset the progress — "go through onboarding again" in Settings.
pub fn reset() -> Vec<String> {
    Vec::new()
}

// ============================================================================
// Interface texts
// ============================================================================
//
// Functions rather than constants: the translation is chosen at display time
// (`crate::i18n`), and a function, unlike a constant, cannot be shown
// untranslated.

/// Title of the question on the first run.
pub fn ask_title() -> String {
    tr("Show a short tour?")
}
/// Explanation of the question.
pub fn ask_text() -> String {
    tr("It takes about a minute: I'll show where to change themes, wallpapers \
        and appearance. You can turn this on or off later in Settings.")
}
/// The consent button.
pub fn ask_yes() -> String {
    // TRANSLATORS: button that agrees to show the first-launch tour.
    tr("Show")
}
/// The refusal button.
pub fn ask_no() -> String {
    // TRANSLATORS: button that declines the first-launch tour.
    tr("No, thanks")
}

/// The button for going to the next step.
pub fn btn_next() -> String {
    tr("Next")
}
/// The button on the last step.
pub fn btn_done() -> String {
    tr("Done")
}
/// The refusal button in the middle of onboarding.
pub fn btn_skip() -> String {
    tr("Skip")
}

/// The "step N of M" caption.
pub fn progress_label(position: usize, total: usize) -> String {
    // TRANSLATORS: %1 is the current step, %2 is the number of steps.
    trf("Step %1 of %2", &[&position, &total])
}

/// Label of the toggle in Settings.
pub fn settings_label() -> String {
    tr("Tour on first launch")
}
/// The button for going through again.
pub fn settings_replay() -> String {
    tr("Take the tour again")
}

// ============================================================================
// Overlay: geometry and palette
// ============================================================================
//
// The overlay's numbers live here, not in the Qt shim: the cut-out, the gap and
// the radius are related to each other (the card stands off from the cut-out by
// exactly the gap, the cut-out by the padding around the target), and if they
// drift apart, you get either a crookedly cropped highlight or a card on top of
// the highlighted place.

/// Dimming around the highlighted place, RGBA. Onboarding has no number of its
/// own: background dimming is one for the product and lives in `ui_base`.
pub const DIM_COLOR: (u8, u8, u8, u8) = crate::ui_base::MODAL_DIM_RGBA;
/// The default skin's highlight neon, RGB. The live color is taken from the
/// CURRENT skin — see [`overlay_palette`]; this entry remains as a reference for
/// checks.
pub const HALO_COLOR: (u8, u8, u8) = (102, 192, 244);
/// Thickness of the neon frame around the cut-out, px.
pub const HALO_WIDTH: i32 = 2;
/// Padding of the cut-out around the target, px: the highlight does not hug the widget.
pub const TARGET_PAD: i32 = 8;
/// Corner radius of the cut-out and the frame, px.
pub const CORNER_RADIUS: i32 = 10;
/// Maximum card width, px: a long line is unreadable.
pub const CARD_MAX_WIDTH: i32 = 420;
/// Gap between the cut-out and the card, px. It is also the minimum margin of the
/// card from the window edge.
pub const CARD_GAP: i32 = 14;
/// Card background of the default skin. The live color is [`overlay_palette`].
pub const CARD_BACKGROUND: &str = "#1b2838";

// ── card style: the numbers that are no longer in the Qt shim ─────────────
//
// Margins, gaps, radii, font sizes and weights lay in `rexon/onboarding.py`
// mixed with QSS strings, although the same module promised in its header that
// it had no numbers left. Now the promise is true: the shim only executes.

/// Card margins `(left, top, right, bottom)`, px.
pub const CARD_MARGINS: (i32, i32, i32, i32) = (18, 16, 18, 14);
/// Gap between card lines, px.
pub const CARD_SPACING: i32 = 8;
/// Gap between buttons in the card's bottom row, px.
pub const CARD_BUTTON_SPACING: i32 = 8;
/// Card corner radius, px (the cut-out has its own — [`CORNER_RADIUS`]).
pub const CARD_RADIUS: i32 = 12;
/// How much smaller than the application font size the progress caption is, and
/// below what value it does not go.
pub const PROGRESS_POINT_DELTA: i32 = -1;
/// Smallest font size of the progress caption, pt.
pub const PROGRESS_POINT_MIN: i32 = 8;
/// How much larger than the application font size the card title is.
pub const TITLE_POINT_DELTA: i32 = 3;
/// Smallest font size of the card title, pt.
pub const TITLE_POINT_MIN: i32 = 14;
/// Weight of the explanation: thin text on a dark background is unreadable.
pub const BODY_FONT_WEIGHT: i32 = 600;
/// Weight of the label on the "Next" button.
pub const NEXT_FONT_WEIGHT: i32 = 700;
/// Corner radius of the "Next" button, px.
pub const NEXT_RADIUS: i32 = 6;
/// Padding inside the "Next" button `(top-bottom, left-right)`, px.
pub const NEXT_PADDING: (i32, i32) = (8, 20);
/// Padding inside the "Skip" button `(top-bottom, left-right)`, px.
pub const SKIP_PADDING: (i32, i32) = (6, 10);
/// Share of white mixed into the accent when hovering over "Next".
pub const HOVER_LIGHTEN: f64 = 0.25;
/// Label on the accent button when the accent itself is LIGHT.
pub const ON_ACCENT_DARK: &str = "#0b1722";
/// Label on the accent button when the accent itself is DARK.
pub const ON_ACCENT_LIGHT: &str = "#ffffff";

/// Overlay palette: the colors come from the skin rather than being baked into
/// onboarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPalette {
    /// Highlight neon and fill of the "Next" button, RGB.
    pub halo: (u8, u8, u8),
    /// The same neon as a `#rrggbb` string.
    pub halo_hex: String,
    /// Fill of the "Next" button on hover.
    pub halo_hover: String,
    /// Card background.
    pub card_background: String,
    /// Main card text (title and explanation).
    pub card_text: String,
    /// Muted text ("step N of M", "Skip").
    pub card_muted: String,
    /// Label on the accent button — contrasting with the accent itself.
    pub on_accent: &'static str,
}

/// Overlay palette for a NAMED skin.
///
/// Warning: previously all three colors were baked for `steam_dark`: a buyer who
/// chose "Light" or "Synthwave" got a dark-blue card with a Steam-blue frame in
/// the middle of their window. The same class of defect as with the QML pages,
/// so it is fixed the same way — the colors come from the skin table (rule
/// #12).
///
/// The muted text is taken from the skin token (`text_dim`) rather than from the
/// former gray-blue `#9fb4c6`: on a dark background the product keeps only
/// white, and gray shades blended into it.
pub fn overlay_palette_for(skin_id: &str) -> OverlayPalette {
    let colors = crate::ui_base::skin_colors(skin_id);
    let pick = |key: &str, fallback: &str| -> String {
        colors.get(key).cloned().unwrap_or_else(|| fallback.to_string())
    };
    let halo_hex = pick("accent", "#66c0f4");
    let halo = crate::ui_base::parse_hex(&halo_hex).unwrap_or(HALO_COLOR);
    let on_accent = if crate::ui_base::is_dark_rgb(halo.0, halo.1, halo.2) {
        ON_ACCENT_LIGHT
    } else {
        ON_ACCENT_DARK
    };
    OverlayPalette {
        halo,
        halo_hover: crate::ui_base::lighten_hex(&halo_hex, HOVER_LIGHTEN),
        halo_hex,
        card_background: pick("bg_primary", CARD_BACKGROUND),
        card_text: pick("text_primary", "#ffffff"),
        card_muted: pick("text_dim", "#ffffff"),
        on_accent,
    }
}

/// Overlay palette for the CURRENT skin.
pub fn overlay_palette() -> OverlayPalette {
    overlay_palette_for(crate::ui_base::active_skin_id())
}

/// Card QSS: background, neon frame, rounding.
pub fn card_qss(pal: &OverlayPalette) -> String {
    format!(
        "QWidget#obCard {{ background: {}; border: {}px solid {}; \
border-radius: {}px; }}",
        pal.card_background, HALO_WIDTH, pal.halo_hex, CARD_RADIUS
    )
}

/// QSS of the progress caption and any other muted text of the card.
pub fn muted_text_qss(pal: &OverlayPalette) -> String {
    format!("color: {};", pal.card_muted)
}

/// QSS of the card title.
pub fn title_qss(pal: &OverlayPalette) -> String {
    format!("color: {};", pal.card_text)
}

/// QSS of the card explanation.
pub fn body_qss(pal: &OverlayPalette) -> String {
    format!("color: {}; font-weight: {};", pal.card_text, BODY_FONT_WEIGHT)
}

/// QSS of the "Skip" button — a flat label, accent-colored under the cursor.
///
/// Warning: the former "muted → main" transition was INVISIBLE on the default
/// skin: the product keeps only white on a dark background (the owner's rule of
/// 11.06.2026), so both colors coincide there and hovering produced no response.
/// The accent differs from the text on any skin.
pub fn skip_button_qss(pal: &OverlayPalette) -> String {
    format!(
        "QPushButton {{ background: transparent; color: {}; border: none; \
padding: {}px {}px; }} QPushButton:hover {{ color: {}; }}",
        pal.card_muted, SKIP_PADDING.0, SKIP_PADDING.1, pal.halo_hex
    )
}

/// QSS of the "Next" button — accent fill, a contrasting label.
pub fn next_button_qss(pal: &OverlayPalette) -> String {
    format!(
        "QPushButton {{ background: {}; color: {}; font-weight: {}; \
border: none; border-radius: {}px; padding: {}px {}px; }} \
QPushButton:hover {{ background: {}; }}",
        pal.halo_hex,
        pal.on_accent,
        NEXT_FONT_WEIGHT,
        NEXT_RADIUS,
        NEXT_PADDING.0,
        NEXT_PADDING.1,
        pal.halo_hover
    )
}

/// Card font sizes from the application font size: `(progress caption, title)`.
pub fn card_point_sizes(base: i32) -> (i32, i32) {
    (
        (base + PROGRESS_POINT_DELTA).max(PROGRESS_POINT_MIN),
        (base + TITLE_POINT_DELTA).max(TITLE_POINT_MIN),
    )
}

/// Where to place the hint card, in overlay coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardPlacement {
    pub x: i32,
    pub y: i32,
}

/// The cut-out rectangle around the target — port of
/// `set_target`/`set_target_rect`: the target is expanded by [`TARGET_PAD`] on
/// all sides.
///
/// `None` on input (there is no target or it is invisible) gives `None` on
/// output too: the whole window is dimmed, the card stands in the center.
pub fn spotlight_hole(target: Option<(i32, i32, i32, i32)>) -> Option<(i32, i32, i32, i32)> {
    let (x, y, w, h) = target?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((
        x - TARGET_PAD,
        y - TARGET_PAD,
        w + TARGET_PAD * 2,
        h + TARGET_PAD * 2,
    ))
}

/// Where to put the card so that it does NOT cover the highlighted place — port
/// of `_Spotlight._place_card`.
///
/// The order of preferences is preserved: below the cut-out if the card fits
/// there; otherwise above it; otherwise in the center of the window.
/// Horizontally the card is centered on the cut-out and pressed against the
/// window edges with the [`CARD_GAP`] gap.
///
/// `hole` = `None` — the card is exactly in the center of the window.
pub fn card_placement(
    overlay: (i32, i32),
    card: (i32, i32),
    hole: Option<(i32, i32, i32, i32)>,
) -> CardPlacement {
    let (ow, oh) = overlay;
    let (cw, ch) = card;
    let Some((hx, hy, hw, hh)) = hole else {
        return CardPlacement {
            x: (ow - cw) / 2,
            y: (oh - ch) / 2,
        };
    };
    // QRect::bottom()/right() in Qt are the last pixel inside, not past it.
    let below = hy + hh - 1 + CARD_GAP;
    let above = hy - CARD_GAP - ch;
    let y = if below + ch <= oh {
        below
    } else if above >= 0 {
        above
    } else {
        ((oh - ch) / 2).max(0)
    };
    let center_x = hx + hw / 2;
    let x = (center_x - cw / 2).clamp(CARD_GAP, (ow - cw - CARD_GAP).max(CARD_GAP));
    CardPlacement { x, y }
}

// ============================================================================
// Card contents
// ============================================================================

/// What to write in a step's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardContent {
    /// The "step N of M" caption.
    pub progress: String,
    pub title: String,
    pub text: String,
    /// Label of the forward button: on the last step it is "Done".
    pub next_text: String,
    /// Label of the refusal button; empty on the last step.
    pub skip_text: String,
    /// Whether to show the refusal button: on the last step there is nothing to
    /// refuse, and an extra button confuses (rule #13).
    pub skip_visible: bool,
}

/// Card contents for a step — port of the `Tour._show` branching.
///
/// A step that is not in the scenario gives `None`: showing an empty card is
/// worse than showing nothing.
pub fn card_content(step_id: &str) -> Option<CardContent> {
    let step = step(step_id)?;
    let position = step_position(step_id)?;
    let total = total_steps();
    let last = position >= total;
    Some(CardContent {
        progress: progress_label(position, total),
        title: step.title(),
        text: step.text(),
        next_text: if last { btn_done() } else { btn_next() },
        skip_text: if last { String::new() } else { btn_skip() },
        skip_visible: !last,
    })
}

// ============================================================================
// Aiming the "spotlight"
// ============================================================================

/// What the Qt side should look for to aim the highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AimTarget {
    /// Look for nothing: dim the whole window, the card in the center.
    WholeWindow,
    /// A tab title in the top bar.
    TabHeader(&'static str),
    /// The content of a tab; it also has to be switched to.
    TabContent(&'static str),
}

/// What to aim the highlight at for a step — port of `Tour._aim`.
///
/// A step without a tab attachment and an unknown step give
/// [`AimTarget::WholeWindow`]: an empty frame bothers the person more than its
/// absence.
pub fn aim_target(step_id: &str) -> AimTarget {
    match step(step_id).map(|s| s.anchor) {
        Some(Anchor::Tab(id)) => AimTarget::TabHeader(id),
        Some(Anchor::TabContent(id)) => AimTarget::TabContent(id),
        _ => AimTarget::WholeWindow,
    }
}

// ============================================================================
// State in the settings
// ============================================================================

/// Value of the "onboarding enabled" key from the settings.
///
/// A non-boolean (the file was edited by hand, it has a string or a number) is
/// read as "not asked yet": the question will be repeated once and will write
/// an honest value. The Python bridge crashed at this point while parsing the
/// argument and silently showed nothing — fixed here, for both entry points.
pub fn enabled_from(settings: &serde_json::Map<String, serde_json::Value>) -> Option<bool> {
    settings.get(KEY_ENABLED).and_then(serde_json::Value::as_bool)
}

/// Shown steps from the settings, reduced to a list of strings.
pub fn seen_from(settings: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    normalize_seen(settings.get(KEY_SEEN))
}

/// Decision at start-up directly from the contents of the settings.
pub fn start_from(settings: &serde_json::Map<String, serde_json::Value>) -> Start {
    on_start(enabled_from(settings), &seen_from(settings))
}

// ============================================================================
// Onboarding session on top of the settings file
// ============================================================================
//
// The state lives in the SHARED `settings.json` (`crate::settings`) under the
// same keys the Python window writes: a person who went through onboarding in
// one login will not see it a second time in another. Each step is saved
// immediately, not at the end: a window closed halfway does not restart
// onboarding from the beginning.
//
// The functions take the settings manager from outside instead of opening the
// file themselves: the checks run them on a temporary file without touching
// the person's profile. The live entry point is
// [`crate::settings::SettingsManager::new`].
//
// A write error is returned as a SECOND value, not instead of the answer: as
// with the Python window, progress that failed to save is no reason to cut
// onboarding short, but the reason must reach the log.

use crate::settings::SettingsManager;

/// What was decided when the window started.
pub fn session_start(manager: &SettingsManager) -> Start {
    start_from(&manager.settings)
}

/// Answer to the first-run question. A refusal is remembered — we do not ask a
/// second time; consent immediately gives the first step not yet shown.
pub fn session_answer(manager: &mut SettingsManager, show: bool) -> (Start, std::io::Result<()>) {
    let saved = manager.set(KEY_ENABLED, serde_json::Value::Bool(show));
    let start = if show {
        session_start(manager)
    } else {
        Start::Nothing
    };
    (start, saved)
}

/// "Next": mark the current step as shown, save and return the next one.
/// `None` — the scenario is complete.
pub fn session_advance(
    manager: &mut SettingsManager,
    current: &str,
) -> (Option<&'static Step>, std::io::Result<()>) {
    let mut seen = seen_from(&manager.settings);
    mark_seen(&mut seen, current);
    let next = step_after(current, &seen);
    let saved = manager.set(KEY_SEEN, seen_value(&seen));
    (next, saved)
}

/// "Skip": mark the whole scenario as completed, otherwise onboarding would pop
/// up again on the next start.
pub fn session_skip(manager: &mut SettingsManager) -> std::io::Result<()> {
    manager.set(KEY_SEEN, seen_value(&skip_all()))
}

/// "Go through onboarding again" from Settings: reset the progress, enable
/// onboarding and immediately decide what to show.
pub fn session_replay(manager: &mut SettingsManager) -> (Start, std::io::Result<()>) {
    let seen = manager.set(KEY_SEEN, seen_value(&reset()));
    let enabled = manager.set(KEY_ENABLED, serde_json::Value::Bool(true));
    (session_start(manager), seen.and(enabled))
}

fn seen_value(seen: &[String]) -> serde_json::Value {
    serde_json::Value::Array(
        seen.iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect(),
    )
}

/// Reduce the value of the "shown steps" key from the settings to a list of
/// strings — port of `Tour._seen`.
///
/// The settings are a file on disk, and anything may turn up there (a number, a
/// string, `null`): if it is not a list, we start from a clean slate rather than
/// crash.
pub fn normalize_seen(raw: Option<&serde_json::Value>) -> Vec<String> {
    let Some(serde_json::Value::Array(items)) = raw else {
        return Vec::new();
    };
    items
        .iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}
