//! Вкладка «Рабочие столы»: содержимое и решения.
//!
//! Вторая вкладка, перенесённая на QML (после «Сравнения DE»). Здесь живёт ВСЁ, что вкладка
//! показывает и решает: секции списка, подписи и подсказки пунктов, шапка
//! карточки, ТТХ-полоса, строка пакетов, состав кнопок по состоянию стола,
//! тексты подтверждений и — главное — обе защиты удаления. Разметка (QML
//! либо виджеты) только раскладывает это по экрану.
//!
//! ⚠️ Продуктовых чисел и строк во вкладке быть не должно. Вторая копия
//! разъезжается молча, а замечает её покупатель: у него на витрине окажется
//! стол в чужой секции или кнопка удаления там, где её быть не может.
//!
//! Что взято у соседей, а не переписано:
//!   * значки столов — [`crate::qt_compare::de_icon`] (таблица одна на
//!     продукт);
//!   * ТТХ столов — [`crate::system::DE_META`];
//!   * тексты переключения — [`crate::qt_compare::switch_texts`]: у обеих
//!     вкладок они совпадают дословно, и разводить их незачем;
//!   * можно ли ставить стол на этом дистрибутиве —
//!     [`crate::distro_compat`] через общее окно подтверждения.
//!
//! Подписи идут через каталог перевода (`crate::i18n`): исходный язык —
//! английский, русский живёт в `po/ru.po`. Признаки (`kind`, `style`,
//! идентификаторы столов) от подписей отделены и не переводятся.

use serde::Serialize;

use crate::i18n::{fill, ntr, tr, trc, trf};

use crate::qt_compare::de_icon;
use crate::shell_bundle::{self, ShellState, Warning};
use crate::system::{is_available, DeMeta, DesktopShell, PkgFamily, DE_META};

// ============================================================================
// Окно
// ============================================================================

pub fn window_title() -> String {
    tr("Rexon — Desktops")
}
pub const WINDOW_MIN_SIZE: (u32, u32) = (960, 720);

// ============================================================================
// Левая панель: список столов
// ============================================================================

pub fn list_header() -> String {
    tr("🖥️ Desktops")
}
pub const LIST_HEADER_POINT_SIZE: i32 = 13;
/// Кегль пункта списка (QSS виджетной вкладки: `font-size: 12pt`).
pub const LIST_ITEM_POINT_SIZE: i32 = 12;
/// Кегль строки-заголовка секции.
pub const SECTION_POINT_SIZE: i32 = 9;

/// Заголовок секции полноценных рабочих столов.
///
/// Не «Рабочие столы»: так называется сама панель, и дублировать заголовок
/// нельзя. «Классические» ↔ «Тайловые» — пара прилагательных; третья
/// секция — «Композиторы с эффектами» ([`section_effects`]).
pub fn section_classic() -> String {
    // TRANSLATORS: list section heading: full desktop environments, as
    // opposed to tiling window managers.
    tr("Classic")
}
/// Заголовок секции тайловых оконных менеджеров.
pub fn section_tiling() -> String {
    // TRANSLATORS: list section heading: tiling window managers.
    tr("Tiling")
}
/// Заголовок секции композиторов с эффектами (Wayfire): окна плавающие, как
/// у классических, но это не полный рабочий стол со своими программами.
pub fn section_effects() -> String {
    // TRANSLATORS: list section heading: Wayland compositors with 3D effects
    // (Wayfire) — floating windows, but not full desktop environments.
    tr("Compositors with effects")
}

/// Состав и ПОРЯДОК секции «Тайловые»: Hyprland первым как самый
/// популярный, дальше niri, sway и River.
///
/// Wayfire сюда НЕ входит: окна у него плавающие (как в Compiz), это не
/// тайловый композитор — он в своей секции [`EFFECTS_IDS`].
///
/// COSMIC сюда НЕ входит — это полноценный гибрид (DE с опциональным
/// тайлингом) и остаётся в «Классических» с подсказкой. Сам вопрос
/// «тайловый ли это композитор» ядро умеет решать
/// ([`crate::appdefaults::is_tiling_wm`]), а вот выбор «кого вынести в
/// отдельную секцию и в каком порядке» — продуктовый, и живёт здесь.
/// Сторож их согласия — `qt_desktops_parity::tiling_section_matches_core`.
pub const TILING_IDS: &[&str] = &["hyprland", "niri", "sway", "river"];

/// Состав и порядок секции «Композиторы с эффектами» (после «Тайловых»):
/// окна плавающие, эффекты в духе Compiz, но своих программ и общих
/// настроек системы нет — в «Классических» он обещал бы больше, чем даёт.
pub const EFFECTS_IDS: &[&str] = &["wayfire"];

/// Подсказка гибрида — единственная привязанная к конкретному столу.
pub fn cosmic_hint() -> String {
    tr("Hybrid: a desktop with optional tiling")
}
pub fn tip_installed() -> String {
    tr("Installed")
}
pub fn tip_unavailable() -> String {
    tr("Not available on this distribution")
}
/// Разделитель кусков подсказки пункта списка.
pub const TIP_SEPARATOR: &str = " · ";

/// Строка списка: либо заголовок секции, либо стол.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListRow {
    /// `section` — невыбираемый заголовок, `de` — рабочий стол.
    pub kind: &'static str,
    /// Идентификатор стола (у заголовка секции пусто).
    pub de_id: String,
    /// Текст строки.
    pub text: String,
    /// Подсказка строки (пустая — подсказки нет).
    pub tooltip: String,
    /// Стоит ли стол — делегату для отметки.
    pub installed: bool,
    /// Доступен ли стол на этом дистрибутиве.
    pub available: bool,
}

/// Подпись пункта: значок стола и его имя через два пробела.
pub fn item_text(de_id: &str) -> String {
    format!("{}  {}", de_icon(de_id), display_name(de_id))
}

/// Подсказка пункта списка; пустая строка — подсказки нет.
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

/// Весь список левой панели: «Классические» в порядке базы, затем
/// «Тайловые» и «Композиторы с эффектами» в своём фиксированном порядке.
///
/// Заголовок секции появляется только если в ней есть хоть один стол:
/// пустая секция — это подпись без содержимого, покупателю она ни о чём.
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
// Панель «Инструменты»
// ============================================================================

pub fn tools_header() -> String {
    tr("🧰 Tools")
}
/// Минимальная высота контрола — канон Big Picture / Steam Deck.
pub const TOOL_BUTTON_MIN_HEIGHT: i32 = 40;

/// Кнопка панели инструментов.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolButton {
    /// Что делать: `compare`, `repair_current`, `panel_backup`,
    /// `panel_restore`.
    pub kind: &'static str,
    pub text: String,
    pub tooltip: String,
}

/// Умеет ли Rexon снять копию панели ТЕКУЩЕГО стола: KDE — своим снимком
/// конфигов Plasma; MATE — `dconf dump /org/mate/panel/` (тот же файл, что
/// у «Вернуть прежнюю панель» в «Раскладке панели», `mate_panel`); XFCE —
/// `xfce4-panel-profiles save/load` (пакет в Arch extra, Debian, Fedora,
/// openSUSE). `have(программа)` — есть ли она в системе.
pub fn panel_backup_supported(current_de: &str, have: &dyn Fn(&str) -> bool) -> bool {
    match current_de {
        "kde" => true,
        "mate" => have("mate-panel") && have("dconf"),
        "xfce" => have("xfce4-panel-profiles"),
        _ => false,
    }
}

