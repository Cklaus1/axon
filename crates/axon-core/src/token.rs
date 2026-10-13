use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")] // skip non-newline whitespace; \n is emitted as Newline
#[logos(skip r"//[^\n]*")] // skip line comments
                           // Block comments are NOT skipped — they are emitted as BlockComment tokens so
                           // the lexer can detect newlines inside them and propagate ASI boundaries.
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
            Token::Str(s) => write!(f, "\"{s}\""),
            Token::BlockComment => write!(f, "/*...*/"),
            Token::Asm => write!(f, "asm"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// The body of a string literal, after its opening quote: everything up to the
/// closing `"`, where `\` takes the next character along with it (so `\"` does
/// not close the string) and that character may not be a newline. This is the
/// language the regex `"([^"\\]|\\.)*"` accepted; the body is scanned by a loop
/// because the state machine logos generated for it recursed once per
/// character (compilebench AX-65).
///
/// On failure (EOF before the closing quote, or a `\` with nothing or a
/// newline after it) the token is an error over the text the regex had
/// consumed: up to EOF, or up to the offending `\`.
fn lex_str(lex: &mut logos::Lexer<Token>) -> Result<String, ()> {
    let rem = lex.remainder();
    let bytes = rem.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                lex.bump(i + 1);
                let raw = lex.slice();
                return Ok(unescape(&raw[1..raw.len() - 1]));
            }
            b'\\' => match rem[i + 1..].chars().next() {
                Some(c) if c != '\n' => i += 1 + c.len_utf8(),
                _ => break,
            },
            // Every byte of a multi-byte character is >= 0x80, so none of
            // them is `"` or `\`: stepping bytewise stays on character
            // boundaries wherever the loop stops.
            _ => i += 1,
        }
    }
    lex.bump(i);
    Err(())
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
