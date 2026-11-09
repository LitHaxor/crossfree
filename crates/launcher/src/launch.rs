use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use compatdb_core::{ENV_VAR, Rule, Table, inherited_conflicts};

use crate::{BottleSettings, Engine, Error, Result};

/// One start of a Windows program in a prefix.
///
/// The environment follows the engine's launcher contract: variables the
/// rules replace are removed even when the launcher inherited them, the
/// rules go in `WINE_COMPATDB` written by the rules crate, and `WINEDEBUG`
/// is crossfree's entries followed by the rules' debug channels.
#[derive(Clone, Debug)]
pub struct Launch<'a> {
    engine: &'a Engine,
    prefix: PathBuf,
    program: OsString,
    args: Vec<OsString>,
    rules: Table,
    bottle_rule: Option<Rule>,
    session_env: Vec<(&'static str, &'static str)>,
    debug: Vec<String>,
    current_dir: Option<PathBuf>,
}

/// Where a program given as a Unix path runs from, as Explorer starts a
/// double-clicked program in its own folder: `Some((program, folder))` with
/// the program's absolute path (a relative one resolved against the current
/// directory) and the folder that holds it, when `program` holds a `/` and
/// that folder exists. `None` for a Windows path (`C:\Games\Game.exe`) or a
/// builtin (`cmd`), which keep the launcher's directory.
pub fn program_folder(program: &OsStr) -> Option<(PathBuf, PathBuf)> {
    if !program.as_encoded_bytes().contains(&b'/') {
        return None;
    }
    let program = std::path::absolute(Path::new(program)).ok()?;
    let folder = program.parent().filter(|dir| dir.is_dir())?.to_owned();
    Some((program, folder))
}

impl<'a> Launch<'a> {
    /// Runs `program` (a Windows or Unix path, or a builtin like `cmd`) in
    /// the prefix at `prefix`.
    pub fn new(engine: &'a Engine, prefix: impl Into<PathBuf>, program: impl Into<OsString>) -> Self {
        Self {
            engine,
            prefix: prefix.into(),
            program: program.into(),
            args: Vec::new(),
            rules: Table::new(),
            bottle_rule: None,
            session_env: Vec::new(),
            debug: vec!["-all".into()],
            current_dir: None,
        }
    }

