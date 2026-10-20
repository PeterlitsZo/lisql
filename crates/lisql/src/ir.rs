//! Query values produced by the AST runner and consumed by Rust code
//! generation.

use crate::{ast::Symbol, diagnostic::Span};
use lisql_runtime::{Compare, Direction, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Known expression types used during checking and Rust code generation.
pub enum Type {
    /// A declared 32-bit signed integer parameter.
    I32,
    /// A declared 64-bit signed integer parameter or an integer literal.
    I64,
    /// A boolean parameter, literal, or SQL predicate.
    Bool,
    /// A string parameter or literal.
    String,
    /// Column types remain unknown until the query reaches a database schema.
    Unknown,
}

impl Type {
    /// Whether this type is permitted for integer pagination values.
    pub fn integer(self) -> bool {
        matches!(self, Self::I32 | Self::I64)
    }
}

#[derive(Debug)]
/// A query-local parameter declaration with its generated Rust name.
pub struct Param {
    /// Original Lisql spelling, used when resolving parameter references.
    pub name: Symbol,
    /// Validated Rust identifier after converting source hyphens to
    /// underscores.
    pub rust_name: String,
    /// Declared or inferred type; column expressions may remain unknown.
    pub ty: Type,
    /// Half-open byte range of the corresponding source form.
    pub span: Span,
}

#[derive(Debug)]
/// A composed query declaration awaiting Rust code generation.
pub struct Query {
    /// Validated Rust identifier after converting source hyphens to
    /// underscores.
    pub rust_name: String,
    /// Parameters in declaration order, indexed by expression references.
    pub params: Vec<Param>,
    /// Construction steps in validated pipeline order.
    pub ops: Vec<Op>,
    /// Half-open byte range of the corresponding source form.
    pub span: Span,
}

#[derive(Debug)]
/// A SQL expression with its inferred type and original source location.
pub struct Expr {
    /// Expression content used to generate SQL expression constructors.
    pub kind: ExprKind,
    /// Declared or inferred type; column expressions may remain unknown.
    pub ty: Type,
    /// Half-open byte range of the corresponding source form.
    pub span: Span,
}

#[derive(Debug)]
/// SQL expression values constructed by the AST runner.
pub enum ExprKind {
    /// An unqualified column name to quote for the target dialect.
    Column(String),
    /// Index into the owning query's parameter list; independent of SQL binding
    /// order.
    Param(usize),
    /// An owned constant that becomes a binding rather than inline SQL.
    Literal(Value),
    /// A comparison with left and right operands in source order.
    Compare(Compare, Box<Expr>, Box<Expr>),
    /// A conjunction of at least two predicates.
    And(Vec<Expr>),
    /// A disjunction of at least two predicates.
    Or(Vec<Expr>),
    /// Logical negation of one predicate.
    Not(Box<Expr>),
    /// A NULL test that does not introduce a NULL binding.
    IsNull(Box<Expr>),
    /// A non-NULL test on one expression.
    IsNotNull(Box<Expr>),
}

#[derive(Debug)]
/// One query construction step and its source range.
pub struct Op {
    /// Operator content used to generate a pipeline step.
    pub kind: OpKind,
    /// Half-open byte range of the corresponding source form.
    pub span: Span,
}

#[derive(Debug)]
/// Operators composed in the validated query pipeline order.
pub enum OpKind {
    /// The unqualified source table; the first pipeline operator.
    From(String),
    /// A filter predicate; repeated filters are combined with AND.
    Where(Expr),
    /// A nonempty projection in requested column order.
    Select(Vec<String>),
    /// Nonempty ordering keys with an explicit direction for each column.
    OrderBy(Vec<(String, Direction)>),
    /// An integer parameter or nonnegative constant limiting returned rows.
    Limit(Expr),
    /// An integer row offset, permitted only after a limit.
    Offset(Expr),
}
