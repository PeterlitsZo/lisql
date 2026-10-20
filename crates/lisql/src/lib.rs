//! Compile Lisql source into a callable Rust module. No database connection is
//! needed.

mod ast;
mod codegen;
mod diagnostic;
pub mod ir;
mod lexer;
mod parser;
pub mod runner;
pub mod semantic;

pub use ast::{Ast, Keyword, Node, NodeInner, QuoteKind, Symbol};
pub use diagnostic::{Diagnostic, Span};
pub use lexer::{lex, Token, TokenInner};
pub use lisql_runtime::Dialect;
pub use parser::parse;

/// Compile source into a Rust module without executing queries or accessing a
/// database.
///
/// On failure, the diagnostic refers to a byte range in `source`.
pub fn compile(source: &str, dialect: Dialect) -> Result<String, Diagnostic> {
    let tokens = lexer::lex(source)?;
    let ast = parser::parse(tokens)?;
    let queries = runner::eval(&ast)?;
    codegen::generate(&queries, dialect)
}
