//! Programs named in the Dock.
//!
//! macOS names a running app's Dock tile after its bundle folder. Every
//! Wine process runs the engine's loader, `lib/wine/aarch64-unix/wine.app`,
//! so every game shows as "wine". The loader cannot be renamed or re-signed
//! (its signature carries the entitlements Wine needs), and Wine starts each
//! new process through `wine` next to `ntdll.so`, found from `ntdll.so`'s own
//! real path (`dladdr` and `realpath`).
//!
//! So a name gets a mirror of the engine: links back to the engine for
//! everything, except a copy of `ntdll.so` (so Wine's own folder is the
//! mirror's), the loader bundle copied as `<name>.app` (the same signed
//! files under another folder name) and `wine` pointing into it. There is
//! no `wine.app` beside it: mach9's `ntdll.so` starts processes through
//! `wine.app/Contents/MacOS/wine` when that exists, and through `wine`
//! otherwise. Every process started through the mirror, and every process
//! those start, runs as `<name>.app`, and the Dock says `<name>`. CrossOver gets the same
//! effect with hard links named after each `.exe`, which mach9's loader
//! rejects outside its bundle.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::{Engine, Error, Result};

/// The engine folders on the way to the loader: real folders in a mirror.
const UNIX_DIR: &str = "lib/wine/aarch64-unix";
const LOADER: &str = "lib/wine/aarch64-unix/wine.app/Contents/MacOS/wine";
/// Records what a mirror was made from; a different engine remakes it.
const STAMP: &str = ".crossfree-mirror";
const LAYOUT_VERSION: u32 = 3;

/// Where mirrors are kept: `~/Library/Caches/crossfree/loaders`.
pub fn loader_cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Caches/crossfree/loaders"))
}

/// The engine as a program called `name` would show in the Dock: a mirror
/// of `engine` under `cache` (made or brought up to date here), or `None`
/// when `name` cannot name an app or the engine does not run its loader
/// from `wine.app` (only such an engine can be mirrored).
pub fn named_engine(engine: &Engine, name: &str, cache: &Path) -> Result<Option<Engine>> {
    let Some(bundle) = bundle_name(name) else { return Ok(None) };
    // Absolute: the mirror's links name engine files by their full path.
    let root = &engine.root().canonicalize().map_err(|e| Error::io("cannot find", engine.root(), e))?;
    if !runs_loader_from_bundle(root) {
        return Ok(None);
    }
    let stamp = stamp(root)?;
    let dir = cache.join(format!("{:016x}", fnv1a(root.as_os_str().as_bytes()))).join(&bundle);
    if is_current(&dir, &stamp) {
        return Ok(Some(Engine::mirror(dir)));
    }

    // Built beside it, then moved into place: a launch that comes at the
    // same time sees either the old mirror or the new one, never half.
    std::fs::create_dir_all(dir.parent().unwrap_or(cache)).map_err(|e| Error::io("cannot create", cache, e))?;
    let building = dir.with_file_name(format!(".{bundle}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_dir_all(&building);
    let built = build(root, &building, &bundle).and_then(|()| std::fs::write(building.join(STAMP), &stamp));
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&building);
        return Err(Error::io("cannot mirror the engine into", building, e));
    }
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::rename(&building, &dir) {
        let _ = std::fs::remove_dir_all(&building);
        // Another launch moved its mirror in first.
        if !is_current(&dir, &stamp) {
            return Err(Error::io("cannot move the engine mirror to", dir, e));
        }
    }
    Ok(Some(Engine::mirror(dir)))
}

/// `name` as a bundle folder name: no `/` or `:`, no leading dot, spaces
/// collapsed, at most 64 characters. `None` for an empty name, and for
/// "wine", which the engine's own bundle already has.
pub(crate) fn bundle_name(name: &str) -> Option<String> {
    let spaced: String = name.chars().map(|c| if c == '/' || c == ':' || c.is_control() { ' ' } else { c }).collect();
    let words = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed: String = words.trim_start_matches('.').trim().chars().take(64).collect();
    let trimmed = trimmed.trim_end().to_owned();
    (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("wine")).then_some(trimmed)
}

/// Whether `bin/wine` is the loader inside `wine.app` (mach9's layout).
fn runs_loader_from_bundle(root: &Path) -> bool {
    match (root.join("bin/wine").canonicalize(), root.join(LOADER).canonicalize()) {
        (Ok(wine), Ok(loader)) => wine == loader,
        _ => false,
    }
}

