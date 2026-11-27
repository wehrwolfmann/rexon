//! The «Desktops» tab: its content and decisions.
//!
//! The second tab ported to QML (after «Compare Desktops»). EVERYTHING the
//! tab shows and decides lives here: list sections, item captions and
//! tooltips, the card header, the spec strip, the packages line, the set of
//! buttons by desktop state, confirmation texts and, most importantly, both
//! removal guards. The markup (QML or widgets) only lays this out on screen.
//!
//! ⚠️ There must be no product numbers or strings in the tab. A second copy
//! drifts apart silently, and it is the customer who notices: their showcase
//! ends up with a desktop in the wrong section or a remove button where
//! there can be none.
//!
//! What is taken from the neighbours instead of rewritten:
//!   * desktop icons: [`crate::qt_compare::de_icon`] (one table for the
//!     product);
//!   * desktop specs: [`crate::system::DE_META`];
//!   * switching texts: [`crate::qt_compare::switch_texts`]: they match word
//!     for word in both tabs, and there is no point splitting them;
//!   * whether the desktop may be installed on this distribution:
//!     [`crate::distro_compat`] through the shared confirmation dialog.
//!
//! Captions go through the translation catalog (`crate::i18n`): the source
//! language is English, Russian lives in `po/ru.po`. Attributes (`kind`,
//! `style`, desktop identifiers) are separate from captions and are not translated.

use serde::Serialize;

use crate::i18n::{fill, ntr, tr, trc, trf};

use crate::qt_compare::de_icon;
use crate::shell_bundle::{self, ShellState, Warning};
use crate::system::{is_available, DeMeta, DesktopShell, PkgFamily, DE_META};

// ============================================================================
// Window
// ============================================================================

pub fn window_title() -> String {
    tr("Rexon — Desktops")
}
pub const WINDOW_MIN_SIZE: (u32, u32) = (960, 720);

// ============================================================================
// Left panel: the desktop list
// ============================================================================

pub fn list_header() -> String {
    tr("🖥️ Desktops")
}
pub const LIST_HEADER_POINT_SIZE: i32 = 13;
/// Font size of a list item (QSS of the widget tab: `font-size: 12pt`).
pub const LIST_ITEM_POINT_SIZE: i32 = 12;
/// Font size of a section heading row.
pub const SECTION_POINT_SIZE: i32 = 9;

/// Heading of the full desktops section.
///
/// Not «Desktops»: that is the name of the panel itself, and the heading
/// must not be duplicated. «Classic» ↔ «Tiling» are a pair of adjectives;
/// the third section is «Compositors with effects» ([`section_effects`]).
pub fn section_classic() -> String {
    // TRANSLATORS: list section heading: full desktop environments, as
    // opposed to tiling window managers.
    tr("Classic")
}
/// Heading of the tiling window managers section.
pub fn section_tiling() -> String {
    // TRANSLATORS: list section heading: tiling window managers.
    tr("Tiling")
}
/// Heading of the compositors with effects section (Wayfire): windows float as
/// in classic ones, but it is not a full desktop with its own programs.
pub fn section_effects() -> String {
    // TRANSLATORS: list section heading: Wayland compositors with 3D effects
    // (Wayfire) — floating windows, but not full desktop environments.
    tr("Compositors with effects")
}

/// Set and ORDER of the «Tiling» section: Hyprland first as the most
/// popular, then niri, sway and River.
///
/// Wayfire is NOT here: its windows float (as in Compiz), it is not a
/// tiling compositor; it is in its own section [`EFFECTS_IDS`].
///
/// COSMIC is NOT here: it is a full hybrid (a DE with optional tiling) and
/// stays in «Classic» with a tooltip. The question "is this a tiling
/// compositor" itself is answered by the core
/// ([`crate::appdefaults::is_tiling_wm`]), but the choice "whom to move to a
/// separate section and in what order" is a product one, and lives here.
/// Their agreement is guarded by `qt_desktops_parity::tiling_section_matches_core`.
pub const TILING_IDS: &[&str] = &["hyprland", "niri", "sway", "river"];

/// Set and order of the «Compositors with effects» section (after «Tiling»):
/// windows float, Compiz-style effects, but no own programs and no shared
/// system settings; in «Classic» it would promise more than it gives.
pub const EFFECTS_IDS: &[&str] = &["wayfire"];

/// The hybrid tooltip: the only one tied to a specific desktop.
pub fn cosmic_hint() -> String {
    tr("Hybrid: a desktop with optional tiling")
}
pub fn tip_installed() -> String {
    tr("Installed")
}
pub fn tip_unavailable() -> String {
    tr("Not available on this distribution")
}
/// Separator between the parts of a list item tooltip.
pub const TIP_SEPARATOR: &str = " · ";

/// A list row: either a section heading or a desktop.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListRow {
    /// `section` is a non-selectable heading, `de` is a desktop.
    pub kind: &'static str,
    /// Desktop identifier (empty for a section heading).
    pub de_id: String,
    /// Row text.
    pub text: String,
    /// Row tooltip (empty means no tooltip).
    pub tooltip: String,
    /// Whether the desktop is installed, for the delegate's mark.
    pub installed: bool,
    /// Whether the desktop is available on this distribution.
    pub available: bool,
}

/// Item caption: the desktop icon and its name separated by two spaces.
pub fn item_text(de_id: &str) -> String {
    format!("{}  {}", de_icon(de_id), display_name(de_id))
}

/// Tooltip of a list item; an empty string means no tooltip.
pub fn item_tooltip(de_id: &str, installed: bool, available: bool) -> String {
    let mut tips: Vec<String> = Vec::new();
    if installed {
        tips.push(tip_installed());
    }
    if de_id == "cosmic" {
        tips.push(cosmic_hint());
    }
    if !available {
        tips.push(tip_unavailable());
    }
    tips.join(TIP_SEPARATOR)
}

/// The whole left panel list: «Classic» in database order, then «Tiling»
/// and «Compositors with effects» in their fixed order.
///
/// A section heading appears only if the section has at least one desktop:
/// an empty section is a caption without content, it tells the customer nothing.
pub fn list_rows(family: PkgFamily, installed: &dyn Fn(&str) -> bool) -> Vec<ListRow> {
    let mut out = Vec::new();
    let classic: Vec<&DeMeta> = DE_META
        .iter()
        .filter(|m| !TILING_IDS.contains(&m.id) && !EFFECTS_IDS.contains(&m.id))
        .collect();
    let fixed = |ids: &[&str]| -> Vec<&DeMeta> {
        ids.iter()
            .filter_map(|id| DE_META.iter().find(|m| m.id == *id))
            .collect()
    };

    for (title, group) in [
        (section_classic(), classic),
        (section_tiling(), fixed(TILING_IDS)),
        (section_effects(), fixed(EFFECTS_IDS)),
    ] {
        if group.is_empty() {
            continue;
        }
        out.push(ListRow {
            kind: "section",
            de_id: String::new(),
            text: title,
            tooltip: String::new(),
            installed: false,
            available: true,
        });
        for meta in group {
            let is_installed = installed(meta.id);
            let avail = is_available(meta.id, family);
            out.push(ListRow {
                kind: "de",
                de_id: meta.id.to_string(),
                text: item_text(meta.id),
                tooltip: item_tooltip(meta.id, is_installed, avail),
                installed: is_installed,
                available: avail,
            });
        }
    }
    out
}

// ============================================================================
// The «Tools» panel
// ============================================================================

pub fn tools_header() -> String {
    tr("🧰 Tools")
}
/// Minimum control height: the Big Picture / Steam Deck canon.
pub const TOOL_BUTTON_MIN_HEIGHT: i32 = 40;

/// A tools panel button.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolButton {
    /// What to do: `compare`, `repair_current`, `panel_backup`,
    /// `panel_restore`.
    pub kind: &'static str,
    pub text: String,
    pub tooltip: String,
}

/// Whether Rexon can take a copy of the panel of the CURRENT desktop: KDE with
/// its own snapshot of Plasma configs; MATE with `dconf dump /org/mate/panel/`
/// (the same file as «Restore previous panel» in «Panel Layout», `mate_panel`);
/// XFCE with `xfce4-panel-profiles save/load` (package in Arch extra, Debian,
/// Fedora, openSUSE). `have(program)` is whether it is in the system.
pub fn panel_backup_supported(current_de: &str, have: &dyn Fn(&str) -> bool) -> bool {
    match current_de {
        "kde" => true,
        "mate" => have("mate-panel") && have("dconf"),
        "xfce" => have("xfce4-panel-profiles"),
        _ => false,
    }
}

/// Panel buttons for the CURRENT desktop. `panel_backup` is
/// [`panel_backup_supported`]: where there is nothing to take a copy with, the
/// buttons would lead to an error, so they are absent.
pub fn tool_buttons(_current_de: &str, panel_backup: bool) -> Vec<ToolButton> {
    let mut out = vec![
        ToolButton {
            kind: "compare",
            text: tr("⚔️ Compare Desktops"),
            tooltip: tr("A side-by-side comparison of desktops, quick matchups and recommendations"),
        },
        ToolButton {
            kind: "repair_current",
            text: tr("🔧 Check My Desktop"),
            tooltip: tr("Check that your current desktop has all of its packages, \
                and install any that are missing"),
        },
    ];
    if panel_backup {
        out.push(ToolButton {
            kind: "panel_backup",
            text: tr("🛟 Save Panel"),
            tooltip: tr("Save a copy of your panel settings — in case experimenting \
                with themes breaks it"),
        });
        out.push(ToolButton {
            kind: "panel_restore",
            text: tr("↩️ Restore Panel"),
            tooltip: tr("Restore the panel from the last saved copy"),
        });
    }
    out
}

/// Caption of the button returning from the embedded «Compare Desktops».
pub fn compare_back_text() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("← Desktops")
}

// ============================================================================
// The «Distro Looks» block
// ============================================================================
//
// The middle panel of the tab (owner's decision 25.10.2026: «Tools» moved
// to the bottom of the right panel, and this block took their place). One
// tile per built-in look (`distro_looks::builtin_ids`), the applied one has
// a mark and «Put Back as It Was». The «Apply» flow: an offer
// ([`crate::distro_looks::offer`]) → a question dialog ([`look_confirm`],
// with an "optional" checkbox for the SteamOS game mode) → applying in a
// worker thread → the result. The core executes (`distro_looks::apply_live`),
// the dialog is the tab's shared operation dialog.

use crate::distro_looks as dl;

pub fn looks_header() -> String {
    tr("🎨 Distro Looks")
}

/// The explanation under the block heading.
pub fn looks_hint() -> String {
    tr("The whole look of a well-known distribution in one go. You can always put everything back.")
}

/// A look tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LookTile {
    pub id: String,
    /// Name from the manifest in the interface language.
    pub name: String,
    /// Description from the manifest, as a hover tooltip.
    pub description: String,
    pub applied: bool,
    /// Mark of the applied one; empty means not applied.
    pub badge: String,
    /// Buttons: `look_apply` for one not applied, `look_revert` for the applied one.
    pub buttons: Vec<CardButton>,
}

/// Tiles of the built-in looks in order. `applied(id)` is whether it is applied.
/// A look that failed to parse gets no tile (guarded by
/// `distro_looks_manifests`, and a dead tile is of no use to the customer).
pub fn look_tiles(lang: &str, applied: &dyn Fn(&str) -> bool) -> Vec<LookTile> {
    let mut out = Vec::new();
    for id in dl::builtin_ids() {
        let Some(Ok(look)) = dl::builtin(id) else { continue };
        let is_applied = applied(id);
        let buttons = if is_applied {
            vec![CardButton {
                kind: "look_revert",
                text: tr("↩️ Put Back as It Was"),
                tooltip: tr("Return the look and settings you had before"),
                style: "neutral",
            }]
        } else {
            vec![CardButton {
                kind: "look_apply",
                text: tr("Apply"),
                tooltip: String::new(),
                style: "primary",
            }]
        };
        out.push(LookTile {
            id: id.to_string(),
            name: look.pkg.name.get(lang).to_string(),
            description: look.pkg.description.as_ref().map(|d| d.get(lang).to_string()).unwrap_or_default(),
            applied: is_applied,
            badge: if is_applied { tr("✓ Applied") } else { String::new() },
            buttons,
        });
    }
    out
}

/// Name of the look `id` from the manifest in the interface language (if there is none, the id itself).
pub fn look_name(id: &str) -> String {
    match dl::builtin(id) {
        Some(Ok(look)) => look.pkg.name.get(&crate::i18n::current_language()).to_string(),
        _ => id.to_string(),
    }
}

/// Nothing to apply: the plan did not come together (the question dialog does
/// not let such a case through; this is a safety net, not a path a person takes).
pub fn look_nothing_to_apply() -> String {
    tr("There is nothing to apply.")
}

/// Heading of the «Apply» progress.
pub fn look_apply_title(name: &str) -> String {
    trf("Applying “%1”", &[&name])
}

/// Progress line while the plan is being built (package probes take seconds).
pub fn look_checking() -> String {
    tr("Checking what is needed…")
}

/// The session's desktop is not recognized: there is nothing to apply the look to.
pub fn look_unknown_desktop() -> (String, String) {
    (
        tr("Can't apply the look"),
        tr("Rexon could not recognise the desktop you are using now, so it can't apply a look to it."),
    )
}

