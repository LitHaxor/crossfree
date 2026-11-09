//! Helpers for unit tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::Engine;
use crate::engine::REQUIRED;

/// A directory under the system temp directory, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("crossfree-test-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An engine bundle with the right layout and empty files.
pub fn fake_engine(name: &str) -> (TempDir, Engine) {
    let dir = TempDir::new(name);
    for rel in REQUIRED {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().expect("has parent")).expect("create dirs");
        std::fs::write(path, b"").expect("write file");
    }
    let engine = Engine::open(dir.path()).expect("fake engine opens");
    (dir, engine)
}
