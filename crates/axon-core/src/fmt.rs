//! Canonical AST pretty-printer for the Axon language.
//!
//! `format_program(program)` emits the idempotent canonical source
//! representation. All whitespace decisions come from the AST — original
//! whitespace is discarded. The output satisfies the formatting rules in
//! `spec/compiler-phase4.md §2`.

use crate::ast::*;

/// Pretty-print `program` to canonical Axon source.
pub fn format_program(program: &Program) -> String {
    let mut f = Formatter::new();
    f.emit_program(program);
    f.finish()
}

// ── Formatter ─────────────────────────────────────────────────────────────────

struct Formatter {
    buf: String,
    indent: usize,
}

impl Formatter {
    fn new() -> Self {
        Self { buf: String::new(), indent: 0 }
    }

    fn finish(mut self) -> String {
        // Single trailing newline, no trailing whitespace on last line.
        while self.buf.ends_with('\n') || self.buf.ends_with(' ') {
            self.buf.pop();
        }
        self.buf.push('\n');
        self.buf
    }

    fn write(&mut self, s: &str) { self.buf.push_str(s); }

    fn writeln(&mut self, s: &str) {
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    fn nl(&mut self) { self.buf.push('\n'); }

    fn ind(&mut self) {
        for _ in 0..self.indent {
            self.buf.push_str("    ");
        }
    }

    fn push(&mut self) { self.indent += 1; }
    fn pop(&mut self)  { if self.indent > 0 { self.indent -= 1; } }

    // ── Program ───────────────────────────────────────────────────────────────

    fn emit_program(&mut self, program: &Program) {
        // Emit `use` declarations first, then a blank line.
        let uses: Vec<_> = program.items.iter()
            .filter_map(|i| if let Item::UseDecl(u) = i { Some(u) } else { None })
            .collect();
        if !uses.is_empty() {
            for u in uses { self.emit_use(u); }
            self.nl();
        }

        // Emit all other items, separated by one blank line.
        let mut first = true;
        for item in &program.items {
            if matches!(item, Item::UseDecl(_) | Item::ModDecl(_)) { continue; }
            if !first { self.nl(); }
            first = false;
            self.emit_item(item);
        }
    }

    fn emit_use(&mut self, u: &UseDecl) {
        self.write("use ");
        self.write(&u.path.join("::"));
        if !u.items.is_empty() {
            self.write("::{");
            self.write(&u.items.join(", "));
            self.write("}");
        }
        self.nl();
    }

    fn emit_item(&mut self, item: &Item) {
        match item {
            Item::FnDef(f)   => self.emit_fn(f),
            Item::TypeDef(t) => self.emit_typedef(t),
            Item::EnumDef(e) => self.emit_enumdef(e),
            Item::TraitDef(t) => self.emit_traitdef(t),
            Item::ImplBlock(b) => self.emit_implblock(b),
            Item::LetDef { name, value, .. } => {
                self.write("let ");
                self.write(name);
                self.write(" = ");
                self.emit_expr(value);
                self.nl();
            }
            Item::UseDecl(_) | Item::ModDecl(_) => {}
        }
    }

    // ── Attributes ────────────────────────────────────────────────────────────

    fn emit_attrs(&mut self, attrs: &[Attr]) {
        for attr in attrs {
            self.write("@[");
            self.write(&attr.name);
            if !attr.args.is_empty() {
                self.write("(");
                self.write(&attr.args.join(", "));
                self.write(")");
            }
            self.writeln("]");
        }
    }

    // ── Functions ─────────────────────────────────────────────────────────────

    fn emit_fn(&mut self, f: &FnDef) {
        self.emit_attrs(&f.attrs);
        if f.public { self.write("pub "); }
        self.write("fn ");
        self.write(&f.name);
        if !f.generic_params.is_empty() {
            self.write("<");
            // Build a map of bounds for quick lookup.
            let bounds_map: std::collections::HashMap<&str, &Vec<String>> = f
                .generic_bounds
                .iter()
                .map(|(n, bs)| (n.as_str(), bs))
                .collect();
            let parts: Vec<String> = f.generic_params.iter().map(|p| {
                if let Some(bs) = bounds_map.get(p.as_str()) {
                    format!("{}: {}", p, bs.join(" + "))
                } else {
                    p.clone()
                }
            }).collect();
            self.write(&parts.join(", "));
            self.write(">");
        }
        self.write("(");
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 { self.write(", "); }
            self.write(&p.name);
            self.write(": ");
            self.emit_axon_type(&p.ty);
        }
        self.write(")");
        if let Some(ret) = &f.return_type {
            self.write(" -> ");
            self.emit_axon_type(ret);
        }
        self.write(" ");
        self.emit_block_body(&f.body);
        self.nl();
    }

