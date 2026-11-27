//! The contract with waywallen's C++ part: upstream's `module.qmltypes`
//! against what Rexon registers in the `waywallen.ui` module.
//!
//! WHY. waywallen's vendored QML calls types that upstream emits in C++
//! (`W.WallpaperListQuery`, the `W.Notify` singleton, …), while for us they
//! are written in Rust (`rexon-gui`, `rexon-qt-filter`) and registered by
//! `cpp/rexon_register_we.cpp`. Checking the `W.*` names in the markup (the
//! `[types]` step in `scripts/sync-waywallen-qml.py`) catches only a new type
//! NAME. It does not see a new property, method or signal of an old type,
//! and such a breakage shows up only on screen — as an empty label or
//! «undefined».
//!
//! `module.qmltypes` is the inventory of C++ types that upstream's QML module
//! generator (lito-qt, qmltyperegistrar) puts next to the module: names,
//! singletons, properties, methods, signals. It describes but does not
//! register — the consuming process, that is, we, must register them.
//! cxx-qt-build makes its own inventory with the same qmltyperegistrar
//! (`OUT_DIR`, the `org.rexon.gui` module), so two files of the same format
//! are compared.
//!
//! What is checked:
//!   * every upstream type we register has all its properties, methods and
//!     signals (including those inherited inside the inventory) that the
//!     vendored QML mentions by at least a word. A member whose name does not
//!     appear in the markup at all (internal D-Bus slots, `load_settings`, …)
//!     is printed as a list but does not turn red: only upstream's C++ calls
//!     it. Checking by word rather than by `W.Type.member` is deliberately
//!     coarse — the markup also accesses things through untyped properties
//!     (`details?.author`), and an extra match is safer than a miss; extras
//!     on our side are allowed;
//!   * a singleton upstream is a singleton for us too, and vice versa;
//!   * an upstream type that we do NOT register is not called by name by the
//!     vendored QML (`W.Name`).
//! Known divergences are listed in `KNOWN_GAPS` by name, with a reason.
//!
//! The upstream inventory is
//! `data/qt_wallpaper_engine/qml_ui/waywallen/ui/module.qmltypes` (of the same
//! tag as the vendored QML, see `UPSTREAM_TAG`). To run against another
//! inventory, for example a build of the next release, use the variable
//! `REXON_WW_QMLTYPES=<path to module.qmltypes>`. For the "missing" list to be
//! accurate BEFORE syncing the markup, the markup of the same release is set
//! alongside: `REXON_WW_QML_DIR=<upstream's ui/qml directory>` (otherwise the
//! words are taken from the vendored QML of the tree — the previous
//! release).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MODULE: &str = "waywallen.ui";

/// Divergences we know about: (type, member, why it is so).
const KNOWN_GAPS: &[(&str, &str, &str)] = &[];

// ---------------------------------------------------------------------------
// Parsing qmltypes (the QtQuick.tooling format: objects `Name { key: value }`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num,
    Punct(char),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                s.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(s));
        } else if c.is_ascii_digit() || c == '-' {
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '-')
            {
                i += 1;
            }
            out.push(Tok::Num);
        } else if c.is_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                s.push(chars[i]);
                i += 1;
            }
            out.push(Tok::Ident(s));
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    out
}

#[derive(Debug, Default)]
struct Obj {
    kind: String,
    fields: BTreeMap<String, Vec<String>>,
    children: Vec<Obj>,
}

impl Obj {
    fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }
}

