//! Every shader in Wine's d3d9 tests that lifts must also emit. When the Wine
//! sources are missing this fails on CI (or with `CF_REQUIRE_WINE_CORPUS=1`)
//! and is skipped, with a notice, otherwise; see
//! `wine_test_corpus::load_or_skip`.

#[test]
fn emits_every_lifted_wine_d3d9_shader() {
    let Some(corpus) = wine_test_corpus::load_or_skip() else {
        return;
    };

    let mut emitted = 0;
    let mut problems = Vec::new();
    for shader in &corpus.shaders {
        let Ok(decoded) = d3d9_bytecode::parse(&shader.tokens) else {
            continue; // Covered by d3d9-bytecode's tests.
        };
        let Ok(program) = shader_ir::lift(&decoded) else {
            continue; // Covered by shader-ir's tests.
        };
        match msl_emitter::emit(&program, &msl_emitter::Options::new(&program)) {
            Ok(msl) if msl.contains(" ShaderOutput shader_main(") && msl.ends_with("}\n") => emitted += 1,
            Ok(msl) => problems.push(format!("{}: malformed output:\n{msl}", shader.label())),
            Err(e) => problems.push(format!("{}: {e}", shader.label())),
        }
    }
    eprintln!("Wine d3d9 corpus: {emitted} emitted, {} problems", problems.len());
    assert!(emitted >= 150, "only {emitted} shaders emitted");
    assert!(problems.is_empty(), "emit problems:\n{}", problems.join("\n"));
}
