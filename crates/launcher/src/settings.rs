use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use compatdb_core::{Exe, Key, Rule};

use crate::{D3d9Backend, DxgiBackend, Error, Result};

/// Settings file inside each prefix.
pub const SETTINGS_FILE: &str = "crossfree.conf";

/// Lock file inside each prefix, held while [`BottleSettings::update`] reads,
/// changes and writes [`SETTINGS_FILE`]. It is never removed: removing a lock
/// file lets a waiting process lock a file nobody else sees.
pub const LOCK_FILE: &str = ".crossfree.conf.lock";

/// Writes `bytes` to a new file at `path` and flushes it to disk, so a rename
/// over the real file never leaves an empty one after a crash.
fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Per-bottle settings, applied to every program in the prefix. A program's
/// own [`crate::GameProfile`] overrides them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BottleSettings {
    /// D3D9 implementation; `None` is the engine's default (Auto).
    pub d3d9: Option<D3d9Backend>,
    /// D3D10–12 implementation; `None` is the engine's default (Auto).
    pub dxgi: Option<DxgiBackend>,
    /// Wine synchronisation on Mach semaphores. Takes effect when the
    /// bottle's Wine session starts.
    pub msync: bool,
    /// Metal performance HUD.
    pub metal_hud: bool,
}

impl Default for BottleSettings {
    fn default() -> Self {
        Self { d3d9: None, dxgi: None, msync: true, metal_hud: false }
    }
}

fn on_off(value: &str) -> Option<bool> {
    match value {
        "on" | "true" => Some(true),
        "off" | "false" => Some(false),
        _ => None,
    }
}

