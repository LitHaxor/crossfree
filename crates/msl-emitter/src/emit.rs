use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use shader_ir::{
    BinaryOp, Comparison, Cond, Dest, LoopKind, Operand, Predicate, Program, Reg, Relative, Rounding, Sample,
    SampleKind, Semantic, SrcModifier, Stage, Stmt, TernaryOp, TextureType, UnaryOp, Value,
};

use crate::RUNTIME_STRUCT;

const COMPONENTS: [char; 4] = ['x', 'y', 'z', 'w'];

/// D3D9 hardware runs loops at most this many times.
const MAX_LOOP_ITERATIONS: i32 = 255;

/// Indentation stops growing past this many levels, so output size stays
/// linear in the number of statements however deep they nest.
const MAX_INDENT: usize = 16;

/// Emission settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Name of the generated entry point.
    pub entry_point: String,
    /// Size of the float constant buffer. Relative reads outside it return 0.
    pub float_constants: u16,
    /// Texture types for samplers the shader does not declare (ps_1_x, where
    /// the type comes from the bound texture). Unset samplers default to 2D.
    pub texture_types: BTreeMap<u16, TextureType>,
}

impl Options {
    /// Defaults for `program`: entry point `shader_main`, 256 float constants
    /// for vertex shaders and 224 for pixel shaders (the D3D9 maxima).
    pub fn new(program: &Program) -> Self {
        Self {
            entry_point: "shader_main".into(),
            float_constants: match program.stage {
                Stage::Vertex => 256,
                Stage::Pixel => 224,
            },
            texture_types: BTreeMap::new(),
        }
    }
}

/// Why a program could not be emitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmitError {
    /// A pixel-only operation (derivative, discard) in a vertex shader.
    PixelOnlyOperation(&'static str),
    /// Statements nest deeper than [`shader_ir::MAX_NESTING`], which no
    /// lifted program does. Emission recurses per level, so this bounds it.
    TooDeeplyNested,
}

impl fmt::Display for EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmitError::PixelOnlyOperation(what) => write!(f, "{what} is only valid in pixel shaders"),
            EmitError::TooDeeplyNested => write!(f, "statements nest deeper than {} levels", shader_ir::MAX_NESTING),
        }
    }
}

/// Fails if `body` nests deeper than [`shader_ir::MAX_NESTING`]. Iterative,
/// so it is safe on any input; everything after it may recurse.
fn check_nesting(body: &[Stmt]) -> Result<(), EmitError> {
    let mut pending: Vec<(&[Stmt], usize)> = vec![(body, 1)];
    while let Some((stmts, depth)) = pending.pop() {
        if depth > shader_ir::MAX_NESTING {
            return Err(EmitError::TooDeeplyNested);
        }
        for stmt in stmts {
            match stmt {
                Stmt::If { then, otherwise, .. } => {
                    pending.push((then, depth + 1));
                    pending.push((otherwise, depth + 1));
                }
                Stmt::Loop { body, .. } => pending.push((body, depth + 1)),
                _ => {}
            }
        }
    }
    Ok(())
}

impl std::error::Error for EmitError {}

/// Emits a complete MSL source file for `program`.
pub fn emit(program: &Program, options: &Options) -> Result<String, EmitError> {
    check_nesting(&program.body)?;
    let mut w =
        Writer { program, options, out: String::new(), indent: 1, loop_depth: 0, uses: Uses::scan(&program.body) };
    let mut body = String::new();
    std::mem::swap(&mut w.out, &mut body);
    w.block(&program.body)?;
    std::mem::swap(&mut w.out, &mut body);

    w.prelude();
    w.signature_structs();
    w.entry_point_header();
    w.locals();
    w.out.push_str(&body);
    w.epilogue();
    Ok(w.out)
}

/// What the body uses, to declare only the parameters it needs.
#[derive(Default)]
struct Uses {
    float_consts: bool,
    int_consts: bool,
    bool_consts: bool,
}

impl Uses {
    fn scan(body: &[Stmt]) -> Self {
        let mut uses = Uses::default();
        uses.stmts(body);
        uses
    }

