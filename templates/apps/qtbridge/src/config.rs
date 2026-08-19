//! Runtime config. `build.rs` stages the effective `Application.toml` (local
//! gitignored file if present, else the committed example) into `OUT_DIR`; it's
//! embedded here so it works on-device with no filesystem/env dependence.
//! `APP_<SECTION>__<KEY>` env vars override it (double underscore separates
//! section and key). Add your own sections to [`Application`].

use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

const EMBEDDED: &str = include_str!(concat!(env!("OUT_DIR"), "/application.toml"));

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

/// Load config: embedded effective `Application.toml`, then `APP_*__*` env.
pub fn load() -> Application {
    let fig = Figment::new()
        .merge(Toml::string(EMBEDDED))
        .merge(Env::prefixed("APP_").split("__"));
    match fig.extract::<Application>() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("[config] failed to load, using defaults: {e}");
            Application::default()
        }
    }
}
