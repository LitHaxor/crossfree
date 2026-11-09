//! Decoding tests. Token streams marked "Wine" are the encodings used by
//! Wine's d3d9 conformance tests (dlls/d3d9/tests), checked against the
//! assembly in their comments.

use d3d9_bytecode::{
    Comparison, Declaration, ErrorKind, Opcode, Payload, RegisterType, ShaderKind, SrcModifier, TextureType, Usage,
    parse, parse_bytes,
};

const END: u32 = 0x0000_ffff;

fn disasm(tokens: &[u32]) -> String {
    match parse(tokens) {
        Ok(shader) => shader.to_string(),
        Err(e) => panic!("failed to decode: {e}"),
    }
}

fn error(tokens: &[u32]) -> (usize, ErrorKind) {
    let e = parse(tokens).expect_err("decoding should fail");
    (e.offset, e.kind)
}

/// Wine `simple_vs`: vs_1_1 with a declaration and no instruction lengths.
const SIMPLE_VS: &[u32] = &[
    0xfffe0101, 0x0000001f, 0x80000000, 0x900f0000, 0x00000009, 0xc0010000, 0x90e40000, 0xa0e40000, 0x00000009,
    0xc0020000, 0x90e40000, 0xa0e40001, 0x00000009, 0xc0040000, 0x90e40000, 0xa0e40002, 0x00000009, 0xc0080000,
    0x90e40000, 0xa0e40003, END,
];

#[test]
fn vs_1_1_declaration_and_dp4() {
    assert_eq!(
        disasm(SIMPLE_VS),
        "vs_1_1\n\
         dcl_position v0\n\
         dp4 oPos.x, v0, c0\n\
         dp4 oPos.y, v0, c1\n\
         dp4 oPos.z, v0, c2\n\
         dp4 oPos.w, v0, c3\n"
    );
    let shader = parse(SIMPLE_VS).unwrap();
    assert_eq!(shader.version.kind, ShaderKind::Vertex);
    assert_eq!(
        shader.instructions[0].payload,
        Payload::Declaration(Declaration::Semantic { usage: Usage::Position, index: 0 })
    );
    assert_eq!(shader.instructions[1].offset, 4);
}

