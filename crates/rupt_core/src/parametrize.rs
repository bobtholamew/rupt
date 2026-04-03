use ruff_python_ast::{self as ast, Expr};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ParametrizeArgs {
    pub arg_names: Vec<String>,
    pub case_ids: Vec<String>,
}

pub fn parse_parametrize_call(call: &ast::ExprCall) -> Option<ParametrizeArgs> {
    if call.arguments.args.len() < 2 {
        return None;
    }

    let arg_names = parse_param_names(&call.arguments.args[0])?;
    let mut case_ids = extract_case_ids(call, &call.arguments.args[1]);

    // Deduplicate IDs the same way pytest does: append 0, 1, 2... for collisions
    deduplicate_ids(&mut case_ids);

    Some(ParametrizeArgs {
        arg_names,
        case_ids,
    })
}

pub fn expand_parametrize(base_id: &str, params: &[ParametrizeArgs]) -> Vec<String> {
    if params.is_empty() {
        return vec![base_id.to_string()];
    }

    // pytest combines all stacked parametrize into one bracket: [a-b-c]
    // The topmost decorator's values appear first in the ID, and the bottommost
    // decorator's values vary fastest. So we iterate forward (top-to-bottom)
    // and the last decorator processed becomes the innermost loop.
    let mut combined_ids: Vec<String> = vec![String::new()];

    for param in params.iter() {
        if param.case_ids.is_empty() {
            continue;
        }
        let mut new_combined = Vec::new();
        for existing in &combined_ids {
            for case_id in &param.case_ids {
                if existing.is_empty() {
                    new_combined.push(case_id.clone());
                } else {
                    new_combined.push(format!("{existing}-{case_id}"));
                }
            }
        }
        combined_ids = new_combined;
    }

    let mut final_ids: Vec<String> = combined_ids
        .into_iter()
        .map(|suffix| format!("{base_id}[{suffix}]"))
        .collect();

    deduplicate_ids(&mut final_ids);
    final_ids
}

fn deduplicate_ids(ids: &mut Vec<String>) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for id in ids.iter() {
        *counts.entry(id.clone()).or_insert(0) += 1;
    }

    // Only deduplicate if there are actual duplicates
    let has_dupes = counts.values().any(|&c| c > 1);
    if !has_dupes {
        return;
    }

    let mut seen: HashMap<String, usize> = HashMap::new();
    for id in ids.iter_mut() {
        let count = counts.get(id.as_str()).copied().unwrap_or(0);
        if count > 1 {
            let idx = seen.entry(id.clone()).or_insert(0);
            *id = format!("{id}{idx}");
            *idx += 1;
        }
    }
}

fn parse_param_names(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::StringLiteral(s) => {
            let value = s.value.to_str();
            let names: Vec<String> = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if names.is_empty() { None } else { Some(names) }
        }
        Expr::List(list) => extract_string_list(&list.elts),
        Expr::Tuple(tuple) => extract_string_list(&tuple.elts),
        _ => None,
    }
}

fn extract_string_list(elts: &[Expr]) -> Option<Vec<String>> {
    let names: Vec<String> = elts
        .iter()
        .filter_map(|e| {
            if let Expr::StringLiteral(s) = e {
                Some(s.value.to_str().to_string())
            } else {
                None
            }
        })
        .collect();
    if names.is_empty() { None } else { Some(names) }
}

fn extract_case_ids(call: &ast::ExprCall, values_expr: &Expr) -> Vec<String> {
    // Check for explicit `ids=` keyword argument first
    for kw in call.arguments.keywords.iter() {
        if let Some(ref arg) = kw.arg {
            if arg.as_str() == "ids" {
                if let Some(ids) = extract_explicit_ids(&kw.value) {
                    return ids;
                }
            }
        }
    }

    let cases = match values_expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return vec![],
    };

    cases.iter().map(|case| case_to_id(case)).collect()
}

fn extract_explicit_ids(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::List(list) => extract_string_list(&list.elts),
        Expr::Tuple(tuple) => extract_string_list(&tuple.elts),
        _ => None,
    }
}