/// The "optional" checkbox in the question dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LookOption {
    pub text: String,
    /// The look's warning for this part (simple HTML); empty means none.
    pub hint: String,
}

/// An option of the question dialog switcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LookSelectOption {
    pub value: String,
    /// Caption from the manifest in the interface language.
    pub label: String,
    /// The explanation under the caption (simple HTML); empty means without it, the
    /// option as a plain button. An empty one is not written into look references.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `false` means the option is visible but cannot be chosen (why is in the
    /// explanation). An available one is not written into look references.
    #[serde(skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn is_true(v: &bool) -> bool {
    *v
}

/// A question dialog switcher: a list property of the look (the SteamOS style:
/// Vapor or VGUI2). Captions come from the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LookSelect {
    /// Property name (`variant`).
    pub name: String,
    pub label: String,
    /// Only options with which the look applies: an option with a refusal is not
    /// offered to the person at all.
    pub options: Vec<LookSelectOption>,
    pub current: String,
}

/// What to show in the «Apply» question dialog.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum LookConfirm {
    /// The look does not apply: a dialog with a single «Got It» button.
    Blocked { title: String, text: String },
    /// A question: text (simple HTML), «Yes»/«No» captions, option switchers
    /// and the "optional" checkbox.
    Ask {
        title: String,
        text: String,
        yes_text: String,
        no_text: String,
        choices: Vec<LookSelect>,
        option: Option<LookOption>,
    },
}

fn esc(s: &str) -> String {
    crate::qt_extensions::escape_html(s)
}

/// Whether the "optional" checkbox is offered: the plan with it came together,
/// and the optional part is mentioned in it, either installed or skipped for
/// a reason (for example, an AUR package is needed). If the desktop does not
/// understand such a part at all, there is no checkbox.
fn opt_in_offered(variant: &dl::OfferVariant) -> bool {
    let Some(dl::Prepared::Ready(p)) = &variant.opt_in else { return false };
    let opt = |k: &str| dl::OPT_IN_KINDS.contains(&k);
    p.items.iter().any(|i| opt(&i.kind)) || p.skipped.iter().any(|s| opt(&s.kind) && s.reason != dl::not_chosen_reason())
}

/// Option switchers with the chosen values `values`.
fn look_choices(offer: &dl::Offer, values: &dl::OfferVariant, lang: &str) -> Vec<LookSelect> {
    let mut out = Vec::new();
    for (name, prop) in dl::choice_properties(&offer.look) {
        let current = values.values.get(name).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        let options: Vec<LookSelectOption> = prop
            .options
            .iter()
            .filter(|o| {
                let mut want = values.values.clone();
                want.insert(name.to_string(), serde_json::Value::String(o.value().to_string()));
                let v = offer.variant(&want);
                v.values.get(name).and_then(serde_json::Value::as_str) == Some(o.value())
                    && matches!(v.base, dl::Prepared::Ready(_))
            })
            .map(|o| LookSelectOption {
                value: o.value().to_string(),
                label: o.label(lang).to_string(),
                description: String::new(),
                enabled: true,
            })
            .collect();
        if options.len() < 2 {
            continue;
        }
        out.push(LookSelect {
            name: name.to_string(),
            label: prop.label.as_ref().map(|l| l.get(lang).to_string()).unwrap_or_else(|| name.to_string()),
            options,
            current,
        });
    }
    out
}

/// Question dialog for the offer `offer`: `values` are the chosen options
/// (empty means defaults), `opt_in` is whether the checkbox is checked.
///
/// Decisions (owner's rule 24.10): programs from the official repositories
/// are installed right away (the system password dialog is the protection),
/// AUR ones never: such packages are listed to the person. If something
/// mandatory cannot be installed, a refusal dialog, nothing is applied.
pub fn look_confirm(offer: &dl::Offer, values: &crate::rxn_manifest::PropValues, opt_in: bool) -> LookConfirm {
    let lang = crate::i18n::current_language();
    let name = offer.look.pkg.name.get(&lang).to_string();
    let variant = offer.variant(values);
    let offered = opt_in_offered(variant);
    let prepared = match (&variant.opt_in, opt_in && offered) {
        (Some(p), true) => p,
        _ => &variant.base,
    };
    let plan = match prepared {
        dl::Prepared::Refused { reason, aur } => {
            let mut text = esc(reason).replace('\n', "<br>");
            if !aur.is_empty() {
                text.push_str("<br><br>");
                text.push_str(&esc(&dl::aur_text(aur)));
            }
            return LookConfirm::Blocked { title: trf("“%1” can't be applied", &[&name]), text };
        }
        dl::Prepared::Ready(p) => p,
    };
    let opt = |k: &str| dl::OPT_IN_KINDS.contains(&k);
    let mut lines: Vec<String> = Vec::new();
    // TRANSLATORS: %1 is the name of a look, e.g. “Like SteamOS”.
    lines.push(trf("The look <b>%1</b> will be applied to your desktop.", &[&esc(&name)]));
    for (_, other) in &offer.replaces {
        lines.push(trf("First, “%1” will be put back as it was.", &[&esc(other)]));
    }
    let mut kinds: Vec<String> = Vec::new();
    for it in &plan.items {
        let label = dl::kind_label(&it.kind);
        if !kinds.contains(&label) {
            kinds.push(label);
        }
    }
    if !kinds.is_empty() {
        lines.push(trf("What will change: %1.", &[&esc(&kinds.join(", "))]));
    }
    // Say plainly what Rexon assembles itself from parts of this desktop (instead
    // of taking the author's files): it is close to the intent, but not the same.
    let mut built: Vec<String> = Vec::new();
    for it in plan.items.iter().filter(|i| matches!(i.choice, crate::rxn_manifest::Choice::Generate { .. })) {
        let label = dl::kind_label(&it.kind);
        if !built.contains(&label) {
            built.push(label);
        }
    }
    if !built.is_empty() {
        lines.push(trf(
            "Rexon builds these parts itself from what your desktop has, close to the original: %1.",
            &[&esc(&built.join(", "))],
        ));
    }
    // Replacing the window manager is a noticeable action: say it plainly and
    // on a separate line (owner's decision 31.10.2026: yes by default).
    if let Some(wm) = dl::compiz_replaced_wm(plan) {
        lines.push(trf(
            "Window effects of this look (wobbly windows, magic lamp, cover switch): Rexon will replace the %1 window manager with Compiz. If Compiz does not start, %1 comes back by itself.",
            &[&wm],
        ));
        if plan.aur_build.iter().any(|a| a.role == dl::COMPIZ_ROLE) {
            lines.push(tr("Compiz is built from the AUR: this takes several minutes, and your system will ask for your password."));
        }
    }
    for c in &plan.covered {
        lines.push(trf("Covered by the other parts: %1 — %2", &[&esc(&dl::kind_label(&c.kind)), &esc(&c.reason)]));
    }
    for l in &plan.limits {
        lines.push(trf("Partly: %1 — %2", &[&esc(&dl::kind_label(&l.kind)), &esc(&l.reason)]));
    }
    let mut install: Vec<String> = Vec::new();
    for d in &plan.install {
        for n in d.resolved.install_names() {
            if !install.contains(&n) {
                install.push(n);
            }
        }
    }
    if !install.is_empty() {
        lines.push(trf(
            "Rexon will install the programs it needs: %1. Your system will ask for your password.",
            &[&esc(&install.join(", "))],
        ));
    }
    if !plan.extensions.is_empty() {
        let uuids: Vec<&str> = plan.extensions.iter().map(|e| e.uuid.as_str()).collect();
        lines.push(trf(
            "Rexon will install the GNOME extensions it needs: %1. If GNOME Shell is running, it will ask you to confirm.",
            &[&esc(&uuids.join(", "))],
        ));
    }
    for s in &plan.skipped {
        // An optional part the person did not choose is the checkbox below,
        // not "will not be applied".
        if offered && opt(&s.kind) && !opt_in {
            continue;
        }
        if s.reason == dl::not_chosen_reason() {
            continue;
        }
        lines.push(trf("Won't be applied: %1 — %2", &[&esc(&dl::kind_label(&s.kind)), &esc(&s.reason)]));
    }
    if !plan.aur.is_empty() {
        lines.push(esc(&dl::aur_text(&plan.aur)));
    }
    lines.push(tr("You can put everything back at any time with the “Put Back as It Was” button."));
    let option = offered.then(|| {
        let kind = offer
            .look
            .pkg
            .items
            .iter()
            .map(|i| i.kind.as_str())
            .find(|k| opt(k))
            .unwrap_or_default()
            .to_string();
        LookOption {
            // TRANSLATORS: %1 is an optional part of a look, e.g. “game mode”.
            text: trf("Also turn on %1", &[&dl::kind_label(&kind)]),
            hint: esc(&dl::kind_note(&offer.look, &kind, &lang)),
        }
    });
    LookConfirm::Ask {
        title: trf("Apply “%1”?", &[&name]),
        text: lines.join("<br><br>"),
        yes_text: tr("Apply"),
        no_text: crate::system_de_exec::dialog_cancel(),
        choices: look_choices(offer, variant, &lang),
        option,
    }
}

/// The plan that will be applied with this checkbox choice; `None` means
/// nothing to apply (refusal).
pub fn look_chosen_plan<'a>(offer: &'a dl::Offer, values: &crate::rxn_manifest::PropValues, opt_in: bool) -> Option<&'a dl::LookPlan> {
    let variant = offer.variant(values);
    let prepared = match (&variant.opt_in, opt_in && opt_in_offered(variant)) {
        (Some(p), true) => p,
        _ => &variant.base,
    };
    match prepared {
        dl::Prepared::Ready(p) => Some(p),
        dl::Prepared::Refused { .. } => None,
    }
}

/// The «Put Back as It Was» question: heading, text (simple HTML), «Yes».
pub fn look_revert_confirm(name: &str) -> (String, String, String) {
    (
        trf("Put “%1” back as it was?", &[&name]),
        tr("Rexon will return the look and settings you had before. The programs installed for it stay — you can remove them yourself if you don't need them."),
        tr("Put Back"),
    )
}

/// Heading of the «Put Back as It Was» progress.
pub fn look_revert_title(name: &str) -> String {
    trf("Putting “%1” back", &[&name])
}

/// The result under the log (simple HTML) from the core's result.
pub fn look_result_html(message: &str) -> String {
    esc(message).replace('\n', "<br>")
}

// ============================================================================
// Right panel: the desktop card
// ============================================================================

pub fn placeholder_text() -> String {
    // The desktop list is at the start of the line: in right-to-left languages it
    // is on the right, and the translation must say so (the caption is not physical).
    // TRANSLATORS: in right-to-left languages the list is on the right: say "on the right" and use the arrow →.
    // Also: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("← Choose a desktop from the list on the left")
}
pub const CARD_TITLE_POINT_SIZE: i32 = 22;
pub const CARD_STATUS_POINT_SIZE: i32 = 11;
/// Card width ceiling and the guaranteed gap on the left.
pub const CARD_MAX_WIDTH: i32 = 1340;
pub const CARD_GAP_LEFT: i32 = 12;

/// Green "installed" in the card header (widget tab: `#4ade80`).
pub const STATUS_INSTALLED_COLOR: &str = "#4ade80";
/// Amber "unavailable" (widget tab: `rgba(245,158,11,1)`).
pub const STATUS_UNAVAILABLE_COLOR: &str = "#f59e0b";
/// Green "this is your current desktop" (widget tab: `rgba(34,197,94,1)`).
pub const CURRENT_BADGE_COLOR: &str = "#22c55e";
// The red remove button is drawn by the shared `RexonDangerButton` element,
// and its color and both opacities live in `qt_extensions::DANGER_*`: the
// tab does not introduce its own shade (rule #12).

pub fn status_installed() -> String {
    tr("✅ Installed")
}
/// Card header when less than half of the desktop's set is installed.
pub fn status_partial() -> String {
    tr("⚠️ Partially installed")
}
/// "Partially": more than half of the desktop's own packages are missing. One
/// or two new group packages (pluma-plugins for MATE) mean «Installed» with
/// the hint «Check / Repair», not "partially".
pub fn is_partial_install(p: &crate::system_plan::InstallPreview) -> bool {
    let total = p.members.len();
    total > 0 && p.missing_members().len() * 2 > total
}
pub fn status_not_installed() -> String {
    tr("❌ Not installed")
}
pub fn status_unavailable() -> String {
    tr("⚠️ Unavailable")
}
pub fn unavailable_notice() -> String {
    tr("⚠️ Not available on your distribution")
}
pub fn current_badge_text() -> String {
    tr("✅ Current desktop")
}
pub fn current_badge_tooltip() -> String {
    tr("You're using this desktop right now")
}

/// One column of the spec strip under the screenshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatCell {
    /// Top line: an icon or repeated stars.
    pub icon: String,
    /// Middle line; empty means the space is held by a transparent dot.
    pub value: String,
    /// Bottom caption.
    pub label: String,
}

/// Placeholder for an empty spec value: the space under it is kept, the text is invisible.
pub const STAT_EMPTY_VALUE: &str = "·";

