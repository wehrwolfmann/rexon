//! HMAC key from the Razer Axon client (verified on Axon 2.11.1.0); it is
//! used to compute the password of a wallpaper file (`crate::axon_extract`).

pub const RAZER_HMAC_KEY: &[u8] = b"j6l-aUmhCc@tN%T_";
