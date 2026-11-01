//! Protobuf-структуры протокола управления демоном waywallen.
//!
//! Порт `rexon/qt_wallpaper_engine/proto/control_pb2.py` + `filter_pb2.py`:
//! код НЕ написан руками, а сгенерирован `prost-build` (build.rs) из
//! оригинальных `control.proto`/`filter.proto`. Пакет proto —
//! `waywallen.control.v1`, поэтому prost кладёт всё в файл с этим именем.
//!
//! Сериализация/десериализация — через трейт `prost::Message` (`encode_to_vec`
//! / `decode`). Сверка с Python — golden-тест `proto_parity.rs`: эталонные
//! байты сняты с `google.protobuf` (`SerializeToString(deterministic=True)`),
//! Rust декодирует их и заново кодирует — для сообщений без `map` байты обязаны
//! совпасть один-в-один (у обоих реализаций proto3 поля пишутся по возрастанию
//! номера, дефолты опускаются). `map`-поля не байт-детерминированы (порядок
//! ключей в HashMap произволен) — для них сверка только структурная.
//!
//! Реэкспортим сгенерированный модуль как `control` — короткий путь
//! `rexon_core::proto::control::Request` и т.п.
pub mod control {
    include!(concat!(env!("OUT_DIR"), "/waywallen.control.v1.rs"));
}
