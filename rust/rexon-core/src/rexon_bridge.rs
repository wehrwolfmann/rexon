//! Bridge between Rexon and external applications — port of
//! `rexon/rexon_bridge.py`.
//!
//! Two-way exchange of state between Rexon and BonziBuddy (and others) through
//! a shared JSON file `~/.config/rexon/bridge.json` plus file commands/events in
//! `~/.config/rexon/bridge_commands/`. The original is a self-contained module
//! (pure Python + JSON, no Qt), so it is ported almost entirely: the DE reaction
//! data, building messages, merging sections, state getters, polling and
//! cleaning up commands.
//!
//! I/O boundaries (they stay with the caller/the shim):
//! - the current time — via the parameters `now_iso` (the ISO string of
//!   `datetime.now().isoformat()`) and `epoch_ms`/`now_epoch` (`time.time()`);
//! - the process liveness check (`os.kill(pid, 0)`) — via the `pid_alive`
//!   closure in [`app_running`];
//! - the inter-process lock `fcntl.flock` from `_update` — outside the core (the
//!   shim holds a `.lock` file around [`RexonBridge::update_section`]; the
//!   atomicity of the write itself is ensured by tmp + rename);
//! - the CLI utility `main()` (argparse + random) is not ported — it is a
//!   debugging simulator, not bridge logic.
//!
//! File operations on bridge.json/commands are done directly (as in
//! `favorites.rs`) — these are local files, not sockets or a daemon.
//!
//! Write format: `json.dump(indent=2, ensure_ascii=False)` — checked byte for
//! byte (golden test `rexon_bridge_parity.rs`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::i18n::{n_, tr};

// ============================================================================
// Paths
// ============================================================================

/// The bridge directory (the module constant `BRIDGE_DIR`) — the Rexon settings
/// folder.
///
/// Warning — fix: the path used to be glued together as the literal
/// `~/.config/rexon` from bare `$HOME`, bypassing `$XDG_CONFIG_HOME`. The bridge
/// is inter-process: BonziBuddy and others read `bridge.json` from their path,
/// while the Rexon settings window, favorites and extensions live in the
/// XDG-aware folder. On a machine with a non-standard `XDG_CONFIG_HOME` the
/// bridge drifted into ANOTHER folder: Rexon wrote state where nobody looks for
/// it, file commands from `bridge_commands/` did not arrive, and uninstalling
/// Rexon left this folder on disk. The path is computed by
/// [`crate::utils::rexon_config_dir`] — the same single point as all other
/// Rexon stores in the configs.
pub fn bridge_dir() -> PathBuf {
    crate::utils::rexon_config_dir()
}

/// `~/.config/rexon/bridge.json` (`BRIDGE_FILE`).
pub fn default_bridge_file() -> PathBuf {
    bridge_dir().join("bridge.json")
}

/// `~/.config/rexon/bridge_commands` (`COMMANDS_DIR`).
pub fn default_commands_dir() -> PathBuf {
    bridge_dir().join("bridge_commands")
}

/// Size limit of a command file: 1 MiB — protection against DoS (`MAX_CMD_SIZE`).
pub const MAX_CMD_SIZE: u64 = 1024 * 1024;

// ============================================================================
// DE reactions for BonziBuddy
// ============================================================================

/// Sets of lines for one DE (the keys of the dictionary `DE_REACTIONS[de]`).
pub struct DeReactions {
    pub greetings: &'static [&'static str],
    pub jokes: &'static [&'static str],
    pub on_switch_to: &'static [&'static str],
    pub on_switch_from: &'static [&'static str],
    pub idle_comments: &'static [&'static str],
}

impl DeReactions {
    /// A JSON object with the same key order as the Python dictionary; the
    /// lines are translated into the interface language here.
    pub fn to_value(&self) -> Value {
        let mut obj = Map::new();
        for (key, list) in [
            ("greetings", self.greetings),
            ("jokes", self.jokes),
            ("on_switch_to", self.on_switch_to),
            ("on_switch_from", self.on_switch_from),
            ("idle_comments", self.idle_comments),
        ] {
            obj.insert(
                key.to_string(),
                Value::Array(list.iter().map(|s| Value::from(tr(s))).collect()),
            );
        }
        Value::Object(obj)
    }
}

