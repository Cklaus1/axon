use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")] // skip non-newline whitespace; \n is emitted as Newline
#[logos(skip r"//[^\n]*")]
// skip line comments
// Block comments are NOT skipped — they are emitted as BlockComment tokens so
// the lexer can detect newlines inside them and propagate ASI boundaries.
// `extras`: set by [`lex_str`] when the string it just read holds a string
// literal inside an interpolation slot; `Lexer` then emits [`Token::SlotStr`].
#[logos(extras = bool)]
pub enum Token {
    // Keywords
    #[token("fn")]
    Fn,
    #[token("let")]
    Let,
    #[token("own")]
    Own,
    #[token("ref")]
    Ref,
    #[token("type")]
    Type,
    #[token("enum")]
    Enum,
    #[token("mod")]
    Mod,
    #[token("use")]
    Use,
    #[token("match")]
    Match,
    #[token("spawn")]
    Spawn,
    #[token("select")]
    Select,
    #[token("comptime")]
    Comptime,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("return")]
    Return,
    #[token("Ok")]
    Ok,
    #[token("Err")]
    Err,
    #[token("Some")]
    Some,
    #[token("None")]
    None,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("pub")]
    Pub,
    // Phase 3 keywords
    #[token("trait")]
    Trait,
    #[token("impl")]
    Impl,
    #[token("dyn")]
    Dyn,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("self")]
    SelfKw,
    #[token("where")]
    Where,
    #[token("chan")]
    Chan,
    /// `expr as Type` — postfix numeric cast (parser sugar over `as_i64` /
    /// `as_f64` / etc., which are polymorphic on the source type).
    #[token("as")]
    As,
    /// R17 Slice 1: `asm("template" ::: "clobbers")` — inline assembly.
    /// Substrate-only; E0910 under the interpreter (no hardware).
    #[token("asm")]
    Asm,

    // Operators
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("?")]
    Question,
    #[token("&")]
    Ampersand,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("<<")]
    Shl,
    #[token(">>")]
    Shr,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("!")]
    Bang,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("~")]
    Tilde,
    #[token("..=")]
    DotDotEq,
    #[token("..")]
    DotDot,
    #[token(".")]
    Dot,
    #[token("@")]
    At,
    #[token("#")]
    Hash,
    #[token("::")]
    ColonColon,

    // Delimiters
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,

    /// The start of a numeric literal: a decimal (R21), float, hex, octal,
    /// binary or decimal-integer literal. [`lex_number`] scans the rest and
    /// replaces this token with the [`Token::Decimal`], [`Token::Float`] or
    /// [`Token::Int`] it finds, so `Number` itself never leaves the lexer.
    /// (Six regexes for these were one logos state machine that called itself
    /// once per digit, and a debug wasm32 build ran out of stack on a literal
    /// of about 10,000 digits: compilebench AX-65.)
    #[regex(r"[0-9]", lex_number)]
    Number,

    // The numeric literals [`lex_number`] produces:
    //   Decimal (R21)  a `d` suffix on an int-or-fixed-point form, `1.50d`,
    //                  `0.1d`, `100d` (`-0.25d`: the sign is a separate
    //                  `UnaryOp::Neg`). No scientific notation (exactness): a
    //                  decimal is an exact digit string. The captured String
    //                  keeps the literal EXACT, we never round-trip through
    //                  f64; the stored value strips the `d`.
    //   Float          `1.0`, `1.5e-3`, `1e10`, `3.14E+2` (no trailing-dot or
    //                  leading-dot forms). Requiring at least one digit after
    //                  `.` keeps `0.` from stealing the first dot of `0..n`.
    //   Int            hex `0x3F8`, `0xFF_FF_FF_FF` (R17 Slice 1: MMIO
    //                  addresses), octal `0o17`, `0o755` (file modes), binary
    //                  `0b1010`, `0b1111_0000`, and decimal with optional
    //                  underscore separators `1_000_000`.
    Decimal(String),
    Float(f64),
    Int(i64),

    /// String literal. The body is scanned by [`lex_str`], not by a regex:
    /// logos compiles `"([^"\\]|\\.)*"` into a state machine that calls itself
    /// once per character, and a debug wasm32 build runs out of stack on a
    /// string of about 11,000 characters (compilebench AX-65).
    #[token("\"", lex_str)]
    Str(String),

    /// A string literal whose interpolation slot holds a string literal
    /// (`"hi {name + "!"}"`, see [`scan_str_body`]). Never produced by logos
    /// directly: `Lexer` turns a [`Token::Str`] into this when the scan opened
    /// a nested string. Only an expression accepts it (like `Str`); every other
    /// consumer matches `Str` alone and so refuses it, because the same
    /// characters lex as a shorter plain string today (`"{(" => 1, "x"`).
    SlotStr(String),

    /// Raw string literal `r"…"`: no escape processing, no `{…}` interpolation.
    /// Lets demos embed literal Axon/Rust/JSON code, regex patterns, or shell
    /// snippets that contain `\`, `{`, `}` characters without `{{`/`}}` doubling
    /// or `\\` escaping. Closes ROADMAP §9.5 F16.
    ///
    /// Single-line only — a `"` inside the body terminates it; no doubled-quote
    /// escape. If the body needs a `"`, fall back to a normal string literal
    /// with `\"`. Anything else (backslash, brace, dollar) passes through
    /// untouched.
    #[regex(r#"r"[^"]*""#, |lex| {
        let raw = lex.slice();
        std::option::Option::Some(raw[2..raw.len() - 1].to_string())
    })]
    RawStr(String),

    // Identifiers (must come after keywords)
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    /// Newline character — used internally by the lexer to record ASI boundaries.
    /// The parser never sees this token; `Lexer::tokenize_with_newlines` strips it
    /// and records a `preceding_newline: bool` flag on the following token instead.
    #[token("\n")]
    Newline,

    /// Block comment `/* ... */`.  Emitted (not skipped) so the lexer can detect
    /// newlines inside the comment and propagate `pending_newline` correctly.
    /// The parser never sees this token.
    ///
    /// The body is scanned by [`lex_block_comment`] (see [`Token::Str`]).
    #[token("/*", lex_block_comment)]
    BlockComment,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Ident(s) => write!(f, "{s}"),
            Token::Int(n) => write!(f, "{n}"),
            Token::Float(n) => write!(f, "{n}"),
            Token::Decimal(s) => write!(f, "{s}d"),
            Token::Str(s) | Token::SlotStr(s) => write!(f, "\"{s}\""),
            Token::BlockComment => write!(f, "/*...*/"),
            Token::Asm => write!(f, "asm"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// The body of a string literal, after its opening quote, scanned by
/// [`scan_str_body`]. On failure the token is an error over the text up to
/// EOF or up to the offending `\` (see [`scan_plain_str`]).
fn lex_str(lex: &mut logos::Lexer<Token>) -> Result<String, ()> {
    match scan_str_body(lex.remainder()) {
        Ok(scan) => {
            lex.bump(scan.len);
            lex.extras = scan.nested;
            Ok(scan.value)
        }
        Err(at) => {
            lex.bump(at);
            Err(())
        }
    }
}

/// A string literal's body read by [`scan_str_body`].
pub(crate) struct StrScan {
    /// Bytes from the body's start through the closing quote.
    pub len: usize,
    /// The literal's value.
    pub value: String,
    /// The scan opened a string literal inside an interpolation slot.
    pub nested: bool,
}

/// Scan a string literal's body (`src` starts after the opening quote): the
/// one string scanner shared by the lexer, the slot matcher of
/// `parse_fmt_str_raw` and the source scanners.
///
/// Outside a slot, `\` takes the next character (not a newline) and `"` ends
/// the string; `{{` and `}}` are text, a single `{` opens a slot and a lone
/// `}` is text. Inside a slot `{`/`}` count depth, and a `"` STARTS a nested
/// string literal when the slot text before it ends with an operator or
/// opener (see [`opens_operand`]); otherwise it ends the string, as it always
/// did. `r"` there starts a nested raw string. A nested string is scanned by
/// the same rules and its text is kept verbatim in the value, so the slot's
/// re-lex reads it; the rest of the body is unescaped.
///
/// If that scan does not close (EOF, or a `\` before a newline), the body is
/// read by today's rule (the first unescaped `"` ends it,
/// [`scan_plain_str`]), so a string that lexed before and does not close
/// under the nested reading lexes as before. A string with no nested string
/// is byte-identical to that rule. `Err` carries the offset where the plain
/// scan stopped.
pub(crate) fn scan_str_body(src: &str) -> Result<StrScan, usize> {
    if let Some((len, verbatim)) = scan_slot_strings(src) {
        if !verbatim.is_empty() {
            let body = &src[..len - 1];
            let mut value = String::with_capacity(body.len());
            let mut at = 0;
            for r in verbatim {
                value.push_str(&unescape(&body[at..r.start]));
                value.push_str(&body[r.clone()]);
                at = r.end;
            }
            value.push_str(&unescape(&body[at..]));
            return Ok(StrScan {
                len,
                value,
                nested: true,
            });
        }
    }
    let len = scan_plain_str(src)?;
    Ok(StrScan {
        len,
        value: unescape(&src[..len - 1]),
        nested: false,
    })
}

/// The end (exclusive) of the string literal whose opening `"` is at
/// `source[quote]`, read as the lexer reads it: a raw string `r"…"` ends at
/// the next `"`, any other by [`scan_str_body`]'s rules (so a string literal
/// inside a slot is part of it). An unclosed literal runs to the end of
/// `source`. For the source scanners that skip string literals (W0007,
/// W0008, fmt's comment check).
pub fn string_literal_end(source: &str, quote: usize) -> usize {
    let bytes = source.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let raw = quote > 0 && bytes[quote - 1] == b'r' && (quote < 2 || !ident(bytes[quote - 2]));
    let body = &source[quote + 1..];
    let len = if raw {
        body.find('"').map(|n| n + 1)
    } else {
        str_literal_len(body)
    };
    len.map_or(source.len(), |n| quote + 1 + n)
}

/// The length of a string literal's body through its closing quote, by the
/// rules of [`scan_str_body`], without building its value; `None` when it does
/// not close.
pub(crate) fn str_literal_len(after_quote: &str) -> Option<usize> {
    match scan_slot_strings(after_quote) {
        Some((len, _)) => Some(len),
        None => scan_plain_str(after_quote).ok(),
    }
}

/// The end of an interpolation slot in a string's VALUE: `text` follows the
/// slot's `{`; the result is the offset of its closing `}`. The slot text is
/// code, so a string literal in it (by [`scan_str_body`]'s rules) or a raw
/// string is skipped whole, and a brace inside one does not count. If a
/// string in the slot does not close, or no close is found that way, the
/// brace-depth reading ([`slot_close_by_depth`]) decides, as it always did.
pub(crate) fn slot_close(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some(i),
            b'}' => depth -= 1,
            b'"' => match str_literal_len(&text[i + 1..]) {
                Some(n) => {
                    i += 1 + n;
                    continue;
                }
                None => break,
            },
            b'r' if bytes.get(i + 1) == Some(&b'"')
                && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) =>
            {
                match text[i + 2..].find('"') {
                    Some(n) => {
                        i += n + 3;
                        continue;
                    }
                    None => break,
                }
            }
            _ => {}
        }
        i += 1;
    }
    slot_close_by_depth(text)
}

