use std::collections::{BTreeMap, BTreeSet, HashMap};

use d3d9_bytecode::{
    Declaration, DstParam, Instruction, Opcode, Payload, Register, RegisterType, Shader, ShaderKind, SrcModifier,
    SrcParam, TextureType,
};

use crate::error::LiftError;
use crate::program::*;

// Limits. Bytecode is untrusted: these keep lifting (and everything that walks
// the result recursively, including `Drop`) bounded in time, memory and stack.
// The nesting limits are D3D9's own (d3d9caps.h); shaders the D3D9 runtime
// accepts stay within them.

/// Deepest `call`/`callnz` nesting: the largest `StaticFlowControlDepth`
/// (`D3DVS20_MAX_STATICFLOWCONTROLDEPTH`, `D3DPS20_MAX_STATICFLOWCONTROLDEPTH`),
/// which shader model 3 requires. Recursion always exceeds it.
pub const MAX_CALL_DEPTH: u32 = 4;
/// Deepest `if`/`ifc` nesting within one function: the largest
/// `DynamicFlowControlDepth` (`D3DVS20_MAX_DYNAMICFLOWCONTROLDEPTH`,
/// `D3DPS20_MAX_DYNAMICFLOWCONTROLDEPTH`), which shader model 3 requires.
pub const MAX_IF_DEPTH: usize = 24;
/// Deepest `loop`/`rep` nesting within one function: the largest
/// `StaticFlowControlDepth` (4).
pub const MAX_LOOP_DEPTH: usize = 4;
/// Deepest [`Stmt`] nesting a lifted program can have: each of the main
/// program and [`MAX_CALL_DEPTH`] inlined calls contributes its `if`s and
/// loops plus the `if` a `callnz` wraps around the call.
pub const MAX_NESTING: usize = (MAX_IF_DEPTH + MAX_LOOP_DEPTH + 1) * (MAX_CALL_DEPTH as usize + 1);
/// Most statements a lifted program may hold, counting every inlined copy of a
/// subroutine: twice D3D9's largest instruction-slot count
/// (`D3DMAX30SHADERINSTRUCTIONS`, 32768).
pub const MAX_STATEMENTS: usize = 65_536;

const IDENTITY: [u8; 4] = [0, 1, 2, 3];

/// Lifts a decoded shader into the IR.
pub fn lift(shader: &Shader) -> Result<Program, LiftError> {
    let mut lifter = Lifter::new(shader);
    lifter.declare(&shader.instructions)?;
    let (main, labels) = split_subroutines(&shader.instructions);
    lifter.labels = labels;
    let mut body = lifter.block(main)?;

    if lifter.stage == Stage::Pixel && lifter.version.major < 2 {
        // ps_1_x has no colour output register: the result is what r0 holds.
        let target = lifter.output_for(Semantic::Target(0), 0);
        lifter.outputs[usize::from(target)].mask = 0xf;
        lifter.temps = lifter.temps.max(1);
        body.push(Stmt::Assign {
            dst: Dest { reg: Reg::Output(target), mask: 0xf },
            value: Value::Unary(UnaryOp::Mov, plain(Reg::Temp(0))),
            saturate: false,
            shift: 0,
            predicate: None,
        });
    }

    Ok(Program {
        stage: lifter.stage,
        version: lifter.version,
        inputs: lifter.inputs,
        outputs: lifter.outputs,
        samplers: lifter.samplers,
        constants: lifter.constants,
        temps: lifter.temps,
        texture_regs: lifter.texture_regs,
        uses_position: lifter.uses_position,
        uses_face: lifter.uses_face,
        discards: lifter.discards,
        body,
    })
}

/// Splits off subroutines: the main program ends at the first `label`, and
/// each `label l#` body runs to the next `label`.
fn split_subroutines(instructions: &[Instruction]) -> (&[Instruction], HashMap<u16, &[Instruction]>) {
    let starts: Vec<usize> =
        instructions.iter().enumerate().filter(|(_, ins)| ins.opcode == Opcode::Label).map(|(i, _)| i).collect();
    let main_end = starts.first().copied().unwrap_or(instructions.len());
    let mut labels = HashMap::new();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(instructions.len());
        if let Some(label) = instructions[start].src.first() {
            labels.insert(label.register.index, &instructions[start + 1..end]);
        }
    }
    (&instructions[..main_end], labels)
}

fn plain(reg: Reg) -> Operand {
    Operand { reg, relative: None, swizzle: IDENTITY, modifier: SrcModifier::None }
}

fn unsupported(ins: &Instruction, reason: &'static str) -> LiftError {
    LiftError::Unsupported { offset: ins.offset, opcode: ins.opcode, reason }
}