/// The object from position `pos`, where `Name {` stands.
fn parse_obj(toks: &[Tok], pos: &mut usize) -> Obj {
    let kind = match &toks[*pos] {
        Tok::Ident(s) => s.clone(),
        t => panic!("ждали имя объекта, а там {t:?}"),
    };
    *pos += 1;
    assert_eq!(toks[*pos], Tok::Punct('{'), "после {kind} ждали '{{'");
    *pos += 1;
    let mut obj = Obj {
        kind,
        ..Obj::default()
    };
    loop {
        match &toks[*pos] {
            Tok::Punct('}') => {
                *pos += 1;
                return obj;
            }
            Tok::Punct(';') => *pos += 1,
            Tok::Ident(name) if toks.get(*pos + 1) == Some(&Tok::Punct('{')) => {
                let _ = name;
                obj.children.push(parse_obj(toks, pos));
            }
            Tok::Ident(key) => {
                let key = key.clone();
                *pos += 1;
                assert_eq!(toks[*pos], Tok::Punct(':'), "после ключа {key} ждали ':'");
                *pos += 1;
                let mut values = Vec::new();
                if toks[*pos] == Tok::Punct('[') {
                    *pos += 1;
                    while toks[*pos] != Tok::Punct(']') {
                        if let Tok::Str(s) | Tok::Ident(s) = &toks[*pos] {
                            values.push(s.clone());
                        }
                        *pos += 1;
                    }
                    *pos += 1;
                } else {
                    if let Tok::Str(s) | Tok::Ident(s) = &toks[*pos] {
                        values.push(s.clone());
                    }
                    *pos += 1;
                }
                obj.fields.insert(key, values);
            }
            t => panic!("неожиданный знак {t:?}"),
        }
    }
}