/// The end of a slot by brace depth alone, blind to strings: the reading
/// before CS4-2, kept as [`slot_close`]'s fallback.
pub(crate) fn slot_close_by_depth(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in text.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// One open construct while [`scan_slot_strings`] reads a string.
#[derive(Clone, Copy)]
enum Frame {
    /// A string body; `raw` for `r"…"` (no escapes, no slots).
    Str { raw: bool },
    /// An interpolation slot: its brace depth, and the last non-blank byte
    /// read in it (`None` while the slot is still empty).
    Slot { depth: usize, last: Option<u8> },
}

/// Whether a `"` after `last` in a slot begins an operand, so starts a nested
/// string: the slot is non-empty and ends with an operator or opener. A slot
/// that STARTS with `"` keeps that quote as the string's end, which is how a
/// pattern like `"{" => 1, "}" => 2` lexes.
fn opens_operand(last: Option<u8>) -> bool {
    match last {
        None => false,
        Some(b) => b"([{,;+-*/%=<>!&|?:".contains(&b),
    }
}

/// The length (through the closing quote) of a string body read with nested
/// strings in slots, and the byte ranges of the outermost nested strings, or
/// `None` when it does not close. Iterative with an explicit stack: no
/// recursion per nesting level or character (compilebench AX-65).
fn scan_slot_strings(src: &str) -> Option<(usize, Vec<std::ops::Range<usize>>)> {
    let bytes = src.as_bytes();
    let mut stack = vec![Frame::Str { raw: false }];
    // String frames on the stack; 1 is the literal being read.
    let mut strs = 1usize;
    let mut verbatim = Vec::new();
    let mut nested_at = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let top = stack.len() - 1;
        // A string closes at the quote just read (its slots already popped).
        let mut close = false;
        match stack[top] {
            Frame::Str { raw: true } => {
                if b == b'"' {
                    close = true;
                }
                i += 1;
            }
            Frame::Str { raw: false } => match b {
                b'"' => {
                    close = true;
                    i += 1;
                }
                b'\\' => i += escape_len(src, i)?,
                b'{' if bytes.get(i + 1) == Some(&b'{') => i += 2,
                b'{' => {
                    stack.push(Frame::Slot {
                        depth: 1,
                        last: None,
                    });
                    i += 1;
                }
                _ => i += 1,
            },
            Frame::Slot { depth, last } => match b {
                b'"' if opens_operand(last) => {
                    strs += 1;
                    if strs == 2 {
                        nested_at = i;
                    }
                    stack.push(Frame::Str { raw: false });
                    i += 1;
                }
                b'r' if bytes.get(i + 1) == Some(&b'"') && opens_operand(last) => {
                    strs += 1;
                    if strs == 2 {
                        nested_at = i;
                    }
                    stack.push(Frame::Str { raw: true });
                    i += 2;
                }
                b'"' => {
                    // Not an operand: the quote ends the innermost string.
                    let slots = stack
                        .iter()
                        .rev()
                        .take_while(|f| matches!(f, Frame::Slot { .. }))
                        .count();
                    stack.truncate(stack.len() - slots);
                    close = true;
                    i += 1;
                }
                b'\\' => {
                    let n = escape_len(src, i)?;
                    stack[top] = Frame::Slot {
                        depth,
                        last: Some(bytes[i + 1]),
                    };
                    i += n;
                }
                b'{' => {
                    stack[top] = Frame::Slot {
                        depth: depth + 1,
                        last: Some(b'{'),
                    };
                    i += 1;
                }
                b'}' => {
                    if depth == 1 {
                        stack.pop();
                    } else {
                        stack[top] = Frame::Slot {
                            depth: depth - 1,
                            last: Some(b'}'),
                        };
                    }
                    i += 1;
                }
                b' ' | b'\t' | b'\r' | b'\n' => i += 1,
                _ => {
                    stack[top] = Frame::Slot {
                        depth,
                        last: Some(b),
                    };
                    i += 1;
                }
            },
        }
        if close {
            // `i` is just past the closing quote.
            stack.pop();
            strs -= 1;
            match stack.last_mut() {
                None => return Some((i, verbatim)),
                Some(Frame::Slot { last, .. }) => *last = Some(b'"'),
                Some(Frame::Str { .. }) => {}
            }
            if strs == 1 {
                verbatim.push(nested_at..i);
            }
        }
    }
    None
}

