use crate::ast::*;
use crate::span::Span;
use crate::token::Token;

// ── Format-string helpers ─────────────────────────────────────────────────────

/// Parse a raw string that may contain `{...}` interpolation markers.
///
/// Escape sequences:
///   `{{`  →  literal `{`
///   `}}`  →  literal `}`
///
/// If the string contains no unescaped `{`, return a plain `Expr::Literal(Literal::Str)`.
/// Otherwise, split into alternating literal and expression segments and return
/// `Expr::FmtStr { parts }`.
fn parse_fmt_str_raw(raw: &str) -> Result<Expr> {
    // Fast path: no braces at all.
    if !raw.contains('{') && !raw.contains('}') {
        return Ok(Expr::Literal(Literal::Str(raw.to_string())));
    }

    let mut parts: Vec<FmtPart> = Vec::new();
    let mut remaining = raw;

    while !remaining.is_empty() {
        let next_open  = remaining.find('{');
        let next_close = remaining.find('}');

        // Determine which brace comes first (or if there are none).
        let next_brace = match (next_open, next_close) {
            (None, None) => break,
            (Some(o), None) => ('o', o),
            (None, Some(c)) => ('c', c),
            (Some(o), Some(c)) => if o <= c { ('o', o) } else { ('c', c) },
        };

        match next_brace {
            ('c', c) => {
                // `}}` → literal `}`, lone `}` → treated as literal `}`.
                if c > 0 {
                    parts.push(FmtPart::Lit(remaining[..c].to_string()));
                }
                if remaining[c..].starts_with("}}") {
                    parts.push(FmtPart::Lit("}".to_string()));
                    remaining = &remaining[c + 2..];
                } else {
                    // Lone `}` — treat as literal.
                    parts.push(FmtPart::Lit("}".to_string()));
                    remaining = &remaining[c + 1..];
                }
            }
            _ => {
                let open = next_brace.1;
                // Literal text before the `{`.
                if open > 0 {
                    parts.push(FmtPart::Lit(remaining[..open].to_string()));
                }
                // `{{` → literal `{`.
                if remaining[open..].starts_with("{{") {
                    parts.push(FmtPart::Lit("{".to_string()));
                    remaining = &remaining[open + 2..];
                    continue;
                }
                // Regular interpolation `{expr}`.
                let after_open = &remaining[open + 1..];
                let close = after_open.find('}').ok_or_else(|| {
                    ParseError::Other("unclosed `{` in interpolated string".into())
                })?;
                let inner = after_open[..close].trim();
                let expr = parse_fmt_inner_expr(inner)?;
                parts.push(FmtPart::Expr(Box::new(expr)));
                remaining = &after_open[close + 1..];
            }
        }
    }

    // Any trailing literal text.
    if !remaining.is_empty() {
        parts.push(FmtPart::Lit(remaining.to_string()));
    }

    // Optimise: single literal segment (no actual interpolation).
    if parts.len() == 1 {
        if let FmtPart::Lit(s) = &parts[0] {
            return Ok(Expr::Literal(Literal::Str(s.clone())));
        }
    }

    // Empty format string.
    if parts.is_empty() {
        return Ok(Expr::Literal(Literal::Str(String::new())));
    }

    Ok(Expr::FmtStr { parts })
}

/// Parse the expression inside `{...}` in a format string using the real lexer and parser.
/// This supports arbitrary expressions, e.g. `{to_str(x + 1)}`.
fn parse_fmt_inner_expr(inner: &str) -> Result<Expr> {
    let tokens = crate::lexer::Lexer::tokenize(inner)
        .map_err(|e| ParseError::Other(format!("fmt expr: {:?}", e)))?;
    let token_vals: Vec<Token> = tokens.into_iter().map(|(t, _)| t).collect();
    let mut sub = Parser::new(token_vals);
    sub.parse_expr()
}

pub struct Parser {
    tokens: Vec<Token>,
    /// Byte-offset spans parallel to `tokens` (from the lexer).
    spans: Vec<Span>,
    /// `true` for token[i] when at least one `\n` preceded it in the source.
    newlines: Vec<bool>,
    pos: usize,
    /// Number of currently open `(` or `[` delimiters.  When >0, ASI newline
    /// guards on binary operators are suppressed so that multi-line expressions
    /// inside parentheses / brackets parse correctly.
    paren_depth: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("unexpected end of input")]
    Eof,
    #[error("unexpected token: {0:?}, expected {1}")]
    Unexpected(Token, String),
    #[error("parse error: {0}")]
    Other(String),
}