/// The desktop spec strip: memory, load, customization, gaming friendliness.
///
/// The customization and gaming icons are repeated by rating (not padded with
/// empty stars up to five as in «Compare Desktops», which has its own
/// markup), but at least one: otherwise a zero rating would give an empty
/// column with no hint of what it even means.
pub fn stats(de_id: &str) -> Vec<StatCell> {
    let Some(meta) = meta(de_id) else {
        return Vec::new();
    };
    vec![
        StatCell {
            icon: "💾".to_string(),
            // TRANSLATORS: memory use of a desktop, %1 is a number of megabytes.
            value: trf("%1 MB", &[&meta.ram_mb]),
            // «Memory» and «Processor», not RAM and CPU: abbreviations mean nothing to
            // a customer coming from Windows (rule #13).
            label: trc("desktop spec", "Memory"),
        },
        StatCell {
            icon: "⚡".to_string(),
            value: meta.cpu_usage.to_string(),
            label: trc("desktop spec", "Processor"),
        },
        StatCell {
            // The rating comes from one function shared with «Compare» (`qt_compare::stars`):
            // filled icons plus «☆» up to five. Previously there were only filled ones
            // here, and «Gaming 4» differed from «Gaming 5» only in the length of the
            // line (01.11.2026, rule #12).
            icon: crate::qt_compare::stars(meta.customizable, "⭐"),
            value: String::new(),
            // TRANSLATORS: stat label under stars: how customizable a desktop is.
            label: tr("Customization"),
        },
        StatCell {
            icon: crate::qt_compare::stars(meta.gaming_friendly, "🎮"),
            value: String::new(),
            // TRANSLATORS: stat label under gamepads: how good a desktop is for games.
            label: tr("Gaming"),
        },
    ]
}

/// The desktop description without the leading icon.
///
/// The card heading already carries the icon, it is not needed a second time.
/// ANY first token without letters and digits is stripped, not a list of known
/// ones: a hard-coded list drifted away from the database (LXQt's feather `🪶`
/// was not in it and got duplicated).
pub fn description(de_id: &str) -> String {
    let Some(meta) = meta(de_id) else {
        return String::new();
    };
    // The description goes through the translation catalog (`DeMeta::description_text`).
    let description = meta.description_text();
    let text = description.trim_start();
    let (first, rest) = match text.split_once(' ') {
        Some((f, r)) => (f, r),
        None => (text, ""),
    };
    if !first.is_empty() && !first.chars().any(|c| c.is_alphanumeric()) {
        return rest.trim_start().to_string();
    }
    text.to_string()
}

/// The line "how many packages will be installed and from where".
///
/// The source is ALWAYS named: without the note the line looked unfinished.
pub fn packages_text(main_count: usize, aur_count: usize) -> String {
    let total = (main_count + aur_count) as u64;
    if aur_count > 0 {
        fill(
            &ntr(
                "📦 %n package to install  (%1 main + %2 AUR)",
                "📦 %n packages to install  (%1 main + %2 AUR)",
                total,
            ),
            &[&main_count, &aur_count],
        )
    } else {
        ntr(
            "📦 %n package to install  (all from official repositories)",
            "📦 %n packages to install  (all from official repositories)",
            total,
        )
    }
}

/// The card's "packages" block: a line about the contents and, if any, a warning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PackagesView {
    /// How many will be installed / what is in place; may span two lines.
    pub text: String,
    /// What cannot be installed right now and what follows from it; empty means all is well.
    pub notice: String,
    /// The set will not install because of a conflict with what is installed: the
    /// buttons that install it ([`installs_desktop_set`]) are visible but disabled,
    /// and the reason is in [`Self::notice`] right above them.
    pub install_blocked: bool,
    /// Only the smaller part of the desktop's set is installed (a live case on
    /// 08.11.2026: after GNOME was removed, only gnome-shell was installed back by
    /// hand, while 45 programs of the desktop were missing): the card header says
    /// «Partially installed» instead of «Installed». Empty means the header stays as is.
    pub status_override: String,
}

/// Whether the action installs the desktop's package set («Install», «Check /
/// Repair»). Such buttons are disabled while the set conflicts with what is
/// installed ([`PackagesView::install_blocked`]).
pub fn installs_desktop_set(kind: &str) -> bool {
    matches!(kind, "install" | "repair")
}

/// Whether «Rebuild from Arch sources» helps: with a conflict with what is
/// installed, it does not. A rebuild fixes a package waiting for a new
/// library, while the conflict (mutter49 against mutter) remains, and the
/// button would only run the build in vain.
pub fn rebuild_can_help(p: &crate::system_plan::InstallPreview) -> bool {
    p.conflicts.is_empty()
}

/// While the trial transaction runs (`pacman -Sp`, fractions of a second to seconds).
pub fn packages_counting() -> String {
    tr("📦 Counting packages…")
}

/// How many names to show in the list of missing ones; after that, "…".
const PACKAGES_LIST_MAX: usize = 5;

fn short_list(names: &[String]) -> String {
    let mut out = names
        .iter()
        .take(PACKAGES_LIST_MAX)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > PACKAGES_LIST_MAX {
        out.push_str(", …");
    }
    out
}

/// The card's "packages" block: HONEST, by the desktop state.
///
/// ⚠️ Before: an installed COSMIC showed "📦 29 packages to install", and
/// Pantheon "3 packages to install": the NAMES from the core database were
/// counted, and the `pantheon` group of 63 packages counted as one. Now,
/// where the trial transaction answered (`preview`, Arch), the number is what
/// will ACTUALLY be installed, with groups and dependencies expanded; an
/// installed desktop gets "everything is in place" or which packages went
/// missing. Without an answer (other families, pacman is silent) the name
/// count remains, but an installed desktop is never told "to install" even then.
pub fn packages_view(
    installed: bool,
    preview: Option<&crate::system_plan::InstallPreview>,
    main_count: usize,
    aur_count: usize,
) -> PackagesView {
    let Some(p) = preview else {
        let text = if installed {
            ntr(
                "📦 Desktop set: %n package",
                "📦 Desktop set: %n packages",
                (main_count + aur_count) as u64,
            )
        } else {
            packages_text(main_count, aur_count)
        };
        return PackagesView { text, notice: String::new(), install_blocked: false, status_override: String::new() };
    };

    let notice = packages_notice(p);
    let missing = p.new_packages.len();
    let text = if installed {
        // Missing packages of the desktop ITSELF are named; their dependencies
        // will come along by themselves and would only clutter the list (for
        // Budgie without budgie-extras, granite and python-cairocffi came first).
        let mut gone = p.missing_members();
        if gone.is_empty() {
            gone = p.new_packages.clone();
        }
        let mut text = if missing == 0 {
            ntr(
                "✅ All %n package of this desktop is in place",
                "✅ All %n packages of this desktop are in place",
                p.members.len() as u64,
            )
        } else {
            let button = tr("🔧 Check / Repair");
            let button = button.trim_start_matches(|c: char| !c.is_alphanumeric());
            format!(
                "{}\n{}",
                fill(
                    &ntr(
                        "⚠️ %n package is missing: %1",
                        "⚠️ %n packages are missing: %1",
                        gone.len() as u64,
                    ),
                    &[&short_list(&gone)],
                ),
                trf(
                    // TRANSLATORS: %1 is the label of the “Check / Repair”
                    // button on the desktop's card.
                    "“%1” will install what is missing.",
                    &[&button],
                ),
            )
        };
        // Installed packages for which a new build came out are not missing
        // (19.10.2026: MATE said "2 packages missing" about the installed
        // mate-applets and mate-netbook). The desktop's own packages are named;
        // there is no point listing dependency updates.
        let mut updates: Vec<String> =
            p.updates.iter().filter(|u| p.members.contains(u)).cloned().collect();
        if updates.is_empty() {
            updates = p.updates.clone();
        }
        if !updates.is_empty() {
            text.push('\n');
            text.push_str(&fill(
                &ntr(
                    "🔄 %n package has an update: %1 — it will come with the regular system update.",
                    "🔄 %n packages have an update: %1 — it will come with the regular system update.",
                    updates.len() as u64,
                ),
                &[&short_list(&updates)],
            ));
        }
        // An own build newer than the repository is neither an update nor missing:
        // «Check / Repair» does not touch it (`InstallPreview::kept_local`).
        if !p.kept_local.is_empty() {
            text.push('\n');
            text.push_str(&fill(
                &ntr(
                    "🛠 %n package is your own build, newer than the repository: %1 — Rexon leaves it as is.",
                    "🛠 %n packages are your own builds, newer than the repository: %1 — Rexon leaves them as is.",
                    p.kept_local.len() as u64,
                ),
                &[&short_list(&p.kept_local)],
            ));
        }
        text
    } else {
        let mut lines = vec![packages_text(missing, aur_count)];
        let mut parts: Vec<String> = p
            .groups
            .iter()
            .map(|(group, count)| {
                fill(
                    &ntr("“%1” group: %n package", "“%1” group: %n packages", *count as u64),
                    &[group],
                )
            })
            .collect();
        let deps = p.dependency_count();
        if deps > 0 {
            parts.push(ntr("dependencies: %n", "dependencies: %n", deps as u64));
        }
        let already = p.installed_members();
        if already > 0 {
            parts.push(ntr(
                "already installed: %n",
                "already installed: %n",
                already as u64,
            ));
        }
        if !parts.is_empty() {
            lines.push(parts.join("  ·  "));
        }
        lines.join("\n")
    };
    let status_override = if installed && is_partial_install(p) {
        status_partial()
    } else {
        String::new()
    };
    PackagesView { text, notice, install_blocked: !p.conflicts.is_empty(), status_override }
}

/// Warning of the "packages" block: what from the set cannot be installed right now.
pub fn packages_notice(p: &crate::system_plan::InstallPreview) -> String {
    if !p.conflicts.is_empty() {
        return crate::system_plan::conflict_explanation(&p.conflicts);
    }
    if !p.blocked.is_empty() {
        return fill(
            &ntr(
                "⛔ Installation won't work right now: %1 can't be installed — \
                 its dependencies are missing from the repository (usually it's \
                 waiting for a rebuild). Try again later.",
                "⛔ Installation won't work right now: %1 can't be installed — \
                 their dependencies are missing from the repository (usually \
                 they're waiting for a rebuild). Try again later.",
                p.blocked.len() as u64,
            ),
            &[&short_list(&p.blocked)],
        );
    }
    let mut lines: Vec<String> = Vec::new();
    if !p.conflict_skipped.is_empty() {
        lines.push(crate::system_plan::conflict_skip_explanation(&p.conflict_skipped));
    }
    if !p.skipped.is_empty() {
        lines.push(fill(
            &ntr(
                "⚠️ %1 can't be installed right now — its dependencies are \
                 missing from the repository (usually it's waiting for a \
                 rebuild). Everything else will be installed without it.",
                "⚠️ %1 can't be installed right now — their dependencies are \
                 missing from the repository (usually they're waiting for a \
                 rebuild). Everything else will be installed without them.",
                p.skipped.len() as u64,
            ),
            &[&short_list(&p.skipped)],
        ));
    }
    lines.join("\n")
}

/// A card button.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardButton {
    /// `install`, `switch`, `repair`, `remove`.
    pub kind: &'static str,
    pub text: String,
    /// Tooltip; empty means no tooltip.
    pub tooltip: String,
    /// `primary` is highlighted, `neutral` is regular, `destructive` is red.
    pub style: &'static str,
}

/// The set of card buttons by desktop state.
///
/// ⛔ The key place of the foolproofing (rule #13): the CURRENT desktop has
/// neither «Switch» (we are already in it) nor «Remove»: after logging out
/// the person would be left without graphics. The second line of the same
/// protection is [`remove_plan`], the third is `system::remove_de(force=false)` in the core.
pub fn card_buttons(installed: bool, available: bool, is_current: bool) -> Vec<CardButton> {
    if !available {
        return Vec::new();
    }
    if !installed {
        return vec![CardButton {
            kind: "install",
            text: tr("📦 Install"),
            tooltip: String::new(),
            style: "primary",
        }];
    }
    let mut out = Vec::new();
    if !is_current {
        out.push(CardButton {
            kind: "switch",
            text: tr("🔄 Switch"),
            tooltip: tr("Switch to this desktop (a copy of your current desktop's settings will be saved)"),
            style: "primary",
        });
    }
    out.push(CardButton {
        kind: "repair",
        text: tr("🔧 Check / Repair"),
        tooltip: tr("Check that all of this desktop's packages are in place, and install any that are missing"),
        style: "neutral",
    });
    if !is_current {
        out.push(CardButton {
            kind: "remove",
            text: tr("🗑️ Remove"),
            tooltip: String::new(),
            style: "destructive",
        });
    }
    out
}

/// Whether the action touches system packages (installs, completes, removes).
///
/// Such buttons on SteamOS (Steam Deck and Steam Machine) are decided by
/// [`steamos_button_block`]. «Switch» does not touch packages and stays as
/// is; so do «Compare» and the panel copies.
pub fn is_package_action(kind: &str) -> bool {
    matches!(kind, "install" | "repair" | "remove" | "repair_current")
}

