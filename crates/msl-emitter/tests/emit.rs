//! Emitter tests: golden MSL for representative shaders, and targeted checks.
//!
//! Golden files live in `tests/golden/`. After an intended output change,
//! regenerate them with `CF_BLESS=1 cargo test -p msl-emitter --test emit`
//! and review the diff. `--test metal_compile -- --ignored` compiles them
//! with Metal.

mod shaders;

use std::path::PathBuf;

use msl_emitter::{EmitError, Options, emit};
use shader_ir::{Cond, MAX_NESTING, Stmt};
use shaders::*;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.metal"))
}

#[test]
fn golden_msl() {
    let bless = std::env::var_os("CF_BLESS").is_some();
    let mut mismatches = Vec::new();
    for (name, tokens) in all() {
        let actual = msl(&tokens);
        let path = golden_path(name);
        if bless {
            std::fs::write(&path, &actual).expect("writes golden file");
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing {}; run with CF_BLESS=1", path.display()));
        if actual != expected {
            let line = actual
                .lines()
                .zip(expected.lines())
                .position(|(a, e)| a != e)
                .unwrap_or_else(|| actual.lines().count().min(expected.lines().count()));
            mismatches.push(format!(
                "{name}: first difference at line {}\n  actual:   {:?}\n  expected: {:?}",
                line + 1,
                actual.lines().nth(line),
                expected.lines().nth(line)
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "emitted MSL differs from tests/golden (CF_BLESS=1 regenerates):\n{}",
        mismatches.join("\n")
    );
}

/// Wine texkill_test `shader_code_11`: `texkill t0` in ps_1_1 tests the
/// interpolated texture coordinates.
#[test]
fn ps_1_1_texkill_tests_texture_coordinates() {
    let msl = msl(&[
        0xffff0101, 0x00000051, 0xa00f0000, 0x3f800000, 0x00000000, 0x00000000, 0x3f800000, 0x00000041, 0xb00f0000,
        0x00000001, 0x800f0000, 0xa0e40000, 0x0000ffff,
    ]);
    assert!(msl.contains("float4 in_texcoord0 [[user(texcoord0)]];"), "{msl}");
    assert!(msl.contains("if (any((shader_in.in_texcoord0).xyz < float3(0.0))) discard_fragment();"), "{msl}");
    assert!(!msl.contains("float4 t0 ="), "{msl}");
}

/// Wine `ps_code_swizzle`: `texld r0, v0, s0.yzwx` swizzles the sample.
#[test]
fn sampler_swizzle_applies_to_the_sample() {
    let out = msl(&[
        0xffff0300, 0x0200001f, 0x80000005, 0x900f0000, 0x0200001f, 0x90000000, 0xa00f0800, 0x03000042, 0x800f0000,
        0x90e40000, 0xa0390800, 0x02000001, 0x800f0800, 0x80e40000, 0x0000ffff,
    ]);
    assert!(out.contains("r0.xyzw = (t0_texture.sample(s0, (shader_in.in_texcoord0).xy).yzwx).xyzw;"), "{out}");
    // An identity swizzle adds nothing (vs_3_0 texldl).
    let out = msl(&vs_3_0());
    assert!(out.contains("(t0_texture.sample(s0, (shader_in.v1).xy, level((shader_in.v1).w))).xyzw"), "{out}");
    let out = msl(&ps_3_0());
    assert!(out.contains(".sample(s0, (shader_in.in_texcoord0).xy).yzwx"), "{out}");
}

/// Wine constant_clamp_ps_test: ps_1_x constants are clamped to [-1, 1],
/// `def` ones when lifted and uploaded ones on every read; ps_2_0's are not.
#[test]
fn ps_1_x_constants_are_clamped() {
    let ps14 = msl(&[
        0xffff0104, 0x00000051, 0xa00f0002, 0xbf000000, 0x3fa00000, 0x40000000, 0x3f800000, 0x00000001, 0x800f0001,
        0xa0e40001, 0x00000002, 0x800f0000, 0x80e40001, 0xa0e40002, 0x0000ffff,
    ]);
    assert!(ps14.contains("r1.xyzw = (clamp(c[1], -1.0, 1.0)).xyzw;"), "{ps14}");
    assert!(ps14.contains("float4(-5e-1, 1.0, 1.0, 1.0)"), "{ps14}");
    let ps20 = msl(&[
        0xffff0200, 0x02000001, 0x800f0001, 0xa0e40001, 0x03000002, 0x800f0000, 0x80e40001, 0xa0e40002, 0x02000001,
        0x800f0800, 0x80e40000, 0x0000ffff,
    ]);
    assert!(ps20.contains("r1.xyzw = (c[1]).xyzw;"), "{ps20}");
    assert!(!ps20.contains("clamp(c["), "{ps20}");
    // Vertex shaders never clamp.
    assert!(!msl(&vs_1_1()).contains("clamp(c["));
}

/// Programs from `lift` nest at most `MAX_NESTING` deep; anything deeper is
/// refused before emission recurses into it.
#[test]
fn too_deep_programs_are_refused() {
    let shader = d3d9_bytecode::parse(&ps_2_0()).expect("decodes");
    let mut program = shader_ir::lift(&shader).expect("lifts");
    let nest = |depth: usize| {
        let mut body = Vec::new();
        for _ in 0..depth {
            body = vec![Stmt::If { cond: Cond::Bool(0), then: body, otherwise: Vec::new() }];
        }
        body
    };
    program.body = nest(MAX_NESTING - 1);
    assert!(emit(&program, &Options::new(&program)).is_ok());
    program.body = nest(MAX_NESTING);
    assert_eq!(emit(&program, &Options::new(&program)), Err(EmitError::TooDeeplyNested));
    program.body = nest(1_000);
    assert_eq!(emit(&program, &Options::new(&program)), Err(EmitError::TooDeeplyNested));
}

/// Indentation stops at 16 levels, so output stays linear in statements.
#[test]
fn indentation_is_capped() {
    // vs_2_0: 24 nested `if b0` (D3D9's limit) around a mov.
    let mut t = vec![0xfffe0200];
    (0..24).for_each(|_| t.extend([0x01000028, 0xe0e40800]));
    t.extend([0x02000001, 0x800f0000, 0xa0e40000]);
    (0..24).for_each(|_| t.push(0x0000002b));
    t.push(0x0000ffff);
    let msl = msl(&t);
    let deepest = msl.lines().map(|l| l.len() - l.trim_start().len()).max().unwrap_or(0);
    assert_eq!(deepest, 16 * 4);
    assert!(msl.contains("r0.xyzw = (c[0]).xyzw;"));
}

#[test]
fn vertex_shaders_get_the_position_fixup_and_clamped_colours() {
    let msl = msl(&vs_1_1());
    assert!(msl.starts_with("#include <metal_stdlib>"));
    assert!(msl.contains("vertex ShaderOutput shader_main("));
    assert!(msl.contains("float4 v0 [[attribute(0)]];"));
    assert!(msl.contains("shader_out.position.xy = shader_out.position.xy * runtime.pos_fixup.xy"));
    assert!(msl.contains("shader_out.out_color0 = saturate("));
    assert!(msl.contains("d3d9_const(c, 5 + a0.x)"));
    assert!(msl.contains("a0.x = int(floor("));
}

#[test]
fn pixel_only_operations_are_rejected_in_vertex_shaders() {
    // The lifter accepts texkill only in pixel shaders' layouts, so build the
    // IR directly: a vertex program with a Kill.
    let shader = d3d9_bytecode::parse(&vs_1_1()).expect("decodes");
    let mut program = shader_ir::lift(&shader).expect("lifts");
    let src = match &program.body[0] {
        Stmt::Assign { value: shader_ir::Value::Binary(_, a, _), .. } => *a,
        other => panic!("unexpected {other:?}"),
    };
    program.body.push(Stmt::Kill { src, mask: 0xf });
    assert_eq!(emit(&program, &Options::new(&program)), Err(EmitError::PixelOnlyOperation("texkill")));
}
