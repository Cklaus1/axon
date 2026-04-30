use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")] // skip non-newline whitespace; \n is emitted as Newline
#[logos(skip r"//[^\n]*")]   // skip line comments
// Block comments are NOT skipped — they are emitted as BlockComment tokens so
// the lexer can detect newlines inside them and propagate ASI boundaries.
pub enum Token {
    // Keywords
    #[token("fn")]    Fn,
    #[token("let")]   Let,
    #[token("own")]   Own,
    #[token("ref")]   Ref,
    #[token("type")]  Type,
    #[token("enum")]  Enum,
    #[token("mod")]   Mod,
    #[token("use")]   Use,
    #[token("match")] Match,
    #[token("spawn")] Spawn,
    #[token("select")]Select,
    #[token("comptime")] Comptime,
    #[token("if")]    If,
    #[token("else")]  Else,
    #[token("while")] While,
    #[token("break")]  Break,
    #[token("continue")] Continue,
    #[token("return")]Return,
    #[token("Ok")]    Ok,
    #[token("Err")]   Err,
    #[token("Some")]  Some,
    #[token("None")]  None,
    #[token("true")]  True,
    #[token("false")] False,
    #[token("pub")]   Pub,
    // Phase 3 keywords
    #[token("trait")]  Trait,
    #[token("impl")]   Impl,
    #[token("dyn")]    Dyn,
    #[token("for")]    For,
    #[token("in")]     In,
    #[token("self")]   SelfKw,
    #[token("where")]  Where,
    #[token("chan")]    Chan,

    // Operators
    #[token("->")] Arrow,
    #[token("=>")] FatArrow,
    #[token("&&")] And,
    #[token("||")] Or,
    #[token("?")]  Question,
    #[token("&")]  Ampersand,
    #[token("==")]  EqEq,
    #[token("!=")]  NotEq,
    #[token("<=")]  LtEq,
    #[token(">=")]  GtEq,
    #[token("=")]   Eq,
    #[token("<")]   Lt,
    #[token(">")]   Gt,
    #[token("+")]   Plus,
    #[token("-")]   Minus,
    #[token("*")]   Star,
    #[token("/")]   Slash,
    #[token("%")]   Percent,
    #[token("!")]   Bang,
    #[token("|")]   Pipe,
    #[token("..=")] DotDotEq,
    #[token("..")]  DotDot,
    #[token(".")]   Dot,
    #[token("@")]   At,
    #[token("#")]   Hash,
    #[token("::")] ColonColon,

    // Delimiters
    #[token("{")] LBrace,
    #[token("}")] RBrace,
    #[token("(")] LParen,
    #[token(")")] RParen,
    #[token("[")] LBracket,
    #[token("]")] RBracket,
    #[token(",")] Comma,
    #[token(":")] Colon,
    #[token(";")] Semi,

    // Literals — float must be tried before int so `1.0` doesn't lex as Int(1) + Dot + Int(0).
    // Matches: 1.0  1.5e-3  1e10  3.14E+2   (no trailing-dot or leading-dot forms)
    // Requiring ≥1 digit after `.` prevents `0.` from stealing the first dot of `0..n`.
    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| ()))]
    Int(i64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let raw = lex.slice();
        let s = &raw[1..raw.len() - 1]; // strip surrounding quotes
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n')  => result.push('\n'),
                    Some('t')  => result.push('\t'),
                    Some('r')  => result.push('\r'),
                    Some('\\') => result.push('\\'),
                    Some('"')  => result.push('"'),
                    Some('0')  => result.push('\0'),
                    Some(c)    => { result.push('\\'); result.push(c); }
                    None       => {}
                }
            } else {
                result.push(c);
            }
        }
        std::option::Option::Some(result)
    })]
    Str(String),

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
    #[regex(r"/\*([^*]|\*[^/])*\*/")]
    BlockComment,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Ident(s)     => write!(f, "{s}"),
            Token::Int(n)       => write!(f, "{n}"),
            Token::Float(n)     => write!(f, "{n}"),
            Token::Str(s)       => write!(f, "\"{s}\""),
            Token::BlockComment => write!(f, "/*...*/"),
            other               => write!(f, "{other:?}"),
        }
    }
}
