use std::fmt;

use crate::opcode::Opcode;
use crate::version::Version;

/// Why bytecode could not be decoded, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Token offset at which the problem was found.
    pub offset: usize,
    /// What went wrong.
    pub kind: ErrorKind,
}

impl Error {
    pub(crate) fn new(offset: usize, kind: ErrorKind) -> Self {
        Self { offset, kind }
    }
}

/// Kinds of decoding failure.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No tokens at all.
    Empty,
    /// Byte input whose length is not a multiple of 4.
    UnalignedLength(usize),
    /// The first token is not a vertex or pixel shader version.
    InvalidVersionToken(u32),
    /// A version Direct3D 9 does not define.
    UnsupportedVersion(Version),
    /// The bytecode ended before the end token.
    MissingEndToken,
    /// The bytecode ended inside an instruction or comment.
    UnexpectedEof,
    /// A parameter token (bit 31 set) where an instruction token was expected.
    UnexpectedParameterToken(u32),
    /// An operand token without bit 31 set.
    MissingParameterBit(u32),
    /// Unknown opcode value.
    InvalidOpcode(u16),
    /// The opcode exists but not in this shader version.
    OpcodeNotInVersion {
        /// The opcode.
        opcode: Opcode,
        /// The shader version.
        version: Version,
    },
    /// Predicated instruction in a version without predication.
    PredicationNotSupported,
    /// The instruction's length field disagrees with its operands.
    LengthMismatch {
        /// Tokens the length field announces.
        declared: usize,
        /// Tokens the operands took.
        actual: usize,
    },
    /// Unknown register type.
    InvalidRegisterType(u8),
    /// Unknown source modifier.
    InvalidSrcModifier(u8),
    /// Unknown destination modifier bits.
    InvalidDstModifier(u8),
    /// A relative-address token that does not name `a0` or `aL`.
    InvalidRelativeAddress(u32),
    /// Unknown semantic usage in a declaration.
    InvalidUsage(u8),
    /// Unknown texture type in a sampler declaration.
    InvalidTextureType(u8),
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Empty => write!(f, "no tokens"),
            ErrorKind::UnalignedLength(len) => {
                write!(f, "byte length {len} is not a multiple of 4")
            }
            ErrorKind::InvalidVersionToken(t) => write!(f, "invalid version token {t:#010x}"),
            ErrorKind::UnsupportedVersion(v) => write!(f, "unsupported shader version {v}"),
            ErrorKind::MissingEndToken => write!(f, "missing end token"),
            ErrorKind::UnexpectedEof => write!(f, "unexpected end of bytecode"),
            ErrorKind::UnexpectedParameterToken(t) => {
                write!(f, "expected an instruction, found parameter token {t:#010x}")
            }
            ErrorKind::MissingParameterBit(t) => {
                write!(f, "operand token {t:#010x} lacks the parameter bit")
            }
            ErrorKind::InvalidOpcode(op) => write!(f, "invalid opcode {op:#06x}"),
            ErrorKind::OpcodeNotInVersion { opcode, version } => {
                write!(f, "{} is not available in {version}", opcode.mnemonic())
            }
            ErrorKind::PredicationNotSupported => {
                write!(f, "predication requires shader model 2.0 or later")
            }
            ErrorKind::LengthMismatch { declared, actual } => {
                write!(f, "instruction length is {declared} but operands take {actual}")
            }
            ErrorKind::InvalidRegisterType(t) => write!(f, "invalid register type {t}"),
            ErrorKind::InvalidSrcModifier(m) => write!(f, "invalid source modifier {m}"),
            ErrorKind::InvalidDstModifier(m) => write!(f, "invalid destination modifier {m:#x}"),
            ErrorKind::InvalidRelativeAddress(t) => {
                write!(f, "invalid relative-address token {t:#010x}")
            }
            ErrorKind::InvalidUsage(u) => write!(f, "invalid declaration usage {u}"),
            ErrorKind::InvalidTextureType(t) => write!(f, "invalid sampler texture type {t}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "token {}: {}", self.offset, self.kind)
    }
}

impl std::error::Error for Error {}
