//! Representative shaders as token streams, shared by the emitter tests.
//!
//! Each one exercises a group of features end to end (lift, then emit); the
//! golden test pins their MSL and the Metal test compiles it.

#![allow(dead_code)]

pub const END: u32 = 0x0000_ffff;

// Register files (`D3DSPR_*`).
pub const TEMP: u32 = 0;
pub const INPUT: u32 = 1;
pub const CONST: u32 = 2;
/// `a0` in vertex shaders, `t#` in pixel shaders.
pub const ADDR: u32 = 3;
pub const RASTOUT: u32 = 4;
pub const ATTROUT: u32 = 5;
pub const OUTPUT: u32 = 6;
pub const CONSTINT: u32 = 7;
pub const COLOROUT: u32 = 8;
pub const DEPTHOUT: u32 = 9;
pub const SAMPLER: u32 = 10;
pub const CONSTBOOL: u32 = 14;
pub const LOOP: u32 = 15;
pub const MISC: u32 = 17;
pub const LABEL: u32 = 18;
pub const PRED: u32 = 19;

// Source modifiers.
pub const BIAS: u32 = 2;
pub const SIGN: u32 = 4;
pub const COMP: u32 = 6;
pub const DZ: u32 = 9;
pub const DW: u32 = 10;
pub const NOT: u32 = 13;

/// Destination modifiers.
pub const SAT: u32 = 1;
pub const CENTROID: u32 = 4;

/// Relative-addressing bit of a source token.
pub const REL: u32 = 1 << 13;
/// Predicated bit of an instruction token.
pub const PREDICATED: u32 = 1 << 28;
/// Co-issue bit of an instruction token.
pub const COISSUE: u32 = 1 << 30;

pub fn f(x: f32) -> u32 {
    x.to_bits()
}

/// Swizzle bits from a string such as `"yzwx"`.
pub fn swz(s: &str) -> u32 {
    s.bytes()
        .enumerate()
        .map(|(i, c)| {
            let n = match c {
                b'x' => 0,
                b'y' => 1,
                b'z' => 2,
                _ => 3,
            };
            n << (2 * i)
        })
        .sum()
}

fn reg(ty: u32, index: u32) -> u32 {
    0x8000_0000 | ((ty & 7) << 28) | (((ty >> 3) & 3) << 11) | (index & 0x7ff)
}

pub fn dst(ty: u32, index: u32, mask: u32) -> u32 {
    reg(ty, index) | (mask << 16)
}

/// Destination with modifiers and a 4-bit shift (1 = `_x2`, 0xf = `_d2`).
pub fn dstm(ty: u32, index: u32, mask: u32, modifiers: u32, shift: u32) -> u32 {
    dst(ty, index, mask) | (modifiers << 20) | (shift << 24)
}

pub fn src(ty: u32, index: u32, swizzle: &str) -> u32 {
    reg(ty, index) | (swz(swizzle) << 16)
}

pub fn srcm(ty: u32, index: u32, swizzle: &str, modifier: u32) -> u32 {
    src(ty, index, swizzle) | (modifier << 24)
}

/// Shader model 2+ instruction: the token records the operand count.
pub fn op(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let mut t = vec![opcode | ((operands.len() as u32) << 24)];
    t.extend_from_slice(operands);
    t
}

/// Shader model 2+ instruction with control bits (comparison, texld flags).
pub fn opc(opcode: u32, control: u32, operands: &[u32]) -> Vec<u32> {
    let mut t = op(opcode, operands);
    t[0] |= control << 16;
    t
}

/// Shader model 2+ predicated instruction; `predicate` is the p0 source token.
pub fn opp(opcode: u32, dst: u32, predicate: u32, sources: &[u32]) -> Vec<u32> {
    let mut operands = vec![dst, predicate];
    operands.extend_from_slice(sources);
    let mut t = op(opcode, &operands);
    t[0] |= PREDICATED;
    t
}