/// `DE_REACTIONS` — BonziBuddy's lines per DE (order as in the original).
pub static DE_REACTIONS: &[(&str, DeReactions)] = &[
    (
        "kde",
        DeReactions {
            greetings: &[
                // TRANSLATORS: the lines of the BonziBuddy mascot (a purple gorilla,
                // he speaks of himself as "he") about the desktop the person is in.
                // Keep the playful tone and the jokes, not the exact words.
                n_("Oh, KDE! It's so pretty here that I forgot why I came."),
                n_("KDE? So you love tweaking everything. We'll get along!"),
                n_("Hi! What a beautiful desktop you have."),
                n_("KDE! It's cozy here. Just like home."),
            ],
            jokes: &[
                n_("So many settings here that even I got lost!"),
                n_("Pretty windows, pretty buttons... And me? Am I pretty too?"),
                n_("They say you can tweak absolutely everything here. Even me?"),
                n_("I wandered around the settings for a bit... came back three hours later."),
            ],
            on_switch_to: &[
                n_("Whoa, a new desktop! It's pretty here!"),
                n_("We moved! I like it here, lots of buttons and panels."),
                n_("Oh, everything's new! Let me look around..."),
            ],
            on_switch_from: &[
                n_("Leaving? I just got used to these pretty windows..."),
                n_("Fine, we're moving. I hope it's pretty there too!"),
            ],
            idle_comments: &[
                n_("You know, I like it here. Nice and cozy."),
                n_("And these desktop widgets are a neat thing!"),
                n_("The panel at the bottom, like in Windows. Almost like home!"),
                n_("You can even set animated wallpapers here. Cool!"),
            ],
        },
    ),
    (
        "gnome",
        DeReactions {
            greetings: &[
                n_("Oh, everything here is so... simple and clean!"),
                n_("Hi! A minimalist desktop, I like it."),
                n_("So roomy here! No extra buttons at all."),
                n_("Hi! Where's the taskbar here? Ah, got it, things work differently here..."),
            ],
            jokes: &[
                n_("Everything here is so simple that even the settings are simple!"),
                n_("Minimalism is fewer buttons and more beauty."),
                n_("I'm the brightest thing on this desktop!"),
                n_("Everything's gray and stylish. And I'm purple. I stand out!"),
            ],
            on_switch_to: &[
                n_("A new desktop! So clean and tidy here."),
                n_("We moved! It's roomy here. Plenty of space to stroll around."),
                n_("Oh, a new home! Unfamiliar, but pretty."),
            ],
            on_switch_from: &[
                n_("Leaving? I just got used to this simplicity!"),
                n_("Fine, let's move on. I wonder what's over there."),
            ],
            idle_comments: &[
                n_("You know, sometimes simple is good. Nothing extra."),
                n_("It's so quiet and calm here... Not like on the last desktop!"),
                n_("And I like that there isn't a million buttons here."),
            ],
        },
    ),
    (
        "mate",
        DeReactions {
            greetings: &[
                n_("Oh! A classic desktop! Nostalgia!"),
                n_("Hi! It's just like the good old days here."),
                n_("Two panels! Top and bottom! Just like it used to be!"),
                n_("So familiar here... Like I'm back in 2005."),
            ],
            jokes: &[
                n_("Retro style! I'm retro too, so we're a match!"),
                n_("Classics never go out of style. Just like me!"),
                n_("Two panels mean twice as much room for me!"),
                n_("This desktop reminds me of my youth..."),
            ],
            on_switch_to: &[
                n_("Oh, a classic desktop! Like the good old days!"),
                n_("We moved! It's cozy here, like at grandma's."),
                n_("Finally, proper panels! Top and bottom!"),
            ],
            on_switch_from: &[
                n_("Leaving the classics? Oh well, but I'll miss it."),
                n_("Farewell, cozy desktop! It was nice."),
            ],
            idle_comments: &[
                n_("I feel right at home here. Everything's in its place."),
                n_("The classic look is the easiest to understand. No surprises!"),
                n_("Everything here works. Just works. And that's good!"),
            ],
        },
    ),
    (
        "xfce",
        DeReactions {
            greetings: &[
                n_("Xfce! Fast and light! I like it!"),
                n_("Hi! Everything runs so snappy here!"),
                n_("Oh, there's a mouse on the icon! Cute."),
                n_("A light desktop. The computer isn't even breaking a sweat!"),
            ],
            jokes: &[
                n_("Everything opens so fast here that I can barely keep up!"),
                n_("This desktop runs even on weak computers. Respect!"),
                n_("Light, fast, simple. Three qualities I'm missing."),
                n_("It uses so few resources that I'm ashamed of myself."),
            ],
            on_switch_to: &[
                n_("A new desktop! Everything's so fast here! Whoosh!"),
                n_("We moved! The computer just came back to life!"),
            ],
            on_switch_from: &[n_("Leaving? I hope it's fast there too...")],
            idle_comments: &[
                n_("Fast, simple, reliable. What else do you need?"),
                n_("Everything opens instantly here. Lovely!"),
                n_("A simple, handy desktop. Nothing extra."),
            ],
        },
    ),
    (
        "cinnamon",
        DeReactions {
            greetings: &[
                n_("Hi! This looks very familiar... Kind of like Windows!"),
                n_("Oh, a handy desktop! The Start menu — I mean, the \"Menu\"!"),
                n_("Cinnamon! Everything here makes sense at first glance."),
                n_("Hi! A pretty and handy desktop. I like it!"),
            ],
            jokes: &[
                n_("It's like Windows, only better. And free!"),
                n_("Taskbar at the bottom, menu on the left... Just like home!"),
                n_("The friendliest desktop for beginners. And for gorillas!"),
                n_("Even grandma could figure it out here. Tested!"),
            ],
            on_switch_to: &[
                n_("Oh, a new desktop! Cozy here, everything's familiar."),
                n_("We moved! It's just like Windows here, only better."),
            ],
            on_switch_from: &[n_("Leaving? But it was so handy here!")],
            idle_comments: &[
                n_("You know, this is the handiest desktop for people coming from Windows."),
                n_("Everything's in its place. Panel, menu, clock. Lovely!"),
            ],
        },
    ),
    (
        "pantheon",
        DeReactions {
            greetings: &[
                n_("Hi! Pantheon — quiet, clean, nothing extra."),
                n_("Oh, elementary! Even I try to behave myself here."),
            ],
            jokes: &[
                n_("A pantheon is a temple! Can I be a god here?"),
                n_("Dock at the bottom, panel at the top. Almost like the apple folks, only without the apples."),
            ],
            on_switch_to: &[n_("A new desktop! So peaceful!")],
            on_switch_from: &[n_("Leaving the temple? It was cozy...")],
            idle_comments: &[
                n_("It's so quiet here you can hear the processor thinking."),
                n_("Minimalism. There's nothing for me to even knock over."),
            ],
        },
    ),
    (
        "deepin",
        DeReactions {
            greetings: &[
                n_("Hi! Deepin — round corners and glass, lovely!"),
                n_("Oh, Deepin! Even the windows are transparent here, like my intentions."),
            ],
            jokes: &[
                n_("The corners are so round I'm afraid I'll roll away."),
                n_("The blur behind the windows isn't my bad eyesight, it's meant to be that way."),
            ],
            on_switch_to: &[n_("A new desktop! So smooth!")],
            on_switch_from: &[n_("Leaving Deepin? The glass was pretty...")],
            idle_comments: &[
                n_("If you stare into the transparent panel long enough, it seems to stare back."),
                n_("Everything here is so soft that even errors seem polite."),
            ],
        },
    ),
    (
        "budgie",
        DeReactions {
            greetings: &[
                n_("Hi! A pretty, modern desktop!"),
                n_("Budgie! Simple, stylish, nothing extra."),
                n_("Oh, it's pretty here! I like this design."),
            ],
            jokes: &[
                n_("A budgie is a parakeet! Hello, fellow animal!"),
                n_("A stylish desktop! Even I'm embarrassed to walk around in pajamas here."),
                n_("Simple and pretty. Like me. Well, pretty for sure."),
            ],
            on_switch_to: &[n_("A new desktop! So stylish here!")],
            on_switch_from: &[n_("Leaving? It was pretty...")],
            idle_comments: &[
                n_("It's nice to be here. Everything's neat."),
                n_("Modern design. I like it!"),
            ],
        },
    ),
    (
        "cosmic",
        DeReactions {
            greetings: &[
                n_("COSMIC! This is something new! Interesting!"),
                n_("Oh, a new desktop! Looks futuristic!"),
                n_("Hi! Everything here is so... cosmic!"),
            ],
            jokes: &[
                n_("A cosmic desktop! And I'm a cosmic gorilla!"),
                n_("It's still in development, but already prettier than many finished ones!"),
                n_("The future is here! And it looks pretty good."),
            ],
            on_switch_to: &[n_("We moved into the future! Cool!"), n_("COSMIC! We're pioneers!")],
            on_switch_from: &[n_("Leaving the future? Oh well, back to the present.")],
            idle_comments: &[n_("Not everything's ready here yet, but the potential is huge!")],
        },
    ),
    (
        "hyprland",
        DeReactions {
            greetings: &[
                n_("Whoa, the windows snap into place by themselves here! Magic!"),
                n_("Hi! A pretty desktop with animations!"),
                n_("Everything arranges itself automatically here. Handy!"),
            ],
            jokes: &[
                n_("The windows arrange themselves! I wish life worked like that..."),
                n_("So many animations! It's prettier than the movies here!"),
                n_("Everything's on the keyboard here. The mouse gets a rest. I don't!"),
            ],
            on_switch_to: &[n_("A new desktop! The windows snap into place by themselves here!")],
            on_switch_from: &[n_("Leaving? But the animations here were so pretty!")],
            idle_comments: &[n_("Everything moves so smoothly here. Mesmerizing!")],
        },
    ),
    // Warning — rule #12: the list of DEs here must match `_DE_DATABASE`
    // (rexon/system.py). LXQt, niri and sway were added there but forgotten here —
    // BonziBuddy was silent in three supported environments (get_de_reactions
    // returned {}, because there is no "-" in the name and normalization did not
    // help).
    (
        "lxqt",
        DeReactions {
            greetings: &[
                n_("Oh, LXQt! Modest and fast — respect."),
                n_("Hi! Everything's light here, even I feel light as a feather."),
                n_("LXQt? So you appreciate it when nothing lags."),
            ],
            jokes: &[
                n_("It's so light here that I almost floated away!"),
                n_("No unnecessary animations. So I'll do the moving for everyone!"),
                n_("An old computer, and it flies. I wish I could do that!"),
            ],
            on_switch_to: &[n_("A new desktop! And such a fast one!")],
            on_switch_from: &[n_("Leaving? It was so snappy here...")],
            idle_comments: &[n_("Quiet, fast, nothing in the way. Lovely!")],
        },
    ),
    (
        "niri",
        DeReactions {
            greetings: &[
                n_("Whoa, the windows lined up in a strip! Never seen anything like it."),
                n_("Hi! The desktop slides off to the side here — I'm following it with my eyes."),
                n_("niri? So you love horizontal order."),
            ],
            jokes: &[
                n_("Windows in a row, like train cars. Then I'll be the locomotive!"),
                n_("The desktop rides left and right here. I'm already getting dizzy!"),
                n_("I scrolled the desktop and lost a window. It'll come back, right?"),
            ],
            on_switch_to: &[n_("A new desktop! Everything lines up in a strip here!")],
            on_switch_from: &[n_("Leaving? I just learned how to scroll!")],
            idle_comments: &[n_("Watching the windows slide off the edge. Mesmerizing.")],
        },
    ),
    (
        "sway",
        DeReactions {
            greetings: &[
                n_("Oh, sway! The windows share the screen here without fighting."),
                n_("Hi! You can hide the mouse, everything's done from the keyboard here."),
                n_("sway? So you know what you're doing. Respect!"),
            ],
            jokes: &[
                n_("Everything here is hotkeys. I remembered three and forgot the rest."),
                n_("The windows split the screen fairly. Save a piece for me too!"),
                n_("I pressed the wrong thing and a window ran off. Please bring it back."),
            ],
            on_switch_to: &[n_("A new desktop! The windows share the space by themselves here!")],
            on_switch_from: &[n_("Leaving? I only learned half the keys...")],
            idle_comments: &[n_("Not a single wasted pixel. You keep it tidy here.")],
        },
    ),
    (
        "river",
        DeReactions {
            greetings: &[
                n_("Oh, River! The windows flow like a river — and each one in its place."),
                n_("Hi! Windows are sorted by tags here, not by desktops."),
                n_("River? Quiet, fast, all from the keyboard. Respect!"),
            ],
            jokes: &[
                n_("A window with tag three, a window with tag five... And what tag am I?"),
                n_("The river flows, the windows float. I think I'm drifting off too."),
                n_("Super and a number — and the window's already here. I wish bananas worked like that!"),
            ],
            on_switch_to: &[n_("A new desktop! The windows go with the flow by themselves here!")],
            on_switch_from: &[n_("Leaving? I had just figured out the tags...")],
            idle_comments: &[n_("Watching the windows share the screen. Calm, like a river.")],
        },
    ),
    (
        "wayfire",
        DeReactions {
            greetings: &[
                n_("Whoa, Wayfire! Any second now the desktop will spin into a cube!"),
                n_("Hi! The windows wobble like jelly here. Lovely!"),
                n_("Wayfire? So you love effects. Me too!"),
            ],
            jokes: &[
                n_("I shook a window — it wobbles. I shook myself — I wobble too!"),
                n_("A cube desktop! Can I sit inside?"),
                n_("So many effects that I forgot why I came."),
            ],
            on_switch_to: &[n_("A new desktop! The windows wiggle here like they're alive!")],
            on_switch_from: &[n_("Leaving? I never finished spinning the cube...")],
            idle_comments: &[n_("The windows float, the lights glow. Cozy here.")],
        },
    ),
];