/// One C++ type of the inventory.
#[derive(Debug, Default)]
struct Comp {
    name: String,
    prototype: Option<String>,
    exports: Vec<String>,
    singleton: bool,
    members: BTreeSet<(&'static str, String)>,
}

fn parse_qmltypes(text: &str) -> Vec<Comp> {
    let toks = tokenize(text);
    let mut pos = toks
        .iter()
        .position(|t| *t == Tok::Ident("Module".into()))
        .expect("в описи нет объекта Module");
    let module = parse_obj(&toks, &mut pos);
    module
        .children
        .iter()
        .filter(|c| c.kind == "Component")
        .map(|c| {
            let mut comp = Comp {
                name: c.field("name").unwrap_or_default().to_string(),
                prototype: c.field("prototype").map(str::to_string),
                exports: c.fields.get("exports").cloned().unwrap_or_default(),
                singleton: c.field("isSingleton") == Some("true"),
                ..Comp::default()
            };
            for m in &c.children {
                let kind = match m.kind.as_str() {
                    "Property" => "свойство",
                    "Method" => "метод",
                    "Signal" => "сигнал",
                    _ => continue,
                };
                if let Some(name) = m.field("name") {
                    comp.members.insert((kind, name.to_string()));
                }
            }
            comp
        })
        .collect()
}

/// The last segment of a C++ name (`waywallen::model::X` → `X`).
fn short(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// The type's members together with inherited ones — along the prototype
/// chain INSIDE the inventory (Qt base classes do not lie in the module
/// inventory and are not compared).
fn members_with_bases(comps: &[Comp], comp: &Comp) -> BTreeSet<(&'static str, String)> {
    let mut out = comp.members.clone();
    let mut seen = BTreeSet::new();
    let mut proto = comp.prototype.clone();
    while let Some(p) = proto {
        if !seen.insert(p.clone()) {
            break;
        }
        let base = comps
            .iter()
            .find(|c| c.name == p)
            .or_else(|| comps.iter().find(|c| short(&c.name) == short(&p)));
        match base {
            Some(b) => {
                out.extend(b.members.iter().cloned());
                proto = b.prototype.clone();
            }
            None => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Our registrations
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Reg {
    class: String,
    singleton: bool,
    file: String,
}

/// `qmlRegister{Type,SingletonType}<Class>("waywallen.ui", 1, 0, "Name", …)`
/// from the registration sources of both crates.
fn registrations(crate_dir: &Path) -> BTreeMap<String, Reg> {
    let dirs = [
        crate_dir.join("cpp"),
        crate_dir.join("../rexon-qt-filter/cpp"),
    ];
    let mut out = BTreeMap::new();
    for dir in dirs {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("cpp") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut rest = text.as_str();
            while let Some(at) = rest.find("qmlRegister") {
                rest = &rest[at + "qmlRegister".len()..];
                let singleton = rest.starts_with("SingletonType<");
                let Some(lt) = rest.find('<') else { break };
                let Some(gt) = rest.find('>') else { break };
                if lt > 20 || gt < lt {
                    continue;
                }
                let class = rest[lt + 1..gt].trim().to_string();
                let Some(close) = rest.find(')') else { break };
                let args: Vec<&str> = rest[gt + 1..close]
                    .trim_start_matches('(')
                    .split(',')
                    .collect();
                if args.len() < 4 || args[0].trim() != format!("\"{MODULE}\"") {
                    continue;
                }
                let name = args[3].trim().trim_matches('"').to_string();
                out.insert(
                    name,
                    Reg {
                        class,
                        singleton,
                        file: file.clone(),
                    },
                );
            }
        }
    }
    out
}

/// Members of the filter model: rexon-qt-filter is not a cxx-qt QML module and
/// has no inventory of its own, so the names are taken from the bridge
/// (`#[qproperty]`, `#[qinvokable]`, `#[qsignal]`; the name for QML is
/// `cxx_name`, otherwise the Rust name).
fn filter_model_members(crate_dir: &Path) -> BTreeSet<(&'static str, String)> {
    let path = crate_dir.join("../rexon-qt-filter/src/filter_rule_model.rs");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("#[qproperty(") {
            // A property may be split over several lines.
            let mut decl = rest.to_string();
            let mut j = i;
            while !decl.contains(")]") && j + 1 < lines.len() {
                j += 1;
                decl.push(' ');
                decl.push_str(lines[j]);
            }
            let parts: Vec<&str> = decl.split(',').map(str::trim).collect();
            let name = parts
                .iter()
                .find_map(|p| {
                    p.strip_prefix("cxx_name = \"")
                        .map(|s| s.trim_end_matches('"'))
                })
                .unwrap_or(parts[1]);
            out.insert(("свойство", name.to_string()));
            out.insert(("сигнал", format!("{name}Changed")));
        } else if line == "#[qinvokable]" || line == "#[qsignal]" {
            let kind = if line == "#[qinvokable]" {
                "метод"
            } else {
                "сигнал"
            };
            let mut cxx = None;
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with("#[") {
                if let Some(n) = lines[j].strip_prefix("#[cxx_name = \"") {
                    cxx = Some(n.trim_end_matches("\"]").to_string());
                }
                j += 1;
            }
            if let Some(fn_at) = lines.get(j).and_then(|l| l.find("fn ")) {
                let tail = &lines[j][fn_at + 3..];
                let rust = tail.split(['(', '<']).next().unwrap_or_default().trim();
                out.insert((kind, cxx.unwrap_or_else(|| rust.to_string())));
            }
        }
        i += 1;
    }
    out
}

/// The `W.Name` names the vendored QML calls, and all identifier words of the
/// markup.
fn vendor_uses(qml_dir: &Path) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut out = BTreeSet::new();
    let mut words = BTreeSet::new();
    let mut stack = vec![qml_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("qml") {
                let text = std::fs::read_to_string(&path).unwrap();
                words.extend(
                    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .filter(|w| !w.is_empty())
                        .map(str::to_string),
                );
                let bytes = text.as_bytes();
                let mut from = 0;
                while let Some(at) = text[from..].find("W.") {
                    let at = from + at;
                    from = at + 2;
                    if at > 0
                        && (bytes[at - 1].is_ascii_alphanumeric()
                            || bytes[at - 1] == b'_'
                            || bytes[at - 1] == b'.')
                    {
                        continue;
                    }
                    let name: String = text[at + 2..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                        out.insert(name);
                    }
                }
            }
        }
    }
    (out, words)
}

fn repo_root(crate_dir: &Path) -> PathBuf {
    crate_dir.join("../..")
}

fn upstream_qmltypes(crate_dir: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("REXON_WW_QMLTYPES") {
        return PathBuf::from(p);
    }
    repo_root(crate_dir).join("data/qt_wallpaper_engine/qml_ui/waywallen/ui/module.qmltypes")
}

fn vendor_qml_dir(crate_dir: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("REXON_WW_QML_DIR") {
        return PathBuf::from(p);
    }
    repo_root(crate_dir).join("data/qt_wallpaper_engine/qml_ui/waywallen/ui/qml")
}

#[test]
fn rexon_provides_what_upstream_module_qmltypes_describes() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let upstream_path = upstream_qmltypes(&crate_dir);
    let upstream_text = std::fs::read_to_string(&upstream_path).unwrap_or_else(|e| {
        panic!(
            "нет описи апстрима {}: {e}. Её кладёт генератор QML-модуля waywallen \
             (lito build -p waywallen-ui; build/<профиль>/generated/waywallen-ui/lito-qml/waywallen/ui/)",
            upstream_path.display()
        )
    });
    let upstream = parse_qmltypes(&upstream_text);
    let ours_path = PathBuf::from(env!("OUT_DIR"))
        .join("qt-build-utils/qml_modules/org/rexon/gui/plugin.qmltypes");
    let ours = parse_qmltypes(
        &std::fs::read_to_string(&ours_path)
            .unwrap_or_else(|e| panic!("нет нашей описи {}: {e}", ours_path.display())),
    );
    let regs = registrations(&crate_dir);
    let qml_dir = vendor_qml_dir(&crate_dir);
    let (uses, words) = vendor_uses(&qml_dir);
    let known: BTreeSet<(&str, &str)> = KNOWN_GAPS.iter().map(|(t, m, _)| (*t, *m)).collect();