    /// Starts Wine in `dir`, which becomes the Windows program's current
    /// directory (by default the launcher's own; see [`program_folder`]).
    pub fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(dir.into());
        self
    }

    /// Adds arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Per-process rules for the whole process tree.
    pub fn rules(mut self, rules: Table) -> Self {
        self.rules = rules;
        self
    }

    /// The bottle's settings, for every program; per-program rules from
    /// [`Launch::rules`] override them.
    pub fn settings(mut self, settings: &BottleSettings) -> Result<Self> {
        self.bottle_rule = settings.rule()?;
        self.session_env = settings.session_env();
        Ok(self)
    }

    /// crossfree's own `WINEDEBUG` entries (default `-all`).
    pub fn debug_channels(mut self, entries: Vec<String>) -> Self {
        self.debug = entries;
        self
    }

    /// The command to run.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(self.engine.wine());
        cmd.arg(&self.program).args(&self.args);
        if let Some(dir) = &self.current_dir {
            cmd.current_dir(dir);
        }
        for name in inherited_conflicts() {
            cmd.env_remove(name);
        }
        cmd.env("WINEPREFIX", &self.prefix);
        for (name, value) in &self.session_env {
            cmd.env(name, value);
        }
        // Bottle-wide rule first; the more specific per-program rules win.
        let mut table = Table::new();
        if let Some(rule) = &self.bottle_rule {
            table.push(rule.clone());
        }
        table.append(self.rules.clone());
        let mut debug = self.debug.clone();
        debug.extend(table.winedebug());
        cmd.env("WINEDEBUG", debug.join(","));
        // WINE_COMPATDB is not among the inherited conflicts (launchers are
        // expected to set it), so remove an inherited one when there are no
        // rules: stray rules must never reach Wine.
        let rules = table.to_string();
        if rules.is_empty() {
            cmd.env_remove(ENV_VAR);
        } else {
            cmd.env(ENV_VAR, rules);
        }
        cmd
    }

    /// Runs to completion and captures the output.
    pub fn output(&self) -> Result<Output> {
        self.command().output().map_err(|e| Error::io("cannot run", self.engine.wine(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, fake_engine};
    use crate::{D3d9Backend, GameProfile, rules};

    fn env_of(cmd: &Command, name: &str) -> Option<Option<String>> {
        cmd.get_envs().find(|(k, _)| *k == name).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn builds_the_engine_environment() {
        let (_dir, engine) = fake_engine("launch-env");
        let mut profile = GameProfile::new("Game.exe");
        profile.d3d9 = Some(D3d9Backend::Wined3d);
        let cmd = Launch::new(&engine, "/prefixes/main", "C:\\game\\Game.exe")
            .args(["-fullscreen"])
            .rules(rules(&[profile]).unwrap())
            .command();

        assert_eq!(cmd.get_program(), engine.wine().as_os_str());
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["C:\\game\\Game.exe", "-fullscreen"]);
        assert_eq!(env_of(&cmd, "WINEPREFIX"), Some(Some("/prefixes/main".into())));
        assert_eq!(env_of(&cmd, "WINEDEBUG"), Some(Some("-all".into())));
        assert_eq!(env_of(&cmd, ENV_VAR), Some(Some("exe=Game.exe;d3d9=wined3d".into())));
        // Inherited variables the rules replace are removed.
        assert_eq!(env_of(&cmd, "WINEDLLOVERRIDES"), Some(None));
    }

    #[test]
    fn bottle_settings_come_first_and_set_the_session() {
        let (_dir, engine) = fake_engine("launch-bottle");
        let mut bottle = BottleSettings::default();
        bottle.set("d3d9", "mtld3d").unwrap();
        bottle.set("msync", "off").unwrap();
        let mut game = GameProfile::new("Game.exe");
        game.d3d9 = Some(D3d9Backend::Wined3d);
        let cmd =
            Launch::new(&engine, "/p", "Game.exe").settings(&bottle).unwrap().rules(rules(&[game]).unwrap()).command();
        assert_eq!(env_of(&cmd, ENV_VAR), Some(Some("exe=*;d3d9=mtld3d\nexe=Game.exe;d3d9=wined3d".into())));
        assert_eq!(env_of(&cmd, "WINEMSYNC"), Some(Some("0".into())));
    }

    #[test]
    fn no_rules_means_no_rules_variable() {
        let (_dir, engine) = fake_engine("launch-norules");
        let cmd = Launch::new(&engine, "/p", "cmd").debug_channels(vec!["-all".into(), "+loaddll".into()]).command();
        assert_eq!(env_of(&cmd, ENV_VAR), Some(None));
        assert_eq!(env_of(&cmd, "WINEDEBUG"), Some(Some("-all,+loaddll".into())));
    }

    #[test]
    fn working_directory_is_the_launchers_unless_set() {
        let (dir, engine) = fake_engine("launch-cwd");
        assert_eq!(Launch::new(&engine, "/p", "cmd").command().get_current_dir(), None);
        let cmd = Launch::new(&engine, "/p", "Game.exe").current_dir(dir.path()).command();
        assert_eq!(cmd.get_current_dir(), Some(dir.path()));
    }

    #[test]
    fn unix_programs_run_from_their_folder() {
        let dir = TempDir::new("program-folder");
        let games = dir.path().join("Some Game");
        std::fs::create_dir_all(&games).unwrap();
        let game = games.join("Game.exe");
        assert_eq!(program_folder(game.as_os_str()), Some((game.clone(), games.clone())));

        // A relative path becomes absolute, since Wine then starts elsewhere.
        // Cargo runs unit tests in the package's directory.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            program_folder(OsStr::new("src/main.rs")),
            Some((manifest.join("src/main.rs"), manifest.join("src")))
        );

        // Windows paths, builtins and missing folders keep the launcher's.
        for program in [r"C:\Games\Game.exe", "cmd", "Game.exe"] {
            assert_eq!(program_folder(OsStr::new(program)), None, "{program}");
        }
        assert_eq!(program_folder(games.join("missing/Game.exe").as_os_str()), None);
    }
}
