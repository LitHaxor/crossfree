use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

/// Paths every engine bundle must have, relative to its root.
pub(crate) const REQUIRED: &[&str] = &[
    "bin/wine",
    "bin/wineserver",
    "lib/wine/aarch64-unix/wine.app/Contents/MacOS/wine",
    "lib/wine/aarch64-unix/wine.app/Contents/embedded.provisionprofile",
    "lib/wine/aarch64-unix/compatdb.so",
];

/// The signed loader app, relative to the root. Its signature carries the
/// entitlements Wine needs (4K pages, hardware TSO); embedders must leave it
/// untouched.
const LOADER_APP: &str = "lib/wine/aarch64-unix/wine.app";

/// A Wine engine bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    root: PathBuf,
}

/// Result of checking the engine's loader signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verification {
    /// `codesign --verify --strict` accepted the loader.
    pub signature_valid: bool,
    /// Gatekeeper's assessment source, e.g. "Notarized Developer ID".
    pub gatekeeper_source: Option<String>,
    /// Team identifier of the signing certificate.
    pub team_id: Option<String>,
}

impl Verification {
    /// Whether the loader is validly signed and notarized.
    pub fn is_notarized(&self) -> bool {
        self.signature_valid && self.gatekeeper_source.as_deref() == Some("Notarized Developer ID")
    }
}

impl Engine {
    /// Opens the engine bundle at `root` (the directory holding `bin/` and
    /// `lib/`), checking that it has the expected layout.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let missing: Vec<&str> = REQUIRED.iter().copied().filter(|rel| !root.join(rel).exists()).collect();
        if !missing.is_empty() {
            return Err(Error::Engine(format!(
                "{} is not an engine bundle (missing {})",
                root.display(),
                missing.join(", ")
            )));
        }
        Ok(Self { root })
    }

    /// A mirror of an engine (see [`named_engine`](crate::named_engine)),
    /// which has no `wine.app` on purpose, so [`Engine::open`]'s layout
    /// check does not apply.
    pub(crate) fn mirror(root: PathBuf) -> Self {
        Self { root }
    }

    /// Root directory of the bundle.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `bin/wine`.
    pub fn wine(&self) -> PathBuf {
        self.root.join("bin/wine")
    }

    /// `bin/wineserver`.
    pub fn wineserver(&self) -> PathBuf {
        self.root.join("bin/wineserver")
    }

    /// The signed loader app.
    pub fn loader_app(&self) -> PathBuf {
        self.root.join(LOADER_APP)
    }

    /// Directory of Wine's graphics conformance tests (full bundles only).
    pub fn tests_dir(&self) -> Option<PathBuf> {
        let dir = self.root.join("lib/wine/tests");
        dir.is_dir().then_some(dir)
    }

    /// The Wine version string, from `wine --version`.
    pub fn version(&self) -> Result<String> {
        let output =
            Command::new(self.wine()).arg("--version").output().map_err(|e| Error::io("cannot run", self.wine(), e))?;
        if !output.status.success() {
            return Err(Error::Command {
                what: "wine --version".into(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Checks the loader's code signature and Gatekeeper assessment.
    pub fn verify(&self) -> Result<Verification> {
        let app = self.loader_app();
        let run = |program: &str, args: &[&str]| {
            Command::new(program)
                .args(args)
                .arg(&app)
                .output()
                .map_err(|e| Error::io(format!("cannot run {program} on"), &app, e))
        };
        let strict = run("codesign", &["--verify", "--strict"])?;
        let details = run("codesign", &["-dv"])?;
        let assessment = run("spctl", &["-a", "-vv", "-t", "exec"])?;
        Ok(Verification {
            signature_valid: strict.status.success(),
            team_id: field(&String::from_utf8_lossy(&details.stderr), "TeamIdentifier="),
            gatekeeper_source: if assessment.status.success() {
                field(&String::from_utf8_lossy(&assessment.stderr), "source=")
            } else {
                None
            },
        })
    }
}

/// The value after `prefix` on the first line that starts with it.
fn field(text: &str, prefix: &str) -> Option<String> {
    text.lines().find_map(|line| line.trim().strip_prefix(prefix)).map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn fake_bundle(dir: &Path, skip: Option<&str>) {
        for rel in REQUIRED.iter().filter(|r| Some(**r) != skip) {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }
    }

    #[test]
    fn opens_a_complete_bundle() {
        let dir = TempDir::new("engine-ok");
        fake_bundle(dir.path(), None);
        let engine = Engine::open(dir.path()).unwrap();
        assert_eq!(engine.wine(), dir.path().join("bin/wine"));
        assert_eq!(engine.wineserver(), dir.path().join("bin/wineserver"));
        assert!(engine.loader_app().ends_with("aarch64-unix/wine.app"));
        assert_eq!(engine.tests_dir(), None);
        std::fs::create_dir_all(dir.path().join("lib/wine/tests")).unwrap();
        assert!(engine.tests_dir().is_some());
    }

    #[test]
    fn names_what_is_missing() {
        let dir = TempDir::new("engine-missing");
        fake_bundle(dir.path(), Some("lib/wine/aarch64-unix/compatdb.so"));
        let message = Engine::open(dir.path()).unwrap_err().to_string();
        assert!(message.contains("missing lib/wine/aarch64-unix/compatdb.so"), "{message}");
    }

    #[test]
    fn parses_signature_fields() {
        let details = "Identifier=app.mach9.loader\nTeamIdentifier=WLKUUJD25W\n";
        assert_eq!(field(details, "TeamIdentifier="), Some("WLKUUJD25W".into()));
        let assessment = "/x/wine.app: accepted\nsource=Notarized Developer ID\norigin=...";
        assert_eq!(field(assessment, "source="), Some("Notarized Developer ID".into()));
        assert_eq!(field("nothing", "source="), None);
        let verified = Verification {
            signature_valid: true,
            gatekeeper_source: Some("Notarized Developer ID".into()),
            team_id: None,
        };
        assert!(verified.is_notarized());
        assert!(!Verification { signature_valid: false, ..verified }.is_notarized());
    }
}