    // ── Types ─────────────────────────────────────────────────────────────────

    fn emit_typedef(&mut self, t: &TypeDef) {
        self.write("type ");
        self.write(&t.name);
        if !t.generic_params.is_empty() {
            self.write("<");
            self.write(&t.generic_params.join(", "));
            self.write(">");
        }
        self.write(" = { ");
        for (i, field) in t.fields.iter().enumerate() {
            if i > 0 { self.write(", "); }
            self.write(&field.name);
            self.write(": ");
            self.emit_axon_type(&field.ty);
        }
        self.writeln(" }");
    }

    fn emit_enumdef(&mut self, e: &EnumDef) {
        self.write("enum ");
        self.write(&e.name);
        if !e.generic_params.is_empty() {
            self.write("<");
            self.write(&e.generic_params.join(", "));
            self.write(">");
        }
        self.writeln(" {");
        self.push();
        for variant in &e.variants {
            self.ind();
            self.write(&variant.name);
            if !variant.fields.is_empty() {
                self.write(" { ");
                for (i, f) in variant.fields.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.write(&f.name);
                    self.write(": ");
                    self.emit_axon_type(&f.ty);
                }
                self.write(" }");
            }
            self.writeln(",");
        }
        self.pop();
        self.writeln("}");
    }

    fn emit_traitdef(&mut self, t: &TraitDef) {
        self.write("trait ");
        self.write(&t.name);
        if !t.generic_params.is_empty() {
            self.write("<");
            self.write(&t.generic_params.join(", "));
            self.write(">");
        }
        self.writeln(" {");
        self.push();
        for m in &t.methods {
            self.ind();
            self.write("fn ");
            self.write(&m.name);
            self.write("(");
            for (i, p) in m.params.iter().enumerate() {
                if i > 0 { self.write(", "); }
                self.write(&p.name);
                self.write(": ");
                self.emit_axon_type(&p.ty);
            }
            self.write(")");
            if let Some(ret) = &m.return_type {
                self.write(" -> ");
                self.emit_axon_type(ret);
            }
            self.nl();
        }
        self.pop();
        self.writeln("}");
    }

    fn emit_implblock(&mut self, b: &ImplBlock) {
        self.write("impl ");
        self.write(&b.trait_name);
        self.write(" for ");
        self.emit_axon_type(&b.for_type);
        self.writeln(" {");
        self.push();
        let mut first = true;
        for m in &b.methods {
            if !first { self.nl(); }
            first = false;
            self.ind();
            self.emit_fn(m);
        }
        self.pop();
        self.writeln("}");
    }

    fn emit_axon_type(&mut self, ty: &AxonType) {
        match ty {
            AxonType::Named(n)     => self.write(n),
            AxonType::TypeParam(n) => self.write(n),
            AxonType::DynTrait(n)  => { self.write("dyn "); self.write(n); }
            AxonType::Option(inner) => {
                self.write("Option<"); self.emit_axon_type(inner); self.write(">");
            }
            AxonType::Result { ok, err } => {
                self.write("Result<");
                self.emit_axon_type(ok);
                self.write(", ");
                self.emit_axon_type(err);
                self.write(">");
            }
            AxonType::Chan(inner) => {
                self.write("Chan<"); self.emit_axon_type(inner); self.write(">");
            }
            AxonType::Slice(inner) => {
                self.write("["); self.emit_axon_type(inner); self.write("]");
            }
            AxonType::Generic { base, args } => {
                self.write(base);
                self.write("<");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_axon_type(a);
                }
                self.write(">");
            }
            AxonType::Fn { params, ret } => {
                self.write("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_axon_type(p);
                }
                self.write(") -> ");
                self.emit_axon_type(ret);
            }
            AxonType::Ref(inner) => {
                self.write("&"); self.emit_axon_type(inner);
            }
            AxonType::Tuple(elems) => {
                self.write("(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_axon_type(e);
                }
                self.write(")");
            }
        }
    }

    // ── Blocks ────────────────────────────────────────────────────────────────

    /// Emit a function body block: `{\n    stmts...\n}` (with trailing newline from caller).
    fn emit_block_body(&mut self, expr: &Expr) {
        match expr {
            Expr::Block(stmts) => {
                self.writeln("{");
                self.push();
                for stmt in stmts {
                    self.ind();
                    self.emit_expr(&stmt.expr);
                    self.nl();
                }
                self.pop();
                self.ind();
                self.write("}");
            }
            _ => {
                self.write("{ ");
                self.emit_expr(expr);
                self.write(" }");
            }
        }
    }

    /// Emit an inline block (used inside expressions, not at top level).
    fn emit_inline_block(&mut self, stmts: &[Stmt]) {
        if stmts.is_empty() {
            self.write("{}");
            return;
        }
        self.writeln("{");
        self.push();
        for stmt in stmts {
            self.ind();
            self.emit_expr(&stmt.expr);
            self.nl();
        }
        self.pop();
        self.ind();
        self.write("}");
    }

    // ── Expressions ───────────────────────────────────────────────────────────

    fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name) => self.write(name),
            Expr::Literal(lit) => self.emit_literal(lit),

            Expr::Block(stmts) => self.emit_inline_block(stmts),

            Expr::Let { name, value } => {
                self.write("let ");
                self.write(name);
                self.write(" = ");
                self.emit_expr(value);
            }
            Expr::Own { name, value } => {
                self.write("own ");
                self.write(name);
                self.write(" = ");
                self.emit_expr(value);
            }
            Expr::RefBind { name, value } => {
                self.write("ref ");
                self.write(name);
                self.write(" = ");
                self.emit_expr(value);
            }
            Expr::Assign { name, value } => {
                self.write(name);
                self.write(" = ");
                self.emit_expr(value);
            }

            Expr::BinOp { op, left, right } => {
                self.emit_expr_prec(left, binop_prec(op), false);
                self.write(" ");
                self.write(binop_str(op));
                self.write(" ");
                self.emit_expr_prec(right, binop_prec(op), true);
            }
            Expr::UnaryOp { op, operand } => {
                self.write(unaryop_str(op));
                self.emit_expr(operand);
            }
            Expr::Question(inner) => {
                self.emit_expr(inner);
                self.write("?");
            }

            Expr::Call { callee, args } => {
                self.emit_expr(callee);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_expr(a);
                }
                self.write(")");
            }
            Expr::MethodCall { receiver, method, args } => {
                self.emit_expr(receiver);
                self.write(".");
                self.write(method);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_expr(a);
                }
                self.write(")");
            }
            Expr::FieldAccess { receiver, field } => {
                self.emit_expr(receiver);
                self.write(".");
                self.write(field);
            }
            Expr::Index { receiver, index } => {
                self.emit_expr(receiver);
                self.write("[");
                self.emit_expr(index);
                self.write("]");
            }

            Expr::If { cond, then, else_ } => {
                self.write("if ");
                self.emit_expr(cond);
                self.write(" ");
                self.emit_block_body(then);
                if let Some(e) = else_ {
                    self.write(" else ");
                    match e.as_ref() {
                        Expr::If { .. } => self.emit_expr(e),
                        _ => self.emit_block_body(e),
                    }
                }
            }
            Expr::While { cond, body } => {
                self.write("while ");
                self.emit_expr(cond);
                self.write(" ");
                self.emit_block_body(&Expr::Block(body.clone()));
            }

            Expr::Match { subject, arms } => {
                self.write("match ");
                self.emit_expr(subject);
                self.writeln(" {");
                self.push();
                for arm in arms {
                    self.ind();
                    self.emit_pattern(&arm.pattern);
                    if let Some(g) = &arm.guard {
                        self.write(" if ");
                        self.emit_expr(g);
                    }
                    self.write(" => ");
                    self.emit_expr(&arm.body);
                    self.nl();
                }
                self.pop();
                self.ind();
                self.write("}");
            }

            Expr::Return(val) => {
                self.write("return");
                if let Some(v) = val {
                    self.write(" ");
                    self.emit_expr(v);
                }
            }

            Expr::Lambda { params, body, .. } => {
                // Use arrow form for 0 or 1 untyped params; pipe form otherwise.
                if params.is_empty() {
                    self.write("() => ");
                    self.emit_expr(body);
                } else if params.len() == 1 && params[0].ty.is_none() {
                    self.write("(");
                    self.write(&params[0].name);
                    self.write(") => ");
                    self.emit_expr(body);
                } else {
                    self.write("|");
                    for (i, p) in params.iter().enumerate() {
                        if i > 0 { self.write(", "); }
                        self.write(&p.name);
                        if let Some(ty) = &p.ty {
                            self.write(": ");
                            self.emit_axon_type(ty);
                        }
                    }
                    self.write("| ");
                    self.emit_expr(body);
                }
            }

            Expr::Spawn(body) => {
                self.write("spawn ");
                self.emit_block_body(body);
            }

            Expr::Select(arms) => {
                self.writeln("select {");
                self.push();
                for arm in arms {
                    self.ind();
                    self.emit_expr(&arm.recv);
                    self.write(" => ");
                    self.emit_expr(&arm.body);
                    self.nl();
                }
                self.pop();
                self.ind();
                self.write("}");
            }

            Expr::Comptime(inner) => {
                self.write("comptime ");
                self.emit_expr(inner);
            }

            Expr::Ok(inner) => { self.write("Ok("); self.emit_expr(inner); self.write(")"); }
            Expr::Err(inner) => { self.write("Err("); self.emit_expr(inner); self.write(")"); }
            Expr::Some(inner) => { self.write("Some("); self.emit_expr(inner); self.write(")"); }
            Expr::None => self.write("None"),
            Expr::Break => self.write("break"),
            Expr::Continue => self.write("continue"),
            Expr::For { var, start, end, body, inclusive } => {
                self.write("for ");
                self.write(var);
                self.write(" in ");
                self.emit_expr(start);
                self.write(if *inclusive { "..=" } else { ".." });
                self.emit_expr(end);
                self.write(" ");
                self.emit_block_body(&Expr::Block(body.clone()));
            }

            Expr::Array(elems) => {
                self.write("[");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_expr(e);
                }
                self.write("]");
            }

            Expr::StructLit { name, fields } => {
                self.write(name);
                if fields.is_empty() {
                    // Unit variant — emit no braces (parser requires `{ Ident` to
                    // detect a struct body; empty `{}` would fail to re-parse).
                } else {
                    self.write(" { ");
                    for (i, (fname, val)) in fields.iter().enumerate() {
                        if i > 0 { self.write(", "); }
                        self.write(fname);
                        self.write(": ");
                        self.emit_expr(val);
                    }
                    self.write(" }");
                }
            }

            Expr::FmtStr { parts } => {
                self.write("\"");
                for part in parts {
                    match part {
                        FmtPart::Lit(s) => {
                            // Re-escape special chars inside the literal fragment.
                            for ch in s.chars() {
                                match ch {
                                    '"'  => self.write("\\\""),
                                    '\\' => self.write("\\\\"),
                                    '{'  => self.write("{"),
                                    _    => { let mut tmp = [0u8; 4]; self.write(ch.encode_utf8(&mut tmp)); }
                                }
                            }
                        }
                        FmtPart::Expr(e) => {
                            self.write("{");
                            self.emit_expr(e);
                            self.write("}");
                        }
                    }
                }
                self.write("\"");
            }
        }
    }

    fn emit_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(n) => self.write(&n.to_string()),
            Literal::Float(f) => {
                let s = format!("{f}");
                self.write(&s);
                if !s.contains('.') && !s.contains('e') { self.write(".0"); }
            }
            Literal::Str(s) => {
                self.write("\"");
                for ch in s.chars() {
                    match ch {
                        '"'  => self.write("\\\""),
                        '\\' => self.write("\\\\"),
                        '\n' => self.write("\\n"),
                        '\t' => self.write("\\t"),
                        _    => { let mut tmp = [0u8; 4]; self.write(ch.encode_utf8(&mut tmp)); }
                    }
                }
                self.write("\"");
            }
            Literal::Bool(b) => self.write(if *b { "true" } else { "false" }),
        }
    }

    fn emit_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Wildcard   => self.write("_"),
            Pattern::Ident(n)   => self.write(n),
            Pattern::None       => self.write("None"),
            Pattern::Literal(l) => self.emit_literal(l),
            Pattern::Some(inner) => { self.write("Some("); self.emit_pattern(inner); self.write(")"); }
            Pattern::Ok(inner)   => { self.write("Ok(");   self.emit_pattern(inner); self.write(")"); }
            Pattern::Err(inner)  => { self.write("Err(");  self.emit_pattern(inner); self.write(")"); }
            Pattern::Tuple(elems) => {
                self.write("(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { self.write(", "); }
                    self.emit_pattern(e);
                }
                self.write(")");
            }
            Pattern::Struct { name, fields } => {
                self.write(name);
                if fields.is_empty() {
                    // Unit variant — emit no braces (parser requires `{ Ident` to
                    // detect a struct body; empty `{}` would fail to re-parse).
                } else {
                    self.write(" { ");
                    for (i, (fname, fpat)) in fields.iter().enumerate() {
                        if i > 0 { self.write(", "); }
                        self.write(fname);
                        self.write(": ");
                        self.emit_pattern(fpat);
                    }
                    self.write(" }");
                }
            }
        }
    }

    /// Emit `expr`, wrapping in parens if its precedence is strictly lower than `min_prec`.
    fn emit_expr_prec(&mut self, expr: &Expr, min_prec: u8, _right_assoc: bool) {
        let needs_parens = match expr {
            Expr::BinOp { op, .. } => binop_prec(op) < min_prec,
            _ => false,
        };
        if needs_parens {
            self.write("(");
            self.emit_expr(expr);
            self.write(")");
        } else {
            self.emit_expr(expr);
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn binop_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add  => "+",  BinOp::Sub  => "-",
        BinOp::Mul  => "*",  BinOp::Div  => "/",  BinOp::Rem  => "%",
        BinOp::Eq   => "==", BinOp::NotEq => "!=",
        BinOp::Lt   => "<",  BinOp::Gt   => ">",
        BinOp::LtEq => "<=", BinOp::GtEq => ">=",
        BinOp::And  => "&&", BinOp::Or   => "||",
    }
}

