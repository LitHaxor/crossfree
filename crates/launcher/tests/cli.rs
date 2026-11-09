//! The `crossfree` command line as the macOS app drives it, against a fake
//! engine: `bin/wine` is a shell script that records how it was started
//! (arguments, working directory) and writes a line to stdout and stderr.
//! No Wine runs, so these tests need no engine bundle.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_crossfree");

const FAKE_WINE: &str = r#"#!/bin/sh
# Records one start: the working directory, then each argument.
{ echo "cwd=$(pwd -P)"; for a in "$@"; do echo "arg=$a"; done; } >> "$FAKE_RECORD"
if [ "$1" = "--version" ]; then echo "wine-fake-1.0"; exit 0; fi
echo "wine stdout"
echo "wine stderr" >&2
exit "${FAKE_EXIT:-0}"
"#;

/// A scratch directory with a fake engine and a prefix root holding the
/// bottle `b`; removed on drop.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let engine = dir.join("engine");
        for (rel, text) in [
            ("bin/wine", FAKE_WINE),
            ("bin/wineserver", "#!/bin/sh\nexit 0\n"),
            ("lib/wine/aarch64-unix/wine.app/Contents/MacOS/wine", ""),
            ("lib/wine/aarch64-unix/wine.app/Contents/embedded.provisionprofile", ""),
            ("lib/wine/aarch64-unix/compatdb.so", ""),
        ] {
            let path = engine.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            if rel.starts_with("bin/") {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        std::fs::create_dir_all(dir.join("prefixes/b/drive_c")).unwrap();
        // Canonical, since the fake wine records `pwd -P`.
        Self { dir: dir.canonicalize().unwrap() }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// `crossfree --engine … --prefixes … ARGS`, recording Wine's starts.
    fn crossfree(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.arg("--engine")
            .arg(self.path("engine"))
            .arg("--prefixes")
            .arg(self.path("prefixes"))
            .args(args)
            .env("FAKE_RECORD", self.path("record"))
            .env_remove("FAKE_EXIT");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.crossfree(args).output().unwrap()
    }

    /// The recorded starts of Wine, one `cwd=` or `arg=` line each, cleared.
    fn starts(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.path("record")).unwrap_or_default();
        let _ = std::fs::remove_file(self.path("record"));
        text.lines().map(str::to_owned).collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The app reads the JSON report whatever it says; only the text report
/// fails for a loader that is not notarized (the fake one is not even
/// signed).
#[test]
fn json_engine_report_is_not_an_error() {
    let f = Fixture::new("engine");
    let out = f.run(&["--json", "engine"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let report = text(&out.stdout);
    assert!(report.starts_with(r#"{"root":"#), "{report}");
    assert!(report.contains(r#""version":"wine-fake-1.0""#), "{report}");
    assert!(report.contains(r#""signature_valid":false"#), "{report}");
    assert!(report.contains(r#""notarized":false"#), "{report}");

    let out = f.run(&["engine"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stdout).contains("signature:  INVALID"));
}

#[test]
fn programs_start_in_their_folder() {
    let f = Fixture::new("cwd");
    let games = f.path("Some Game");
    std::fs::create_dir_all(&games).unwrap();
    let game = games.join("Game.exe");
    let game = game.to_str().unwrap();

    let out = f.run(&["run", "b", "--", game, "-windowed"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(f.starts(), [format!("cwd={}", games.display()), format!("arg={game}"), "arg=-windowed".into()]);

    // A relative path is made absolute, since Wine starts elsewhere.
    let out = f.crossfree(&["run", "b", "--", "Some Game/Game.exe"]).current_dir(&f.dir).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(f.starts(), [format!("cwd={}", games.display()), format!("arg={game}")]);

    let out = f.run(&["start", "b", game]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        f.starts(),
        [format!("cwd={}", games.display()), "arg=start".into(), "arg=/unix".into(), format!("arg={game}")]
    );
}

/// A shortcut carries its own working directory, and a Windows path or a
/// builtin has no Unix folder: those keep crossfree's.
#[test]
fn shortcuts_and_windows_paths_keep_the_working_directory() {
    let f = Fixture::new("cwd-kept");
    let menu = f.path("menu");
    std::fs::create_dir_all(&menu).unwrap();
    let elsewhere = f.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let cwd = format!("cwd={}", elsewhere.display());

    let link = menu.join("Game.lnk");
    let out = f.crossfree(&["start", "b", link.to_str().unwrap()]).current_dir(&elsewhere).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(f.starts()[0], cwd);

    let out = f.crossfree(&["run", "b", "--", r"C:\Games\Game.exe"]).current_dir(&elsewhere).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(f.starts(), [cwd.clone(), r"arg=C:\Games\Game.exe".into()]);

    let out = f.crossfree(&["tool", "b", "winecfg"]).current_dir(&elsewhere).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(f.starts(), [cwd, "arg=winecfg".into()]);
}

/// With `--log`, Wine's output is appended to the log and crossfree's own
/// stdout and stderr hold only crossfree's errors, which is how the app
/// tells a failed launch from a program's output.
#[test]
fn log_takes_wines_output_and_stderr_keeps_crossfrees_errors() {
    let f = Fixture::new("log");
    let log = f.path("logs/b.log");
    let log_arg = log.to_str().unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "earlier\n").unwrap();

    for args in [&["run", "b", "--", "cmd"][..], &["start", "b", "x.lnk"], &["tool", "b", "reboot"]] {
        let out = f.run(&[&["--log", log_arg], args].concat());
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        assert_eq!((text(&out.stdout), text(&out.stderr)), (String::new(), String::new()), "{args:?}");
    }
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "earlier\n".to_owned() + &"wine stdout\nwine stderr\n".repeat(3)
    );

    // The program's own exit status is crossfree's, with nothing on stderr.
    let out = f.crossfree(&["--log", log_arg, "run", "b", "--", "cmd"]).env("FAKE_EXIT", "7").output().unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stderr.is_empty());

    // crossfree's own errors go to its stderr, and Wine never starts.
    let before = std::fs::read_to_string(&log).unwrap();
    f.starts();
    for args in [&["run", "nope", "--", "cmd"][..], &["start", "nope", "x.lnk"], &["tool", "b", "regedit"]] {
        let out = f.run(&[&["--log", log_arg], args].concat());
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(text(&out.stderr).starts_with("crossfree: "), "{args:?}: {}", text(&out.stderr));
    }
    assert_eq!(std::fs::read_to_string(&log).unwrap(), before);
    assert!(f.starts().is_empty());

    // The log's folder is created when missing.
    let fresh = f.path("new/logs/b.log");
    assert!(f.run(&["--log", fresh.to_str().unwrap(), "run", "b", "--", "cmd"]).status.success());
    assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "wine stdout\nwine stderr\n");
}

/// Four `crossfree settings` processes change different settings at once,
/// many times: every change must survive and none may fail.
#[test]
fn concurrent_setting_changes_all_apply() {
    let f = Fixture::new("settings");
    let conf = f.path("prefixes/b/crossfree.conf");
    let changes = [["--d3d9", "wined3d"], ["--dxgi", "dxmt"], ["--msync", "off"], ["--metal-hud", "on"]];
    for round in 0..40 {
        std::fs::write(&conf, "d3d9=auto\ndxgi=auto\nmsync=on\nmetal_hud=off\n").unwrap();
        let children: Vec<_> = changes
            .iter()
            .map(|change| {
                f.crossfree(&[&["--json", "settings", "b"], &change[..]].concat())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in children {
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "round {round}: {}", text(&out.stderr));
        }
        let saved = std::fs::read_to_string(&conf).unwrap();
        assert!(
            saved.contains("d3d9=wined3d\ndxgi=dxmt\nmsync=off\nmetal_hud=on"),
            "round {round} lost a change:\n{saved}"
        );
    }
    let mut names: Vec<_> = std::fs::read_dir(f.path("prefixes/b"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, [".crossfree.conf.lock", "crossfree.conf", "drive_c"]);
}

/// A fake loader that also records where it really runs from.
const SELF_RECORDING_WINE: &str = r#"#!/bin/sh
{ echo "self=$(/bin/realpath "$0")"; for a in "$@"; do echo "arg=$a"; done; } >> "$FAKE_RECORD"
exit 0
"#;

/// With mach9's layout (`bin/wine` is the loader inside `wine.app`), a game
/// starts from a mirror of the engine whose loader bundle carries the
/// game's name, the name the Dock shows; a tool keeps the engine's own.
#[test]
fn games_start_under_their_own_name() {
    let f = Fixture::new("dock");
    let engine = f.path("engine");
    let loader = engine.join("lib/wine/aarch64-unix/wine.app/Contents/MacOS/wine");
    std::fs::write(&loader, SELF_RECORDING_WINE).unwrap();
    std::fs::set_permissions(&loader, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_file(engine.join("bin/wine")).unwrap();
    std::os::unix::fs::symlink("../lib/wine/aarch64-unix/wine.app/Contents/MacOS/wine", engine.join("bin/wine"))
        .unwrap();
    std::fs::write(engine.join("lib/wine/aarch64-unix/ntdll.so"), "").unwrap();
    let home = f.path("home");
    std::fs::create_dir_all(f.path("menu")).unwrap();
    let link = f.path("menu/Steam.lnk");
    std::fs::write(&link, "").unwrap();

    let out = f.crossfree(&["start", "b", link.to_str().unwrap()]).env("HOME", &home).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let starts = f.starts();
    let caches = home.join("Library/Caches/crossfree/loaders");
    let ran = starts.iter().find_map(|l| l.strip_prefix("self=")).expect("the loader ran");
    assert!(ran.starts_with(caches.to_str().unwrap()), "{ran}");
    assert!(ran.ends_with("/Steam/lib/wine/aarch64-unix/Steam.app/Contents/MacOS/wine"), "{ran}");
    assert!(starts.contains(&format!("arg={}", link.display())), "{starts:?}");

    let out = f.crossfree(&["tool", "b", "winecfg"]).env("HOME", &home).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let starts = f.starts();
    assert_eq!(starts.first().map(String::as_str), Some(format!("self={}", loader.display())).as_deref());
}
