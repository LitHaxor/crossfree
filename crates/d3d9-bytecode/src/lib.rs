//! Decoder for Direct3D 9 shader bytecode, shader models 1.x to 3.0.
//!
//! [`parse`] turns the token stream an application passes to
//! `IDirect3DDevice9::CreateVertexShader` / `CreatePixelShader` into a
//! [`Shader`]: its version, every instruction with decoded operands, and any
//! comment blocks (such as the compiler's `CTAB` constant table).
//!
//! Bytecode comes from untrusted programs, so malformed input is reported as an
//! [`Error`] and the decoder never panics.
//!
//! Formatting a [`Shader`] with `{}` prints Direct3D assembly, which the tests
//! use and which helps when debugging translation.

mod disasm;
mod error;
mod instruction;
mod opcode;
mod parse;
mod register;
mod version;

pub use error::{Error, ErrorKind};
pub use instruction::{Comment, Comparison, Declaration, Instruction, Payload, Shader, TextureType, Usage};
pub use opcode::Opcode;
pub use parse::{parse, parse_bytes};
pub use register::{
    DstModifiers, DstParam, Register, RegisterType, RelativeAddress, SrcModifier, SrcParam, Swizzle, WriteMask,
};
pub use version::{ShaderKind, Version};