type Result<T> = std::result::Result<T, ParseError>;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let len = tokens.len();
        Self { tokens, spans: vec![Span::dummy(); len], newlines: vec![false; len], pos: 0, paren_depth: 0 }
    }

    pub fn with_spans(tokens: Vec<Token>, spans: Vec<Span>) -> Self {
        let len = tokens.len();
        Self { tokens, spans, newlines: vec![false; len], pos: 0, paren_depth: 0 }
    }

    pub fn with_newlines(tokens: Vec<Token>, spans: Vec<Span>, newlines: Vec<bool>) -> Self {
        Self { tokens, spans, newlines, pos: 0, paren_depth: 0 }
    }

    fn current_span(&self) -> Span {
        self.spans.get(self.pos).copied().unwrap_or(Span::dummy())
    }

    #[allow(dead_code)]
    fn span_at(&self, pos: usize) -> Span {
        self.spans.get(pos).copied().unwrap_or(Span::dummy())
    }

    /// Returns `true` if the token at the current position was preceded by a
    /// newline in the source AND we are not inside an open parenthesis/bracket
    /// context (where ASI is suppressed).
    fn preceded_by_newline(&self) -> bool {
        if self.paren_depth > 0 { return false; }
        self.newlines.get(self.pos).copied().unwrap_or(false)
    }

    // ── Primitives ───────────────────────────────────────────────────────────

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Result<&Token> {
        let tok = self.tokens.get(self.pos).ok_or(ParseError::Eof)?;
        self.pos += 1;
        Ok(tok)
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        let tok = self.advance()?;
        if std::mem::discriminant(tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            Err(ParseError::Unexpected(tok.clone(), format!("{expected:?}")))
        }
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.peek().map(|t| std::mem::discriminant(t) == std::mem::discriminant(tok)).unwrap_or(false) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance()? {
            Token::Ident(s) => Ok(s.clone()),
            tok => Err(ParseError::Unexpected(tok.clone(), "identifier".into())),
        }
    }

    fn at(&self, tok: &Token) -> bool {
        self.peek().map(|t| std::mem::discriminant(t) == std::mem::discriminant(tok)).unwrap_or(false)
    }

    /// Returns true if the current position looks like the start of a paren-style
    /// lambda: `(` ... `)` `=>`.  Uses lookahead only; does not consume tokens.
    fn is_paren_lambda(&self) -> bool {
        if !matches!(self.tokens.get(self.pos), Some(Token::LParen)) {
            return false;
        }
        let mut depth: usize = 0;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i] {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.tokens.get(i + 1), Some(Token::FatArrow));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Parse `(params) => body` arrow-style lambda.
    fn parse_paren_lambda(&mut self) -> Result<Expr> {
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        while !self.at(&Token::RParen) {
            let name = self.expect_ident()?;
            let ty = if self.eat(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            params.push(LambdaParam { name, ty });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen)?;
        self.expect(&Token::FatArrow)?;
        let body = self.parse_logical()?;
        Ok(Expr::Lambda { params, body: Box::new(body), captures: Vec::new() })
    }

    // ── Program ──────────────────────────────────────────────────────────────

    pub fn parse_program(&mut self) -> Result<Program> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(self.parse_item()?);
        }
        Ok(Program { items })
    }

    fn parse_item(&mut self) -> Result<Item> {
        let mut attrs = Vec::new();
        while self.at(&Token::At) || self.at(&Token::Hash) {
            attrs.push(self.parse_attr()?);
        }
        let public = self.eat(&Token::Pub);
        match self.peek() {
            Some(Token::Fn)    => Ok(Item::FnDef(self.parse_fn(public, attrs)?)),
            Some(Token::Type)  => Ok(Item::TypeDef(self.parse_type_def()?)),
            Some(Token::Enum)  => Ok(Item::EnumDef(self.parse_enum_def()?)),
            Some(Token::Mod)   => Ok(Item::ModDecl(self.parse_mod()?)),
            Some(Token::Use)   => Ok(Item::UseDecl(self.parse_use()?)),
            Some(Token::Trait) => Ok(Item::TraitDef(self.parse_trait_def()?)),
            Some(Token::Impl)  => Ok(Item::ImplBlock(self.parse_impl_block()?)),
            Some(Token::Let)   => {
                let start = self.current_span().start;
                let _ = self.advance();
                let name = self.expect_ident()?;
                self.expect(&Token::Eq)?;
                let value = self.parse_expr()?;
                let span = Span::new(start, self.current_span().end);
                Ok(Item::LetDef { name, value: Box::new(value), span })
            }
            Some(tok) => Err(ParseError::Unexpected(tok.clone(), "item (fn/type/enum/mod/use/trait/impl/let)".into())),
            None => Err(ParseError::Eof),
        }
    }

    fn parse_attr(&mut self) -> Result<Attr> {
        // Accept both `@[attr]` (Axon-native) and `#[attr]` (Rust-style) syntax.
        if !self.eat(&Token::At) {
            self.expect(&Token::Hash)?;
        }
        self.expect(&Token::LBracket)?;
        let name = self.expect_ident()?;
        let mut args = Vec::new();
        if self.eat(&Token::LParen) {
            while !self.at(&Token::RParen) {
                args.push(self.expect_ident()?);
                self.eat(&Token::Comma);
            }
            self.expect(&Token::RParen)?;
        }
        self.expect(&Token::RBracket)?;
        Ok(Attr { name, args })
    }

    // ── Functions ────────────────────────────────────────────────────────────

    fn parse_fn(&mut self, public: bool, attrs: Vec<Attr>) -> Result<FnDef> {
        let start = self.current_span().start;
        self.expect(&Token::Fn)?;
        let name = self.expect_ident()?;
        let (generic_params, generic_bounds) = self.parse_generic_params()?;
        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;
        let return_type = if self.eat(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        let end = self.current_span().end;
        Ok(FnDef { public, name, generic_params, generic_bounds, params, return_type, body, attrs, span: Span::new(start, end) })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = Vec::new();
        while !self.at(&Token::RParen) {
            // Allow `self` as a parameter name (for trait impls).
            let pspan = self.current_span();
            let name = if self.eat(&Token::SelfKw) {
                "self".to_string()
            } else {
                self.expect_ident()?
            };
            // Bare `self` without `: Type` annotation (e.g. `fn greet(self)`) uses `Self`.
            if name == "self" && !self.at(&Token::Colon) {
                let end = self.current_span().end;
                params.push(Param {
                    name,
                    ty: AxonType::Named("Self".into()),
                    span: Span::new(pspan.start, end),
                });
                if !self.eat(&Token::Comma) { break; }
                continue;
            }
            self.expect(&Token::Colon)?;
            let ty = self.parse_type()?;
            let end = self.current_span().end;
            params.push(Param { name, ty, span: Span::new(pspan.start, end) });
            if !self.eat(&Token::Comma) { break; }
        }
        Ok(params)
    }

    /// Parse optional generic type parameter list: `<A, B: Trait1 + Trait2, C>` after an item name.
    /// Returns `(param_names, bounds)` where `bounds` is `Vec<(param_name, trait_names)>`.
    /// Bounds are only stored for entries that actually have a `:` clause.
    fn parse_generic_params(&mut self) -> Result<(Vec<String>, Vec<(String, Vec<String>)>)> {
        // Lookahead: `<` followed by an ident (type param) or `>` (empty list).
        // Distinguish from `<` as a comparison operator.
        let is_generic_start = self.at(&Token::Lt)
            && matches!(
                self.tokens.get(self.pos + 1),
                Some(Token::Ident(_)) | Some(Token::Gt)
            );
        if !is_generic_start {
            return Ok((Vec::new(), Vec::new()));
        }
        self.advance()?; // consume `<`
        let mut params = Vec::new();
        let mut bounds: Vec<(String, Vec<String>)> = Vec::new();
        while !self.at(&Token::Gt) {
            let name = self.expect_ident()?;
            if self.eat(&Token::Colon) {
                let mut traits = vec![self.expect_ident()?];
                while self.eat(&Token::Plus) {
                    traits.push(self.expect_ident()?);
                }
                bounds.push((name.clone(), traits));
            }
            params.push(name);
            if !self.eat(&Token::Comma) { break; }
        }
        self.expect(&Token::Gt)?;
        Ok((params, bounds))
    }

    // ── Types ────────────────────────────────────────────────────────────────

    fn parse_type(&mut self) -> Result<AxonType> {
        let first = self.parse_type_atom()?;
        // TypeScript-style union: `A|B|C`. Collect successive `| T` atoms.
        if self.at(&Token::Pipe) {
            let mut members = vec![first];
            while self.eat(&Token::Pipe) {
                members.push(self.parse_type_atom()?);
            }
            return Ok(AxonType::Union(members));
        }
        Ok(first)
    }

    fn parse_type_atom(&mut self) -> Result<AxonType> {
        // `dyn Trait` — trait object type
        if self.eat(&Token::Dyn) {
            let name = self.expect_ident()?;
            return Ok(AxonType::DynTrait(name));
        }
        if self.eat(&Token::Ampersand) {
            return Ok(AxonType::Ref(Box::new(self.parse_type_atom()?)));
        }
        // Tuple type: `(T1, T2, ...)` or unit `()`.
        // Disambiguation: `(` followed by `)` → unit type `()`.
        //                 `(` followed by a type → tuple (must have ≥1 comma).
        if self.at(&Token::LParen) {
            // Check for `()` unit type first.
            if matches!(self.tokens.get(self.pos + 1), Some(Token::RParen)) {
                self.advance()?; // consume `(`
                self.advance()?; // consume `)`
                return Ok(AxonType::Named("()".to_string()));
            }
            // Parse `(T, T, ...)` tuple type — requires at least one comma.
            self.advance()?; // consume `(`
            let first = self.parse_type()?;
            if self.eat(&Token::Comma) {
                // We have a real tuple.
                let mut elems = vec![first];
                while !self.at(&Token::RParen) {
                    elems.push(self.parse_type()?);
                    if !self.eat(&Token::Comma) { break; }
                }
                self.expect(&Token::RParen)?;
                return Ok(AxonType::Tuple(elems));
            } else {
                // Parenthesised type — just a grouping `(T)`.
                self.expect(&Token::RParen)?;
                return Ok(first);
            }
        }
        if self.eat(&Token::LBracket) {
            let inner = self.parse_type()?;
            self.expect(&Token::RBracket)?;
            return Ok(AxonType::Slice(Box::new(inner)));
        }
        // `fn(P0, P1) -> R` — function type
        if self.eat(&Token::Fn) {
            self.expect(&Token::LParen)?;
            let mut params = Vec::new();
            while !self.at(&Token::RParen) {
                params.push(self.parse_type()?);
                if !self.eat(&Token::Comma) { break; }
            }
            self.expect(&Token::RParen)?;
            let ret = if self.eat(&Token::Arrow) {
                self.parse_type()?
            } else {
                AxonType::Named("()".to_string())
            };
            return Ok(AxonType::Fn { params, ret: Box::new(ret) });
        }
        let name = self.expect_ident()?;
        match name.as_str() {
            "Result" => {
                self.expect(&Token::Lt)?;
                let ok = self.parse_type()?;
                self.expect(&Token::Comma)?;
                let err = self.parse_type()?;
                self.expect(&Token::Gt)?;
                Ok(AxonType::Result { ok: Box::new(ok), err: Box::new(err) })
            }
            "Option" => {
                self.expect(&Token::Lt)?;
                let inner = self.parse_type()?;
                self.expect(&Token::Gt)?;
                Ok(AxonType::Option(Box::new(inner)))
            }
            "Chan" | "chan" => {
                self.expect(&Token::Lt)?;
                let inner = self.parse_type()?;
                self.expect(&Token::Gt)?;
                Ok(AxonType::Chan(Box::new(inner)))
            }
            _ if self.at(&Token::Lt) => {
                self.advance()?;
                let mut args = Vec::new();
                while !self.at(&Token::Gt) {
                    args.push(self.parse_type()?);
                    if !self.eat(&Token::Comma) { break; }
                }
                self.expect(&Token::Gt)?;
                Ok(AxonType::Generic { base: name, args })
            }
            _ => Ok(AxonType::Named(name)),
        }
    }

    // ── Struct / Enum / Mod / Use ─────────────────────────────────────────────

    fn parse_type_def(&mut self) -> Result<TypeDef> {
        let start = self.current_span().start;
        self.expect(&Token::Type)?;
        let name = self.expect_ident()?;
        let (generic_params, _) = self.parse_generic_params()?;
        self.expect(&Token::Eq)?;
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while !self.at(&Token::RBrace) {
            let fname = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let ty = self.parse_type()?;
            fields.push(TypeField { name: fname, ty });
            self.eat(&Token::Comma);
        }
        self.expect(&Token::RBrace)?;
        let end = self.current_span().end;
        Ok(TypeDef { name, generic_params, fields, span: Span::new(start, end) })
    }

    fn parse_enum_def(&mut self) -> Result<EnumDef> {
        let start = self.current_span().start;
        self.expect(&Token::Enum)?;
        let name = self.expect_ident()?;
        let (generic_params, _) = self.parse_generic_params()?;
        self.expect(&Token::LBrace)?;
        let mut variants = Vec::new();
        while !self.at(&Token::RBrace) {
            let vname = self.expect_ident()?;
            let fields = if self.eat(&Token::LBrace) {
                let mut fs = Vec::new();
                while !self.at(&Token::RBrace) {
                    let fn_ = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let ty = self.parse_type()?;
                    fs.push(TypeField { name: fn_, ty });
                    self.eat(&Token::Comma);
                }
                self.expect(&Token::RBrace)?;
                fs
            } else { Vec::new() };
            variants.push(EnumVariant { name: vname, fields });
            self.eat(&Token::Comma);
        }
        self.expect(&Token::RBrace)?;
        let end = self.current_span().end;
        Ok(EnumDef { name, generic_params, variants, span: Span::new(start, end) })
    }

    // ── Phase 3: Trait and impl block parsers ────────────────────────────────

    fn parse_trait_def(&mut self) -> Result<TraitDef> {
        let start = self.current_span().start;
        self.expect(&Token::Trait)?;
        let name = self.expect_ident()?;
        let (generic_params, _) = self.parse_generic_params()?;
        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        while !self.at(&Token::RBrace) {
            let mspan = self.current_span();
            self.expect(&Token::Fn)?;
            let mname = self.expect_ident()?;
            self.expect(&Token::LParen)?;
            // First param may be bare `self` or `self: TraitName`.
            let params = if self.at(&Token::SelfKw) {
                self.advance()?;
                // Check for explicit type annotation: `self: SomeType`
                let self_ty = if self.eat(&Token::Colon) {
                    // Explicit annotation — parse the type but normalize to Self.
                    let _ = self.parse_type()?;
                    AxonType::Named("Self".to_string())
                } else {
                    AxonType::Named("Self".to_string())
                };
                let mut ps = vec![Param { name: "self".into(), ty: self_ty, span: mspan }];
                while self.eat(&Token::Comma) {
                    let pspan = self.current_span();
                    let pname = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let pty = self.parse_type()?;
                    let pend = self.current_span().end;
                    ps.push(Param { name: pname, ty: pty, span: Span::new(pspan.start, pend) });
                }
                ps
            } else {
                self.parse_params()?
            };
            self.expect(&Token::RParen)?;
            let return_type = if self.eat(&Token::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };
            let mend = self.current_span().end;
            methods.push(TraitMethod {
                name: mname,
                params,
                return_type,
                span: Span::new(mspan.start, mend),
            });
            self.eat(&Token::Semi);
        }
        self.expect(&Token::RBrace)?;
        let end = self.current_span().end;
        Ok(TraitDef { name, generic_params, methods, span: Span::new(start, end) })
    }

    fn parse_impl_block(&mut self) -> Result<ImplBlock> {
        let start = self.current_span().start;
        self.expect(&Token::Impl)?;
        let trait_name = self.expect_ident()?;
        // Optional generic params on the impl: `impl Foo<T> for Bar`
        let _impl_generic = self.parse_generic_params()?; // bounds on impl<T> discarded for now
        self.expect(&Token::For)?;
        let for_type = self.parse_type()?;
        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        while !self.at(&Token::RBrace) {
            let mut attrs = Vec::new();
            while self.at(&Token::At) || self.at(&Token::Hash) { attrs.push(self.parse_attr()?); }
            let public = self.eat(&Token::Pub);
            methods.push(self.parse_fn(public, attrs)?);
        }
        self.expect(&Token::RBrace)?;
        let end = self.current_span().end;
        Ok(ImplBlock { trait_name, for_type, methods, span: Span::new(start, end) })
    }

    fn parse_mod(&mut self) -> Result<ModDecl> {
        self.expect(&Token::Mod)?;
        let name = self.expect_ident()?;
        Ok(ModDecl { name })
    }

    fn parse_use(&mut self) -> Result<UseDecl> {
        self.expect(&Token::Use)?;
        let mut path = vec![self.expect_ident()?];
        while self.eat(&Token::Dot) || self.eat(&Token::ColonColon) {
            match self.peek() {
                Some(Token::LBrace) => break,
                _ => path.push(self.expect_ident()?),
            }
        }
        let mut items = Vec::new();
        if self.eat(&Token::LBrace) {
            while !self.at(&Token::RBrace) {
                items.push(self.expect_ident()?);
                self.eat(&Token::Comma);
            }
            self.expect(&Token::RBrace)?;
        }
        Ok(UseDecl { path, items })
    }

    // ── Blocks & Statements ──────────────────────────────────────────────────

    fn parse_block(&mut self) -> Result<Expr> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.at(&Token::RBrace) {
            let span = self.current_span();
            let expr = self.parse_expr()?;
            self.eat(&Token::Semi);
            stmts.push(Stmt { expr, span });
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Block(stmts))
    }

    // ── Expressions ──────────────────────────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Token::Let)      => self.parse_let(),
            Some(Token::Own)      => self.parse_own(),
            Some(Token::Ref)      => self.parse_ref_bind(),
            Some(Token::Return)   => self.parse_return(),
            Some(Token::Match)    => self.parse_match(),
            Some(Token::If)       => self.parse_if(),
            Some(Token::While)    => self.parse_while(),
            Some(Token::For)      => self.parse_for(),
            Some(Token::Break)    => { self.advance(); Ok(Expr::Break) }
            Some(Token::Continue) => { self.advance(); Ok(Expr::Continue) }
            Some(Token::Spawn)    => self.parse_spawn(),
            Some(Token::Select)   => self.parse_select(),
            Some(Token::Chan)     => self.parse_chan_new(),
            // Assignment: Ident = expr (but NOT == which is a comparison)
            // ASI: if `=` is on a new line it starts a new statement, not an assignment.
            Some(Token::Ident(_))
                if matches!(self.tokens.get(self.pos + 1), Some(Token::Eq))
                && !matches!(self.tokens.get(self.pos + 2), Some(Token::Eq))
                && !self.newlines.get(self.pos + 1).copied().unwrap_or(false) =>
            {
                self.parse_assign()
            }
            _                     => self.parse_logical(),
        }
    }

    fn parse_while(&mut self) -> Result<Expr> {
        self.expect(&Token::While)?;
        // `while let <pattern> = <expr> { body }`
        if self.eat(&Token::Let) {
            let pattern = self.parse_pattern()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_logical()?;
            self.expect(&Token::LBrace)?;
            let mut body = Vec::new();
            while !self.at(&Token::RBrace) {
                let span = self.current_span();
                let e = self.parse_expr()?;
                self.eat(&Token::Semi);
                body.push(Stmt { expr: e, span });
            }
            self.expect(&Token::RBrace)?;
            return Ok(Expr::WhileLet { pattern, expr: Box::new(expr), body });
        }
        // `while <cond> { body }`
        let cond = self.parse_logical()?;
        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while !self.at(&Token::RBrace) {
            let span = self.current_span();
            let expr = self.parse_expr()?;
            self.eat(&Token::Semi);
            body.push(Stmt { expr, span });
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::While { cond: Box::new(cond), body })
    }

    /// Parse `for <ident> in <start>..<end> { body }` or
    /// `for <ident> in <start>..=<end> { body }` (inclusive range).
    fn parse_for(&mut self) -> Result<Expr> {
        self.expect(&Token::For)?;
        let var = self.expect_ident()?;
        self.expect(&Token::In)?;
        let start = self.parse_logical()?;
        let inclusive = if self.eat(&Token::DotDotEq) {
            true
        } else {
            self.expect(&Token::DotDot)?;
            false
        };
        let end = self.parse_logical()?;
        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while !self.at(&Token::RBrace) {
            let span = self.current_span();
            let expr = self.parse_expr()?;
            self.eat(&Token::Semi);
            body.push(Stmt { expr, span });
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::For { var, start: Box::new(start), end: Box::new(end), inclusive, body })
    }

    fn parse_assign(&mut self) -> Result<Expr> {
        let name = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        Ok(Expr::Assign { name, value: Box::new(value) })
    }

    fn parse_let(&mut self) -> Result<Expr> {
        self.expect(&Token::Let)?;
        let name = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        Ok(Expr::Let { name, value: Box::new(value) })
    }

    fn parse_own(&mut self) -> Result<Expr> {
        self.expect(&Token::Own)?;
        let name = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        Ok(Expr::Own { name, value: Box::new(value) })
    }

    fn parse_ref_bind(&mut self) -> Result<Expr> {
        self.expect(&Token::Ref)?;
        let name = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        Ok(Expr::RefBind { name, value: Box::new(value) })
    }

    fn parse_return(&mut self) -> Result<Expr> {
        self.expect(&Token::Return)?;
        // ASI: a newline between `return` and the next token ends the return value.
        // This prevents `return\nexpr` from greedily consuming `expr` as the value.
        if self.at(&Token::RBrace) || self.at(&Token::Semi) || self.preceded_by_newline() {
            Ok(Expr::Return(None))
        } else {
            Ok(Expr::Return(Some(Box::new(self.parse_expr()?))))
        }
    }

    fn parse_match(&mut self) -> Result<Expr> {
        self.expect(&Token::Match)?;
        let subject = self.parse_logical()?;
        self.expect(&Token::LBrace)?;
        let mut arms = Vec::new();
        while !self.at(&Token::RBrace) {
            let pattern = self.parse_pattern()?;
            let guard = if self.eat(&Token::If) {
                Some(self.parse_logical()?)
            } else { None };
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            self.eat(&Token::Comma);
            arms.push(MatchArm { pattern, guard, body });
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Match { subject: Box::new(subject), arms })
    }

    fn parse_if(&mut self) -> Result<Expr> {
        self.expect(&Token::If)?;
        let cond = self.parse_logical()?;
        let then = self.parse_block()?;
        let else_ = if self.eat(&Token::Else) {
            Some(Box::new(if self.at(&Token::If) {
                self.parse_if()?
            } else {
                self.parse_block()?
            }))
        } else { None };
        Ok(Expr::If { cond: Box::new(cond), then: Box::new(then), else_ })
    }

    fn parse_spawn(&mut self) -> Result<Expr> {
        self.expect(&Token::Spawn)?;
        let body = self.parse_block()?;
        Ok(Expr::Spawn(Box::new(body)))
    }

    fn parse_select(&mut self) -> Result<Expr> {
        self.expect(&Token::Select)?;
        self.expect(&Token::LBrace)?;
        let mut arms = Vec::new();
        while !self.at(&Token::RBrace) {
            let recv = self.parse_comparison()?;
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            self.eat(&Token::Comma);
            arms.push(SelectArm { recv, body });
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Select(arms))
    }

    fn parse_comptime(&mut self) -> Result<Expr> {
        self.expect(&Token::Comptime)?;
        let body = self.parse_block()?;
        Ok(Expr::Comptime(Box::new(body)))
    }

    /// Parse `chan<T>()` — channel creation expression.
    ///
    /// Lowers to a `Call { callee: StructLit { "Chan::new" }, args: [Literal(16)] }`
    /// with capacity 16. The element type T is recorded for the resolver/infer pass
    /// via the special name `"chan::<T>"` encoded in the callee so infer can produce
    /// `Chan<T>` rather than `Chan<i64>` from the generic builtin.
    fn parse_chan_new(&mut self) -> Result<Expr> {
        self.expect(&Token::Chan)?;
        self.expect(&Token::Lt)?;
        let elem_ty = self.parse_type()?;
        self.expect(&Token::Gt)?;
        self.expect(&Token::LParen)?;
        self.expect(&Token::RParen)?;
        // Encode the elem type in the callee name so infer can extract it.
        let callee_name = format!("chan::<{}>", axon_type_to_str(&elem_ty));
        Ok(Expr::Call {
            callee: Box::new(Expr::StructLit {
                name: callee_name,
                fields: Vec::new(),
            }),
            args: vec![Expr::Literal(crate::ast::Literal::Int(16))],
        })
    }

    // ── Binary ops with precedence climbing ─────────────────────────────────

    /// Lowest-precedence binary layer: `&&` and `||`.
    fn parse_logical(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            // ASI: an operator on a new line (outside parens) terminates the expression.
            if self.preceded_by_newline() { break; }
            let op = match self.peek() {
                Some(Token::And) => BinOp::And,
                Some(Token::Or)  => BinOp::Or,
                _ => break,
            };
            self.advance()?;
            let right = self.parse_comparison()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn is_comparison_op(tok: &Token) -> bool {
        matches!(
            tok,
            Token::EqEq | Token::NotEq | Token::Lt | Token::Gt | Token::LtEq | Token::GtEq
        )
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        // ASI: a comparison operator on a new line (outside parens) ends the expression.
        if self.preceded_by_newline() { return Ok(left); }
        let op = match self.peek() {
            Some(Token::EqEq)  => BinOp::Eq,
            Some(Token::NotEq) => BinOp::NotEq,
            Some(Token::Lt)    => BinOp::Lt,
            Some(Token::Gt)    => BinOp::Gt,
            Some(Token::LtEq)  => BinOp::LtEq,
            Some(Token::GtEq)  => BinOp::GtEq,
            _ => return Ok(left),
        };
        self.advance()?;
        let right = self.parse_additive()?;
        // Reject chained comparisons: `1 < 2 < 3` is almost certainly a bug.
        if self.peek().map(Self::is_comparison_op).unwrap_or(false) {
            return Err(ParseError::Other(
                "chained comparisons are not supported; use parentheses".into(),
            ));
        }
        left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            // ASI: operator at the start of a new line (outside parens) terminates.
            if self.preceded_by_newline() { break; }
            let op = match self.peek() {
                Some(Token::Plus)  => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.advance()?;
            let right = self.parse_multiplicative()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_postfix()?;
        loop {
            // ASI: operator at the start of a new line (outside parens) terminates.
            if self.preceded_by_newline() { break; }
            let op = match self.peek() {
                Some(Token::Star)    => BinOp::Mul,
                Some(Token::Slash)   => BinOp::Div,
                Some(Token::Percent) => BinOp::Rem,
                _ => break,
            };
            self.advance()?;
            let right = self.parse_postfix()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Some(Token::Question) => {
                    self.advance()?;
                    expr = Expr::Question(Box::new(expr));
                }
                Some(Token::Dot) => {
                    self.advance()?;
                    let field = self.expect_ident()?;
                    if self.eat(&Token::LParen) {
                        self.paren_depth += 1;
                        let args = self.parse_args()?;
                        self.paren_depth -= 1;
                        self.expect(&Token::RParen)?;
                        expr = Expr::MethodCall { receiver: Box::new(expr), method: field, args };
                    } else {
                        expr = Expr::FieldAccess { receiver: Box::new(expr), field };
                    }
                }
                // ASI: `(` on a new line is NOT a continuation of a call — it's a
                // new statement (parenthesized expression).  This prevents
                // `foo()\n(bar)` from being mis-parsed as `foo()(bar)`.
                Some(Token::LParen) if !self.preceded_by_newline() => {
                    self.advance()?;
                    self.paren_depth += 1;
                    let args = self.parse_args()?;
                    self.paren_depth -= 1;
                    self.expect(&Token::RParen)?;
                    expr = Expr::Call { callee: Box::new(expr), args };
                }
                // ASI: `[` on a new line is NOT an index operation.
                Some(Token::LBracket) if !self.preceded_by_newline() => {
                    self.advance()?;
                    self.paren_depth += 1;
                    let index = self.parse_expr()?;
                    self.paren_depth -= 1;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::Index { receiver: Box::new(expr), index: Box::new(index) };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.at(&Token::RParen) {
            args.push(self.parse_expr()?);
            if !self.eat(&Token::Comma) { break; }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Token::Comptime) => self.parse_comptime(),
            Some(Token::LBrace)  => self.parse_block(),
            Some(Token::LParen)  => {
                if self.is_paren_lambda() {
                    self.parse_paren_lambda()
                } else {
                    self.advance()?;
                    self.paren_depth += 1;
                    let expr = self.parse_expr()?;
                    self.paren_depth -= 1;
                    self.expect(&Token::RParen)?;
                    Ok(expr)
                }
            }
            Some(Token::Int(_))   => {
                if let Token::Int(n) = self.advance()?.clone() {
                    Ok(Expr::Literal(Literal::Int(n)))
                } else { unreachable!() }
            }
            Some(Token::Float(_)) => {
                if let Token::Float(n) = self.advance()?.clone() {
                    Ok(Expr::Literal(Literal::Float(n)))
                } else { unreachable!() }
            }
            Some(Token::Str(_)) => {
                if let Token::Str(s) = self.advance()?.clone() {
                    parse_fmt_str_raw(&s)
                } else { unreachable!() }
            }
            Some(Token::True) => { self.advance()?; Ok(Expr::Literal(Literal::Bool(true))) }
            Some(Token::False) => { self.advance()?; Ok(Expr::Literal(Literal::Bool(false))) }
            Some(Token::None) => { self.advance()?; Ok(Expr::None) }
            Some(Token::Ok) => {
                self.advance()?;
                self.expect(&Token::LParen)?;
                self.paren_depth += 1;
                let inner = self.parse_expr()?;
                self.paren_depth -= 1;
                self.expect(&Token::RParen)?;
                Ok(Expr::Ok(Box::new(inner)))
            }
            Some(Token::Err) => {
                self.advance()?;
                self.expect(&Token::LParen)?;
                self.paren_depth += 1;
                let inner = self.parse_expr()?;
                self.paren_depth -= 1;
                self.expect(&Token::RParen)?;
                Ok(Expr::Err(Box::new(inner)))
            }
            Some(Token::Some) => {
                self.advance()?;
                self.expect(&Token::LParen)?;
                self.paren_depth += 1;
                let inner = self.parse_expr()?;
                self.paren_depth -= 1;
                self.expect(&Token::RParen)?;
                Ok(Expr::Some(Box::new(inner)))
            }
            Some(Token::LBracket) => {
                self.advance()?;
                self.paren_depth += 1;
                let mut elems = Vec::new();
                while !self.at(&Token::RBracket) {
                    elems.push(self.parse_expr()?);
                    if !self.eat(&Token::Comma) { break; }
                }
                self.paren_depth -= 1;
                self.expect(&Token::RBracket)?;
                Ok(Expr::Array(elems))
            }
            Some(Token::Or) => {
                // Empty-parameter lambda: `|| body` (tokenized as Or, not Pipe+Pipe).
                self.advance()?;
                let body = self.parse_logical()?;
                Ok(Expr::Lambda { params: Vec::new(), body: Box::new(body), captures: Vec::new() })
            }
            Some(Token::Pipe) => {
                // Lambda expression: `|params| body`
                self.advance()?; // consume first `|`
                let mut params = Vec::new();
                while !self.at(&Token::Pipe) {
                    let pname = self.expect_ident()?;
                    // Use `parse_type_atom` (not `parse_type`) so the closing
                    // `|` is not mis-parsed as a union-type continuation.
                    let ty = if self.eat(&Token::Colon) {
                        Some(self.parse_type_atom()?)
                    } else { None };
                    params.push(LambdaParam { name: pname, ty });
                    if !self.eat(&Token::Comma) { break; }
                }
                self.expect(&Token::Pipe)?; // consume closing `|`
                let body = self.parse_logical()?;
                Ok(Expr::Lambda { params, body: Box::new(body), captures: Vec::new() })
            }
            Some(Token::Ident(_)) => {
                if let Token::Ident(name) = self.advance()?.clone() {
                    // Check for enum variant: Name :: Variant { ... }
                    // Now uses the dedicated `ColonColon` token.
                    let is_enum_variant = self.at(&Token::ColonColon);
                    if is_enum_variant {
                        self.advance()?; // consume `::`
                        let variant = self.expect_ident()?;
                        let full_name = format!("{name}::{variant}");
                        // Guard against Foo::Bar::Baz — nested paths are not supported.
                        if self.at(&Token::ColonColon) {
                            return Err(ParseError::Other(
                                "nested enum paths like 'A::B::C' are not supported".into(),
                            ));
                        }
                        // Optionally parse `{ field: expr, ... }` (with or without `:` for shorthand).
                        // A brace is a variant body if:
                        //  - next is `{` followed by an Ident (field name)
                        //  - after that is `:`, `,`, or `}` (not `=>` or some expression start)
                        let is_variant_body = self.at(&Token::LBrace)
                            && matches!(self.tokens.get(self.pos + 1), Some(Token::Ident(_)));
                        if is_variant_body {
                            self.advance()?; // consume `{`
                            let mut fields = Vec::new();
                            while !self.at(&Token::RBrace) {
                                let fname = self.expect_ident()?;
                                // Support `field: expr` or shorthand `field`.
                                let fval = if self.eat(&Token::Colon) {
                                    self.parse_expr()?
                                } else {
                                    // Shorthand: field name is also the variable being bound.
                                    Expr::Ident(fname.clone())
                                };
                                fields.push((fname, fval));
                                if !self.eat(&Token::Comma) { break; }
                            }
                            self.expect(&Token::RBrace)?;
                            Ok(Expr::StructLit { name: full_name, fields })
                        } else {
                            // Unit variant: no fields → empty struct lit
                            Ok(Expr::StructLit { name: full_name, fields: Vec::new() })
                        }
                    } else {
                        // Struct literal: Name { field: expr, ... }
                        // Disambiguate from a block by requiring `ident :` after `{`.
                        // Also check that the colon is NOT followed by another colon
                        // (which would indicate `Enum::Variant` inside a block, not a field).
                        // `Foo { field: expr }` or `Foo {}` (empty struct).
                        let is_struct_lit = self.at(&Token::LBrace)
                            && (
                                // Non-empty: next tokens are Ident Colon (not ::)
                                (matches!(self.tokens.get(self.pos + 1), Some(Token::Ident(_)))
                                    && matches!(self.tokens.get(self.pos + 2), Some(Token::Colon))
                                    && !matches!(self.tokens.get(self.pos + 3), Some(Token::Colon)))
                                // Empty: `{ }`
                                || matches!(self.tokens.get(self.pos + 1), Some(Token::RBrace))
                            );
                        if is_struct_lit {
                            self.advance()?; // consume `{`
                            let mut fields = Vec::new();
                            while !self.at(&Token::RBrace) {
                                let fname = self.expect_ident()?;
                                self.expect(&Token::Colon)?;
                                let fval = self.parse_expr()?;
                                fields.push((fname, fval));
                                if !self.eat(&Token::Comma) { break; }
                            }
                            self.expect(&Token::RBrace)?;
                            Ok(Expr::StructLit { name, fields })
                        } else {
                            Ok(Expr::Ident(name))
                        }
                    }
                } else { unreachable!() }
            }
            Some(Token::Ampersand) => {
                self.advance()?;
                let operand = self.parse_postfix()?;
                Ok(Expr::UnaryOp { op: UnaryOp::Ref, operand: Box::new(operand) })
            }
            Some(Token::Minus) => {
                self.advance()?;
                let operand = self.parse_postfix()?;
                Ok(Expr::UnaryOp { op: UnaryOp::Neg, operand: Box::new(operand) })
            }
            Some(Token::Bang) => {
                self.advance()?;
                let operand = self.parse_postfix()?;
                Ok(Expr::UnaryOp { op: UnaryOp::Not, operand: Box::new(operand) })
            }
            Some(Token::SelfKw) => {
                self.advance()?;
                Ok(Expr::Ident("self".to_string()))
            }
            Some(tok) => Err(ParseError::Unexpected(tok.clone(), "expression".into())),
            None => Err(ParseError::Eof),
        }
    }

    // ── Patterns ─────────────────────────────────────────────────────────────

    fn parse_pattern(&mut self) -> Result<Pattern> {
        match self.peek() {
            Some(Token::Ident(s)) if s == "_" => {
                self.advance()?;
                Ok(Pattern::Wildcard)
            }
            // Tuple pattern: `(p1, p2, ...)` or unit pattern `()`.
            Some(Token::LParen) => {
                self.advance()?; // consume `(`
                // `()` — unit / empty tuple pattern.
                if self.eat(&Token::RParen) {
                    return Ok(Pattern::Tuple(Vec::new()));
                }
                let first = self.parse_pattern()?;
                if self.eat(&Token::Comma) {
                    let mut pats = vec![first];
                    while !self.at(&Token::RParen) {
                        pats.push(self.parse_pattern()?);
                        if !self.eat(&Token::Comma) { break; }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Pattern::Tuple(pats))
                } else {
                    // Parenthesised pattern — just grouping, no tuple.
                    self.expect(&Token::RParen)?;
                    Ok(first)
                }
            }
            Some(Token::True) => { self.advance()?; Ok(Pattern::Literal(Literal::Bool(true))) }
            Some(Token::False) => { self.advance()?; Ok(Pattern::Literal(Literal::Bool(false))) }
            // Negative integer literal pattern: `-` INT_LIT
            Some(Token::Minus) => {
                self.advance()?; // consume `-`
                let tok = self.advance()?.clone();
                if let Token::Int(n) = tok {
                    Ok(Pattern::Literal(Literal::Int(-n)))
                } else {
                    Err(ParseError::Unexpected(
                        tok,
                        "integer literal after `-` in pattern".into(),
                    ))
                }
            }
            Some(Token::None) => { self.advance()?; Ok(Pattern::None) }
            Some(Token::Some) => {
                self.advance()?;
                self.expect(&Token::LParen)?;
                let inner = self.parse_pattern()?;
                self.expect(&Token::RParen)?;
                Ok(Pattern::Some(Box::new(inner)))
            }
            Some(Token::Ok) => {
                self.advance()?;
                self.expect(&Token::LParen)?;
                let inner = self.parse_pattern()?;
                self.expect(&Token::RParen)?;
                Ok(Pattern::Ok(Box::new(inner)))
            }
            Some(Token::Err) => {
                self.advance()?;
                self.expect(&Token::LParen)?;
                let inner = self.parse_pattern()?;
                self.expect(&Token::RParen)?;
                Ok(Pattern::Err(Box::new(inner)))
            }
            Some(Token::Int(_)) => {
                if let Token::Int(n) = self.advance()?.clone() {
                    Ok(Pattern::Literal(Literal::Int(n)))
                } else { unreachable!() }
            }
            Some(Token::Str(_)) => {
                if let Token::Str(s) = self.advance()?.clone() {
                    Ok(Pattern::Literal(Literal::Str(s)))
                } else { unreachable!() }
            }
            Some(Token::Ident(_)) => {
                if let Token::Ident(name) = self.advance()?.clone() {
                    // Check for enum variant pattern: Enum :: Variant { fields }
                    let is_enum_variant = self.at(&Token::ColonColon);
                    if is_enum_variant {
                        self.advance()?; // consume `::`
                        let variant = self.expect_ident()?;
                        let full_name = format!("{name}::{variant}");
                        // Guard against Foo::Bar::Baz — nested paths are not supported.
                        if self.at(&Token::ColonColon) {
                            return Err(ParseError::Other(
                                "nested enum paths like 'A::B::C' are not supported".into(),
                            ));
                        }
                        // Optionally parse `{ field: pattern, ... }` or shorthand `{ field }`.
                        if self.eat(&Token::LBrace) {
                            let mut fields = Vec::new();
                            while !self.at(&Token::RBrace) {
                                let fname = self.expect_ident()?;
                                // Support shorthand `{ field }` (no colon) as `{ field: Ident(field) }`.
                                let fpat = if self.eat(&Token::Colon) {
                                    self.parse_pattern()?
                                } else {
                                    Pattern::Ident(fname.clone())
                                };
                                fields.push((fname, fpat));
                                if !self.eat(&Token::Comma) { break; }
                            }
                            self.expect(&Token::RBrace)?;
                            Ok(Pattern::Struct { name: full_name, fields })
                        } else {
                            // Unit variant pattern
                            Ok(Pattern::Struct { name: full_name, fields: Vec::new() })
                        }
                    } else {
                        Ok(Pattern::Ident(name))
                    }
                } else { unreachable!() }
            }
            Some(tok) => Err(ParseError::Unexpected(tok.clone(), "pattern".into())),
            None => Err(ParseError::Eof),
        }
    }
}

/// Convert an `AxonType` to its canonical string form for encoding in synthetic names.
fn axon_type_to_str(ty: &AxonType) -> String {
    match ty {
        AxonType::Named(n) => n.clone(),
        AxonType::Chan(inner) => format!("Chan<{}>", axon_type_to_str(inner)),
        AxonType::Slice(inner) => format!("[{}]", axon_type_to_str(inner)),
        AxonType::Option(inner) => format!("Option<{}>", axon_type_to_str(inner)),
        AxonType::Result { ok, err } => format!("Result<{},{}>", axon_type_to_str(ok), axon_type_to_str(err)),
        AxonType::Ref(inner) => format!("&{}", axon_type_to_str(inner)),
        AxonType::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(|p| axon_type_to_str(p)).collect();
            format!("fn({}) -> {}", ps.join(", "), axon_type_to_str(ret))
        }
        AxonType::Generic { base, args } => {
            let as_: Vec<String> = args.iter().map(|a| axon_type_to_str(a)).collect();
            format!("{}<{}>", base, as_.join(", "))
        }
        AxonType::DynTrait(name) => format!("dyn {name}"),
        AxonType::TypeParam(name) => name.clone(),
        AxonType::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| axon_type_to_str(e)).collect();
            format!("({})", inner.join(", "))
        }
        AxonType::Union(members) => {
            let inner: Vec<String> = members.iter().map(|m| axon_type_to_str(m)).collect();
            inner.join("|")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Program {
        let tokens: Vec<Token> = Lexer::tokenize(src).unwrap().into_iter().map(|(t,_)| t).collect();
        Parser::new(tokens).parse_program().expect("parse failed")
    }

    #[test]
    fn test_fn_def() {
        let prog = parse("fn add(a:i32,b:i32)->i32{a+b}");
        assert_eq!(prog.items.len(), 1);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
    }

    #[test]
    fn test_let_binding() {
        let prog = parse("fn main(){let x=42}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(matches!(&stmts[0].expr, Expr::Let { name, .. } if name == "x"));
    }

    #[test]
    fn test_ownership_modes() {
        let prog = parse("fn main(){own x=Vec.new();ref y=&x}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(matches!(&stmts[0].expr, Expr::Own { name, .. } if name == "x"));
        assert!(matches!(&stmts[1].expr, Expr::RefBind { name, .. } if name == "y"));
    }

    #[test]
    fn test_result_type() {
        let prog = parse("fn fetch(url:str)->Result<Response,Error>{Ok(resp)}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert!(matches!(&f.return_type, Some(AxonType::Result { .. })));
    }

    #[test]
    fn test_match_expr() {
        let prog = parse("fn check(x:Option<i32>){match x{Some(v)=>v,None=>0}}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(matches!(&stmts[0].expr, Expr::Match { .. }));
    }

    #[test]
    fn test_enum_def() {
        let prog = parse("enum Shape{Circle{radius:f64},Rect{w:f64,h:f64},Point}");
        let Item::EnumDef(e) = &prog.items[0] else { panic!() };
        assert_eq!(e.name, "Shape");
        assert_eq!(e.variants.len(), 3);
    }

    #[test]
    fn test_question_operator() {
        let prog = parse("fn process(data:Bytes)->Result<Output,Error>{let parsed=parse(data)?}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::Question(_)));
    }

    #[test]
    fn test_mod_and_use() {
        let prog = parse("mod server\nuse server.{listen,Router}");
        assert_eq!(prog.items.len(), 2);
        assert!(matches!(&prog.items[0], Item::ModDecl(_)));
        assert!(matches!(&prog.items[1], Item::UseDecl(_)));
    }

    // ── New operator tests ────────────────────────────────────────────────────

    #[test]
    fn test_modulo_operator() {
        let prog = parse("fn f(){let r=10%3}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::BinOp { op: BinOp::Rem, .. }));
    }

    #[test]
    fn test_logical_and() {
        let prog = parse("fn f(){let r=true&&false}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::BinOp { op: BinOp::And, .. }));
    }

    #[test]
    fn test_logical_or() {
        let prog = parse("fn f(){let r=true||false}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::BinOp { op: BinOp::Or, .. }));
    }

    #[test]
    fn test_chained_comparison_errors() {
        let src = "fn f(){let x=1<2<3}";
        let tokens: Vec<Token> = Lexer::tokenize(src).unwrap().into_iter().map(|(t,_)| t).collect();
        let result = Parser::new(tokens).parse_program();
        assert!(result.is_err(), "chained comparisons should produce a parse error");
    }

    #[test]
    fn test_nested_path_errors() {
        let src = "fn f(){let x=Foo::Bar::Baz}";
        let tokens: Vec<Token> = Lexer::tokenize(src).unwrap().into_iter().map(|(t,_)| t).collect();
        let result = Parser::new(tokens).parse_program();
        assert!(result.is_err(), "Foo::Bar::Baz should produce a parse error");
    }

    #[test]
    fn test_empty_struct_literal() {
        let prog = parse("fn f(){let x=Point{}}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::StructLit { name, fields } if name == "Point" && fields.is_empty()));
    }

    #[test]
    fn test_fmt_str_double_brace_escapes() {
        // {{ → literal {, }} → literal }
        let prog = parse(r#"fn f(){println("{{hello}}")}"#);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        // Should parse without error; the string contains literal braces
        assert!(matches!(&stmts[0].expr, Expr::Call { .. }));
    }

    #[test]
    fn test_fmt_str_unclosed_brace_errors() {
        let src = r#"fn f(){println("hello {name")}"#;
        let tokens: Vec<Token> = Lexer::tokenize(src).unwrap().into_iter().map(|(t,_)| t).collect();
        let result = Parser::new(tokens).parse_program();
        assert!(result.is_err(), "unclosed {{ in fmt string should error");
    }

    #[test]
    fn test_while_loop() {
        let prog = parse("fn f(){let i=0\nwhile i<10{i=i+1}}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(matches!(&stmts[1].expr, Expr::While { .. }));
    }

    #[test]
    fn test_while_let_some() {
        let prog = parse("fn f(){while let Some(x) = next() {x}}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(
            matches!(&stmts[0].expr, Expr::WhileLet { pattern: Pattern::Some(_), .. }),
            "expected WhileLet with Some pattern, got {:?}", &stmts[0].expr
        );
    }

    #[test]
    fn test_assign_rebind() {
        let prog = parse("fn f(){let x=1\nx=2}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(matches!(&stmts[1].expr, Expr::Assign { name, .. } if name == "x"));
    }

    #[test]
    fn test_unary_not() {
        let prog = parse("fn f(){let r=!true}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::UnaryOp { op: UnaryOp::Not, .. }));
    }

    #[test]
    fn test_lambda() {
        let prog = parse("fn f(){let add=|a,b|a+b}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::Lambda { params, .. } if params.len() == 2));
    }

    #[test]
    fn test_arrow_lambda_one_param() {
        let prog = parse("fn f(){let add=(x)=>x+1}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        let Expr::Lambda { params, .. } = value.as_ref() else { panic!("not a lambda") };
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "x");
    }

    #[test]
    fn test_arrow_lambda_zero_params() {
        let prog = parse("fn f(){let g=()=>42}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        let Expr::Lambda { params, .. } = value.as_ref() else { panic!("not a lambda") };
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_arrow_lambda_with_block_body() {
        let prog = parse("fn f(){let g=()=>{42}}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        let Expr::Lambda { params, body, .. } = value.as_ref() else { panic!("not a lambda") };
        assert_eq!(params.len(), 0);
        assert!(matches!(body.as_ref(), Expr::Block(_)));
    }

    #[test]
    fn test_paren_expr_not_lambda() {
        // (x + y) * 2 should NOT be parsed as a lambda
        let prog = parse("fn f(){let r=(a+b)*2}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        // Should be a binary Mul, not a Lambda
        assert!(matches!(value.as_ref(), Expr::BinOp { op: BinOp::Mul, .. }));
    }

    #[test]
    fn test_enum_variant_with_fields() {
        let prog = parse("fn f(){let s=Shape::Circle{radius:1.0}}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::StructLit { name, .. } if name == "Shape::Circle"));
    }

    #[test]
    fn test_precedence_logical_vs_comparison() {
        // x > 3 && y < 20 should parse as (x > 3) && (y < 20)
        let prog = parse("fn f(){let r=x>3&&y<20}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        // Top-level should be And
        assert!(matches!(value.as_ref(), Expr::BinOp { op: BinOp::And, .. }));
    }

    // ── ASI tests ────────────────────────────────────────────────────────────

    fn parse_nl(src: &str) -> Program {
        crate::parse_source(src).expect("parse failed")
    }

    #[test]
    fn asi_return_newline_no_value() {
        // `return` followed by a newline should produce Return(None), not consume `42`.
        let prog = parse_nl("fn f()->i64{\nreturn\n42\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        // First stmt: return with no value
        assert!(matches!(&stmts[0].expr, Expr::Return(None)));
        // Second stmt: the literal 42 (dead code, but parsed as a separate statement)
        assert!(matches!(&stmts[1].expr, Expr::Literal(crate::ast::Literal::Int(42))));
    }

    #[test]
    fn asi_return_same_line_has_value() {
        // `return expr` on the same line should still carry the value.
        let prog = parse_nl("fn f()->i64{return 42}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert!(matches!(&stmts[0].expr, Expr::Return(Some(_))));
    }

    #[test]
    fn asi_call_newline_not_continuation() {
        // `foo()\n(bar)` — the `(` on the next line is a new statement (paren expr),
        // NOT a chained call `foo()(bar)`.
        let prog = parse_nl("fn f(){\nfoo()\n(bar)\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 2, "should be two statements, not a chained call");
        // First: call to foo()
        assert!(matches!(&stmts[0].expr, Expr::Call { .. }));
        // Second: parenthesized ident (bar)
        assert!(matches!(&stmts[1].expr, Expr::Ident(_)));
    }

    #[test]
    fn asi_index_newline_not_continuation() {
        // `foo()\n[0]` — `[` on next line is a separate array literal, not an index.
        // (We just check it doesn't parse as Index on the call result.)
        let prog = parse_nl("fn f(){\nfoo()\n[1,2]\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 2, "should be two statements, not foo()[1,2]");
    }

    #[test]
    fn asi_dot_chain_same_effective_line_ok() {
        // `.method()` on the next line IS allowed as a continuation (common style).
        // The dot is still consumed greedily regardless of newline — this is intentional.
        let prog = parse_nl("fn f(){\nlet x=foo()\n.bar()\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        // Should be ONE let-binding whose value is a method call chain.
        assert_eq!(stmts.len(), 1);
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        assert!(matches!(value.as_ref(), Expr::MethodCall { .. }));
    }

    // ── Binary operator ASI guards ────────────────────────────────────────────

    #[test]
    fn asi_binary_op_newline_is_not_continuation() {
        // `+` at the start of a new line must NOT continue the previous expression.
        // `1\n+2` is NOT `1 + 2` — the `+` starts an invalid new statement.
        let src = "fn f(){\n1\n+2\n}";
        let result = crate::parse_source(src);
        assert!(result.is_err(), "operator at start of new line should not continue the expression");
    }

    #[test]
    fn asi_binary_op_end_of_line_is_continuation() {
        // Operator at the END of a line (before the newline) must still continue.
        // `return 1 +\n2` — the `+` is on the same line as `1`, so `1 + 2` is the return value.
        let prog = parse_nl("fn f()->i64{\nreturn 1 +\n2\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 1);
        let Expr::Return(Some(v)) = &stmts[0].expr else { panic!("expected return with value") };
        assert!(matches!(v.as_ref(), Expr::BinOp { op: BinOp::Add, .. }),
            "return value should be 1 + 2");
    }

    #[test]
    fn asi_comparison_newline_is_not_continuation() {
        // `>` at the start of a new line ends the previous expression.
        let src = "fn f()->i64{\nreturn 1\n>2\n}";
        let result = crate::parse_source(src);
        assert!(result.is_err(), "comparison op at start of new line should not continue");
    }

    #[test]
    fn asi_logical_and_newline_is_not_continuation() {
        // `&&` at the start of a new line ends the previous expression.
        let src = "fn f()->i64{\nreturn true\n&&false\n}";
        let result = crate::parse_source(src);
        assert!(result.is_err(), "&& at start of new line should not continue the expression");
    }

    // ── Assignment `=` ASI guard ──────────────────────────────────────────────

    #[test]
    fn asi_assign_eq_newline_not_assignment() {
        // `foo\n= bar` — the `=` is on a new line and must NOT be parsed as assignment.
        // Instead, `foo` is a standalone expression and `= bar` is a parse error.
        let src = "fn f(){\nlet foo=1\nfoo\n=2\n}";
        let result = crate::parse_source(src);
        assert!(result.is_err(), "= on a new line must not be treated as assignment");
    }

    #[test]
    fn asi_assign_same_line_is_assignment() {
        // `foo = bar` on the same line is still a valid assignment.
        let prog = parse_nl("fn f(){\nlet x=1\nx=2\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[1].expr, Expr::Assign { name, .. } if name == "x"));
    }

    // ── Block comment newline propagation ─────────────────────────────────────

    #[test]
    fn asi_block_comment_newline_prevents_call() {
        // `foo /* multi\nline */ (bar)` — the newline inside the comment means `(`
        // is considered newline-preceded, so it is NOT a call on `foo`.
        let prog = parse_nl("fn f(){\nfoo /* comment\ncontinued */ (bar)\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 2, "newline inside block comment must prevent call chaining");
        assert!(matches!(&stmts[0].expr, Expr::Ident(_)), "first stmt is `foo`");
        assert!(matches!(&stmts[1].expr, Expr::Ident(_)), "second stmt is the parenthesised `bar`");
    }

    #[test]
    fn asi_inline_block_comment_does_not_prevent_call() {
        // `foo /* no newline */ (bar)` — single-line block comment must NOT set the
        // newline flag, so `foo(bar)` parses as a call.
        let prog = parse_nl("fn f(){\nfoo /* inline */ (bar)\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 1, "inline block comment must not prevent call");
        assert!(matches!(&stmts[0].expr, Expr::Call { .. }), "should be a call expression");
    }

    // ── Paren depth suppression ───────────────────────────────────────────────

    #[test]
    fn asi_paren_suppresses_newline_guard_on_binop() {
        // Inside `(...)`, a binary operator on the next line IS a continuation.
        // `(1\n+ 2)` must parse as `1 + 2`, not error.
        let prog = parse_nl("fn f()->i64{\nreturn (1\n+2)\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        assert_eq!(stmts.len(), 1);
        let Expr::Return(Some(v)) = &stmts[0].expr else { panic!("expected return") };
        assert!(matches!(v.as_ref(), Expr::BinOp { op: BinOp::Add, .. }),
            "operator inside parens must still be a continuation");
    }

    #[test]
    fn asi_bracket_suppresses_newline_guard() {
        // Inside `[...]`, operators on the next line are continuations.
        let prog = parse_nl("fn f()->i64{\nlet a=[1\n+2]\na[0]\n}");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        // The array literal `[1+2]` should parse as a single element.
        let Expr::Let { value, .. } = &stmts[0].expr else { panic!() };
        let Expr::Array(elems) = value.as_ref() else { panic!("expected array") };
        assert_eq!(elems.len(), 1, "array should have one element (1+2)");
        assert!(matches!(&elems[0], Expr::BinOp { op: BinOp::Add, .. }));
    }

    // ── New grammar additions ─────────────────────────────────────────────────

    /// `for i in 0..=10 { ... }` — inclusive range
    ///
    /// Axon snippet:
    /// ```axon
    /// fn sum_to(n: i64) -> i64 {
    ///     let acc = 0
    ///     for i in 0..=n { acc = acc + i }
    ///     acc
    /// }
    /// ```
    #[test]
    fn parse_inclusive_range_for() {
        let src = "fn f() { for i in 0..=10 { let x = i } }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::For { inclusive, .. } = &stmts[0].expr else {
            panic!("expected For, got {:?}", stmts[0].expr)
        };
        assert!(*inclusive, "..= should set inclusive=true");
    }

    /// `for i in 0..10 { ... }` — exclusive range (existing, still works)
    #[test]
    fn parse_exclusive_range_for() {
        let src = "fn f() { for i in 0..10 { let x = i } }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::For { inclusive, .. } = &stmts[0].expr else { panic!() };
        assert!(!inclusive, ".. should set inclusive=false");
    }

    /// Boolean patterns in match arms.
    ///
    /// Axon snippet:
    /// ```axon
    /// fn describe(b: bool) -> str {
    ///     match b {
    ///         true  => "yes",
    ///         false => "no",
    ///     }
    /// }
    /// ```
    #[test]
    fn parse_bool_pattern_in_match() {
        let src = r#"fn f(b: bool) -> str { match b { true => "yes", false => "no" } }"#;
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Match { arms, .. } = &stmts[0].expr else { panic!() };
        assert_eq!(arms.len(), 2);
        assert!(
            matches!(&arms[0].pattern, Pattern::Literal(Literal::Bool(true))),
            "first arm should be true pattern"
        );
        assert!(
            matches!(&arms[1].pattern, Pattern::Literal(Literal::Bool(false))),
            "second arm should be false pattern"
        );
    }

    /// Negative integer literal patterns in match arms.
    ///
    /// Axon snippet:
    /// ```axon
    /// fn sign(n: i64) -> i64 {
    ///     match n {
    ///         -1 => -1,
    ///         0  =>  0,
    ///         1  =>  1,
    ///         _  =>  2,
    ///     }
    /// }
    /// ```
    #[test]
    fn parse_negative_int_pattern_in_match() {
        let src = "fn f(n: i64) -> i64 { match n { -1 => 0, 0 => 1, _ => 2 } }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Match { arms, .. } = &stmts[0].expr else { panic!() };
        assert_eq!(arms.len(), 3);
        assert!(
            matches!(&arms[0].pattern, Pattern::Literal(Literal::Int(-1))),
            "first arm should be -1 pattern"
        );
        assert!(
            matches!(&arms[1].pattern, Pattern::Literal(Literal::Int(0))),
            "second arm should be 0 pattern"
        );
        assert!(matches!(&arms[2].pattern, Pattern::Wildcard));
    }

    // ── Tuple patterns ────────────────────────────────────────────────────────

    /// Tuple pattern `(a, b)` in a match arm.
    ///
    /// Axon snippet:
    /// ```axon
    /// fn swap(pair: (i64, i64)) -> (i64, i64) {
    ///     match pair {
    ///         (x, y) => (y, x),
    ///     }
    /// }
    /// ```
    #[test]
    fn parse_tuple_pattern_two_elems() {
        let src = "fn f(p: (i64, i64)) { match p { (a, b) => a } }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Match { arms, .. } = &stmts[0].expr else { panic!() };
        assert_eq!(arms.len(), 1);
        let Pattern::Tuple(pats) = &arms[0].pattern else {
            panic!("expected Tuple pattern, got {:?}", arms[0].pattern)
        };
        assert_eq!(pats.len(), 2);
        assert!(matches!(&pats[0], Pattern::Ident(n) if n == "a"));
        assert!(matches!(&pats[1], Pattern::Ident(n) if n == "b"));
    }

    /// Nested tuple pattern `(a, (b, c))` — verifies recursion.
    #[test]
    fn parse_tuple_pattern_nested() {
        let src = "fn f() { match p { (a, (b, c)) => a } }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Match { arms, .. } = &stmts[0].expr else { panic!() };
        let Pattern::Tuple(pats) = &arms[0].pattern else { panic!("expected outer Tuple") };
        assert_eq!(pats.len(), 2);
        // Second element should be an inner tuple (b, c).
        assert!(matches!(&pats[1], Pattern::Tuple(inner) if inner.len() == 2));
    }

    /// Parenthesised pattern (single element, no comma) is NOT a tuple.
    #[test]
    fn parse_paren_pattern_not_tuple() {
        let src = "fn f() { match x { (v) => v } }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Expr::Block(stmts) = &f.body else { panic!() };
        let Expr::Match { arms, .. } = &stmts[0].expr else { panic!() };
        // A single-element paren group should lower to the inner pattern, not a Tuple.
        assert!(
            matches!(&arms[0].pattern, Pattern::Ident(_)),
            "single-element paren group should not be a Tuple, got {:?}", arms[0].pattern
        );
    }

    // ── Tuple type syntax ─────────────────────────────────────────────────────

    /// `(i64, bool)` as a return-type annotation produces `AxonType::Tuple`.
    ///
    /// Axon snippet:
    /// ```axon
    /// fn divide(a: i64, b: i64) -> (i64, i64) {
    ///     (a / b, a % b)
    /// }
    /// ```
    #[test]
    fn parse_tuple_type_return() {
        let src = "fn f(a: i64, b: i64) -> (i64, bool) { a }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Some(ret) = &f.return_type else { panic!("no return type") };
        let AxonType::Tuple(elems) = ret else {
            panic!("expected Tuple return type, got {ret:?}")
        };
        assert_eq!(elems.len(), 2);
        assert!(matches!(&elems[0], AxonType::Named(n) if n == "i64"));
        assert!(matches!(&elems[1], AxonType::Named(n) if n == "bool"));
    }

    /// `(i64, i64, str)` — three-element tuple type in a param annotation.
    #[test]
    fn parse_tuple_type_three_elems() {
        let src = "fn f(t: (i64, i64, str)) { t }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let AxonType::Tuple(elems) = &f.params[0].ty else {
            panic!("expected Tuple param type, got {:?}", f.params[0].ty)
        };
        assert_eq!(elems.len(), 3);
    }

    /// `()` unit type is still parsed as `AxonType::Named("()")`, not a `Tuple`.
    #[test]
    fn parse_unit_type_not_tuple() {
        let src = "fn f() -> () { 0 }";
        let prog = parse(src);
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        let Some(ret) = &f.return_type else { panic!("no return type") };
        assert!(
            matches!(ret, AxonType::Named(n) if n == "()"),
            "unit type should be Named(\"()\"), got {ret:?}"
        );
    }

    // ── Attribute annotation tests ────────────────────────────────────────────

    /// `@[test]` produces one attr with name "test" and no args.
    #[test]
    fn parse_attr_at_test() {
        let prog = parse("@[test] fn test_ok() { 1 }");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert_eq!(f.attrs.len(), 1, "expected one attribute");
        assert_eq!(f.attrs[0].name, "test");
        assert!(f.attrs[0].args.is_empty(), "test attr has no args");
    }

    /// `#[test]` (Rust-style hash sigil) produces the same attr as `@[test]`.
    #[test]
    fn parse_attr_hash_test() {
        let prog = parse("#[test] fn test_hash() { 1 }");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert_eq!(f.attrs.len(), 1, "expected one attribute");
        assert_eq!(f.attrs[0].name, "test");
        assert!(f.attrs[0].args.is_empty(), "test attr has no args");
    }

    /// `#[adaptive]` parses correctly with no args.
    #[test]
    fn parse_attr_hash_adaptive() {
        let prog = parse("#[adaptive] fn smart_fn(x: i64) -> i64 { x }");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert_eq!(f.attrs.len(), 1);
        assert_eq!(f.attrs[0].name, "adaptive");
        assert!(f.attrs[0].args.is_empty());
    }

    /// Multiple attributes on the same function — mix of `@[...]` and `#[...]`.
    #[test]
    fn parse_multiple_attrs_mixed_sigils() {
        let prog = parse("@[goal] #[adaptive] fn mixed(x: i64) -> i64 { x }");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert_eq!(f.attrs.len(), 2, "expected two attributes");
        assert_eq!(f.attrs[0].name, "goal");
        assert_eq!(f.attrs[1].name, "adaptive");
    }

    /// `#[goal(maximize_throughput)]` — attr with a single argument.
    #[test]
    fn parse_attr_hash_with_arg() {
        let prog = parse("#[goal(maximize_throughput)] fn optimize(x: i64) -> i64 { x }");
        let Item::FnDef(f) = &prog.items[0] else { panic!() };
        assert_eq!(f.attrs.len(), 1);
        assert_eq!(f.attrs[0].name, "goal");
        assert_eq!(f.attrs[0].args, vec!["maximize_throughput"]);
    }

    // ── Union types (TypeScript-style `A|B|C`) ────────────────────────────────

    /// Simple two-arm union as a parameter type.
    #[test]
    fn parse_union_param_two_members() {
        let prog = parse("fn foo(x: i64|str) -> i64 { x }");
        let Item::FnDef(f) = &prog.items[0] else { panic!("expected FnDef") };
        let AxonType::Union(members) = &f.params[0].ty else {
            panic!("expected Union, got {:?}", f.params[0].ty);
        };
        assert_eq!(members.len(), 2);
        assert!(matches!(&members[0], AxonType::Named(n) if n == "i64"));
        assert!(matches!(&members[1], AxonType::Named(n) if n == "str"));
    }

    /// Three-arm union as a parameter type.
    #[test]
    fn parse_union_param_three_members() {
        let prog = parse("fn foo(x: i64|str|bool) -> i64 { x }");
        let Item::FnDef(f) = &prog.items[0] else { panic!("expected FnDef") };
        let AxonType::Union(members) = &f.params[0].ty else {
            panic!("expected Union, got {:?}", f.params[0].ty);
        };
        assert_eq!(members.len(), 3);
        assert!(matches!(&members[2], AxonType::Named(n) if n == "bool"));
    }

    /// Union nested as the `ok` branch of a `Result<...>` return type.
    #[test]
    fn parse_union_in_result_ok_branch() {
        let prog = parse("fn foo(x: i64) -> Result<i64|str, Error> { Ok(x) }");
        let Item::FnDef(f) = &prog.items[0] else { panic!("expected FnDef") };
        let Some(AxonType::Result { ok, err }) = &f.return_type else {
            panic!("expected Result return type");
        };
        let AxonType::Union(members) = ok.as_ref() else {
            panic!("expected Union in ok branch, got {:?}", ok);
        };
        assert_eq!(members.len(), 2);
        assert!(matches!(&members[0], AxonType::Named(n) if n == "i64"));
        assert!(matches!(&members[1], AxonType::Named(n) if n == "str"));
        assert!(matches!(err.as_ref(), AxonType::Named(n) if n == "Error"));
    }

    /// A bare type with no `|` should NOT be wrapped in a Union.
    #[test]
    fn parse_single_type_is_not_a_union() {
        let prog = parse("fn foo(x: i64) -> i64 { x }");
        let Item::FnDef(f) = &prog.items[0] else { panic!("expected FnDef") };
        assert!(matches!(&f.params[0].ty, AxonType::Named(n) if n == "i64"));
        assert!(matches!(f.return_type.as_ref(), Some(AxonType::Named(n)) if n == "i64"));
    }

    /// Round-trip through the canonical formatter: `i64|str|bool`.
    #[test]
    fn parse_union_round_trip_via_fmt() {
        let src = "fn foo(x: i64|str|bool) -> i64 { x }";
        let prog = parse(src);
        let formatted = crate::fmt::format_program(&prog);
        assert!(
            formatted.contains("i64|str|bool"),
            "expected canonical form to contain `i64|str|bool`, got:\n{formatted}"
        );
        // Re-parse the formatted source and verify the Union shape survives.
        let prog2 = parse(&formatted);
        let Item::FnDef(f2) = &prog2.items[0] else { panic!("re-parse failed") };
        let AxonType::Union(members) = &f2.params[0].ty else {
            panic!("expected Union after round-trip, got {:?}", f2.params[0].ty);
        };
        assert_eq!(members.len(), 3);
    }
}