/// Bridge event name: the live wallpaper started. The scene name goes in the
/// `name` field, if the sender knows it.
pub const WALLPAPER_STARTED_EVENT: &str = "video_wallpaper_started";
/// Bridge event name: the live wallpaper stopped.
pub const WALLPAPER_STOPPED_EVENT: &str = "video_wallpaper_stopped";

/// `EVENT_REACTIONS` — reaction templates for events.
pub static EVENT_REACTIONS: &[(&str, &[&str])] = &[
    (
        "de_installed",
        &[
            // TRANSLATORS: the BonziBuddy mascot (a purple gorilla) reacts to an
            // event in Rexon. Keep the playful tone, not the exact words.
            n_("Whoa, a new desktop got installed! I want to see it!"),
            n_("A new desktop? I wonder what it's like there!"),
            n_("A new desktop is installed! We can switch over and take a look."),
        ],
    ),
    (
        "de_removed",
        &[
            n_("Removed a desktop? Farewell, old friend..."),
            n_("One desktop fewer. Oh well!"),
        ],
    ),
    (
        "theme_changed",
        &[
            n_("Oh, a new theme! Pretty!"),
            n_("Changed the look? I like it!"),
            n_("New colors! I hope I fit in."),
        ],
    ),
    (
        "extension_installed",
        &[
            n_("A new extension! What does it do?"),
            n_("Installed something new? Cool!"),
        ],
    ),
    (
        "backup_created",
        &[
            n_("Backup created! Great, we're covered."),
            n_("A good person is one who makes backups. Like you!"),
        ],
    ),
    (
        "video_wallpaper_started",
        &[
            n_("Ooh, live wallpaper! It moves so nicely!"),
            n_("Live wallpaper? Can I move around too?"),
        ],
    ),
    (
        "video_wallpaper_stopped",
        &[
            n_("The wallpaper stopped. Back to staring at a boring desktop."),
            n_("That's it, the picture doesn't move anymore. Oh well."),
        ],
    ),
];

