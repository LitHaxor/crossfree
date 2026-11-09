use crate::version::{ShaderKind, Version};

/// Shader instruction opcode. Values and names follow `D3DSIO_*`.
///
/// Comment blocks (`0xfffe`) and the end token (`0xffff`) are structural and
/// are not opcodes here.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
#[non_exhaustive]
pub enum Opcode {
    Nop = 0x00,
    Mov = 0x01,
    Add = 0x02,
    Sub = 0x03,
    Mad = 0x04,
    Mul = 0x05,
    Rcp = 0x06,
    Rsq = 0x07,
    Dp3 = 0x08,
    Dp4 = 0x09,
    Min = 0x0a,
    Max = 0x0b,
    Slt = 0x0c,
    Sge = 0x0d,
    Exp = 0x0e,
    Log = 0x0f,
    Lit = 0x10,
    Dst = 0x11,
    Lrp = 0x12,
    Frc = 0x13,
    M4x4 = 0x14,
    M4x3 = 0x15,
    M3x4 = 0x16,
    M3x3 = 0x17,
    M3x2 = 0x18,
    Call = 0x19,
    CallNz = 0x1a,
    Loop = 0x1b,
    Ret = 0x1c,
    EndLoop = 0x1d,
    Label = 0x1e,
    Dcl = 0x1f,
    Pow = 0x20,
    Crs = 0x21,
    Sgn = 0x22,
    Abs = 0x23,
    Nrm = 0x24,
    SinCos = 0x25,
    Rep = 0x26,
    EndRep = 0x27,
    If = 0x28,
    Ifc = 0x29,
    Else = 0x2a,
    EndIf = 0x2b,
    Break = 0x2c,
    BreakC = 0x2d,
    Mova = 0x2e,
    DefB = 0x2f,
    DefI = 0x30,
    TexCoord = 0x40,
    TexKill = 0x41,
    Tex = 0x42,
    TexBem = 0x43,
    TexBemL = 0x44,
    TexReg2Ar = 0x45,
    TexReg2Gb = 0x46,
    TexM3x2Pad = 0x47,
    TexM3x2Tex = 0x48,
    TexM3x3Pad = 0x49,
    TexM3x3Tex = 0x4a,
    TexM3x3Diff = 0x4b,
    TexM3x3Spec = 0x4c,
    TexM3x3VSpec = 0x4d,
    ExpP = 0x4e,
    LogP = 0x4f,
    Cnd = 0x50,
    Def = 0x51,
    TexReg2Rgb = 0x52,
    TexDp3Tex = 0x53,
    TexM3x2Depth = 0x54,
    TexDp3 = 0x55,
    TexM3x3 = 0x56,
    TexDepth = 0x57,
    Cmp = 0x58,
    Bem = 0x59,
    Dp2Add = 0x5a,
    Dsx = 0x5b,
    Dsy = 0x5c,
    TexLdd = 0x5d,
    SetP = 0x5e,
    TexLdl = 0x5f,
    BreakP = 0x60,
    Phase = 0xfffd,
}

