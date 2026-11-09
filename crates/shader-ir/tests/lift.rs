//! Lifting tests. Token streams marked "Wine" come from Wine's d3d9 tests.

use d3d9_bytecode::{Opcode, Usage, parse};
use shader_ir::{
    BinaryOp, Cond, Dest, LiftError, LoopKind, Operand, Predicate, Reg, Relative, Rounding, SampleKind, Semantic,
    SrcModifier, Stage, Stmt, TextureType, UnaryOp, Value, lift,
};

const END: u32 = 0x0000_ffff;

fn lifted(tokens: &[u32]) -> shader_ir::Program {
    let shader = parse(tokens).expect("decodes");
    lift(&shader).unwrap_or_else(|e| panic!("lift failed: {e}"))
}

fn lift_error(tokens: &[u32]) -> LiftError {
    lift(&parse(tokens).expect("decodes")).expect_err("lift should fail")
}

fn operand(reg: Reg, swizzle: [u8; 4]) -> Operand {
    Operand { reg, relative: None, swizzle, modifier: SrcModifier::None }
}

const XYZW: [u8; 4] = [0, 1, 2, 3];

/// Wine `simple_vs`.
#[test]
fn vs_1_1_signature_and_dp4() {
    let p = lifted(&[
        0xfffe0101, 0x0000001f, 0x80000000, 0x900f0000, 0x00000009, 0xc0010000, 0x90e40000, 0xa0e40000, 0x00000009,
        0xc0020000, 0x90e40000, 0xa0e40001, END,
    ]);
    assert_eq!(p.stage, Stage::Vertex);
    assert_eq!(p.inputs.len(), 1);
    assert_eq!(p.inputs[0].semantic, Semantic::Generic { usage: Usage::Position, index: 0 });
    assert_eq!((p.inputs[0].register, p.inputs[0].mask), (0, 0xf));
    assert_eq!(p.outputs.len(), 1);
    assert_eq!((p.outputs[0].semantic, p.outputs[0].mask), (Semantic::Position, 0b0011));
    assert_eq!(p.constants.floats_read.iter().copied().collect::<Vec<_>>(), [0, 1]);
    assert_eq!(
        p.body[0],
        Stmt::Assign {
            dst: Dest { reg: Reg::Output(0), mask: 1 },
            value: Value::Binary(BinaryOp::Dp4, operand(Reg::Input(0), XYZW), operand(Reg::Const(0), XYZW)),
            saturate: false,
            shift: 0,
            predicate: None,
        }
    );
}

