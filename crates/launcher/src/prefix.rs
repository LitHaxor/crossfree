use std::path::{Path, PathBuf};
use std::process::Command;

use compatdb_core::inherited_conflicts;

use crate::{Engine, Error, Launch, Result};

/// The Wine prefixes crossfree manages, one directory each under a root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefixes {
    root: PathBuf,
}

/// Checks a prefix name: 1–64 letters, digits, spaces, `.`, `_` or `-`,
/// not starting with `.` or a space.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', ' '])
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'));
    if ok { Ok(()) } else { Err(Error::InvalidName(name.to_owned())) }
}

fn is_prefix(path: &Path) -> bool {
    path.join("drive_c").is_dir() || path.join("system.reg").is_file()
}

impl Prefixes {
    /// Prefixes under `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `~/Library/Application Support/crossfree/prefixes`.
    pub fn default_root() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").ok_or_else(|| Error::Engine("HOME is not set".into()))?;
        Ok(PathBuf::from(home).join("Library/Application Support/crossfree/prefixes"))
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory of the prefix `name` (which need not exist).
    pub fn path(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.root.join(name))
    }

    /// Names of the existing prefixes, sorted.
    pub fn list(&self) -> Result<Vec<String>> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io("cannot list", &self.root, e)),
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| is_prefix(&e.path()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| validate_name(n).is_ok())
            .collect();
        names.sort();
        Ok(names)
    }

    /// Creates and initialises the prefix `name` (runs `wineboot --init`).
    pub fn create(&self, engine: &Engine, name: &str) -> Result<PathBuf> {
        let path = self.path(name)?;
        std::fs::create_dir_all(&path).map_err(|e| Error::io("cannot create", &path, e))?;
        let output = Launch::new(engine, &path, "wineboot").args(["--init"]).output()?;
        if !output.status.success() || !is_prefix(&path) {
            return Err(Error::Command {
                what: format!("wineboot --init in {}", path.display()),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(path)
    }

    /// Directory of an existing prefix.
    pub fn existing(&self, name: &str) -> Result<PathBuf> {
        let path = self.path(name)?;
        if is_prefix(&path) { Ok(path) } else { Err(Error::NoSuchPrefix(name.to_owned())) }
    }

    /// Quits every Windows program in the prefix (stops its wineserver).
    pub fn kill(&self, engine: &Engine, name: &str) -> Result<()> {
        let path = self.existing(name)?;
        let mut kill = Command::new(engine.wineserver());
        for var in inherited_conflicts() {
            kill.env_remove(var);
        }
        // A prefix without a running wineserver makes `-k` fail; that is fine.
        let _ = kill.arg("-k").env("WINEPREFIX", &path).status();
        Ok(())
    }

    /// Stops the prefix's Wine processes and deletes it.
    pub fn remove(&self, engine: &Engine, name: &str) -> Result<()> {
        self.kill(engine, name)?;
        let path = self.existing(name)?;
        std::fs::remove_dir_all(&path).map_err(|e| Error::io("cannot remove", &path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, fake_engine};

    #[test]
    fn names() {
        for ok in ["main", "Rise of Nations", "steam-1", "a.b_c"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".hidden", " lead", "a/b", "..", "a\\b", "é", &"x".repeat(65)] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn paths_stay_under_the_root() {
        let prefixes = Prefixes::new("/data/prefixes");
        assert_eq!(prefixes.path("main").unwrap(), PathBuf::from("/data/prefixes/main"));
        assert!(prefixes.path("../escape").is_err());
    }

    #[test]
    fn lists_only_prefixes() {
        let dir = TempDir::new("prefix-list");
        let prefixes = Prefixes::new(dir.path());
        assert!(prefixes.list().unwrap().is_empty());
        std::fs::create_dir_all(dir.path().join("b/drive_c")).unwrap();
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/system.reg"), b"").unwrap();
        std::fs::create_dir_all(dir.path().join("not-a-prefix")).unwrap();
        assert_eq!(prefixes.list().unwrap(), ["a", "b"]);
        assert!(Prefixes::new(dir.path().join("missing")).list().unwrap().is_empty());
    }

    #[test]
    fn removes_only_prefixes() {
        let (_engine_dir, engine) = fake_engine("prefix-remove");
        let dir = TempDir::new("prefix-remove-root");
        let prefixes = Prefixes::new(dir.path());
        std::fs::create_dir_all(dir.path().join("plain")).unwrap();
        assert!(matches!(prefixes.remove(&engine, "plain"), Err(Error::NoSuchPrefix(_))));
        assert!(dir.path().join("plain").exists());
        std::fs::create_dir_all(dir.path().join("game/drive_c")).unwrap();
        prefixes.remove(&engine, "game").unwrap();
        assert!(!dir.path().join("game").exists());
    }
}
