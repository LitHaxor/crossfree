use std::fmt;
use std::path::PathBuf;
use std::process::ExitStatus;

/// Launcher errors.
#[derive(Debug)]
pub enum Error {
    /// A file system operation failed.
    Io {
        /// What was being done.
        what: String,
        /// The path involved.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The engine bundle is unusable.
    Engine(String),
    /// A prefix name is not acceptable.
    InvalidName(String),
    /// A prefix does not exist, or is not a prefix.
    NoSuchPrefix(String),
    /// A bottle setting that is not valid.
    Setting(String),
    /// A per-game setting the engine's rules refuse.
    Rules(compatdb_core::FieldError),
    /// A helper program failed.
    Command {
        /// What was run.
        what: String,
        /// How it ended.
        status: ExitStatus,
        /// Its standard error, trimmed.
        stderr: String,
    },
}

impl Error {
    pub(crate) fn io(what: impl Into<String>, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { what: what.into(), path: path.into(), source }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { what, path, source } => write!(f, "{what} {}: {source}", path.display()),
            Error::Engine(message) => write!(f, "engine: {message}"),
            Error::InvalidName(name) => {
                write!(f, "invalid prefix name {name:?}: use letters, digits, spaces, '.', '_' or '-' (at most 64)")
            }
            Error::NoSuchPrefix(name) => write!(f, "no prefix named {name:?}"),
            Error::Setting(message) => write!(f, "settings: {message}"),
            Error::Rules(e) => write!(f, "rules: {e}"),
            Error::Command { what, status, stderr } => {
                write!(f, "{what} failed ({status})")?;
                if !stderr.is_empty() {
                    write!(f, ": {stderr}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::Rules(e) => Some(e),
            _ => None,
        }
    }
}

impl From<compatdb_core::FieldError> for Error {
    fn from(e: compatdb_core::FieldError) -> Self {
        Error::Rules(e)
    }
}