/// Every opcode with its assembler mnemonic.
const OPCODES: &[(Opcode, &str)] = {
    use Opcode::*;
    &[
        (Nop, "nop"),
        (Mov, "mov"),
        (Add, "add"),
        (Sub, "sub"),
        (Mad, "mad"),
        (Mul, "mul"),
        (Rcp, "rcp"),
        (Rsq, "rsq"),
        (Dp3, "dp3"),
        (Dp4, "dp4"),
        (Min, "min"),
        (Max, "max"),
        (Slt, "slt"),
        (Sge, "sge"),
        (Exp, "exp"),
        (Log, "log"),
        (Lit, "lit"),
        (Dst, "dst"),
        (Lrp, "lrp"),
        (Frc, "frc"),
        (M4x4, "m4x4"),
        (M4x3, "m4x3"),
        (M3x4, "m3x4"),
        (M3x3, "m3x3"),
        (M3x2, "m3x2"),
        (Call, "call"),
        (CallNz, "callnz"),
        (Loop, "loop"),
        (Ret, "ret"),
        (EndLoop, "endloop"),
        (Label, "label"),
        (Dcl, "dcl"),
        (Pow, "pow"),
        (Crs, "crs"),
        (Sgn, "sgn"),
        (Abs, "abs"),
        (Nrm, "nrm"),
        (SinCos, "sincos"),
        (Rep, "rep"),
        (EndRep, "endrep"),
        (If, "if"),
        (Ifc, "ifc"),
        (Else, "else"),
        (EndIf, "endif"),
        (Break, "break"),
        (BreakC, "breakc"),
        (Mova, "mova"),
        (DefB, "defb"),
        (DefI, "defi"),
        (TexCoord, "texcoord"),
        (TexKill, "texkill"),
        (Tex, "tex"),
        (TexBem, "texbem"),
        (TexBemL, "texbeml"),
        (TexReg2Ar, "texreg2ar"),
        (TexReg2Gb, "texreg2gb"),
        (TexM3x2Pad, "texm3x2pad"),
        (TexM3x2Tex, "texm3x2tex"),
        (TexM3x3Pad, "texm3x3pad"),
        (TexM3x3Tex, "texm3x3tex"),
        (TexM3x3Diff, "texm3x3diff"),
        (TexM3x3Spec, "texm3x3spec"),
        (TexM3x3VSpec, "texm3x3vspec"),
        (ExpP, "expp"),
        (LogP, "logp"),
        (Cnd, "cnd"),
        (Def, "def"),
        (TexReg2Rgb, "texreg2rgb"),
        (TexDp3Tex, "texdp3tex"),
        (TexM3x2Depth, "texm3x2depth"),
        (TexDp3, "texdp3"),
        (TexM3x3, "texm3x3"),
        (TexDepth, "texdepth"),
        (Cmp, "cmp"),
        (Bem, "bem"),
        (Dp2Add, "dp2add"),
        (Dsx, "dsx"),
        (Dsy, "dsy"),
        (TexLdd, "texldd"),
        (SetP, "setp"),
        (TexLdl, "texldl"),
        (BreakP, "breakp"),
        (Phase, "phase"),
    ]
};

impl Opcode {
    /// Looks up the opcode for the low 16 bits of an instruction token.
    pub fn from_raw(raw: u16) -> Option<Self> {
        OPCODES.iter().find(|(op, _)| *op as u16 == raw).map(|(op, _)| *op)
    }

    /// The `D3DSIO_*` value.
    pub fn raw(self) -> u16 {
        self as u16
    }

    /// Base assembler mnemonic. Some opcodes are spelled differently depending
    /// on the shader version (`tex` / `texld`, `texcoord` / `texcrd`); the
    /// disassembler handles those.
    pub fn mnemonic(self) -> &'static str {
        OPCODES.iter().find(|(op, _)| *op == self).map_or("?", |(_, name)| *name)
    }
}

/// How many operands an opcode takes in a given shader version.
///
/// `dcl`, `def`, `defi` and `defb` are listed with one destination and no
/// sources; their extra tokens are read by the parser as a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) dst: u8,
    pub(crate) src: u8,
}

const fn ops(dst: u8, src: u8) -> Option<Layout> {
    Some(Layout { dst, src })
}