/// Reason for a disabled button `kind` on the card of desktop `de_id` on SteamOS;
/// `None` means the button works (or it is not SteamOS).
///
/// Tested desktops (`steamos_sysext::DESKTOPS`) are installed, rebuilt and
/// removed as a systemd-sysext image, so their buttons are live; the others
/// get a reason from [`crate::distro_compat::steamos_desktop_card_block`].
/// Rebuilding the image of the desktop the person is working in RIGHT NOW
/// («Check / Repair» on its card and «Check My Desktop») is not offered:
/// swapping the image under a live session has not been tested in the VM.
pub fn steamos_button_block(kind: &str, de_id: &str, is_current: bool, steamos: bool) -> Option<String> {
    use crate::distro_compat::{steamos_desktop_card_block, SteamOsBlock};
    if !steamos || !is_package_action(kind) {
        return None;
    }
    if kind == "repair_current" || (kind == "repair" && is_current) {
        return Some(SteamOsBlock::Desktops.reason());
    }
    steamos_desktop_card_block(de_id, true)
}

/// SteamOS: texts of the install dialog (and «Check / Repair») of a desktop
/// installed as a systemd-sysext image. `steamos_version` is SteamOS's
/// `VERSION_ID` (`None` means not SteamOS); for other desktops and outside
/// SteamOS, `base` as is. The question comes from `steamos_sysext`: the
/// package text ("packages", "choose on the login screen") is untrue there.
/// The executor writes the result to the log itself
/// (`steamos_sysext::install_done_text` or a warning that the desktop did not
/// become a session), so there is no note under the log (empty `done_text*`):
/// otherwise "all done" would stand under the warning.
pub fn steamos_op_texts(base: OpTexts, de_id: &str, steamos_version: Option<&str>) -> OpTexts {
    use crate::steamos_sysext::{install_confirm_text, support, Support};
    let (Some(version), Support::Supported(spec)) = (steamos_version, support(de_id)) else {
        return base;
    };
    OpTexts {
        confirm_html: install_confirm_text(spec.name, version),
        done_text: String::new(),
        done_text_restart: String::new(),
        ..base
    }
}

/// SteamOS: the question before removing an image desktop; `None` means the
/// regular `remove_plan` text (not SteamOS, or the desktop is not in `steamos_sysext::DESKTOPS`).
pub fn steamos_remove_text(de_id: &str, steamos: bool) -> Option<String> {
    use crate::steamos_sysext::{remove_confirm_text, support, Support};
    match support(de_id) {
        Support::Supported(spec) if steamos => Some(remove_confirm_text(spec.name)),
        _ => None,
    }
}

/// Card header: the status text, its color and boldness.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusView {
    pub text: String,
    /// An empty string means no coloring, it is regular secondary text.
    pub color: &'static str,
    pub bold: bool,
}

pub fn status_view(installed: bool, available: bool) -> StatusView {
    if installed {
        return StatusView {
            text: status_installed(),
            color: STATUS_INSTALLED_COLOR,
            bold: true,
        };
    }
    if available {
        StatusView {
            text: status_not_installed(),
            color: "",
            bold: false,
        }
    } else {
        StatusView {
            text: status_unavailable(),
            color: STATUS_UNAVAILABLE_COLOR,
            bold: true,
        }
    }
}

// ── Version from the distribution repositories ──────────────────────────

/// While the package manager answers.
pub fn version_checking() -> String {
    tr("Checking version…")
}

/// The line "what will actually be downloaded"; empty means hide the line.
///
/// This is the version from the repositories of the CURRENT distribution, not
/// "the latest in the world": the repository may lag behind the developers,
/// and it is more honest to show what will arrive on the machine.
pub fn version_line(version: &str) -> String {
    if version.is_empty() {
        String::new()
    } else {
        trf("Available in your distribution: %1", &[&version])
    }
}

/// The version line of an INSTALLED desktop; empty means hide the line.
///
/// The version is of the same key package of the desktop (`check_package` of
/// the core database) as «Available in your distribution» of one not
/// installed: otherwise the two lines would talk about different packages.
/// `repo_newer` means the repository version is newer than the installed one
/// (`Some(true)`), not newer (`Some(false)`) or there is nothing to compare
/// with (`None`: then on a mismatch the second version is simply named).
pub fn installed_version_line(installed: &str, available: &str, repo_newer: Option<bool>) -> String {
    if installed.is_empty() {
        return String::new();
    }
    if available.is_empty() || available == installed || repo_newer == Some(false) {
        return trf("Installed version: %1", &[&installed]);
    }
    if repo_newer == Some(true) {
        trf("Installed version: %1, available: %2", &[&installed, &available])
    } else {
        trf("Installed version: %1, in the repository: %2", &[&installed, &available])
    }
}

// ============================================================================
// Screenshot carousel
// ============================================================================

pub fn no_screenshots_text() -> String {
    tr("📷  No screenshots")
}
pub fn carousel_zoom_tooltip() -> String {
    tr("Click the picture to view it full screen")
}
pub fn carousel_prev_tooltip() -> String {
    tr("Previous screenshot")
}
pub fn carousel_next_tooltip() -> String {
    tr("Next screenshot")
}
pub fn carousel_zoom_button_tooltip() -> String {
    tr("View full screen")
}
pub fn zoom_prev_tooltip() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("Previous (←)")
}
pub fn zoom_next_tooltip() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("Next (→)")
}
pub fn zoom_close_tooltip() -> String {
    tr("Close")
}
pub fn zoom_title() -> String {
    trc("screenshot viewer title", "Screenshot")
}

/// Minimum height of the screenshot frame.
pub const CAROUSEL_MIN_HEIGHT: i32 = 320;
/// Frame height ceiling in pixels and as a fraction of the window.
pub const CAROUSEL_MAX_HEIGHT: i32 = 720;
pub const CAROUSEL_WINDOW_SHARE: f64 = 0.55;

/// Screenshot frame height: 16:9 of the available width, but no lower than 320
/// and no higher than 55% of the window or 720 pixels.
///
/// On a maximized Full HD window the screenshot was pinned to 320 and got lost
/// in a stretched frame: it could not be read.
pub fn carousel_height(window_height: i32, available_width: i32) -> i32 {
    let cap = CAROUSEL_MIN_HEIGHT
        .max((window_height as f64 * CAROUSEL_WINDOW_SHARE) as i32)
        .min(CAROUSEL_MAX_HEIGHT);
    let ideal = available_width * 9 / 16;
    CAROUSEL_MIN_HEIGHT.max(ideal.min(cap))
}

/// How much the screenshot frame is ready to shrink when the card lacks height.
///
/// ⚠️ THE NUMBER IS MEASURED, NOT DERIVED. Measured 19.09.2026 on the standard
/// 1100×750 window (steam_dark skin, the «Tools» panel after `41075f1`, card
/// 569 points wide), over all eleven desktops of the tab:
///
/// ```text
/// card viewport                676 (no footer)     642 (with footer)
/// card needs: nine DEs         666                 666
/// card needs: hyprland,        687                 687
///             niri
/// ```
///
/// So with the status footer the card lacks 24 points, and for hyprland and
/// niri 45, and exactly that much the bottom row of buttons slid under the
/// footer. For these two the card did not fit even WITHOUT the footer: by 11 points.
///
/// The ceiling has a margin for the «Checking version…» line, which was not
/// on the bench (the package manager is silent there), while on a live
/// machine it adds about 20 more points to the card: 45 + 20 = 65, rounded up
/// to 72, a multiple of eight, the tab's spacing step. This is a safety
/// limit, not a target: the frame gives up exactly as much as is missing,
/// and the viewport never goes below the product's smallest window (750 points).
pub const CAROUSEL_MAX_SQUEEZE: i32 = 72;

/// Height of the screenshot frame shrunk to fit the card viewport.
///
/// `full` is the height from [`carousel_height`], `room` is how much height is
/// left for the frame after everything else in the card (margins, the text
/// column, indicator dots). The frame gives the card no more than
/// [`CAROUSEL_MAX_SQUEEZE`]: the screenshot is the main thing on the card, and
/// it must not be dissolved entirely for the sake of buttons, while the card can scroll.
///
/// ⚠️ This height must NOT change the frame WIDTH, and this is not a style nitpick.
/// The width is computed from the UNshrunk height; compute it from here and
/// you get a "ratchet": narrower frame → narrower text column → the
/// description wraps to a new line → the card needs even more height → the
/// frame shrinks again. Shrunk, the frame simply crops the screenshot more vertically.
pub fn carousel_height_fitted(full: i32, room: i32) -> i32 {
    full.min(room.max(full - CAROUSEL_MAX_SQUEEZE))
}

// ============================================================================
// Markup colors and sizes
// ============================================================================
//
// The widget tab's stylesheet written out into the core as numbers. QML
// introduces no colors and sizes of its own: they come as the `palette` and
// `metrics` maps from DesktopsModel (rust/rexon-gui/src/desktops_page.rs).
// Colors are written the way QSS understands them (`rgba(...)`), and
// `ui_base::css_color_to_qml` converts them into `#AARRGGBB` for QML, one
// converter for the product. Agreement with the original is guarded by
// `qt_desktops_parity::layout_numbers_match_python`.

// ── Left list ───────────────────────────────────────────────────────────

/// Margins of the list panel: left, top, right, bottom.
pub const LIST_MARGINS: (i32, i32, i32, i32) = (12, 16, 8, 12);
pub const LIST_SPACING: i32 = 8;
pub const LIST_MIN_WIDTH: i32 = 200;
pub const LIST_MAX_WIDTH: i32 = 280;
/// Initial width of the panel in the splitter.
pub const LIST_INITIAL_WIDTH: i32 = 240;
/// Padding of a list item (QSS: `padding: 8px 12px`) and its corner radius.
pub const LIST_ITEM_PADDING_V: i32 = 8;
pub const LIST_ITEM_PADDING_H: i32 = 12;
pub const LIST_ITEM_RADIUS: i32 = 4;
/// Background of the selected item and its text (QSS: `color: white`).
pub const LIST_SELECTED_QSS: &str = "rgba(59,130,246,0.4)";
pub const LIST_SELECTED_TEXT_COLOR: &str = "#ffffff";

/// Background of the hovered item: white on a dark theme, black on a light one.
pub fn list_hover_qss(dark: bool) -> &'static str {
    if dark {
        "rgba(255,255,255,0.05)"
    } else {
        "rgba(0,0,0,0.05)"
    }
}

// ── Tools panel ─────────────────────────────────────────────────────────

pub const TOOLS_MARGINS: (i32, i32, i32, i32) = (8, 16, 8, 12);
pub const TOOLS_SPACING: i32 = 8;
pub const TOOLS_MIN_WIDTH: i32 = 160;
pub const TOOLS_INITIAL_WIDTH: i32 = 170;
/// Width ceiling of the button column: buttons do not stretch across the whole panel.
pub const TOOL_BUTTON_MAX_WIDTH: i32 = 240;

/// Width ceiling of the middle panel's tile column («Distro Looks»): tiles
/// do not stretch across the whole panel. The same as for the button column
/// of the former «Tools» in this place ([`TOOL_BUTTON_MAX_WIDTH`]).
pub const LOOKS_COLUMN_MAX_WIDTH: i32 = TOOL_BUTTON_MAX_WIDTH;

/// The smallest width of the MIDDLE panel (since 25.10.2026 the «Distro
/// Looks» block; before that «Tools» lived there) at which its captions are
/// fully visible. `header_width` is how much the block heading asks for,
/// `column_width` is how much the tile column asks for under the longest
/// label; Qt itself measures both by the font.
///
/// ⚠️ WHY A SEPARATE NUMBER, NOT [`TOOLS_MIN_WIDTH`]. 160 is
/// `tools.setMinimumWidth(160)` of the widget original, and it suited ITS
/// captions. Qt itself computed the panel minimum differently, from the
/// content: a measurement on 18.09 on the widget tab gave
/// `tools.minimumSizeHint().width() == 165`, that is 149 for the buttons plus
/// 8+8 margins. Exactly this mechanism is repeated here: at the standard
/// minimum window of 1100×750 the splitter gives the panel exactly 160
/// ([`split_widths`]), while the block heading asks for more; without this
/// the caption would break mid-word at the edge of the neighbouring card.
///
/// Since 28.10.2026 «Tools» live in the middle panel again, to the right of
/// «Distro Looks» or below it ([`looks_tools_side_by_side`]). The panel's
/// floor follows the wider of the two blocks: the page passes here the
/// largest heading and the largest column, because on a narrow panel the
/// blocks stack and each gets its full width.
///
/// Splitting space between panels still follows the original ([`split_widths`]).
pub fn looks_panel_min_width(header_width: i32, column_width: i32) -> i32 {
    let content = column_width
        .clamp(0, LOOKS_COLUMN_MAX_WIDTH)
        .max(header_width.max(0));
    TOOLS_MIN_WIDTH.max(content + TOOLS_MARGINS.0 + TOOLS_MARGINS.2)
}

/// The gap between the middle panel blocks («Distro Looks» and «Tools»): the
/// panel margin on the left plus on the right, with a thin line in the panel
/// border color in the middle.
pub const LOOKS_TOOLS_GAP: i32 = TOOLS_MARGINS.0 + TOOLS_MARGINS.2;