/// Reactions for a DE with the normalization `"cinnamon-mint"` → `"cinnamon"` —
/// port of `get_de_reactions` (without the branch reading current_de from the
/// file: the caller does that via [`get_current_de`]).
pub fn lookup_de_reactions(de: &str) -> Option<&'static DeReactions> {
    if let Some((_, r)) = DE_REACTIONS.iter().find(|(k, _)| *k == de) {
        return Some(r);
    }
    if de.contains('-') {
        let base = de.split('-').next().unwrap_or(de);
        return DE_REACTIONS.iter().find(|(k, _)| *k == base).map(|(_, r)| r);
    }
    None
}

/// The same, but as a JSON object (Python returns `{}` for an unknown DE).
pub fn de_reactions_value(de: &str) -> Value {
    match lookup_de_reactions(de) {
        Some(r) => r.to_value(),
        None => Value::Object(Map::new()),
    }
}

/// All reactions of all DEs — port of `get_all_de_reactions` (for preloading).
pub fn all_de_reactions_value() -> Value {
    let mut obj = Map::new();
    for (de, r) in DE_REACTIONS {
        obj.insert((*de).to_string(), r.to_value());
    }
    Value::Object(obj)
}

/// Reaction templates for an event, translated into the interface language —
/// port of `get_event_reactions` (empty if the type is unknown).
pub fn event_reactions(event_type: &str) -> Vec<String> {
    EVENT_REACTIONS
        .iter()
        .find(|(k, _)| *k == event_type)
        .map(|(_, v)| v.iter().map(|s| tr(s)).collect())
        .unwrap_or_default()
}

