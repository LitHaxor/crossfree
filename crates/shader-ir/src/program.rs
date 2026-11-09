use std::collections::{BTreeMap, BTreeSet};

pub use d3d9_bytecode::{Comparison, SrcModifier, TextureType, Usage, Version};

/// Pipeline stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Vertex shader.
    Vertex,
    /// Pixel (fragment) shader.
    Pixel,
}

/// A lifted shader.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// Stage.
    pub stage: Stage,
    /// Original shader version.
    pub version: Version,
    /// Inputs: vertex attributes, or interpolated values for pixel shaders.
    pub inputs: Vec<Signature>,
    /// Outputs: varyings for vertex shaders, render targets and depth for
    /// pixel shaders.
    pub outputs: Vec<Signature>,
    /// Samplers used, by sampler index.
    pub samplers: BTreeMap<u16, TextureType>,
    /// Constant registers.
    pub constants: Constants,
    /// Number of temporary registers (`r#`) referenced.
    pub temps: u16,
    /// Number of ps_1_0–1_3 texture registers (`t#`) referenced.
    pub texture_regs: u16,
    /// Whether `vPos` is read.
    pub uses_position: bool,
    /// Whether `vFace` is read.
    pub uses_face: bool,
    /// Whether the shader can discard pixels.
    pub discards: bool,
    /// Statements in program order.
    pub body: Vec<Stmt>,
}

/// One input or output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Signature {
    /// What the value means; also how vertex outputs link to pixel inputs.
    pub semantic: Semantic,
    /// D3D9 register number. For vertex inputs this is the attribute index.
    pub register: u16,
    /// Components read (inputs) or written (outputs).
    pub mask: u8,
    /// Interpolated at the centroid (`dcl_centroid`).
    pub centroid: bool,
}

/// Meaning of an input or output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Semantic {
    /// Clip-space position (vertex output).
    Position,
    /// Point size (vertex output).
    PointSize,
    /// Fog factor or coordinate.
    Fog,
    /// Diffuse (0) or specular (1) colour, or any `COLOR` semantic.
    Color(u8),
    /// Texture coordinate set.
    TexCoord(u8),
    /// A declared usage kept verbatim: all vertex inputs, and `vs_3_0`/`ps_3_0`
    /// varyings without a dedicated variant.
    Generic {
        /// Declared usage.
        usage: Usage,
        /// Declared usage index.
        index: u8,
    },
    /// Render target colour (pixel output `oC#`, or `r0` in ps_1_x).
    Target(u8),
    /// Depth (pixel output `oDepth`).
    Depth,
}

impl Semantic {
    /// Identifier used to link a vertex output to a pixel input.
    pub fn name(self) -> String {
        match self {
            Semantic::Position => "position".into(),
            Semantic::PointSize => "psize".into(),
            Semantic::Fog => "fog".into(),
            Semantic::Color(i) | Semantic::Target(i) => format!("color{i}"),
            Semantic::TexCoord(i) => format!("texcoord{i}"),
            Semantic::Generic { usage, index } => format!("{}{index}", usage.name()),
            Semantic::Depth => "depth".into(),
        }
    }

    pub(crate) fn from_usage(usage: Usage, index: u8) -> Self {
        match (usage, index) {
            (Usage::Position | Usage::PositionT, 0) => Semantic::Position,
            (Usage::PointSize, 0) => Semantic::PointSize,
            (Usage::Fog, 0) => Semantic::Fog,
            (Usage::Color, i) => Semantic::Color(i),
            (Usage::TexCoord, i) => Semantic::TexCoord(i),
            (usage, index) => Semantic::Generic { usage, index },
        }
    }
}

/// Constant registers read and defined by the shader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constants {
    /// `def` values by register. Applications cannot override these. Already
    /// clamped to [-1, 1] when [`Constants::clamp_floats`] is set.
    pub float_defs: BTreeMap<u16, [f32; 4]>,
    /// `defi` values by register.
    pub int_defs: BTreeMap<u16, [i32; 4]>,
    /// `defb` values by register.
    pub bool_defs: BTreeMap<u16, bool>,
    /// Float registers read directly (not through relative addressing).
    pub floats_read: BTreeSet<u16>,
    /// Whether float constants are read with relative addressing. The runtime
    /// must then merge `float_defs` into the uploaded constant buffer.
    pub float_relative: bool,
    /// Integer registers read.
    pub ints_read: BTreeSet<u16>,
    /// Boolean registers read.
    pub bools_read: BTreeSet<u16>,
    /// Float constants read `clamp(c#, -1, 1)` (ps_1_x, whose constant
    /// registers hold [-1, 1]). Backends must clamp values the application
    /// uploads; `float_defs` are clamped already.
    pub clamp_floats: bool,
}