/// Кнопки панели по ТЕКУЩЕМУ столу. `panel_backup` —
/// [`panel_backup_supported`]: где копию снимать нечем, кнопки вели бы в
/// ошибку, и их нет.
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

/// Подпись кнопки возврата из встроенного «Сравнения DE».
pub fn compare_back_text() -> String {
    tr("← Desktops")
}

// ============================================================================
// Блок «Как в дистрибутиве»
// ============================================================================
//
// Средняя панель вкладки (решение владельца 25.10.2026: «Инструменты» ушли
// вниз правой панели, на их место встал этот блок). Плитка на каждый
// встроенный образ (`distro_looks::builtin_ids`), у применённого — пометка
// и «Вернуть как было». Поток «Применить»: предложение
// ([`crate::distro_looks::offer`]) → окно вопроса ([`look_confirm`], для
// игрового режима SteamOS — галочка «по желанию») → применение в рабочем
// потоке → итог. Исполняет ядро (`distro_looks::apply_live`), окно —
// общее окно операции вкладки.

use crate::distro_looks as dl;

pub fn looks_header() -> String {
    tr("🎨 Distro Looks")
}

/// Пояснение под заголовком блока.
pub fn looks_hint() -> String {
    tr("The whole look of a well-known distribution in one go. You can always put everything back.")
}

/// Плитка образа.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LookTile {
    pub id: String,
    /// Имя из манифеста на языке интерфейса.
    pub name: String,
    /// Описание из манифеста — подсказкой под курсором.
    pub description: String,
    pub applied: bool,
    /// Пометка применённого; пусто — не применён.
    pub badge: String,
    /// Кнопки: `look_apply` у неприменённого, `look_revert` у применённого.
    pub buttons: Vec<CardButton>,
}

/// Плитки встроенных образов по порядку. `applied(id)` — применён ли.
/// Образ, который не разобрался, плитки не получает (сторожит это
/// `distro_looks_manifests`, а покупателю мёртвая плитка ни к чему).
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

/// Имя образа `id` из манифеста на языке интерфейса (нет такого — сам id).
pub fn look_name(id: &str) -> String {
    match dl::builtin(id) {
        Some(Ok(look)) => look.pkg.name.get(&crate::i18n::current_language()).to_string(),
        _ => id.to_string(),
    }
}

/// Применять нечего: план не собрался (окно вопроса такого не пускает —
/// это страховка, а не путь человека).
pub fn look_nothing_to_apply() -> String {
    tr("There is nothing to apply.")
}

/// Заголовок хода дела «Применить».
pub fn look_apply_title(name: &str) -> String {
    trf("Applying “%1”", &[&name])
}

/// Строка хода, пока собирается план (пробы пакетов — секунды).
pub fn look_checking() -> String {
    tr("Checking what is needed…")
}

/// Стол сеанса не опознан — образ применять не к чему.
pub fn look_unknown_desktop() -> (String, String) {
    (
        tr("Can't apply the look"),
        tr("Rexon could not recognise the desktop you are using now, so it can't apply a look to it."),
    )
}

/// Галочка «по желанию» в окне вопроса.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LookOption {
    pub text: String,
    /// Предупреждение образа к этому виду (простой HTML); пусто — нет.
    pub hint: String,
}

/// Вариант переключателя окна вопроса.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LookSelectOption {
    pub value: String,
    /// Подпись из манифеста на языке интерфейса.
    pub label: String,
}

/// Переключатель окна вопроса — свойство-список образа (стиль SteamOS:
/// Vapor или VGUI2). Подписи — из манифеста.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LookSelect {
    /// Имя свойства (`variant`).
    pub name: String,
    pub label: String,
    /// Только варианты, с которыми образ применяется: вариант с отказом
    /// человеку не предлагается вовсе.
    pub options: Vec<LookSelectOption>,
    pub current: String,
}

