//! Translation IR for Direct3D 9 shaders.
//!
//! [`lift`] turns a decoded [`d3d9_bytecode::Shader`] into a [`Program`] that
//! backends can translate without knowing D3D9's encoding quirks:
//!
//! - **Registers have meaning.** `t3` is a texture coordinate input in
//!   `ps_2_0` but a writable texture register in `ps_1_1`; `oT0`, `o0` and
//!   `v0` are resolved to [`Semantic`]s in [`Program::inputs`] and
//!   [`Program::outputs`].
//! - **Control flow is structured.** `if`/`else`/`endif`, `loop`, `rep` and
//!   `break` become nested [`Stmt`]s; subroutines (`call`/`label`) are inlined.
//! - **Input is bounded.** Bytecode is untrusted, so D3D9's nesting limits
//!   ([`MAX_IF_DEPTH`], [`MAX_LOOP_DEPTH`], [`MAX_CALL_DEPTH`]) and a statement
//!   budget ([`MAX_STATEMENTS`]) are enforced; a lifted program nests at most
//!   [`MAX_NESTING`] deep.
//! - **Instruction rules are explicit.** Scalar instructions read the source
//!   component D3D9 specifies, matrix instructions are expanded into dot
//!   products, and `texkill`, `texcoord` and the ps_1_x colour output follow
//!   the version-specific rules.
//!
//! The IR is register-based rather than SSA. It feeds the MSL backend, which
//! hands optimisation to the Metal compiler; an SSA form for the direct-AIR
//! tier can be derived from it later.

mod error;
mod lift;
mod program;

pub use error::LiftError;
pub use lift::{MAX_CALL_DEPTH, MAX_IF_DEPTH, MAX_LOOP_DEPTH, MAX_NESTING, MAX_STATEMENTS, lift};
pub use program::*;