// ============================================================================
// Python semantics helpers
// ============================================================================

/// Truthiness of a value per Python's rules (`bool(x)`).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `json.dumps(obj, ensure_ascii=False, indent=2)` — the format of bridge.json
/// and of the command files. A local analogue of `models::to_json_indent1`, but
/// with an indent of 2 (the serde_json `PrettyFormatter` default).
fn to_json_indent2(value: &Value) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::pretty(&mut buf);
    value
        .serialize(&mut ser)
        .expect("a serde_json::Value always serializes");
    String::from_utf8(buf).expect("serde_json writes valid UTF-8")
}

// ============================================================================
// Reading / writing / merging bridge.json (pure logic over data)
// ============================================================================

/// Read bridge.json — port of `_read`. Any read/parse error or a non-object at
/// the top level → an empty dictionary (Python logs a warning and returns `{}`).
pub fn read_bridge(path: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(_) => Map::new(),
    }
}

/// Write bridge.json through the shared door ([`crate::utils::atomic_write`]) —
/// port of `_write`. Format `json.dump(indent=2, ensure_ascii=False)`, without a
/// trailing newline. The error is returned (Python logs it and keeps quiet).
pub fn write_bridge(path: &Path, data: &Map<String, Value>) -> std::io::Result<()> {
    let text = to_json_indent2(&Value::Object(data.clone()));
    crate::utils::atomic_write(&path.to_string_lossy(), &text)
}

/// Merge updates into a section + set `_updated_at` — the core of `_update`
/// (without the file and flock). Key order: existing keys keep their position,
/// new ones are appended at the end — as with a Python dict.
///
/// A deliberate divergence: in the original a section that is NOT a dictionary
/// (garbage from a third-party application) raised `AttributeError` both in the
/// main branch and in the fallback without a lock — a crash of any notify. Here
/// such a section is silently replaced with a fresh dictionary.
pub fn merge_update(
    data: &mut Map<String, Value>,
    section: &str,
    updates: Map<String, Value>,
    now_iso: &str,
) {
    let entry = data
        .entry(section.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    let obj = entry.as_object_mut().expect("the section has just become an object");
    for (k, v) in updates {
        obj.insert(k, v);
    }
    obj.insert("_updated_at".to_string(), Value::from(now_iso));
}

// ============================================================================
// State getters (pure, over the dictionary that was read)
// ============================================================================

/// A section of the dictionary (the analogue of `data.get(name, {})`); a
/// non-object → None.
fn section<'a>(data: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    data.get(name).and_then(|v| v.as_object())
}

/// A string field of a section with a default. A non-string value → the default
/// (Python returned the value as is, contrary to the `-> str` annotation).
fn section_str(data: &Map<String, Value>, sect: &str, key: &str, default: &str) -> String {
    section(data, sect)
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

/// Whether Rexon is installed — port of `is_rexon_installed` (a truthy check).
pub fn is_rexon_installed(data: &Map<String, Value>) -> bool {
    truthy(section(data, "rexon").and_then(|s| s.get("rexon_installed")))
}

/// Rexon version — port of `get_rexon_version`.
pub fn get_rexon_version(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "rexon_version", "")
}

/// Current DE (lowercase) — port of `get_current_de`.
pub fn get_current_de(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_de", "unknown")
}

/// Human-readable DE name — port of `get_de_display`.
pub fn get_de_display(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_de_display", "")
}