/// A register after D3D9's version-dependent meanings are resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reg {
    /// Temporary `r#`.
    Temp(u16),
    /// Writable ps_1_0–1_3 texture register `t#`, once written. Until then
    /// `t#` reads resolve to the texture coordinate [`Reg::Input`].
    Texture(u16),
    /// Input, by index into [`Program::inputs`].
    Input(u16),
    /// Output, by index into [`Program::outputs`].
    Output(u16),
    /// Float constant `c#`.
    Const(u16),
    /// Integer constant `i#`.
    ConstInt(u16),
    /// Boolean constant `b#`.
    ConstBool(u16),
    /// Address register `a0` (integer vector).
    Addr,
    /// Loop counter `aL` (integer).
    Loop,
    /// Predicate `p0` (boolean vector).
    Predicate,
    /// Pixel position `vPos`.
    Position,
    /// Face register `vFace`: positive for front faces, negative for back.
    Face,
}

/// A source operand: register, optional relative index, swizzle, modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Operand {
    /// Register read.
    pub reg: Reg,
    /// Relative index register, for `c[a0.x + n]` and `c[aL + n]`.
    pub relative: Option<Relative>,
    /// Component selection (0 = x … 3 = w).
    pub swizzle: [u8; 4],
    /// Modifier applied after the swizzle.
    pub modifier: SrcModifier,
}

impl Operand {
    /// The operand with every component reading source component `slot` of
    /// the original swizzle, as D3D9 scalar instructions do.
    pub fn replicate(self, slot: usize) -> Self {
        let component = self.swizzle[slot & 3];
        Self { swizzle: [component; 4], ..self }
    }

    /// The operand with its register index advanced by `rows`, for matrix
    /// instructions. Only constant and temporary registers can be advanced.
    pub fn offset(self, rows: u16) -> Option<Self> {
        let reg = match self.reg {
            Reg::Const(i) => Reg::Const(i.checked_add(rows)?),
            Reg::Temp(i) => Reg::Temp(i.checked_add(rows)?),
            _ => return None,
        };
        Some(Self { reg, ..self })
    }
}

/// Index register for relative addressing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Relative {
    /// [`Reg::Addr`] or [`Reg::Loop`].
    pub reg: Reg,
    /// Component of `a0` used (ignored for `aL`).
    pub component: u8,
}

/// Destination register and write mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Dest {
    /// Register written.
    pub reg: Reg,
    /// Components written; bit 0 is x.
    pub mask: u8,
}

/// Per-component predicate guarding a write: component `i` of the
/// destination is written when `p0[swizzle[i]]` (negated if `negate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Predicate {
    /// Predicate component per destination component.
    pub swizzle: [u8; 4],
    /// `!p0`.
    pub negate: bool,
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `dst.mask = saturate(value * 2^shift)` (saturate and shift optional).
    Assign {
        /// Destination.
        dst: Dest,
        /// Value computed as a 4-component vector.
        value: Value,
        /// Clamp to [0, 1].
        saturate: bool,
        /// Scale by 2^shift before saturating (ps_1_x `_x2`, `_d2`, ...).
        shift: i8,
        /// Optional per-component predicate.
        predicate: Option<Predicate>,
    },
    /// `a0.mask = int(round or floor(src))`.
    LoadAddress {
        /// Components written.
        mask: u8,
        /// Value converted.
        src: Operand,
        /// Conversion.
        rounding: Rounding,
    },
    /// `p0.mask = compare(a, b)`, optionally predicated.
    SetPredicate {
        /// Components written.
        mask: u8,
        /// Comparison.
        comparison: Comparison,
        /// Left operand.
        a: Operand,
        /// Right operand.
        b: Operand,
        /// Optional per-component predicate.
        predicate: Option<Predicate>,
    },
    /// Discard the pixel if any selected component of `src` is negative.
    Kill {
        /// Value tested.
        src: Operand,
        /// Components tested.
        mask: u8,
    },
    /// `if (cond) { then } else { otherwise }`.
    If {
        /// Condition.
        cond: Cond,
        /// Taken branch.
        then: Vec<Stmt>,
        /// Other branch (may be empty).
        otherwise: Vec<Stmt>,
    },
    /// A counted loop.
    Loop {
        /// Loop kind and its integer constant.
        kind: LoopKind,
        /// Body.
        body: Vec<Stmt>,
    },
    /// Leave the innermost loop.
    Break,
    /// Leave the innermost loop if the condition holds.
    BreakIf(Cond),
}