/// Whether «Distro Looks» and «Tools» stand side by side (owner's decision
/// 28.10.2026: «Tools» on the right, as a column of buttons). `inner_width` is
/// the panel width without margins; `looks_width` and `tools_width` are how
/// much each block asks for (the largest of the heading and the column, measured by Qt).
///
/// Side by side only if BOTH blocks are fully readable in half of the panel:
/// otherwise «Tools» go under «Distro Looks» as the same column. Columns
/// wider than their ceiling are not taken into account: the ceiling clamps them anyway.
pub fn looks_tools_side_by_side(inner_width: i32, looks_width: i32, tools_width: i32) -> bool {
    let half = looks_tools_block_width(inner_width, true);
    let looks = looks_width.clamp(0, LOOKS_COLUMN_MAX_WIDTH);
    let tools = tools_width.clamp(0, TOOL_BUTTON_MAX_WIDTH);
    half >= looks.max(tools)
}

/// Width of one middle panel block: half minus the gap when the blocks are
/// side by side, otherwise the whole panel width without margins.
pub fn looks_tools_block_width(inner_width: i32, side_by_side: bool) -> i32 {
    let inner = inner_width.max(0);
    if side_by_side {
        ((inner - LOOKS_TOOLS_GAP) / 2).max(0)
    } else {
        inner
    }
}
/// The light panel background and its right border: fractions of neutral gray
/// (`ui_base::neutral_rgba`).
pub const TOOLS_PANEL_BG_ALPHA: f64 = 0.05;
pub const TOOLS_PANEL_BORDER_ALPHA: f64 = 0.18;

// ── Card ────────────────────────────────────────────────────────────────

/// Card margins at the top, right and bottom (left is [`CARD_GAP_LEFT`]).
pub const CARD_MARGIN_TOP: i32 = 20;
pub const CARD_MARGIN_RIGHT: i32 = 24;
pub const CARD_MARGIN_BOTTOM: i32 = 24;
pub const CARD_SPACING: i32 = 14;
pub const HEADER_ROW_SPACING: i32 = 12;
/// Allowance for the vertical scroll bar in the panel width ceiling.
pub const DETAIL_SCROLLBAR_RESERVE: i32 = 20;
/// Stretch of the card against the left spacer in its wrapper. In the
/// original the card is pinned to the RIGHT edge of the panel at the owner's
/// request: the wrapper is a horizontal layout of a spacer with stretch 1 and
/// the card with stretch 100, so extra width is split 100:1 and a hundredth
/// of the viewport always remains on the left.
pub const CARD_WRAP_STRETCH: i32 = 100;
/// The smallest card width: below it the original does not shrink at all
/// (it gives horizontal scrolling), and then there is no spacer on the left.
/// ⚠️ The number is MEASURED from the live tab, not derived: on windows of
/// 960–1300 the card stayed at 924 points with a viewport from 537 to 852
/// (measured 05.09, steam_dark skin).
pub const CARD_MIN_WIDTH: i32 = 924;

/// The left spacer of the card wrapper at viewport width `viewport`.
///
/// ⚠️ THE NUMBERS ARE MEASURED, NOT DERIVED. Measured 05.09: the widget tab
/// was built at eleven window widths (height 6000, steam_dark skin), and
/// after showing, the geometry of both elements of the right panel wrapper was read:
///
/// ```text
/// viewport  537 577 677 769 852 934 1017 1099 1181 1264 1331
/// spacer      0   0   0   0   0   9   10   11   12   13   13
/// ```
///
/// While the QML markup had no spacer at all, the card was pressed to the
/// LEFT edge of the panel: on the bench window its content started 10 points
/// further left than the original (457 against 467).
pub fn card_left_spacer(viewport: i32) -> i32 {
    if viewport <= 0 {
        return 0;
    }
    // Division rounded to the nearest: the Qt layout hands out points as whole
    // numbers, and dropping the fractional part would be off by a point starting
    // from viewport 1099.
    let share = (viewport + CARD_WRAP_STRETCH / 2) / (CARD_WRAP_STRETCH + 1);
    share.clamp(0, (viewport - CARD_MIN_WIDTH).max(0))
}
/// Separator between card blocks: a fraction of neutral gray.
pub const SEPARATOR_ALPHA: f64 = 0.25;
pub const PLACEHOLDER_POINT_SIZE: i32 = 14;
pub const CARD_DESC_POINT_SIZE: i32 = 11;
pub const STAT_LABEL_POINT_SIZE: i32 = 9;
pub const STAT_SPACING: i32 = 2;
pub const PACKAGES_POINT_SIZE: i32 = 10;
pub const VERSION_POINT_SIZE: i32 = 10;
pub const BUTTON_ROW_TOP_MARGIN: i32 = 8;
pub const BUTTON_ROW_SPACING: i32 = 8;

/// Card action buttons: corner radius and padding, colors by kind. Side
/// padding 12 instead of the former 18 (QSS of the original: `padding: 8px 18px`):
/// with three buttons in a row the wide padding inflated them edge to edge,
/// and the captions got cramped (Wehrwolfmann's remark 10.10.2026).
pub const CARD_BUTTON_RADIUS: i32 = 6;
pub const CARD_BUTTON_PADDING_V: i32 = 8;
pub const CARD_BUTTON_PADDING_H: i32 = 12;
/// Main (blue): «Install», «Switch».
pub const PRIMARY_BUTTON_QSS: &str = "rgba(59,130,246,0.85)";
pub const PRIMARY_BUTTON_HOVER_QSS: &str = "rgba(59,130,246,1.0)";
/// Red: «Remove». Its own, not the shared `RexonDangerButton`: the original
/// has background 0.75 against 0.2 in «Extensions», radius 6 and a bold caption.
pub const DESTRUCTIVE_BUTTON_QSS: &str = "rgba(220,80,80,0.75)";
pub const DESTRUCTIVE_BUTTON_HOVER_QSS: &str = "rgba(220,80,80,1.0)";
/// Text of the main and red buttons (QSS: `color: white`).
pub const ACCENT_BUTTON_TEXT_COLOR: &str = "#ffffff";
/// Neutral: «Check / Repair» is a safe and secondary operation, so it has no
/// accent fill: a thin border and a light hover.
pub const NEUTRAL_BUTTON_BG_ALPHA: f64 = 0.10;
pub const NEUTRAL_BUTTON_BORDER_ALPHA: f64 = 0.30;
pub const NEUTRAL_BUTTON_HOVER_ALPHA: f64 = 0.20;

/// Colors of a "special" button with an accent fill: what the QML page passes
/// into `RexonButton.tone` (background and hover background in QSS notation,
/// caption color, corner radius, boldness).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentTone {
    pub background: &'static str,
    pub hover: &'static str,
    pub color: &'static str,
    pub radius: i32,
    pub bold: bool,
}

/// Blue: «Install», «Switch», «Yes» of a safe operation.
pub const PRIMARY_TONE: AccentTone = AccentTone {
    background: PRIMARY_BUTTON_QSS,
    hover: PRIMARY_BUTTON_HOVER_QSS,
    color: ACCENT_BUTTON_TEXT_COLOR,
    radius: CARD_BUTTON_RADIUS,
    bold: true,
};

/// Red: «Remove», «Yes» of an irreversible operation, «Abort».
pub const DESTRUCTIVE_TONE: AccentTone = AccentTone {
    background: DESTRUCTIVE_BUTTON_QSS,
    hover: DESTRUCTIVE_BUTTON_HOVER_QSS,
    color: ACCENT_BUTTON_TEXT_COLOR,
    radius: CARD_BUTTON_RADIUS,
    bold: true,
};

/// Tones that the shared desktop operation dialog (`DeOperationDialog.qml`)
/// looks up in the page palette: the palette key and the colors. ONE table for
/// all pages with this dialog («Desktops» and «Compare Desktops»): a page
/// without these keys would get «Yes» as a regular skin button instead of a
/// blue or red one.
pub const OPERATION_DIALOG_TONES: [(&str, AccentTone); 2] = [
    ("primary_tone", PRIMARY_TONE),
    ("destructive_tone", DESTRUCTIVE_TONE),
];

/// Width ceiling of the right panel by window height: the left gap + the ideal
/// width of the 16:9 carousel frame + the right margin + the scroll bar. It is
/// computed ONLY from the window height, never from the actual frame width:
/// the feedback produced a "ratchet" of 3–4 points per layout pass. In a
/// narrow window the ceiling is not reached; in a wide one the extra width
/// goes to the «Tools» panel.
pub fn detail_panel_max_width(window_height: i32) -> i32 {
    let h = CAROUSEL_MIN_HEIGHT
        .max((window_height as f64 * CAROUSEL_WINDOW_SHARE) as i32)
        .min(CAROUSEL_MAX_HEIGHT);
    let ideal_w = h * 16 / 9 + 2 * CAROUSEL_FRAME_PADDING_H;
    CARD_GAP_LEFT + ideal_w + CARD_MARGIN_RIGHT + DETAIL_SCROLLBAR_RESERVE
}

/// Initial width of the right panel in the splitter and the stretch of the
/// three panels (list : tools : card). The extra window width is split between
/// tools and card 1:4, the list does not grow; once the card hits its ceiling
/// ([`detail_panel_max_width`]), the «Tools» panel takes the whole remainder.
pub const DETAIL_INITIAL_WIDTH: i32 = 800;
pub const SPLIT_STRETCH_TOOLS: i32 = 1;
pub const SPLIT_STRETCH_DETAIL: i32 = 4;

/// Width of the splitter handle: `splitter.setHandleWidth(1)` of the original.
/// There are two, and they take their space BEFORE the split between panels.
pub const SPLIT_HANDLE_WIDTH: i32 = 1;
pub const SPLIT_HANDLE_COUNT: i32 = 2;

/// Color of the splitter handle: an overlay on the page background, like the
/// list row highlight ([`list_hover_qss`]).
///
/// Measured 05.09 on live Qt (breeze style, steam_dark skin): a one-point-wide
/// handle comes out as (73,83,96) on the page background (27,40,56), which is
/// exactly white at 0.20. No palette role gives such a color (Mid (28,31,33),
/// Midlight (46,51,55), Light (57,62,67)): the style draws it itself. The QML
/// markup did not paint the handle at all and got a light gray (198,200,201)
/// from the Basic style: on a dark skin that is a foreign bright stripe the
/// full height of the tab.
pub fn split_handle_qss(dark: bool) -> &'static str {
    if dark {
        "rgba(255,255,255,0.20)"
    } else {
        "rgba(0,0,0,0.20)"
    }
}

/// Widths of the three panels at window width `total`: the way the original's
/// QSplitter divides space.
///
/// ⚠️ TWO CORRECTIONS AGAINST THE OBVIOUS (measured 05.09, seventeen window
/// widths from 960 to 3000, taken straight from `QSplitter.sizes()` of the widget tab):
///
///   * not the whole window width is divided, but the width MINUS the handles:
///     there are two of one point each, and the panels get `total - 2`;
///   * the extra space is divided NOT by the stretch 0:1:4, as one might
///     think from `setStretchFactor`, but in proportion to the INITIAL panel
///     widths (170 to 800). In QSplitter the stretch only decides who grows
///     at all; the split itself goes from the current sizes. The former
///     stretch-based formula gave the «Tools» panel 228 points instead of 220
///     on a 1500 window, and the separator together with the whole panel
///     moved 8 points to the right: the bench measured 44,159 background
///     points against the panel background plus 6,373 points of the separator itself.
///
/// Verified on all seventeen measurements, including hitting the smallest
/// panel width (windows 960–1100) and the card hitting its ceiling (windows 1900–3000).
pub fn split_widths(total: i32, window_height: i32) -> (i32, i32, i32) {
    let panes = (total - SPLIT_HANDLE_WIDTH * SPLIT_HANDLE_COUNT).max(0);
    // The list has no stretch and keeps its initial width.
    let rest = (panes - LIST_INITIAL_WIDTH).max(0);
    let share = TOOLS_INITIAL_WIDTH + DETAIL_INITIAL_WIDTH;
    let mut tools =
        ((f64::from(rest) * f64::from(TOOLS_INITIAL_WIDTH) / f64::from(share)) + 0.5) as i32;
    tools = tools.max(TOOLS_MIN_WIDTH).min(rest);
    let mut detail = rest - tools;
    let cap = detail_panel_max_width(window_height);
    if detail > cap {
        tools += detail - cap;
        detail = cap;
    }
    (LIST_INITIAL_WIDTH, tools, detail)
}

// ── Carousel and viewer ─────────────────────────────────────────────────

/// Screenshot frame: a fraction of neutral gray, corner radius, side padding
/// of the picture (no padding at the top and bottom).
pub const CAROUSEL_FRAME_ALPHA: f64 = 0.10;
pub const CAROUSEL_FRAME_RADIUS: i32 = 8;
pub const CAROUSEL_FRAME_PADDING_H: i32 = 8;
pub const CAROUSEL_DOTS_SPACING: i32 = 6;
pub const NO_SCREENSHOTS_POINT_SIZE: i32 = 20;
/// Arrows and magnifier: round dark buttons on top of the picture.
pub const OVERLAY_BUTTON_QSS: &str = "rgba(0,0,0,0.55)";
pub const ARROW_HOVER_QSS: &str = "rgba(0,0,0,0.75)";
pub const ZOOM_BUTTON_HOVER_QSS: &str = "rgba(0,0,0,0.85)";
pub const OVERLAY_TEXT_COLOR: &str = "#ffffff";
pub const OVERLAY_MARGIN: i32 = 10;
pub const ARROW_SIZE: i32 = 40;
pub const ARROW_POINT_SIZE: i32 = 16;
pub const ZOOM_BUTTON_SIZE: i32 = 44;
pub const ZOOM_BUTTON_POINT_SIZE: i32 = 20;
/// Full-screen viewer: background, margins, buttons, hint.
pub const ZOOM_BG_QSS: &str = "rgba(0,0,0,0.95)";
pub const ZOOM_MARGIN: i32 = 16;
pub const ZOOM_ARROW_SIZE: i32 = 48;
pub const ZOOM_ARROW_POINT_SIZE: i32 = 18;
pub const ZOOM_CLOSE_POINT_SIZE: i32 = 26;
pub const ZOOM_HINT_QSS: &str = "rgba(255,255,255,0.6)";
pub const ZOOM_HINT_PADDING: i32 = 8;