/// List of installed DEs — port of `get_installed_des`. Non-string items are
/// skipped (Python returned the list as is, contrary to the `list[str]`
/// annotation).
pub fn get_installed_des(data: &Map<String, Value>) -> Vec<String> {
    section(data, "rexon")
        .and_then(|s| s.get("installed_des"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Current GTK theme — port of `get_current_theme`.
pub fn get_current_theme(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_theme", "")
}

/// Current icon set — port of `get_current_icons`.
pub fn get_current_icons(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_icons", "")
}

/// Number of extensions — port of `get_extensions_count` (a non-integer → 0).
pub fn get_extensions_count(data: &Map<String, Value>) -> i64 {
    section(data, "rexon")
        .and_then(|s| s.get("extensions_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// Whether the live wallpaper is running — port of `is_video_wallpaper_active`.
pub fn is_video_wallpaper_active(data: &Map<String, Value>) -> bool {
    truthy(section(data, "rexon").and_then(|s| s.get("video_wallpaper_active")))
}

/// The last DE switch — port of `get_last_switch`:
/// `{"from": …, "to": …, "time": …}` or None if there are no
/// `switched_from`/`switched_to` keys. The values are returned as is.
pub fn get_last_switch(data: &Map<String, Value>) -> Option<Value> {
    let rexon = section(data, "rexon")?;
    let from = rexon.get("switched_from")?;
    let to = rexon.get("switched_to")?;
    let mut obj = Map::new();
    obj.insert("from".to_string(), from.clone());
    obj.insert("to".to_string(), to.clone());
    obj.insert(
        "time".to_string(),
        rexon.get("last_de_switch").cloned().unwrap_or_else(|| Value::from("")),
    );
    Some(Value::Object(obj))
}

/// Application state — port of `get_app_state` (`data["app_<id>"]`).
pub fn get_app_state(data: &Map<String, Value>, app_id: &str) -> Map<String, Value> {
    section(data, &format!("app_{app_id}")).cloned().unwrap_or_default()
}

/// Whether an application is running — port of `is_app_running` without
/// `os.kill`: pid liveness is decided by the `pid_alive` closure (the I/O
/// boundary).
///
/// A deliberate divergence: in the original a non-integer pid (a string/float)
/// raised `TypeError` from `os.kill` past the `except`, and a pid ≤ 0 sent a
/// signal to a process group (kill's special semantics). Here such a pid is
/// treated as missing — the return follows truthy(`running`), as in the branch
/// without a pid.
pub fn app_running(
    state: &Map<String, Value>,
    pid_alive: impl FnOnce(i64) -> bool,
) -> bool {
    if !truthy(state.get("running")) {
        return false;
    }
    match state.get("pid").and_then(|v| v.as_i64()) {
        Some(pid) if pid > 0 => pid_alive(pid),
        _ => true, // running is already truthy
    }
}

/// BonziBuddy's mood — port of `get_bonzi_mood` (default `"neutral"`).
pub fn get_bonzi_mood(data: &Map<String, Value>) -> String {
    get_app_state(data, "bonzi")
        .get("mood")
        .and_then(|v| v.as_str())
        .unwrap_or("neutral")
        .to_string()
}

/// BonziBuddy's position — port of `get_bonzi_position`
/// (default `{"x": 0, "y": 0}`).
pub fn get_bonzi_position(data: &Map<String, Value>) -> Value {
    get_app_state(data, "bonzi")
        .get("position")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"x": 0, "y": 0}))
}

// ============================================================================
// Building commands and events (pure logic)
// ============================================================================

/// A ready bridge message: id, file name and JSON content.
pub struct BridgeMessage {
    pub id: String,
    pub filename: String,
    pub value: Value,
}

/// Command/event id: `{prefix}_{epoch_ms}` + an anti-collision suffix.
///
/// A deliberate divergence: in the original the id was built from the
/// milliseconds only — two commands within one ms got the same file name, and
/// the second silently overwrote the first (a command was lost). Here with
/// `seq > 0`, `_{seq}` is appended (the writer picks the first free number).
fn message_id(prefix: &str, epoch_ms: u64, seq: u32) -> String {
    if seq == 0 {
        format!("{prefix}_{epoch_ms}")
    } else {
        format!("{prefix}_{epoch_ms}_{seq}")
    }
}

/// Build a command — port of the dictionary from `send_command`.
/// Key order: target, command, params, timestamp, id.
pub fn build_command(
    target: &str,
    command: &str,
    params: Option<Map<String, Value>>,
    timestamp_iso: &str,
    epoch_ms: u64,
    seq: u32,
) -> BridgeMessage {
    let id = message_id(&format!("{target}_{command}"), epoch_ms, seq);
    let mut obj = Map::new();
    obj.insert("target".to_string(), Value::from(target));
    obj.insert("command".to_string(), Value::from(command));
    obj.insert(
        "params".to_string(),
        Value::Object(params.unwrap_or_default()),
    );
    obj.insert("timestamp".to_string(), Value::from(timestamp_iso));
    obj.insert("id".to_string(), Value::from(id.clone()));
    BridgeMessage {
        filename: format!("{id}.json"),
        id,
        value: Value::Object(obj),
    }
}

/// Build a broadcast event — port of the dictionary from `send_event`:
/// `target="*"`, `command="event"`, `params={"type": …, **data}`.
pub fn build_event(
    event_type: &str,
    data: Map<String, Value>,
    timestamp_iso: &str,
    epoch_ms: u64,
    seq: u32,
) -> BridgeMessage {
    let id = message_id(&format!("event_{event_type}"), epoch_ms, seq);
    let mut params = Map::new();
    params.insert("type".to_string(), Value::from(event_type));
    for (k, v) in data {
        params.insert(k, v); // a repeated "type" overwrites the value, the position is kept
    }
    let mut obj = Map::new();
    obj.insert("target".to_string(), Value::from("*"));
    obj.insert("command".to_string(), Value::from("event"));
    obj.insert("params".to_string(), Value::Object(params));
    obj.insert("timestamp".to_string(), Value::from(timestamp_iso));
    obj.insert("id".to_string(), Value::from(id.clone()));
    BridgeMessage {
        filename: format!("{id}.json"),
        id,
        value: Value::Object(obj),
    }
}

/// Updates of the `rexon` section on a DE switch — port of the dictionary from
/// `notify_de_switch` (`last_de_switch` = the same `now_iso`).
pub fn de_switch_updates(from_de: &str, to_de: &str, now_iso: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("current_de".to_string(), Value::from(to_de));
    m.insert("last_de_switch".to_string(), Value::from(now_iso));
    m.insert("switched_from".to_string(), Value::from(from_de));
    m.insert("switched_to".to_string(), Value::from(to_de));
    m
}

// ============================================================================
// RexonBridge — the file plumbing (bridge.json + bridge_commands/)
// ============================================================================

/// A bridge with concrete paths. Unlike the Python original (where
/// `commands_dir` was always taken from the global constant even with a custom
/// `bridge_file`), the command directory is given explicitly — otherwise tests
/// and sandboxes would write into the real `~/.config/rexon`.
pub struct RexonBridge {
    pub bridge_file: PathBuf,
    pub commands_dir: PathBuf,
}

impl RexonBridge {
    /// A bridge on the standard paths (`~/.config/rexon/…`).
    pub fn new_default() -> Self {
        Self {
            bridge_file: default_bridge_file(),
            commands_dir: default_commands_dir(),
        }
    }

    /// A bridge on explicit paths (tests, sandboxes).
    pub fn new(bridge_file: PathBuf, commands_dir: PathBuf) -> Self {
        Self { bridge_file, commands_dir }
    }

    /// Create the directories (like `os.makedirs(..., exist_ok=True)` in `__init__`).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        if let Some(parent) = self.bridge_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&self.commands_dir)
    }

    /// Port of `_read`.
    pub fn read(&self) -> Map<String, Value> {
        read_bridge(&self.bridge_file)
    }

    /// Port of `_write`.
    pub fn write(&self, data: &Map<String, Value>) -> std::io::Result<()> {
        write_bridge(&self.bridge_file, data)
    }

    /// Port of `_update`: read → merge → write. The inter-process flock is an I/O
    /// boundary (the shim); write atomicity — tmp + rename.
    pub fn update_section(
        &self,
        section: &str,
        updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        let mut data = self.read();
        merge_update(&mut data, section, updates, now_iso);
        self.write(&data)
    }

    /// Port of `update_rexon_state`: forces `rexon_installed = true` (the position
    /// of an already passed key is kept — dict semantics).
    pub fn update_rexon_state(
        &self,
        mut updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        updates.insert("rexon_installed".to_string(), Value::Bool(true));
        self.update_section("rexon", updates, now_iso)
    }

    /// Port of `notify_de_switch`: section update + the `de_switched` event.
    pub fn notify_de_switch(
        &self,
        from_de: &str,
        to_de: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        self.update_section("rexon", de_switch_updates(from_de, to_de, now_iso), now_iso)?;
        let mut data = Map::new();
        data.insert("from".to_string(), Value::from(from_de));
        data.insert("to".to_string(), Value::from(to_de));
        self.send_event("de_switched", data, now_iso, epoch_ms)?;
        Ok(())
    }

    /// Port of `notify_de_installed`.
    pub fn notify_de_installed(
        &self,
        de_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut data = Map::new();
        data.insert("de_name".to_string(), Value::from(de_name));
        self.send_event("de_installed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Port of `notify_de_removed`.
    pub fn notify_de_removed(
        &self,
        de_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut data = Map::new();
        data.insert("de_name".to_string(), Value::from(de_name));
        self.send_event("de_removed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Port of `notify_theme_changed`: theme update + an event.
    pub fn notify_theme_changed(
        &self,
        theme_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut updates = Map::new();
        updates.insert("current_theme".to_string(), Value::from(theme_name));
        self.update_section("rexon", updates, now_iso)?;
        let mut data = Map::new();
        data.insert("theme_name".to_string(), Value::from(theme_name));
        self.send_event("theme_changed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Port of `notify_extension_installed`.
    pub fn notify_extension_installed(
        &self,
        ext_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut data = Map::new();
        data.insert("ext_name".to_string(), Value::from(ext_name));
        self.send_event("extension_installed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Port of `notify_backup_created`.
    pub fn notify_backup_created(&self, now_iso: &str, epoch_ms: u64) -> std::io::Result<()> {
        self.send_event("backup_created", Map::new(), now_iso, epoch_ms).map(|_| ())
    }

    /// Port of `notify_video_wallpaper`: the flag in the section and the news that
    /// the live wallpaper started or stopped.
    ///
    /// The `video_wallpaper_active` flag is ALWAYS written, whatever the caller
    /// decides about the news: the reading side looks at it when events did not
    /// reach it, and it has no right to lie.
    ///
    /// Warning — fix: stopping sent NOTHING — it only cleared the flag. A listener
    /// learned about the start through an event, but about the stop only if it
    /// thought of re-reading the flag itself. Now stopping has its own event
    /// [`WALLPAPER_STOPPED_EVENT`], paired with [`WALLPAPER_STARTED_EVENT`].
    ///
    /// `name` — the scene name; an empty one (or only spaces) gives no field in the
    /// event at all, there is nothing to invent on the engine's behalf. Stopping has
    /// no name: it stopped, that is all — the listener remembers which wallpaper
    /// stopped from the start.
    ///
    /// `announce` — whether to announce the change with an event. False keeps the
    /// flag correct and stays silent: this is how REPLACING the wallpaper with
    /// another one goes (the engine removes the old one before starting the new
    /// one), which is not a stop, and a "stopped — started" pair would only mislead
    /// the listener.
    pub fn notify_video_wallpaper(
        &self,
        active: bool,
        name: &str,
        announce: bool,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut updates = Map::new();
        updates.insert("video_wallpaper_active".to_string(), Value::Bool(active));
        self.update_section("rexon", updates, now_iso)?;
        if !announce {
            return Ok(());
        }
        let mut data = Map::new();
        let event = if active {
            let name = name.trim();
            if !name.is_empty() {
                data.insert("name".to_string(), Value::from(name));
            }
            WALLPAPER_STARTED_EVENT
        } else {
            WALLPAPER_STOPPED_EVENT
        };
        self.send_event(event, data, now_iso, epoch_ms)?;
        Ok(())
    }

    /// Port of `update_app_state` (the `app_<id>` section).
    pub fn update_app_state(
        &self,
        app_id: &str,
        updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        self.update_section(&format!("app_{app_id}"), updates, now_iso)
    }

    /// Port of `get_app_state`.
    pub fn get_app_state(&self, app_id: &str) -> Map<String, Value> {
        get_app_state(&self.read(), app_id)
    }

    /// Port of `is_app_running` (pid liveness via a closure, see [`app_running`]).
    pub fn is_app_running(
        &self,
        app_id: &str,
        pid_alive: impl FnOnce(i64) -> bool,
    ) -> bool {
        app_running(&self.get_app_state(app_id), pid_alive)
    }

    /// Port of `send_command`: write the command as a JSON file into the command
    /// directory. Returns the name of the written file.
    ///
    /// Deliberate divergences: the write is atomic (tmp + rename) — in the original
    /// the receiver could read a half-written file, consider it broken and delete
    /// it (the command was lost); on a name collision within one ms a free suffix
    /// `_{N}` is picked instead of a silent overwrite.
    pub fn send_command(
        &self,
        target: &str,
        command: &str,
        params: Option<Map<String, Value>>,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<String> {
        let prefix = format!("{target}_{command}");
        let msg = self.next_free_message(&prefix, epoch_ms, |seq| {
            build_command(target, command, params.clone(), now_iso, epoch_ms, seq)
        });
        self.write_message(&msg)?;
        Ok(msg.filename)
    }

    /// Port of `send_event`: a broadcast event (`target="*"`). Returns the name of
    /// the written file. The same atomicity/anti-collision as for commands.
    pub fn send_event(
        &self,
        event_type: &str,
        data: Map<String, Value>,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<String> {
        let prefix = format!("event_{event_type}");
        let msg = self.next_free_message(&prefix, epoch_ms, |seq| {
            build_event(event_type, data.clone(), now_iso, epoch_ms, seq)
        });
        self.write_message(&msg)?;
        Ok(msg.filename)
    }

    /// The first free suffix number for a message file name.
    fn next_free_message(
        &self,
        prefix: &str,
        epoch_ms: u64,
        build: impl Fn(u32) -> BridgeMessage,
    ) -> BridgeMessage {
        let mut seq = 0u32;
        loop {
            let candidate = format!("{}.json", message_id(prefix, epoch_ms, seq));
            if !self.commands_dir.join(&candidate).exists() {
                return build(seq);
            }
            seq += 1;
        }
    }

    /// Writing a message file through the shared door ([`crate::utils::atomic_write`]).
    ///
    /// The message reader is a foreign process, it takes the file as soon as it
    /// sees it. Half a message is "broken JSON" for it, and it DELETES such a file:
    /// the command is lost silently, without a trace.
    fn write_message(&self, msg: &BridgeMessage) -> std::io::Result<()> {
        let path = self.commands_dir.join(&msg.filename);
        let text = to_json_indent2(&msg.value);
        crate::utils::atomic_write(&path.to_string_lossy(), &text)
    }

    /// Port of `poll_commands`: commands for `app_id` in name-sorted order.
    /// `delete` — delete the ones returned. Broken files (non-JSON, JSON that is not
    /// an object) and those exceeding [`MAX_CMD_SIZE`] are deleted regardless of
    /// `delete` (as in the original); files without `.json` and commands for other
    /// targets are not touched. A missing directory → empty.
    pub fn poll_commands(&self, app_id: &str, delete: bool) -> Vec<Value> {
        let mut commands = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.commands_dir) else {
            return commands;
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        for filename in names {
            if !filename.ends_with(".json") {
                continue;
            }
            let filepath = self.commands_dir.join(&filename);
            let size = match std::fs::metadata(&filepath) {
                Ok(m) => m.len(),
                Err(_) => {
                    // The file disappeared between listing and stat — as with the general
                    // except: attempt to delete, suppress the error.
                    let _ = std::fs::remove_file(&filepath);
                    continue;
                }
            };
            if size > MAX_CMD_SIZE {
                let _ = std::fs::remove_file(&filepath);
                continue;
            }
            let parsed = std::fs::read_to_string(&filepath)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok());
            let Some(cmd @ Value::Object(_)) = parsed else {
                // A broken file — delete it (suppress a deletion error).
                let _ = std::fs::remove_file(&filepath);
                continue;
            };
            if cmd.get("target").and_then(|v| v.as_str()) == Some(app_id) {
                commands.push(cmd);
                if delete {
                    let _ = std::fs::remove_file(&filepath);
                }
            }
        }
        commands
    }

    /// Port of `poll_events`: all broadcast events (`target="*"`).
    pub fn poll_events(&self, delete: bool) -> Vec<Value> {
        self.poll_commands("*", delete)
    }

    /// Port of `cleanup_commands`: delete command files older than
    /// `max_age_seconds` (by mtime). `now_epoch` — the current time
    /// (`time.time()`), as a parameter.
    ///
    /// A deliberate divergence: in the original an error on one file (it
    /// disappeared between listing and stat) aborted the whole pass — here it is
    /// suppressed per file, and the cleanup continues.
    pub fn cleanup_commands(&self, max_age_seconds: f64, now_epoch: f64) {
        let Ok(entries) = std::fs::read_dir(&self.commands_dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let filepath = entry.path();
            if !filepath.is_file() {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&filepath) else {
                continue;
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            let Some(mtime) = mtime else { continue };
            if now_epoch - mtime > max_age_seconds {
                let _ = std::fs::remove_file(&filepath);
            }
        }
    }

    /// Port of `reset`: an empty bridge.json + cleaning up the command directory.
    ///
    /// A deliberate divergence: in the original the first `os.remove` failure (for
    /// example, a subdirectory in the command directory) aborted the cleanup — here
    /// errors are suppressed per file, directories are skipped.
    pub fn reset(&self) -> std::io::Result<()> {
        self.write(&Map::new())?;
        if let Ok(entries) = std::fs::read_dir(&self.commands_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }
}
