//! Runtime configuration via [`figment`].
//!
//! `Application.toml` is embedded into the binary as a baseline (so device/
//! release builds have values without a shell environment), then overridden by
//! **sectional** environment variables: `APP_<SECTION>__<KEY>`. The section and
//! key are separated by a double underscore so keys that themselves contain an
//! underscore map correctly:
//!
//! ```text
//! APP_DEV__HOST -> dev.host
//! APP_DEV__PORT -> dev.port
//! ```
//!
//! Add your own `#[derive(Deserialize)]` sections to [`Application`].

use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

// The committed baseline, compiled in. Env vars layered on top win.
const BASELINE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Application.toml"));

#[derive(Debug, Default, Deserialize)]
pub struct Application {
    #[serde(default)]
    pub dev: Dev,
}

/// Hot-reload dev server the app dials OUT to (the dev machine, not the phone).
#[derive(Debug, Deserialize)]
pub struct Dev {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for Dev {
    fn default() -> Self {
        Self { host: default_host(), port: default_port() }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_owned()
}
fn default_port() -> u16 {
    8790
}

/// Load the whole application config: embedded `Application.toml`, then
/// `APP_*__*` sectional environment overrides.
pub fn load() -> Application {
    Figment::new()
        .merge(Toml::string(BASELINE))
        .merge(Env::prefixed("APP_").split("__"))
        .extract()
        .unwrap_or_default()
}
