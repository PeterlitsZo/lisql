//! Build a source AST from Lisp tokens; query-specific rules belong to semantic
//! analysis.

use crate::{
    ast::{Ast, Node, NodeInner, QuoteKind},
    diagnostic::{Diagnostic, Span},
    lexer::{Token, TokenInner},
};

/// A pending list or reader prefix. Prefixes count toward the nesting limit
/// too.
enum Frame {
    /// An open list with its opening byte offset and completed children.
    List { start: usize, nodes: Vec<Node> },
    /// A reader prefix waiting for exactly one complete operand.
    Quote { span: Span, kind: QuoteKind },
}

/// Parse tokens into a syntax tree without resolving symbols or evaluating
/// forms.
///
/// An empty source produces an empty AST. SQL compilation separately requires
/// at
/// least one query declaration. Lists and reader prefixes share a 128-level
/// limit.
///
/// ```
/// use lisql::{lex, parse, NodeInner};
///
/// let ast = parse(lex("(+ 1 2)")?)?;
/// assert!(matches!(ast.nodes[0].inner, NodeInner::List(_)));
/// # Ok::<(), lisql::Diagnostic>(())
/// ```
pub fn parse(tokens: Vec<Token>) -> Result<Ast, Diagnostic> {
    // Iterative construction bounds nesting before recursive semantic/codegen
    // passes run.
    let mut stack: Vec<Frame> = Vec::new();
    let mut ast = Ast::default();

    for token in tokens {
        let inner = match token.inner {
            TokenInner::LeftParen | TokenInner::Quote(_) => {
                if stack.len() >= 128 {
                    return Err(Diagnostic::new(
                        token.span,
                        "nesting exceeds the 128-level limit",
                    ));
                }
                let frame = match token.inner {
                    TokenInner::Quote(kind) => Frame::Quote {
                        span: token.span,
                        kind,
                    },
                    _ => Frame::List {
                        start: token.span.start,
                        nodes: Vec::new(),
                    },
                };
                stack.push(frame);
                continue;
            }
            TokenInner::RightParen => {
                let frame = stack.pop().ok_or_else(|| {
                    Diagnostic::new(token.span.clone(), "unexpected closing parenthesis")
                })?;
                match frame {
                    Frame::List { start, nodes } => {
                        append(
                            Node {
                                inner: NodeInner::List(nodes),
                                span: start..token.span.end,
                            },
                            &mut stack,
                            &mut ast,
                        );
                        continue;
                    }
                    Frame::Quote { span, .. } => {
                        return Err(Diagnostic::new(
                            span,
                            "expected expression after reader prefix",
                        ));
                    }
                }
            }
            TokenInner::Symbol(value) => NodeInner::Symbol(value),
            TokenInner::Keyword(value) => NodeInner::Keyword(value),
            TokenInner::Integer(value) => NodeInner::Integer(value),
            TokenInner::Float(value) => NodeInner::Float(value),
            TokenInner::Boolean(value) => NodeInner::Boolean(value),
            TokenInner::String(value) => NodeInner::String(value),
        };

        append(
            Node {
                inner,
                span: token.span,
            },
            &mut stack,
            &mut ast,
        );
    }

    match stack.last() {
        Some(Frame::List { start, .. }) => {
            Err(Diagnostic::new(*start..start + 1, "unclosed parenthesis"))
        }
        Some(Frame::Quote { span, .. }) => Err(Diagnostic::new(
            span.clone(),
            "expected expression after reader prefix",
        )),
        None => Ok(ast),
    }
}

fn append(mut node: Node, stack: &mut Vec<Frame>, ast: &mut Ast) {
    // Prefixes bind to exactly one complete expression, including an entire
    // nested list.
    while let Some(Frame::Quote { .. }) = stack.last() {
        let Some(Frame::Quote { span, kind }) = stack.pop() else {
            unreachable!()
        };
        node = Node {
            span: span.start..node.span.end,
            inner: NodeInner::Quoted {
                kind,
                value: Box::new(node),
            },
        };
    }

    if let Some(Frame::List { nodes, .. }) = stack.last_mut() {
        nodes.push(node);
    } else {
        ast.nodes.push(node);
    }
}
