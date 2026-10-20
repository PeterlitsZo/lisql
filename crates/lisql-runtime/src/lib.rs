//! SQL structure is built first; SQL text and ordered bindings are rendered
//! together.

mod dialect;

pub use dialect::Dialect;

/// A SQL template and its owned bindings; this value does not execute the
/// query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Dialect-specific SQL text with placeholders for all bound values.
    pub sql: String,

    /// Bind in this order, including repeated references to the same source
    /// parameter.
    pub params: Vec<Value>,
}

/// Driver-independent parameter values, preserving the declared integer widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A 32-bit signed integer binding.
    I32(i32),
    /// A 64-bit signed integer binding.
    I64(i64),
    /// A boolean binding; the database driver chooses its wire representation.
    Bool(bool),
    /// An owned string binding independent of caller input lifetimes.
    String(String),
}

/// Invalid operator composition, identifier, or runtime pagination value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError(pub String);

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BuildError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Binary SQL comparison operators rendered inside parentheses.
pub enum Compare {
    /// Equality, rendered as `=`.
    Eq,
    /// Inequality, rendered as `<>`.
    Ne,
    /// Strictly greater than, rendered as `>`.
    Gt,
    /// Greater than or equal to, rendered as `>=`.
    Ge,
    /// Strictly less than, rendered as `<`.
    Lt,
    /// Less than or equal to, rendered as `<=`.
    Le,
}

impl Compare {
    fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Le => "<=",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Explicit SQL ordering direction for one column.
pub enum Direction {
    /// Ascending order, rendered as `ASC`.
    Asc,
    /// Descending order, rendered as `DESC`.
    Desc,
}

/// A SQL expression, not an interpreter for Lisql's future evaluation language.
#[derive(Debug, Clone)]
pub struct Expr {
    kind: ExprKind,
}

#[derive(Debug, Clone)]
/// Owned expression structure traversed when rendering SQL and bindings.
enum ExprKind {
    /// A static column name validated and quoted during rendering.
    Column(&'static str),
    /// An owned value emitted as a placeholder and appended to bindings.
    Value(Value),
    /// A binary comparison rendered left operand first.
    Compare(Compare, Box<Expr>, Box<Expr>),
    /// A conjunction requiring at least two operands.
    And(Vec<Expr>),
    /// A disjunction requiring at least two operands.
    Or(Vec<Expr>),
    /// Logical negation of one expression.
    Not(Box<Expr>),
    /// A NULL test without a separate NULL binding.
    IsNull(Box<Expr>),
    /// A non-NULL test without a separate NULL binding.
    IsNotNull(Box<Expr>),
}

impl Expr {
    /// Generated identifiers are static literals; they are validated when the
    /// query is built.
    pub fn column(name: &'static str) -> Self {
        Self {
            kind: ExprKind::Column(name),
        }
    }

    /// Construct an expression whose value will become an ordered binding.
    pub fn value(value: Value) -> Self {
        Self {
            kind: ExprKind::Value(value),
        }
    }

    /// Construct a comparison, preserving left-to-right binding order.
    pub fn compare(op: Compare, left: Self, right: Self) -> Self {
        Self {
            kind: ExprKind::Compare(op, Box::new(left), Box::new(right)),
        }
    }

    /// Construct AND; build rejects fewer than two operands.
    pub fn and(items: Vec<Self>) -> Self {
        Self {
            kind: ExprKind::And(items),
        }
    }

    /// Construct OR; build rejects fewer than two operands.
    pub fn or(items: Vec<Self>) -> Self {
        Self {
            kind: ExprKind::Or(items),
        }
    }

    /// Construct SQL NOT around one expression.
    // Named constructor is the generated-code contract.
    #[allow(clippy::should_implement_trait)]
    pub fn not(item: Self) -> Self {
        Self {
            kind: ExprKind::Not(Box::new(item)),
        }
    }

    /// Construct an IS NULL test without binding a NULL value.
    pub fn is_null(item: Self) -> Self {
        Self {
            kind: ExprKind::IsNull(Box::new(item)),
        }
    }

    /// Construct an IS NOT NULL test without binding a NULL value.
    pub fn is_not_null(item: Self) -> Self {
        Self {
            kind: ExprKind::IsNotNull(Box::new(item)),
        }
    }

