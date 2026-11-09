use crate::error::{Error, ErrorKind};
use crate::instruction::{Comment, Declaration, Instruction, Payload, Shader, TextureType, Usage};
use crate::opcode::{Opcode, layout};
use crate::register::{
    DstModifiers, DstParam, Register, RegisterType, RelativeAddress, SrcModifier, SrcParam, Swizzle, WriteMask,
};
use crate::version::Version;

const END_TOKEN: u32 = 0x0000_ffff;
const COMMENT_OPCODE: u32 = 0xfffe;
const PARAMETER_BIT: u32 = 1 << 31;
const COISSUE_BIT: u32 = 1 << 30;
const PREDICATED_BIT: u32 = 1 << 28;
const RELATIVE_BIT: u32 = 1 << 13;

/// Decodes shader bytecode given as 32-bit tokens.
///
/// Decoding stops at the end token; anything after it is ignored, as the
/// Direct3D runtime does.
pub fn parse(tokens: &[u32]) -> Result<Shader, Error> {
    let mut reader = Reader { tokens, pos: 0 };
    let first = reader.next().ok_or(Error::new(0, ErrorKind::Empty))?;
    let version = Version::from_token(first).ok_or(Error::new(0, ErrorKind::InvalidVersionToken(first)))?;
    if !version.is_supported() {
        return Err(Error::new(0, ErrorKind::UnsupportedVersion(version)));
    }

    let mut shader = Shader { version, instructions: Vec::new(), comments: Vec::new() };
    loop {
        let offset = reader.pos;
        let token = reader.next().ok_or(Error::new(offset, ErrorKind::MissingEndToken))?;
        if token == END_TOKEN {
            return Ok(shader);
        }
        if token & PARAMETER_BIT != 0 {
            return Err(Error::new(offset, ErrorKind::UnexpectedParameterToken(token)));
        }
        if token & 0xffff == COMMENT_OPCODE {
            let len = ((token >> 16) & 0x7fff) as usize;
            let data = reader.take(len).ok_or(Error::new(offset, ErrorKind::UnexpectedEof))?;
            shader.comments.push(Comment { offset, data: data.to_vec() });
            continue;
        }
        let instruction = read_instruction(&mut reader, version, offset, token)?;
        shader.instructions.push(instruction);
    }
}

/// Decodes shader bytecode given as little-endian bytes.
pub fn parse_bytes(bytes: &[u8]) -> Result<Shader, Error> {
    if bytes.len() % 4 != 0 {
        return Err(Error::new(bytes.len() / 4, ErrorKind::UnalignedLength(bytes.len())));
    }
    let tokens: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    parse(&tokens)
}

struct Reader<'a> {
    tokens: &'a [u32],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn next(&mut self) -> Option<u32> {
        let token = *self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(token)
    }

    fn take(&mut self, len: usize) -> Option<&'a [u32]> {
        let end = self.pos.checked_add(len)?;
        let slice = self.tokens.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// Reads any token.
    fn raw(&mut self) -> Result<u32, Error> {
        let at = self.pos;
        self.next().ok_or(Error::new(at, ErrorKind::UnexpectedEof))
    }

    /// Reads an operand token, which must have the parameter bit set.
    fn parameter(&mut self) -> Result<u32, Error> {
        let at = self.pos;
        let token = self.raw()?;
        if token & PARAMETER_BIT == 0 {
            return Err(Error::new(at, ErrorKind::MissingParameterBit(token)));
        }
        Ok(token)
    }

    fn raw4(&mut self) -> Result<[u32; 4], Error> {
        Ok([self.raw()?, self.raw()?, self.raw()?, self.raw()?])
    }
}