/// vs_1_1: matrix transform, `mov a0` with a relative read, fog, scalar ops.
pub fn vs_1_1() -> Vec<u32> {
    let mut t = vec![0xfffe_0101];
    t.extend([0x1f, 0x8000_0000, dst(INPUT, 0, 0xf)]); // dcl_position v0
    t.extend([0x1f, 0x8000_000a, dst(INPUT, 1, 0xf)]); // dcl_color v1
    t.extend([0x51, dst(CONST, 8, 0xf), f(0.5), f(1.0), f(2.0), f(-1.0)]); // def c8
    t.extend([0x14, dst(RASTOUT, 0, 0xf), src(INPUT, 0, "xyzw"), src(CONST, 0, "xyzw")]); // m4x4 oPos, v0, c0
    t.extend([0x01, dst(ADDR, 0, 0x1), src(CONST, 4, "xxxx")]); // mov a0.x, c4.x
    t.extend([0x01, dst(ATTROUT, 0, 0xf), src(CONST, 5, "xyzw") | REL]); // mov oD0, c[a0.x + 5]
    t.extend([0x05, dst(OUTPUT, 0, 0xf), src(INPUT, 1, "xyzw"), src(CONST, 8, "xyzw")]); // mul oT0, v1, c8
    t.extend([0x01, dst(RASTOUT, 1, 0x1), src(CONST, 8, "wwww")]); // mov oFog, c8.w
    t.extend([0x06, dst(TEMP, 0, 0x1), src(CONST, 9, "xxxx")]); // rcp r0.x, c9.x
    t.extend([0x07, dst(TEMP, 0, 0x2), src(CONST, 9, "yyyy")]); // rsq r0.y, c9.y
    t.extend([0x4e, dst(TEMP, 1, 0xf), src(CONST, 9, "zzzz")]); // expp r1, c9.z
    t.extend([0x10, dst(TEMP, 2, 0xf), src(CONST, 10, "xyzw")]); // lit r2, c10
    t.extend([0x05, dst(OUTPUT, 1, 0xf), src(TEMP, 0, "xyzw"), src(TEMP, 1, "xyzw")]); // mul oT1, r0, r1
    t.extend([0x01, dst(ATTROUT, 1, 0xf), src(TEMP, 2, "xyzw")]); // mov oD1, r2
    t.push(END);
    t
}

/// ps_1_1: `texkill` before the register is written, `tex`, every ps_1_x
/// modifier, co-issue, `cnd`, clamped constants.
pub fn ps_1_1() -> Vec<u32> {
    let mut t = vec![0xffff_0101];
    t.extend([0x51, dst(CONST, 0, 0xf), f(2.0), f(-3.0), f(0.25), f(1.0)]); // def c0, 2, -3, 0.25, 1
    t.extend([0x41, dst(ADDR, 2, 0xf)]); // texkill t2
    t.extend([0x42, dst(ADDR, 0, 0xf)]); // tex t0
    t.extend([0x42, dst(ADDR, 1, 0xf)]); // tex t1
    // dp3_x2 r1, t0_bx2, t1_bx2
    t.extend([0x08, dstm(TEMP, 1, 0xf, 0, 1), srcm(ADDR, 0, "xyzw", SIGN), srcm(ADDR, 1, "xyzw", SIGN)]);
    // mad_d2_sat r0.rgb, t0_bias, 1 - r1, v0
    t.extend([
        0x04,
        dstm(TEMP, 0, 0x7, SAT, 0xf),
        srcm(ADDR, 0, "xyzw", BIAS),
        srcm(TEMP, 1, "xyzw", COMP),
        src(INPUT, 0, "xyzw"),
    ]);
    t.extend([COISSUE | 0x01, dst(TEMP, 0, 0x8), src(CONST, 1, "wwww")]); // +mov r0.a, c1.a
    // cnd r0, r0.a, r1, c0
    t.extend([0x50, dst(TEMP, 0, 0xf), src(TEMP, 0, "wwww"), src(TEMP, 1, "xyzw"), src(CONST, 0, "xyzw")]);
    t.push(END);
    t
}