/// The engine's identity for a mirror: its folder, and the size and time of
/// the files a mirror copies.
fn stamp(root: &Path) -> Result<String> {
    let mut stamp = format!("crossfree engine mirror {LAYOUT_VERSION}\n{}\n", root.display());
    for rel in [format!("{UNIX_DIR}/ntdll.so"), LOADER.to_owned()] {
        let path = root.join(&rel);
        let meta = std::fs::metadata(&path).map_err(|e| Error::io("cannot read", &path, e))?;
        let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        stamp.push_str(&format!("{rel} {} {modified}\n", meta.len()));
    }
    Ok(stamp)
}

fn is_current(dir: &Path, stamp: &str) -> bool {
    std::fs::read_to_string(dir.join(STAMP)).is_ok_and(|s| s == stamp)
}

/// Lays out the mirror of `root` at `mirror` for `<bundle>.app`.
fn build(root: &Path, mirror: &Path, bundle: &str) -> std::io::Result<()> {
    // The folders from the top down to the loader's are real; whatever else
    // they hold links back to the engine.
    let real = ["", "bin", "lib", "lib/wine", UNIX_DIR];
    for rel in real {
        std::fs::create_dir_all(mirror.join(rel))?;
    }
    for rel in real {
        for entry in std::fs::read_dir(root.join(rel))? {
            let name = entry?.file_name();
            let inner = Path::new(rel).join(&name);
            let own = real.iter().any(|r| Path::new(r) == inner)
                || inner == Path::new("bin/wine")
                || inner == Path::new(UNIX_DIR).join("wine")
                || inner == Path::new(UNIX_DIR).join("wine.app")
                || inner == Path::new(UNIX_DIR).join("ntdll.so");
            if !own {
                std::os::unix::fs::symlink(root.join(&inner), mirror.join(&inner))?;
            }
        }
    }
    let unix = Path::new(UNIX_DIR);
    // Wine's own folder is where ntdll.so really is: this copy makes it the
    // mirror's, so Wine starts its processes through the mirror's `wine`.
    std::fs::copy(root.join(unix).join("ntdll.so"), mirror.join(unix).join("ntdll.so"))?;
    copy_tree(&root.join(unix).join("wine.app"), &mirror.join(unix).join(format!("{bundle}.app")))?;
    let loader = format!("{bundle}.app/Contents/MacOS/wine");
    std::os::unix::fs::symlink(&loader, mirror.join(unix).join("wine"))?;
    std::os::unix::fs::symlink(format!("../{UNIX_DIR}/{loader}"), mirror.join("bin/wine"))
}

