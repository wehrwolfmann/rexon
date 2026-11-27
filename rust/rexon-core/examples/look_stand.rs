//! Bench of the «Animation» and «Window title bar buttons» sections: apply a setting
//! with the same core call as the button (`animations::apply_change`,
//! `window_buttons::apply_preset` / `apply_choice` / `apply_border` /
//! `apply_defaults`), and nothing else. The result is read with the environment's native reader from outside
//! (`tools/de_matrix_stand/inner_look.sh` via `run_de.sh`).
//!
//! ⛔ Run ONLY with private HOME / XDG_CONFIG_HOME / XDG_RUNTIME_DIR
//! and its own bus — like `de_matrix_stand` (see `tools/de_matrix_stand/run_de.sh`).
//!
//!   anim-state | anim-enabled true|false | anim-speed <multiplier> |
//!   anim-row <row>=<value> | anim-defaults
//!   buttons-state | buttons <right|left>:<buttons,comma,separated> |
//!   buttons-sides <left,buttons>:<right,buttons> |
//!   border <variant> | buttons-defaults
//!   widgets-state | widgets-menu true|false | widgets-buttons true|false |
//!   widgets-toolbar icons|text|beside|under | widgets-other <same> |
//!   widgets-defaults («Icons and toolbars», `app_widgets`)

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("anim-state");
    let value = args.get(1).cloned().unwrap_or_default();
    let (ok, message) = look(action, &value);
    println!("ИТОГ ok={ok} {message}");
}

// ── «Animation» and «Window title bar buttons»: the same core calls as the buttons ──

fn look(action: &str, value: &str) -> (bool, String) {
    use rexon_core::animations::{self as anim, AnimChange, LookCtx, SystemIo};
    use rexon_core::app_widgets as aw;
    use rexon_core::window_buttons as wb;
    let io = SystemIo;
    let ctx = LookCtx::from_env();
    let msg = match action {
        "anim-state" => return (true, serde_json::to_string_pretty(&anim::read_state(&io, &ctx)).unwrap_or_default()),
        "buttons-state" => return (true, serde_json::to_string_pretty(&wb::read_state(&io, &ctx)).unwrap_or_default()),
        "anim-enabled" => anim::apply_change(&io, &ctx, &AnimChange::Enabled(value == "true")),
        "anim-speed" => anim::apply_change(&io, &ctx, &AnimChange::Speed(value.parse().unwrap_or(1.0))),
        "anim-row" => {
            let (id, v) = value.split_once('=').unwrap_or((value, ""));
            anim::apply_change(&io, &ctx, &AnimChange::Row { id: id.into(), value: v.into() })
        }
        "anim-defaults" => anim::apply_change(&io, &ctx, &AnimChange::Defaults),
        "buttons" => {
            let (side, list) = value.split_once(':').unwrap_or(("right", value));
            wb::apply_preset(&io, &ctx, side, &split_ids(list))
        }
        "buttons-sides" => {
            let (left, right) = value.split_once(':').unwrap_or(("", value));
            wb::apply_choice(&io, &ctx, &wb::Choice { left: split_ids(left), right: split_ids(right) })
        }
        "border" => wb::apply_border(&io, &ctx, value),
        "buttons-defaults" => wb::apply_defaults(&io, &ctx),
        "widgets-state" => {
            let theme = std::env::var("QT_QPA_PLATFORMTHEME").unwrap_or_default();
            return (true, serde_json::to_string_pretty(&aw::read_state(&io, &ctx, &theme)).unwrap_or_default());
        }
        "widgets-menu" => aw::apply(&io, &ctx, &aw::WidgetsChange::MenuIcons(value == "true")),
        "widgets-buttons" => aw::apply(&io, &ctx, &aw::WidgetsChange::ButtonIcons(value == "true")),
        "widgets-toolbar" | "widgets-other" => match aw::Toolbar::from_id(value) {
            Some(t) if action == "widgets-toolbar" => aw::apply(&io, &ctx, &aw::WidgetsChange::Toolbar(t)),
            Some(t) => aw::apply(&io, &ctx, &aw::WidgetsChange::OtherToolbars(t)),
            None => return (false, format!("незнакомый вид подписей {value}")),
        },
        "widgets-defaults" => aw::apply_defaults(&io, &ctx),
        other => return (false, format!("незнакомое действие {other}")),
    };
    // An empty answer — there was nothing to change (the same value repeated).
    let applied = [
        rexon_core::i18n::tr("Animation settings applied."),
        rexon_core::i18n::tr("Title bar settings applied."),
        rexon_core::i18n::tr("Icon and toolbar settings applied. Applications that are already open pick them up after a restart."),
    ];
    (msg.is_empty() || applied.contains(&msg), if msg.is_empty() { "без изменений".into() } else { msg })
}

fn split_ids(list: &str) -> Vec<String> {
    list.split(',').filter(|b| !b.is_empty()).map(String::from).collect()
}
