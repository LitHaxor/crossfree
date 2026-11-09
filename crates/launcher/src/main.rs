//! `crossfree`: command-line front end of the launcher. The macOS app drives
//! it with `--json`.

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

use launcher::{
    AppShortcut, BottleSettings, D3d9Backend, DxgiBackend, Engine, GameProfile, Launch, Prefixes, icon_cache_dir,
    loader_cache_dir, named_engine, program_folder, rules, shortcuts,
};

const USAGE: &str = "\
usage:
  crossfree engine
      Check the engine: layout, version, signature. Exits 1 when the loader
      is not notarized, except with --json, where the report says so.
  crossfree prefix list
  crossfree prefix create NAME
  crossfree prefix remove NAME
  crossfree apps NAME
      Programs installed in a bottle (its Start Menu shortcuts), with their
      icons as .ico files in ~/Library/Caches/crossfree/icons.
  crossfree settings NAME [--d3d9 auto|mtld3d|wined3d] [--dxgi auto|dxmt|wined3d]
                          [--msync on|off] [--metal-hud on|off]
      Show or change a bottle's settings.
  crossfree start NAME PATH
      Open a shortcut (.lnk, .url) or program in a bottle and return. A
      program starts in its own folder.
  crossfree kill NAME
      Quit every program in a bottle.
  crossfree tool NAME winecfg|controllers|reboot
  crossfree run PREFIX [--d3d9 mtld3d|wined3d] [--dxgi dxmt|wined3d]
                [--env NAME=VALUE]... -- PROGRAM [ARGS...]
      Run a Windows program with per-program settings, and wait for it. A
      program given as a Unix path starts in its own folder.

options (before the command):
  --engine DIR     engine bundle (default: $CROSSFREE_ENGINE)
  --prefixes DIR   prefix root (default: $CROSSFREE_PREFIXES, or
                   ~/Library/Application Support/crossfree/prefixes)
  --log FILE       append the Windows programs' output to FILE (start, tool,
                   run); crossfree's own errors still go to stderr
  --json           machine-readable output (engine, prefix, apps, settings)";

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("crossfree: {message}");
            ExitCode::from(2)
        }
    }
}

struct Global {
    engine: Option<PathBuf>,
    prefixes: Option<PathBuf>,
    log: Option<PathBuf>,
    json: bool,
}

