use crate::protocol::{Diagnostic, Hover, Location, MarkupContent, Position, Range};
use trulang_syntax::ast::*;
use trulang_syntax::span::Span;

pub struct Analyzer;

impl Analyzer {
    /// Validates the document for syntax and type errors, producing LSP Diagnostics
    pub fn validate(content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let program = match trulang_syntax::parse(content) {
            Ok(prog) => prog,
            Err(err) => {
                let range = span_to_range(&err.span, content);
                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(1), // Error
                    message: err.message,
                    source: Some("trulang".into()),
                });
                return diagnostics;
            }
        };

        let mut checker = trulang_compiler::type_checker::TypeChecker::new();
        if let Err(err) = checker.check_program(&program) {
            let range = span_to_range(&err.span, content);
            let msg = if let Some(hint) = err.hint {
                format!("{}\nHint: {}", err.message, hint)
            } else {
                err.message
            };
            diagnostics.push(Diagnostic {
                range,
                severity: Some(1), // Error
                message: msg,
                source: Some("trulang-typecheck".into()),
            });
        }

        diagnostics
    }

    /// Computes hover tooltip markdown at the given position
    pub fn hover(content: &str, pos: Position) -> Option<Hover> {
        let program = trulang_syntax::parse(content).ok()?;
        let target_word = get_word_at_position(content, &pos)?;

        // Search Declarations
        for decl in &program.declarations {
            match &decl.node {
                Declaration::Service(svc) => {
                    if svc.name == target_word {
                        let mut req_budget = "16 MB".to_string();
                        let mut persistent_budget = "512 MB".to_string();
                        for b in &svc.budgets {
                            let mb = b.node.size_bytes / (1024 * 1024);
                            match b.node.region {
                                BudgetRegion::Request => {
                                    if let Some(max) = b.node.max_size_bytes {
                                        req_budget = format!("{} MB (burst up to {} MB)", mb, max / (1024 * 1024));
                                    } else {
                                        req_budget = format!("{} MB (strict)", mb);
                                    }
                                }
                                BudgetRegion::Persistent => {
                                    persistent_budget = format!("{} MB", mb);
                                }
                            }
                        }

                        let doc = format!(
                            "### Service Actor: `{}`\n\n\
                            **TruLang Zero-Lock Concurrency Unit**\n\n\
                            - **Request Arena (Region A)**: `{}` with $O(1)$ zero-pause reclamation\n\
                            - **Persistent State (Region B)**: `{}` reference-counted DAG\n\
                            - **Mailbox**: Bounded single-threaded queue with automatic backpressure",
                            svc.name, req_budget, persistent_budget
                        );
                        return Some(Hover {
                            contents: MarkupContent {
                                kind: "markdown".into(),
                                value: doc,
                            },
                            range: None,
                        });
                    }

                    // Search service fields
                    for field in &svc.fields {
                        if field.node.name == target_word {
                            let mutability = if field.node.is_mut { "mut " } else { "" };
                            let ty_str = field.node.ty.as_ref().map_or("inferred".into(), |t| format!("{:?}", t));
                            let doc = format!(
                                "```trulang\nlet {}{}: {}\n```\n\n\
                                **Persistent Actor Field (Region B)**\n\n\
                                Values assigned or inserted here are automatically promoted to Region B with cycle detection.",
                                mutability, field.node.name, ty_str
                            );
                            return Some(Hover {
                                contents: MarkupContent {
                                    kind: "markdown".into(),
                                    value: doc,
                                },
                                range: None,
                            });
                        }
                    }

                    // Search service methods
                    for method in &svc.methods {
                        if method.node.name == target_word {
                            let params_str: Vec<String> = method.node.params.iter().map(|p| {
                                let m = if p.is_mut { "mut " } else { "" };
                                format!("{}{}: {:?}", m, p.name, p.ty)
                            }).collect();
                            let ret_str = method.node.return_type.as_ref().map_or("void".into(), |t| format!("{:?}", t));

                            let doc = format!(
                                "```trulang\nfn {}({}) -> {}\n```\n\n\
                                **Actor Request Handler Method**\n\n\
                                Executes sequentially inside `{}`'s dedicated thread. Transient request variables are allocated inside Region A.",
                                method.node.name, params_str.join(", "), ret_str, svc.name
                            );
                            return Some(Hover {
                                contents: MarkupContent {
                                    kind: "markdown".into(),
                                    value: doc,
                                },
                                range: None,
                            });
                        }
                    }
                }
                Declaration::Function(func) if func.name == target_word => {
                    let params_str: Vec<String> = func.params.iter().map(|p| {
                        let m = if p.is_mut { "mut " } else { "" };
                        format!("{}{}: {:?}", m, p.name, p.ty)
                    }).collect();
                    let ret_str = func.return_type.as_ref().map_or("void".into(), |t| format!("{:?}", t));

                    let doc = format!(
                        "```trulang\nfn {}({}) -> {}\n```\n\n\
                        **Standard Function**",
                        func.name, params_str.join(", "), ret_str
                    );
                    return Some(Hover {
                        contents: MarkupContent {
                            kind: "markdown".into(),
                            value: doc,
                        },
                        range: None,
                    });
                }
                Declaration::Import(imp) if imp.alias == target_word => {
                    let doc = format!(
                        "### Module `{}`\n\nForeign import from `{}` via Safe Rust/C FFI Bridge.",
                        imp.alias, imp.path
                    );
                    return Some(Hover {
                        contents: MarkupContent {
                            kind: "markdown".into(),
                            value: doc,
                        },
                        range: None,
                    });
                }
                _ => {}
            }
        }

        // Keywords hover
        match target_word.as_str() {
            "persist" => Some(Hover {
                contents: MarkupContent {
                    kind: "markdown".into(),
                    value: "### `persist(value)`\n\nPromotes an object from temporary Region A (execution memory) to persistent Region B (long-lived actor state).\n\n*Note: TruLang compiler automatically infers persist calls when writing to actor persistent fields.*".into(),
                },
                range: None,
            }),
            "defer" => Some(Hover {
                contents: MarkupContent {
                    kind: "markdown".into(),
                    value: "### `defer <expr>;`\n\nGuarantees execution of `<expr>` in LIFO order upon exiting the declaring scope or throwing an exception.".into(),
                },
                range: None,
            }),
            "memory_budget" => Some(Hover {
                contents: MarkupContent {
                    kind: "markdown".into(),
                    value: "### `memory_budget <region> = <base> [max <ceiling>];`\n\nConfigures bounded execution memory. Region `request` supports elastic burst headroom via `max`.".into(),
                },
                range: None,
            }),
            _ => None,
        }
    }

    /// Locates the declaration for the symbol at the given position
    pub fn definition(content: &str, uri: &str, pos: Position) -> Option<Location> {
        let program = trulang_syntax::parse(content).ok()?;
        let target_word = get_word_at_position(content, &pos)?;

        for decl in &program.declarations {
            match &decl.node {
                Declaration::Service(svc) => {
                    if svc.name == target_word {
                        return Some(Location {
                            uri: uri.to_string(),
                            range: span_to_range(&decl.span, content),
                        });
                    }
                    for field in &svc.fields {
                        if field.node.name == target_word {
                            return Some(Location {
                                uri: uri.to_string(),
                                range: span_to_range(&field.span, content),
                            });
                        }
                    }
                    for method in &svc.methods {
                        if method.node.name == target_word {
                            return Some(Location {
                                uri: uri.to_string(),
                                range: span_to_range(&method.span, content),
                            });
                        }
                    }
                }
                Declaration::Function(func) if func.name == target_word => {
                    return Some(Location {
                        uri: uri.to_string(),
                        range: span_to_range(&decl.span, content),
                    });
                }
                Declaration::Import(imp) if imp.alias == target_word => {
                    return Some(Location {
                        uri: uri.to_string(),
                        range: span_to_range(&decl.span, content),
                    });
                }
                _ => {}
            }
        }

        None
    }
}

fn span_to_range(span: &Span, content: &str) -> Range {
    let line = span.line.saturating_sub(1) as u32;
    let col = span.col.saturating_sub(1) as u32;
    
    // Calculate approximate length from byte delta
    let delta = if span.end > span.start {
        (span.end - span.start) as u32
    } else {
        1
    };

    // Make sure we don't overflow the line
    let line_len = content.lines().nth(line as usize).map_or(col + delta, |l| l.len() as u32);
    let end_col = (col + delta).min(line_len);

    Range::new(
        Position::new(line, col),
        Position::new(line, end_col.max(col + 1)),
    )
}

fn get_word_at_position(content: &str, pos: &Position) -> Option<String> {
    let line = content.lines().nth(pos.line as usize)?;
    let col = pos.character as usize;
    if col >= line.len() {
        return None;
    }

    let bytes = line.as_bytes();
    let mut start = col;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }

    let mut end = col;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }

    if start == end {
        None
    } else {
        Some(line[start..end].to_string())
    }
}
