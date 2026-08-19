//! Runtime configuration via [`figment`].
//!
//! The **effective** `Application.toml` — the developer's local (gitignored)
//! file if present, else the committed `Application.example.toml` — is staged
//! into `OUT_DIR` by `build.rs` and embedded here with `include_str!`. That way
//! config works with zero runtime dependence: a real device has neither the dev
//! machine's file paths nor a shell environment, so embedding is the only thing
//! that reaches it. Secrets stay out of git (the local file is gitignored) yet
//! still ship in the binary.
//!
//! `APP_<SECTION>__<KEY>` **sectional** env vars override the embedded values at
//! runtime (double underscore between section and key):
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

// The effective config, staged by build.rs and compiled in.
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
