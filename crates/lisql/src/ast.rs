//! Source-level Lisp syntax, independent of SQL operators and Rust code
//! generation.

use crate::diagnostic::{Diagnostic, Span};

/// All top-level forms in one source file, in source order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ast {
    /// Top-level expressions in source order; an empty source has no nodes.
    pub nodes: Vec<Node>,
}

/// A reader prefix retained in the syntax tree without evaluating its operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    /// The `'` prefix, retaining one quoted expression.
    Quote,
    /// The backtick prefix, retaining a template expression.
    Quasiquote,
    /// The comma prefix, marking an expression for later quasiquote evaluation.
    Unquote,
    /// The `,@` prefix, marking a sequence for later splicing into a template.
    UnquoteSplicing,
}

/// An ordinary Lisp name, kept distinct from keywords and string literals.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(pub String);

impl Symbol {
    /// Borrow the stored name without allocating or changing its spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Symbol {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Symbol {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A Lisp keyword name without its leading colon.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keyword(pub String);

impl Keyword {
    /// Borrow the stored name without allocating or changing its spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Keyword {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Keyword {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl std::fmt::Display for Keyword {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A source expression and its half-open UTF-8 byte range.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Expression content independent of its source location.
    pub inner: NodeInner,
    /// Source byte range covering the complete expression and any reader
    /// prefix.
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
/// Source expression forms retained by the parser without evaluation.
pub enum NodeInner {
    /// Ordinary names, including operators such as `>` and `->`.
    Symbol(Symbol),
    /// A keyword whose stored name excludes the leading colon, such as `:age`.
    Keyword(Keyword),
    /// A signed decimal integer decoded by the lexer within the i64 range.
    Integer(i64),
    /// A finite floating-point value read from decimal or scientific notation.
    Float(f64),
    /// A boolean literal spelled `true` or `false`.
    Boolean(bool),
    /// A double-quoted string with escapes decoded and Unicode content
    /// preserved.
    String(String),
    /// Parenthesized expressions in source order, including the empty list
    /// `()`.
    List(Vec<Node>),
    /// A reader prefix and its complete operand, retained without evaluation.
    Quoted {
        /// Reader prefix applied to the following expression.
        kind: QuoteKind,
        /// The complete operand, including a nested list or another prefix.
        value: Box<Node>,
    },
}

impl Node {
    /// Borrow a symbol without discarding its type or cloning its name.
    pub fn symbol(&self) -> Result<&Symbol, Diagnostic> {
        if let NodeInner::Symbol(s) = &self.inner {
            Ok(s)
        } else {
            Err(self.error("expected symbol"))
        }
    }

    /// Borrow a keyword, reporting this node's source range on a type mismatch.
    pub fn keyword(&self) -> Result<&Keyword, Diagnostic> {
        if let NodeInner::Keyword(s) = &self.inner {
            Ok(s)
        } else {
            Err(self.error("expected keyword"))
        }
    }

    /// Borrow list elements or report a type mismatch at this node.
    pub(crate) fn list(&self) -> Result<&[Node], Diagnostic> {
        if let NodeInner::List(items) = &self.inner {
            Ok(items)
        } else {
            Err(self.error("expected list"))
        }
    }

    /// Attach an explanation to this node’s complete source range.
    pub(crate) fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.span.clone(), message)
    }
}