fn run(mut args: Vec<OsString>) -> Result<ExitCode, String> {
    let mut global = Global {
        engine: std::env::var_os("CROSSFREE_ENGINE").map(PathBuf::from),
        prefixes: std::env::var_os("CROSSFREE_PREFIXES").map(PathBuf::from),
        log: None,
        json: false,
    };
    while let Some(first) = args.first().and_then(|a| a.to_str()).map(str::to_owned) {
        match first.as_str() {
            "--engine" | "--prefixes" | "--log" => {
                args.remove(0);
                let value =
                    (!args.is_empty()).then(|| PathBuf::from(args.remove(0))).ok_or(format!("{first} needs a path"))?;
                match first.as_str() {
                    "--engine" => global.engine = Some(value),
                    "--prefixes" => global.prefixes = Some(value),
                    _ => global.log = Some(value),
                }
            }
            "--json" => {
                args.remove(0);
                global.json = true;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            _ => break,
        }
    }

    let words: Vec<String> = args.iter().take_while(|a| *a != "--").map(|a| a.to_string_lossy().into_owned()).collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["engine"] => engine_info(&global, &open_engine(&global)?),
        ["prefix", "list"] => list_prefixes(&global),
        ["prefix", "create", name] => {
            let path = prefixes(&global)?.create(&open_engine(&global)?, name).map_err(|e| e.to_string())?;
            if global.json {
                println!(r#"{{"name":{},"path":{}}}"#, json_string(name), json_path(&path));
            } else {
                println!("created {}", path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        ["prefix", "remove", name] => {
            prefixes(&global)?.remove(&open_engine(&global)?, name).map_err(|e| e.to_string())?;
            if !global.json {
                println!("removed {name}");
            }
            Ok(ExitCode::SUCCESS)
        }
        ["apps", name] => list_apps(&global, name),
        ["settings", name, changes @ ..] => bottle_settings(&global, name, changes),
        ["start", name, _] => {
            let prefix = existing(&global, name)?;
            // The path as given, not its lossy UTF-8 form in `words`.
            let (target, folder) = start_target(&args[2]);
            let engine = engine_named(&global, Path::new(&args[2]))?;
            launch(&global, &engine, &prefix, "start", &["/unix".into(), target], folder)
        }
        ["kill", name] => {
            prefixes(&global)?.kill(&open_engine(&global)?, name).map_err(|e| e.to_string())?;
            Ok(ExitCode::SUCCESS)
        }
        ["tool", name, tool] => {
            let (program, tool_args): (&str, &[&str]) = match *tool {
                "winecfg" => ("winecfg", &[]),
                "controllers" => ("control", &["joy.cpl"]),
                "reboot" => ("wineboot", &["-r"]),
                other => return Err(format!("unknown tool {other}")),
            };
            let prefix = existing(&global, name)?;
            let tool_args: Vec<OsString> = tool_args.iter().map(OsString::from).collect();
            launch(&global, &open_engine(&global)?, &prefix, program, &tool_args, None)
        }
        ["run", prefix, ..] => run_program(&global, prefix, &args),
        _ => Err(format!("unknown command\n{USAGE}")),
    }
}

fn open_engine(global: &Global) -> Result<Engine, String> {
    let root = global.engine.as_ref().ok_or("no engine: pass --engine DIR or set CROSSFREE_ENGINE")?;
    Engine::open(root).map_err(|e| e.to_string())
}

/// The engine to start the program at `path` with: mirrored so the Dock
/// shows the program's name (a shortcut's or executable's, without its
/// extension) instead of "wine". Without a mirror (another engine layout,
/// or one could not be made: said on stderr) the engine itself, which
/// works the same.
fn engine_named(global: &Global, path: &Path) -> Result<Engine, String> {
    let engine = open_engine(global)?;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let name = name.as_deref().map(|n| n.rsplit('\\').next().unwrap_or(n).to_owned());
    let (Some(name), Some(cache)) = (name, loader_cache_dir()) else { return Ok(engine) };
    match named_engine(&engine, &name, &cache) {
        Ok(Some(named)) => Ok(named),
        Ok(None) => Ok(engine),
        Err(e) => {
            eprintln!("crossfree: {e}; the Dock shows the program as \"wine\"");
            Ok(engine)
        }
    }
}

fn prefixes(global: &Global) -> Result<Prefixes, String> {
    match &global.prefixes {
        Some(root) => Ok(Prefixes::new(root)),
        None => Prefixes::default_root().map(Prefixes::new).map_err(|e| e.to_string()),
    }
}

fn existing(global: &Global, name: &str) -> Result<PathBuf, String> {
    prefixes(global)?.existing(name).map_err(|e| e.to_string())
}

fn engine_info(global: &Global, engine: &Engine) -> Result<ExitCode, String> {
    let version = engine.version().map_err(|e| e.to_string())?;
    let v = engine.verify().map_err(|e| e.to_string())?;
    if global.json {
        println!(
            r#"{{"root":{},"version":{},"signature_valid":{},"team_id":{},"gatekeeper_source":{},"notarized":{}}}"#,
            json_path(engine.root()),
            json_string(&version),
            v.signature_valid,
            v.team_id.as_deref().map_or("null".into(), json_string),
            v.gatekeeper_source.as_deref().map_or("null".into(), json_string),
            v.is_notarized()
        );
    } else {
        println!("engine:     {}", engine.root().display());
        println!("version:    {version}");
        println!("signature:  {}", if v.signature_valid { "valid" } else { "INVALID" });
        println!("team:       {}", v.team_id.as_deref().unwrap_or("-"));
        println!("gatekeeper: {}", v.gatekeeper_source.as_deref().unwrap_or("rejected"));
    }
    Ok(engine_exit_code(global.json, &v))
}

/// The text report fails when the loader is not notarized, for scripts. The
/// JSON report succeeds whenever it was printed: it says so itself, and a
/// caller that reads it (the app) must get it rather than an error.
fn engine_exit_code(json: bool, v: &launcher::Verification) -> ExitCode {
    if json || v.is_notarized() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn list_prefixes(global: &Global) -> Result<ExitCode, String> {
    let prefixes = prefixes(global)?;
    let names = prefixes.list().map_err(|e| e.to_string())?;
    if !global.json {
        for name in names {
            println!("{name}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut items = Vec::new();
    for name in names {
        let path = prefixes.path(&name).map_err(|e| e.to_string())?;
        let settings = BottleSettings::load(&path).unwrap_or_default();
        items.push(format!(
            r#"{{"name":{},"path":{},"settings":{}}}"#,
            json_string(&name),
            json_path(&path),
            settings_json(&settings)
        ));
    }
    println!("[{}]", items.join(","));
    Ok(ExitCode::SUCCESS)
}

fn list_apps(global: &Global, name: &str) -> Result<ExitCode, String> {
    let apps = shortcuts(&existing(global, name)?, icon_cache_dir().as_deref());
    if global.json {
        let items: Vec<String> = apps.iter().map(app_json).collect();
        println!("[{}]", items.join(","));
    } else {
        for app in apps {
            let icon = app.icon.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            println!("{}\t{}\t{}", app.name, app.path.display(), icon);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// One app for `apps --json`: name, shortcut, icon, and Steam artwork.
fn app_json(app: &AppShortcut) -> String {
    let path = |p: &Option<PathBuf>| p.as_deref().map_or_else(|| "null".into(), json_path);
    let art = &app.artwork;
    let id = art.steam_app_id.map_or_else(|| "null".into(), |id| id.to_string());
    let text = |s: &Option<String>| s.as_deref().map_or_else(|| "null".into(), json_string);
    format!(
        r#"{{"name":{},"path":{},"icon":{},"steam_app_id":{},"hero":{},"logo":{},"cover":{},"header":{},"program":{},"install_dir":{}}}"#,
        json_string(&app.name),
        json_path(&app.path),
        path(&app.icon),
        id,
        path(&art.hero),
        path(&art.logo),
        path(&art.cover),
        path(&art.header),
        text(&app.program),
        text(&app.install_dir),
    )
}

fn bottle_settings(global: &Global, name: &str, changes: &[&str]) -> Result<ExitCode, String> {
    let prefix = existing(global, name)?;
    let changes = setting_changes(changes)?;
    let settings = if changes.is_empty() {
        BottleSettings::load(&prefix)
    } else {
        // Under the settings lock, so concurrent changes all apply.
        BottleSettings::update(&prefix, |settings| changes.iter().try_for_each(|(key, value)| settings.set(key, value)))
    }
    .map_err(|e| e.to_string())?;
    if global.json {
        println!("{}", settings_json(&settings));
    } else {
        print!("{}", settings.to_text());
    }
    Ok(ExitCode::SUCCESS)
}

/// `--name value` pairs of `crossfree settings`, as setting keys
/// (`--metal-hud` is `metal_hud`) and values.
fn setting_changes(changes: &[&str]) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    let mut changes = changes.iter();
    while let Some(option) = changes.next() {
        let key = option.strip_prefix("--").ok_or(format!("expected an option, got {option}"))?.replace('-', "_");
        let value = changes.next().ok_or(format!("{option} needs a value"))?;
        pairs.push((key, (*value).to_owned()));
    }
    Ok(pairs)
}

/// What `start` opens and where: a program (not a `.lnk` or `.url`
/// shortcut, which carries its own working directory) at a Unix path starts
/// in its folder, as Explorer starts it; its path is made absolute for that.
fn start_target(path: &OsStr) -> (OsString, Option<PathBuf>) {
    let shortcut =
        Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk") || e.eq_ignore_ascii_case("url"));
    match program_folder(path).filter(|_| !shortcut) {
        Some((program, folder)) => (program.into_os_string(), Some(folder)),
        None => (path.to_owned(), None),
    }
}

/// Runs a program in a bottle with its settings, waiting for it.
fn launch(
    global: &Global,
    engine: &Engine,
    prefix: &Path,
    program: &str,
    args: &[OsString],
    folder: Option<PathBuf>,
) -> Result<ExitCode, String> {
    let settings = BottleSettings::load(prefix).map_err(|e| e.to_string())?;
    let mut launch = Launch::new(engine, prefix, program)
        .args(args.iter().cloned())
        .settings(&settings)
        .map_err(|e| e.to_string())?;
    if let Some(folder) = folder {
        launch = launch.current_dir(folder);
    }
    wait(global, launch.command())
}

/// Runs Wine to completion, its output going to the `--log` file if there is
/// one, and exits with its status.
fn wait(global: &Global, mut wine: Command) -> Result<ExitCode, String> {
    if let Some(log) = &global.log {
        let file = open_log(log)?;
        let copy = file.try_clone().map_err(|e| format!("cannot open {}: {e}", log.display()))?;
        wine.stdout(file).stderr(copy);
    }
    let status = wine.status().map_err(|e| format!("cannot start wine: {e}"))?;
    Ok(exit_code(status))
}

/// The log file, opened to append: every launch writes at its end, however
/// many Wine processes hold it open from earlier launches.
fn open_log(path: &Path) -> Result<File, String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))
}

fn exit_code(status: ExitStatus) -> ExitCode {
    ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8)
}

fn run_program(global: &Global, prefix: &str, args: &[OsString]) -> Result<ExitCode, String> {
    let split = args.iter().position(|a| a == "--").ok_or("run needs `-- PROGRAM [ARGS...]`")?;
    let options: Vec<String> = args[2..split].iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let program = args.get(split + 1).ok_or("run needs a PROGRAM after `--`")?;
    let program_args = &args[split + 2..];

    // The profile applies to the program by its basename.
    let basename = Path::new(program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let basename = basename.rsplit('\\').next().unwrap_or(&basename).to_owned();
    let exe = if basename.contains('.') { basename } else { format!("{basename}.exe") };
    let mut profile = GameProfile::new(exe);

    let mut options = options.iter();
    while let Some(option) = options.next() {
        let value = options.next().ok_or(format!("{option} needs a value"))?;
        match option.as_str() {
            "--d3d9" => {
                profile.d3d9 = Some(D3d9Backend::from_name(value).ok_or(format!("unknown d3d9 backend {value}"))?);
            }
            "--dxgi" => {
                profile.dxgi = Some(DxgiBackend::from_name(value).ok_or(format!("unknown dxgi backend {value}"))?);
            }
            "--env" => {
                let (name, val) = value.split_once('=').ok_or("--env needs NAME=VALUE")?;
                profile.env.push((name.to_owned(), val.to_owned()));
            }
            other => return Err(format!("unknown option {other}")),
        }
    }

    let engine = engine_named(global, Path::new(program))?;
    let path = existing(global, prefix).map_err(|e| format!("{e}; create it with `crossfree prefix create`"))?;
    let settings = BottleSettings::load(&path).map_err(|e| e.to_string())?;
    let table = rules(&[profile]).map_err(|e| e.to_string())?;
    // A program at a Unix path starts in its folder, as Explorer starts it.
    let (program, folder) = match program_folder(program) {
        Some((program, folder)) => (program.into_os_string(), Some(folder)),
        None => (program.clone(), None),
    };
    let mut launch = Launch::new(&engine, path, program)
        .args(program_args.iter().cloned())
        .settings(&settings)
        .map_err(|e| e.to_string())?
        .rules(table);
    if let Some(folder) = folder {
        launch = launch.current_dir(folder);
    }
    wait(global, launch.command())
}

fn settings_json(s: &BottleSettings) -> String {
    format!(
        r#"{{"d3d9":"{}","dxgi":"{}","msync":{},"metal_hud":{}}}"#,
        s.d3d9.map_or("auto", D3d9Backend::name),
        s.dxgi.map_or("auto", DxgiBackend::name),
        s.msync,
        s.metal_hud
    )
}

fn json_path(path: &Path) -> String {
    json_string(&path.to_string_lossy())
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("plain"), r#""plain""#);
        assert_eq!(json_string("C:\\games\\\"x\"\n\u{1}"), r#""C:\\games\\\"x\"\n\u0001""#);
    }

    #[test]
    fn apps_as_json() {
        use launcher::Artwork;
        let mut app = AppShortcut {
            name: "Steam".into(),
            path: "/p/Steam.lnk".into(),
            icon: None,
            artwork: Artwork::default(),
            program: None,
            install_dir: None,
        };
        assert_eq!(
            app_json(&app),
            r#"{"name":"Steam","path":"/p/Steam.lnk","icon":null,"steam_app_id":null,"hero":null,"logo":null,"cover":null,"header":null,"program":null,"install_dir":null}"#
        );
        app.icon = Some("/c/1.ico".into());
        app.artwork = Artwork {
            steam_app_id: Some(287450),
            hero: Some("/a/hero.jpg".into()),
            logo: Some("/a/logo.png".into()),
            cover: None,
            header: Some("/a/header.jpg".into()),
        };
        app.program = Some(r"C:\Program Files (x86)\Steam\steam.exe".into());
        app.install_dir = Some(r"C:\Games\Rise of Nations".into());
        assert_eq!(
            app_json(&app),
            r#"{"name":"Steam","path":"/p/Steam.lnk","icon":"/c/1.ico","steam_app_id":287450,"hero":"/a/hero.jpg","logo":"/a/logo.png","cover":null,"header":"/a/header.jpg","program":"C:\\Program Files (x86)\\Steam\\steam.exe","install_dir":"C:\\Games\\Rise of Nations"}"#
        );
    }

    #[test]
    fn settings_as_json() {
        let mut s = BottleSettings::default();
        assert_eq!(settings_json(&s), r#"{"d3d9":"auto","dxgi":"auto","msync":true,"metal_hud":false}"#);
        s.set("d3d9", "wined3d").unwrap();
        s.set("metal_hud", "on").unwrap();
        assert_eq!(settings_json(&s), r#"{"d3d9":"wined3d","dxgi":"auto","msync":true,"metal_hud":true}"#);
    }

    #[test]
    fn json_engine_report_succeeds_when_not_notarized() {
        let notarized = launcher::Verification {
            signature_valid: true,
            gatekeeper_source: Some("Notarized Developer ID".into()),
            team_id: Some("TEAM".into()),
        };
        let rejected = launcher::Verification { signature_valid: false, gatekeeper_source: None, team_id: None };
        assert_eq!(engine_exit_code(false, &notarized), ExitCode::SUCCESS);
        assert_eq!(engine_exit_code(false, &rejected), ExitCode::FAILURE);
        assert_eq!(engine_exit_code(true, &notarized), ExitCode::SUCCESS);
        assert_eq!(engine_exit_code(true, &rejected), ExitCode::SUCCESS);
    }

    #[test]
    fn setting_options_become_keys_and_values() {
        assert_eq!(
            setting_changes(&["--metal-hud", "on", "--d3d9", "wined3d"]).unwrap(),
            [("metal_hud".to_owned(), "on".to_owned()), ("d3d9".to_owned(), "wined3d".to_owned())]
        );
        assert!(setting_changes(&[]).unwrap().is_empty());
        assert!(setting_changes(&["d3d9", "auto"]).unwrap_err().contains("expected an option"));
        assert!(setting_changes(&["--msync"]).unwrap_err().contains("needs a value"));
    }

    #[test]
    fn start_opens_programs_in_their_folder_and_shortcuts_as_they_are() {
        let dir = std::env::temp_dir().join(format!("crossfree-start-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let game = dir.join("Game.exe");
        assert_eq!(start_target(game.as_os_str()), (game.clone().into_os_string(), Some(dir.clone())));
        for shortcut in ["Game.lnk", "Store Page.URL"] {
            let path = dir.join(shortcut);
            assert_eq!(start_target(path.as_os_str()), (path.clone().into_os_string(), None), "{shortcut}");
        }
        let windows = OsStr::new(r"C:\Games\Game.exe");
        assert_eq!(start_target(windows), (windows.to_owned(), None));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
