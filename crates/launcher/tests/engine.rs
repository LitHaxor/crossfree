//! End-to-end tests against a real engine bundle. They skip (with a note)
//! when none is present. Set `CROSSFREE_ENGINE` to the bundle's root, or keep
//! the spike's copy under `research/engines/` (git-ignored).

use std::path::{Path, PathBuf};
use std::process::Command;

use launcher::{D3d9Backend, Engine, GameProfile, Launch, Prefixes, rules};

const DEFAULT_ENGINE: &str = "../../research/engines/mach9-v0.2.0/bundle/wine";

fn engine() -> Option<Engine> {
    let root = std::env::var_os("CROSSFREE_ENGINE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_ENGINE));
    match Engine::open(&root) {
        Ok(engine) => Some(engine),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\r', "")
}

#[test]
fn engine_is_signed_and_notarized() {
    let Some(engine) = engine() else { return };
    assert!(engine.version().unwrap().starts_with("wine-"));
    let verification = engine.verify().unwrap();
    assert!(verification.is_notarized(), "{verification:?}");
    assert!(verification.team_id.is_some());
}

/// One prefix for the whole lifecycle: creating one takes ~15 s.
#[test]
fn prefix_lifecycle_and_rules_reach_windows_processes() {
    let Some(engine) = engine() else { return };
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("launcher-prefixes");
    let _ = std::fs::remove_dir_all(&root);
    let prefixes = Prefixes::new(&root);

    let path = prefixes.create(&engine, "e2e").unwrap();
    assert_eq!(prefixes.list().unwrap(), ["e2e"]);

    // A rule for cmd.exe reaches a native ARM64 cmd...
    let mut profile = GameProfile::new("cmd.exe");
    profile.env.push(("CROSSFREE_CHECK".into(), "from-rules".into()));
    let table = rules(&[profile]).unwrap();
    let out = Launch::new(&engine, &path, "cmd")
        .args(["/c", "echo [%CROSSFREE_CHECK%] %PROCESSOR_ARCHITECTURE%"])
        .rules(table.clone())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(text(&out.stdout).trim(), "[from-rules] ARM64");

    // ...and the 32-bit x86 cmd that FEX runs through WoW64.
    let out = Launch::new(&engine, &path, r"C:\windows\syswow64\cmd.exe")
        .args(["/c", "echo [%CROSSFREE_CHECK%] %PROCESSOR_ARCHITECTURE%"])
        .rules(table)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(text(&out.stdout).trim(), "[from-rules] x86");

    // A D3D9 backend rule selects wined3d for the conformance test.
    if let Some(tests) = engine.tests_dir() {
        let mut d3d9 = GameProfile::new("d3d9_test.exe");
        d3d9.d3d9 = Some(D3d9Backend::Wined3d);
        let out = Launch::new(&engine, &path, tests.join("x86_64-windows/d3d9_test.exe"))
            .args(["stateblock"])
            .rules(rules(&[d3d9]).unwrap())
            .output()
            .unwrap();
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains("d3d9 = wined3d") && stderr.contains("WINE_COMPATDB line 1"),
            "backend rule not applied:\n{stderr}"
        );
        assert!(out.status.success(), "{} failed checks", out.status.code().unwrap_or(-1));
    }

    prefixes.remove(&engine, "e2e").unwrap();
    assert!(prefixes.list().unwrap().is_empty());
}

#[test]
fn cli_engine_check_and_usage_errors() {
    let bin = env!("CARGO_BIN_EXE_crossfree");
    let out = Command::new(bin).arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("crossfree run PREFIX"));

    let out = Command::new(bin).arg("bogus").env_remove("CROSSFREE_ENGINE").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("unknown command"));

    let out = Command::new(bin).arg("engine").env_remove("CROSSFREE_ENGINE").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("no engine"));

    let Some(engine) = engine() else { return };
    let out =
        Command::new(bin).args(["--engine".as_ref(), engine.root().as_os_str(), "engine".as_ref()]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("gatekeeper: Notarized Developer ID"));
}