fn malformed(ins: &Instruction, reason: &'static str) -> LiftError {
    LiftError::Malformed { offset: ins.offset, reason }
}

fn src(ins: &Instruction, n: usize) -> Result<&SrcParam, LiftError> {
    ins.src.get(n).ok_or_else(|| malformed(ins, "missing source operand"))
}

fn dst(ins: &Instruction) -> Result<DstParam, LiftError> {
    ins.dst.ok_or_else(|| malformed(ins, "missing destination operand"))
}

fn const_index(register: Register) -> Option<u16> {
    let base = match register.ty {
        RegisterType::Const => 0,
        RegisterType::Const2 => 2048,
        RegisterType::Const3 => 4096,
        RegisterType::Const4 => 6144,
        _ => return None,
    };
    Some(base + register.index)
}

fn swizzle_mask(swizzle: [u8; 4]) -> u8 {
    swizzle.iter().fold(0, |mask, c| mask | 1 << (c & 3))
}

/// Fails if opening another `if` (or loop, if `is_loop`) inside `frames`
/// would nest deeper than D3D9 allows.
fn check_nesting(frames: &[Frame], is_loop: bool, ins: &Instruction) -> Result<(), LiftError> {
    let loops = frames.iter().filter(|f| matches!(f, Frame::Loop { .. })).count();
    let ifs = frames.len() - loops;
    if is_loop && loops >= MAX_LOOP_DEPTH {
        return Err(malformed(ins, "loop/rep nested deeper than D3D9 allows (4)"));
    }
    if !is_loop && ifs >= MAX_IF_DEPTH {
        return Err(malformed(ins, "if nested deeper than D3D9 allows (24)"));
    }
    Ok(())
}

/// A control-flow construct being built.
enum Frame {
    If { offset: usize, cond: Cond, then: Option<Vec<Stmt>>, outer: Vec<Stmt> },
    Loop { offset: usize, kind: LoopKind, outer: Vec<Stmt> },
}

struct Lifter<'a> {
    stage: Stage,
    version: Version,
    labels: HashMap<u16, &'a [Instruction]>,
    /// `dcl` semantics by (register type, index), with the centroid flag.
    decls: HashMap<(RegisterType, u16), (Declaration, bool)>,
    inputs: Vec<Signature>,
    input_keys: HashMap<(RegisterType, u16), u16>,
    outputs: Vec<Signature>,
    output_keys: HashMap<Semantic, u16>,
    samplers: BTreeMap<u16, TextureType>,
    constants: Constants,
    temps: u16,
    texture_regs: u16,
    /// ps_1_0–1_3 `t#` registers written so far, in program order (these
    /// versions have no flow control). Until written, `t#` reads the
    /// texture coordinate set #.
    textures_written: BTreeSet<u16>,
    uses_position: bool,
    uses_face: bool,
    discards: bool,
    call_depth: u32,
    /// Statements created so far, for [`MAX_STATEMENTS`].
    statements: usize,
}

impl<'a> Lifter<'a> {
    fn new(shader: &Shader) -> Self {
        Self {
            stage: match shader.version.kind {
                ShaderKind::Vertex => Stage::Vertex,
                ShaderKind::Pixel => Stage::Pixel,
            },
            version: shader.version,
            labels: HashMap::new(),
            decls: HashMap::new(),
            inputs: Vec::new(),
            input_keys: HashMap::new(),
            outputs: Vec::new(),
            output_keys: HashMap::new(),
            samplers: BTreeMap::new(),
            constants: Constants {
                clamp_floats: shader.version.kind == ShaderKind::Pixel && shader.version.major < 2,
                ..Constants::default()
            },
            temps: 0,
            texture_regs: 0,
            textures_written: BTreeSet::new(),
            uses_position: false,
            uses_face: false,
            discards: false,
            call_depth: 0,
            statements: 0,
        }
    }

    /// Appends `stmt` to `out`, enforcing [`MAX_STATEMENTS`].
    fn push(&mut self, out: &mut Vec<Stmt>, stmt: Stmt, ins: &Instruction) -> Result<(), LiftError> {
        self.statements += 1;
        if self.statements > MAX_STATEMENTS {
            return Err(unsupported(ins, "shader too large once subroutines are inlined"));
        }
        out.push(stmt);
        Ok(())
    }

    fn pixel(&self) -> bool {
        self.stage == Stage::Pixel
    }

    /// Pixel shaders before 1.4 treat `t#` as writable texture registers.
    fn legacy_textures(&self) -> bool {
        self.pixel() && !self.version.at_least(1, 4)
    }