    fn stmts(&mut self, body: &[Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Assign { value, .. } => self.value(value),
                Stmt::LoadAddress { src, .. } | Stmt::Kill { src, .. } => self.operand(src),
                Stmt::SetPredicate { a, b, .. } => {
                    self.operand(a);
                    self.operand(b);
                }
                Stmt::If { cond, then, otherwise } => {
                    self.cond(cond);
                    self.stmts(then);
                    self.stmts(otherwise);
                }
                Stmt::Loop { body, .. } => {
                    self.int_consts = true;
                    self.stmts(body);
                }
                Stmt::BreakIf(cond) => self.cond(cond),
                Stmt::Break => {}
            }
        }
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Unary(_, a) => self.operand(a),
            Value::Binary(_, a, b) => {
                self.operand(a);
                self.operand(b);
            }
            Value::Ternary(_, a, b, c) => {
                self.operand(a);
                self.operand(b);
                self.operand(c);
            }
            Value::Sample(s) => {
                self.operand(&s.coord);
                if let SampleKind::Grad(dx, dy) = &s.kind {
                    self.operand(dx);
                    self.operand(dy);
                }
            }
        }
    }

    fn cond(&mut self, cond: &Cond) {
        match cond {
            Cond::Bool(_) => self.bool_consts = true,
            Cond::Compare(_, a, b) => {
                self.operand(a);
                self.operand(b);
            }
            Cond::Predicate { .. } => {}
        }
    }

    fn operand(&mut self, op: &Operand) {
        match op.reg {
            Reg::Const(_) => self.float_consts = true,
            Reg::ConstInt(_) => self.int_consts = true,
            Reg::ConstBool(_) => self.bool_consts = true,
            _ => {}
        }
    }
}

struct Writer<'a> {
    program: &'a Program,
    options: &'a Options,
    out: String,
    indent: usize,
    loop_depth: usize,
    uses: Uses,
}

fn mask_suffix(mask: u8) -> String {
    (0..4).filter(|i| mask & (1 << i) != 0).map(|i| COMPONENTS[i]).collect()
}

fn float(x: f32) -> String {
    if x.is_nan() {
        "NAN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "INFINITY".into() } else { "(-INFINITY)".into() }
    } else if x.fract() == 0.0 && x.abs() < 1e9 {
        format!("{x:.1}")
    } else {
        format!("{x:e}")
    }
}

