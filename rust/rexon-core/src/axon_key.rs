//! Ключ HMAC из клиента Razer Axon (проверен на Axon 2.11.1.0) — им
//! считается пароль файла обоев (`crate::axon_extract`).

pub const RAZER_HMAC_KEY: &[u8] = b"j6l-aUmhCc@tN%T_";