fn word(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

impl BottleSettings {
    /// Reads the prefix's settings file; a missing file means defaults.
    pub fn load(prefix: &Path) -> Result<Self> {
        let path = prefix.join(SETTINGS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io("cannot read", &path, e)),
        }
    }

    /// Writes the settings file atomically: a temporary file of this save
    /// alone, flushed to disk, then renamed over the file. Concurrent saves
    /// therefore never see each other's temporary file; use
    /// [`BottleSettings::update`] so that they also keep each other's
    /// changes.
    pub fn save(&self, prefix: &Path) -> Result<()> {
        static SAVES: AtomicU64 = AtomicU64::new(0);
        let path = prefix.join(SETTINGS_FILE);
        let n = SAVES.fetch_add(1, Ordering::Relaxed);
        let temp = prefix.join(format!(".{SETTINGS_FILE}.{}.{n}.tmp", std::process::id()));
        let written = write_synced(&temp, self.to_text().as_bytes())
            .map_err(|e| Error::io("cannot write", &temp, e))
            .and_then(|()| std::fs::rename(&temp, &path).map_err(|e| Error::io("cannot write", &path, e)));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written
    }

    /// Changes the prefix's settings: loads them, applies `change` and saves
    /// the result, holding a lock on [`LOCK_FILE`] throughout so that
    /// concurrent changes, from threads or from several `crossfree settings`
    /// at once, all apply. Nothing is saved when `change` fails. Returns the
    /// settings as saved.
    pub fn update(prefix: &Path, change: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        let lock_path = prefix.join(LOCK_FILE);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| Error::io("cannot open", &lock_path, e))?;
        lock.lock().map_err(|e| Error::io("cannot lock", &lock_path, e))?;
        let mut settings = Self::load(prefix)?;
        change(&mut settings)?;
        settings.save(prefix)?;
        Ok(settings)
        // Dropping `lock` releases it.
    }

    /// Parses `key=value` lines; `#` starts a comment. Unknown keys are
    /// ignored so newer files still load.
    pub fn parse(text: &str) -> Result<Self> {
        let mut settings = Self::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').ok_or_else(|| Error::Setting(format!("not key=value: {line}")))?;
            match settings.set(key.trim(), value.trim()) {
                Err(Error::Setting(message)) if message.starts_with("unknown setting") => {}
                other => other?,
            }
        }
        Ok(settings)
    }

    /// The settings file's text.
    pub fn to_text(&self) -> String {
        format!(
            "# crossfree bottle settings\nd3d9={}\ndxgi={}\nmsync={}\nmetal_hud={}\n",
            self.d3d9.map_or("auto", D3d9Backend::name),
            self.dxgi.map_or("auto", DxgiBackend::name),
            word(self.msync),
            word(self.metal_hud),
        )
    }

    /// Sets one setting: `d3d9` (auto, mtld3d, wined3d), `dxgi` (auto, dxmt,
    /// wined3d), `msync` or `metal_hud` (on, off).
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let bad = || Error::Setting(format!("invalid value {value:?} for {key}"));
        match key {
            "d3d9" => {
                self.d3d9 = match value {
                    "auto" => None,
                    name => Some(D3d9Backend::from_name(name).ok_or_else(bad)?),
                }
            }
            "dxgi" => {
                self.dxgi = match value {
                    "auto" => None,
                    name => Some(DxgiBackend::from_name(name).ok_or_else(bad)?),
                }
            }
            "msync" => self.msync = on_off(value).ok_or_else(bad)?,
            "metal_hud" => self.metal_hud = on_off(value).ok_or_else(bad)?,
            _ => return Err(Error::Setting(format!("unknown setting {key}"))),
        }
        Ok(())
    }

    /// The engine rule for every program in the bottle, or `None` if the
    /// settings leave everything to the engine. Uses [`Rule::set`] so the
    /// engine's checks for whole-bottle rules apply.
    pub fn rule(&self) -> Result<Option<Rule>> {
        let mut rule = Rule::new(Exe::Any);
        if let Some(d3d9) = self.d3d9 {
            rule.set(Key::D3d9, d3d9.name())?;
        }
        if let Some(dxgi) = self.dxgi {
            rule.set(Key::Dxgi, dxgi.name())?;
        }
        if self.metal_hud {
            rule.set(Key::MetalHud, "true")?;
        }
        Ok((!rule.sets_nothing()).then_some(rule))
    }

    /// Environment for the bottle's Wine session. Only the first process of
    /// a session (the one that starts the wineserver) reads `WINEMSYNC`.
    pub fn session_env(&self) -> Vec<(&'static str, &'static str)> {
        if self.msync { Vec::new() } else { vec![("WINEMSYNC", "0")] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn defaults_and_round_trip() {
        let defaults = BottleSettings::default();
        assert_eq!(BottleSettings::parse("").unwrap(), defaults);
        let settings = BottleSettings {
            d3d9: Some(D3d9Backend::Wined3d),
            dxgi: Some(DxgiBackend::Dxmt),
            msync: false,
            metal_hud: true,
        };
        assert_eq!(BottleSettings::parse(&settings.to_text()).unwrap(), settings);
        assert!(defaults.to_text().contains("d3d9=auto\ndxgi=auto\nmsync=on\nmetal_hud=off"));
    }

    #[test]
    fn parsing_is_strict_about_values_lenient_about_keys() {
        let s = BottleSettings::parse("# c\n d3d9 = wined3d \nfuture_key=1\n").unwrap();
        assert_eq!(s.d3d9, Some(D3d9Backend::Wined3d));
        assert!(BottleSettings::parse("d3d9=gptk").is_err());
        assert!(BottleSettings::parse("msync=maybe").is_err());
        assert!(BottleSettings::parse("no equals sign").is_err());
        let mut s = BottleSettings::default();
        assert!(matches!(s.set("bogus", "1"), Err(Error::Setting(_))));
        s.set("dxgi", "auto").unwrap();
        assert_eq!(s.dxgi, None);
    }

    #[test]
    fn load_and_save() {
        let dir = TempDir::new("settings");
        assert_eq!(BottleSettings::load(dir.path()).unwrap(), BottleSettings::default());
        let mut s = BottleSettings::default();
        s.set("metal_hud", "on").unwrap();
        s.save(dir.path()).unwrap();
        s.save(dir.path()).unwrap();
        assert_eq!(BottleSettings::load(dir.path()).unwrap(), s);
        assert_eq!(entries(dir.path()), [SETTINGS_FILE]);
    }

    /// The names in `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    #[test]
    fn update_changes_and_saves_or_saves_nothing() {
        let dir = TempDir::new("settings-update");
        let saved = BottleSettings::update(dir.path(), |s| s.set("dxgi", "wined3d")).unwrap();
        assert_eq!(saved.dxgi, Some(DxgiBackend::Wined3d));
        assert_eq!(BottleSettings::load(dir.path()).unwrap(), saved);

        let before = std::fs::read(dir.path().join(SETTINGS_FILE)).unwrap();
        let failed = BottleSettings::update(dir.path(), |s| {
            s.set("msync", "off")?;
            s.set("d3d9", "gptk")
        });
        assert!(matches!(failed, Err(Error::Setting(_))));
        assert_eq!(std::fs::read(dir.path().join(SETTINGS_FILE)).unwrap(), before);
        assert_eq!(entries(dir.path()), [LOCK_FILE, SETTINGS_FILE]);
    }

    /// Four changes to different settings at once, many times: every change
    /// must survive (the lock), and none may fail on another's temporary
    /// file (unique names).
    #[test]
    fn concurrent_updates_all_apply() {
        let dir = TempDir::new("settings-concurrent");
        let changes = [("d3d9", "wined3d"), ("dxgi", "dxmt"), ("msync", "off"), ("metal_hud", "on")];
        let expected = BottleSettings {
            d3d9: Some(D3d9Backend::Wined3d),
            dxgi: Some(DxgiBackend::Dxmt),
            msync: false,
            metal_hud: true,
        };
        let path = dir.path();
        for _ in 0..50 {
            BottleSettings::default().save(path).unwrap();
            std::thread::scope(|scope| {
                for (key, value) in changes {
                    scope.spawn(move || BottleSettings::update(path, |s| s.set(key, value)).unwrap());
                }
            });
            assert_eq!(BottleSettings::load(path).unwrap(), expected);
        }
        assert_eq!(entries(dir.path()), [LOCK_FILE, SETTINGS_FILE]);
    }

    #[test]
    fn becomes_a_rule_for_every_program() {
        assert!(BottleSettings::default().rule().unwrap().is_none());
        let mut s = BottleSettings::default();
        s.set("d3d9", "wined3d").unwrap();
        s.set("metal_hud", "on").unwrap();
        let mut table = compatdb_core::Table::new();
        table.push(s.rule().unwrap().unwrap());
        assert_eq!(table.to_string(), "exe=*;d3d9=wined3d;metal_hud=true");
    }

    #[test]
    fn msync_off_sets_the_session_variable() {
        assert!(BottleSettings::default().session_env().is_empty());
        let mut s = BottleSettings::default();
        s.set("msync", "off").unwrap();
        assert_eq!(s.session_env(), [("WINEMSYNC", "0")]);
    }
}