/// The bytes an escape at `src[i] == '\\'` takes: the `\` and the next
/// character, which may not be a newline (`None`, as in [`scan_plain_str`]).
fn escape_len(src: &str, i: usize) -> Option<usize> {
    match src[i + 1..].chars().next() {
        Some(c) if c != '\n' => Some(1 + c.len_utf8()),
        _ => None,
    }
}

/// A string body by the rule before CS4-2: everything up to the first `"`,
/// where `\` takes the next character along with it (so `\"` does not close
/// the string) and that character may not be a newline. This is the language
/// the regex `"([^"\\]|\\.)*"` accepted; the body is scanned by a loop
/// because the state machine logos generated for it recursed once per
/// character (compilebench AX-65). `Ok` is the length through the closing
/// quote; `Err` the offset the regex had consumed: EOF, or the offending `\`.
fn scan_plain_str(src: &str) -> Result<usize, usize> {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok(i + 1),
            b'\\' => match escape_len(src, i) {
                Some(n) => i += n,
                None => break,
            },
            // Every byte of a multi-byte character is >= 0x80, so none of
            // them is `"` or `\`: stepping bytewise stays on character
            // boundaries wherever the loop stops.
            _ => i += 1,
        }
    }
    Err(i)
}

/// The value of a string literal's body: `\n`, `\t`, `\r`, `\\`, `\"` and `\0`
/// are their characters; any other `\c` stays as written.
fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('0') => result.push('\0'),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => {}
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// The body of a `/* ... */` comment, after its opening: everything up to the
/// first `*/`, except that a `*` followed by any other character takes that
/// character along with it, so `**/` does not close (the language of the regex
/// `/\*([^*]|\*[^/])*\*/`, which logos compiled into a state machine that
/// recursed once per character, compilebench AX-65; the quirk is kept so that
/// no program lexes differently).
///
/// On failure (EOF before the close) the token is an error over the text the
/// regex had consumed: up to EOF, or up to a final lone `*`.
fn lex_block_comment(lex: &mut logos::Lexer<Token>) -> Result<(), ()> {
    let rem = lex.remainder();
    let bytes = rem.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'*' {
            // `*` and `/` are ASCII: a bytewise step stays on boundaries.
            i += 1;
            continue;
        }
        match rem[i + 1..].chars().next() {
            Some('/') => {
                lex.bump(i + 2);
                return Ok(());
            }
            Some(c) => i += 1 + c.len_utf8(),
            None => break,
        }
    }
    lex.bump(i);
    Err(())
}

