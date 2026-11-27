//! Generation of the waywallen daemon protocol Rust structs from `.proto`.
//!
//! The only source is the original `control.proto`/`filter.proto` in
//! `data/qt_wallpaper_engine/proto/` (waywallen vendor, MIT). We do NOT copy them into
//! the crate, but read them directly by a path relative to `CARGO_MANIFEST_DIR`,
//! so as not to diverge from upstream on update (git diff of their proto).
//!
//! Requires `protoc` in PATH (prost-build calls it). Two files are generated:
//!
//! * `OUT_DIR/waywallen.control.v1.rs` — the structs themselves, included with
//!   `include!` from `src/proto.rs`;
//! * `OUT_DIR/request_payload_fields.rs` — the table "field name of the `oneof payload`
//!   of the `Request` message → field number", included with `include!` from
//!   `src/we_client.rs`.
//!
//! Why the table is GENERATED too rather than written by hand: the universal path
//! `WaywallenClient.call(field, msg)` builds the frame by field name, and a handwritten
//! list silently falls behind the vendor protocol. That is exactly what happened: upstream
//! added fields 69–74 (`plugin_action`, `subscription_status`,
//! `subscription_set`, `qr_login_cancel`, `remote_settings_patch`,
//! `wallpaper_unsubscribe`), the handwritten table did not know about them, and all these
//! requests quietly went to the Python fallback. We read the `FileDescriptorSet` that
//! protoc produces during compilation — the same source as the structs themselves.

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

    // Rebuild when the .proto changes.
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

/// `REXON_GIT_REV` — the short commit hash for the startup line in the log
/// (`applog`). No git or no tree (build from an archive) — the variable is not
/// set, and the log writes only the crate version.
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

/// `OUT_DIR/helper_catalogs.rs` — translation catalogs for messages of the
/// root helpers (`src/system_l10n.rs`): for each code from `po/LINGUAS` — the text
/// of its `.po`, embedded with `include_str!`. Add a language to LINGUAS — the helpers
/// will speak it from the next build on, the code is not touched.
///
/// `REXON_HELPER_PO_DIR` substitutes a draft for the `po/` directory (its own LINGUAS and
/// `.po`) — to check the helper texts before the translation is merged into the shared
/// catalog.
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
        "// GENERATED by build.rs from po/LINGUAS — do not edit by hand.\n\
         pub(crate) const HELPER_CATALOGS: &[(&str, &str)] = &[\n",
    );
    for code in text
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(str::split_whitespace)
    {
        // The code goes into the text of a root script: safe characters only.
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

/// Code of the `REQUEST_PAYLOAD_FIELDS` table from the descriptor: the `oneof payload` fields
/// of the `Request` message in declaration order (which is also ascending by number).
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
        "// GENERATED by build.rs from control.proto — do not edit by hand.\n\
         /// Field numbers of `oneof payload` of the `Request` message (control.proto),\n\
         /// in declaration order. The source is the `FileDescriptorSet` from protoc.\n\
         pub static REQUEST_PAYLOAD_FIELDS: &[(&str, u32)] = &[\n",
    );
    for field in &message.field {
        // proto3 optional also creates a synthetic oneof — take only ours.
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