/// ps_1_4 (Wine constant_clamp_ps_test plus texcrd/texld with _dz/_dw and a
/// phase).
pub fn ps_1_4() -> Vec<u32> {
    let mut t = vec![0xffff_0104];
    t.extend([0x51, dst(CONST, 2, 0xf), f(-0.5), f(1.25), f(2.0), f(1.0)]); // def c2, -0.5, 1.25, 2, 1
    t.extend([0x40, dst(TEMP, 0, 0x7), srcm(ADDR, 0, "xyzw", DZ)]); // texcrd r0.xyz, t0_dz
    t.extend([0x42, dst(TEMP, 1, 0xf), srcm(ADDR, 1, "xyzw", DW)]); // texld r1, t1_dw
    t.push(0xfffd); // phase
    t.extend([0x42, dst(TEMP, 2, 0xf), src(TEMP, 0, "xyzw")]); // texld r2, r0
    t.extend([0x01, dst(TEMP, 3, 0xf), src(CONST, 1, "xyzw")]); // mov r3, c1
    t.extend([0x02, dst(TEMP, 0, 0xf), src(TEMP, 3, "xyzw"), src(CONST, 2, "xyzw")]); // add r0, r3, c2
    // cnd r0, r1, r2, r0 (per component in ps_1_4)
    t.extend([0x50, dst(TEMP, 0, 0xf), src(TEMP, 1, "xyzw"), src(TEMP, 2, "xyzw"), src(TEMP, 0, "xyzw")]);
    t.push(END);
    t
}

/// ps_2_0: 2D and cube samplers, texld/texldp/texldb, a centroid input.
pub fn ps_2_0() -> Vec<u32> {
    let mut t = vec![0xffff_0200];
    t.extend(op(0x1f, &[0x9000_0000, dst(SAMPLER, 0, 0xf)])); // dcl_2d s0
    t.extend(op(0x1f, &[0x9800_0000, dst(SAMPLER, 1, 0xf)])); // dcl_cube s1
    t.extend(op(0x1f, &[0x8000_0000, dstm(ADDR, 0, 0xf, CENTROID, 0)])); // dcl_centroid t0
    t.extend(op(0x1f, &[0x8000_0000, dst(INPUT, 0, 0xf)])); // dcl v0
    t.extend(op(0x42, &[dst(TEMP, 0, 0xf), src(ADDR, 0, "xyzw"), src(SAMPLER, 0, "xyzw")])); // texld
    t.extend(opc(0x42, 1, &[dst(TEMP, 1, 0xf), src(ADDR, 0, "xyzw"), src(SAMPLER, 0, "xyzw")])); // texldp
    t.extend(opc(0x42, 2, &[dst(TEMP, 2, 0xf), src(ADDR, 0, "xyzw"), src(SAMPLER, 1, "xyzw")])); // texldb
    // mad r0, r1, r2, r0
    t.extend(op(0x04, &[dst(TEMP, 0, 0xf), src(TEMP, 1, "xyzw"), src(TEMP, 2, "xyzw"), src(TEMP, 0, "xyzw")]));
    // dp2add r0.w, r1, r2, c0.x
    t.extend(op(0x5a, &[dst(TEMP, 0, 0x8), src(TEMP, 1, "xyzw"), src(TEMP, 2, "xyzw"), src(CONST, 0, "xxxx")]));
    t.extend(op(0x05, &[dst(COLOROUT, 0, 0xf), src(TEMP, 0, "xyzw"), src(INPUT, 0, "xyzw")])); // mul oC0, r0, v0
    t.push(END);
    t
}

