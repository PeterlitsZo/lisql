//! Read typed Lisp tokens while retaining byte spans for diagnostics.

use std::{iter::Peekable, str::CharIndices};

use crate::{
    ast::{Keyword, QuoteKind, Symbol},
    diagnostic::{Diagnostic, Span},
};

#[derive(Debug, Clone, PartialEq)]
/// The decoded content of one lexical token, before list construction.
pub enum TokenInner {
    /// An opening parenthesis that starts a list.
    LeftParen,
    /// A closing parenthesis that ends a list.
    RightParen,
    /// An ordinary identifier or operator name, such as `min-age` or `->`.
    Symbol(Symbol),
    /// The keyword name excludes its leading colon.
    Keyword(Keyword),
    /// A signed decimal integer checked against the i64 range.
    Integer(i64),
    /// A finite decimal or scientific-notation floating-point literal.
    Float(f64),
    /// A literal spelled `true` or `false`.
    Boolean(bool),
    /// String contents with quotes removed and escape sequences decoded.
    String(String),
    /// `'`, backtick, `,`, or `,@`; the parser attaches the following
    /// expression.
    Quote(QuoteKind),
}

#[derive(Debug, Clone, PartialEq)]
/// A lexical value paired with its original half-open UTF-8 byte range.
pub struct Token {
    /// Decoded token content; whitespace and comments produce no tokens.
    pub inner: TokenInner,
    /// Original spelling range, including delimiters for strings and keywords.
    pub span: Span,
}

/// Tokenize Lisql's Lisp reader syntax without applying SQL-specific
/// restrictions.
///
/// Numeric values and string escapes are decoded here. Symbols and keywords
/// retain
/// their names; SQL identifier and parameter-name validation belongs to a later
/// pass.
pub fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    Lexer::new(source).tokenize()
}

/// Keep the source and cursor together so token spans and diagnostics use the
/// same
/// UTF-8 byte offsets, even when the input contains multibyte characters.
struct Lexer<'a> {
    source: &'a str,
    chars: Peekable<CharIndices<'a>>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.char_indices().peekable(),
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, Diagnostic> {
        let mut tokens = Vec::new();

        while let Some((start, c)) = self.chars.next() {
            if c.is_whitespace() {
                continue;
            }
            if c == ';' {
                self.skip_comment();
                continue;
            }

            let inner = match c {
                '(' => TokenInner::LeftParen,
                ')' => TokenInner::RightParen,
                '\'' => TokenInner::Quote(QuoteKind::Quote),
                '`' => TokenInner::Quote(QuoteKind::Quasiquote),
                ',' => {
                    if self.chars.peek().is_some_and(|(_, c)| *c == '@') {
                        self.chars.next();
                        TokenInner::Quote(QuoteKind::UnquoteSplicing)
                    } else {
                        TokenInner::Quote(QuoteKind::Unquote)
                    }
                }
                '"' => TokenInner::String(self.read_string(start)?),
                ':' => self.read_keyword(start)?,
                '0'..='9' => self.read_number(start)?,
                '+' | '-' | '.' if self.starts_number(c) => self.read_number(start)?,
                '+' | '-' | '.' => self.read_identifier(start)?,
                c if identifier_initial(c) => self.read_identifier(start)?,
                _ => {
                    return Err(Diagnostic::new(
                        start..self.position(),
                        "unexpected character",
                    ))
                }
            };

            tokens.push(Token {
                inner,
                span: start..self.position(),
            });
        }

        Ok(tokens)
    }

    /// The next unread byte offset is also the exclusive end of the current
    /// token.
    fn position(&mut self) -> usize {
        self.chars.peek().map_or(self.source.len(), |(pos, _)| *pos)
    }

    fn skip_comment(&mut self) {
        // Consume the newline too; it has no significance to the reader.
        for (_, c) in self.chars.by_ref() {
            if c == '\n' {
                break;
            }
        }
    }

    /// Read after the opening quote, retaining its position for unclosed
    /// strings.
    fn read_string(&mut self, start: usize) -> Result<String, Diagnostic> {
        let mut value = String::new();

        while let Some((pos, c)) = self.chars.next() {
            match c {
                '"' => {
                    // Adjacent atoms and strings must not silently become
                    // separate values.
                    if let Some(&(pos, c)) = self.chars.peek() {
                        if !delimiter(c) {
                            return Err(Diagnostic::new(
                                pos..pos + c.len_utf8(),
                                "expected delimiter after string",
                            ));
                        }
                    }
                    return Ok(value);
                }
                '\\' => value.push(self.read_escape(pos)?),
                _ => value.push(c),
            }
        }

        Err(Diagnostic::new(
            start..self.source.len(),
            "unterminated string",
        ))
    }

    /// The backslash is already consumed; error spans include it and the
    /// escaped character.
    fn read_escape(&mut self, start: usize) -> Result<char, Diagnostic> {
        let (pos, escaped) = self.chars.next().ok_or_else(|| {
            Diagnostic::new(start..self.source.len(), "unterminated string escape")
        })?;

        match escaped {
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            _ => Err(Diagnostic::new(
                start..pos + escaped.len_utf8(),
                "unknown string escape",
            )),
        }
    }

    /// Signs and dots also begin peculiar identifiers, so inspect the numeric
    /// prefix.
    fn starts_number(&self, first: char) -> bool {
        let mut rest = self.chars.clone();
        if matches!(first, '+' | '-') && rest.peek().is_some_and(|(_, c)| *c == '.') {
            rest.next();
        }
        rest.peek().is_some_and(|(_, c)| c.is_ascii_digit())
    }

    /// Consume a complete spelling so `1n` is rejected as one malformed number,
    /// rather than silently becoming the two tokens `1` and `n`.
    fn read_spelling(&mut self, start: usize) -> Result<&'a str, Diagnostic> {
        while let Some(&(pos, c)) = self.chars.peek() {
            if delimiter(c) {
                break;
            }
            if c == '"' {
                return Err(Diagnostic::new(
                    start..pos + 1,
                    "expected delimiter before string",
                ));
            }
            self.chars.next();
        }
        let end = self.position();
        Ok(&self.source[start..end])
    }

    fn read_keyword(&mut self, start: usize) -> Result<TokenInner, Diagnostic> {
        let spelling = self.read_spelling(start)?;
        let name = &spelling[1..];
        if name.is_empty() {
            return Err(Diagnostic::new(
                start..self.position(),
                "keyword requires a name after ':'",
            ));
        }
        // Keyword names retain Lisql's existing syntax; SQL names are checked
        // later.
        Ok(TokenInner::Keyword(name.into()))
    }

    /// Validate an identifier spelling, then distinguish reserved boolean
    /// literals.
    fn read_identifier(&mut self, start: usize) -> Result<TokenInner, Diagnostic> {
        let value = self.read_spelling(start)?;
        if !valid_identifier(value) {
            return Err(Diagnostic::new(
                start..self.position(),
                "invalid identifier",
            ));
        }
        Ok(match value {
            "true" => TokenInner::Boolean(true),
            "false" => TokenInner::Boolean(false),
            _ => TokenInner::Symbol(value.into()),
        })
    }

    fn read_number(&mut self, start: usize) -> Result<TokenInner, Diagnostic> {
        let value = self.read_spelling(start)?;
        decode_number(value, start..self.position())
    }
}

