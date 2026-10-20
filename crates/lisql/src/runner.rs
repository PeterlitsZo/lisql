//! Evaluate query declarations into query IR for Rust code generation.

use crate::{
    ast::{Ast, Node, NodeInner},
    diagnostic::Diagnostic,
    ir::{Expr, ExprKind, Op, OpKind, Param, Query, Type},
    semantic,
};
use lisql_runtime::{Compare, Direction, Value};

/// Evaluate supported query forms in source order without executing database
/// SQL.
///
/// Parameters remain symbolic references because their values are supplied to
/// the
/// generated Rust functions. Validation runs before evaluation, so invalid
/// input
/// returns a diagnostic rather than a partial collection of queries.
pub fn eval(ast: &Ast) -> Result<Vec<Query>, Diagnostic> {
    semantic::check(ast)?;
    Ok(ast.nodes.iter().map(query).collect())
}

// These helpers are private: eval validates the entire AST before entering
// them.
// They construct values only; language diagnostics belong to semantic::check.
/// Evaluate one checked declaration with its own parameter environment.
fn query(node: &Node) -> Query {
    let form = node.list().expect("checked declaration");
    let params: Vec<_> = form[2]
        .list()
        .expect("checked parameters")
        .chunks_exact(2)
        .map(|pair| {
            let name = pair[0].symbol().expect("checked parameter name").clone();
            let ty = match pair[1].keyword().expect("checked parameter type").as_str() {
                "i32" => Type::I32,
                "i64" => Type::I64,
                "bool" => Type::Bool,
                "string" => Type::String,
                _ => unreachable!("checked parameter type"),
            };
            Param {
                rust_name: name.as_str().replace('-', "_"),
                name,
                ty,
                span: pair[0].span.clone(),
            }
        })
        .collect();

    // -> evaluates its operators in source order into one composed query.
    let ops = form[3].list().expect("checked pipeline")[1..]
        .iter()
        .map(|node| operator(node, &params))
        .collect();
    Query {
        rust_name: form[1]
            .symbol()
            .expect("checked query name")
            .as_str()
            .replace('-', "_"),
        params,
        ops,
        span: node.span.clone(),
    }
}

/// Extract a SQL identifier already validated by the semantic checker.
fn column(node: &Node) -> String {
    node.keyword().expect("checked column").as_str().to_owned()
}

/// Evaluate one checked pipeline step using the owning query’s parameters.
fn operator(node: &Node, params: &[Param]) -> Op {
    let items = node.list().expect("checked operator");
    let head = items[0].symbol().expect("checked operator name").as_str();
    let kind = match head {
        "from" => OpKind::From(column(&items[1])),
        "where" => OpKind::Where(expression(&items[1], params)),
        "select" => OpKind::Select(
            items[1]
                .list()
                .expect("checked selection")
                .iter()
                .map(column)
                .collect(),
        ),
        "order-by" => OpKind::OrderBy(
            items[1..]
                .iter()
                .map(|item| {
                    let parts = item.list().expect("checked ordering");
                    let direction = match parts[1].keyword().expect("checked direction").as_str() {
                        "asc" => Direction::Asc,
                        "desc" => Direction::Desc,
                        _ => unreachable!("checked direction"),
                    };
                    (column(&parts[0]), direction)
                })
                .collect(),
        ),
        "limit" => OpKind::Limit(expression(&items[1], params)),
        "offset" => OpKind::Offset(expression(&items[1], params)),
        _ => unreachable!("checked operator"),
    };
    Op {
        kind,
        span: node.span.clone(),
    }
}

/// Build SQL expression IR while preserving symbolic parameter references.
fn expression(node: &Node, params: &[Param]) -> Expr {
    let (kind, ty) = match &node.inner {
        NodeInner::String(value) => (
            ExprKind::Literal(Value::String(value.clone())),
            Type::String,
        ),
        NodeInner::Integer(value) => (ExprKind::Literal(Value::I64(*value)), Type::I64),
        NodeInner::Boolean(value) => (ExprKind::Literal(Value::Bool(*value)), Type::Bool),
        NodeInner::Keyword(_) => (ExprKind::Column(column(node)), Type::Unknown),
        NodeInner::Symbol(name) => {
            let index = params
                .iter()
                .position(|param| &param.name == name)
                .expect("checked parameter reference");
            (ExprKind::Param(index), params[index].ty)
        }
        NodeInner::List(items) => {
            let op = items[0].keyword().expect("checked SQL operator").as_str();
            let mut args = items[1..].iter().map(|arg| expression(arg, params));
            let kind = match op {
                "and" => ExprKind::And(args.collect()),
                "or" => ExprKind::Or(args.collect()),
                "not" => ExprKind::Not(Box::new(args.next().expect("checked arity"))),
                "is-null" => ExprKind::IsNull(Box::new(args.next().expect("checked arity"))),
                "is-not-null" => ExprKind::IsNotNull(Box::new(args.next().expect("checked arity"))),
                _ => {
                    let comparison = match op {
                        "=" => Compare::Eq,
                        "!=" => Compare::Ne,
                        ">" => Compare::Gt,
                        ">=" => Compare::Ge,
                        "<" => Compare::Lt,
                        "<=" => Compare::Le,
                        _ => unreachable!("checked comparison"),
                    };
                    ExprKind::Compare(
                        comparison,
                        Box::new(args.next().expect("checked arity")),
                        Box::new(args.next().expect("checked arity")),
                    )
                }
            };
            (kind, Type::Bool)
        }
        NodeInner::Float(_) | NodeInner::Quoted { .. } => {
            unreachable!("unsupported forms rejected by checker")
        }
    };
    Expr {
        kind,
        ty,
        span: node.span.clone(),
    }
}