/// vs_3_0: mova and relative reads, loop with aL, rep, if/else on a `defb`,
/// setp and a predicated write, callnz into a subroutine, vertex texture
/// fetch, matrix to a declared output.
pub fn vs_3_0() -> Vec<u32> {
    let mut t = vec![0xfffe_0300];
    t.extend(op(0x30, &[dst(CONSTINT, 0, 0xf), 3, 0, 1, 0])); // defi i0, 3, 0, 1, 0
    t.extend(op(0x2f, &[dst(CONSTBOOL, 0, 0xf), 1])); // defb b0, true
    t.extend(op(0x1f, &[0x8000_0000, dst(INPUT, 0, 0xf)])); // dcl_position v0
    t.extend(op(0x1f, &[0x8000_0005, dst(INPUT, 1, 0xf)])); // dcl_texcoord v1
    t.extend(op(0x1f, &[0x8000_0000, dst(OUTPUT, 0, 0xf)])); // dcl_position o0
    t.extend(op(0x1f, &[0x8000_0005, dst(OUTPUT, 1, 0xf)])); // dcl_texcoord o1
    t.extend(op(0x1f, &[0x9000_0000, dst(SAMPLER, 0, 0xf)])); // dcl_2d s0
    t.extend(op(0x2e, &[dst(ADDR, 0, 0x1), src(INPUT, 1, "xxxx")])); // mova a0.x, v1.x
    // mov r0, c[a0.x + 10]
    t.extend(op(0x01, &[dst(TEMP, 0, 0xf), src(CONST, 10, "xyzw") | REL, src(ADDR, 0, "xxxx")]));
    t.extend(op(0x1b, &[src(LOOP, 0, "xyzw"), src(CONSTINT, 0, "xyzw")])); // loop aL, i0
    // add r0, r0, c[aL + 20]
    t.extend(op(0x02, &[dst(TEMP, 0, 0xf), src(TEMP, 0, "xyzw"), src(CONST, 20, "xyzw") | REL, src(LOOP, 0, "xxxx")]));
    t.extend(opc(0x2d, 1, &[src(TEMP, 0, "xxxx"), src(CONST, 1, "xxxx")])); // breakc_gt r0.x, c1.x
    t.extend(op(0x1d, &[])); // endloop
    t.extend(op(0x26, &[src(CONSTINT, 1, "xyzw")])); // rep i1
    t.extend(op(0x28, &[src(CONSTBOOL, 0, "xyzw")])); // if b0
    t.extend(op(0x05, &[dst(TEMP, 0, 0xf), src(TEMP, 0, "xyzw"), src(CONST, 2, "xyzw")])); // mul r0, r0, c2
    t.extend(op(0x2a, &[])); // else
    t.extend(op(0x02, &[dst(TEMP, 0, 0xf), src(TEMP, 0, "xyzw"), src(CONST, 3, "xyzw")])); // add r0, r0, c3
    t.extend(op(0x2b, &[])); // endif
    t.extend(op(0x27, &[])); // endrep
    t.extend(opc(0x5e, 4, &[dst(PRED, 0, 0xf), src(INPUT, 1, "xyzw"), src(CONST, 4, "xyzw")])); // setp_lt p0, v1, c4
    t.extend(opp(0x01, dst(TEMP, 0, 0xf), src(PRED, 0, "xyzw"), &[src(CONST, 5, "xyzw")])); // (p0) mov r0, c5
    t.extend(op(0x1a, &[src(LABEL, 0, "xyzw"), src(CONSTBOOL, 1, "xyzw")])); // callnz l0, b1
    t.extend(op(0x5f, &[dst(TEMP, 1, 0xf), src(INPUT, 1, "xyzw"), src(SAMPLER, 0, "xyzw")])); // texldl r1, v1, s0
    t.extend(op(0x14, &[dst(OUTPUT, 0, 0xf), src(INPUT, 0, "xyzw"), src(CONST, 40, "xyzw")])); // m4x4 o0, v0, c40
    t.extend(op(0x02, &[dst(OUTPUT, 1, 0xf), src(TEMP, 0, "xyzw"), src(TEMP, 1, "xyzw")])); // add o1, r0, r1
    t.extend(op(0x1c, &[])); // ret
    t.extend(op(0x1e, &[src(LABEL, 0, "xyzw")])); // label l0
    t.extend(op(0x25, &[dst(TEMP, 1, 0x3), src(TEMP, 0, "wwww")])); // sincos r1.xy, r0.w
    t.extend(op(0x02, &[dst(TEMP, 0, 0xf), src(TEMP, 0, "xyzw"), src(TEMP, 1, "xyzw")])); // add r0, r0, r1
    t.extend(op(0x1c, &[])); // ret
    t.push(END);
    t
}