#[test]
fn scalar_instructions_read_the_w_slot() {
    // vs_2_0: rcp r0, c0 (identity swizzle) reads c0.w.
    let p = lifted(&[0xfffe0200, 0x02000006, 0x800f0000, 0xa0e40000, END]);
    match &p.body[0] {
        Stmt::Assign { value: Value::Unary(UnaryOp::Rcp, src), .. } => assert_eq!(src.swizzle, [3; 4]),
        other => panic!("unexpected {other:?}"),
    }
    // pow reads the x slot of both sources.
    let p = lifted(&[0xfffe0200, 0x03000020, 0x800f0000, 0xa0e40000, 0xa0e40001, END]);
    match &p.body[0] {
        Stmt::Assign { value: Value::Binary(BinaryOp::Pow, a, b), .. } => {
            assert_eq!((a.swizzle, b.swizzle), ([0; 4], [0; 4]));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn address_loads_round_by_version() {
    // vs_1_1 mov a0.x, c7.x floors.
    let p = lifted(&[0xfffe0101, 0x00000001, 0xb0010000, 0xa0000007, END]);
    assert_eq!(
        p.body[0],
        Stmt::LoadAddress { mask: 1, src: operand(Reg::Const(7), [0; 4]), rounding: Rounding::Floor }
    );
    // vs_2_0 mova rounds to nearest.
    let p = lifted(&[0xfffe0200, 0x0200002e, 0xb0010000, 0xa0000007, END]);
    assert!(matches!(p.body[0], Stmt::LoadAddress { rounding: Rounding::Nearest, .. }));
}

/// Wine `mova_test` (shortened).
#[test]
fn relative_constant_reads() {
    let p = lifted(&[
        0xfffe0200, 0x0200001f, 0x80000000, 0x900f0000, 0x05000051, 0xa00f0000, 0x3f800000, 0x00000000, 0x00000000,
        0x3f800000, 0x0200002e, 0xb0010000, 0xa0000007, 0x03000001, 0xd00f0000, 0xa0e42003, 0xb0000000, END,
    ]);
    assert!(p.constants.float_relative);
    assert_eq!(p.constants.float_defs.get(&0), Some(&[1.0, 0.0, 0.0, 1.0]));
    assert_eq!(p.outputs[0].semantic, Semantic::Color(0));
    match &p.body[1] {
        Stmt::Assign { value: Value::Unary(UnaryOp::Mov, src), .. } => {
            assert_eq!(src.reg, Reg::Const(3));
            assert_eq!(src.relative, Some(Relative { reg: Reg::Addr, component: 0 }));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn matrix_instructions_expand_into_dot_products() {
    // vs_1_1: dcl_position v0; m4x4 oPos, v0, c4
    let p =
        lifted(&[0xfffe0101, 0x0000001f, 0x80000000, 0x900f0000, 0x00000014, 0xc00f0000, 0x90e40000, 0xa0e40004, END]);
    assert_eq!(p.body.len(), 4);
    for (row, stmt) in p.body.iter().enumerate() {
        match stmt {
            Stmt::Assign { dst, value: Value::Binary(BinaryOp::Dp4, _, m), .. } => {
                assert_eq!(dst.mask, 1 << row);
                assert_eq!(m.reg, Reg::Const(4 + row as u16));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    // m3x2 r0.xy, v0, c0 → two dp3 rows.
    let p =
        lifted(&[0xfffe0101, 0x0000001f, 0x80000000, 0x900f0000, 0x00000018, 0x80030000, 0x90e40000, 0xa0e40000, END]);
    assert_eq!(p.body.len(), 2);
    assert!(matches!(p.body[1], Stmt::Assign { value: Value::Binary(BinaryOp::Dp3, ..), .. }));
}

/// Wine `shader_code_11_coissue` (shortened): texcoord, and r0 as the output.
#[test]
fn ps_1_1_texcoord_and_implicit_output() {
    let p = lifted(&[0xffff0101, 0x00000040, 0xb00f0000, 0x00000001, 0x800f0000, 0xb0e40000, END]);
    assert_eq!(p.inputs[0].semantic, Semantic::TexCoord(0));
    assert_eq!(p.texture_regs, 1);
    assert_eq!(
        p.body[0],
        Stmt::Assign {
            dst: Dest { reg: Reg::Texture(0), mask: 0xf },
            value: Value::Unary(UnaryOp::Mov, operand(Reg::Input(0), XYZW)),
            saturate: true,
            shift: 0,
            predicate: None,
        }
    );
    // The last statement copies r0 to the colour output.
    assert_eq!(p.outputs[0].semantic, Semantic::Target(0));
    assert_eq!(
        p.body.last(),
        Some(&Stmt::Assign {
            dst: Dest { reg: Reg::Output(0), mask: 0xf },
            value: Value::Unary(UnaryOp::Mov, operand(Reg::Temp(0), XYZW)),
            saturate: false,
            shift: 0,
            predicate: None,
        })
    );
}

#[test]
fn ps_1_x_tex_samples_its_own_coordinate_set() {
    // ps_1_1: tex t2
    let p = lifted(&[0xffff0101, 0x00000042, 0xb00f0002, END]);
    assert_eq!(p.samplers.get(&2), Some(&TextureType::Unknown));
    assert_eq!(p.inputs[0].semantic, Semantic::TexCoord(2));
    match &p.body[0] {
        Stmt::Assign { dst, value: Value::Sample(s), .. } => {
            assert_eq!(dst.reg, Reg::Texture(2));
            assert_eq!((s.sampler, s.coord.reg, s.kind), (2, Reg::Input(0), SampleKind::Plain));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn ps_1_4_texld_projects_with_dz() {
    // ps_1_4: texld r1, t1_dz
    let p = lifted(&[0xffff0104, 0x00000042, 0x800f0001, 0xb9e40001, END]);
    match &p.body[0] {
        Stmt::Assign { value: Value::Sample(s), .. } => {
            assert_eq!((s.sampler, s.kind, s.coord.modifier), (1, SampleKind::ProjectZ, SrcModifier::None));
            assert_eq!(p.inputs[s.coord.reg_input().unwrap()].semantic, Semantic::TexCoord(1));
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// Wine projected-texture test: dcl_2d s0, texld, texldp.
#[test]
fn ps_2_0_sampling() {
    let p = lifted(&[
        0xffff0200, 0x0200001f, 0x90000000, 0xa00f0800, 0x0200001f, 0x80000000, 0xb00f0000, 0x03000042, 0x800f0000,
        0xb0e40000, 0xa0e40800, 0x03010042, 0x800f0001, 0xb0e40000, 0xa0e40800, 0x02000001, 0x800f0800, 0x80e40000,
        END,
    ]);
    assert_eq!(p.samplers.get(&0), Some(&TextureType::Texture2D));
    let kinds: Vec<_> = p
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Assign { value: Value::Sample(sample), .. } => Some(sample.kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [SampleKind::Plain, SampleKind::ProjectW]);
    assert_eq!(p.outputs[0].semantic, Semantic::Target(0));
}

#[test]
fn texkill_tests_three_or_four_components() {
    // Wine texkill_test, ps_1_1: texkill t0 before t0 is written tests texture
    // coordinate set 0 (xyz only), not a zeroed register.
    let ps11 = lifted(&[
        0xffff0101, 0x00000051, 0xa00f0000, 0x3f800000, 0x00000000, 0x00000000, 0x3f800000, 0x00000041, 0xb00f0000,
        0x00000001, 0x800f0000, 0xa0e40000, END,
    ]);
    assert!(ps11.discards);
    assert!(matches!(ps11.body[0], Stmt::Kill { mask: 0x7, src: Operand { reg: Reg::Input(0), .. } }));
    assert_eq!(ps11.inputs[0].semantic, Semantic::TexCoord(0));
    assert_eq!(ps11.texture_regs, 0);
    let ps20 = lifted(&[0xffff0200, 0x0200001f, 0x80000000, 0xb00f0000, 0x01000041, 0xb00f0000, END]);
    assert!(matches!(ps20.body[0], Stmt::Kill { mask: 0xf, src: Operand { reg: Reg::Input(0), .. } }));
}

#[test]
fn ps_1_x_texture_registers_hold_coordinates_until_written() {
    // ps_1_1: texkill t1; mov r0, t1; tex t1; texkill t1; mov r1, t1
    let p = lifted(&[
        0xffff0101, 0x00000041, 0xb00f0001, 0x00000001, 0x800f0000, 0xb0e40001, 0x00000042, 0xb00f0001, 0x00000041,
        0xb00f0001, 0x00000001, 0x800f0001, 0xb0e40001, END,
    ]);
    assert_eq!(p.inputs.len(), 1);
    assert_eq!(p.inputs[0].semantic, Semantic::TexCoord(1));
    // Before `tex t1`, t1 is the coordinate set.
    assert!(matches!(p.body[0], Stmt::Kill { src: Operand { reg: Reg::Input(0), .. }, .. }));
    match &p.body[1] {
        Stmt::Assign { value: Value::Unary(UnaryOp::Mov, src), .. } => assert_eq!(src.reg, Reg::Input(0)),
        other => panic!("unexpected {other:?}"),
    }
    // After it, t1 is the sampled value.
    assert!(matches!(p.body[2], Stmt::Assign { dst: Dest { reg: Reg::Texture(1), .. }, .. }));
    assert!(matches!(p.body[3], Stmt::Kill { src: Operand { reg: Reg::Texture(1), .. }, .. }));
    match &p.body[4] {
        Stmt::Assign { value: Value::Unary(UnaryOp::Mov, src), .. } => assert_eq!(src.reg, Reg::Texture(1)),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(p.texture_regs, 2);
}

/// Wine constant_clamp_ps_test: ps_1_x constants, `def` ones included, are
/// clamped to [-1, 1]; ps_2_0 constants are not.
#[test]
fn ps_1_x_float_constants_are_clamped() {
    let ps14 = lifted(&[
        0xffff0104, 0x00000051, 0xa00f0002, 0xbf000000, 0x3fa00000, 0x40000000, 0x3f800000, 0x00000001, 0x800f0001,
        0xa0e40001, 0x00000002, 0x800f0000, 0x80e40001, 0xa0e40002, END,
    ]);
    assert!(ps14.constants.clamp_floats);
    assert_eq!(ps14.constants.float_defs.get(&2), Some(&[-0.5, 1.0, 1.0, 1.0]));
    let ps20 = lifted(&[
        0xffff0200, 0x05000051, 0xa00f0002, 0xbf000000, 0x3fa00000, 0x40000000, 0x3f800000, 0x02000001, 0x800f0800,
        0xa0e40002, END,
    ]);
    assert!(!ps20.constants.clamp_floats);
    assert_eq!(ps20.constants.float_defs.get(&2), Some(&[-0.5, 1.25, 2.0, 1.0]));
    // Vertex shaders never clamp.
    let vs11 = lifted(&[0xfffe0101, 0x00000051, 0xa00f0000, 0x40000000, 0, 0, 0, END]);
    assert!(!vs11.constants.clamp_floats);
    assert_eq!(vs11.constants.float_defs.get(&0), Some(&[2.0, 0.0, 0.0, 0.0]));
}

/// Wine ps_code_swizzle: `texld r0, v0, s0.yzwx` swizzles the sampled value.
#[test]
fn sampler_swizzle_is_kept() {
    let p = lifted(&[
        0xffff0300, 0x0200001f, 0x80000005, 0x900f0000, 0x0200001f, 0x90000000, 0xa00f0800, 0x03000042, 0x800f0000,
        0x90e40000, 0xa0390800, 0x02000001, 0x800f0800, 0x80e40000, END,
    ]);
    match &p.body[0] {
        Stmt::Assign { value: Value::Sample(s), .. } => assert_eq!(s.swizzle, [1, 2, 3, 0]),
        other => panic!("unexpected {other:?}"),
    }
    // texldl and texldd keep it too; ps_1_x samples are unswizzled.
    let p = lifted(&[
        0xffff0300, 0x0200001f, 0x80000005, 0x900f0000, 0x0200001f, 0x90000000, 0xa00f0800, 0x0300005f, 0x800f0000,
        0x90e40000, 0xa01b0800, 0x0500005d, 0x800f0001, 0x90e40000, 0xa0000800, 0x90e40000, 0x90e40000, END,
    ]);
    let swizzles: Vec<_> = p
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Assign { value: Value::Sample(sample), .. } => Some(sample.swizzle),
            _ => None,
        })
        .collect();
    assert_eq!(swizzles, [[3, 2, 1, 0], [0, 0, 0, 0]]);
    let ps11 = lifted(&[0xffff0101, 0x00000042, 0xb00f0000, END]);
    assert!(matches!(&ps11.body[0], Stmt::Assign { value: Value::Sample(s), .. } if s.swizzle == XYZW));
}

// ---- Limits ---------------------------------------------------------------

const IF_B0: [u32; 2] = [0x01000028, 0xe0e40800];
const ENDIF: u32 = 0x0000002b;
const REP_I0: [u32; 2] = [0x01000026, 0xf0e40000];
const ENDREP: u32 = 0x00000027;
const MOV_R0_C0: [u32; 3] = [0x02000001, 0x800f0000, 0xa0e40000];

fn call(label: u32) -> [u32; 2] {
    [0x01000019, 0xa0001000 | label]
}

fn label(label: u32) -> [u32; 2] {
    [0x0100001e, 0xa0001000 | label]
}

/// vs_2_0: `if b0` nested `ifs` deep, with `reps` nested `rep i0` inside.
fn nested(ifs: usize, reps: usize) -> Vec<u32> {
    let mut t = vec![0xfffe0200];
    (0..ifs).for_each(|_| t.extend(IF_B0));
    (0..reps).for_each(|_| t.extend(REP_I0));
    t.extend(MOV_R0_C0);
    (0..reps).for_each(|_| t.push(ENDREP));
    (0..ifs).for_each(|_| t.push(ENDIF));
    t.push(END);
    t
}

/// vs_2_0: main calls l0; each of l0..l(levels-2) calls the next `fanout`
/// times; the last label holds one instruction. Inlining copies the last
/// label fanout^(levels-1) times.
fn call_tree(levels: u32, fanout: usize) -> Vec<u32> {
    let mut t = vec![0xfffe0200];
    t.extend(call(0));
    t.push(0x0000001c);
    for l in 0..levels {
        t.extend(label(l));
        if l + 1 < levels {
            (0..fanout).for_each(|_| t.extend(call(l + 1)));
        } else {
            t.extend(MOV_R0_C0);
        }
        t.push(0x0000001c);
    }
    t.push(END);
    t
}

fn malformed_reason(tokens: &[u32]) -> &'static str {
    match lift_error(tokens) {
        LiftError::Malformed { reason, .. } => reason,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn nesting_follows_d3d9_limits() {
    assert_eq!((shader_ir::MAX_IF_DEPTH, shader_ir::MAX_LOOP_DEPTH), (24, 4));
    lifted(&nested(24, 4));
    assert_eq!(malformed_reason(&nested(25, 0)), "if nested deeper than D3D9 allows (24)");
    assert_eq!(malformed_reason(&nested(0, 5)), "loop/rep nested deeper than D3D9 allows (4)");
    // loop counts with rep.
    let mut t = vec![0xffff0300];
    (0..4).for_each(|_| t.extend(REP_I0));
    t.extend([0x0200001b, 0xf0e40800, 0xf0e40000]);
    t.extend([0x0000001d, ENDREP, ENDREP, ENDREP, ENDREP, END]);
    assert_eq!(malformed_reason(&t), "loop/rep nested deeper than D3D9 allows (4)");
}

/// A million nested `if`s used to lift into a tree whose `Drop` overflowed
/// the stack. Now lifting stops at the 25th.
#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let tokens = nested(1_000_000, 0);
    let start = std::time::Instant::now();
    assert!(matches!(lift_error(&tokens), LiftError::Malformed { .. }));
    assert!(start.elapsed().as_secs() < 10);
}

#[test]
fn call_nesting_follows_d3d9_limit() {
    // main → l0 → l1 → l2 → l3 is four nested calls: allowed.
    let p = lifted(&call_tree(4, 1));
    assert_eq!(p.body.len(), 1);
    assert_eq!(malformed_reason(&call_tree(5, 1)), "subroutine calls nested deeper than D3D9 allows (4)");
    // Recursion always exceeds the limit.
    let mut t = vec![0xfffe0200];
    t.extend(call(0));
    t.push(0x0000001c);
    t.extend(label(0));
    t.extend(call(0));
    t.extend([0x0000001c, END]);
    assert_eq!(malformed_reason(&t), "subroutine calls nested deeper than D3D9 allows (4)");
}

/// The review's stress shape: 172 tokens that used to inline into ten
/// million statements (2 GB). Now rejected by the call-depth limit, and a
/// within-limits fan-out (200³ = 8M copies) by the statement budget.
#[test]
fn subroutine_inlining_is_bounded() {
    let start = std::time::Instant::now();
    let tokens = call_tree(8, 10);
    assert_eq!(tokens.len(), 172);
    assert!(matches!(lift_error(&tokens), LiftError::Malformed { .. }));

    match lift_error(&call_tree(4, 200)) {
        LiftError::Unsupported { opcode, reason, .. } => {
            assert_eq!(opcode, Opcode::Mov);
            assert_eq!(reason, "shader too large once subroutines are inlined");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(start.elapsed().as_secs() < 10);

    // Just under the budget still lifts: 40³ = 64000 statements.
    assert_eq!(lifted(&call_tree(4, 40)).body.len(), 64_000);
}

/// Wine loop test: nested `loop aL, i0`.
#[test]
fn loops_nest() {
    let p = lifted(&[
        0xffff0300, 0x05000030, 0xf00f0000, 0x00000004, 0x00000000, 0x00000002, 0x00000000, 0x0200001b, 0xf0e40800,
        0xf0e40000, 0x0200001b, 0xf0e40800, 0xf0e40000, 0x03000002, 0x800f0000, 0x80e40000, 0x80e40000, 0x0000001d,
        0x0000001d, 0x02000001, 0x800f0800, 0x80e40000, END,
    ]);
    assert_eq!(p.constants.int_defs.get(&0), Some(&[4, 0, 2, 0]));
    match &p.body[0] {
        Stmt::Loop { kind: LoopKind::Loop { control: 0 }, body } => {
            assert!(matches!(&body[0], Stmt::Loop { kind: LoopKind::Loop { control: 0 }, body } if body.len() == 1));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn if_else_and_subroutines() {
    // vs_2_0: if b0; mov r0, c0; else; mov r0, c1; endif; call l0; ret; label l0; mov r1, c2; ret
    let p = lifted(&[
        0xfffe0200, 0x01000028, 0xe0e40800, 0x02000001, 0x800f0000, 0xa0e40000, 0x0000002a, 0x02000001, 0x800f0000,
        0xa0e40001, 0x0000002b, 0x01000019, 0xa0001000, 0x0000001c, 0x0100001e, 0xa0001000, 0x02000001, 0x800f0001,
        0xa0e40002, 0x0000001c, END,
    ]);
    assert_eq!(p.body.len(), 2);
    match &p.body[0] {
        Stmt::If { cond: Cond::Bool(0), then, otherwise } => assert_eq!((then.len(), otherwise.len()), (1, 1)),
        other => panic!("unexpected {other:?}"),
    }
    // The call was inlined.
    assert!(matches!(&p.body[1], Stmt::Assign { dst: Dest { reg: Reg::Temp(1), .. }, .. }));
    assert_eq!(p.temps, 2);
}

/// Predication: setp_gt p0, r0, r1; (p0.x) add r0, r1, r2; (!p0.x) add ...
#[test]
fn predicates() {
    let p = lifted(&[
        0xffff0201, 0x0301005e, 0xb00f1000, 0x80e40000, 0x80e40001, 0x14000002, 0x800f0000, 0xb0001000, 0x80e40001,
        0x80e40002, 0x14000002, 0x800f0000, 0xbd001000, 0x80e40001, 0x80e40002, END,
    ]);
    assert!(matches!(p.body[0], Stmt::SetPredicate { mask: 0xf, .. }));
    match (&p.body[1], &p.body[2]) {
        (Stmt::Assign { predicate: Some(a), .. }, Stmt::Assign { predicate: Some(b), .. }) => {
            assert_eq!(*a, Predicate { swizzle: [0; 4], negate: false });
            assert_eq!(*b, Predicate { swizzle: [0; 4], negate: true });
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn vface_and_ps_3_0_inputs() {
    // ps_3_0: dcl_texcoord2 v0.xy; dcl vFace; cmp oC0, vFace, v0, c0
    let p = lifted(&[
        0xffff0300, 0x0200001f, 0x80020005, 0x90030000, 0x0200001f, 0x80000000, 0x900f1001, 0x04000058, 0x800f0800,
        0x90e41001, 0x90e40000, 0xa0e40000, END,
    ]);
    assert!(p.uses_face);
    assert_eq!(p.inputs[0].semantic, Semantic::TexCoord(2));
}

#[test]
fn rejects_what_it_cannot_translate() {
    // ps_1_1 texbem t1, t0
    match lift_error(&[0xffff0101, 0x00000042, 0xb00f0000, 0x00000043, 0xb00f0001, 0xb0e40000, END]) {
        LiftError::Unsupported { opcode, .. } => assert_eq!(opcode, Opcode::TexBem),
        other => panic!("unexpected {other:?}"),
    }
    // ret inside if.
    assert!(matches!(
        lift_error(&[0xfffe0200, 0x01000028, 0xe0e40800, 0x0000001c, 0x0000002b, END]),
        LiftError::Unsupported { opcode: Opcode::Ret, .. }
    ));
}

#[test]
fn rejects_malformed_programs() {
    let reason = |tokens: &[u32]| match lift_error(tokens) {
        LiftError::Malformed { reason, .. } => reason,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(reason(&[0xfffe0200, 0x0000002a, END]), "else without if");
    assert_eq!(reason(&[0xfffe0200, 0x0000002b, END]), "endif without if");
    assert_eq!(reason(&[0xfffe0200, 0x01000028, 0xe0e40800, END]), "control flow not closed");
    assert_eq!(reason(&[0xfffe0201, 0x0000002c, END]), "break outside a loop");
    assert_eq!(reason(&[0xfffe0200, 0x01000019, 0xa0001000, END]), "call to an undefined label");
    // vs_1_1 mov r0, v0 without dcl.
    assert_eq!(reason(&[0xfffe0101, 0x00000001, 0x800f0000, 0x90e40000, END]), "vertex input read without dcl");
}

#[test]
fn errors_display() {
    let e = lift_error(&[0xfffe0200, 0x0000002a, END]);
    assert_eq!(e.to_string(), "token 1: else without if");
}

#[test]
fn semantic_names() {
    assert_eq!(Semantic::Color(1).name(), "color1");
    assert_eq!(Semantic::Target(0).name(), "color0");
    assert_eq!(Semantic::TexCoord(7).name(), "texcoord7");
    assert_eq!(Semantic::Generic { usage: Usage::Normal, index: 2 }.name(), "normal2");
    assert_eq!(Semantic::Fog.name(), "fog");
}

#[test]
fn operand_helpers() {
    let op = operand(Reg::Const(5), [3, 2, 1, 0]);
    assert_eq!(op.replicate(0).swizzle, [3; 4]);
    assert_eq!(op.replicate(3).swizzle, [0; 4]);
    assert_eq!(op.offset(2).map(|o| o.reg), Some(Reg::Const(7)));
    assert_eq!(operand(Reg::Input(0), XYZW).offset(1), None);
}

trait InputIndex {
    fn reg_input(&self) -> Option<usize>;
}

impl InputIndex for Operand {
    fn reg_input(&self) -> Option<usize> {
        match self.reg {
            Reg::Input(i) => Some(usize::from(i)),
            _ => None,
        }
    }
}
