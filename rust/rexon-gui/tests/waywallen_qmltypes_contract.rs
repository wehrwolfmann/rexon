//! Договор с C++-частью waywallen: `module.qmltypes` апстрима против того,
//! что Rexon регистрирует в модуле `waywallen.ui`.
//!
//! ЗАЧЕМ. Вендорный QML waywallen зовёт типы, которые у апстрима печатает
//! C++ (`W.WallpaperListQuery`, синглтон `W.Notify`, …), а у нас их пишет
//! Rust (`rexon-gui`, `rexon-qt-filter`) и регистрирует
//! `cpp/rexon_register_we.cpp`. Сверка по именам `W.*` в разметке (шаг
//! `[types]` в `scripts/sync-waywallen-qml.py`) ловит только новое ИМЯ
//! типа. Новое свойство, метод или сигнал у старого типа она не видит, и
//! такая поломка вылезает только на экране — пустой подписью или «undefined».
//!
//! `module.qmltypes` — это опись C++-типов, которую генератор QML-модуля
//! апстрима (lito-qt, qmltyperegistrar) кладёт рядом с модулем: имена,
//! синглтоны, свойства, методы, сигналы. Описывает, но не регистрирует —
//! регистрировать обязан процесс-потребитель, то есть мы. Свою опись
//! cxx-qt-build делает тем же qmltyperegistrar (`OUT_DIR`, модуль
//! `org.rexon.gui`), поэтому сравниваются два файла одного формата.
//!
//! Что проверяется:
//!   * каждый тип апстрима, который мы регистрируем, имеет у нас все его
//!     свойства, методы и сигналы (с унаследованными внутри описи), которые
//!     вендорный QML упоминает хотя бы словом. Член, имени которого в
//!     разметке нет вовсе (внутренние слоты D-Bus, `load_settings`, …),
//!     печатается списком, но не краснеет: зовут его только из C++
//!     апстрима. Проверка по слову, а не по `W.Тип.член`, нарочно грубая —
//!     разметка обращается и через нетипизированные свойства
//!     (`details?.author`), и лишнее совпадение безопаснее пропуска;
//!     лишнее у нас допустимо;
//!   * синглтон у апстрима — синглтон и у нас, и наоборот;
//!   * тип апстрима, который мы НЕ регистрируем, вендорный QML не зовёт
//!     по имени (`W.Имя`).
//! Известные расхождения перечислены в `KNOWN_GAPS` поимённо, с причиной.
//!
//! Опись апстрима — `data/qt_wallpaper_engine/qml_ui/waywallen/ui/module.qmltypes`
//! (того же тега, что вендорный QML, см. `UPSTREAM_TAG`). Прогнать против
//! другой описи, например сборки следующего выпуска, — переменная
//! `REXON_WW_QMLTYPES=<путь к module.qmltypes>`. Чтобы список «не хватает»
//! был точным ДО синхронизации разметки, рядом задаётся и разметка того же
//! выпуска: `REXON_WW_QML_DIR=<каталог ui/qml апстрима>` (иначе слова берутся
//! из вендорного QML дерева — прежнего выпуска).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MODULE: &str = "waywallen.ui";

/// Расхождения, о которых знаем: (тип, член, почему так).
const KNOWN_GAPS: &[(&str, &str, &str)] = &[];

// ---------------------------------------------------------------------------
// Разбор qmltypes (формат QtQuick.tooling: объекты `Имя { ключ: значение }`)
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

/// Объект с позиции `pos`, где стоит `Имя {`.
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

/// Один C++-тип описи.
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

/// Последний сегмент имени C++ (`waywallen::model::X` → `X`).
fn short(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// Члены типа вместе с унаследованными — по цепочке прототипов ВНУТРИ описи
/// (базовые классы Qt в описи модуля не лежат и не сравниваются).
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
// Наши регистрации
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Reg {
    class: String,
    singleton: bool,
    file: String,
}

/// `qmlRegister{Type,SingletonType}<Класс>("waywallen.ui", 1, 0, "Имя", …)`
/// из исходников регистраций обоих крейтов.
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

/// Члены модели фильтров: rexon-qt-filter — не QML-модуль cxx-qt, своей
/// описи у него нет, поэтому имена берутся из моста (`#[qproperty]`,
/// `#[qinvokable]`, `#[qsignal]`; имя для QML — `cxx_name`, иначе имя Rust).
fn filter_model_members(crate_dir: &Path) -> BTreeSet<(&'static str, String)> {
    let path = crate_dir.join("../rexon-qt-filter/src/filter_rule_model.rs");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("#[qproperty(") {
            // Свойство может быть разбито на несколько строк.
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

/// Имена `W.Имя`, которые зовёт вендорный QML, и все слова-идентификаторы
/// разметки.
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
    // Устаревшая запись в списке известных расхождений краснеет только против
    // своей описи: против чужой (переменная) список и не обязан сходиться.
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

/// Вендорный `qmldir` — того же вида, что пишет генератор QML-модуля апстрима
/// (lito-qt `qt.qml_module` с `prefer_resources = false`): `module`, затем
/// `typeinfo module.qmltypes`, компоненты с версией модуля апстрима, в конце
/// `depends`; строки `prefer` нет (с ней Qt ищет файлы в ресурсах чужого
/// бинаря). Тогда вендорная копия и модуль, установленный самим waywallen,
/// взаимозаменяемы: Rexon может грузить любой из них одним и тем же путём.
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