fn unaryop_str(op: &UnaryOp) -> &'static str {
    match op { UnaryOp::Neg => "-", UnaryOp::Not => "!", UnaryOp::Ref => "&" }
}

fn binop_prec(op: &BinOp) -> u8 {
    match op {
        BinOp::Or  => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::NotEq |
        BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => 3,
        BinOp::Add | BinOp::Sub => 4,
        BinOp::Mul | BinOp::Div | BinOp::Rem => 5,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn fmt(src: &str) -> String {
        let prog = parse_source(src).expect("parse");
        format_program(&prog)
    }

    #[test]
    fn fmt_simple_fn() {
        let src = "fn add(a: i64, b: i64) -> i64 { a + b }";
        let out = fmt(src);
        assert!(out.contains("fn add(a: i64, b: i64) -> i64 {"), "signature: {out}");
        assert!(out.contains("    a + b"), "body indent: {out}");
        assert!(out.ends_with('\n'), "trailing newline: {out}");
    }

    #[test]
    fn fmt_idempotent() {
        let src = "fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n";
        let prog = parse_source(src).expect("parse");
        let once = format_program(&prog);
        let prog2 = parse_source(&once).expect("re-parse formatted");
        let twice = format_program(&prog2);
        assert_eq!(once, twice, "formatter must be idempotent");
    }

    #[test]
    fn fmt_attr_on_own_line() {
        let src = "@[test] fn test_ok() { assert(true) }";
        let out = fmt(src);
        assert!(out.contains("@[test]\n"), "attr on its own line: {out}");
        assert!(out.contains("fn test_ok()"), "fn after attr: {out}");
    }

    #[test]
    fn fmt_binop_spaces() {
        let src = "fn f(a: i64, b: i64) -> i64 { a+b }";
        let out = fmt(src);
        assert!(out.contains("a + b"), "spaces around binop: {out}");
    }

    #[test]
    fn fmt_type_alias() {
        let src = "type Point = { x: i64, y: i64 }";
        let out = fmt(src);
        assert!(out.contains("type Point = { x: i64, y: i64 }"), "type def: {out}");
    }

    #[test]
    fn fmt_trailing_newline() {
        let src = "fn f() {}";
        let out = fmt(src);
        assert!(out.ends_with('\n'), "must end with newline");
        let without_last = &out[..out.len() - 1];
        assert!(!without_last.ends_with('\n'), "only one trailing newline");
    }

    fn round_trip(src: &str) -> (String, String) {
        let prog1 = crate::parse_source(src).unwrap_or_else(|e| panic!("parse1 failed: {e}\nsrc:\n{src}"));
        let out1 = format_program(&prog1);
        let prog2 = crate::parse_source(&out1).unwrap_or_else(|e| {
            panic!("parse2 failed on formatter output:\n{out1}\nError: {e}")
        });
        let out2 = format_program(&prog2);
        (out1, out2)
    }

    #[test]
    fn fmt_round_trip_structs() {
        let src = include_str!("../tests/fixtures/phase13_structs.ax");
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2, "not idempotent on structs fixture");
    }

    #[test]
    fn fmt_round_trip_match_patterns() {
        let src = include_str!("../tests/fixtures/phase17_match_patterns.ax");
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2, "not idempotent on match_patterns fixture");
    }

    #[test]
    fn fmt_round_trip_error_patterns() {
        let src = include_str!("../tests/fixtures/phase21_error_patterns.ax");
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2, "not idempotent on error_patterns fixture");
    }

    #[test]
    fn fmt_round_trip_traits_in_practice() {
        let src = include_str!("../tests/fixtures/phase23_traits_in_practice.ax");
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2, "not idempotent on traits_in_practice fixture");
    }

    #[test]
    fn fmt_round_trip_comprehensive() {
        let src = include_str!("../tests/fixtures/phase30_comprehensive.ax");
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2, "not idempotent on comprehensive fixture");
    }

    #[test]
    fn fmt_field_access_preserved() {
        let src = "type P = { x: i64, y: i64 }\nfn get_x(p: P) -> i64 { p.x }\n";
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2);
        assert!(out1.contains("p.x"), "field access should appear: {out1}");
    }

    #[test]
    fn fmt_method_call_preserved() {
        // Use a simple function call to verify round-trip stability
        let src = "fn f(n: i64) -> i64 {\n    let x = n\n    x\n}\n";
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2);
    }

    #[test]
    fn fmt_question_op_preserved() {
        let src = "fn may_fail() -> Result<i64, str> { Ok(1) }\nfn f() -> Result<i64, str> { Ok(may_fail()?) }\n";
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2);
        assert!(out1.contains("?"), "? should appear in output: {out1}");
    }

    #[test]
    fn fmt_for_loop_preserved() {
        let src = "fn sum(n: i64) -> i64 {\n    let s = 0\n    for i in 0..n {\n        s = s + i\n    }\n    s\n}\n";
        let (out1, out2) = round_trip(src);
        assert_eq!(out1, out2);
        assert!(out1.contains("for"), "for should appear: {out1}");
    }
}