fn read_instruction(
    reader: &mut Reader<'_>,
    version: Version,
    offset: usize,
    token: u32,
) -> Result<Instruction, Error> {
    let raw = token as u16;
    let opcode = Opcode::from_raw(raw).ok_or(Error::new(offset, ErrorKind::InvalidOpcode(raw)))?;
    let ops = layout(opcode, version).ok_or(Error::new(offset, ErrorKind::OpcodeNotInVersion { opcode, version }))?;
    let predicated = token & PREDICATED_BIT != 0;
    if predicated && version.major < 2 {
        return Err(Error::new(offset, ErrorKind::PredicationNotSupported));
    }

    let start = reader.pos;
    let mut instruction = Instruction {
        offset,
        opcode,
        control: (token >> 16) as u8,
        coissue: token & COISSUE_BIT != 0,
        predicate: None,
        dst: None,
        src: Vec::with_capacity(usize::from(ops.src)),
        payload: Payload::None,
    };

    match opcode {
        Opcode::Dcl => {
            let declaration_at = reader.pos;
            let declaration = reader.parameter()?;
            let dst = read_dst(reader, version)?;
            instruction.payload = Payload::Declaration(read_declaration(declaration, dst.register.ty, declaration_at)?);
            instruction.dst = Some(dst);
        }
        Opcode::Def => {
            instruction.dst = Some(read_dst(reader, version)?);
            instruction.payload = Payload::Float(reader.raw4()?);
        }
        Opcode::DefI => {
            instruction.dst = Some(read_dst(reader, version)?);
            instruction.payload = Payload::Int(reader.raw4()?.map(|v| v as i32));
        }
        Opcode::DefB => {
            instruction.dst = Some(read_dst(reader, version)?);
            instruction.payload = Payload::Bool(reader.raw()? != 0);
        }
        _ => {
            if ops.dst == 1 {
                instruction.dst = Some(read_dst(reader, version)?);
            }
            // The predicate sits between the destination and the sources.
            if predicated {
                instruction.predicate = Some(read_src(reader, version)?);
            }
            for _ in 0..ops.src {
                instruction.src.push(read_src(reader, version)?);
            }
        }
    }

    // Shader model 2+ records how many tokens follow the instruction token;
    // shader model 1 leaves the field zero and relies on the opcode table.
    if version.major >= 2 {
        let declared = ((token >> 24) & 0xf) as usize;
        let actual = reader.pos - start;
        if declared != actual {
            return Err(Error::new(offset, ErrorKind::LengthMismatch { declared, actual }));
        }
    }
    Ok(instruction)
}

fn read_register(token: u32, at: usize) -> Result<Register, Error> {
    let raw = (((token >> 28) & 0x7) | ((token >> 8) & 0x18)) as u8;
    let ty = RegisterType::from_raw(raw).ok_or(Error::new(at, ErrorKind::InvalidRegisterType(raw)))?;
    Ok(Register { ty, index: (token & 0x7ff) as u16 })
}

fn read_relative(reader: &mut Reader<'_>, version: Version, token: u32) -> Result<Option<RelativeAddress>, Error> {
    if token & RELATIVE_BIT == 0 {
        return Ok(None);
    }
    if version.major < 2 {
        // Shader model 1 has one address register and no address token.
        return Ok(Some(RelativeAddress { register: Register { ty: RegisterType::Addr, index: 0 }, component: 0 }));
    }
    let at = reader.pos;
    let address = reader.parameter()?;
    let register = read_register(address, at)?;
    if !matches!(register.ty, RegisterType::Addr | RegisterType::Loop) {
        return Err(Error::new(at, ErrorKind::InvalidRelativeAddress(address)));
    }
    Ok(Some(RelativeAddress { register, component: ((address >> 16) & 3) as u8 }))
}

fn read_dst(reader: &mut Reader<'_>, version: Version) -> Result<DstParam, Error> {
    let at = reader.pos;
    let token = reader.parameter()?;
    let register = read_register(token, at)?;
    let relative = read_relative(reader, version, token)?;
    let modifiers = ((token >> 20) & 0xf) as u8;
    if modifiers & !0x7 != 0 {
        return Err(Error::new(at, ErrorKind::InvalidDstModifier(modifiers)));
    }
    let shift = ((token >> 24) & 0xf) as i8;
    Ok(DstParam {
        register,
        relative,
        write_mask: WriteMask(((token >> 16) & 0xf) as u8),
        modifiers: DstModifiers {
            saturate: modifiers & 1 != 0,
            partial_precision: modifiers & 2 != 0,
            centroid: modifiers & 4 != 0,
        },
        // 4-bit two's complement: 1..=3 scale up, 13..=15 scale down.
        shift: if shift >= 8 { shift - 16 } else { shift },
    })
}

fn read_src(reader: &mut Reader<'_>, version: Version) -> Result<SrcParam, Error> {
    let at = reader.pos;
    let token = reader.parameter()?;
    let register = read_register(token, at)?;
    let relative = read_relative(reader, version, token)?;
    let raw_modifier = ((token >> 24) & 0xf) as u8;
    let modifier =
        SrcModifier::from_raw(raw_modifier).ok_or(Error::new(at, ErrorKind::InvalidSrcModifier(raw_modifier)))?;
    Ok(SrcParam { register, relative, swizzle: Swizzle::from_bits((token >> 16) as u8), modifier })
}

fn read_declaration(token: u32, target: RegisterType, at: usize) -> Result<Declaration, Error> {
    if target == RegisterType::Sampler {
        let raw = ((token >> 27) & 0xf) as u8;
        let ty = TextureType::from_raw(raw).ok_or(Error::new(at, ErrorKind::InvalidTextureType(raw)))?;
        return Ok(Declaration::Sampler(ty));
    }
    let raw = (token & 0xf) as u8;
    let usage = Usage::from_raw(raw).ok_or(Error::new(at, ErrorKind::InvalidUsage(raw)))?;
    Ok(Declaration::Semantic { usage, index: ((token >> 16) & 0xf) as u8 })
}