/// Operand layout of `op` in version `v`, or `None` if `v` does not have `op`.
pub(crate) fn layout(op: Opcode, v: Version) -> Option<Layout> {
    use Opcode::*;
    let sm2 = v.at_least(2, 0);
    let sm2x = v.at_least(2, 1);
    let sm3 = v.at_least(3, 0);
    match v.kind {
        ShaderKind::Vertex => match op {
            Nop => ops(0, 0),
            Mov | Rcp | Rsq | Exp | Log | ExpP | LogP | Lit | Frc => ops(1, 1),
            Add | Sub | Mul | Dp3 | Dp4 | Min | Max | Slt | Sge | Dst | M4x4 | M4x3 | M3x4 | M3x3 | M3x2 => ops(1, 2),
            Mad => ops(1, 3),
            Dcl | Def => ops(1, 0),
            Mova | Abs | Nrm if sm2 => ops(1, 1),
            Pow | Crs if sm2 => ops(1, 2),
            Lrp if sm2 => ops(1, 3),
            Sgn | SinCos if sm3 => ops(1, 1),
            Sgn | SinCos if sm2 => ops(1, 3),
            DefB | DefI if sm2 => ops(1, 0),
            Rep | If | Call | Label if sm2 => ops(0, 1),
            EndRep | Else | EndIf | Ret | EndLoop if sm2 => ops(0, 0),
            CallNz | Loop if sm2 => ops(0, 2),
            Ifc | BreakC if sm2x => ops(0, 2),
            Break if sm2x => ops(0, 0),
            BreakP if sm2x => ops(0, 1),
            SetP if sm2x => ops(1, 2),
            TexLdl if sm3 => ops(1, 2),
            _ => None,
        },
        ShaderKind::Pixel => match op {
            Nop => ops(0, 0),
            Mov => ops(1, 1),
            Add | Sub | Mul | Dp3 => ops(1, 2),
            Mad | Lrp => ops(1, 3),
            Def => ops(1, 0),
            // texkill encodes its operand as a destination in every version.
            TexKill => ops(1, 0),
            Dp4 if v.at_least(1, 2) => ops(1, 2),
            Cmp if v.at_least(1, 2) => ops(1, 3),
            Cnd if v.at_most(1, 4) => ops(1, 3),
            TexCoord if v.at_most(1, 3) => ops(1, 0),
            TexCoord if v.is(1, 4) => ops(1, 1),
            Tex if v.at_most(1, 3) => ops(1, 0),
            Tex if v.is(1, 4) => ops(1, 1),
            Tex if sm2 => ops(1, 2),
            TexBem | TexBemL | TexReg2Ar | TexReg2Gb | TexM3x2Pad | TexM3x2Tex | TexM3x3Pad | TexM3x3Tex
            | TexM3x3Diff | TexM3x3VSpec
                if v.at_most(1, 3) =>
            {
                ops(1, 1)
            }
            TexM3x3Spec if v.at_most(1, 3) => ops(1, 2),
            TexReg2Rgb | TexDp3Tex | TexDp3 | TexM3x3 if v.at_least(1, 2) && v.at_most(1, 3) => ops(1, 1),
            TexM3x2Depth if v.is(1, 3) => ops(1, 1),
            TexDepth if v.is(1, 4) => ops(1, 0),
            Bem if v.is(1, 4) => ops(1, 2),
            Phase if v.is(1, 4) => ops(0, 0),
            Rcp | Rsq | Abs | Exp | Log | Frc | Nrm if sm2 => ops(1, 1),
            Min | Max | Pow | Crs | M4x4 | M4x3 | M3x4 | M3x3 | M3x2 if sm2 => ops(1, 2),
            Dp2Add if sm2 => ops(1, 3),
            SinCos if sm3 => ops(1, 1),
            SinCos if sm2 => ops(1, 3),
            Dcl | DefB if sm2 => ops(1, 0),
            DefI if sm2x => ops(1, 0),
            Rep | If | Call | Label | BreakP if sm2x => ops(0, 1),
            EndRep | Else | EndIf | Break | Ret if sm2x => ops(0, 0),
            Ifc | BreakC | CallNz if sm2x => ops(0, 2),
            Dsx | Dsy if sm2x => ops(1, 1),
            TexLdd if sm2x => ops(1, 4),
            SetP if sm2x => ops(1, 2),
            Loop if sm3 => ops(0, 2),
            EndLoop if sm3 => ops(0, 0),
            TexLdl if sm3 => ops(1, 2),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(token: u32) -> Version {
        Version::from_token(token).unwrap()
    }

    #[test]
    fn raw_values_round_trip() {
        for (op, _) in OPCODES {
            assert_eq!(Opcode::from_raw(op.raw()), Some(*op));
        }
        assert_eq!(Opcode::from_raw(0x31), None);
        assert_eq!(Opcode::from_raw(0xfffe), None);
        assert_eq!(Opcode::from_raw(0xffff), None);
    }

    #[test]
    fn every_opcode_has_a_mnemonic() {
        for (op, name) in OPCODES {
            assert_eq!(op.mnemonic(), *name);
        }
    }

    #[test]
    fn layouts_depend_on_version() {
        let ps11 = version(0xffff_0101);
        let ps14 = version(0xffff_0104);
        let ps20 = version(0xffff_0200);
        let vs20 = version(0xfffe_0200);
        let vs30 = version(0xfffe_0300);

        assert_eq!(layout(Opcode::Tex, ps11), ops(1, 0));
        assert_eq!(layout(Opcode::Tex, ps14), ops(1, 1));
        assert_eq!(layout(Opcode::Tex, ps20), ops(1, 2));
        assert_eq!(layout(Opcode::TexCoord, ps14), ops(1, 1));
        assert_eq!(layout(Opcode::TexCoord, ps20), None);
        assert_eq!(layout(Opcode::SinCos, vs20), ops(1, 3));
        assert_eq!(layout(Opcode::SinCos, vs30), ops(1, 1));
        assert_eq!(layout(Opcode::Cnd, ps20), None);
        assert_eq!(layout(Opcode::Rcp, ps14), None);
        assert_eq!(layout(Opcode::TexLdl, ps20), None);
        assert_eq!(layout(Opcode::Phase, ps14), ops(0, 0));
        assert_eq!(layout(Opcode::Phase, ps11), None);
        assert_eq!(layout(Opcode::Mova, version(0xfffe_0101)), None);
    }
}