/// Что показать в окне вопроса «Применить».
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum LookConfirm {
    /// Образ не применяется: окно с одной кнопкой «Понятно».
    Blocked { title: String, text: String },
    /// Вопрос: текст (простой HTML), подписи «Да»/«Нет», переключатели
    /// вариантов и галочка «по желанию».
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

/// Предложена ли галочка «по желанию»: план с ней собрался, и вид «по
/// желанию» в нём упомянут — встаёт либо пропущен по делу (например, нужен
/// пакет из AUR). Если стол такой вид не понимает вовсе, галочки нет.
fn opt_in_offered(variant: &dl::OfferVariant) -> bool {
    let Some(dl::Prepared::Ready(p)) = &variant.opt_in else { return false };
    let opt = |k: &str| dl::OPT_IN_KINDS.contains(&k);
    p.items.iter().any(|i| opt(&i.kind)) || p.skipped.iter().any(|s| opt(&s.kind) && s.reason != dl::not_chosen_reason())
}

/// Переключатели вариантов при выбранных значениях `values`.
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
            .map(|o| LookSelectOption { value: o.value().to_string(), label: o.label(lang).to_string() })
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

/// Окно вопроса по предложению `offer`: `values` — выбранные варианты
/// (пусто — по умолчанию), `opt_in` — стоит ли галочка.
///
/// Решения (правило владельца 24.10): программы из официальных
/// репозиториев ставятся сразу (защита — окно пароля системы), AUR —
/// никогда: такие пакеты называются человеку списком. Не встаёт
/// обязательное — окно отказа, ничего не применяется.
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
    // Что Rexon собирает сам из частей этого стола (а не берёт файлами
    // автора) — сказать прямо: это близко к задуманному, но не то же самое.
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
    // Замена оконного менеджера — заметное действие: сказать прямо и
    // отдельной строкой (решение владельца 31.10.2026 — по умолчанию да).
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
    for s in &plan.skipped {
        // Вид «по желанию», который человек не выбрал, — это галочка ниже,
        // а не «не будет применено».
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

/// План, который пойдёт в применение при этом выборе галочки; `None` —
/// применять нечего (отказ).
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

/// Вопрос «Вернуть как было»: заголовок, текст (простой HTML), «Да».
pub fn look_revert_confirm(name: &str) -> (String, String, String) {
    (
        trf("Put “%1” back as it was?", &[&name]),
        tr("Rexon will return the look and settings you had before. The programs installed for it stay — you can remove them yourself if you don't need them."),
        tr("Put Back"),
    )
}

/// Заголовок хода дела «Вернуть как было».
pub fn look_revert_title(name: &str) -> String {
    trf("Putting “%1” back", &[&name])
}

/// Итог под журналом (простой HTML) из итога ядра.
pub fn look_result_html(message: &str) -> String {
    esc(message).replace('\n', "<br>")
}

// ============================================================================
// Правая панель: карточка стола
// ============================================================================

pub fn placeholder_text() -> String {
    tr("← Choose a desktop from the list on the left")
}
pub const CARD_TITLE_POINT_SIZE: i32 = 22;
pub const CARD_STATUS_POINT_SIZE: i32 = 11;
/// Потолок ширины карточки и гарантированный зазор слева.
pub const CARD_MAX_WIDTH: i32 = 1340;
pub const CARD_GAP_LEFT: i32 = 12;

/// Зелёный «стоит» в шапке карточки (виджетная вкладка: `#4ade80`).
pub const STATUS_INSTALLED_COLOR: &str = "#4ade80";
/// Янтарный «недоступен» (виджетная вкладка: `rgba(245,158,11,1)`).
pub const STATUS_UNAVAILABLE_COLOR: &str = "#f59e0b";
/// Зелёный «это твой текущий стол» (виджетная вкладка: `rgba(34,197,94,1)`).
pub const CURRENT_BADGE_COLOR: &str = "#22c55e";
// Красную кнопку удаления рисует общий элемент `RexonDangerButton`, а её
// цвет и обе прозрачности живут в `qt_extensions::DANGER_*`: своего оттенка
// вкладка не заводит (правило #12).

pub fn status_installed() -> String {
    tr("✅ Installed")
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

/// Одна колонка ТТХ-полосы под скриншотом.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatCell {
    /// Верхняя строка: значок либо повторённые звёзды.
    pub icon: String,
    /// Средняя строка; пустая — место занято прозрачной точкой.
    pub value: String,
    /// Нижняя подпись.
    pub label: String,
}

/// Заполнитель пустого значения ТТХ: место под ним держится, текст невидим.
pub const STAT_EMPTY_VALUE: &str = "·";

/// ТТХ-полоса стола: память, нагрузка, настраиваемость, дружелюбие к играм.
///
/// Значки настраиваемости и игр повторяются по оценке (не дополняются
/// пустыми звёздами до пяти, как в «Сравнении DE», — там своя разметка), но
/// не меньше одного: нулевая оценка иначе давала бы пустую колонку без
/// намёка на то, что она вообще значит.
pub fn stats(de_id: &str) -> Vec<StatCell> {
    let Some(meta) = meta(de_id) else {
        return Vec::new();
    };
    vec![
        StatCell {
            icon: "💾".to_string(),
            // TRANSLATORS: memory use of a desktop, %1 is a number of megabytes.
            value: trf("%1 MB", &[&meta.ram_mb]),
            // «Память» и «Процессор», а не RAM и CPU: покупателю с Windows
            // сокращения ничего не говорят (правило #13).
            label: trc("desktop spec", "Memory"),
        },
        StatCell {
            icon: "⚡".to_string(),
            value: meta.cpu_usage.to_string(),
            label: trc("desktop spec", "Processor"),
        },
        StatCell {
            // Оценка — одной функцией с «Сравнением» (`qt_compare::stars`):
            // заполненные значки плюс «☆» до пяти. Раньше здесь были одни
            // заполненные, и «Игры 4» от «Игр 5» отличались только длиной
            // строки (01.11.2026, правило #12).
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

/// Описание стола без ведущего значка.
///
/// Заголовок карточки значок уже несёт, второй раз он не нужен. Стрипается
/// ЛЮБОЙ первый токен без букв и цифр, а не список известных: захардкоженный
/// список разъезжался с базой (у LXQt перо `🪶` в него не попало и
/// дублировалось).
pub fn description(de_id: &str) -> String {
    let Some(meta) = meta(de_id) else {
        return String::new();
    };
    // Описание — через каталог перевода (`DeMeta::description_text`).
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

/// Строка «сколько пакетов поставится и откуда».
///
/// Источник называется ВСЕГДА: без приписки строка выглядела как недоделка.
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

/// Блок «пакеты» карточки: строка про состав и, если есть, предупреждение.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PackagesView {
    /// Сколько поставится / что на месте; может быть в две строки.
    pub text: String,
    /// Что сейчас не ставится и что из этого следует; пусто — всё в порядке.
    pub notice: String,
    /// Набор не встанет из-за конфликта с установленным: кнопки, которые его
    /// ставят ([`installs_desktop_set`]), видны, но неактивны, а причина —
    /// в [`Self::notice`] прямо над ними.
    pub install_blocked: bool,
}

/// Ставит ли действие набор пакетов стола («Установить», «Проверить /
/// Восстановить»). Такие кнопки неактивны, пока набор конфликтует с
/// установленным ([`PackagesView::install_blocked`]).
pub fn installs_desktop_set(kind: &str) -> bool {
    matches!(kind, "install" | "repair")
}

/// Поможет ли «Собрать заново из исходников Arch»: при конфликте с
/// установленным — нет. Пересборка чинит пакет, ждущий новой библиотеки, а
/// конфликт (mutter49 против mutter) остаётся, и кнопка только зря
/// гоняла бы сборку.
pub fn rebuild_can_help(p: &crate::system_plan::InstallPreview) -> bool {
    p.conflicts.is_empty()
}

/// Пока идёт пробная транзакция (`pacman -Sp`, доли секунды — секунды).
pub fn packages_counting() -> String {
    tr("📦 Counting packages…")
}

/// Сколько имён показывать в перечне недостающих, дальше — «…».
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

/// Блок «пакеты» карточки — ЧЕСТНО, по состоянию стола.
///
/// ⚠️ Было: у установленного COSMIC стояло «📦 К установке 29 пакетов»,
/// а у Pantheon «К установке 3 пакета» — считались ИМЕНА из базы ядра, а
/// группа `pantheon` из 63 пакетов шла за одно. Теперь, где пробная
/// транзакция ответила (`preview`, Arch), число — то, что РЕАЛЬНО
/// поставится, с раскрытыми группами и зависимостями; у установленного
/// стола — «всё на месте» либо какие пакеты пропали. Без ответа (другие
/// семейства, pacman молчит) остаётся счёт имён, но установленному столу
/// «к установке» не пишется и тогда.
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
        return PackagesView { text, notice: String::new(), install_blocked: false };
    };

    let notice = packages_notice(p);
    let missing = p.new_packages.len();
    let text = if installed {
        // Называются пропавшие пакеты САМОГО стола; их зависимости
        // доедут сами и в перечне только мешали бы (у Budgie без
        // budgie-extras первыми шли granite и python-cairocffi).
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
        // Установленные пакеты, для которых вышла новая сборка, — не
        // пропажа (19.10.2026: у MATE писалось «Не хватает 2 пакетов» про
        // стоящие mate-applets и mate-netbook). Называются пакеты самого
        // стола; обновления зависимостей перечислять незачем.
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
    PackagesView { text, notice, install_blocked: !p.conflicts.is_empty() }
}

/// Предупреждение блока «пакеты»: что из набора сейчас не ставится.
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
    if !p.skipped.is_empty() {
        return fill(
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
        );
    }
    String::new()
}

/// Кнопка карточки.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardButton {
    /// `install`, `switch`, `repair`, `remove`.
    pub kind: &'static str,
    pub text: String,
    /// Подсказка; пустая — подсказки нет.
    pub tooltip: String,
    /// `primary` — выделенная, `neutral` — обычная, `destructive` — красная.
    pub style: &'static str,
}

/// Состав кнопок карточки по состоянию стола.
///
/// ⛔ Ключевое место защиты от дурака (правило #13): у ТЕКУЩЕГО стола нет ни
/// «Переключиться» (мы уже в нём), ни «Удалить» — после выхода из сессии
/// человек остался бы без графики. Второй рубеж той же защиты —
/// [`remove_plan`], третий — `system::remove_de(force=false)` в ядре.
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

/// Трогает ли действие пакеты системы (ставит, доустанавливает, удаляет).
///
/// Такие кнопки на SteamOS (Steam Deck и Steam Machine) решает
/// [`steamos_button_block`]. «Переключиться» пакетов не трогает и остаётся
/// как есть; «Сравнение» и копии панели — тоже.
pub fn is_package_action(kind: &str) -> bool {
    matches!(kind, "install" | "repair" | "remove" | "repair_current")
}

/// Причина неактивной кнопки `kind` карточки стола `de_id` на SteamOS;
/// `None` — кнопка работает (или это не SteamOS).
///
/// Проверенные столы (`steamos_sysext::DESKTOPS`) ставятся, пересобираются
/// и удаляются образом systemd-sysext — их кнопки живые; прочим — причина
/// из [`crate::distro_compat::steamos_desktop_card_block`]. Пересборку
/// образа стола, в котором человек СЕЙЧАС работает («Проверить /
/// Восстановить» на его карточке и «Проверить мой стол»), не предлагаем:
/// подмена образа под живым сеансом в ВМ не проверена.
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

/// SteamOS: тексты окна установки (и «Проверить / Восстановить») стола,
/// который ставится образом systemd-sysext. `steamos_version` — `VERSION_ID`
/// SteamOS (`None` — не SteamOS); для прочих столов и не на SteamOS —
/// `base` как есть. Вопрос — из `steamos_sysext`: пакетный текст
/// («пакеты», «выберите на экране входа») там неправда. Итог пишет в журнал
/// сам исполнитель (`steamos_sysext::install_done_text` или предупреждение,
/// что сессией стол не стал), поэтому приписки под журналом нет (пустые
/// `done_text*`): иначе под предупреждением стояло бы «всё готово».
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

/// SteamOS: вопрос перед удалением стола-образа; `None` — обычный текст
/// `remove_plan` (не SteamOS или стол не из `steamos_sysext::DESKTOPS`).
pub fn steamos_remove_text(de_id: &str, steamos: bool) -> Option<String> {
    use crate::steamos_sysext::{remove_confirm_text, support, Support};
    match support(de_id) {
        Support::Supported(spec) if steamos => Some(remove_confirm_text(spec.name)),
        _ => None,
    }
}

/// Шапка карточки: текст статуса, его цвет и жирность.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusView {
    pub text: String,
    /// Пустая строка — красить не надо, это обычный вторичный текст.
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

// ── Версия из репозиториев дистрибутива ─────────────────────────────────

/// Пока пакетный менеджер отвечает.
pub fn version_checking() -> String {
    tr("Checking version…")
}

/// Строка «что реально скачается»; пустая — строку скрыть.
///
/// Это версия из репозиториев ТЕКУЩЕГО дистрибутива, а не «последняя в
/// мире»: репозиторий может отставать от разработчиков, и честнее показать
/// то, что придёт на машину.
pub fn version_line(version: &str) -> String {
    if version.is_empty() {
        String::new()
    } else {
        trf("Available in your distribution: %1", &[&version])
    }
}

/// Строка версии у УСТАНОВЛЕННОГО стола; пустая — строку скрыть.
///
/// Версия — того же ключевого пакета стола (`check_package` базы ядра), что
/// и «Доступно в вашем дистрибутиве» у неустановленного: иначе две строки
/// говорили бы о разных пакетах. `repo_newer` — в репозитории версия новее
/// установленной (`Some(true)`), не новее (`Some(false)`) или сравнить
/// нечем (`None`: тогда при расхождении просто называется вторая версия).
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
// Карусель скриншотов
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
    tr("Previous (←)")
}
pub fn zoom_next_tooltip() -> String {
    tr("Next (→)")
}
pub fn zoom_close_tooltip() -> String {
    tr("Close")
}
pub fn zoom_title() -> String {
    trc("screenshot viewer title", "Screenshot")
}

/// Минимальная высота рамки скриншота.
pub const CAROUSEL_MIN_HEIGHT: i32 = 320;
/// Потолок высоты рамки в пикселях и в долях окна.
pub const CAROUSEL_MAX_HEIGHT: i32 = 720;
pub const CAROUSEL_WINDOW_SHARE: f64 = 0.55;

/// Высота рамки скриншота: 16:9 от доступной ширины, но не ниже 320 и не
/// выше 55% окна либо 720 пикселей.
///
/// На развёрнутом Full HD скриншот был прибит к 320 и терялся в растянутой
/// рамке — читать его было нельзя.
pub fn carousel_height(window_height: i32, available_width: i32) -> i32 {
    let cap = CAROUSEL_MIN_HEIGHT
        .max((window_height as f64 * CAROUSEL_WINDOW_SHARE) as i32)
        .min(CAROUSEL_MAX_HEIGHT);
    let ideal = available_width * 9 / 16;
    CAROUSEL_MIN_HEIGHT.max(ideal.min(cap))
}

/// Насколько рамка снимка готова ужаться, когда карточке не хватает высоты.
///
/// ⚠️ ЧИСЛО СНЯТО, А НЕ ВЫВЕДЕНО. Замер 19.09.2026 на штатном окне 1100×750
/// (скин steam_dark, панель «Инструментов» после `41075f1`, карточка шириной
/// 569 точек), по всем одиннадцати рабочим столам вкладки:
///
/// ```text
/// вьюпорт карточки             676 (без подвала)   642 (с подвалом)
/// нужно карточке: девять DE    666                 666
/// нужно карточке: hyprland,    687                 687
///                 niri
/// ```
///
/// То есть с подвалом состояния карточке не хватает 24 точек, а у hyprland и
/// niri — 45, и ровно на столько нижний ряд кнопок и уезжал под подвал. У
/// этих двух карточка не помещалась и БЕЗ подвала: на 11 точек.
///
/// Потолок взят с запасом на строку «Проверяю версию…», которой на стенде не
/// было (пакетный менеджер там молчит), а на живой машине она добавляет
/// карточке ещё около 20 точек: 45 + 20 = 65, округлено вверх до 72 —
/// кратного восьми, шагу отступов вкладки. Это предохранитель, а не цель:
/// рамка отдаёт ровно столько, сколько не хватает, и ниже наименьшего окна
/// продукта (750 точек) вьюпорт не опускается.
pub const CAROUSEL_MAX_SQUEEZE: i32 = 72;

/// Высота рамки снимка, ужатой под вьюпорт карточки.
///
/// `full` — высота из [`carousel_height`], `room` — сколько высоты остаётся
/// рамке после всего остального в карточке (поля, текстовая колонка,
/// точки-индикаторы). Рамка отдаёт карточке не больше
/// [`CAROUSEL_MAX_SQUEEZE`]: снимок — главное на карточке, и растворить его
/// совсем ради кнопок нельзя, а прокрутка у карточки есть.
///
/// ⚠️ ШИРИНУ рамки эта высота менять НЕ должна, и это не придирка к стилю.
/// Ширина считается от НЕужатой высоты; посчитай её отсюда — и выйдет
/// «храповик»: уже рамка → уже текстовая колонка → описание переносится на
/// новую строку → карточке нужно ещё больше высоты → рамка ужимается снова.
/// В ужатом виде рамка просто сильнее кадрирует снимок по вертикали.
pub fn carousel_height_fitted(full: i32, room: i32) -> i32 {
    full.min(room.max(full - CAROUSEL_MAX_SQUEEZE))
}

// ============================================================================
// Краски и размеры разметки
// ============================================================================
//
// Таблица стилей виджетной вкладки, выписанная в ядро числами. QML своих
// красок и размеров не заводит: они приходят картами `palette` и `metrics`
// от DesktopsModel (rust/rexon-gui/src/desktops_page.rs). Цвета записаны
// так, как их понимает QSS (`rgba(...)`), а в `#AARRGGBB` для QML их
// переводит `ui_base::css_color_to_qml` — один переводчик на продукт.
// Сторож согласия с оригиналом — `qt_desktops_parity::layout_numbers_match_python`.

// ── Список слева ────────────────────────────────────────────────────────

/// Поля панели списка: слева, сверху, справа, снизу.
pub const LIST_MARGINS: (i32, i32, i32, i32) = (12, 16, 8, 12);
pub const LIST_SPACING: i32 = 8;
pub const LIST_MIN_WIDTH: i32 = 200;
pub const LIST_MAX_WIDTH: i32 = 280;
/// Стартовая ширина панели у сплиттера.
pub const LIST_INITIAL_WIDTH: i32 = 240;
/// Поля пункта списка (QSS: `padding: 8px 12px`) и его скругление.
pub const LIST_ITEM_PADDING_V: i32 = 8;
pub const LIST_ITEM_PADDING_H: i32 = 12;
pub const LIST_ITEM_RADIUS: i32 = 4;
/// Подложка выбранного пункта и его текст (QSS: `color: white`).
pub const LIST_SELECTED_QSS: &str = "rgba(59,130,246,0.4)";
pub const LIST_SELECTED_TEXT_COLOR: &str = "#ffffff";

/// Подложка пункта под курсором: на тёмной теме белая, на светлой чёрная.
pub fn list_hover_qss(dark: bool) -> &'static str {
    if dark {
        "rgba(255,255,255,0.05)"
    } else {
        "rgba(0,0,0,0.05)"
    }
}

