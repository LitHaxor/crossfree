//! Direct3D assembly output, for tests and debugging.

use std::fmt::{self, Write};

use crate::instruction::{Declaration, Instruction, Payload, Shader, TextureType};
use crate::opcode::Opcode;
use crate::register::{DstParam, Register, RegisterType, RelativeAddress, SrcModifier, SrcParam, Swizzle, WriteMask};
use crate::version::{ShaderKind, Version};

const COMPONENTS: [char; 4] = ['x', 'y', 'z', 'w'];

impl fmt::Display for Shader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.version)?;
        for instruction in &self.instructions {
            writeln!(f, "{}", instruction.display(self.version))?;
        }
        Ok(())
    }
}

impl Instruction {
    /// Formats the instruction as assembly. Register names depend on the
    /// shader version, so it has to be supplied.
    pub fn display(&self, version: Version) -> impl fmt::Display + '_ {
        Disassembly { instruction: self, version }
    }
}

struct Disassembly<'a> {
    instruction: &'a Instruction,
    version: Version,
}

impl fmt::Display for Disassembly<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ins = self.instruction;
        let v = self.version;
        if ins.coissue {
            f.write_char('+')?;
        }
        if let Some(predicate) = &ins.predicate {
            write!(f, "({}) ", source(predicate, v))?;
        }
        f.write_str(&mnemonic(ins, v))?;
        if let Some(dst) = &ins.dst {
            f.write_str(&dst_suffixes(dst))?;
        }

        let mut operands = Vec::new();
        if let Some(dst) = &ins.dst {
            operands.push(destination(dst, v));
        }
        operands.extend(ins.src.iter().map(|s| source(s, v)));
        match ins.payload {
            Payload::Float(bits) => {
                operands.extend(bits.iter().map(|b| float(f32::from_bits(*b))));
            }
            Payload::Int(values) => operands.extend(values.iter().map(i32::to_string)),
            Payload::Bool(value) => operands.push(value.to_string()),
            Payload::None | Payload::Declaration(_) => {}
        }
        if !operands.is_empty() {
            write!(f, " {}", operands.join(", "))?;
        }
        Ok(())
    }
}

fn mnemonic(ins: &Instruction, v: Version) -> String {
    let pixel = v.kind == ShaderKind::Pixel;
    match ins.opcode {
        Opcode::Tex if pixel && v.at_least(1, 4) => match ins.control {
            1 => "texldp".into(),
            2 => "texldb".into(),
            _ => "texld".into(),
        },
        Opcode::TexCoord if pixel && v.is(1, 4) => "texcrd".into(),
        Opcode::Ifc | Opcode::BreakC | Opcode::SetP => match ins.comparison() {
            Some(c) => format!("{}_{}", ins.opcode.mnemonic(), c.suffix()),
            None => ins.opcode.mnemonic().into(),
        },
        Opcode::Dcl => {
            let mut name = String::from("dcl");
            match ins.payload {
                Payload::Declaration(Declaration::Sampler(ty)) => name.push_str(match ty {
                    TextureType::Unknown => "",
                    TextureType::Texture2D => "_2d",
                    TextureType::Cube => "_cube",
                    TextureType::Volume => "_volume",
                }),
                Payload::Declaration(Declaration::Semantic { usage, index }) => {
                    if shows_semantic(ins, v) {
                        name.push('_');
                        name.push_str(usage.name());
                        if index != 0 {
                            name.push_str(&index.to_string());
                        }
                    }
                }
                _ => {}
            }
            name
        }
        op => op.mnemonic().into(),
    }
}

/// Vertex shaders always name the semantic; pixel shaders only from 3.0, and
/// never for `vPos` / `vFace`.
fn shows_semantic(ins: &Instruction, v: Version) -> bool {
    let target = ins.dst.map(|d| d.register.ty);
    match v.kind {
        ShaderKind::Vertex => true,
        ShaderKind::Pixel => v.major >= 3 && target != Some(RegisterType::MiscType),
    }
}

fn dst_suffixes(dst: &DstParam) -> String {
    let mut s = String::new();
    match dst.shift {
        0 => {}
        1 => s.push_str("_x2"),
        2 => s.push_str("_x4"),
        3 => s.push_str("_x8"),
        -1 => s.push_str("_d2"),
        -2 => s.push_str("_d4"),
        -3 => s.push_str("_d8"),
        n => write!(s, "_shift{n}").unwrap_or(()),
    }
    if dst.modifiers.saturate {
        s.push_str("_sat");
    }
    if dst.modifiers.partial_precision {
        s.push_str("_pp");
    }
    if dst.modifiers.centroid {
        s.push_str("_centroid");
    }
    s
}