// R7RS ordinary identifier characters, with Unicode alphabetic letters
// supported.
fn identifier_initial(c: char) -> bool {
    c.is_alphabetic()
        || matches!(
            c,
            '!' | '$' | '%' | '&' | '*' | '/' | ':' | '<' | '=' | '>' | '?' | '@' | '^' | '_' | '~'
        )
}

fn identifier_subsequent(c: char) -> bool {
    identifier_initial(c) || c.is_ascii_digit() || matches!(c, '+' | '-' | '.')
}

fn sign_subsequent(c: char) -> bool {
    identifier_initial(c) || matches!(c, '+' | '-')
}

fn dot_subsequent(c: char) -> bool {
    sign_subsequent(c) || c == '.'
}

/// R7RS peculiar identifiers allow `+`, `-`, `->`, and `...`, but not a lone
/// dot.
fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some('+' | '-') => match chars.next() {
            None => return true,
            Some('.') => {
                if !chars.next().is_some_and(dot_subsequent) {
                    return false;
                }
            }
            Some(c) if sign_subsequent(c) => (),
            _ => return false,
        },
        Some('.') => {
            if !chars.next().is_some_and(dot_subsequent) {
                return false;
            }
        }
        Some(c) if identifier_initial(c) => (),
        _ => return false,
    }
    chars.all(identifier_subsequent)
}

/// Decode a numeric spelling and report malformed or out-of-range literals.
fn decode_number(value: &str, span: Span) -> Result<TokenInner, Diagnostic> {
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    if unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return value
            .parse::<i64>()
            .map(TokenInner::Integer)
            .map_err(|_| Diagnostic::new(span, "integer literal exceeds i64 range"));
    }

    // Decimal points and scientific notation are reader syntax even though SQL
    // v1
    // does not yet expose floating-point bindings. Reject malformed
    // numeric-looking atoms.
    if valid_float(unsigned) {
        if let Ok(number) = value.parse::<f64>() {
            if number.is_finite() {
                return Ok(TokenInner::Float(number));
            }
        }
        return Err(Diagnostic::new(
            span,
            "floating-point literal exceeds finite f64 range",
        ));
    }

    Err(Diagnostic::new(span, "invalid numeric literal"))
}

/// Check decimal/exponent syntax after the reader has found a numeric prefix
/// and removed any leading sign. An exponent must contain at least one digit.
fn valid_float(unsigned: &str) -> bool {
    let bytes = unsigned.as_bytes();
    let mut index = 0;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }

    let mut has_fraction = false;
    if bytes.get(index) == Some(&b'.') {
        has_fraction = true;
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }

    let mut has_exponent = false;
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        has_exponent = true;
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }

    index == bytes.len() && (has_fraction || has_exponent)
}

// Double quotes are checked separately so adjacent strings and atoms produce an
// error.
fn delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | ';' | '\'' | '`' | ',')
}