impl Writer<'_> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.indent.min(MAX_INDENT) {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn vertex(&self) -> bool {
        self.program.stage == Stage::Vertex
    }

    fn texture_type(&self, sampler: u16) -> TextureType {
        match self.program.samplers.get(&sampler) {
            Some(TextureType::Unknown) | None => {
                self.options.texture_types.get(&sampler).copied().unwrap_or(TextureType::Texture2D)
            }
            Some(ty) => *ty,
        }
    }

    // ---- Declarations ------------------------------------------------------

    fn prelude(&mut self) {
        let n = self.options.float_constants;
        let _ = write!(
            self.out,
            "#include <metal_stdlib>\nusing namespace metal;\n\n{RUNTIME_STRUCT}\n\
             static inline float4 d3d9_const(constant float4 *c, int i)\n{{\n    \
             return (i >= 0 && i < {n}) ? c[i] : float4(0.0);\n}}\n\n\
             static inline float4 d3d9_nrm(float4 v)\n{{\n    \
             float len2 = dot(v.xyz, v.xyz);\n    return len2 == 0.0 ? v : v * rsqrt(len2);\n}}\n\n\
             static inline float4 d3d9_lit(float4 v)\n{{\n    \
             float power = clamp(v.w, -128.0, 128.0);\n    \
             float specular = (v.x > 0.0 && v.y > 0.0) ? pow(v.y, power) : 0.0;\n    \
             return float4(1.0, max(v.x, 0.0), specular, 1.0);\n}}\n\n\
             static inline float4 d3d9_expp(float4 v)\n{{\n    \
             return float4(exp2(floor(v.x)), v.x - floor(v.x), exp2(v.x), 1.0);\n}}\n\n\
             static inline float4 d3d9_pow(float4 a, float4 b)\n{{\n    \
             return select(pow(abs(a), b), float4(1.0), b == float4(0.0));\n}}\n\n"
        );
    }

    fn input_member(&self, slot: usize) -> String {
        let sig = &self.program.inputs[slot];
        match self.program.stage {
            Stage::Vertex => format!("v{}", sig.register),
            Stage::Pixel => format!("in_{}", sig.semantic.name()),
        }
    }

    fn output_member(&self, slot: usize) -> String {
        match self.program.outputs[slot].semantic {
            Semantic::Position => "position".into(),
            Semantic::PointSize => "point_size".into(),
            Semantic::Depth => "depth".into(),
            semantic => format!("out_{}", semantic.name()),
        }
    }

    fn signature_structs(&mut self) {
        let program = self.program;
        if !program.inputs.is_empty() {
            self.out.push_str("struct ShaderInput\n{\n");
            for (slot, sig) in program.inputs.iter().enumerate() {
                let attribute = match program.stage {
                    Stage::Vertex => format!("attribute({})", sig.register),
                    Stage::Pixel if sig.centroid => {
                        format!("user({}), centroid_perspective", sig.semantic.name())
                    }
                    Stage::Pixel => format!("user({})", sig.semantic.name()),
                };
                let member = self.input_member(slot);
                let _ = writeln!(self.out, "    float4 {member} [[{attribute}]];");
            }
            self.out.push_str("};\n\n");
        }

        self.out.push_str("struct ShaderOutput\n{\n");
        if self.vertex() {
            self.out.push_str("    float4 position [[position]];\n");
        }
        for (slot, sig) in program.outputs.iter().enumerate() {
            let member = self.output_member(slot);
            let decl = match sig.semantic {
                Semantic::Position => continue,
                Semantic::PointSize => format!("float {member} [[point_size]]"),
                Semantic::Depth => format!("float {member} [[depth(any)]]"),
                Semantic::Target(n) => format!("float4 {member} [[color({n})]]"),
                semantic => format!("float4 {member} [[user({})]]", semantic.name()),
            };
            let _ = writeln!(self.out, "    {decl};");
        }
        self.out.push_str("};\n\n");
    }

    fn entry_point_header(&mut self) {
        let program = self.program;
        let mut params = Vec::new();
        if !program.inputs.is_empty() {
            params.push("ShaderInput shader_in [[stage_in]]".to_owned());
        }
        if !self.vertex() {
            if program.uses_position {
                params.push("float4 frag_position [[position]]".into());
            }
            if program.uses_face {
                params.push("bool front_facing [[front_facing]]".into());
            }
        }
        if self.uses.float_consts {
            params.push("constant float4 *c [[buffer(0)]]".into());
        }
        if self.uses.int_consts {
            params.push("constant int4 *ci [[buffer(1)]]".into());
        }
        if self.uses.bool_consts {
            params.push("constant uint *cb [[buffer(2)]]".into());
        }
        if self.vertex() {
            params.push("constant RuntimeConstants &runtime [[buffer(3)]]".into());
        }
        for &n in program.samplers.keys() {
            let texture = match self.texture_type(n) {
                TextureType::Cube => "texturecube<float>",
                TextureType::Volume => "texture3d<float>",
                TextureType::Texture2D | TextureType::Unknown => "texture2d<float>",
            };
            params.push(format!("{texture} t{n}_texture [[texture({n})]]"));
            params.push(format!("sampler s{n} [[sampler({n})]]"));
        }
        let kind = if self.vertex() { "vertex" } else { "fragment" };
        let _ = write!(
            self.out,
            "{kind} ShaderOutput {}(\n    {})\n{{\n",
            self.options.entry_point,
            params.join(",\n    ")
        );
    }

    fn locals(&mut self) {
        let program = self.program;
        for i in 0..program.temps {
            self.line(&format!("float4 r{i} = float4(0.0);"));
        }
        for i in 0..program.texture_regs {
            self.line(&format!("float4 t{i} = float4(0.0);"));
        }
        for i in 0..program.outputs.len() {
            self.line(&format!("float4 o{i} = float4(0.0);"));
        }
        self.line("int4 a0 = int4(0);");
        self.line("int aL = 0;");
        self.line("bool4 p0 = bool4(false);");
        if program.uses_position {
            self.line("float4 vpos = float4(floor(frag_position.xy), frag_position.zw);");
        }
        if program.uses_face {
            self.line("float4 vface = float4(front_facing ? 1.0 : -1.0);");
        }
    }

    fn epilogue(&mut self) {
        let program = self.program;
        self.line("ShaderOutput shader_out;");
        if self.vertex() {
            let position = program.outputs.iter().position(|s| s.semantic == Semantic::Position);
            match position {
                Some(slot) => self.line(&format!("shader_out.position = o{slot};")),
                None => self.line("shader_out.position = float4(0.0, 0.0, 0.0, 1.0);"),
            }
            self.line(
                "shader_out.position.xy = shader_out.position.xy * runtime.pos_fixup.xy \
                 + runtime.pos_fixup.zw * shader_out.position.w;",
            );
        }
        for (slot, sig) in program.outputs.iter().enumerate() {
            let member = self.output_member(slot);
            let value = match sig.semantic {
                Semantic::Position => continue,
                Semantic::PointSize | Semantic::Depth => format!("o{slot}.x"),
                // Vertex colours are clamped to [0, 1] before shader model 3.
                Semantic::Color(_) if self.vertex() && program.version.major < 3 => {
                    format!("saturate(o{slot})")
                }
                _ => format!("o{slot}"),
            };
            self.line(&format!("shader_out.{member} = {value};"));
        }
        self.line("return shader_out;");
        self.out.push_str("}\n");
    }

    // ---- Statements -------------------------------------------------------

    fn block(&mut self, body: &[Stmt]) -> Result<(), EmitError> {
        for stmt in body {
            self.stmt(stmt)?;
        }
        Ok(())
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), EmitError> {
        match stmt {
            Stmt::Assign { dst, value, saturate, shift, predicate } => {
                let mut expr = self.value(value)?;
                if *shift != 0 {
                    let scale = 2f32.powi(i32::from(*shift));
                    expr = format!("({expr} * {})", float(scale));
                }
                if *saturate {
                    expr = format!("saturate({expr})");
                }
                let target = self.dest(dst);
                self.write_masked(&target, dst.mask, &expr, predicate.as_ref());
            }
            Stmt::LoadAddress { mask, src, rounding } => {
                let value = self.operand(src);
                let round = match rounding {
                    Rounding::Floor => "floor",
                    Rounding::Nearest => "round",
                };
                for i in (0..4).filter(|i| mask & (1 << i) != 0) {
                    let c = COMPONENTS[i];
                    self.line(&format!("a0.{c} = int({round}(({value}).{c}));"));
                }
            }
            Stmt::SetPredicate { mask, comparison, a, b, predicate } => {
                let expr = format!("({} {} {})", self.operand(a), op(*comparison), self.operand(b));
                self.write_masked("p0", *mask, &expr, predicate.as_ref());
            }
            Stmt::Kill { src, mask } => {
                if self.vertex() {
                    return Err(EmitError::PixelOnlyOperation("texkill"));
                }
                let suffix = mask_suffix(*mask);
                let zero = format!("float{}(0.0)", suffix.len());
                let test = format!("any(({}).{suffix} < {zero})", self.operand(src));
                self.line(&format!("if ({test}) discard_fragment();"));
            }
            Stmt::If { cond, then, otherwise } => {
                let cond = self.cond(cond);
                self.line(&format!("if ({cond})"));
                self.line("{");
                self.indent += 1;
                self.block(then)?;
                self.indent -= 1;
                self.line("}");
                if !otherwise.is_empty() {
                    self.line("else");
                    self.line("{");
                    self.indent += 1;
                    self.block(otherwise)?;
                    self.indent -= 1;
                    self.line("}");
                }
            }
            Stmt::Loop { kind, body } => self.loop_stmt(kind, body)?,
            Stmt::Break => self.line("break;"),
            Stmt::BreakIf(cond) => {
                let cond = self.cond(cond);
                self.line(&format!("if ({cond}) break;"));
            }
        }
        Ok(())
    }

    fn loop_stmt(&mut self, kind: &LoopKind, body: &[Stmt]) -> Result<(), EmitError> {
        let d = self.loop_depth;
        self.loop_depth += 1;
        match kind {
            LoopKind::Rep { count } => {
                let control = self.int_const(*count);
                self.line(&format!(
                    "for (int loop{d} = 0; loop{d} < min(({control}).x, {MAX_LOOP_ITERATIONS}); ++loop{d})"
                ));
                self.line("{");
                self.indent += 1;
                self.block(body)?;
                self.indent -= 1;
                self.line("}");
            }
            LoopKind::Loop { control } => {
                // aL belongs to the innermost loop: save and restore it.
                let control = self.int_const(*control);
                self.line("{");
                self.indent += 1;
                self.line(&format!("int4 loop_control{d} = {control};"));
                self.line(&format!("int saved_aL{d} = aL;"));
                self.line(&format!("aL = loop_control{d}.y;"));
                self.line(&format!(
                    "for (int loop{d} = 0; loop{d} < min(loop_control{d}.x, {MAX_LOOP_ITERATIONS}); \
                     ++loop{d}, aL += loop_control{d}.z)"
                ));
                self.line("{");
                self.indent += 1;
                self.block(body)?;
                self.indent -= 1;
                self.line("}");
                self.line(&format!("aL = saved_aL{d};"));
                self.indent -= 1;
                self.line("}");
            }
        }
        self.loop_depth -= 1;
        Ok(())
    }

    /// Writes `expr` to the masked components of `target`, honouring an
    /// optional per-component predicate.
    fn write_masked(&mut self, target: &str, mask: u8, expr: &str, predicate: Option<&Predicate>) {
        let suffix = mask_suffix(mask);
        if suffix.is_empty() {
            return;
        }
        match predicate {
            None => self.line(&format!("{target}.{suffix} = ({expr}).{suffix};")),
            Some(p) => {
                self.line("{");
                self.indent += 1;
                self.line(&format!("auto value = {expr};"));
                for i in (0..4).filter(|i| mask & (1 << i) != 0) {
                    let c = COMPONENTS[i];
                    let pc = COMPONENTS[usize::from(p.swizzle[i] & 3)];
                    let not = if p.negate { "!" } else { "" };
                    self.line(&format!("if ({not}p0.{pc}) {target}.{c} = value.{c};"));
                }
                self.indent -= 1;
                self.line("}");
            }
        }
    }

    fn dest(&self, dst: &Dest) -> String {
        match dst.reg {
            Reg::Temp(i) => format!("r{i}"),
            Reg::Texture(i) => format!("t{i}"),
            Reg::Output(i) => format!("o{i}"),
            Reg::Predicate => "p0".into(),
            Reg::Addr => "a0".into(),
            // The lifter only produces the destinations above.
            other => format!("/* unexpected destination {other:?} */ r0"),
        }
    }

    // ---- Expressions ------------------------------------------------------

    fn cond(&self, cond: &Cond) -> String {
        match cond {
            Cond::Bool(n) => match self.program.constants.bool_defs.get(n) {
                Some(value) => value.to_string(),
                None => format!("(cb[{n}] != 0u)"),
            },
            Cond::Predicate { component, negate } => {
                let not = if *negate { "!" } else { "" };
                format!("{not}p0.{}", COMPONENTS[usize::from(component & 3)])
            }
            Cond::Compare(comparison, a, b) => {
                format!("(({}).x {} ({}).x)", self.operand(a), op(*comparison), self.operand(b))
            }
        }
    }

    fn int_const(&self, n: u16) -> String {
        match self.program.constants.int_defs.get(&n) {
            Some([x, y, z, w]) => format!("int4({x}, {y}, {z}, {w})"),
            None => format!("ci[{n}]"),
        }
    }

    fn register(&self, op: &Operand) -> String {
        let constants = &self.program.constants;
        match op.reg {
            Reg::Temp(i) => format!("r{i}"),
            Reg::Texture(i) => format!("t{i}"),
            Reg::Input(i) => format!("shader_in.{}", self.input_member(usize::from(i))),
            Reg::Output(i) => format!("o{i}"),
            Reg::Const(i) => {
                let uploaded = match op.relative {
                    Some(Relative { reg, component }) => {
                        let index = match reg {
                            Reg::Loop => "aL".to_owned(),
                            _ => format!("a0.{}", COMPONENTS[usize::from(component & 3)]),
                        };
                        format!("d3d9_const(c, {i} + {index})")
                    }
                    None => match constants.float_defs.get(&i) {
                        // `def` values are inlined, already clamped if need be.
                        Some(v) => {
                            return format!(
                                "float4({}, {}, {}, {})",
                                float(v[0]),
                                float(v[1]),
                                float(v[2]),
                                float(v[3])
                            );
                        }
                        None if i < self.options.float_constants => format!("c[{i}]"),
                        None => return "float4(0.0)".into(),
                    },
                };
                // ps_1_x constant registers hold [-1, 1] (wined3d clamps
                // uploads for these shaders); do it here so the runtime
                // uploads application values unchanged.
                if constants.clamp_floats { format!("clamp({uploaded}, -1.0, 1.0)") } else { uploaded }
            }
            Reg::ConstInt(i) => format!("float4({})", self.int_const(i)),
            Reg::ConstBool(i) => match constants.bool_defs.get(&i) {
                Some(&value) => format!("float4({})", if value { "1.0" } else { "0.0" }),
                None => format!("float4(cb[{i}] != 0u ? 1.0 : 0.0)"),
            },
            Reg::Addr => "float4(a0)".into(),
            Reg::Loop => "float4(float(aL))".into(),
            Reg::Predicate => "float4(p0)".into(),
            Reg::Position => "vpos".into(),
            Reg::Face => "vface".into(),
        }
    }

    fn operand(&self, op: &Operand) -> String {
        if op.reg == Reg::Predicate {
            // Only conditions read the predicate as booleans.
            let swizzle: String = op.swizzle.iter().map(|c| COMPONENTS[usize::from(c & 3)]).collect();
            let not = if op.modifier == SrcModifier::Not { "!" } else { "" };
            return format!("{not}p0.{swizzle}");
        }
        let mut v = self.register(op);
        if op.swizzle != [0, 1, 2, 3] {
            let swizzle: String = op.swizzle.iter().map(|c| COMPONENTS[usize::from(c & 3)]).collect();
            v = format!("{v}.{swizzle}");
        }
        match op.modifier {
            SrcModifier::None | SrcModifier::Not => v,
            SrcModifier::Neg => format!("(-{v})"),
            SrcModifier::Bias => format!("({v} - 0.5)"),
            SrcModifier::BiasNeg => format!("(-({v} - 0.5))"),
            SrcModifier::Sign => format!("({v} * 2.0 - 1.0)"),
            SrcModifier::SignNeg => format!("(-({v} * 2.0 - 1.0))"),
            SrcModifier::Comp => format!("(1.0 - {v})"),
            SrcModifier::X2 => format!("({v} * 2.0)"),
            SrcModifier::X2Neg => format!("(-({v} * 2.0))"),
            SrcModifier::Dz => format!("({v} / ({v}).z)"),
            SrcModifier::Dw => format!("({v} / ({v}).w)"),
            SrcModifier::Abs => format!("abs({v})"),
            SrcModifier::AbsNeg => format!("(-abs({v}))"),
        }
    }

    fn value(&self, value: &Value) -> Result<String, EmitError> {
        Ok(match value {
            Value::Unary(op, a) => {
                let a = self.operand(a);
                match op {
                    UnaryOp::Mov => a,
                    UnaryOp::Rcp => format!("(1.0 / {a})"),
                    UnaryOp::Rsq => format!("rsqrt(abs({a}))"),
                    UnaryOp::Exp => format!("exp2({a})"),
                    UnaryOp::Log => format!("log2(abs({a}))"),
                    UnaryOp::ExpP1 => format!("d3d9_expp({a})"),
                    UnaryOp::Frc => format!("fract({a})"),
                    UnaryOp::Abs => format!("abs({a})"),
                    UnaryOp::Sgn => format!("sign({a})"),
                    UnaryOp::Nrm => format!("d3d9_nrm({a})"),
                    UnaryOp::Lit => format!("d3d9_lit({a})"),
                    UnaryOp::SinCos => format!("float4(cos(({a}).x), sin(({a}).x), 0.0, 0.0)"),
                    UnaryOp::Dsx | UnaryOp::Dsy if self.vertex() => {
                        return Err(EmitError::PixelOnlyOperation("dsx/dsy"));
                    }
                    UnaryOp::Dsx => format!("dfdx({a})"),
                    UnaryOp::Dsy => format!("dfdy({a})"),
                }
            }
            Value::Binary(op, a, b) => {
                let (a, b) = (self.operand(a), self.operand(b));
                match op {
                    BinaryOp::Add => format!("({a} + {b})"),
                    BinaryOp::Sub => format!("({a} - {b})"),
                    BinaryOp::Mul => format!("({a} * {b})"),
                    BinaryOp::Min => format!("min({a}, {b})"),
                    BinaryOp::Max => format!("max({a}, {b})"),
                    BinaryOp::Slt => format!("select(float4(0.0), float4(1.0), {a} < {b})"),
                    BinaryOp::Sge => format!("select(float4(0.0), float4(1.0), {a} >= {b})"),
                    BinaryOp::Dp3 => format!("float4(dot(({a}).xyz, ({b}).xyz))"),
                    BinaryOp::Dp4 => format!("float4(dot({a}, {b}))"),
                    BinaryOp::Pow => format!("d3d9_pow({a}, {b})"),
                    BinaryOp::Crs => format!("float4(cross(({a}).xyz, ({b}).xyz), 0.0)"),
                    BinaryOp::Dst => format!("float4(1.0, ({a}).y * ({b}).y, ({a}).z, ({b}).w)"),
                }
            }
            Value::Ternary(op, a, b, c) => {
                let (a, b, c) = (self.operand(a), self.operand(b), self.operand(c));
                match op {
                    TernaryOp::Mad => format!("({a} * {b} + {c})"),
                    TernaryOp::Lrp => format!("mix({c}, {b}, {a})"),
                    TernaryOp::Cmp => format!("select({c}, {b}, {a} >= float4(0.0))"),
                    TernaryOp::Cnd => format!("select({c}, {b}, {a} > float4(0.5))"),
                    TernaryOp::Dp2Add => format!("float4(dot(({a}).xy, ({b}).xy) + ({c}).x)"),
                }
            }
            Value::Sample(sample) => self.sample(sample),
        })
    }

    fn sample(&self, sample: &Sample) -> String {
        let n = sample.sampler;
        let ty = self.texture_type(n);
        let coord = self.operand(&sample.coord);
        let dims = match ty {
            TextureType::Cube | TextureType::Volume => "xyz",
            TextureType::Texture2D | TextureType::Unknown => "xy",
        };
        let base = format!("({coord}).{dims}");
        let (coords, extra) = match &sample.kind {
            SampleKind::Plain => (base, None),
            SampleKind::ProjectW => (format!("({base} / ({coord}).w)"), None),
            SampleKind::ProjectZ => (format!("({base} / ({coord}).z)"), None),
            SampleKind::Bias => (base, Some(format!("bias(({coord}).w)"))),
            SampleKind::Lod => (base, Some(format!("level(({coord}).w)"))),
            SampleKind::Grad(dx, dy) => {
                let (dx, dy) = (self.operand(dx), self.operand(dy));
                let gradient = match ty {
                    TextureType::Cube => "gradientcube",
                    TextureType::Volume => "gradient3d",
                    TextureType::Texture2D | TextureType::Unknown => "gradient2d",
                };
                (base, Some(format!("{gradient}(({dx}).{dims}, ({dy}).{dims})")))
            }
        };
        // Vertex shaders have no derivatives, so they sample an explicit level.
        let extra = match extra {
            None if self.vertex() => Some("level(0.0)".to_owned()),
            Some(e) if self.vertex() && !e.starts_with("level") => Some("level(0.0)".to_owned()),
            other => other,
        };
        let call = match extra {
            Some(extra) => format!("t{n}_texture.sample(s{n}, {coords}, {extra})"),
            None => format!("t{n}_texture.sample(s{n}, {coords})"),
        };
        // The sampler register's swizzle rearranges the result.
        if sample.swizzle == [0, 1, 2, 3] {
            call
        } else {
            let swizzle: String = sample.swizzle.iter().map(|c| COMPONENTS[usize::from(c & 3)]).collect();
            format!("{call}.{swizzle}")
        }
    }
}

fn op(comparison: Comparison) -> &'static str {
    match comparison {
        Comparison::Gt => ">",
        Comparison::Eq => "==",
        Comparison::Ge => ">=",
        Comparison::Lt => "<",
        Comparison::Ne => "!=",
        Comparison::Le => "<=",
    }
}