fn destination(dst: &DstParam, v: Version) -> String {
    let mut s = register(dst.register, dst.relative, v);
    if dst.write_mask != WriteMask::ALL {
        s.push('.');
        s.extend((0..4).filter(|&i| dst.write_mask.contains(i)).map(|i| COMPONENTS[usize::from(i)]));
    }
    s
}

fn source(src: &SrcParam, v: Version) -> String {
    let reg = register(src.register, src.relative, v);
    let swizzle = swizzle(src.swizzle);
    match src.modifier {
        SrcModifier::None => format!("{reg}{swizzle}"),
        SrcModifier::Neg => format!("-{reg}{swizzle}"),
        SrcModifier::Bias => format!("{reg}_bias{swizzle}"),
        SrcModifier::BiasNeg => format!("-{reg}_bias{swizzle}"),
        SrcModifier::Sign => format!("{reg}_bx2{swizzle}"),
        SrcModifier::SignNeg => format!("-{reg}_bx2{swizzle}"),
        SrcModifier::Comp => format!("1 - {reg}{swizzle}"),
        SrcModifier::X2 => format!("{reg}_x2{swizzle}"),
        SrcModifier::X2Neg => format!("-{reg}_x2{swizzle}"),
        SrcModifier::Dz => format!("{reg}_dz{swizzle}"),
        SrcModifier::Dw => format!("{reg}_dw{swizzle}"),
        SrcModifier::Abs => format!("{reg}_abs{swizzle}"),
        SrcModifier::AbsNeg => format!("-{reg}_abs{swizzle}"),
        SrcModifier::Not => format!("!{reg}{swizzle}"),
    }
}

fn swizzle(s: Swizzle) -> String {
    if s.is_identity() {
        return String::new();
    }
    let [x, y, z, w] = s.0.map(|c| COMPONENTS[usize::from(c & 3)]);
    if x == y && y == z && z == w { format!(".{x}") } else { format!(".{x}{y}{z}{w}") }
}

/// Register name split into a prefix and an index, so relative addressing
/// can be written as `c[a0.x + 3]`.
fn register_parts(reg: Register, v: Version) -> (&'static str, Option<u32>) {
    let index = u32::from(reg.index);
    let vertex = v.kind == ShaderKind::Vertex;
    match reg.ty {
        RegisterType::Temp => ("r", Some(index)),
        RegisterType::Input => ("v", Some(index)),
        RegisterType::Const => ("c", Some(index)),
        RegisterType::Const2 => ("c", Some(index + 2048)),
        RegisterType::Const3 => ("c", Some(index + 4096)),
        RegisterType::Const4 => ("c", Some(index + 6144)),
        RegisterType::Addr if vertex => ("a", Some(index)),
        RegisterType::Addr => ("t", Some(index)),
        RegisterType::RastOut => match index {
            0 => ("oPos", None),
            1 => ("oFog", None),
            2 => ("oPts", None),
            _ => ("oRast", Some(index)),
        },
        RegisterType::AttrOut => ("oD", Some(index)),
        RegisterType::Output if vertex && v.major < 3 => ("oT", Some(index)),
        RegisterType::Output => ("o", Some(index)),
        RegisterType::ConstInt => ("i", Some(index)),
        RegisterType::ColorOut => ("oC", Some(index)),
        RegisterType::DepthOut => ("oDepth", None),
        RegisterType::Sampler => ("s", Some(index)),
        RegisterType::ConstBool => ("b", Some(index)),
        RegisterType::Loop => ("aL", None),
        RegisterType::TempFloat16 => ("h", Some(index)),
        RegisterType::MiscType => match index {
            0 => ("vPos", None),
            1 => ("vFace", None),
            _ => ("vMisc", Some(index)),
        },
        RegisterType::Label => ("l", Some(index)),
        RegisterType::Predicate => ("p", Some(index)),
    }
}

fn register(reg: Register, relative: Option<RelativeAddress>, v: Version) -> String {
    let (prefix, index) = register_parts(reg, v);
    match (relative, index) {
        (None, Some(i)) => format!("{prefix}{i}"),
        (None, None) => prefix.to_owned(),
        (Some(rel), index) => {
            let address = match rel.register.ty {
                RegisterType::Loop => "aL".to_owned(),
                _ => format!("a{}.{}", rel.register.index, COMPONENTS[usize::from(rel.component & 3)]),
            };
            match index {
                Some(i) if i > 0 => format!("{prefix}[{address} + {i}]"),
                _ => format!("{prefix}[{address}]"),
            }
        }
    }
}

fn float(x: f32) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e9 { format!("{x:.1}") } else { format!("{x}") }
}
