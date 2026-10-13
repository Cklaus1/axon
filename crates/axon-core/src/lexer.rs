use crate::token::Token;
use logos::Logos;

pub struct Lexer<'src> {
    _marker: std::marker::PhantomData<&'src ()>,
}

impl<'src> Lexer<'src> {
    pub fn new(_src: &'src str) -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Tokenize `src`, stripping `Newline` tokens and recording where each token
    /// was preceded by at least one newline.  The third element of each tuple is
    /// `true` when one or more `\n` characters appeared between the previous token
    /// and this one (or at the start of the file).
    pub fn tokenize_with_newlines(
        src: &str,
    ) -> Result<Vec<(Token, std::ops::Range<usize>, bool)>, LexError> {
        let mut out = Vec::new();
        let mut lex = Token::lexer(src);
        let mut pending_newline = false;
        while let Some(result) = lex.next() {
            match result {
                Ok(Token::Newline) => {
                    pending_newline = true;
                }
                // Block comments are emitted so we can detect newlines inside them.
                // A multi-line block comment counts as a newline boundary.
                Ok(Token::BlockComment) => {
                    if lex.slice().contains('\n') {
                        pending_newline = true;
                    }
                }
                Ok(tok) => {
                    out.push((tok, lex.span(), pending_newline));
                    pending_newline = false;
                }
                Err(_) => {
                    let slice = lex.slice().to_string();
                    let span = lex.span();
                    if slice.chars().all(|c| c.is_ascii_digit()) {
                        return Err(LexError::IntegerTooLarge { span, src: slice });
                    }
                    return Err(LexError::UnexpectedChar { span, src: slice });
                }
            }
        }
        Ok(out)
    }

    /// Tokenize `src` without newline tracking (backward-compatible).
    pub fn tokenize(src: &str) -> Result<Vec<(Token, std::ops::Range<usize>)>, LexError> {
        Self::tokenize_with_newlines(src)
            .map(|v| v.into_iter().map(|(t, span, _)| (t, span)).collect())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LexError {
    #[error("unexpected character '{src}' at {span:?}")]
    UnexpectedChar {
        span: std::ops::Range<usize>,
        src: String,
    },
    #[error("integer literal too large for i64: '{src}' at {span:?}")]
    IntegerTooLarge {
        span: std::ops::Range<usize>,
        src: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::tokenize(src)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    #[test]
    fn test_fn_decl() {
        let tokens = lex("fn add(a:i32,b:i32)->i32{a+b}");
        assert!(tokens.contains(&Token::Fn));
        assert!(tokens.contains(&Token::Arrow));
        assert!(tokens.contains(&Token::Plus));
    }

    #[test]
    fn test_let_binding() {
        let tokens = lex("let x=42");
        assert_eq!(tokens[0], Token::Let);
        assert_eq!(tokens[2], Token::Eq);
        assert_eq!(tokens[3], Token::Int(42));
    }

    #[test]
    fn test_ownership() {
        let tokens = lex("own x=Vec.new()");
        assert_eq!(tokens[0], Token::Own);
        let tokens2 = lex("ref y=&x");
        assert_eq!(tokens2[0], Token::Ref);
    }

    #[test]
    fn test_question_operator() {
        let tokens = lex("parse(data)?");
        assert!(tokens.contains(&Token::Question));
    }

    #[test]
    fn test_string_literal() {
        let tokens = lex(r#"let s="hello""#);
        assert!(matches!(&tokens[3], Token::Str(s) if s == "hello"));
    }

    #[test]
    fn test_comment_skipped() {
        let tokens = lex("let x=1 // this is ignored\nlet y=2");
        assert_eq!(tokens.iter().filter(|t| **t == Token::Let).count(), 2);
    }

    #[test]
    fn test_block_comment_skipped() {
        let tokens = lex("let x=1 /* this is ignored */ let y=2");
        assert_eq!(tokens.iter().filter(|t| **t == Token::Let).count(), 2);
    }

    #[test]
    fn test_block_comment_newline_sets_pending() {
        // A block comment containing a newline must set pending_newline on the
        // next real token — this is what makes ASI work correctly across comments.
        let result = Lexer::tokenize_with_newlines("foo /* multi\nline */ bar").unwrap();
        // result: [Ident("foo"), Ident("bar")]
        assert_eq!(result.len(), 2);
        let (_, _, foo_nl) = &result[0];
        let (_, _, bar_nl) = &result[1];
        assert!(!foo_nl, "foo should not be preceded by a newline");
        assert!(
            *bar_nl,
            "bar must be marked as preceded by a newline (newline inside comment)"
        );
    }

    #[test]
    fn test_single_line_block_comment_no_pending() {
        // A block comment WITHOUT a newline should NOT set pending_newline.
        let result = Lexer::tokenize_with_newlines("foo /* no newline here */ bar").unwrap();
        let (_, _, bar_nl) = &result[1];
        assert!(
            !bar_nl,
            "bar must NOT be marked as newline-preceded for inline block comment"
        );
    }

    #[test]
    fn test_percent_operator() {
        let tokens = lex("a%b");
        assert_eq!(
            tokens,
            vec![
                Token::Ident("a".into()),
                Token::Percent,
                Token::Ident("b".into())
            ]
        );
    }

    #[test]
    fn test_logical_and_or() {
        let tokens = lex("x&&y||z");
        assert!(tokens.contains(&Token::And));
        assert!(tokens.contains(&Token::Or));
    }

    #[test]
    fn test_float_scientific_notation() {
        let tokens = lex("1.5e2");
        assert!(matches!(&tokens[0], Token::Float(f) if (*f - 150.0).abs() < 0.001));

        let tokens2 = lex("3.14E-2");
        assert!(matches!(&tokens2[0], Token::Float(f) if (*f - 0.0314).abs() < 0.0001));

        let tokens3 = lex("1e10");
        assert!(matches!(&tokens3[0], Token::Float(f) if (*f - 1e10_f64).abs() < 1.0));
    }

    #[test]
    fn test_escape_sequences_in_strings() {
        let tokens = lex(r#""hello\nworld""#);
        assert!(matches!(&tokens[0], Token::Str(s) if s.contains('\n')));

        let tokens2 = lex(r#""tab\there""#);
        assert!(matches!(&tokens2[0], Token::Str(s) if s.contains('\t')));

        let tokens3 = lex(r#""quote\"here""#);
        assert!(matches!(&tokens3[0], Token::Str(s) if s.contains('"')));

        let tokens4 = lex(r#""back\\slash""#);
        assert!(matches!(&tokens4[0], Token::Str(s) if s.contains('\\')));
    }

    #[test]
    fn test_integer_overflow_error() {
        let result = Lexer::tokenize("99999999999999999999999999");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LexError::IntegerTooLarge { .. }
        ));
    }

    #[test]
    fn test_unexpected_char_error() {
        // '@' is a valid token (At), so use a truly invalid char
        let result2 = Lexer::tokenize("let x = \x07");
        assert!(result2.is_err());
    }

    /// Tokens and spans of `src`, or the error's message.
    fn lexed(src: &str) -> Result<Vec<(Token, std::ops::Range<usize>)>, String> {
        Lexer::tokenize(src).map_err(|e| e.to_string())
    }

    /// Run `f` on a 96 KiB stack. The state machine logos generates for a
    /// string, a block comment or a numeric literal called itself once per
    /// character, so a long one overflowed the wasm32 debug build's stack
    /// (compilebench AX-65); on this stack it would overflow within a few
    /// thousand characters in any build.
    fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(96 * 1024)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap()
    }

    #[test]
    fn long_literals_do_not_recurse_per_character_ax65() {
        let n = 200_000;
        on_small_stack(move || {
            let long = "a".repeat(n);
            let esc = "\\n".repeat(n);
            let toks = lex(&format!("\"{long}\" \"{esc}\""));
            assert!(matches!(&toks[0], Token::Str(s) if s.len() == n));
            assert!(matches!(&toks[1], Token::Str(s) if s.len() == n && s.starts_with('\n')));

            // A block comment is skipped, and a multi-line one counts as a
            // newline for the token after it.
            let toks = Lexer::tokenize_with_newlines(&format!("1 /* {long} */ 2 /* {long}\n */ 3"))
                .unwrap();
            let seen: Vec<bool> = toks.iter().map(|t| t.2).collect();
            assert_eq!(seen, [false, false, true]);
            // `*x` pairs: the `*` takes its next character along, and the
            // comment still ends at the first `*/`.
            assert_eq!(lex(&format!("/* {} */ 7", "*x".repeat(n))), [Token::Int(7)]);

            // Numeric literals: a long digit run is an integer too large for
            // i64 (the error of an ordinary overflow), a long fraction is a
            // float or a decimal, and a long run of separators is an Int.
            let digits = "1".repeat(n);
            assert!(matches!(
                Lexer::tokenize(&digits),
                Err(LexError::IntegerTooLarge { span, .. }) if span == (0..n)
            ));
            assert!(matches!(lex(&format!("1.{digits}"))[..], [Token::Float(_)]));
            assert!(
                matches!(&lex(&format!("1.{digits}d"))[..], [Token::Decimal(d)] if d.len() == n + 2)
            );
            assert!(matches!(&lex(&format!("{digits}d"))[..], [Token::Decimal(d)] if d.len() == n));
            assert_eq!(lex(&format!("1{}", "_".repeat(n))), [Token::Int(1)]);
            assert!(lexed(&format!("0x{digits}")).is_err());
        });
    }

    /// The numeric forms, as the six regexes they replaced lexed them
    /// (checked against the old lexer over every string of up to seven
    /// characters of `0 1 7 9 . e E + - _ d x o b a F` and space).
    #[test]
    fn numeric_literal_forms() {
        let one = |src: &str| {
            let t = lexed(src).unwrap_or_else(|e| panic!("{src}: {e}"));
            assert_eq!(t.len(), 1, "{src}: {t:?}");
            assert_eq!(t[0].1, 0..src.len(), "{src}");
            t[0].0.clone()
        };
        assert_eq!(one("42"), Token::Int(42));
        assert_eq!(one("1_000_000"), Token::Int(1_000_000));
        assert_eq!(one("1_"), Token::Int(1));
        assert_eq!(one("0xFF_FF"), Token::Int(65535));
        assert_eq!(one("0XfF"), Token::Int(255));
        assert_eq!(one("0o755"), Token::Int(0o755));
        assert_eq!(one("0b1111_0000"), Token::Int(0b1111_0000));
        assert_eq!(one("0x7FFFFFFFFFFFFFFF"), Token::Int(i64::MAX));
        assert_eq!(one("9223372036854775807"), Token::Int(i64::MAX));
        assert_eq!(one("1.5"), Token::Float(1.5));
        assert_eq!(one("1.5e3"), Token::Float(1500.0));
        assert_eq!(one("1.5E-3"), Token::Float(0.0015));
        assert_eq!(one("12e+2"), Token::Float(1200.0));
        assert_eq!(one("1d"), Token::Decimal("1".into()));
        assert_eq!(one("1_000.5d"), Token::Decimal("1_000.5".into()));
        assert_eq!(one("1._d"), Token::Decimal("1._".into()));
        assert_eq!(one("0d"), Token::Decimal("0".into()));

        // The longest literal wins, and what follows it is lexed on its own.
        let toks = |src: &str| -> Vec<Token> { lex(src) };
        assert_eq!(toks("1..2"), [Token::Int(1), Token::DotDot, Token::Int(2)]);
        assert_eq!(
            toks("1.e5"),
            [Token::Int(1), Token::Dot, Token::Ident("e5".into())]
        );
        assert_eq!(
            toks("1e+"),
            [Token::Int(1), Token::Ident("e".into()), Token::Plus]
        );
        assert_eq!(
            toks("1.5e3d"),
            [Token::Float(1500.0), Token::Ident("d".into())]
        );
        assert_eq!(
            toks("1dx"),
            [Token::Decimal("1".into()), Token::Ident("x".into())]
        );
        assert_eq!(toks("1.5d."), [Token::Decimal("1.5".into()), Token::Dot]);
        assert_eq!(toks("0xG"), [Token::Int(0), Token::Ident("xG".into())]);
        assert_eq!(toks("0o8"), [Token::Int(0), Token::Ident("o8".into())]);
        assert_eq!(toks("0b102"), [Token::Int(2), Token::Int(2)]);

        // A value that does not fit is an error over the whole literal; so
        // is a fraction with an underscore and no `d` (nothing accepts there:
        // the generated machine did not fall back to the integer before the
        // `.`, and this keeps it so).
        assert!(matches!(
            Lexer::tokenize("0x8000000000000000"),
            Err(LexError::UnexpectedChar { span, .. }) if span == (0..18)
        ));
        assert!(matches!(
            Lexer::tokenize("9223372036854775808"),
            Err(LexError::IntegerTooLarge { span, .. }) if span == (0..19)
        ));
        assert!(matches!(
            Lexer::tokenize("1.5_ + 2"),
            Err(LexError::UnexpectedChar { span, src }) if span == (0..4) && src == "1.5_"
        ));
        assert!(matches!(
            Lexer::tokenize("1_.5"),
            Err(LexError::UnexpectedChar { span, .. }) if span == (0..4)
        ));
    }

    #[test]
    fn string_and_comment_edges() {
        // `\c` takes the next character along, so `\"` does not close the
        // string, and an unknown escape stays as written.
        assert_eq!(lex(r#""a\"b" "#), [Token::Str("a\"b".into())]);
        assert_eq!(lex(r#""\q\0é日""#), [Token::Str("\\q\0é日".into())]);
        // No closing quote, or a `\` before a newline or the end: an error
        // over the text the old regex had consumed.
        for (src, end) in [("\"abc", 4), ("\"", 1), ("\"a\\", 2), ("\"a\\\nb\"", 2)] {
            assert!(
                matches!(Lexer::tokenize(src), Err(LexError::UnexpectedChar { span, .. }) if span == (0..end)),
                "{src:?}: {:?}",
                lexed(src)
            );
        }
        // A raw newline inside a string is fine.
        assert_eq!(lex("\"a\nb\""), [Token::Str("a\nb".into())]);
        // Block comments: `/**/` closes; `/***/` and `/* **/` do not (a `*`
        // takes its next character along, as the old regex had it).
        assert!(lex("/**/ /* a */ /* a * b */").is_empty());
        assert!(Lexer::tokenize("/***/").is_err());
        assert!(Lexer::tokenize("/* **/").is_err());
        assert!(Lexer::tokenize("/* unterminated").is_err());
        assert!(Lexer::tokenize("/*").is_err());
        assert!(Lexer::tokenize("/*/").is_err());
        assert_eq!(
            lex("a /* é */ b"),
            [Token::Ident("a".into()), Token::Ident("b".into())]
        );
    }
}