/// How `a0` is loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rounding {
    /// `vs_1_1` `mov a0`: round towards negative infinity.
    Floor,
    /// `mova`: round to nearest.
    Nearest,
}

/// A boolean condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cond {
    /// Boolean constant `b#`.
    Bool(u16),
    /// Predicate component `p0.c`, optionally negated.
    Predicate {
        /// Component.
        component: u8,
        /// `!p0.c`.
        negate: bool,
    },
    /// Scalar comparison of the first components of two operands.
    Compare(Comparison, Operand, Operand),
}

/// Loop kinds. Both run at most 255 iterations, as D3D9 hardware does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopKind {
    /// `rep i#`: `i#.x` iterations.
    Rep {
        /// Integer constant register.
        count: u16,
    },
    /// `loop aL, i#`: `i#.x` iterations, `aL` starting at `i#.y`, stepping `i#.z`.
    Loop {
        /// Integer constant register.
        control: u16,
    },
}

/// A computed 4-component value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// One-operand operation, applied per component unless noted.
    Unary(UnaryOp, Operand),
    /// Two-operand operation.
    Binary(BinaryOp, Operand, Operand),
    /// Three-operand operation.
    Ternary(TernaryOp, Operand, Operand, Operand),
    /// Texture sample.
    Sample(Sample),
}

/// One-operand operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// Copy.
    Mov,
    /// `1 / x`.
    Rcp,
    /// `1 / sqrt(|x|)`.
    Rsq,
    /// `2^x`.
    Exp,
    /// `log2(|x|)`.
    Log,
    /// Shader model 1 `expp`: `(2^floor(x), x - floor(x), 2^x, 1)` of a scalar.
    ExpP1,
    /// `x - floor(x)`.
    Frc,
    /// `|x|`.
    Abs,
    /// Sign: -1, 0 or 1.
    Sgn,
    /// Normalise by the length of `xyz`; zero vectors pass through.
    Nrm,
    /// `lit`: lighting coefficients from `x`, `y` and `w`.
    Lit,
    /// `(cos(x), sin(x), _, _)` of a scalar.
    SinCos,
    /// Screen-space derivative along x (pixel shaders).
    Dsx,
    /// Screen-space derivative along y (pixel shaders).
    Dsy,
}

/// Two-operand operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `a + b`.
    Add,
    /// `a - b`.
    Sub,
    /// `a * b`.
    Mul,
    /// Minimum.
    Min,
    /// Maximum.
    Max,
    /// `a < b ? 1 : 0`.
    Slt,
    /// `a >= b ? 1 : 0`.
    Sge,
    /// 3-component dot product, replicated.
    Dp3,
    /// 4-component dot product, replicated.
    Dp4,
    /// `|a|^b` of scalars.
    Pow,
    /// Cross product of `xyz`.
    Crs,
    /// `dst`: `(1, a.y * b.y, a.z, b.w)`.
    Dst,
}

/// Three-operand operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TernaryOp {
    /// `a * b + c`.
    Mad,
    /// `a * (b - c) + c`.
    Lrp,
    /// `a >= 0 ? b : c` per component.
    Cmp,
    /// `a > 0.5 ? b : c` per component.
    Cnd,
    /// `a.x * b.x + a.y * b.y + c` (scalar `c`), replicated.
    Dp2Add,
}

/// A texture sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sample {
    /// Sampler index.
    pub sampler: u16,
    /// Texture coordinates.
    pub coord: Operand,
    /// Sampling mode.
    pub kind: SampleKind,
    /// Swizzle applied to the sampled value: the sampler register's swizzle
    /// (`texld r0, v0, s0.yzwx`), identity before shader model 2.
    pub swizzle: [u8; 4],
}

/// Sampling modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleKind {
    /// Implicit level of detail.
    Plain,
    /// Divide coordinates by `w` (`texldp`, ps_1_4 `_dw`).
    ProjectW,
    /// Divide coordinates by `z` (ps_1_4 `_dz`).
    ProjectZ,
    /// Level-of-detail bias from `coord.w` (`texldb`).
    Bias,
    /// Explicit level of detail from `coord.w` (`texldl`).
    Lod,
    /// Explicit gradients (`texldd`).
    Grad(Operand, Operand),
}
