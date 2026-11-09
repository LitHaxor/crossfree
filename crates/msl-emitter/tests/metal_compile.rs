//! Compiles emitted MSL with Metal: the representative shaders from
//! `tests/shaders` and every Wine d3d9 test shader that lifts.
//!
//! Opt-in, macOS only. It builds `tests/metal/mslc.swift` with `swiftc`
//! (Command Line Tools are enough; the `metal` CLI is not needed) and
//! compiles through `MTLDevice.makeLibrary(source:)`, as the runtime will:
//!
//! ```text
//! cargo test -p msl-emitter --test metal_compile -- --ignored
//! ```
#![cfg(target_os = "macos")]

mod shaders;

use std::path::{Path, PathBuf};
use std::process::Command;

fn build_compiler(dir: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/metal/mslc.swift");
    let tool = dir.join("mslc");
    let status = Command::new("swiftc")
        .args(["-O", "-o"])
        .arg(&tool)
        .arg(&source)
        .status()
        .expect("runs swiftc (install the Xcode Command Line Tools)");
    assert!(status.success(), "swiftc failed to build {}", source.display());
    tool
}

#[test]
#[ignore = "needs swiftc and a Metal device; run with --ignored"]
fn emitted_msl_compiles_with_metal() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("msl-emitter-metal");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("msl")).expect("creates output directory");

    let mut files = Vec::new();
    let mut write = |name: String, msl: String| {
        let path = dir.join("msl").join(format!("{name}.metal"));
        std::fs::write(&path, msl).expect("writes MSL");
        files.push(path);
    };
    for (name, tokens) in shaders::all() {
        write(name.to_owned(), shaders::msl(&tokens));
    }
    if let Some(corpus) = wine_test_corpus::load_or_skip() {
        for (i, shader) in corpus.shaders.iter().enumerate() {
            let Ok(decoded) = d3d9_bytecode::parse(&shader.tokens) else { continue };
            let Ok(program) = shader_ir::lift(&decoded) else { continue };
            let msl = msl_emitter::emit(&program, &msl_emitter::Options::new(&program)).expect("emits");
            let label = shader.label().replace([':', '.', '/'], "_");
            write(format!("{i:03}_{label}"), format!("// {}\n{msl}", shader.label()));
        }
    }

    let tool = build_compiler(&dir);
    let output = Command::new(&tool).args(&files).output().expect("runs mslc");
    let log = String::from_utf8_lossy(&output.stdout);
    let ok = log.lines().filter(|l| l.starts_with("OK ")).count();
    eprintln!("Metal compiled {ok} of {} shaders (sources in {})", files.len(), dir.display());
    assert!(
        output.status.success() && ok == files.len(),
        "Metal rejected emitted MSL:\n{}{}",
        log.lines().filter(|l| !l.starts_with("OK ")).collect::<Vec<_>>().join("\n"),
        String::from_utf8_lossy(&output.stderr)
    );
}