    let prefix = format!("{MODULE}/");
    let mut problems = Vec::new();
    let mut unregistered = Vec::new();
    let mut checked = 0;
    let mut used_gaps = BTreeSet::new();
    let mut unused_missing = Vec::new();
    for comp in &upstream {
        let Some(export) = comp.exports.iter().find_map(|e| e.strip_prefix(&prefix)) else {
            continue;
        };
        let name = export.split_whitespace().next().unwrap_or_default();
        let Some(reg) = regs.get(name) else {
            if uses.contains(name) {
                problems.push(format!(
                    "{name}: вендорный QML зовёт W.{name}, а Rexon его не регистрирует"
                ));
            } else {
                unregistered.push(name.to_string());
            }
            continue;
        };
        checked += 1;
        if reg.singleton != comp.singleton {
            problems.push(format!(
                "{name}: у апстрима {}синглтон, у нас ({}) — {}",
                if comp.singleton { "" } else { "не " },
                reg.file,
                if reg.singleton {
                    "синглтон"
                } else {
                    "обычный тип"
                }
            ));
        }
        let ours_members =
            if reg.class == "WallpaperFilterRuleModel" && reg.file == "rexon_register.cpp" {
                filter_model_members(&crate_dir)
            } else {
                let Some(our) = ours.iter().find(|c| short(&c.name) == short(&reg.class)) else {
                    problems.push(format!(
                        "{name}: класса {} нет в нашей описи {}",
                        reg.class,
                        ours_path.display()
                    ));
                    continue;
                };
                members_with_bases(&ours, our)
            };
        for (kind, member) in members_with_bases(&upstream, comp) {
            if ours_members.contains(&(kind, member.clone())) {
                continue;
            }
            if known.contains(&(name, member.as_str())) {
                used_gaps.insert((name.to_string(), member.clone()));
                continue;
            }
            if !words.contains(&member) {
                unused_missing.push(format!("{name}.{member}"));
                continue;
            }
            let what = match kind {
                "свойство" => "свойства",
                "метод" => "метода",
                _ => "сигнала",
            };
            problems.push(format!("{name}: нет {what} `{member}`"));
        }
    }