fn case_to_id(expr: &Expr) -> String {
    match expr {
        Expr::Tuple(tuple) => {
            tuple.elts.iter().map(|e| value_repr(e)).collect::<Vec<_>>().join("-")
        }
        Expr::Call(call) => {
            let chain = resolve_attr_chain(&call.func);
            if chain.last().is_some_and(|s| s == "param") {
                // Check for id= keyword
                for kw in call.arguments.keywords.iter() {
                    if let Some(ref arg) = kw.arg {
                        if arg.as_str() == "id" {
                            if let Expr::StringLiteral(s) = &kw.value {
                                return s.value.to_str().to_string();
                            }
                        }
                    }
                }
                call.arguments.args.iter().map(|e| value_repr(e)).collect::<Vec<_>>().join("-")
            } else {
                // Generic function call — use the function name + args
                let func_name = chain.last().cloned().unwrap_or_default();
                let args: Vec<String> = call.arguments.args.iter().map(|e| value_repr(e)).collect();
                if args.is_empty() {
                    format!("{func_name}()")
                } else {
                    format!("{}({})", func_name, args.join(", "))
                }
            }
        }
        _ => value_repr(expr),
    }
}

fn value_repr(expr: &Expr) -> String {
    match expr {
        Expr::NumberLiteral(n) => {
            match &n.value {
                ast::Number::Int(i) => format!("{i}"),
                ast::Number::Float(f) => {
                    let s = format!("{f}");
                    // Ensure float always has decimal point (pytest shows "1.0" not "1")
                    if s.contains('.') { s } else { format!("{s}.0") }
                }
                ast::Number::Complex { real, imag } => format!("{real}+{imag}j"),
            }
        }
        Expr::StringLiteral(s) => s.value.to_str().to_string(),
        Expr::BytesLiteral(b) => {
            // Convert bytes to string, matching pytest's behavior
            String::from_utf8(b.value.bytes().collect())
                .unwrap_or_else(|_| {
                    // Non-UTF8 bytes: show as hex
                    let bytes: Vec<u8> = b.value.bytes().collect();
                    format!("{bytes:?}")
                })
        }
        Expr::BooleanLiteral(b) => {
            if b.value { "True".to_string() } else { "False".to_string() }
        }
        Expr::NoneLiteral(_) => "None".to_string(),
        Expr::Name(n) => n.id.to_string(),
        Expr::UnaryOp(u) => {
            let operand = value_repr(&u.operand);
            match u.op {
                ast::UnaryOp::USub => format!("-{operand}"),
                ast::UnaryOp::UAdd => format!("+{operand}"),
                _ => operand,
            }
        }
        Expr::Tuple(t) => {
            t.elts.iter().map(|e| value_repr(e)).collect::<Vec<_>>().join("-")
        }
        Expr::List(l) => {
            l.elts.iter().map(|e| value_repr(e)).collect::<Vec<_>>().join("-")
        }
        Expr::Dict(d) => {
            let items: Vec<String> = d.items.iter().map(|item| {
                let key = item.key.as_ref().map(|k| value_repr(k)).unwrap_or_default();
                let val = value_repr(&item.value);
                format!("{key}: {val}")
            }).collect();
            format!("{{{}}}", items.join(", "))
        }
        Expr::Call(call) => {
            let chain = resolve_attr_chain(&call.func);
            let func_name = if chain.is_empty() {
                "call".to_string()
            } else {
                chain.join(".")
            };
            let args: Vec<String> = call.arguments.args.iter().map(|e| value_repr(e)).collect();
            if args.is_empty() {
                format!("{func_name}()")
            } else {
                format!("{}({})", func_name, args.join(", "))
            }
        }
        Expr::Attribute(attr) => {
            let base = value_repr(&attr.value);
            format!("{base}.{}", attr.attr)
        }
        Expr::FString(f) => {
            // f-strings can't be statically evaluated easily
            let parts: Vec<String> = f.value.iter().map(|part| {
                match part {
                    ast::FStringPart::Literal(s) => s.to_string(),
                    ast::FStringPart::FString(_) => "<fstring>".to_string(),
                }
            }).collect();
            parts.join("")
        }
        Expr::Lambda(_) => "<lambda>".to_string(),
        Expr::Set(s) => {
            let items: Vec<String> = s.elts.iter().map(|e| value_repr(e)).collect();
            format!("{{{}}}", items.join(", "))
        }
        _ => {
            // For anything we can't handle, use a short placeholder
            // This is better than spewing raw AST debug output
            let variant = format!("{expr:?}");
            if variant.len() > 50 {
                format!("{}...", &variant[..47])
            } else {
                variant
            }
        }
    }
}

fn resolve_attr_chain(expr: &Expr) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = expr;
    loop {
        match current {
            Expr::Attribute(attr) => {
                chain.push(attr.attr.to_string());
                current = &attr.value;
            }
            Expr::Name(name) => {
                chain.push(name.id.to_string());
                break;
            }
            _ => break,
        }
    }
    chain.reverse();
    chain
}
