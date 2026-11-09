//! Decodes every shader embedded in Wine's d3d9 conformance tests.
//!
//! When the Wine sources are missing the test fails on CI (or with
//! `CF_REQUIRE_WINE_CORPUS=1`) and is skipped, with a notice, otherwise; see
//! `wine_test_corpus::load_or_skip`.

use d3d9_bytecode::parse;

/// Shaders whose bytecode format Wine's tests expect `CreatePixelShader` to
/// reject: file, array name, and the error the decoder must report. Each must
/// be found and rejected exactly once. Shaders Wine expects to be rejected for
/// register limits or device caps (`ps_1_8`, `vs_3_b16`, ...) decode fine here;
/// that is the job of a later validation pass.
const EXPECTED_FAILURES: &[(&str, &str, &str)] = &[
    ("d3d9ex.c", "ps_2_0_boolint", "defi is not available in ps_2_0"),
    ("device.c", "ps_2_0_boolint", "defi is not available in ps_2_0"),
];

#[test]
fn decodes_wine_d3d9_test_shaders() {
    let Some(corpus) = wine_test_corpus::load_or_skip() else {
        return;
    };

    let mut problems = Vec::new();
    let mut rejected = vec![0usize; EXPECTED_FAILURES.len()];
    for shader in &corpus.shaders {
        let expected =
            EXPECTED_FAILURES.iter().position(|(file, name, _)| *file == shader.file && *name == shader.name);
        match (parse(&shader.tokens), expected) {
            (Ok(_), None) => {}
            (Err(e), Some(i)) if e.to_string().contains(EXPECTED_FAILURES[i].2) => rejected[i] += 1,
            (Err(e), _) => problems.push(format!("{}: {e}", shader.label())),
            (Ok(_), Some(i)) => problems.push(format!(
                "{}: decoded, but Wine expects rejection ({})",
                shader.label(),
                EXPECTED_FAILURES[i].2
            )),
        }
    }
    let expected_seen: usize = rejected.iter().sum();
    eprintln!(
        "Wine d3d9 corpus: {} shaders, {} rejected as Wine expects, {} problems, {} non-literal arrays skipped",
        corpus.shaders.len(),
        expected_seen,
        problems.len(),
        corpus.skipped_arrays
    );
    assert!(corpus.shaders.len() >= 190, "corpus extraction found only {} shaders", corpus.shaders.len());
    let not_once: Vec<String> = EXPECTED_FAILURES
        .iter()
        .zip(&rejected)
        .filter(|&(_, &count)| count != 1)
        .map(|((file, name, _), count)| format!("{file}:{name} rejected {count} times"))
        .collect();
    assert!(not_once.is_empty(), "expected failures not seen exactly once: {not_once:?}");
    assert_eq!(expected_seen, 2, "ps_2_0_boolint in device.c and d3d9ex.c");
    assert!(problems.is_empty(), "corpus problems:\n{}", problems.join("\n"));
}