// ── Панель инструментов ─────────────────────────────────────────────────

pub const TOOLS_MARGINS: (i32, i32, i32, i32) = (8, 16, 8, 12);
pub const TOOLS_SPACING: i32 = 8;
pub const TOOLS_MIN_WIDTH: i32 = 160;
pub const TOOLS_INITIAL_WIDTH: i32 = 170;
/// Потолок ширины колонки кнопок: кнопки не растягиваются на всю панель.
pub const TOOL_BUTTON_MAX_WIDTH: i32 = 240;

/// Потолок ширины колонки плиток средней панели («Как в дистрибутиве»):
/// плитки не растягиваются на всю панель. Тот же, что у колонки кнопок
/// прежних «Инструментов» на этом месте ([`TOOL_BUTTON_MAX_WIDTH`]).
pub const LOOKS_COLUMN_MAX_WIDTH: i32 = TOOL_BUTTON_MAX_WIDTH;

/// Наименьшая ширина СРЕДНЕЙ панели (с 25.10.2026 — блок «Как в
/// дистрибутиве»; прежде там жили «Инструменты»), при которой её подписи
/// видны целиком. `header_width` — сколько просит заголовок блока,
/// `column_width` — сколько просит колонка плиток под самую длинную
/// надпись; обе величины меряет по шрифту сам Qt.
///
/// ⚠️ ЗАЧЕМ ОТДЕЛЬНОЕ ЧИСЛО, А НЕ [`TOOLS_MIN_WIDTH`]. 160 — это
/// `tools.setMinimumWidth(160)` виджетного оригинала, и оно подходило ЕГО
/// подписям. Сам Qt считал минимум панели иначе — по содержимому: замер
/// 18.09 на виджетной вкладке дал `tools.minimumSizeHint().width() == 165`,
/// то есть 149 кнопки плюс поля 8+8. Ровно этот механизм здесь и повторён:
/// на штатном минимуме окна 1100×750 сплиттер даёт панели ровно 160
/// ([`split_widths`]), а заголовок блока просит больше — без этого подпись
/// рвалась бы посреди слова краем соседней карточки.
///
/// С 28.10.2026 в средней панели снова живут и «Инструменты» — справа от
/// «Как в дистрибутиве» или под ним ([`looks_tools_side_by_side`]). Пол
/// панели — по более широкому из двух блоков: страница передаёт сюда
/// наибольший заголовок и наибольшую колонку, ведь на узкой панели блоки
/// встают друг под другом и каждый получает всю её ширину.
///
/// Делёж места между панелями — по-прежнему от оригинала ([`split_widths`]).
pub fn looks_panel_min_width(header_width: i32, column_width: i32) -> i32 {
    let content = column_width
        .clamp(0, LOOKS_COLUMN_MAX_WIDTH)
        .max(header_width.max(0));
    TOOLS_MIN_WIDTH.max(content + TOOLS_MARGINS.0 + TOOLS_MARGINS.2)
}

