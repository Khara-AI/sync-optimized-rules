//! Where sor keeps its configuration on this machine.

use std::env;
use std::path::PathBuf;

use crate::error::{Error, Result};

const DIR: &str = "sync-optimized-rules";

/// `$SOR_HOME`, else the platform's config folder: `$XDG_CONFIG_HOME` or
/// `~/.config` on Unix, `%APPDATA%` on Windows.
pub fn config_home() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("SOR_HOME") {
        return Ok(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        env::var_os("APPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join(DIR))
        .ok_or_else(|| Error::invalid("cannot find a config folder: set SOR_HOME"))
}
