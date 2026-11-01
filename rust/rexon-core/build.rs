//! Генерация Rust-структур протокола демона waywallen из `.proto`.
//!
//! Единственный источник — оригинальные `control.proto`/`filter.proto` в
//! `data/qt_wallpaper_engine/proto/` (вендор waywallen, MIT). НЕ копируем их в
//! крейт, а читаем напрямую по относительному пути от `CARGO_MANIFEST_DIR`,
//! чтобы не расходиться с апстримом при обновлении (git diff их proto).
//!
//! Требует `protoc` в PATH (prost-build вызывает его). Генерируются два файла:
//!
//! * `OUT_DIR/waywallen.control.v1.rs` — сами структуры, подключается
//!   `include!` из `src/proto.rs`;
//! * `OUT_DIR/request_payload_fields.rs` — таблица «имя поля `oneof payload`
//!   сообщения `Request` → номер поля», подключается `include!` из
//!   `src/we_client.rs`.
//!
//! Почему таблица тоже ГЕНЕРИТСЯ, а не пишется руками: универсальный путь
//! `WaywallenClient.call(field, msg)` собирает кадр по имени поля, и рукописный
//! список молча отстаёт от вендорского протокола. Так и случилось: апстрим
//! добавил поля 69–74 (`plugin_action`, `subscription_status`,
//! `subscription_set`, `qr_login_cancel`, `remote_settings_patch`,
//! `wallpaper_unsubscribe`), рукописная таблица о них не знала, и все эти
//! запросы тихо уходили на Python-фолбэк. Читаем `FileDescriptorSet`, который
//! protoc отдаёт при компиляции — источник тот же, что у самих структур.

use std::path::PathBuf;

use prost::Message;

fn main() {
    // rust/rexon-core → repo root → data/qt_wallpaper_engine/proto
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let proto_dir = manifest
        .join("../../data/qt_wallpaper_engine/proto")
        .canonicalize()
        .expect("каталог proto оригинала должен существовать");

    let control = proto_dir.join("control.proto");
    let filter = proto_dir.join("filter.proto");

    // Пересобирать при изменении .proto.
    println!("cargo:rerun-if-changed={}", control.display());
    println!("cargo:rerun-if-changed={}", filter.display());

    let descriptor_path = out_dir.join("waywallen_descriptor.bin");
    let mut config = prost_build::Config::new();
    config.file_descriptor_set_path(&descriptor_path);
    config
        .compile_protos(&[&control, &filter], &[&proto_dir])
        .expect("prost-build: компиляция .proto (нужен protoc в PATH)");

    let bytes = std::fs::read(&descriptor_path)
        .expect("prost-build должен был записать FileDescriptorSet");
    let set = prost_types::FileDescriptorSet::decode(&bytes[..])
        .expect("FileDescriptorSet должен разбираться");

    let table = request_payload_table(&set);
    std::fs::write(out_dir.join("request_payload_fields.rs"), table)
        .expect("запись таблицы полей Request");

    helper_catalogs(&manifest, &out_dir);
    git_revision(&manifest);
}

/// `REXON_GIT_REV` — короткий хеш коммита для строки запуска в журнале
/// (`applog`). Нет git или дерева (сборка из архива) — переменная не
/// ставится, и журнал пишет только версию крейта.
fn git_revision(manifest: &std::path::Path) {
    let root = manifest.join("../..");
    let git = root.join(".git");
    println!("cargo:rerun-if-changed={}", git.join("HEAD").display());
    if let Ok(head) = std::fs::read_to_string(git.join("HEAD")) {
        if let Some(r) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed={}", git.join(r).display());
        }
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--short=10", "HEAD"])
        .output();
    if let Ok(out) = out {
        let rev = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !rev.is_empty() {
            println!("cargo:rustc-env=REXON_GIT_REV={rev}");
        }
    }
}

/// `OUT_DIR/helper_catalogs.rs` — каталоги перевода для сообщений
/// root-помощников (`src/system_l10n.rs`): по коду из `po/LINGUAS` — текст
/// его `.po`, вшитый `include_str!`. Добавили язык в LINGUAS — помощники
/// заговорят на нём со следующей сборкой, код не трогается.
///
/// `REXON_HELPER_PO_DIR` подменяет каталог `po/` черновиком (свой LINGUAS и
/// `.po`) — проверить тексты помощников до того, как перевод влит в общий
/// каталог.
fn helper_catalogs(manifest: &std::path::Path, out_dir: &std::path::Path) {
    println!("cargo:rerun-if-env-changed=REXON_HELPER_PO_DIR");
    let po_dir = match std::env::var_os("REXON_HELPER_PO_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => manifest.join("../../po"),
    };
    let linguas = po_dir.join("LINGUAS");
    println!("cargo:rerun-if-changed={}", linguas.display());
    let text = std::fs::read_to_string(&linguas).unwrap_or_default();
    let mut out = String::from(
        "// СГЕНЕРИРОВАНО build.rs из po/LINGUAS — руками не править.\n\
         pub(crate) const HELPER_CATALOGS: &[(&str, &str)] = &[\n",
    );
    for code in text
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(str::split_whitespace)
    {
        // Код идёт в текст скрипта от root: только безопасные символы.
        if !code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '@')) {
            println!("cargo:warning=po/LINGUAS: код языка {code:?} пропущен");
            continue;
        }
        let po = po_dir.join(format!("{code}.po"));
        let Ok(po) = po.canonicalize() else {
            println!("cargo:warning=po/LINGUAS: нет файла {code}.po");
            continue;
        };
        println!("cargo:rerun-if-changed={}", po.display());
        out.push_str(&format!("    ({code:?}, include_str!({:?})),\n", po.display().to_string()));
    }
    out.push_str("];\n");
    std::fs::write(out_dir.join("helper_catalogs.rs"), out).expect("запись helper_catalogs.rs");
}

/// Код таблицы `REQUEST_PAYLOAD_FIELDS` из дескриптора: поля `oneof payload`
/// сообщения `Request` в порядке объявления (он же — по возрастанию номера).
fn request_payload_table(set: &prost_types::FileDescriptorSet) -> String {
    let message = set
        .file
        .iter()
        .flat_map(|f| f.message_type.iter())
        .find(|m| m.name() == "Request")
        .expect("в control.proto должно быть сообщение Request");

    let payload_index = message
        .oneof_decl
        .iter()
        .position(|o| o.name() == "payload")
        .expect("у Request должен быть oneof payload") as i32;

    let mut out = String::from(
        "// СГЕНЕРИРОВАНО build.rs из control.proto — руками не править.\n\
         /// Номера полей `oneof payload` сообщения `Request` (control.proto),\n\
         /// в порядке объявления. Источник — `FileDescriptorSet` от protoc.\n\
         pub static REQUEST_PAYLOAD_FIELDS: &[(&str, u32)] = &[\n",
    );
    for field in &message.field {
        // proto3 optional тоже заводит синтетический oneof — берём только наш.
        if field.oneof_index != Some(payload_index) {
            continue;
        }
        out.push_str(&format!(
            "    ({:?}, {}),\n",
            field.name(),
            field.number()
        ));
    }
    out.push_str("];\n");
    out
}