/// Промежуток между блоками средней панели («Как в дистрибутиве» и
/// «Инструменты»): поле панели слева плюс справа, посередине — тонкая
/// граница цвета рамки панели.
pub const LOOKS_TOOLS_GAP: i32 = TOOLS_MARGINS.0 + TOOLS_MARGINS.2;

/// Встают ли «Как в дистрибутиве» и «Инструменты» рядом (решение владельца
/// 28.10.2026: «Инструменты» — справа, столбиком кнопок). `inner_width` —
/// ширина панели без полей; `looks_width` и `tools_width` — сколько просит
/// каждый блок (наибольшее из заголовка и колонки, меряет Qt).
///
/// Рядом — только если ОБА блока в половине панели читаются целиком: иначе
/// «Инструменты» уходят под «Как в дистрибутиве» тем же столбиком. Колонки
/// шире своего потолка в расчёт не берутся — потолок их и так зажмёт.
pub fn looks_tools_side_by_side(inner_width: i32, looks_width: i32, tools_width: i32) -> bool {
    let half = looks_tools_block_width(inner_width, true);
    let looks = looks_width.clamp(0, LOOKS_COLUMN_MAX_WIDTH);
    let tools = tools_width.clamp(0, TOOL_BUTTON_MAX_WIDTH);
    half >= looks.max(tools)
}

/// Ширина одного блока средней панели: половина за вычетом промежутка,
/// когда блоки рядом, иначе вся ширина панели без полей.
pub fn looks_tools_block_width(inner_width: i32, side_by_side: bool) -> i32 {
    let inner = inner_width.max(0);
    if side_by_side {
        ((inner - LOOKS_TOOLS_GAP) / 2).max(0)
    } else {
        inner
    }
}
/// Лёгкий фон панели и её правая граница — доли нейтрального серого
/// (`ui_base::neutral_rgba`).
pub const TOOLS_PANEL_BG_ALPHA: f64 = 0.05;
pub const TOOLS_PANEL_BORDER_ALPHA: f64 = 0.18;

// ── Карточка ────────────────────────────────────────────────────────────

