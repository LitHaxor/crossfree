use crate::opcode::Opcode;
use crate::register::{DstParam, SrcParam};
use crate::version::Version;

/// A decoded shader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shader {
    /// Version from the first token.
    pub version: Version,
    /// Instructions in program order.
    pub instructions: Vec<Instruction>,
    /// Comment blocks in program order.
    pub comments: Vec<Comment>,
}

/// A comment block, such as the `CTAB` constant table the HLSL compiler emits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Token offset of the comment token.
    pub offset: usize,
    /// Contents, excluding the comment token itself.
    pub data: Vec<u32>,
}

impl Comment {
    /// The first four bytes, which tag the block (`b"CTAB"`, ...).
    pub fn fourcc(&self) -> Option<[u8; 4]> {
        self.data.first().map(|word| word.to_le_bytes())
    }
}

/// One decoded instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    /// Token offset of the instruction token.
    pub offset: usize,
    /// Operation.
    pub opcode: Opcode,
    /// Opcode-specific control bits (16–23): comparison for `ifc`/`breakc`/`setp`,
    /// project/bias for `texld`.
    pub control: u8,
    /// Co-issued with the previous instruction (ps_1_x `+` prefix).
    pub coissue: bool,
    /// Predicate guarding the instruction (`(p0.x) add ...`).
    pub predicate: Option<SrcParam>,
    /// Destination. For `texkill` this is the register tested, as encoded.
    pub dst: Option<DstParam>,
    /// Sources in operand order.
    pub src: Vec<SrcParam>,
    /// Extra data of `dcl` and `def*`.
    pub payload: Payload,
}

impl Instruction {
    /// Comparison of `ifc`, `breakc` and `setp`.
    pub fn comparison(&self) -> Option<Comparison> {
        match self.opcode {
            Opcode::Ifc | Opcode::BreakC | Opcode::SetP => Comparison::from_raw(self.control & 7),
            _ => None,
        }
    }
}

/// Extra data carried by declarations and constant definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Nothing.
    None,
    /// `dcl`.
    Declaration(Declaration),
    /// `def`: four IEEE-754 single-precision values, as raw bits.
    Float([u32; 4]),
    /// `defi`.
    Int([i32; 4]),
    /// `defb`.
    Bool(bool),
}

impl Payload {
    /// The `def` values as floats.
    pub fn float_values(&self) -> Option<[f32; 4]> {
        match self {
            Payload::Float(bits) => Some(bits.map(f32::from_bits)),
            _ => None,
        }
    }
}

/// What a `dcl` declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Declaration {
    /// An input or output with a semantic, e.g. `dcl_texcoord1 v1`.
    Semantic {
        /// Semantic usage.
        usage: Usage,
        /// Semantic index.
        index: u8,
    },
    /// A sampler and its texture type, e.g. `dcl_2d s0`.
    Sampler(TextureType),
}

/// Semantic usage (`D3DDECLUSAGE_*`).
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Usage {
    Position = 0,
    BlendWeight = 1,
    BlendIndices = 2,
    Normal = 3,
    PointSize = 4,
    TexCoord = 5,
    Tangent = 6,
    Binormal = 7,
    TessFactor = 8,
    PositionT = 9,
    Color = 10,
    Fog = 11,
    Depth = 12,
    Sample = 13,
}

impl Usage {
    /// Converts the 4-bit usage field.
    pub fn from_raw(raw: u8) -> Option<Self> {
        use Usage::*;
        Some(match raw {
            0 => Position,
            1 => BlendWeight,
            2 => BlendIndices,
            3 => Normal,
            4 => PointSize,
            5 => TexCoord,
            6 => Tangent,
            7 => Binormal,
            8 => TessFactor,
            9 => PositionT,
            10 => Color,
            11 => Fog,
            12 => Depth,
            13 => Sample,
            _ => return None,
        })
    }

    /// Assembler name, as in `dcl_texcoord`.
    pub fn name(self) -> &'static str {
        use Usage::*;
        match self {
            Position => "position",
            BlendWeight => "blendweight",
            BlendIndices => "blendindices",
            Normal => "normal",
            PointSize => "psize",
            TexCoord => "texcoord",
            Tangent => "tangent",
            Binormal => "binormal",
            TessFactor => "tessfactor",
            PositionT => "positiont",
            Color => "color",
            Fog => "fog",
            Depth => "depth",
            Sample => "sample",
        }
    }
}

/// Texture type of a sampler declaration (`D3DSTT_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureType {
    /// Not specified.
    Unknown,
    /// 2D texture.
    Texture2D,
    /// Cube map.
    Cube,
    /// Volume (3D) texture.
    Volume,
}

impl TextureType {
    /// Converts the 4-bit texture type field.
    pub fn from_raw(raw: u8) -> Option<Self> {
        Some(match raw {
            0 => TextureType::Unknown,
            2 => TextureType::Texture2D,
            3 => TextureType::Cube,
            4 => TextureType::Volume,
            _ => return None,
        })
    }
}

/// Comparison used by `ifc`, `breakc` and `setp` (`D3DSPC_*`).
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Comparison {
    Gt,
    Eq,
    Ge,
    Lt,
    Ne,
    Le,
}

impl Comparison {
    /// Converts the 3-bit comparison field.
    pub fn from_raw(raw: u8) -> Option<Self> {
        Some(match raw {
            1 => Comparison::Gt,
            2 => Comparison::Eq,
            3 => Comparison::Ge,
            4 => Comparison::Lt,
            5 => Comparison::Ne,
            6 => Comparison::Le,
            _ => return None,
        })
    }

    /// Assembler suffix, as in `ifc_gt`.
    pub fn suffix(self) -> &'static str {
        match self {
            Comparison::Gt => "gt",
            Comparison::Eq => "eq",
            Comparison::Ge => "ge",
            Comparison::Lt => "lt",
            Comparison::Ne => "ne",
            Comparison::Le => "le",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usages_round_trip() {
        for raw in 0..=13u8 {
            assert_eq!(Usage::from_raw(raw).unwrap() as u8, raw);
        }
        assert_eq!(Usage::from_raw(14), None);
    }

    #[test]
    fn texture_types() {
        assert_eq!(TextureType::from_raw(2), Some(TextureType::Texture2D));
        assert_eq!(TextureType::from_raw(1), None);
        assert_eq!(TextureType::from_raw(5), None);
    }

    #[test]
    fn comparisons() {
        assert_eq!(Comparison::from_raw(0), None);
        assert_eq!(Comparison::from_raw(7), None);
        let all: Vec<_> = (1..=6).map(|r| Comparison::from_raw(r).unwrap().suffix()).collect();
        assert_eq!(all, ["gt", "eq", "ge", "lt", "ne", "le"]);
    }

    #[test]
    fn comment_fourcc() {
        let c = Comment { offset: 1, data: vec![u32::from_le_bytes(*b"CTAB"), 0] };
        assert_eq!(c.fourcc(), Some(*b"CTAB"));
        assert_eq!(Comment { offset: 1, data: vec![] }.fourcc(), None);
    }
}