/// Corner radius of a round button: half its side.
pub fn round_radius(size: i32) -> i32 {
    size / 2
}

/// Carousel indicator dots: `●` for the current slide, `○` for the others.
/// An empty string with one slide: nothing to page through.
pub fn carousel_dots(index: usize, count: usize) -> String {
    if count <= 1 {
        return String::new();
    }
    (0..count)
        .map(|i| if i == index { "●" } else { "○" })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Hint of the full-screen viewer.
pub fn zoom_hint(index: usize, count: usize) -> String {
    if count == 0 {
        return tr("× in the corner, click or Esc to close");
    }
    let nav = if count > 1 {
        // TRANSLATORS: %1 is the number of the shown screenshot, %2 how many there are.
        // Also: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        format!(" · {}", trf("←/→ to browse  ·  %1/%2", &[&(index + 1), &count]))
    } else {
        String::new()
    };
    format!("{}{nav}", tr("× in the corner, click the picture or Esc to close"))
}

// ============================================================================
// Install, repair, switch
// ============================================================================

/// Texts of a desktop operation: confirmation, progress heading and result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpTexts {
    pub confirm_title: String,
    /// Simple HTML (`<b>`, `<br>`): that is what the shared confirmation dialog expects.
    pub confirm_html: String,
    pub operation_title: String,
    pub done_title: String,
    pub done_text: String,
    /// The result when what was done will work only after a reboot
    /// (`DeOpOutcome::restart_needed`); empty means the same [`Self::done_text`].
    pub done_text_restart: String,
}

/// Installing a desktop. The confirmation is shown by the SHARED dialog
/// (`de_progress::confirm_de_install`): it also refuses if the distribution
/// does not allow the install; such a check must not be set up per tab.
pub fn install_texts(display_name: &str) -> OpTexts {
    OpTexts {
        confirm_title: tr("Install?"),
        confirm_html: trf(
            // TRANSLATORS: %1 is the name of a desktop (KDE Plasma, GNOME…).
            "Install the <b>%1</b> desktop?<br><br>\
             Rexon will ask for the administrator password.",
            &[&display_name],
        ),
        operation_title: trf("Installing %1", &[&display_name]),
        done_title: tr("Installation Complete"),
        // For almost all desktops a new login is enough: the session picks up
        // packages, services and groups. Only the kernel, a kernel module and a
        // newly enabled login screen require a reboot; the executor decides that by
        // the install plan (`system_plan::restart_packages`), not by a list of desktops.
        done_text: trf(
            // TRANSLATORS: %1 is the name of a desktop (KDE Plasma, River…).
            "<b>%1</b> is installed.\n\
             Log out and choose %1 on the login screen.",
            &[&display_name],
        ),
        done_text_restart: trf(
            "<b>%1</b> is installed.\n\
             Restart your computer and choose this desktop on the login screen.",
            &[&display_name],
        ),
    }
}

// ── X11 or Wayland session ───────────────────────────────────────────────
//
// Three desktops have both sessions (`system::SESSION_VARIANTS`). The choice is
// made with three cards in the same «Install?» dialog: X11, Wayland or both.
// The recommended one follows Wayland maturity (`SessionVariants::default_pick`).
// The X11 session is in the desktop's main packages, so «Wayland» and «Both»
// install the same thing and differ in the advice on what to pick on the login screen.

/// Name of the session switcher in the question dialog (`setOperationChoice`).
pub const SESSION_CHOICE_NAME: &str = "session";

/// Whether Wayland can be offered: if it is only in the AUR, there is nothing
/// to install it with without an AUR helper (yay, paru).
fn wayland_offerable(v: &crate::system::SessionVariants, aur_helper: bool) -> bool {
    v.wayland_aur.is_empty() || aur_helper
}

/// What actually gets installed: without an AUR helper Wayland from the AUR
/// cannot be installed, so X11 remains, whatever came from the dialog.
pub fn session_pick_effective(
    de_id: &str,
    family: PkgFamily,
    pick: crate::system::SessionPick,
    aur_helper: bool,
) -> crate::system::SessionPick {
    match crate::system::session_variants(de_id, family) {
        Some(v) if pick.needs_wayland() && !wayland_offerable(v, aur_helper) => crate::system::SessionPick::X11,
        _ => pick,
    }
}

/// The explanation under the «Wayland» card: why it is better, whether it is
/// experimental for the desktop and who manages windows (simple HTML).
fn wayland_card_text(v: &crate::system::SessionVariants, name: &str) -> String {
    let mut out = tr("The picture is smoother, without tearing, and sharper on high-resolution screens.");
    if v.wayland_experimental() {
        out.push(' ');
        out.push_str(&trf(
            // TRANSLATORS: %1 is a desktop environment name.
            "For %1 it's still experimental: some applets and settings may not work.",
            &[&name],
        ));
    }
    if !v.wayland_compositor.is_empty() {
        out.push(' ');
        out.push_str(&trf(
            // TRANSLATORS: %1 is a program name (labwc, Wayfire).
            "Windows there are handled by %1, so some of the usual window \
             settings won't apply.",
            &[&v.wayland_compositor],
        ));
    }
    out
}

/// The three «How the desktop will run» cards for the install dialog. `None`
/// means no choice (the desktop has one session, or another family). For
/// Wayland from the AUR without an AUR helper the «Wayland» and «Both» cards
/// are visible but unavailable, with an explanation.
pub fn session_choice(
    de_id: &str,
    family: PkgFamily,
    current: crate::system::SessionPick,
    aur_helper: bool,
) -> Option<LookSelect> {
    use crate::system::{SessionPick, WaylandMaturity};
    let v = crate::system::session_variants(de_id, family)?;
    let name = display_name(de_id);
    let offerable = wayland_offerable(v, aur_helper);
    let aur = v.maturity == WaylandMaturity::ExperimentalAur;
    let option = |pick: SessionPick| -> LookSelectOption {
        let mut label = match pick {
            SessionPick::X11 => tr("X11 — proven"),
            SessionPick::Wayland => tr("Wayland — newer"),
            SessionPick::Both => tr("Both (choose on the login screen)"),
        };
        if pick.needs_wayland() && aur {
            // From the AUR: said honestly on the card itself, not only in the text above.
            // TRANSLATORS: %1 is a session option such as "Wayland — newer".
            label = trf("%1 ⚠️ from the AUR", &[&label]);
        }
        if pick == v.default_pick() {
            // TRANSLATORS: %1 is a session option such as "X11 — proven".
            label = trf("%1 (recommended)", &[&label]);
        }
        let enabled = !pick.needs_wayland() || offerable;
        let description = if !enabled {
            tr("Not available: installing from the AUR needs yay or paru.")
        } else {
            match pick {
                SessionPick::X11 => tr(
                    "This is how the desktop has worked for years: everything is \
                     familiar and everything works. The best choice for older computers.",
                ),
                SessionPick::Wayland => wayland_card_text(v, &name),
                SessionPick::Both => tr("Install both versions and choose on the login screen."),
            }
        };
        LookSelectOption {
            value: pick.as_str().to_string(),
            label,
            description,
            enabled,
        }
    };
    Some(LookSelect {
        name: SESSION_CHOICE_NAME.to_string(),
        label: tr("How the desktop will run"),
        options: SessionPick::ALL.into_iter().map(option).collect(),
        current: session_pick_effective(de_id, family, current, aur_helper).as_str().to_string(),
    })
}

/// The recommended option for the install dialog (`None` means no choice).
pub fn session_default(de_id: &str, family: PkgFamily) -> Option<crate::system::SessionPick> {
    crate::system::session_variants(de_id, family).map(|v| v.default_pick())
}

/// Explanation of session `kind`: the «Add the … version» dialog for an
/// installed desktop, where there are no cards (simple HTML). `aur_risk` are
/// the AUR package risk signs (as on the AUR tile in «Appearance»), empty means nothing to say.
pub fn session_choice_html(
    de_id: &str,
    family: PkgFamily,
    kind: crate::system::SessionKind,
    aur_helper: bool,
    aur_risk: &str,
) -> String {
    use crate::system::SessionKind;
    let Some(v) = crate::system::session_variants(de_id, family) else {
        return String::new();
    };
    let name = display_name(de_id);
    if kind == SessionKind::Wayland && !wayland_offerable(v, aur_helper) {
        return session_x11_only_text(&name);
    }
    match kind {
        SessionKind::X11 => tr(
            "<b>X11</b> is the proven version: this is how the desktop has worked \
             for years, and all programs and settings work as expected.",
        ),
        SessionKind::Wayland => {
            let mut out = if v.wayland_experimental() {
                trf(
                    "<b>Wayland</b> is newer: the picture is smoother and sharper on \
                     high-resolution screens. But for %1 it's still experimental — \
                     some programs, panel applets and settings may not work.",
                    &[&name],
                )
            } else {
                tr("<b>Wayland</b> is newer: the picture is smoother and sharper on \
                    high-resolution screens.")
            };
            if !v.wayland_compositor.is_empty() {
                out.push(' ');
                out.push_str(&trf(
                    // TRANSLATORS: %1 is a program name (labwc, Wayfire).
                    "Windows there are handled by %1, so some of the usual window \
                     settings won't apply.",
                    &[&v.wayland_compositor],
                ));
            }
            out.push_str("<br>");
            out.push_str(&tr(
                "The X11 version stays too: you can choose either one on the login screen.",
            ));
            out.push_str(&session_aur_warning(de_id, family, aur_risk));
            out
        }
    }
}

/// Wayland only from the AUR, and there is no AUR helper: X11 gets installed.
fn session_x11_only_text(name: &str) -> String {
    trf(
        "%1 can also run on Wayland, but that version exists only in the AUR, \
         and to install from there Rexon needs yay or paru. For now %1 will \
         be installed in the proven X11 version.",
        &[&name],
    )
}

/// The warning "Wayland only from the AUR" with risk signs (empty if the
/// desktop has Wayland from the repositories). Starts with a line break.
fn session_aur_warning(de_id: &str, family: PkgFamily, aur_risk: &str) -> String {
    let Some(v) = crate::system::session_variants(de_id, family) else {
        return String::new();
    };
    if v.maturity != crate::system::WaylandMaturity::ExperimentalAur {
        return String::new();
    }
    let mut out = String::from("<br><br>");
    out.push_str(&trf(
        "⚠️ For %1, the Wayland version comes only from the AUR — packages \
         from users that are built on your computer and aren't checked by \
         your distribution. Expect problems.",
        &[&display_name(de_id)],
    ));
    if !aur_risk.is_empty() {
        out.push_str("<br>");
        out.push_str(&aur_risk.replace('\n', "<br>"));
    }
    out
}

/// Result line: which session to pick on the login screen.
pub fn session_done_line(de_id: &str, family: PkgFamily, kind: crate::system::SessionKind) -> String {
    use crate::system::SessionKind;
    let Some(v) = crate::system::session_variants(de_id, family) else {
        return String::new();
    };
    let session = match kind {
        SessionKind::X11 => v.x11_session_name,
        SessionKind::Wayland => v.wayland_session_name,
    };
    // TRANSLATORS: %1 is the session name as the login screen shows it.
    trf("On the login screen, choose the “%1” session.", &[&session])
}

/// Result line by the dialog option: for «Both», both names, as the login
/// screen writes them.
pub fn session_pick_done_line(de_id: &str, family: PkgFamily, pick: crate::system::SessionPick) -> String {
    use crate::system::SessionPick;
    match (pick, crate::system::session_variants(de_id, family)) {
        (SessionPick::Both, Some(v)) => trf(
            // TRANSLATORS: %1 and %2 are session names as the login screen shows them.
            "On the login screen, choose the “%1” or “%2” session.",
            &[&v.x11_session_name, &v.wayland_session_name],
        ),
        _ => session_done_line(de_id, family, pick.install_kind()),
    }
}

