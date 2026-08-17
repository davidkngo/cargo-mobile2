use crate::util::cli::{Report, Reportable};
use std::{collections::HashMap, ffi::OsString, fmt::Debug, path::Path};
use thiserror::Error;

pub trait ExplicitEnv: Debug {
    fn explicit_env(&self) -> HashMap<String, OsString>;
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The `{0}` environment variable isn't set, which is quite weird")]
    NotSet(&'static str),
}

impl Reportable for Error {
    fn report(&self) -> Report {
        Report::error("Failed to initialize base environment", self)
    }
}

#[derive(Clone, Debug)]
pub struct Env {
    vars: HashMap<String, std::ffi::OsString>,
}

impl Env {
    pub fn new() -> Result<Self, Error> {
        let mut vars = HashMap::new();

        let home = std::env::var_os("HOME").ok_or(Error::NotSet("HOME"))?;
        let path = std::env::var_os("PATH").ok_or(Error::NotSet("PATH"))?;
        // Forward a small allowlist of optional vars from the surrounding
        // environment. `QMAKE` selects the Qt kit used by qt-build-utils; for a
        // cross-compile (iOS/Android) it MUST point at a Qt kit built for the
        // target, otherwise the build falls back to the host (desktop) Qt and
        // links the wrong frameworks (e.g. `AppKit`/`OpenGL` for an iOS build).
        // Because build commands run with a fully-replaced environment
        // (`full_env`), an un-forwarded `QMAKE` would be invisible to the build
        // script even when the user has it set.
        for var in ["TERM", "SSH_AUTH_SOCK", "QMAKE"] {
            if let Some(value) = std::env::var_os(var) {
                vars.insert(var.into(), value);
            }
        }

        vars.insert("HOME".into(), home);
        vars.insert("PATH".into(), path);

        Ok(Self { vars })
    }

    pub fn path(&self) -> &OsString {
        self.vars.get("PATH").unwrap()
    }

    pub fn prepend_to_path(mut self, path: impl AsRef<Path>) -> Self {
        let mut path = path.as_ref().as_os_str().to_os_string();
        path.push(":");
        path.push(self.path().clone());
        self.vars.insert("PATH".into(), path);
        self
    }

    pub fn insert_env_var(&mut self, key: String, value: OsString) {
        self.vars.insert(key, value);
    }

    pub fn explicit_env_vars(mut self, vars: HashMap<String, OsString>) -> Self {
        self.vars.extend(vars);
        self
    }
}

impl ExplicitEnv for Env {
    fn explicit_env(&self) -> HashMap<String, OsString> {
        self.vars.clone()
    }
}