/// Поля карточки сверху, справа и снизу (слева — [`CARD_GAP_LEFT`]).
pub const CARD_MARGIN_TOP: i32 = 20;
pub const CARD_MARGIN_RIGHT: i32 = 24;
pub const CARD_MARGIN_BOTTOM: i32 = 24;
pub const CARD_SPACING: i32 = 14;
pub const HEADER_ROW_SPACING: i32 = 12;
/// Запас под вертикальную полосу прокрутки в потолке ширины панели.
pub const DETAIL_SCROLLBAR_RESERVE: i32 = 20;
/// Растяжка карточки против левой распорки в её обёртке. У оригинала
/// карточка прибита к ПРАВОМУ краю панели просьбой владельца: обёртка —
/// горизонтальная раскладка из распорки с растяжкой 1 и карточки с
/// растяжкой 100, поэтому лишняя ширина делится 100:1 и слева всегда
/// остаётся сотая доля вьюпорта.
pub const CARD_WRAP_STRETCH: i32 = 100;
/// Наименьшая ширина карточки: ниже неё оригинал не сжимается вовсе (даёт
/// горизонтальную прокрутку), и распорки слева тогда нет. ⚠️ Число СНЯТО с
/// живой вкладки, а не выведено: на окнах 960–1300 карточка держалась на
/// 924 точках при вьюпорте от 537 до 852 (замер 05.09, скин steam_dark).
pub const CARD_MIN_WIDTH: i32 = 924;

/// Левая распорка обёртки карточки при ширине вьюпорта `viewport`.
///
/// ⚠️ ЧИСЛА СНЯТЫ, А НЕ ВЫВЕДЕНЫ. Замер 05.09: виджетная вкладка строилась
/// на одиннадцати ширинах окна (высота 6000, скин steam_dark), и после
/// показа читалась геометрия обоих элементов обёртки правой панели:
///
/// ```text
/// вьюпорт   537 577 677 769 852 934 1017 1099 1181 1264 1331
/// распорка    0   0   0   0   0   9   10   11   12   13   13
/// ```
///
/// Пока распорки в разметке QML не было вовсе, карточка прижималась к
/// ЛЕВОМУ краю панели: на окне стенда её содержимое начиналось на 10 точек
/// левее оригинала (457 против 467).
pub fn card_left_spacer(viewport: i32) -> i32 {
    if viewport <= 0 {
        return 0;
    }
    // Деление с округлением к ближайшему: раскладка Qt раздаёт точки
    // целыми, и отбрасывание дробной части ошибалось бы на точку начиная
    // с вьюпорта 1099.
    let share = (viewport + CARD_WRAP_STRETCH / 2) / (CARD_WRAP_STRETCH + 1);
    share.clamp(0, (viewport - CARD_MIN_WIDTH).max(0))
}
/// Разделитель между блоками карточки — доля нейтрального серого.
pub const SEPARATOR_ALPHA: f64 = 0.25;
pub const PLACEHOLDER_POINT_SIZE: i32 = 14;
pub const CARD_DESC_POINT_SIZE: i32 = 11;
pub const STAT_LABEL_POINT_SIZE: i32 = 9;
pub const STAT_SPACING: i32 = 2;
pub const PACKAGES_POINT_SIZE: i32 = 10;
pub const VERSION_POINT_SIZE: i32 = 10;
pub const BUTTON_ROW_TOP_MARGIN: i32 = 8;
pub const BUTTON_ROW_SPACING: i32 = 8;

/// Кнопки действий карточки: скругление и поля, краски по виду. Боковое поле
/// 12 вместо прежних 18 (QSS оригинала: `padding: 8px 18px`): при трёх
/// кнопках в ряд широкие поля раздували их от края до края, а подписи
/// становилось тесно (замечание Wehrwolfmann 10.10.2026).
pub const CARD_BUTTON_RADIUS: i32 = 6;
pub const CARD_BUTTON_PADDING_V: i32 = 8;
pub const CARD_BUTTON_PADDING_H: i32 = 12;
/// Главная (синяя): «Установить», «Переключиться».
pub const PRIMARY_BUTTON_QSS: &str = "rgba(59,130,246,0.85)";
pub const PRIMARY_BUTTON_HOVER_QSS: &str = "rgba(59,130,246,1.0)";
/// Красная: «Удалить». Своя, а не общая `RexonDangerButton`: у оригинала
/// подложка 0.75 против 0.2 у «Расширений», скругление 6 и жирная подпись.
pub const DESTRUCTIVE_BUTTON_QSS: &str = "rgba(220,80,80,0.75)";
pub const DESTRUCTIVE_BUTTON_HOVER_QSS: &str = "rgba(220,80,80,1.0)";
/// Текст главной и красной кнопок (QSS: `color: white`).
pub const ACCENT_BUTTON_TEXT_COLOR: &str = "#ffffff";
/// Нейтральная: «Проверить / Восстановить» — операция безопасная и не
/// основная, поэтому без заливки-акцента: тонкая рамка и лёгкий hover.
pub const NEUTRAL_BUTTON_BG_ALPHA: f64 = 0.10;
pub const NEUTRAL_BUTTON_BORDER_ALPHA: f64 = 0.30;
pub const NEUTRAL_BUTTON_HOVER_ALPHA: f64 = 0.20;

/// Краски «особой» кнопки с заливкой-акцентом — то, что QML-страница
/// отдаёт в `RexonButton.tone` (подложка и подложка под курсором в записи
/// QSS, цвет надписи, скругление, жирность).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentTone {
    pub background: &'static str,
    pub hover: &'static str,
    pub color: &'static str,
    pub radius: i32,
    pub bold: bool,
}

/// Синяя: «Установить», «Переключиться», «Да» безопасной операции.
pub const PRIMARY_TONE: AccentTone = AccentTone {
    background: PRIMARY_BUTTON_QSS,
    hover: PRIMARY_BUTTON_HOVER_QSS,
    color: ACCENT_BUTTON_TEXT_COLOR,
    radius: CARD_BUTTON_RADIUS,
    bold: true,
};

/// Красная: «Удалить», «Да» необратимой операции, «Прервать».
pub const DESTRUCTIVE_TONE: AccentTone = AccentTone {
    background: DESTRUCTIVE_BUTTON_QSS,
    hover: DESTRUCTIVE_BUTTON_HOVER_QSS,
    color: ACCENT_BUTTON_TEXT_COLOR,
    radius: CARD_BUTTON_RADIUS,
    bold: true,
};

/// Тоны, которые общее окно операции над столом (`DeOperationDialog.qml`)
/// ищет в палитре страницы, — ключ палитры и краски. Таблица ОДНА на все
/// страницы с этим окном («Рабочие столы» и «Сравнение DE»): страница без
/// этих ключей получала бы «Да» обычной кнопкой скина вместо синей или
/// красной.
pub const OPERATION_DIALOG_TONES: [(&str, AccentTone); 2] = [
    ("primary_tone", PRIMARY_TONE),
    ("destructive_tone", DESTRUCTIVE_TONE),
];

/// Потолок ширины правой панели по высоте окна: левый зазор + идеальная
/// ширина 16:9-рамки карусели + правое поле + полоса прокрутки. Считается
/// ТОЛЬКО от высоты окна, никогда от фактической ширины рамки: обратная
/// связь давала «храповик» по 3–4 точки за проход раскладки. В узком окне
/// потолок не достигается; в широком лишняя ширина уходит панели
/// «Инструменты».
pub fn detail_panel_max_width(window_height: i32) -> i32 {
    let h = CAROUSEL_MIN_HEIGHT
        .max((window_height as f64 * CAROUSEL_WINDOW_SHARE) as i32)
        .min(CAROUSEL_MAX_HEIGHT);
    let ideal_w = h * 16 / 9 + 2 * CAROUSEL_FRAME_PADDING_H;
    CARD_GAP_LEFT + ideal_w + CARD_MARGIN_RIGHT + DETAIL_SCROLLBAR_RESERVE
}