/// Wine `mova_test` (shortened): vs_2_0 relative addressing with an address token.
#[test]
fn vs_2_0_relative_addressing_uses_an_address_token() {
    let tokens = [
        0xfffe0200, 0x0200001f, 0x80000000, 0x900f0000, 0x05000051, 0xa00f0000, 0x3f800000, 0x00000000, 0x00000000,
        0x3f800000, 0x0200002e, 0xb0010000, 0xa0000007, 0x03000001, 0xd00f0000, 0xa0e42003, 0xb0000000, 0x02000001,
        0xc00f0000, 0x90e40000, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "vs_2_0\n\
         dcl_position v0\n\
         def c0, 1.0, 0.0, 0.0, 1.0\n\
         mova a0.x, c7.x\n\
         mov oD0, c[a0.x + 3]\n\
         mov oPos, v0\n"
    );
    let shader = parse(&tokens).unwrap();
    let src = shader.instructions[3].src[0];
    let rel = src.relative.expect("relative address");
    assert_eq!((rel.register.ty, rel.register.index, rel.component), (RegisterType::Addr, 0, 0));
    assert_eq!(shader.instructions[1].payload.float_values(), Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn vs_1_1_relative_addressing_has_no_address_token() {
    let tokens = [0xfffe0101, 0x00000001, 0xd00f0000, 0xa0e42003, END];
    assert_eq!(disasm(&tokens), "vs_1_1\nmov oD0, c[a0.x + 3]\n");
}

/// Wine loop test: ps_3_0 nested loops over aL with integer constants.
#[test]
fn ps_3_0_loops_and_integer_constants() {
    let tokens = [
        0xffff0300, 0x05000051, 0xa00f0000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x05000051, 0xa00f0001,
        0x3d000000, 0x00000000, 0x00000000, 0x00000000, 0x05000030, 0xf00f0000, 0x00000004, 0x00000000, 0x00000002,
        0x00000000, 0x02000001, 0x800f0000, 0xa0e40000, 0x0200001b, 0xf0e40800, 0xf0e40000, 0x0200001b, 0xf0e40800,
        0xf0e40000, 0x03000002, 0x800f0000, 0x80e40000, 0xa0e40001, 0x0000001d, 0x0000001d, 0x02000001, 0x800f0800,
        0x80e40000, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "ps_3_0\n\
         def c0, 0.0, 0.0, 0.0, 1.0\n\
         def c1, 0.03125, 0.0, 0.0, 0.0\n\
         defi i0, 4, 0, 2, 0\n\
         mov r0, c0\n\
         loop aL, i0\n\
         loop aL, i0\n\
         add r0, r0, c1\n\
         endloop\n\
         endloop\n\
         mov oC0, r0\n"
    );
    let shader = parse(&tokens).unwrap();
    assert_eq!(shader.instructions[2].payload, Payload::Int([4, 0, 2, 0]));
}

/// Wine projected-texture test: ps_2_0 sampler declaration, texld and texldp.
#[test]
fn ps_2_0_samplers_and_texld_variants() {
    let tokens = [
        0xffff0200, 0x0200001f, 0x90000000, 0xa00f0800, 0x0200001f, 0x80000000, 0xb00f0000, 0x05000051, 0xa00f0000,
        0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x02000001, 0x800f0001, 0xa0e40000, 0x03000042, 0x800f0000,
        0xb0e40000, 0xa0e40800, 0x03010042, 0x800f0001, 0xb0e40000, 0xa0e40800, 0x02000001, 0x80020000, 0x80000001,
        0x02000001, 0x800f0800, 0x80e40000, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "ps_2_0\n\
         dcl_2d s0\n\
         dcl t0\n\
         def c0, 0.0, 0.0, 0.0, 1.0\n\
         mov r1, c0\n\
         texld r0, t0, s0\n\
         texldp r1, t0, s0\n\
         mov r0.y, r1.x\n\
         mov oC0, r0\n"
    );
    let shader = parse(&tokens).unwrap();
    assert_eq!(shader.instructions[0].payload, Payload::Declaration(Declaration::Sampler(TextureType::Texture2D)));
}

/// Wine sincos test: vs_2_0 sincos takes three sources; swizzles.
#[test]
fn vs_2_0_sincos_and_swizzles() {
    let tokens = [
        0xfffe0200, 0x0200001f, 0x80000000, 0x900f0000, 0x05000051, 0xa00f0002, 0x40490fdb, 0x3f800000, 0x00000000,
        0x3f59999a, 0x03000005, 0x80010001, 0x90000000, 0xa0000002, 0x04000025, 0x80020000, 0x80000001, 0xa0e40000,
        0xa0e40001, 0x02000001, 0xc00d0000, 0x90e40000, 0x03000005, 0xc0020000, 0x80550000, 0xa0ff0002, 0x02000001,
        0xd00f0000, 0xa0a60002, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "vs_2_0\n\
         dcl_position v0\n\
         def c2, 3.1415927, 1.0, 0.0, 0.85\n\
         mul r1.x, v0.x, c2.x\n\
         sincos r0.y, r1.x, c0, c1\n\
         mov oPos.xzw, v0\n\
         mul oPos.y, r0.y, c2.w\n\
         mov oD0, c2.zyzz\n"
    );
}

/// Wine texkill tests in ps_1_1 (no lengths) and ps_2_0 (with lengths).
#[test]
fn texkill_encodes_its_operand_as_a_destination() {
    let ps11 = [
        0xffff0101, 0x00000051, 0xa00f0000, 0x3f800000, 0x00000000, 0x00000000, 0x3f800000, 0x00000041, 0xb00f0000,
        0x00000001, 0x800f0000, 0xa0e40000, END,
    ];
    assert_eq!(disasm(&ps11), "ps_1_1\ndef c0, 1.0, 0.0, 0.0, 1.0\ntexkill t0\nmov r0, c0\n");
    let ps20 = [
        0xffff0200, 0x0200001f, 0x80000000, 0xb00f0000, 0x01000041, 0xb00f0000, 0x02000001, 0x800f0800, 0xa0e40000, END,
    ];
    assert_eq!(disasm(&ps20), "ps_2_0\ndcl t0\ntexkill t0\nmov oC0, c0\n");
}

/// Wine `shader_code_11_coissue`: ps_1_1 texcoord and a co-issued cnd.
#[test]
fn ps_1_1_coissue() {
    let tokens = [
        0xffff0101, 0x00000051, 0xa00f0000, 0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000051, 0xa00f0003,
        0x3f000000, 0x3f000000, 0x3f000000, 0x00000000, 0x00000040, 0xb00f0000, 0x00000001, 0x800f0000, 0xb0e40000,
        0x00000003, 0x800f0000, 0x80e40000, 0xa0e40003, 0x00000002, 0x800f0001, 0xa0e40000, 0xa0e40000, 0x00000008,
        0x800f0001, 0x80e40000, 0x80e40001, 0x00000001, 0x80080000, 0x80ff0001, 0x40000050, 0x80070000, 0x80ff0000,
        0xa0e40001, 0xa0e40002, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "ps_1_1\n\
         def c0, 1.0, 0.0, 0.0, 0.0\n\
         def c3, 0.5, 0.5, 0.5, 0.0\n\
         texcoord t0\n\
         mov r0, t0\n\
         sub r0, r0, c3\n\
         add r1, c0, c0\n\
         dp3 r1, r0, r1\n\
         mov r0.w, r1.w\n\
         +cnd r0.xyz, r0.w, c1, c2\n"
    );
    assert!(parse(&tokens).unwrap().instructions[8].coissue);
}

/// Wine vFace test: ps_3_0 misc-type input declared without a semantic.
#[test]
fn ps_3_0_vface() {
    let tokens = [
        0xffff0300, 0x05000051, 0xa00f0000, 0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0x05000051, 0xa00f0001,
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x0200001f, 0x80000000, 0x900f1001, 0x02000001, 0x800f0001,
        0xa0e40001, 0x04000058, 0x800f0000, 0x90e41001, 0xa0e40000, 0x80e40001, 0x02000001, 0x800f0800, 0x80e40000,
        END,
    ];
    assert_eq!(
        disasm(&tokens),
        "ps_3_0\n\
         def c0, 0.0, 1.0, 0.0, 0.0\n\
         def c1, 1.0, 0.0, 0.0, 0.0\n\
         dcl vFace\n\
         mov r1, c1\n\
         cmp r0, vFace, c0, r1\n\
         mov oC0, r0\n"
    );
}

#[test]
fn ps_1_4_phase_texcrd_and_modifiers() {
    let tokens = [
        0xffff0104, 0x00000040, 0x80070000, 0xb0e40000, 0x00000042, 0x800f0001, 0xb9e40001, 0x0000fffd, 0x00000001,
        0x811f0000, 0x84e40001, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "ps_1_4\n\
         texcrd r0.xyz, t0\n\
         texld r1, t1_dz\n\
         phase\n\
         mov_x2_sat r0, r1_bx2\n"
    );
    let shader = parse(&tokens).unwrap();
    let mov = &shader.instructions[3];
    let dst = mov.dst.unwrap();
    assert_eq!((dst.shift, dst.modifiers.saturate), (1, true));
    assert_eq!(mov.src[0].modifier, SrcModifier::Sign);
}

#[test]
fn ps_2_x_predication() {
    let tokens = [
        0xffff0201, 0x0301005e, 0xb00f1000, 0x80e40000, 0x80e40001, 0x14000002, 0x800f0000, 0xb0001000, 0x80e40001,
        0x80e40002, 0x14000002, 0x800f0000, 0xbd001000, 0x80e40001, 0x80e40002, END,
    ];
    assert_eq!(
        disasm(&tokens),
        "ps_2_x\n\
         setp_gt p0, r0, r1\n\
         (p0.x) add r0, r1, r2\n\
         (!p0.x) add r0, r1, r2\n"
    );
    let shader = parse(&tokens).unwrap();
    assert_eq!(shader.instructions[0].comparison(), Some(Comparison::Gt));
    assert_eq!(shader.instructions[1].comparison(), None);
    assert_eq!(shader.instructions[1].predicate.unwrap().register.ty, RegisterType::Predicate);
    assert_eq!(shader.instructions[2].predicate.unwrap().modifier, SrcModifier::Not);
}

#[test]
fn comments_are_collected() {
    let ctab = u32::from_le_bytes(*b"CTAB");
    let tokens = [0xffff0200, 0x0002fffe, ctab, 0x1c, 0x02000001, 0x800f0800, 0xa0e40000, END];
    let shader = parse(&tokens).unwrap();
    assert_eq!(shader.comments.len(), 1);
    assert_eq!(shader.comments[0].offset, 1);
    assert_eq!(shader.comments[0].fourcc(), Some(*b"CTAB"));
    assert_eq!(shader.comments[0].data, vec![ctab, 0x1c]);
    assert_eq!(shader.instructions.len(), 1);
    assert_eq!(shader.instructions[0].opcode, Opcode::Mov);
}

#[test]
fn tokens_after_the_end_are_ignored() {
    let tokens = [0xfffe0101, END, 0xdead_beef];
    assert!(parse(&tokens).unwrap().instructions.is_empty());
}

#[test]
fn bytes_are_little_endian() {
    let bytes: Vec<u8> = SIMPLE_VS.iter().flat_map(|t| t.to_le_bytes()).collect();
    assert_eq!(parse_bytes(&bytes).unwrap(), parse(SIMPLE_VS).unwrap());
    let e = parse_bytes(&bytes[..5]).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnalignedLength(5));
}

#[test]
fn rejects_malformed_headers() {
    assert_eq!(error(&[]), (0, ErrorKind::Empty));
    assert_eq!(error(&[0x1234_5678]), (0, ErrorKind::InvalidVersionToken(0x1234_5678)));
    assert!(matches!(error(&[0xffff0105, END]), (0, ErrorKind::UnsupportedVersion(_))));
    assert_eq!(error(&[0xfffe0101]), (1, ErrorKind::MissingEndToken));
}

#[test]
fn rejects_malformed_instructions() {
    // Instruction runs past the end.
    assert_eq!(error(&[0xfffe0101, 0x00000001, 0xc00f0000]), (3, ErrorKind::UnexpectedEof));
    // Length field says 3, operands take 2.
    assert_eq!(
        error(&[0xffff0200, 0x03000001, 0x800f0800, 0xa0e40000, END]),
        (1, ErrorKind::LengthMismatch { declared: 3, actual: 2 })
    );
    assert_eq!(error(&[0xfffe0101, 0x00000031, END]), (1, ErrorKind::InvalidOpcode(0x31)));
    assert!(matches!(
        error(&[0xffff0200, 0x0300005f, 0x800f0000, 0xb0e40000, 0xa0e40800, END]),
        (1, ErrorKind::OpcodeNotInVersion { opcode: Opcode::TexLdl, .. })
    ));
    assert_eq!(error(&[0xffff0101, 0x10000001, 0x800f0000, 0xa0e40000, END]), (1, ErrorKind::PredicationNotSupported));
    assert_eq!(error(&[0xfffe0101, 0x80000000, END]), (1, ErrorKind::UnexpectedParameterToken(0x80000000)));
    assert_eq!(comment_overrun(), (1, ErrorKind::UnexpectedEof));
}

fn comment_overrun() -> (usize, ErrorKind) {
    error(&[0xffff0200, 0x0005fffe, 1, 2])
}

#[test]
fn rejects_malformed_operands() {
    assert_eq!(
        error(&[0xfffe0101, 0x00000001, 0x400f0000, 0x90e40000, END]),
        (2, ErrorKind::MissingParameterBit(0x400f0000))
    );
    assert_eq!(error(&[0xfffe0101, 0x00000001, 0xc00f0000, 0x9ee40000, END]), (3, ErrorKind::InvalidSrcModifier(14)));
    assert_eq!(error(&[0xfffe0101, 0x00000001, 0xc00f1000, 0x90e40000, END]), (2, ErrorKind::InvalidRegisterType(20)));
    assert_eq!(error(&[0xfffe0101, 0x00000001, 0x808f0000, 0x90e40000, END]), (2, ErrorKind::InvalidDstModifier(8)));
    assert_eq!(
        error(&[0xfffe0200, 0x03000001, 0xc00f0000, 0xa0e42003, 0x80000000, END]),
        (4, ErrorKind::InvalidRelativeAddress(0x80000000))
    );
    assert_eq!(error(&[0xfffe0200, 0x0200001f, 0x8000000e, 0x900f0000, END]), (2, ErrorKind::InvalidUsage(14)));
    assert_eq!(error(&[0xffff0200, 0x0200001f, 0x88000000, 0xa00f0800, END]), (2, ErrorKind::InvalidTextureType(1)));
}

#[test]
fn errors_describe_the_location() {
    let e = parse(&[0xfffe0101, 0x00000031, END]).unwrap_err();
    assert_eq!(e.to_string(), "token 1: invalid opcode 0x0031");
    let e = parse(&[0xffff0200, 0x0300005f, 0x800f0000, 0xb0e40000, 0xa0e40800, END]).unwrap_err();
    assert_eq!(e.to_string(), "token 1: texldl is not available in ps_2_0");
}

/// Truncating any valid shader must fail cleanly, never panic or succeed.
#[test]
fn every_truncation_fails_cleanly() {
    for len in 0..SIMPLE_VS.len() {
        assert!(parse(&SIMPLE_VS[..len]).is_err(), "prefix of {len} tokens decoded");
    }
}

/// Random token streams must never panic (a cheap stand-in for fuzzing).
#[test]
fn random_input_never_panics() {
    let versions = [0xfffe0101u32, 0xfffe0200, 0xfffe0300, 0xffff0101, 0xffff0104, 0xffff0200, 0xffff0300];
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..20_000 {
        let len = (next() % 48) as usize;
        let mut tokens = vec![versions[(next() % versions.len() as u64) as usize]];
        for _ in 0..len {
            let r = next();
            // Mix plausible instruction tokens with arbitrary parameter tokens.
            tokens.push(match r % 4 {
                0 => (r >> 8) as u32 & 0x0f00_00ff,
                1 => END,
                _ => (r >> 16) as u32 | 0x8000_0000,
            });
        }
        let _ = parse(&tokens);
    }
}