/// The numeric literal starting at the digit `lex` just matched: the longest of
///
/// - decimal   `[0-9][0-9_]*(\.[0-9_]+)?d`
/// - float     `[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?` and `[0-9]+[eE][+-]?[0-9]+`
/// - hex       `0[xX][0-9a-fA-F][0-9a-fA-F_]*`
/// - octal     `0[oO][0-7][0-7_]*`
/// - binary    `0[bB][01][01_]*`
/// - integer   `[0-9][0-9_]*`
///
/// (no two can match the same length: a decimal ends in `d`, a float holds a
/// `.` or an exponent the integer's run stops at, and a prefixed one holds the
/// `x`, `o` or `b` it stops at). These were six logos regexes, the longest
/// match between them found by a state machine that recursed once per digit
/// (compilebench AX-65); this is a loop.
///
/// A literal whose value does not fit (an integer past `i64`, a radix number
/// past `i64`) is an error token over the whole literal, as before.
fn lex_number(lex: &mut logos::Lexer<Token>) -> Result<Token, ()> {
    let s = &lex.source().as_bytes()[lex.span().start..];
    // The length of the run of bytes `s[from..]` satisfying `f`.
    let run = |from: usize, f: fn(u8) -> bool| s[from..].iter().take_while(|&&c| f(c)).count();
    let is_digit = |c: u8| c.is_ascii_digit();
    let is_digit_us = |c: u8| c.is_ascii_digit() || c == b'_';
    // `[eE][+-]?[0-9]+` at `at`: its length, 0 if there is none.
    let exponent = |at: usize| -> usize {
        if !matches!(s.get(at), Some(b'e' | b'E')) {
            return 0;
        }
        let sign = usize::from(matches!(s.get(at + 1), Some(b'+' | b'-')));
        match run(at + 1 + sign, is_digit) {
            0 => 0,
            n => 1 + sign + n,
        }
    };

    let digits = run(0, is_digit);
    let int_len = run(0, is_digit_us);
    let (mut len, mut radix) = (int_len, 10);

    // A radix prefix: `0x`, `0o`, `0b` and a digit of that radix.
    let prefixed = match (s[0], s.get(1)) {
        (b'0', Some(b'x' | b'X')) => Some((16, (|c: u8| c.is_ascii_hexdigit()) as fn(u8) -> bool)),
        (b'0', Some(b'o' | b'O')) => {
            Some((8, (|c: u8| matches!(c, b'0'..=b'7')) as fn(u8) -> bool))
        }
        (b'0', Some(b'b' | b'B')) => {
            Some((2, (|c: u8| matches!(c, b'0' | b'1')) as fn(u8) -> bool))
        }
        _ => None,
    };
    if let Some((r, ok)) = prefixed {
        if s.get(2).is_some_and(|&c| ok(c)) {
            len = 2 + 1 + s[3..].iter().take_while(|&&c| ok(c) || c == b'_').count();
            radix = r;
        }
    }

    // A float: `digits.digits[exp]` or `digits exp`.
    let mut float = 0;
    if s.get(digits) == Some(&b'.') {
        let frac = run(digits + 1, is_digit);
        if frac > 0 {
            float = digits + 1 + frac;
            float += exponent(float);
        }
    } else {
        let e = exponent(digits);
        if e > 0 {
            float = digits + e;
        }
    }
    let is_float = float > len;
    if is_float {
        len = float;
    }

    // A decimal: the integer run, optionally `.[0-9_]+`, then `d`.
    let frac_end = if s.get(int_len) == Some(&b'.') {
        match run(int_len + 1, is_digit_us) {
            0 => None,
            f => Some(int_len + 1 + f),
        }
    } else {
        Some(int_len)
    };
    // The decimal's fraction has run out without a `d`. If it is no float
    // either (an underscore in it or before it), nothing accepts there, and
    // the token is an error over what was consumed: the generated machine
    // never fell back to the integer in front of the `.`, so `1._` and
    // `1.5_` have always been lexical errors. Kept, so no program changes.
    if let Some(e) = frac_end.filter(|&e| e > int_len) {
        let float_too = digits == int_len && run(int_len + 1, is_digit) == e - int_len - 1;
        if s.get(e) != Some(&b'd') && !float_too {
            lex.bump(e - 1);
            return Err(());
        }
    }
    let decimal = frac_end.filter(|&e| s.get(e) == Some(&b'd')).map(|e| e + 1);
    if let Some(d) = decimal.filter(|&d| d > len) {
        lex.bump(d - 1);
        let lit = lex.slice();
        return Ok(Token::Decimal(lit[..lit.len() - 1].to_string()));
    }

    lex.bump(len - 1);
    let lit = lex.slice();
    if is_float {
        return lit.parse::<f64>().map(Token::Float).map_err(|_| ());
    }
    let (digits, radix) = if radix == 10 {
        (lit, 10)
    } else {
        (&lit[2..], radix)
    };
    i64::from_str_radix(&digits.replace('_', ""), radix)
        .map(Token::Int)
        .map_err(|_| ())
}