/// Стартовая ширина правой панели у сплиттера и растяжка трёх панелей
/// (список : инструменты : карточка). Лишняя ширина окна делится между
/// инструментами и карточкой 1:4, список не растёт; когда карточка упёрлась
/// в свой потолок ([`detail_panel_max_width`]), весь остаток забирает
/// панель «Инструменты».
pub const DETAIL_INITIAL_WIDTH: i32 = 800;
pub const SPLIT_STRETCH_TOOLS: i32 = 1;
pub const SPLIT_STRETCH_DETAIL: i32 = 4;

/// Ширина рукоятки сплиттера: `splitter.setHandleWidth(1)` у оригинала.
/// Их две, и место они забирают ДО дележа между панелями.
pub const SPLIT_HANDLE_WIDTH: i32 = 1;
pub const SPLIT_HANDLE_COUNT: i32 = 2;

/// Краска рукоятки сплиттера — накладка на фон страницы, как у подсветки
/// строки списка ([`list_hover_qss`]).
///
/// Замер 05.09 на живой Qt (стиль breeze, скин steam_dark): рукоятка шириной
/// в точку выходит (73,83,96) на фоне страницы (27,40,56) — это ровно белый
/// 0.20. Ни одна роль палитры такого цвета не даёт (Mid (28,31,33),
/// Midlight (46,51,55), Light (57,62,67)): её рисует сам стиль. Разметка QML
/// рукоятку не красила вовсе и получала светло-серую (198,200,201) от стиля
/// Basic — на тёмном скине это чужая яркая полоса во всю высоту вкладки.
pub fn split_handle_qss(dark: bool) -> &'static str {
    if dark {
        "rgba(255,255,255,0.20)"
    } else {
        "rgba(0,0,0,0.20)"
    }
}