    eprintln!(
        "опись {} (разметка {}): сверено типов {checked}; не регистрируем \
         (вендорный QML по имени не зовёт): {}",
        upstream_path.display(),
        qml_dir.display(),
        unregistered.join(", ")
    );
    eprintln!(
        "нет у нас, но разметка их не упоминает: {}",
        if unused_missing.is_empty() {
            "—".to_string()
        } else {
            unused_missing.join(", ")
        }
    );
    let stale: Vec<String> = KNOWN_GAPS
        .iter()
        .filter(|(t, m, _)| !used_gaps.contains(&(t.to_string(), m.to_string())))
        .map(|(t, m, _)| format!("{t}.{m}"))
        .collect();
    // A stale entry in the list of known divergences turns red only against its
    // own inventory: against another one (the variable) the list is not obliged
    // to match.
    if std::env::var_os("REXON_WW_QMLTYPES").is_none() && !stale.is_empty() {
        problems.push(format!(
            "KNOWN_GAPS: расхождения больше нет, убрать запись: {}",
            stale.join(", ")
        ));
    }
    assert!(
        problems.is_empty(),
        "Rexon расходится с описью C++-типов waywallen ({} шт.):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

#[test]
fn parser_reads_a_component_with_inherited_members() {
    let text = r#"import QtQuick.tooling 1.2
Module {
    Component {
        name: "ns::Base"
        Property { name: "status"; type: "int" }
    }
    Component {
        name: "ns::Child"
        prototype: "ns::Base"
        exports: ["waywallen.ui/Child 0.0"]
        isSingleton: true
        Signal { name: "done" }
        Method {
            name: "run"
            Parameter { name: "x"; type: "int" }
        }
    }
}"#;
    let comps = parse_qmltypes(text);
    assert_eq!(comps.len(), 2);
    let child = &comps[1];
    assert!(child.singleton);
    assert_eq!(child.exports, vec!["waywallen.ui/Child 0.0".to_string()]);
    let all = members_with_bases(&comps, child);
    assert!(all.contains(&("свойство", "status".to_string())));
    assert!(all.contains(&("сигнал", "done".to_string())));
    assert!(all.contains(&("метод", "run".to_string())));
}

/// The vendored `qmldir` is of the same kind that upstream's QML module
/// generator writes (lito-qt `qt.qml_module` with `prefer_resources = false`):
/// `module`, then `typeinfo module.qmltypes`, components with upstream's
/// module version, `depends` at the end; there is no `prefer` line (with it Qt
/// looks for files in the resources of someone else's binary). Then the
/// vendored copy and the module installed by waywallen itself are
/// interchangeable: Rexon can load either of them by the same path.
#[test]
fn vendor_qmldir_has_the_shape_lito_qt_writes() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui = repo_root(&crate_dir).join("data/qt_wallpaper_engine/qml_ui/waywallen/ui");
    let text = std::fs::read_to_string(ui.join("qmldir")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.first(),
        Some(&"module waywallen.ui"),
        "первая строка — имя модуля"
    );
    assert_eq!(
        lines.get(1),
        Some(&"typeinfo module.qmltypes"),
        "вторая строка — опись типов"
    );
    assert!(
        ui.join("module.qmltypes").is_file(),
        "опись, на которую ссылается qmldir, лежит рядом"
    );
    assert!(
        !text.lines().any(|l| l.starts_with("prefer")),
        "строки prefer быть не должно"
    );
    assert!(
        !text.lines().any(|l| l.starts_with('#')),
        "lito-qt комментариев не пишет"
    );

    let body = &lines[2..];
    let first_depends = body
        .iter()
        .position(|l| l.starts_with("depends "))
        .unwrap_or(body.len());
    assert!(
        body[first_depends..]
            .iter()
            .all(|l| l.starts_with("depends ")),
        "depends — только в конце"
    );
    assert!(first_depends < body.len(), "зависимости модуля перечислены");
    let mut versions = BTreeSet::new();
    for line in &body[..first_depends] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let parts = if parts.first() == Some(&"singleton") {
            &parts[1..]
        } else {
            &parts[..]
        };
        assert_eq!(
            parts.len(),
            3,
            "строка компонента «Имя версия путь»: {line}"
        );
        let path = parts[2];
        assert_eq!(
            Path::new(path).file_stem().and_then(|s| s.to_str()),
            Some(parts[0]),
            "имя компонента — имя файла: {line}"
        );
        assert!(ui.join(path).is_file(), "файл компонента есть: {path}");
        versions.insert(parts[1].to_string());
    }
    assert_eq!(
        versions.len(),
        1,
        "у всех компонентов одна версия модуля: {versions:?}"
    );
}
