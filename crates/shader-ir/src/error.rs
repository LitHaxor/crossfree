use std::fmt;

use d3d9_bytecode::Opcode;

/// Why a decoded shader could not be lifted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiftError {
    /// Valid D3D9 that this translator does not handle yet.
    Unsupported {
        /// Token offset of the instruction.
        offset: usize,
        /// The instruction.
        opcode: Opcode,
        /// What is missing.
        reason: &'static str,
    },
    /// Structurally invalid program, e.g. an `else` without `if`.
    Malformed {
        /// Token offset of the instruction.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },
}

impl fmt::Display for LiftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiftError::Unsupported { offset, opcode, reason } => {
                write!(f, "token {offset}: {} not supported: {reason}", opcode.mnemonic())
            }
            LiftError::Malformed { offset, reason } => write!(f, "token {offset}: {reason}"),
        }
    }
}

impl std::error::Error for LiftError {}