/// The install question with the chosen session option: the explanations are
/// on the cards themselves ([`session_choice`]), only the AUR warning (if
/// Wayland is from the AUR) and the result line are appended to the question.
pub fn install_texts_with_session(
    de_id: &str,
    family: PkgFamily,
    pick: crate::system::SessionPick,
    aur_helper: bool,
    aur_risk: &str,
) -> OpTexts {
    let mut t = install_texts(display_name(de_id));
    let Some(v) = crate::system::session_variants(de_id, family) else {
        return t;
    };
    let pick = session_pick_effective(de_id, family, pick, aur_helper);
    let explain = if !wayland_offerable(v, aur_helper) {
        session_x11_only_text(&display_name(de_id))
    } else if pick.needs_wayland() {
        session_aur_warning(de_id, family, aur_risk).trim_start_matches("<br><br>").to_string()
    } else {
        String::new()
    };
    if !explain.is_empty() {
        // The explanation right under the question, the password line last.
        t.confirm_html = match t.confirm_html.split_once("<br><br>") {
            Some((question, rest)) => format!("{question}<br><br>{explain}<br><br>{rest}"),
            None => format!("{}<br><br>{explain}", t.confirm_html),
        };
    }
    let line = session_pick_done_line(de_id, family, pick);
    if !line.is_empty() {
        t.done_text = format!("{}\n{line}", t.done_text);
        t.done_text_restart = format!("{}\n{line}", t.done_text_restart);
    }
    t
}

/// A line of an installed desktop's card: which sessions already work
/// (`system::installed_sessions`). Empty means the desktop has no session
/// choice, or none works.
pub fn session_status_line(de_id: &str, family: PkgFamily, sessions: (bool, bool)) -> String {
    let Some(v) = crate::system::session_variants(de_id, family) else {
        return String::new();
    };
    let experimental = v.wayland_experimental();
    match sessions {
        (true, true) => {
            let both = if experimental { tr("X11 + Wayland (experimental)") } else { tr("X11 + Wayland") };
            // TRANSLATORS: %1 is a list of sessions, e.g. "X11 + Wayland".
            trf("Sessions: %1", &[&both])
        }
        (true, false) => trf("Session: %1", &[&"X11"]),
        (false, true) => {
            let way = if experimental { tr("Wayland (experimental)") } else { "Wayland".to_string() };
            // TRANSLATORS: %1 is a session, e.g. "X11" or "Wayland (experimental)".
            trf("Session: %1", &[&way])
        }
        (false, false) => String::new(),
    }
}

/// A button on the card of an installed desktop that lacks session `kind`.
pub fn session_offer_button(kind: crate::system::SessionKind) -> String {
    match kind {
        crate::system::SessionKind::Wayland => tr("➕ Add the Wayland version"),
        crate::system::SessionKind::X11 => tr("➕ Add the X11 version"),
    }
}

/// The explanation under this button.
pub fn session_offer_hint(de_id: &str, kind: crate::system::SessionKind) -> String {
    if kind == crate::system::SessionKind::X11 {
        return trf(
            "%1 can also run on X11 — the proven version. \
             Rexon can add it, and the Wayland version will stay.",
            &[&display_name(de_id)],
        );
    }
    // Wayland maturity is a property of the desktop, not of the family: the
    // session choice exists only in Arch, so it is taken from there.
    let experimental = crate::system::session_variants(de_id, PkgFamily::Arch)
        .map_or(true, |v| v.wayland_experimental());
    if experimental {
        trf(
            "%1 can also run on Wayland — newer and smoother, but still experimental. \
             Rexon can add it, and the X11 version will stay.",
            &[&display_name(de_id)],
        )
    } else {
        trf(
            "%1 can also run on Wayland — newer and smoother. \
             Rexon can add it, and the X11 version will stay.",
            &[&display_name(de_id)],
        )
    }
}

/// Question, progress and result of "add session `kind`" to an installed desktop.
pub fn session_add_texts(
    de_id: &str,
    family: PkgFamily,
    kind: crate::system::SessionKind,
    aur_helper: bool,
    aur_risk: &str,
) -> OpTexts {
    use crate::system::SessionKind;
    let name = display_name(de_id);
    let explain = session_choice_html(de_id, family, kind, aur_helper, aur_risk);
    let (confirm_title, operation_title, done) = match kind {
        SessionKind::Wayland => (
            tr("Add the Wayland version?"),
            trf("Adding Wayland to %1", &[&name]),
            trf("The Wayland version of <b>%1</b> is installed.", &[&name]),
        ),
        SessionKind::X11 => (
            tr("Add the X11 version?"),
            trf("Adding X11 to %1", &[&name]),
            trf("The X11 version of <b>%1</b> is installed.", &[&name]),
        ),
    };
    OpTexts {
        confirm_title,
        confirm_html: format!(
            "{explain}<br><br>{}",
            tr("Rexon will ask for the administrator password.")
        ),
        operation_title,
        done_title: tr("Installation Complete"),
        done_text: format!("{done}\n{}", session_done_line(de_id, family, kind)),
        done_text_restart: String::new(),
    }
}

/// The executor's refusal: the Wayland session is only in the AUR, and there is no AUR helper.
pub fn session_no_aur_helper_text(display_name: &str) -> String {
    trf(
        "The Wayland version of %1 comes only from the AUR, and Rexon needs yay \
         or paru to install it — neither is installed. Choose the X11 version \
         or install yay or paru first.",
        &[&display_name],
    )
}

/// The install dialog for a desktop with Wayland only from the AUR. There is no
/// such desktop in the table now (MATE on Wayfire was dropped 16.11.2026), so it
/// uses the sample `system::test_session_variants::AUR_SAMPLE`: the mechanism waits for MATE (Polo).
#[cfg(test)]
mod aur_session_tests {
    use super::*;
    use crate::system::test_session_variants::with_aur_sample;
    use crate::system::{SessionKind, SessionPick, WaylandMaturity};

    const SAMPLE: &str = "mate";

    fn en() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    /// Without an AUR helper the Wayland and «Both» cards are visible but
    /// unavailable, and X11 stays chosen whatever came from the dialog. The AUR
    /// addition is among the desktop's packages and is removed together with it.
    #[test]
    fn without_aur_helper_only_x11() {
        en();
        let arch = PkgFamily::Arch;
        with_aur_sample(|| {
            let v = crate::system::session_variants(SAMPLE, arch).expect("образец");
            assert_eq!(v.maturity, WaylandMaturity::ExperimentalAur);
            let c = session_choice(SAMPLE, arch, SessionPick::Wayland, false).unwrap();
            let enabled: Vec<bool> = c.options.iter().map(|o| o.enabled).collect();
            assert_eq!(enabled, [true, false, false]);
            assert_eq!(c.current, "x11");
            assert!(c.options[1].description.contains("yay or paru"), "{:?}", c.options[1]);
            let with_helper = session_choice(SAMPLE, arch, SessionPick::Wayland, true).unwrap();
            assert!(with_helper.options.iter().all(|o| o.enabled));
            for pick in SessionPick::ALL {
                assert_eq!(session_pick_effective(SAMPLE, arch, pick, false), SessionPick::X11);
                assert_eq!(session_pick_effective(SAMPLE, arch, pick, true), pick);
            }
            let aur = crate::system::session_aur_packages(SAMPLE, arch, SessionKind::Wayland);
            assert_eq!(aur, ["mate-wayland-session"]);
            assert!(crate::system::session_aur_packages(SAMPLE, arch, SessionKind::X11).is_empty());
            let all = crate::system::desktop_packages(SAMPLE, arch);
            assert!(all.iter().any(|p| p == "mate-wayland-session"), "{all:?}");
            assert_eq!(crate::system::session_removal_packages(SAMPLE, arch), ["mate-wayland-session"]);
        });
        // Outside the sample MATE has no session choice.
        assert!(crate::system::session_variants(SAMPLE, arch).is_none());
    }

    /// The Wayland and «Both» cards say "from the AUR" themselves; the question
    /// carries the warning and the risk signs; without a helper, an explanation
    /// and a result about X11.
    #[test]
    fn aur_texts_on_cards_and_question() {
        en();
        let arch = PkgFamily::Arch;
        with_aur_sample(|| {
            let c = session_choice(SAMPLE, arch, SessionPick::X11, true).unwrap();
            assert!(c.options[1].label.contains("AUR"), "{:?}", c.options);
            assert!(c.options[2].label.contains("AUR"), "{:?}", c.options);
            assert!(!c.options[0].label.contains("AUR"));
            let plain = install_texts(display_name(SAMPLE)).confirm_html;
            let (question, password) = plain.split_once("<br><br>").unwrap();
            for pick in [SessionPick::Wayland, SessionPick::Both] {
                let t = install_texts_with_session(SAMPLE, arch, pick, true, "RISK-LINE");
                assert!(t.confirm_html.contains("AUR") && t.confirm_html.contains("RISK-LINE"), "{}", t.confirm_html);
                // The question starts with the former «Install …?», the password line last.
                assert!(t.confirm_html.starts_with(question), "{}", t.confirm_html);
                assert!(t.confirm_html.ends_with(password), "{}", t.confirm_html);
            }
            assert!(!install_texts_with_session(SAMPLE, arch, SessionPick::X11, true, "RISK-LINE")
                .confirm_html
                .contains("RISK-LINE"));
            let no_helper = install_texts_with_session(SAMPLE, arch, SessionPick::Wayland, false, "");
            assert!(no_helper.confirm_html.contains("yay or paru"), "{}", no_helper.confirm_html);
            assert!(no_helper.done_text.contains("“MATE”"), "{}", no_helper.done_text);
            let meta = DE_META.iter().find(|m| m.id == SAMPLE).unwrap();
            assert_eq!(meta.session_text(), "X11 + Wayland (experimental, AUR)");
        });
    }
}

/// Checking and repairing a desktop.
///
/// "Repair" is a rerun of the regular install on an already installed
/// desktop: `pacman -S --needed` itself skips what is installed and brings
/// only what is missing. No separate diff logic is needed, which means the
/// distribution restrictions are the same too, so the same dialog asks.
pub fn repair_texts(display_name: &str) -> OpTexts {
    OpTexts {
        confirm_title: tr("Check and Repair?"),
        confirm_html: trf(
            "Check <b>%1</b> and install any missing packages? \
             Packages that are already installed will be skipped.<br><br>\
             Rexon will ask for the administrator password.",
            &[&display_name],
        ),
        operation_title: trf("Repairing %1", &[&display_name]),
        done_title: tr("Check Complete"),
        done_text: trf(
            "<b>%1</b> has been checked.\n\
             Missing packages (if there were any) have been installed; the ones \
             already there were skipped.",
            &[&display_name],
        ),
        done_text_restart: trf(
            "<b>%1</b> has been checked.\n\
             Missing packages (if there were any) have been installed; the ones \
             already there were skipped.\n\
             Restart your computer for the changes to take effect.",
            &[&display_name],
        ),
    }
}

/// The "current desktop not detected" dialog for the «Check My Desktop» button.
pub fn repair_current_unknown_title() -> String {
    tr("Desktop Not Detected")
}
pub fn repair_current_unknown_text() -> String {
    // The button name is taken from the button itself, without the icon, as the person sees it.
    let button = tr("🔧 Check / Repair");
    let button = button.trim_start_matches(|c: char| !c.is_alphanumeric());
    trf(
        // TRANSLATORS: %1 is the label of the “Check / Repair” button on the
        // desktop's card.
        "Couldn't detect your current desktop — choose it in the list on the \
         left and click “%1” on its card.",
        &[&button],
    )
}

// ============================================================================
// Removing a desktop: both guards
// ============================================================================

/// What to do when «Remove» is pressed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemovePlan {
    /// `blocked_current` means this is the current desktop; `blocked_last` means
    /// it is the last installed one; `confirm` means a confirmation may be asked.
    pub kind: &'static str,
    pub title: String,
    /// Dialog text (simple HTML).
    pub text: String,
}

impl RemovePlan {
    /// Whether starting the removal after this dialog is allowed at all.
    pub fn allowed(&self) -> bool {
        self.kind == "confirm"
    }
}

/// The decision about removing a desktop.
///
/// ⛔ Both guards live HERE, not in the markup: there may come to be more than
/// one tab that can remove a desktop, and a guard forgotten in the second one
/// costs the person their graphical session.
///
/// * `blocked_current`: removing the desktop currently in use. After the next
///   login the person would be left with a black screen.
/// * `blocked_last`: it is the only installed one. The same black screen,
///   only after the next reboot.
/// * `confirm`: allowed, but with explicit confirmation and a reminder that
///   the current desktop will not be affected.
///
/// `other_installed` is how many OTHER desktops are installed (the one being
/// removed does not count).
pub fn remove_plan(
    de_id: &str,
    display_name: &str,
    current_de: &str,
    other_installed: usize,
) -> RemovePlan {
    if de_id == current_de {
        return RemovePlan {
            kind: "blocked_current",
            title: tr("You Can't Remove Your Current Desktop"),
            text: trf(
                "<b>%1</b> is the desktop you're using right now.\n\n\
                 If you remove it now, you'll be left without a desktop \
                 (a black screen) the next time you log in.\n\n\
                 What to do:\n\
                 1. First switch to another installed desktop.\n\
                 2. Log in to it.\n\
                 3. Only then remove this one.",
                &[&display_name],
            ),
        };
    }
    if other_installed == 0 {
        return RemovePlan {
            kind: "blocked_last",
            title: tr("This Is the Last Installed Desktop"),
            text: trf(
                "<b>%1</b> is the only desktop installed on this system.\n\n\
                 If you remove it, you'll be left without a desktop (a black \
                 screen) the next time you log out or restart.\n\n\
                 Install another desktop first, then remove this one.",
                &[&display_name],
            ),
        };
    }
    // ⭐ Fix: the identifier ("kde") was printed here instead of the desktop name.
    // By the full path: the `display_name` parameter shadows the module function.
    let current_name = crate::qt_desktops::display_name(current_de);
    RemovePlan {
        kind: "confirm",
        title: tr("Remove?"),
        text: trf(
            // TRANSLATORS: %1 is the desktop to remove, %2 the one in use now.
            "Remove the <b>%1</b> desktop?\n\n\
             Current desktop: <b>%2</b> — it won't be affected.\n\
             All of this desktop's packages will be removed. Rexon will ask for \
             the administrator password.",
            &[&display_name, &current_name],
        ),
    }
}