    /// Records declarations and constant definitions, wherever they appear.
    fn declare(&mut self, instructions: &[Instruction]) -> Result<(), LiftError> {
        for ins in instructions {
            match (ins.opcode, ins.payload) {
                (Opcode::Dcl, Payload::Declaration(decl)) => {
                    let d = dst(ins)?;
                    match decl {
                        Declaration::Sampler(ty) => {
                            self.samplers.insert(d.register.index, ty);
                        }
                        Declaration::Semantic { .. } => {
                            self.decls.insert((d.register.ty, d.register.index), (decl, d.modifiers.centroid));
                            match d.register.ty {
                                RegisterType::Input | RegisterType::Addr if self.pixel() => {
                                    self.input(d.register.ty, d.register.index, ins)?;
                                }
                                RegisterType::Input => {
                                    self.input(d.register.ty, d.register.index, ins)?;
                                }
                                RegisterType::Output => {
                                    self.output(d.register.ty, d.register.index, ins)?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                (Opcode::Def, Payload::Float(bits)) => {
                    if let Some(index) = const_index(dst(ins)?.register) {
                        let mut values = bits.map(f32::from_bits);
                        if self.constants.clamp_floats {
                            // ps_1_x clamps constants, `def` ones included
                            // (wined3d shader.c, Wine's constant_clamp_ps_test).
                            values = values.map(|v| v.clamp(-1.0, 1.0));
                        }
                        self.constants.float_defs.insert(index, values);
                    }
                }
                (Opcode::DefI, Payload::Int(values)) => {
                    self.constants.int_defs.insert(dst(ins)?.register.index, values);
                }
                (Opcode::DefB, Payload::Bool(value)) => {
                    self.constants.bool_defs.insert(dst(ins)?.register.index, value);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn input(&mut self, ty: RegisterType, index: u16, ins: &Instruction) -> Result<u16, LiftError> {
        if let Some(&slot) = self.input_keys.get(&(ty, index)) {
            return Ok(slot);
        }
        let decl = self.decls.get(&(ty, index)).copied();
        let centroid = decl.is_some_and(|(_, centroid)| centroid);
        let usage = match decl {
            Some((Declaration::Semantic { usage, index }, _)) => Some((usage, index)),
            _ => None,
        };
        let small = u8::try_from(index).map_err(|_| malformed(ins, "input register out of range"))?;
        let semantic = match (self.stage, ty) {
            // Vertex inputs keep the declared usage verbatim for attribute binding.
            (Stage::Vertex, RegisterType::Input) => match usage {
                Some((usage, index)) => Semantic::Generic { usage, index },
                None => return Err(malformed(ins, "vertex input read without dcl")),
            },
            (Stage::Pixel, RegisterType::Input) if self.version.major < 3 => Semantic::Color(small),
            (Stage::Pixel, RegisterType::Input) => match usage {
                Some((usage, index)) => Semantic::from_usage(usage, index),
                None => return Err(malformed(ins, "ps_3_0 input read without dcl")),
            },
            (Stage::Pixel, RegisterType::Addr) => Semantic::TexCoord(small),
            _ => return Err(malformed(ins, "register is not an input")),
        };
        let slot = self.inputs.len() as u16;
        self.inputs.push(Signature { semantic, register: index, mask: 0, centroid });
        self.input_keys.insert((ty, index), slot);
        Ok(slot)
    }

    fn output(&mut self, ty: RegisterType, index: u16, ins: &Instruction) -> Result<u16, LiftError> {
        let small = u8::try_from(index).map_err(|_| malformed(ins, "output register out of range"))?;
        let semantic = match (self.stage, ty) {
            (Stage::Vertex, RegisterType::RastOut) => match index {
                0 => Semantic::Position,
                1 => Semantic::Fog,
                2 => Semantic::PointSize,
                _ => return Err(malformed(ins, "invalid rasterizer output")),
            },
            (Stage::Vertex, RegisterType::AttrOut) => Semantic::Color(small),
            (Stage::Vertex, RegisterType::Output) if self.version.major < 3 => Semantic::TexCoord(small),
            (Stage::Vertex, RegisterType::Output) => match self.decls.get(&(ty, index)) {
                Some((Declaration::Semantic { usage, index }, _)) => Semantic::from_usage(*usage, *index),
                _ => return Err(malformed(ins, "vs_3_0 output written without dcl")),
            },
            (Stage::Pixel, RegisterType::ColorOut) => Semantic::Target(small),
            (Stage::Pixel, RegisterType::DepthOut) => Semantic::Depth,
            _ => return Err(malformed(ins, "register is not an output")),
        };
        let centroid = self.decls.get(&(ty, index)).is_some_and(|(_, c)| *c);
        let slot = self.output_for(semantic, index);
        self.outputs[usize::from(slot)].centroid |= centroid;
        Ok(slot)
    }

    fn output_for(&mut self, semantic: Semantic, register: u16) -> u16 {
        if let Some(&slot) = self.output_keys.get(&semantic) {
            return slot;
        }
        let slot = self.outputs.len() as u16;
        self.outputs.push(Signature { semantic, register, mask: 0, centroid: false });
        self.output_keys.insert(semantic, slot);
        slot
    }

    fn sampler(&mut self, register: Register, ins: &Instruction) -> Result<u16, LiftError> {
        if register.ty != RegisterType::Sampler {
            return Err(malformed(ins, "expected a sampler register"));
        }
        self.samplers.entry(register.index).or_insert(TextureType::Unknown);
        Ok(register.index)
    }

    /// A register as a source.
    fn source_reg(&mut self, register: Register, ins: &Instruction) -> Result<Reg, LiftError> {
        let index = register.index;
        Ok(match register.ty {
            RegisterType::Temp => {
                self.temps = self.temps.max(index + 1);
                Reg::Temp(index)
            }
            RegisterType::Input => Reg::Input(self.input(register.ty, index, ins)?),
            RegisterType::Const | RegisterType::Const2 | RegisterType::Const3 | RegisterType::Const4 => {
                Reg::Const(const_index(register).unwrap_or(index))
            }
            RegisterType::Addr if !self.pixel() => Reg::Addr,
            RegisterType::Addr if self.legacy_textures() && self.textures_written.contains(&index) => {
                self.texture_regs = self.texture_regs.max(index + 1);
                Reg::Texture(index)
            }
            // ps_1_0–1_3 `t#` not yet written holds texture coordinate set #,
            // as in ps_1_4 and ps_2_x (wined3d initialises `T#` from the
            // varying; vkd3d ir.c "lower_texture_writes"). Later versions
            // always read the varying.
            RegisterType::Addr => Reg::Input(self.input(register.ty, index, ins)?),
            RegisterType::ConstInt => {
                self.constants.ints_read.insert(index);
                Reg::ConstInt(index)
            }
            RegisterType::ConstBool => {
                self.constants.bools_read.insert(index);
                Reg::ConstBool(index)
            }
            RegisterType::Loop => Reg::Loop,
            RegisterType::Predicate => Reg::Predicate,
            RegisterType::MiscType if index == 0 => {
                self.uses_position = true;
                Reg::Position
            }
            RegisterType::MiscType if index == 1 => {
                self.uses_face = true;
                Reg::Face
            }
            _ => return Err(unsupported(ins, "register file cannot be read here")),
        })
    }

    fn read(&mut self, param: &SrcParam, ins: &Instruction) -> Result<Operand, LiftError> {
        let reg = self.source_reg(param.register, ins)?;
        let relative = match param.relative {
            None => None,
            Some(_) if !matches!(reg, Reg::Const(_)) => {
                return Err(unsupported(ins, "relative addressing of non-constant registers"));
            }
            Some(rel) => {
                self.constants.float_relative = true;
                Some(Relative {
                    reg: if rel.register.ty == RegisterType::Loop { Reg::Loop } else { Reg::Addr },
                    component: rel.component,
                })
            }
        };
        match (reg, relative) {
            (Reg::Const(i), None) => {
                self.constants.floats_read.insert(i);
            }
            (Reg::Input(i), _) => self.inputs[usize::from(i)].mask |= swizzle_mask(param.swizzle.0),
            _ => {}
        }
        Ok(Operand { reg, relative, swizzle: param.swizzle.0, modifier: param.modifier })
    }

    fn read_src(&mut self, ins: &Instruction, n: usize) -> Result<Operand, LiftError> {
        let param = *src(ins, n)?;
        self.read(&param, ins)
    }

    fn write(&mut self, d: &DstParam, ins: &Instruction) -> Result<Dest, LiftError> {
        if d.relative.is_some() {
            return Err(unsupported(ins, "relative addressing of destinations"));
        }
        let index = d.register.index;
        let reg = match d.register.ty {
            RegisterType::Temp => {
                self.temps = self.temps.max(index + 1);
                Reg::Temp(index)
            }
            RegisterType::Addr if self.legacy_textures() => {
                // From here on `t#` is a plain register, not the varying.
                self.textures_written.insert(index);
                self.texture_regs = self.texture_regs.max(index + 1);
                Reg::Texture(index)
            }
            RegisterType::RastOut
            | RegisterType::AttrOut
            | RegisterType::Output
            | RegisterType::ColorOut
            | RegisterType::DepthOut => {
                let slot = self.output(d.register.ty, index, ins)?;
                self.outputs[usize::from(slot)].mask |= d.write_mask.0;
                Reg::Output(slot)
            }
            RegisterType::Predicate => Reg::Predicate,
            _ => return Err(unsupported(ins, "register file cannot be written here")),
        };
        Ok(Dest { reg, mask: d.write_mask.0 })
    }

    fn predicate(&self, ins: &Instruction) -> Option<Predicate> {
        ins.predicate.map(|p| Predicate { swizzle: p.swizzle.0, negate: p.modifier == SrcModifier::Not })
    }

    fn assign(&mut self, ins: &Instruction, value: Value) -> Result<Stmt, LiftError> {
        let d = dst(ins)?;
        Ok(Stmt::Assign {
            dst: self.write(&d, ins)?,
            value,
            saturate: d.modifiers.saturate,
            shift: d.shift,
            predicate: self.predicate(ins),
        })
    }

    fn condition(&mut self, param: &SrcParam, ins: &Instruction) -> Result<Cond, LiftError> {
        match param.register.ty {
            RegisterType::ConstBool => {
                self.constants.bools_read.insert(param.register.index);
                Ok(Cond::Bool(param.register.index))
            }
            RegisterType::Predicate => {
                Ok(Cond::Predicate { component: param.swizzle.0[0], negate: param.modifier == SrcModifier::Not })
            }
            _ => Err(malformed(ins, "condition must be a boolean constant or the predicate")),
        }
    }

    fn compare(&mut self, ins: &Instruction) -> Result<Cond, LiftError> {
        let comparison = ins.comparison().ok_or_else(|| malformed(ins, "invalid comparison"))?;
        let a = self.read_src(ins, 0)?.replicate(0);
        let b = self.read_src(ins, 1)?.replicate(0);
        Ok(Cond::Compare(comparison, a, b))
    }

    fn call(&mut self, ins: &Instruction, label: &SrcParam) -> Result<Vec<Stmt>, LiftError> {
        if label.register.ty != RegisterType::Label {
            return Err(malformed(ins, "call target is not a label"));
        }
        let body: &'a [Instruction] = self
            .labels
            .get(&label.register.index)
            .copied()
            .ok_or_else(|| malformed(ins, "call to an undefined label"))?;
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(malformed(ins, "subroutine calls nested deeper than D3D9 allows (4)"));
        }
        self.call_depth += 1;
        let result = self.block(body);
        self.call_depth -= 1;
        result
    }

    /// Lifts a straight run of instructions with balanced control flow. A
    /// top-level `ret` ends the run (end of `main` or of a subroutine).
    fn block(&mut self, instructions: &'a [Instruction]) -> Result<Vec<Stmt>, LiftError> {
        let mut frames: Vec<Frame> = Vec::new();
        let mut current: Vec<Stmt> = Vec::new();
        for ins in instructions {
            let flow = matches!(
                ins.opcode,
                Opcode::If
                    | Opcode::Ifc
                    | Opcode::Else
                    | Opcode::EndIf
                    | Opcode::Rep
                    | Opcode::EndRep
                    | Opcode::Loop
                    | Opcode::EndLoop
                    | Opcode::Break
                    | Opcode::BreakC
                    | Opcode::BreakP
                    | Opcode::Call
                    | Opcode::CallNz
                    | Opcode::Ret
            );
            if flow && ins.predicate.is_some() {
                return Err(unsupported(ins, "predicated flow control"));
            }
            match ins.opcode {
                Opcode::Ret if frames.is_empty() => break,
                Opcode::Ret => return Err(unsupported(ins, "ret inside control flow")),
                Opcode::If => {
                    check_nesting(&frames, false, ins)?;
                    let cond = self.condition(src(ins, 0)?, ins)?;
                    frames.push(Frame::If {
                        offset: ins.offset,
                        cond,
                        then: None,
                        outer: std::mem::take(&mut current),
                    });
                }
                Opcode::Ifc => {
                    check_nesting(&frames, false, ins)?;
                    let cond = self.compare(ins)?;
                    frames.push(Frame::If {
                        offset: ins.offset,
                        cond,
                        then: None,
                        outer: std::mem::take(&mut current),
                    });
                }
                Opcode::Else => match frames.last_mut() {
                    Some(Frame::If { then: then @ None, .. }) => {
                        *then = Some(std::mem::take(&mut current));
                    }
                    _ => return Err(malformed(ins, "else without if")),
                },
                Opcode::EndIf => match frames.pop() {
                    Some(Frame::If { cond, then, outer, .. }) => {
                        let (then, otherwise) = match then {
                            Some(then) => (then, std::mem::take(&mut current)),
                            None => (std::mem::take(&mut current), Vec::new()),
                        };
                        current = outer;
                        self.push(&mut current, Stmt::If { cond, then, otherwise }, ins)?;
                    }
                    _ => return Err(malformed(ins, "endif without if")),
                },
                Opcode::Rep => {
                    check_nesting(&frames, true, ins)?;
                    let count = src(ins, 0)?;
                    if count.register.ty != RegisterType::ConstInt {
                        return Err(malformed(ins, "rep count must be an integer constant"));
                    }
                    self.constants.ints_read.insert(count.register.index);
                    frames.push(Frame::Loop {
                        offset: ins.offset,
                        kind: LoopKind::Rep { count: count.register.index },
                        outer: std::mem::take(&mut current),
                    });
                }
                Opcode::Loop => {
                    check_nesting(&frames, true, ins)?;
                    let control = src(ins, 1)?;
                    if src(ins, 0)?.register.ty != RegisterType::Loop || control.register.ty != RegisterType::ConstInt {
                        return Err(malformed(ins, "loop needs aL and an integer constant"));
                    }
                    self.constants.ints_read.insert(control.register.index);
                    frames.push(Frame::Loop {
                        offset: ins.offset,
                        kind: LoopKind::Loop { control: control.register.index },
                        outer: std::mem::take(&mut current),
                    });
                }
                Opcode::EndRep | Opcode::EndLoop => {
                    let rep = ins.opcode == Opcode::EndRep;
                    match frames.pop() {
                        Some(Frame::Loop { kind, outer, .. }) if rep == matches!(kind, LoopKind::Rep { .. }) => {
                            let body = std::mem::replace(&mut current, outer);
                            self.push(&mut current, Stmt::Loop { kind, body }, ins)?;
                        }
                        _ => return Err(malformed(ins, "loop end without matching start")),
                    }
                }
                Opcode::Break | Opcode::BreakC | Opcode::BreakP => {
                    if !frames.iter().any(|f| matches!(f, Frame::Loop { .. })) {
                        return Err(malformed(ins, "break outside a loop"));
                    }
                    let stmt = match ins.opcode {
                        Opcode::Break => Stmt::Break,
                        Opcode::BreakC => Stmt::BreakIf(self.compare(ins)?),
                        _ => Stmt::BreakIf(self.condition(src(ins, 0)?, ins)?),
                    };
                    self.push(&mut current, stmt, ins)?;
                }
                Opcode::Call => {
                    let label = *src(ins, 0)?;
                    let body = self.call(ins, &label)?;
                    current.extend(body);
                }
                Opcode::CallNz => {
                    let label = *src(ins, 0)?;
                    let cond = self.condition(src(ins, 1)?, ins)?;
                    let then = self.call(ins, &label)?;
                    let stmt = Stmt::If { cond, then, otherwise: Vec::new() };
                    self.push(&mut current, stmt, ins)?;
                }
                _ => self.instruction(ins, &mut current)?,
            }
        }
        if let Some(frame) = frames.last() {
            let offset = match frame {
                Frame::If { offset, .. } | Frame::Loop { offset, .. } => *offset,
            };
            return Err(LiftError::Malformed { offset, reason: "control flow not closed" });
        }
        Ok(current)
    }

    /// Lifts one non-flow-control instruction.
    fn instruction(&mut self, ins: &Instruction, out: &mut Vec<Stmt>) -> Result<(), LiftError> {
        use Opcode::*;
        let v = self.version;
        let value = match ins.opcode {
            Nop | Phase | Dcl | Def | DefI | DefB => return Ok(()),

            Mov | Mova
                if self.stage == Stage::Vertex && ins.dst.is_some_and(|d| d.register.ty == RegisterType::Addr) =>
            {
                let stmt = Stmt::LoadAddress {
                    mask: dst(ins)?.write_mask.0,
                    src: self.read_src(ins, 0)?,
                    rounding: if ins.opcode == Mova { Rounding::Nearest } else { Rounding::Floor },
                };
                return self.push(out, stmt, ins);
            }
            Mova => return Err(malformed(ins, "mova must write a0")),

            Mov => Value::Unary(UnaryOp::Mov, self.read_src(ins, 0)?),
            Frc => Value::Unary(UnaryOp::Frc, self.read_src(ins, 0)?),
            Abs => Value::Unary(UnaryOp::Abs, self.read_src(ins, 0)?),
            Sgn => Value::Unary(UnaryOp::Sgn, self.read_src(ins, 0)?),
            Nrm => Value::Unary(UnaryOp::Nrm, self.read_src(ins, 0)?),
            Lit => Value::Unary(UnaryOp::Lit, self.read_src(ins, 0)?),
            Dsx => Value::Unary(UnaryOp::Dsx, self.read_src(ins, 0)?),
            Dsy => Value::Unary(UnaryOp::Dsy, self.read_src(ins, 0)?),

            // Scalar instructions read the source's w slot before shader model 4.
            Rcp => Value::Unary(UnaryOp::Rcp, self.read_src(ins, 0)?.replicate(3)),
            Rsq => Value::Unary(UnaryOp::Rsq, self.read_src(ins, 0)?.replicate(3)),
            Exp => Value::Unary(UnaryOp::Exp, self.read_src(ins, 0)?.replicate(3)),
            Log | LogP => Value::Unary(UnaryOp::Log, self.read_src(ins, 0)?.replicate(3)),
            ExpP if v.major < 2 => Value::Unary(UnaryOp::ExpP1, self.read_src(ins, 0)?.replicate(3)),
            ExpP => Value::Unary(UnaryOp::Exp, self.read_src(ins, 0)?.replicate(3)),
            // pow and sincos read the x slot; vs_2_0 sincos's extra sources only
            // carried constants for old hardware.
            Pow => {
                Value::Binary(BinaryOp::Pow, self.read_src(ins, 0)?.replicate(0), self.read_src(ins, 1)?.replicate(0))
            }
            SinCos => Value::Unary(UnaryOp::SinCos, self.read_src(ins, 0)?.replicate(0)),

            Add | Sub | Mul | Min | Max | Slt | Sge | Dp3 | Dp4 | Crs | Dst => {
                let op = match ins.opcode {
                    Add => BinaryOp::Add,
                    Sub => BinaryOp::Sub,
                    Mul => BinaryOp::Mul,
                    Min => BinaryOp::Min,
                    Max => BinaryOp::Max,
                    Slt => BinaryOp::Slt,
                    Sge => BinaryOp::Sge,
                    Dp3 => BinaryOp::Dp3,
                    Dp4 => BinaryOp::Dp4,
                    Crs => BinaryOp::Crs,
                    _ => BinaryOp::Dst,
                };
                Value::Binary(op, self.read_src(ins, 0)?, self.read_src(ins, 1)?)
            }
            Mad | Lrp | Cmp | Cnd | Dp2Add => {
                let op = match ins.opcode {
                    Mad => TernaryOp::Mad,
                    Lrp => TernaryOp::Lrp,
                    Cmp => TernaryOp::Cmp,
                    Cnd => TernaryOp::Cnd,
                    _ => TernaryOp::Dp2Add,
                };
                let mut a = self.read_src(ins, 0)?;
                let b = self.read_src(ins, 1)?;
                let mut c = self.read_src(ins, 2)?;
                // Before ps_1_4, cnd tests a single component (r0.a).
                if ins.opcode == Cnd && !v.at_least(1, 4) {
                    a = a.replicate(0);
                }
                if ins.opcode == Dp2Add {
                    c = c.replicate(0);
                }
                Value::Ternary(op, a, b, c)
            }

            M4x4 => return self.matrix(ins, 4, BinaryOp::Dp4, out),
            M4x3 => return self.matrix(ins, 3, BinaryOp::Dp4, out),
            M3x4 => return self.matrix(ins, 4, BinaryOp::Dp3, out),
            M3x3 => return self.matrix(ins, 3, BinaryOp::Dp3, out),
            M3x2 => return self.matrix(ins, 2, BinaryOp::Dp3, out),

            SetP => {
                let d = dst(ins)?;
                let comparison = ins.comparison().ok_or_else(|| malformed(ins, "invalid comparison"))?;
                let stmt = Stmt::SetPredicate {
                    mask: d.write_mask.0,
                    comparison,
                    a: self.read_src(ins, 0)?,
                    b: self.read_src(ins, 1)?,
                    predicate: self.predicate(ins),
                };
                return self.push(out, stmt, ins);
            }

            TexKill => {
                // The tested register is encoded as a destination. Shader model
                // 1 tests xyz only; later versions test all four components. In
                // ps_1_0–1_3, `texkill t#` before `t#` is written tests texture
                // coordinate set # (Wine's texkill_test).
                let reg = self.source_reg(dst(ins)?.register, ins)?;
                let mask = if v.major >= 2 { 0xf } else { 0x7 };
                if let Reg::Input(i) = reg {
                    self.inputs[usize::from(i)].mask |= mask;
                }
                self.discards = true;
                return self.push(out, Stmt::Kill { src: plain(reg), mask }, ins);
            }

            TexCoord if self.legacy_textures() => {
                // texcoord t#: the interpolated coordinates, clamped to [0, 1].
                let n = dst(ins)?.register.index;
                let coord = self.texcoord(n, ins)?;
                let d = self.write(&dst(ins)?, ins)?;
                let stmt = Stmt::Assign {
                    dst: d,
                    value: Value::Unary(UnaryOp::Mov, coord),
                    saturate: true,
                    shift: 0,
                    predicate: None,
                };
                return self.push(out, stmt, ins);
            }
            // ps_1_4 texcrd: a copy; _dz/_dw are ordinary operand modifiers.
            TexCoord => Value::Unary(UnaryOp::Mov, self.read_src(ins, 0)?),

            Tex if v.major >= 2 => Value::Sample(Sample {
                coord: self.read_src(ins, 0)?,
                sampler: self.sampler(src(ins, 1)?.register, ins)?,
                kind: match ins.control {
                    1 => SampleKind::ProjectW,
                    2 => SampleKind::Bias,
                    _ => SampleKind::Plain,
                },
                swizzle: src(ins, 1)?.swizzle.0,
            }),
            Tex if v.at_least(1, 4) => {
                // ps_1_4 texld r#: the sampler is the destination's number.
                let sampler = dst(ins)?.register.index;
                self.samplers.entry(sampler).or_insert(TextureType::Unknown);
                let mut coord = self.read_src(ins, 0)?;
                let kind = match coord.modifier {
                    SrcModifier::Dz => SampleKind::ProjectZ,
                    SrcModifier::Dw => SampleKind::ProjectW,
                    _ => SampleKind::Plain,
                };
                if kind != SampleKind::Plain {
                    coord.modifier = SrcModifier::None;
                }
                Value::Sample(Sample { sampler, coord, kind, swizzle: IDENTITY })
            }
            Tex => {
                // tex t#: sample texture # at coordinate set #.
                let n = dst(ins)?.register.index;
                self.samplers.entry(n).or_insert(TextureType::Unknown);
                Value::Sample(Sample {
                    sampler: n,
                    coord: self.texcoord(n, ins)?,
                    kind: SampleKind::Plain,
                    swizzle: IDENTITY,
                })
            }
            TexLdl => Value::Sample(Sample {
                coord: self.read_src(ins, 0)?,
                sampler: self.sampler(src(ins, 1)?.register, ins)?,
                kind: SampleKind::Lod,
                swizzle: src(ins, 1)?.swizzle.0,
            }),
            TexLdd => Value::Sample(Sample {
                coord: self.read_src(ins, 0)?,
                sampler: self.sampler(src(ins, 1)?.register, ins)?,
                kind: SampleKind::Grad(self.read_src(ins, 2)?, self.read_src(ins, 3)?),
                swizzle: src(ins, 1)?.swizzle.0,
            }),

            TexBem | TexBemL | TexReg2Ar | TexReg2Gb | TexReg2Rgb | TexM3x2Pad | TexM3x2Tex | TexM3x3Pad
            | TexM3x3Tex | TexM3x3Diff | TexM3x3Spec | TexM3x3VSpec | TexDp3Tex | TexM3x2Depth | TexDp3 | TexM3x3
            | TexDepth | Bem => {
                return Err(unsupported(ins, "ps_1_x texture-addressing instruction"));
            }
            _ => return Err(unsupported(ins, "instruction")),
        };
        let stmt = self.assign(ins, value)?;
        self.push(out, stmt, ins)
    }

    /// Interpolated texture coordinate set `n`, as an operand.
    fn texcoord(&mut self, n: u16, ins: &Instruction) -> Result<Operand, LiftError> {
        let slot = self.input(RegisterType::Addr, n, ins)?;
        self.inputs[usize::from(slot)].mask |= 0xf;
        Ok(plain(Reg::Input(slot)))
    }

    /// `m4x4` and friends: one dot product per row, each row the next register.
    fn matrix(&mut self, ins: &Instruction, rows: u16, op: BinaryOp, out: &mut Vec<Stmt>) -> Result<(), LiftError> {
        let d = dst(ins)?;
        let vector = self.read_src(ins, 0)?;
        let matrix = self.read_src(ins, 1)?;
        for row in 0..rows {
            if d.write_mask.0 & (1 << row) == 0 {
                continue;
            }
            let row_operand = matrix
                .offset(row)
                .ok_or_else(|| unsupported(ins, "matrix rows must be constant or temporary registers"))?;
            match (row_operand.reg, row_operand.relative) {
                (Reg::Const(i), None) => {
                    self.constants.floats_read.insert(i);
                }
                (Reg::Temp(i), _) => self.temps = self.temps.max(i + 1),
                _ => {}
            }
            let mut dest = self.write(&d, ins)?;
            dest.mask = 1 << row;
            let stmt = Stmt::Assign {
                dst: dest,
                value: Value::Binary(op, vector, row_operand),
                saturate: d.modifiers.saturate,
                shift: d.shift,
                predicate: self.predicate(ins),
            };
            self.push(out, stmt, ins)?;
        }
        Ok(())
    }
}
