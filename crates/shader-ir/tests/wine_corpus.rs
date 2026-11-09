//! Lifts every decodable shader in Wine's d3d9 tests. When the Wine sources
//! are missing this fails on CI (or with `CF_REQUIRE_WINE_CORPUS=1`) and is
//! skipped, with a notice, otherwise; see `wine_test_corpus::load_or_skip`.

use std::collections::BTreeMap;

use shader_ir::{LiftError, lift};

/// Instructions the IR does not handle yet. Anything else failing is a bug.
const KNOWN_UNSUPPORTED: &[&str] = &["ps_1_x texture-addressing instruction"];

#[test]
fn lifts_wine_d3d9_test_shaders() {
    let Some(corpus) = wine_test_corpus::load_or_skip() else {
        return;
    };

    let mut lifted = 0;
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut problems = Vec::new();
    for shader in &corpus.shaders {
        let Ok(decoded) = d3d9_bytecode::parse(&shader.tokens) else {
            continue; // Decoder failures are covered by d3d9-bytecode's tests.
        };
        match lift(&decoded) {
            Ok(_) => lifted += 1,
            Err(LiftError::Unsupported { opcode, reason, .. }) if KNOWN_UNSUPPORTED.contains(&reason) => {
                *unsupported.entry(opcode.mnemonic().to_owned()).or_default() += 1;
            }
            Err(e) => problems.push(format!("{}: {e}", shader.label())),
        }
    }
    eprintln!(
        "Wine d3d9 corpus: {lifted} lifted, {} not yet supported {unsupported:?}, {} problems",
        unsupported.values().sum::<usize>(),
        problems.len()
    );
    assert!(lifted >= 150, "only {lifted} shaders lifted");
    assert!(problems.is_empty(), "lift problems:\n{}", problems.join("\n"));
}