/// Copies a folder, its links as links. Files are copies, never hard links:
/// macOS may report a hard-linked program by its other name, which would
/// put the Dock tile back on `wine.app`. On APFS `std::fs::copy` clones, so
/// a copy takes no space.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&source)?, &target)?;
        } else {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// 64-bit FNV-1a: the engine folder's key under the cache.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::REQUIRED;
    use crate::test_support::TempDir;
    use std::os::unix::fs::MetadataExt;

    /// An engine in mach9's layout: `bin/wine` and `aarch64-unix/wine` link
    /// to the loader in `wine.app`, beside other files and folders.
    fn mach9_engine(dir: &Path) -> Engine {
        for rel in REQUIRED.iter().filter(|r| !r.ends_with("/wine") || r.contains("MacOS")) {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, rel.as_bytes()).unwrap();
        }
        for (rel, text) in [
            ("lib/wine/aarch64-unix/ntdll.so", "ntdll"),
            ("lib/wine/aarch64-unix/winemac.so", "winemac"),
            ("lib/wine/aarch64-unix/wine.app/Contents/Info.plist", "plist"),
            ("lib/wine/aarch64-windows/kernel32.dll", "pe"),
            ("lib/libfoo.dylib", "foo"),
            ("share/wine/wine.inf", "inf"),
        ] {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        std::os::unix::fs::symlink("wine.app/Contents/MacOS/wine", dir.join(UNIX_DIR).join("wine")).unwrap();
        let _ = std::fs::remove_file(dir.join("bin/wine"));
        std::os::unix::fs::symlink(format!("../{LOADER}"), dir.join("bin/wine")).unwrap();
        Engine::open(dir).unwrap()
    }

    #[test]
    fn names_become_bundle_names() {
        assert_eq!(bundle_name("Steam").as_deref(), Some("Steam"));
        assert_eq!(
            bundle_name("  Rise of Nations:  Extended/Edition ").as_deref(),
            Some("Rise of Nations Extended Edition")
        );
        assert_eq!(bundle_name("..hidden").as_deref(), Some("hidden"));
        assert_eq!(bundle_name(&"x".repeat(100)).map(|n| n.len()), Some(64));
        for nothing in ["", "  ", "/", ".", "wine", "WINE"] {
            assert_eq!(bundle_name(nothing), None, "{nothing:?}");
        }
    }

    #[test]
    fn mirrors_the_engine_for_a_name() {
        let dir = TempDir::new("dock-mirror");
        let engine = mach9_engine(&dir.path().join("engine"));
        let cache = dir.path().join("cache");

        let named = named_engine(&engine, "Steam", &cache).unwrap().expect("a mirror");
        let m = named.root().to_owned();
        assert!(m.starts_with(&cache) && m.ends_with("Steam"));
        let app = m.join(UNIX_DIR).join("Steam.app/Contents/MacOS/wine");
        // Every way into Wine leads to the renamed bundle.
        assert_eq!(named.wine().canonicalize().unwrap(), app.canonicalize().unwrap());
        assert_eq!(m.join(UNIX_DIR).join("wine").canonicalize().unwrap(), app.canonicalize().unwrap());
        // The bundle and ntdll.so are the engine's files, but in the mirror.
        let unix = engine.root().join(UNIX_DIR);
        assert_eq!(std::fs::read(&app).unwrap(), std::fs::read(engine.root().join(LOADER)).unwrap());
        // A file of its own, not a second name for the engine's: macOS may
        // report a hard-linked program by either name.
        let inode = |p: &Path| std::fs::metadata(p).unwrap().ino();
        assert_ne!(inode(&app), inode(&engine.root().join(LOADER)));
        assert_eq!(std::fs::read(m.join(UNIX_DIR).join("Steam.app/Contents/Info.plist")).unwrap(), b"plist");
        let ntdll = m.join(UNIX_DIR).join("ntdll.so");
        assert!(!std::fs::symlink_metadata(&ntdll).unwrap().file_type().is_symlink());
        assert_eq!(ntdll.canonicalize().unwrap().parent().unwrap(), m.join(UNIX_DIR).canonicalize().unwrap());
        // Everything else links back to the engine.
        for rel in [
            "lib/wine/aarch64-unix/winemac.so",
            "lib/wine/aarch64-windows",
            "lib/libfoo.dylib",
            "share",
            "bin/wineserver",
        ] {
            assert!(std::fs::symlink_metadata(m.join(rel)).unwrap().file_type().is_symlink(), "{rel}");
            assert_eq!(m.join(rel).canonicalize().unwrap(), engine.root().join(rel).canonicalize().unwrap(), "{rel}");
        }
        // No wine.app: Wine would start processes through it, as "wine".
        assert!(std::fs::symlink_metadata(m.join(UNIX_DIR).join("wine.app")).is_err());

        // The same name reuses the mirror; a changed engine remakes it.
        let before = inode(&m.join(STAMP));
        named_engine(&engine, "Steam", &cache).unwrap().unwrap();
        assert_eq!(inode(&m.join(STAMP)), before);
        std::fs::write(unix.join("ntdll.so"), "a newer ntdll").unwrap();
        named_engine(&engine, "Steam", &cache).unwrap().unwrap();
        assert_eq!(std::fs::read(&ntdll).unwrap(), b"a newer ntdll");
        // Another name, another mirror; nothing left half-built.
        let other = named_engine(&engine, "Epic Games", &cache).unwrap().unwrap();
        assert!(other.root().join(UNIX_DIR).join("Epic Games.app").is_dir());
        let leftovers: Vec<_> = std::fs::read_dir(m.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn only_a_bundled_loader_and_a_real_name_get_a_mirror() {
        let dir = TempDir::new("dock-none");
        let cache = dir.path().join("cache");
        let engine = mach9_engine(&dir.path().join("engine"));
        assert!(named_engine(&engine, "wine", &cache).unwrap().is_none());
        assert!(named_engine(&engine, " / ", &cache).unwrap().is_none());
        // A `bin/wine` of its own (a script, another layout): left alone.
        let (_plain_dir, plain) = crate::test_support::fake_engine("dock-plain");
        assert!(named_engine(&plain, "Steam", &cache).unwrap().is_none());
        assert!(!cache.exists());
    }
}
