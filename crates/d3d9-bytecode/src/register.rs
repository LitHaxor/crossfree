/// Register file of an operand (`D3DSPR_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RegisterType {
    /// Temporary `r#`.
    Temp = 0,
    /// Input `v#`.
    Input = 1,
    /// Float constant `c#` (0–2047).
    Const = 2,
    /// Address register `a0` in vertex shaders, texture register `t#` in pixel shaders.
    Addr = 3,
    /// Rasterizer output: `oPos` (0), `oFog` (1), `oPts` (2).
    RastOut = 4,
    /// Color output `oD#`.
    AttrOut = 5,
    /// Texture-coordinate output `oT#` before `vs_3_0`, generic output `o#` in `vs_3_0`.
    Output = 6,
    /// Integer constant `i#`.
    ConstInt = 7,
    /// Pixel color output `oC#`.
    ColorOut = 8,
    /// Pixel depth output `oDepth`.
    DepthOut = 9,
    /// Sampler `s#`.
    Sampler = 10,
    /// Float constants 2048–4095.
    Const2 = 11,
    /// Float constants 4096–6143.
    Const3 = 12,
    /// Float constants 6144–8191.
    Const4 = 13,
    /// Boolean constant `b#`.
    ConstBool = 14,
    /// Loop counter `aL`.
    Loop = 15,
    /// Half-precision temporary.
    TempFloat16 = 16,
    /// Miscellaneous input: `vPos` (0) and `vFace` (1).
    MiscType = 17,
    /// Subroutine label `l#`.
    Label = 18,
    /// Predicate `p0`.
    Predicate = 19,
}

impl RegisterType {
    /// Converts the 5-bit register type field.
    pub fn from_raw(raw: u8) -> Option<Self> {
        use RegisterType::*;
        Some(match raw {
            0 => Temp,
            1 => Input,
            2 => Const,
            3 => Addr,
            4 => RastOut,
            5 => AttrOut,
            6 => Output,
            7 => ConstInt,
            8 => ColorOut,
            9 => DepthOut,
            10 => Sampler,
            11 => Const2,
            12 => Const3,
            13 => Const4,
            14 => ConstBool,
            15 => Loop,
            16 => TempFloat16,
            17 => MiscType,
            18 => Label,
            19 => Predicate,
            _ => return None,
        })
    }
}

/// A register: file plus index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Register {
    /// Register file.
    pub ty: RegisterType,
    /// Index within the file (11 bits).
    pub index: u16,
}

/// Source component selection: for each result component, the index of the
/// source component it reads (0 = x … 3 = w).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Swizzle(pub [u8; 4]);

impl Swizzle {
    /// `.xyzw`.
    pub const IDENTITY: Self = Self([0, 1, 2, 3]);

    /// Decodes the 8-bit swizzle field (2 bits per component, x lowest).
    pub fn from_bits(bits: u8) -> Self {
        Self([bits & 3, (bits >> 2) & 3, (bits >> 4) & 3, (bits >> 6) & 3])
    }

    /// Whether this is `.xyzw`.
    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }
}

/// Destination components written; bit 0 is x.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WriteMask(pub u8);

impl WriteMask {
    /// `.xyzw`.
    pub const ALL: Self = Self(0xf);

    /// Whether component `index` (0 = x … 3 = w) is written.
    pub fn contains(self, index: u8) -> bool {
        index < 4 && self.0 & (1 << index) != 0
    }
}

/// Source operand modifier (`D3DSPSM_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SrcModifier {
    /// No modifier.
    None,
    /// `-r`.
    Neg,
    /// `r_bias`: r - 0.5.
    Bias,
    /// `-r_bias`.
    BiasNeg,
    /// `r_bx2`: 2r - 1.
    Sign,
    /// `-r_bx2`.
    SignNeg,
    /// `1 - r`.
    Comp,
    /// `r_x2`.
    X2,
    /// `-r_x2`.
    X2Neg,
    /// `r_dz`: divide by z (ps_1_4 texture coordinates).
    Dz,
    /// `r_dw`: divide by w (ps_1_4 texture coordinates).
    Dw,
    /// `r_abs`.
    Abs,
    /// `-r_abs`.
    AbsNeg,
    /// `!p`: predicate negation.
    Not,
}

impl SrcModifier {
    /// Converts the 4-bit source modifier field.
    pub fn from_raw(raw: u8) -> Option<Self> {
        use SrcModifier::*;
        Some(match raw {
            0 => None,
            1 => Neg,
            2 => Bias,
            3 => BiasNeg,
            4 => Sign,
            5 => SignNeg,
            6 => Comp,
            7 => X2,
            8 => X2Neg,
            9 => Dz,
            10 => Dw,
            11 => Abs,
            12 => AbsNeg,
            13 => Not,
            _ => return Option::None,
        })
    }
}

/// Destination result modifiers (`D3DSPDM_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DstModifiers {
    /// `_sat`: clamp the result to [0, 1].
    pub saturate: bool,
    /// `_pp`: partial precision is enough.
    pub partial_precision: bool,
    /// `_centroid`: interpolate at the centroid (declarations only).
    pub centroid: bool,
}

/// Register that supplies a relative (indirect) index: `a0.x` or `aL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RelativeAddress {
    /// `a0` (`RegisterType::Addr`) or `aL` (`RegisterType::Loop`).
    pub register: Register,
    /// Component of the address register used (0 = x … 3 = w).
    pub component: u8,
}

/// Destination operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DstParam {
    /// Register written.
    pub register: Register,
    /// Relative index, if the register is indexed indirectly.
    pub relative: Option<RelativeAddress>,
    /// Components written.
    pub write_mask: WriteMask,
    /// Result modifiers.
    pub modifiers: DstModifiers,
    /// Result scale as a power of two (ps_1_x): 1 = `_x2`, -1 = `_d2`, ...
    pub shift: i8,
}

/// Source operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SrcParam {
    /// Register read.
    pub register: Register,
    /// Relative index, if the register is indexed indirectly.
    pub relative: Option<RelativeAddress>,
    /// Component selection.
    pub swizzle: Swizzle,
    /// Modifier applied after reading.
    pub modifier: SrcModifier,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_types_round_trip() {
        for raw in 0..=19u8 {
            assert_eq!(RegisterType::from_raw(raw).unwrap() as u8, raw);
        }
        assert_eq!(RegisterType::from_raw(20), None);
        assert_eq!(RegisterType::from_raw(31), None);
    }

    #[test]
    fn swizzle_bits() {
        assert!(Swizzle::from_bits(0xe4).is_identity());
        assert_eq!(Swizzle::from_bits(0x00), Swizzle([0, 0, 0, 0]));
        assert_eq!(Swizzle::from_bits(0xa6), Swizzle([2, 1, 2, 2]));
    }

    #[test]
    fn write_mask_components() {
        let mask = WriteMask(0b1101);
        assert!(mask.contains(0) && !mask.contains(1) && mask.contains(2) && mask.contains(3));
        assert!(!mask.contains(4));
    }

    #[test]
    fn source_modifiers() {
        for raw in 0..=13u8 {
            assert!(SrcModifier::from_raw(raw).is_some());
        }
        assert_eq!(SrcModifier::from_raw(14), None);
        assert_eq!(SrcModifier::from_raw(13), Some(SrcModifier::Not));
    }
}