/// Progress heading during removal.
pub fn remove_operation_title(display_name: &str) -> String {
    trf("Removing %1", &[&display_name])
}

// ============================================================================
// KDE panel protection
// ============================================================================

/// Dialog with the result of a panel action: heading and text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PanelTexts {
    pub title: String,
    pub text: String,
}

/// The question before «Save Panel» when this desktop has a single copy and
/// it already exists (MATE, XFCE): the new one will replace it. (heading, text)
///
/// `mate`: for MATE this is also the «Restore previous panel» copy of the
/// «Panel Layout» section: say so.
pub fn panel_backup_replace_confirm(mate: bool) -> (String, String) {
    let text = if mate {
        tr("A saved copy of the panel already exists — the same one “Restore previous panel” \
            brings back. The new copy will replace it.")
    } else {
        tr("A saved copy of the panel already exists — the new copy will replace it.")
    };
    (tr("Replace the saved panel copy?"), text)
}

pub fn panel_restore_confirm_title() -> String {
    tr("Restore Panel?")
}
pub fn panel_restore_confirm_text() -> String {
    tr("Restore the panel from the last saved copy?\n\n\
        Your current panel settings will be replaced with the saved ones.")
}

/// Result of taking a panel copy.
pub fn panel_backup_result(ok: bool, message: &str) -> PanelTexts {
    if ok {
        PanelTexts {
            title: tr("Panel Saved"),
            text: format!("{}\n{message}", tr("A copy of your panel settings has been saved.")),
        }
    } else {
        PanelTexts {
            title: tr("Couldn't Save"),
            text: trf("The panel copy wasn't saved: %1", &[&message]),
        }
    }
}

/// Result of restoring the panel from the copy.
pub fn panel_restore_result(ok: bool, message: &str) -> PanelTexts {
    if ok {
        PanelTexts {
            title: tr("Panel Restored"),
            text: format!("{}\n{message}", tr("The panel has been restored from the copy.")),
        }
    } else {
        PanelTexts {
            title: tr("Couldn't Restore"),
            text: trf("Restore failed: %1", &[&message]),
        }
    }
}

// ============================================================================
// The «Shells» block in a tiling compositor's card
// ============================================================================
//
// Owner's decision 01.10.2026: a shell is an add-on to the desktop and lives
// in its card on the right; the left part of the card does not change. What
// to install, where to write the line and how to remove it is
// `crate::shell_bundle`; here are the captions and the set of buttons.

/// Width of the «Shells» block to the right of the card text, its padding, the
/// gap between shell rows and the heading font size. The block is an add-on,
/// the card text on the left stays the main thing, so the width is constant and narrow.
pub const SHELLS_PANEL_WIDTH: i32 = 300;
pub const SHELLS_PANEL_PADDING: i32 = 12;
pub const SHELLS_ROW_SPACING: i32 = 10;
pub const SHELLS_HEADER_POINT_SIZE: i32 = 12;
/// From which card text width the «Shells» block stands to the RIGHT of it.
/// Narrower, and the block goes under the text: otherwise the desktop's button
/// row (its smallest width is about 380 points) would overlap the block. The
/// number is the block width, the gap and 546 points for the text. Bench
/// measurement 03.10.2026 (ru and de): on the standard 1100×750 window the card
/// gets about 590 points, so the block goes down; on a 1400×1000 window about
/// 880, and the block stands on the right without overlaps.
pub const SHELLS_SIDE_BY_SIDE_MIN_WIDTH: i32 =
    SHELLS_PANEL_WIDTH + CARD_SPACING + 546;

/// Block heading.
pub fn shells_header() -> String {
    // TRANSLATORS: heading of the card block with desktop shells (Noctalia…)
    // that run on top of a tiling compositor.
    tr("Shells")
}

/// Heading tooltip: what a shell is, without jargon.
pub fn shells_header_tooltip() -> String {
    tr("A shell adds a panel, an app launcher and notifications on top of the desktop. \
        The desktop itself stays as it is: you can log in with the shell or without it.")
}

/// One line about what the shell gives.
pub fn shell_description(shell: DesktopShell) -> String {
    match shell {
        DesktopShell::Noctalia => tr("A ready-made panel, launcher and notification center"),
        DesktopShell::DankMaterialShell => {
            tr("A Material-style panel, launcher and control center")
        }
    }
}

/// Status of a shell that is running right now.
pub fn shell_running_text() -> String {
    tr("✅ Running now")
}

/// A button in a shell row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellButton {
    /// `shell_install`, `shell_switch`, `shell_remove`.
    pub kind: &'static str,
    pub text: String,
    pub tooltip: String,
    /// The same meaning as [`CardButton::style`].
    pub style: &'static str,
    pub enabled: bool,
}

/// A shell row in the block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellRow {
    /// Shell identifier (`DesktopShell::as_str`): an attribute, not a caption.
    pub shell: &'static str,
    pub name: &'static str,
    pub description: String,
    /// Empty means not installed.
    pub status_text: String,
    pub running: bool,
    /// Why the buttons are disabled; empty means nothing to explain.
    pub hint: String,
    pub buttons: Vec<ShellButton>,
}

/// Rows of the «Shells» block for the card `wm`. Empty means no block (Plasma
/// and all non-tiling desktops). `state` is the shell state on this machine,
/// `steamos_block` is the SteamOS reason (packages are not touched).
pub fn shell_rows(
    wm: &str,
    family: PkgFamily,
    state: &dyn Fn(DesktopShell) -> ShellState,
    steamos_block: Option<&str>,
) -> Vec<ShellRow> {
    let wm_name = display_name(wm);
    shell_bundle::shells_for(wm)
        .iter()
        .map(|&shell| {
            let st = state(shell);
            let mut hint = String::new();
            let mut buttons = Vec::new();
            let package_ok = steamos_block.is_none();
            if st.installed() {
                if !st.running_now() {
                    buttons.push(ShellButton {
                        kind: "shell_switch",
                        text: tr("🔄 Switch"),
                        tooltip: trf(
                            // TRANSLATORS: %1 is a compositor, %2 a desktop shell.
                            "Log in to %1 with %2",
                            &[&wm_name, &shell.label()],
                        ),
                        style: "primary",
                        enabled: true,
                    });
                }
                buttons.push(ShellButton {
                    kind: "shell_remove",
                    text: trc("take a shell off a desktop", "Remove"),
                    tooltip: trf(
                        // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
                        "Remove %1 — %2 stays installed",
                        &[&shell.label(), &wm_name],
                    ),
                    style: "destructive",
                    enabled: package_ok,
                });
                if let Some(why) = steamos_block {
                    hint = why.to_string();
                }
            } else {
                let mut enabled = package_ok;
                if let Err(reason) = shell_bundle::packages(shell, wm, family) {
                    hint = shell_bundle::unavailable_text(reason, shell, wm_name);
                    enabled = false;
                } else if !st.wm_installed {
                    hint = shell_bundle::install_desktop_first(wm_name);
                    enabled = false;
                } else if let Some(why) = steamos_block {
                    hint = why.to_string();
                }
                buttons.push(ShellButton {
                    kind: "shell_install",
                    text: tr("📦 Install"),
                    tooltip: hint.clone(),
                    style: "primary",
                    enabled,
                });
            }
            ShellRow {
                shell: shell.as_str(),
                name: shell.label(),
                description: shell_description(shell),
                status_text: if st.running_now() {
                    shell_running_text()
                } else if st.installed() {
                    status_installed()
                } else {
                    String::new()
                },
                running: st.running_now(),
                hint,
                buttons,
            }
        })
        .collect()
}

/// The warning before installing, in one phrase for a person.
pub fn shell_warning_text(warning: Warning) -> String {
    match warning {
        Warning::OwnWallpaper => tr("The shell draws its own wallpaper, on top of the wallpaper set in Rexon."),
        Warning::ColorScheme => tr("The shell switches the system between light and dark mode itself \
            and can override the choice made in Rexon."),
        Warning::NotificationsAndTray => tr("The shell takes over notifications and tray icons: another \
            panel (for example, waybar) may stop showing them."),
        Warning::DmsMovesHyprlandConf => tr("You have both hyprland.lua and hyprland.conf. Each time \
            DankMaterialShell starts, it moves hyprland.conf (and the files in hypr/dms) into the \
            .dms-backups folder."),
    }
}

/// Shell install texts. `packages` is what will actually be installed (empty
/// means everything is already installed, no password is needed).
pub fn shell_install_texts(
    shell: DesktopShell,
    wm: &str,
    packages: &[&str],
    warnings: &[Warning],
) -> OpTexts {
    let wm_name = display_name(wm);
    let name = shell.label();
    let mut html = trf(
        // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
        "Install <b>%1</b> for <b>%2</b>?",
        &[&name, &wm_name],
    );
    html.push_str("<br><br>");
    html.push_str(&trf(
        // TRANSLATORS: %1 is a compositor, %2 a desktop shell.
        "Rexon adds one line to the startup settings of %1, and “Remove” takes out exactly \
         that line. %1 itself stays as it is: “Switch” next to %2 logs you in with the shell, \
         the desktop's own “Switch” logs you in without it.",
        &[&wm_name, &name],
    ));
    if !warnings.is_empty() {
        html.push_str("<br><br><b>");
        html.push_str(&tr("Before you install:"));
        html.push_str("</b>");
        for w in warnings {
            html.push_str("<br>• ");
            html.push_str(&shell_warning_text(*w));
        }
    }
    html.push_str("<br><br>");
    if packages.is_empty() {
        html.push_str(&tr("The packages are already installed — no password is needed."));
    } else {
        html.push_str(&trf(
            // TRANSLATORS: %1 is a list of package names.
            "Packages to install: %1.",
            &[&packages.join(", ")],
        ));
        html.push_str("<br>");
        html.push_str(&tr("Rexon will ask for the administrator password."));
    }
    OpTexts {
        confirm_title: tr("Install?"),
        confirm_html: html,
        operation_title: trf("Installing %1", &[&name]),
        done_title: tr("Installation Complete"),
        done_text: trf(
            // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
            "<b>%1</b> is set up for %2.\nClick “Switch” next to it to log in with it.",
            &[&name, &wm_name],
        ),
        done_text_restart: String::new(),
    }
}

/// Texts of «Remove» for a shell. `packages` is what will be removed (empty
/// means the packages stay: another desktop uses them, or they are already gone).
pub fn shell_remove_texts(shell: DesktopShell, wm: &str, packages: &[&str]) -> OpTexts {
    let wm_name = display_name(wm);
    let name = shell.label();
    let mut html = trf(
        // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
        "Remove <b>%1</b> from <b>%2</b>?",
        &[&name, &wm_name],
    );
    html.push_str("<br><br>");
    html.push_str(&trf(
        // TRANSLATORS: %1 is a compositor.
        "The startup line Rexon added is taken out, and %1 goes back to how it was before.",
        &[&wm_name],
    ));
    html.push_str("<br><br>");
    if packages.is_empty() {
        html.push_str(&tr("No packages are removed."));
    } else {
        html.push_str(&trf(
            // TRANSLATORS: %1 is a list of package names.
            "Packages to remove: %1.",
            &[&packages.join(", ")],
        ));
        html.push_str("<br>");
        html.push_str(&tr("Rexon will ask for the administrator password."));
    }
    OpTexts {
        confirm_title: tr("Remove?"),
        confirm_html: html,
        operation_title: trf("Removing %1", &[&name]),
        done_title: String::new(),
        done_text: String::new(),
        done_text_restart: String::new(),
    }
}

/// Texts of «Switch» for a shell: the question and the progress heading.
pub fn shell_switch_texts(shell: DesktopShell, wm: &str) -> OpTexts {
    let wm_name = display_name(wm);
    let name = shell.label();
    OpTexts {
        confirm_title: tr("Switch Desktops?"),
        confirm_html: trf(
            // TRANSLATORS: %1 is a compositor, %2 a desktop shell.
            "Log in to <b>%1</b> with <b>%2</b>?<br><br>Save your work: Rexon will log you out. \
             On the login screen, choose %1.",
            &[&wm_name, &name],
        ),
        operation_title: trf(
            // TRANSLATORS: %1 is a compositor, %2 a desktop shell.
            "Switching to %1 with %2",
            &[&wm_name, &name],
        ),
        done_title: String::new(),
        done_text: String::new(),
        done_text_restart: String::new(),
    }
}

// ============================================================================
// Odds and ends
// ============================================================================

fn meta(de_id: &str) -> Option<&'static DeMeta> {
    DE_META.iter().find(|m| m.id == de_id)
}

/// Display name of the desktop; the identifier itself if the desktop is not in
/// the database, which is better in the interface than an empty string.
pub fn display_name(de_id: &str) -> &str {
    DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name)
        .unwrap_or(de_id)
}
