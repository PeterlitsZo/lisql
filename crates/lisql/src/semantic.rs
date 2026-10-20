//! Resolve names, check known types, and enforce the supported SQL operator
//! protocol.

use crate::{
    ast::{Ast, Node, NodeInner},
    diagnostic::Diagnostic,
    ir::Type,
};
use lisql_runtime::valid_identifier;
use std::collections::HashSet;

/// Check declarations, scope, types, and operator order without constructing
/// queries.
pub fn check(ast: &Ast) -> Result<(), Diagnostic> {
    if ast.nodes.is_empty() {
        return Err(Diagnostic::new(0..0, "expected at least one def-query"));
    }

    let mut names = HashSet::new();

    for node in &ast.nodes {
        let form = node.list()?;
        if form.len() != 4 || form[0].symbol()?.as_str() != "def-query" {
            return Err(node.error("expected (def-query name (parameters) (-> operators))"));
        }

        // Check generated names as well as source spellings: a-b and a_b
        // collide.
        let query_rust_name = rust_name(&form[1])?;
        if !names.insert(query_rust_name.clone()) {
            return Err(form[1].error("duplicate query name after Rust name conversion"));
        }

        let declarations = form[2].list()?;
        if declarations.len() % 2 != 0 {
            return Err(form[2].error("parameters must be name/type pairs"));
        }

        let mut params = Vec::new();
        let mut param_names = HashSet::new();

        for pair in declarations.chunks_exact(2) {
            let name = pair[0].symbol()?.to_owned();
            let rust_name = rust_name(&pair[0])?;
            if !param_names.insert(rust_name.clone()) {
                return Err(pair[0].error("duplicate parameter name after Rust name conversion"));
            }
            let ty = match pair[1].keyword()?.as_str() {
                "i32" => Type::I32,
                "i64" => Type::I64,
                "bool" => Type::Bool,
                "string" => Type::String,
                _ => {
                    return Err(pair[1].error(
                        "unsupported parameter type; expected :i32, :i64, :bool or :string",
                    ))
                }
            };
            params.push(Binding {
                name,
                ty,
                span: pair[0].span.clone(),
            });
        }

        // Resolve expressions against this query only; never capture another
        // query's parameters.
        let mut context = Context {
            params: &params,
            used: HashSet::new(),
        };
        let pipeline = form[3].list()?;
        if pipeline
            .first()
            .and_then(|n| n.symbol().ok().map(|symbol| symbol.as_str()))
            != Some("->")
        {
            return Err(form[3].error("expected -> operator composition"));
        }

        let mut stage = 0;
        let mut selected = false;
        let mut limited = false;

        for node in &pipeline[1..] {
            let items = node.list()?;
            let head = items
                .first()
                .ok_or_else(|| node.error("empty query operator"))?
                .symbol()?;

            // Ranks encode the v1 composition protocol. Only WHERE may repeat.
            let rank = match head.as_str() {
                "from" => 1,
                "where" => 2,
                "select" => 3,
                "order-by" => 4,
                "limit" => 5,
                "offset" => 6,
                _ => return Err(items[0].error(format!("unknown query operator: {head}"))),
            };
            if (stage == 0 && rank != 1) || rank < stage || (rank == stage && rank != 2) {
                return Err(node.error("invalid operator order or duplicate operator; expected from → where* → select → order-by? → limit? → offset?"));
            }

            stage = rank;
            if head.as_str() != "order-by" && items.len() != 2 {
                return Err(node.error(format!("{head} requires exactly one argument")));
            }

            match head.as_str() {
                "from" => {
                    column(&items[1])?;
                }
                "where" => {
                    let ty = context.expr(&items[1])?;
                    predicate(&items[1], ty)?;
                }
                "select" => {
                    let columns = items[1].list()?;
                    if columns.is_empty() {
                        return Err(items[1].error("select requires at least one column"));
                    }
                    selected = true;
                    for item in columns {
                        column(item)?;
                    }
                }
                "order-by" => {
                    if items.len() < 2 {
                        return Err(node.error("order-by requires at least one item"));
                    }
                    for item in &items[1..] {
                        let parts = item.list()?;
                        if parts.len() != 2 {
                            return Err(item.error("expected (:column :asc|:desc)"));
                        }
                        match parts[1].keyword()?.as_str() {
                            "asc" | "desc" => (),
                            _ => return Err(parts[1].error("expected :asc or :desc")),
                        }
                        column(&parts[0])?;
                    }
                }
                "limit" | "offset" => {
                    let value = context.expr(&items[1])?;
                    if !value.integer()
                        || !matches!(items[1].inner, NodeInner::Symbol(_) | NodeInner::Integer(_))
                    {
                        return Err(
                            items[1].error("pagination requires an integer parameter or constant")
                        );
                    }

                    // Constant failures are known now; dynamic values are
                    // checked by build().
                    if matches!(items[1].inner, NodeInner::Integer(v) if v < 0) {
                        return Err(items[1].error("pagination constant must be nonnegative"));
                    }
                    if head.as_str() == "limit" {
                        limited = true;
                    } else if !limited {
                        return Err(node.error("offset requires limit"));
                    }
                }
                _ => unreachable!(),
            };
        }

        if !selected {
            return Err(form[3].error("query requires from and a nonempty select"));
        }

        // Report unused declarations only after all operator expressions have
        // been visited.
        for (index, param) in params.iter().enumerate() {
            if !context.used.contains(&index) {
                return Err(Diagnostic::new(
                    param.span.clone(),
                    format!("unused parameter: {}", param.name),
                ));
            }
        }
    }

    Ok(())
}