/// ps_3_0: sampler swizzle, texldd on a volume, vPos, vFace, setp, if on the
/// predicate, rep with breakp, a predicated write, cmp, oDepth.
pub fn ps_3_0() -> Vec<u32> {
    let mut t = vec![0xffff_0300];
    t.extend(op(0x30, &[dst(CONSTINT, 0, 0xf), 4, 0, 0, 0])); // defi i0, 4, 0, 0, 0
    t.extend(op(0x1f, &[0x8000_0005, dst(INPUT, 0, 0xf)])); // dcl_texcoord v0
    t.extend(op(0x1f, &[0x8001_000a, dst(INPUT, 1, 0xf)])); // dcl_color1 v1
    t.extend(op(0x1f, &[0x8000_0000, dst(MISC, 0, 0x3)])); // dcl vPos.xy
    t.extend(op(0x1f, &[0x8000_0000, dst(MISC, 1, 0x1)])); // dcl vFace
    t.extend(op(0x1f, &[0x9000_0000, dst(SAMPLER, 0, 0xf)])); // dcl_2d s0
    t.extend(op(0x1f, &[0xa000_0000, dst(SAMPLER, 1, 0xf)])); // dcl_volume s1
    t.extend(op(0x42, &[dst(TEMP, 0, 0xf), src(INPUT, 0, "xyzw"), src(SAMPLER, 0, "yzwx")])); // texld r0, v0, s0.yzwx
    // texldd r1, v0, s1, v1, v1
    t.extend(op(
        0x5d,
        &[
            dst(TEMP, 1, 0xf),
            src(INPUT, 0, "xyzw"),
            src(SAMPLER, 1, "xyzw"),
            src(INPUT, 1, "xyzw"),
            src(INPUT, 1, "xyzw"),
        ],
    ));
    t.extend(opc(0x5e, 3, &[dst(PRED, 0, 0xf), src(INPUT, 1, "xyzw"), src(CONST, 0, "xyzw")])); // setp_ge p0, v1, c0
    t.extend(op(0x28, &[src(PRED, 0, "xxxx")])); // if p0.x
    t.extend(op(0x02, &[dst(TEMP, 0, 0xf), src(TEMP, 0, "xyzw"), src(MISC, 0, "xyxy")])); // add r0, r0, vPos.xyxy
    t.extend(op(0x2b, &[])); // endif
    t.extend(op(0x26, &[src(CONSTINT, 0, "xyzw")])); // rep i0
    t.extend(op(0x60, &[srcm(PRED, 0, "yyyy", NOT)])); // breakp !p0.y
    t.extend(op(0x05, &[dst(TEMP, 0, 0xf), src(TEMP, 0, "xyzw"), src(CONST, 1, "xyzw")])); // mul r0, r0, c1
    t.extend(op(0x27, &[])); // endrep
    t.extend(opp(0x01, dst(TEMP, 0, 0x4), srcm(PRED, 0, "zzzz", NOT), &[src(TEMP, 1, "zzzz")])); // (!p0.z) mov r0.z, r1.z
    // cmp oC0, vFace.x, r0, r1
    t.extend(op(0x58, &[dst(COLOROUT, 0, 0xf), src(MISC, 1, "xxxx"), src(TEMP, 0, "xyzw"), src(TEMP, 1, "xyzw")]));
    t.extend(op(0x01, &[dst(DEPTHOUT, 0, 0x1), src(TEMP, 0, "wwww")])); // mov oDepth, r0.w
    t.push(END);
    t
}

/// Every representative shader, by name.
pub fn all() -> Vec<(&'static str, Vec<u32>)> {
    vec![
        ("vs_1_1", vs_1_1()),
        ("ps_1_1", ps_1_1()),
        ("ps_1_4", ps_1_4()),
        ("ps_2_0", ps_2_0()),
        ("vs_3_0", vs_3_0()),
        ("ps_3_0", ps_3_0()),
    ]
}

/// Decodes, lifts and emits `tokens` with default options.
pub fn msl(tokens: &[u32]) -> String {
    let shader = d3d9_bytecode::parse(tokens).expect("decodes");
    let program = shader_ir::lift(&shader).unwrap_or_else(|e| panic!("lift failed: {e}"));
    msl_emitter::emit(&program, &msl_emitter::Options::new(&program)).unwrap_or_else(|e| panic!("emit failed: {e}"))
}
