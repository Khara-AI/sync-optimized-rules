//! The crate's single error type.

use std::fmt;
use std::io;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(PathBuf, io::Error),
    Json(PathBuf, serde_json::Error),
    /// A user mistake: unknown group, invalid name, path that is not a repository…
    Invalid(String),
}

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "{}: {e}", path.display()),
            Self::Json(path, e) => write!(f, "{}: invalid JSON: {e}", path.display()),
            Self::Invalid(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

/// Attach the path to an I/O error.
pub trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|e| Error::Io(path.into(), e))
    }
}