/// Validate a source symbol and its converted Rust spelling, including reserved
/// words.
fn rust_name(node: &Node) -> Result<String, Diagnostic> {
    let name = node.symbol()?.as_str();
    let mapped = name.replace('-', "_");
    if !valid_identifier(&mapped)
        || mapped == "_"
        || !name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        return Err(node.error("expected an ASCII name beginning with a letter or underscore"));
    }

    // Include reserved and edition-specific keywords, even when syn accepts
    // them as raw identifiers.
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do",
        "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
        "gen",
    ];
    if KEYWORDS.contains(&mapped.as_str()) || syn::parse_str::<syn::Ident>(&mapped).is_err() {
        return Err(node.error(format!("name maps to a Rust reserved word: {mapped}")));
    }

    Ok(mapped)
}

/// Require a keyword containing a supported unqualified SQL identifier.
fn column(node: &Node) -> Result<(), Diagnostic> {
    let name = node
        .keyword()
        .map_err(|_| node.error("SQL identifiers must use a keyword, e.g. :users"))?;
    if !valid_identifier(name.as_str()) {
        return Err(node.error("invalid SQL identifier; expected an unqualified ASCII name"));
    }
    Ok(())
}

fn predicate(node: &Node, ty: Type) -> Result<(), Diagnostic> {
    // Without a schema, a column could be boolean; reject only known
    // non-boolean types.
    if !matches!(ty, Type::Bool | Type::Unknown) {
        return Err(node.error("expected a boolean SQL expression"));
    }
    Ok(())
}

/// Checker-local bindings carry only the information required for validation.
struct Binding {
    name: crate::ast::Symbol,
    ty: Type,
    span: crate::diagnostic::Span,
}

/// Query-local type environment and parameter-use tracking; contains no query
/// IR.
struct Context<'a> {
    params: &'a [Binding],
    used: HashSet<usize>,
}

impl Context<'_> {
    /// Check one SQL expression, infer its type, and record referenced
    /// parameters.
    fn expr(&mut self, node: &Node) -> Result<Type, Diagnostic> {
        let ty = match &node.inner {
            NodeInner::String(_) => Type::String,
            NodeInner::Keyword(_) => {
                column(node)?;
                Type::Unknown
            }
            NodeInner::Boolean(_) => Type::Bool,
            NodeInner::Integer(_) => Type::I64,
            NodeInner::Float(_) => {
                return Err(node.error("floating-point literals are not supported in SQL v1"));
            }
            NodeInner::Quoted { .. } => {
                return Err(node.error("quoted expressions are not supported in SQL v1"));
            }
            NodeInner::Symbol(s) => {
                let (index, param) = self
                    .params
                    .iter()
                    .enumerate()
                    .find(|(_, p)| &p.name == s)
                    .ok_or_else(|| node.error(format!("unknown parameter: {s}")))?;
                self.used.insert(index);
                param.ty
            }
            NodeInner::List(items) => {
                let head = items
                    .first()
                    .ok_or_else(|| node.error("empty SQL expression"))?;
                let op = head
                    .keyword()
                    .map_err(|_| head.error("SQL operators use keywords, e.g. :> instead of >"))?;
                let args = &items[1..];
                let expected = match op.as_str() {
                    "=" | "!=" | ">" | ">=" | "<" | "<=" => 2,
                    "not" | "is-null" | "is-not-null" => 1,
                    "and" | "or" => 0,
                    _ => {
                        return Err(head.error(format!(
                            "unknown SQL operator: :{op}; SQL operators use keywords"
                        )))
                    }
                };
                if (expected != 0 && args.len() != expected) || (expected == 0 && args.len() < 2) {
                    return Err(node.error(format!("wrong operand count for :{op}")));
                }
                let types = args
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                match op.as_str() {
                    "and" | "or" | "not" => {
                        for (arg, ty) in args.iter().zip(types) {
                            predicate(arg, ty)?;
                        }
                    }
                    "is-null" | "is-not-null" => (),
                    _ => {
                        let (left, right) = (types[0], types[1]);
                        if left != Type::Unknown
                            && right != Type::Unknown
                            && left != right
                            && !(left.integer() && right.integer())
                        {
                            return Err(
                                node.error("incompatible known operand types in comparison")
                            );
                        }
                        if !matches!(op.as_str(), "=" | "!=")
                            && (left == Type::Bool || right == Type::Bool)
                        {
                            return Err(
                                node.error("ordered comparison does not support boolean operands")
                            );
                        }
                    }
                }
                Type::Bool
            }
        };
        Ok(ty)
    }
}
