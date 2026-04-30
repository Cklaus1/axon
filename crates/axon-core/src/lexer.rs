use logos::Logos;
use crate::token::Token;

pub struct Lexer<'src> {
    _marker: std::marker::PhantomData<&'src ()>,
}

impl<'src> Lexer<'src> {
    pub fn new(_src: &'src str) -> Self {
        Self { _marker: std::marker::PhantomData }
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
    UnexpectedChar { span: std::ops::Range<usize>, src: String },
    #[error("integer literal too large for i64: '{src}' at {span:?}")]
    IntegerTooLarge { span: std::ops::Range<usize>, src: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::tokenize(src).unwrap().into_iter().map(|(t, _)| t).collect()
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
        assert!(*bar_nl, "bar must be marked as preceded by a newline (newline inside comment)");
    }

    #[test]
    fn test_single_line_block_comment_no_pending() {
        // A block comment WITHOUT a newline should NOT set pending_newline.
        let result = Lexer::tokenize_with_newlines("foo /* no newline here */ bar").unwrap();
        let (_, _, bar_nl) = &result[1];
        assert!(!bar_nl, "bar must NOT be marked as newline-preceded for inline block comment");
    }

    #[test]
    fn test_percent_operator() {
        let tokens = lex("a%b");
        assert_eq!(tokens, vec![Token::Ident("a".into()), Token::Percent, Token::Ident("b".into())]);
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
        assert!(matches!(result.unwrap_err(), LexError::IntegerTooLarge { .. }));
    }

    #[test]
    fn test_unexpected_char_error() {
        // '@' is a valid token (At), so use a truly invalid char
        let result2 = Lexer::tokenize("let x = \x07");
        assert!(result2.is_err());
    }
}
