//! Emits Metal Shading Language (MSL) from the shader IR.
//!
//! # Binding contract with the runtime
//!
//! | Resource | Binding |
//! |---|---|
//! | Float constants `c#` | `[[buffer(0)]]`, `float4` array of [`Options::float_constants`] entries |
//! | Integer constants `i#` | `[[buffer(1)]]`, `int4` array of 16 |
//! | Boolean constants `b#` | `[[buffer(2)]]`, `uint` array of 16 (non-zero is true) |
//! | Runtime values | `[[buffer(3)]]`, [`RUNTIME_STRUCT`] |
//! | Sampler `s#` | `[[texture(#)]]` and `[[sampler(#)]]` |
//! | Vertex input `v#` | `[[attribute(#)]]`; vertex buffers start at [`FIRST_VERTEX_BUFFER`] |
//! | Vertex output → pixel input | `[[user(name)]]`, name from [`shader_ir::Semantic::name`] |
//! | Pixel output `oC#` | `[[color(#)]]`; `oDepth` is `[[depth(any)]]` |
//!
//! `def` constants are inlined where read directly. When a shader reads
//! float constants through relative addressing
//! ([`shader_ir::Constants::float_relative`]), the runtime must also write
//! the `def` values into buffer 0.
//!
//! ps_1_x shaders ([`shader_ir::Constants::clamp_floats`]) see float
//! constants clamped to [-1, 1], as D3D9 hardware (and wined3d) clamps them.
//! The emitted shader clamps every `c#` read itself and `def` values are
//! clamped when lifted, so the runtime uploads the application's values
//! unchanged.
//!
//! Input is bounded: [`emit`] rejects programs nesting deeper than
//! [`shader_ir::MAX_NESTING`] (lifted programs never do), and indentation is
//! capped so output size is linear in the number of statements.
//!
//! Vertex shaders apply the runtime's position fix-up last:
//! `position.xy = position.xy * pos_fixup.xy + pos_fixup.zw * position.w`.
//! It carries D3D9's half-pixel offset (pixel centres at integer
//! coordinates, where Metal uses .5).

mod emit;

pub use emit::{EmitError, Options, emit};

/// Buffer index of the float constants.
pub const FLOAT_CONSTANTS_BUFFER: u32 = 0;
/// Buffer index of the integer constants.
pub const INT_CONSTANTS_BUFFER: u32 = 1;
/// Buffer index of the boolean constants.
pub const BOOL_CONSTANTS_BUFFER: u32 = 2;
/// Buffer index of [`RUNTIME_STRUCT`].
pub const RUNTIME_BUFFER: u32 = 3;
/// First buffer index the runtime may use for vertex buffers.
pub const FIRST_VERTEX_BUFFER: u32 = 16;

/// Layout of the runtime-values buffer, as MSL.
pub const RUNTIME_STRUCT: &str = "struct RuntimeConstants\n{\n    float4 pos_fixup;\n};\n";