/// Ширины трёх панелей при ширине окна `total` — так делит место QSplitter
/// оригинала.
///
/// ⚠️ ДВЕ ПОПРАВКИ ПРОТИВ ОЧЕВИДНОГО (замер 05.09, семнадцать ширин окна от
/// 960 до 3000, снято прямо с `QSplitter.sizes()` виджетной вкладки):
///
///   * делится не вся ширина окна, а ширина МИНУС рукоятки: их две по одной
///     точке, и панелям достаётся `total - 2`;
///   * лишнее место делится НЕ по растяжке 0:1:4, как можно подумать по
///     `setStretchFactor`, а пропорционально СТАРТОВЫМ ширинам панелей
///     (170 к 800). Растяжка у QSplitter решает только, кто вообще растёт;
///     сам делёж идёт от текущих размеров. Прежняя формула по растяжке
///     давала на окне 1500 панель «Инструменты» 228 точек вместо 220, и
///     разделитель вместе со всей панелью уезжал на 8 точек вправо: замер
///     стенда — 44 159 точек фона против фона панели плюс 6 373 точки
///     самого разделителя.
///
/// Проверено на всех семнадцати замерах, включая упор в наименьшую ширину
/// панели (окна 960–1100) и упор карточки в свой потолок (окна 1900–3000).
pub fn split_widths(total: i32, window_height: i32) -> (i32, i32, i32) {
    let panes = (total - SPLIT_HANDLE_WIDTH * SPLIT_HANDLE_COUNT).max(0);
    // Список растяжки не имеет и остаётся стартовой ширины.
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

// ── Карусель и просмотр ─────────────────────────────────────────────────

/// Рамка скриншота: доля нейтрального серого, скругление, боковые поля
/// картинки (сверху и снизу полей нет).
pub const CAROUSEL_FRAME_ALPHA: f64 = 0.10;
pub const CAROUSEL_FRAME_RADIUS: i32 = 8;
pub const CAROUSEL_FRAME_PADDING_H: i32 = 8;
pub const CAROUSEL_DOTS_SPACING: i32 = 6;
pub const NO_SCREENSHOTS_POINT_SIZE: i32 = 20;
/// Стрелки и лупа — круглые тёмные кнопки поверх картинки.
pub const OVERLAY_BUTTON_QSS: &str = "rgba(0,0,0,0.55)";
pub const ARROW_HOVER_QSS: &str = "rgba(0,0,0,0.75)";
pub const ZOOM_BUTTON_HOVER_QSS: &str = "rgba(0,0,0,0.85)";
pub const OVERLAY_TEXT_COLOR: &str = "#ffffff";
pub const OVERLAY_MARGIN: i32 = 10;
pub const ARROW_SIZE: i32 = 40;
pub const ARROW_POINT_SIZE: i32 = 16;
pub const ZOOM_BUTTON_SIZE: i32 = 44;
pub const ZOOM_BUTTON_POINT_SIZE: i32 = 20;
/// Полноэкранный просмотр: фон, поля, кнопки, подсказка.
pub const ZOOM_BG_QSS: &str = "rgba(0,0,0,0.95)";
pub const ZOOM_MARGIN: i32 = 16;
pub const ZOOM_ARROW_SIZE: i32 = 48;
pub const ZOOM_ARROW_POINT_SIZE: i32 = 18;
pub const ZOOM_CLOSE_POINT_SIZE: i32 = 26;
pub const ZOOM_HINT_QSS: &str = "rgba(255,255,255,0.6)";
pub const ZOOM_HINT_PADDING: i32 = 8;

/// Скругление круглой кнопки — половина её стороны.
pub fn round_radius(size: i32) -> i32 {
    size / 2
}

/// Точки-индикаторы карусели: `●` у текущего слайда, `○` у остальных.
/// Пустая строка при одном слайде — листать нечего.
pub fn carousel_dots(index: usize, count: usize) -> String {
    if count <= 1 {
        return String::new();
    }
    (0..count)
        .map(|i| if i == index { "●" } else { "○" })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Подсказка полноэкранного просмотра.
pub fn zoom_hint(index: usize, count: usize) -> String {
    if count == 0 {
        return tr("× in the corner, click or Esc to close");
    }
    let nav = if count > 1 {
        // TRANSLATORS: %1 is the number of the shown screenshot, %2 how many there are.
        format!(" · {}", trf("←/→ to browse  ·  %1/%2", &[&(index + 1), &count]))
    } else {
        String::new()
    };
    format!("{}{nav}", tr("× in the corner, click the picture or Esc to close"))
}

// ============================================================================
// Установка, восстановление, переключение
// ============================================================================

/// Тексты операции со столом: подтверждение, заголовок хода дела и итог.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpTexts {
    pub confirm_title: String,
    /// Простой HTML (`<b>`, `<br>`) — так его ждёт общее окно подтверждения.
    pub confirm_html: String,
    pub operation_title: String,
    pub done_title: String,
    pub done_text: String,
    /// Итог, когда сделанное заработает только после перезагрузки
    /// (`DeOpOutcome::restart_needed`); пусто — тот же [`Self::done_text`].
    pub done_text_restart: String,
}

/// Установка стола. Подтверждение показывает ОБЩЕЕ окно
/// (`de_progress::confirm_de_install`): оно же откажет, если дистрибутив
/// установку не пустит, — по вкладке на такую проверку заводить нельзя.
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
        // Почти всем столам хватает нового входа: пакеты, службы и группы
        // подхватывает сеанс. Перезагрузку требуют только ядро, модуль ядра
        // и впервые включённый экран входа — это решает исполнитель по плану
        // установки (`system_plan::restart_packages`), а не список столов.
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

/// Проверка и восстановление стола.
///
/// «Восстановление» — это повторный запуск штатной установки на уже
/// установленном столе: `pacman -S --needed` сам пропускает то, что стоит, и
/// доставляет только недостающее. Отдельной разностной логики не нужно, а
/// значит и ограничения дистрибутива те же — спрашиваем тем же окном.
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

/// Окно «текущий стол не опознан» для кнопки «Проверить мой стол».
pub fn repair_current_unknown_title() -> String {
    tr("Desktop Not Detected")
}
pub fn repair_current_unknown_text() -> String {
    // Имя кнопки берётся с самой кнопки — без значка, как его видит человек.
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
// Удаление стола: обе защиты
// ============================================================================

/// Что делать по нажатию «Удалить».
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemovePlan {
    /// `blocked_current` — это текущий стол; `blocked_last` — он последний
    /// установленный; `confirm` — можно спрашивать подтверждение.
    pub kind: &'static str,
    pub title: String,
    /// Текст окна (простой HTML).
    pub text: String,
}

impl RemovePlan {
    /// Разрешено ли вообще запускать удаление после этого окна.
    pub fn allowed(&self) -> bool {
        self.kind == "confirm"
    }
}

/// Решение об удалении стола.
///
/// ⛔ Обе защиты живут ЗДЕСЬ, а не в разметке: вкладок, которые умеют
/// удалять стол, может стать больше одной, и защита, забытая во второй,
/// стоит человеку графической сессии.
///
/// * `blocked_current` — удаляют тот стол, в котором сейчас работают. После
///   следующего входа он остался бы с чёрным экраном.
/// * `blocked_last` — он единственный установленный. Тот же чёрный экран,
///   только после ближайшей перезагрузки.
/// * `confirm` — можно, но с явным подтверждением и напоминанием, что
///   текущий стол не пострадает.
///
/// `other_installed` — сколько ДРУГИХ столов установлено (сам удаляемый не
/// считается).
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
    // ⭐ Починка: здесь печатался идентификатор («kde»), а не имя стола.
    // Полным путём: параметр `display_name` перекрывает функцию модуля.
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

/// Заголовок хода дела при удалении.
pub fn remove_operation_title(display_name: &str) -> String {
    trf("Removing %1", &[&display_name])
}

// ============================================================================
// Защита панели KDE
// ============================================================================

/// Окно с итогом действия над панелью: заголовок и текст.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PanelTexts {
    pub title: String,
    pub text: String,
}

/// Вопрос перед «Сохранить панель», когда копия у этого стола одна и уже
/// есть (MATE, XFCE): новая её заменит. (заголовок, текст)
///
/// `mate` — у MATE это ещё и копия «Вернуть прежнюю панель» раздела
/// «Раскладка панели»: сказать об этом.
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

/// Итог снятия копии панели.
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

/// Итог возврата панели из копии.
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
// Блок «Оболочки» в карточке тайлового композитора
// ============================================================================
//
// Решение владельца 01.10.2026: оболочка — надстройка над столом и живёт в
// его карточке справа; левая часть карточки не меняется. Что ставить, куда
// вписать строку и как её снять — `crate::shell_bundle`; здесь подписи и
// состав кнопок.

/// Ширина блока «Оболочки» справа от текста карточки, его поля, промежуток
/// между строками оболочек и кегль заголовка. Блок — надстройка, текст
/// карточки слева остаётся главным, поэтому ширина постоянная и узкая.
pub const SHELLS_PANEL_WIDTH: i32 = 300;
pub const SHELLS_PANEL_PADDING: i32 = 12;
pub const SHELLS_ROW_SPACING: i32 = 10;
pub const SHELLS_HEADER_POINT_SIZE: i32 = 12;
/// С какой ширины текста карточки блок «Оболочки» встаёт СПРАВА от него.
/// Уже — блок уходит под текст: иначе ряд кнопок стола (его наименьшая
/// ширина около 380 точек) налезал бы на блок. Число — ширина блока, зазор
/// и 546 точек на текст. Замер стенда 03.10.2026 (ru и de): на штатном окне
/// 1100×750 карточке достаётся около 590 точек — блок уходит вниз; на окне
/// 1400×1000 — около 880, и блок встаёт справа без наложений.
pub const SHELLS_SIDE_BY_SIDE_MIN_WIDTH: i32 =
    SHELLS_PANEL_WIDTH + CARD_SPACING + 546;

/// Заголовок блока.
pub fn shells_header() -> String {
    // TRANSLATORS: heading of the card block with desktop shells (Noctalia…)
    // that run on top of a tiling compositor.
    tr("Shells")
}

/// Подсказка заголовка: что такое оболочка, без жаргона.
pub fn shells_header_tooltip() -> String {
    tr("A shell adds a panel, an app launcher and notifications on top of the desktop. \
        The desktop itself stays as it is: you can log in with the shell or without it.")
}

/// Одна строка о том, что даёт оболочка.
pub fn shell_description(shell: DesktopShell) -> String {
    match shell {
        DesktopShell::Noctalia => tr("A ready-made panel, launcher and notification center"),
        DesktopShell::DankMaterialShell => {
            tr("A Material-style panel, launcher and control center")
        }
    }
}

/// Статус оболочки, которая работает прямо сейчас.
pub fn shell_running_text() -> String {
    tr("✅ Running now")
}

/// Кнопка в строке оболочки.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellButton {
    /// `shell_install`, `shell_switch`, `shell_remove`.
    pub kind: &'static str,
    pub text: String,
    pub tooltip: String,
    /// Тот же смысл, что у [`CardButton::style`].
    pub style: &'static str,
    pub enabled: bool,
}

/// Строка оболочки в блоке.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellRow {
    /// Идентификатор оболочки (`DesktopShell::as_str`) — признак, не подпись.
    pub shell: &'static str,
    pub name: &'static str,
    pub description: String,
    /// Пусто — не установлена.
    pub status_text: String,
    pub running: bool,
    /// Почему кнопки неактивны; пусто — объяснять нечего.
    pub hint: String,
    pub buttons: Vec<ShellButton>,
}

/// Строки блока «Оболочки» для карточки `wm`. Пусто — блока нет (Plasma и
/// все не тайловые столы). `state` — состояние оболочки на этой машине,
/// `steamos_block` — причина SteamOS (пакеты не трогаем).
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

/// Предупреждение перед установкой — одной фразой для человека.
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

/// Тексты установки оболочки. `packages` — что реально поставится (пусто —
/// всё уже стоит, пароль не нужен).
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

/// Тексты «Убрать» у оболочки. `packages` — что снимется (пусто — пакеты
/// остаются: ими пользуется другой стол либо их уже нет).
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

/// Тексты «Переключиться» у оболочки: вопрос и заголовок хода.
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
// Мелочи
// ============================================================================

fn meta(de_id: &str) -> Option<&'static DeMeta> {
    DE_META.iter().find(|m| m.id == de_id)
}

/// Отображаемое имя стола; сам идентификатор, если стола в базе нет — в
/// интерфейсе это лучше пустой строки.
pub fn display_name(de_id: &str) -> &str {
    DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name)
        .unwrap_or(de_id)
}