    fn render(&self, state: &mut Render) -> Result<String, BuildError> {
        Ok(match &self.kind {
            ExprKind::Column(name) => state.identifier(name)?,
            ExprKind::Value(value) => state.bind(value.clone()),
            ExprKind::Compare(op, left, right) => {
                format!(
                    "({} {} {})",
                    left.render(state)?,
                    op.sql(),
                    right.render(state)?
                )
            }
            ExprKind::And(items) | ExprKind::Or(items) => {
                if items.len() < 2 {
                    return Err(BuildError("AND/OR requires at least two operands".into()));
                }

                let sep = if matches!(self.kind, ExprKind::And(_)) {
                    " AND "
                } else {
                    " OR "
                };
                let parts = items
                    .iter()
                    .map(|e| e.render(state))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("({})", parts.join(sep))
            }
            ExprKind::Not(item) => format!("(NOT {})", item.render(state)?),
            ExprKind::IsNull(item) => format!("({} IS NULL)", item.render(state)?),
            ExprKind::IsNotNull(item) => format!("({} IS NOT NULL)", item.render(state)?),
        })
    }
}

/// One SQL construction step. Operators are combined before any SQL is
/// rendered.
#[derive(Debug, Clone)]
pub enum Op {
    /// Set the source table; must be the first operator.
    From(&'static str),
    /// Add a predicate; repeated WHERE operators combine with AND.
    Where(Expr),
    /// Set a nonempty projection in output column order.
    Select(Vec<&'static str>),
    /// Set nonempty ordering keys after the projection.
    OrderBy(Vec<(&'static str, Direction)>),
    /// Bind a nonnegative integer row limit.
    Limit(Value),
    /// Bind a nonnegative integer offset; requires a preceding limit.
    Offset(Value),
}

/// Owns the operator sequence; `then` does not render or execute anything.
#[derive(Debug, Clone, Default)]
pub struct Pipeline {
    ops: Vec<Op>,
}

impl Pipeline {
    /// Start an empty pipeline; required clauses are checked by build.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a construction step, deferring validation and rendering to build.
    pub fn then(mut self, op: Op) -> Self {
        self.ops.push(op);
        self
    }

    /// Validate the complete operator sequence, then render SQL and bindings
    /// together.
    ///
    /// Returns an error for invalid order, missing clauses, malformed
    /// identifiers,
    /// or negative/non-integer pagination values, including manually
    /// constructed pipelines.
    pub fn build(self, dialect: Dialect) -> Result<Query, BuildError> {
        let mut table = None;
        let mut predicates = Vec::new();
        let mut columns = None;
        let mut ordering = None;
        let mut limit = None;
        let mut offset = None;
        let mut stage = 0;

        // Runtime validation also protects callers that construct operators
        // without the compiler.
        for op in self.ops {
            let next = match &op {
                Op::From(_) => 1,
                Op::Where(_) => 2,
                Op::Select(_) => 3,
                Op::OrderBy(_) => 4,
                Op::Limit(_) => 5,
                Op::Offset(_) => 6,
            };
            if (stage == 0 && next != 1) || next < stage || (next == stage && next != 2) {
                return Err(BuildError(
                    "invalid operator order or duplicate operator".into(),
                ));
            }

            stage = next;
            match op {
                Op::From(t) => table = Some(t),
                Op::Where(p) => predicates.push(p),
                Op::Select(c) => columns = Some(c),
                Op::OrderBy(o) => ordering = Some(o),
                Op::Limit(v) => {
                    validate_page(&v, "limit")?;
                    limit = Some(v);
                }
                Op::Offset(v) => {
                    validate_page(&v, "offset")?;
                    offset = Some(v);
                }
            }
        }

        let table = table.ok_or_else(|| BuildError("from is required".into()))?;
        let columns = columns
            .filter(|c| !c.is_empty())
            .ok_or_else(|| BuildError("nonempty select is required".into()))?;
        if offset.is_some() && limit.is_none() {
            return Err(BuildError("offset requires limit".into()));
        }

        // Render in final SQL order so parameter collection and placeholder
        // numbering agree.
        let mut state = Render {
            dialect,
            params: Vec::new(),
        };
        let columns = columns
            .iter()
            .map(|c| state.identifier(c))
            .collect::<Result<Vec<_>, _>>()?;
        let mut sql = format!(
            "SELECT {} FROM {}",
            columns.join(", "),
            state.identifier(table)?
        );

        if !predicates.is_empty() {
            // Preserve a single predicate; combine repeated WHERE steps with
            // explicit AND.
            let predicate = if predicates.len() == 1 {
                predicates.remove(0)
            } else {
                Expr::and(predicates)
            };
            sql.push_str(" WHERE ");
            sql.push_str(&predicate.render(&mut state)?);
        }

        if let Some(ordering) = ordering {
            if ordering.is_empty() {
                return Err(BuildError("order-by requires at least one item".into()));
            }
            let items = ordering
                .iter()
                .map(|(name, dir)| {
                    Ok(format!(
                        "{} {}",
                        state.identifier(name)?,
                        if *dir == Direction::Asc {
                            "ASC"
                        } else {
                            "DESC"
                        }
                    ))
                })
                .collect::<Result<Vec<_>, BuildError>>()?;
            sql.push_str(" ORDER BY ");
            sql.push_str(&items.join(", "));
        }

        if let Some(value) = limit {
            sql.push_str(" LIMIT ");
            sql.push_str(&state.bind(value));
        }

        if let Some(value) = offset {
            sql.push_str(" OFFSET ");
            sql.push_str(&state.bind(value));
        }

        Ok(Query {
            sql,
            params: state.params,
        })
    }
}

/// Validate dynamic pagination values before adding any SQL bindings.
fn validate_page(value: &Value, name: &str) -> Result<(), BuildError> {
    match value {
        Value::I32(v) if *v >= 0 => Ok(()),
        Value::I64(v) if *v >= 0 => Ok(()),
        _ => Err(BuildError(format!("{name} must be a nonnegative integer"))),
    }
}

/// V1 accepts only unqualified ASCII SQL identifiers.
pub fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Per-query rendering state that keeps placeholders and bindings in sync.
struct Render {
    dialect: Dialect,
    params: Vec<Value>,
}

impl Render {
    fn identifier(&self, name: &str) -> Result<String, BuildError> {
        if !valid_identifier(name) {
            return Err(BuildError(format!("invalid SQL identifier: {name}")));
        }
        Ok(self.dialect.identifier(name))
    }

    fn bind(&mut self, value: Value) -> String {
        // Never deduplicate: every rendered occurrence owns a binding and a
        // placeholder.
        self.params.push(value);
        self.dialect.placeholder(self.params.len())
    }
}
