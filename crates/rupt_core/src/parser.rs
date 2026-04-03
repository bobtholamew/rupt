use ruff_python_ast::{self as ast, Stmt, Expr, Decorator};
use ruff_python_parser::{parse_unchecked, Mode, ParseOptions};
use std::path::Path;

use crate::markers::Marker;
use crate::parametrize::ParametrizeArgs;

#[derive(Debug, Clone)]
pub struct TestItem {
    pub node_id: String,
    pub markers: Vec<Marker>,
    pub parametrize: Vec<ParametrizeArgs>,
    pub is_method: bool,
    pub class_name: Option<String>,
    pub function_name: String,
}

pub fn parse_test_file(path: &Path, relative: &Path) -> Vec<TestItem> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let parsed = parse_unchecked(&source, ParseOptions::from(Mode::Module));
    let module = parsed.into_syntax();

    let file_path_str = relative.to_string_lossy().replace('\\', "/");
    let mut items = Vec::new();

    let body = match module {
        ast::Mod::Module(m) => m.body,
        _ => return vec![],
    };

    for stmt in &body {
        match stmt {
            Stmt::FunctionDef(func) => {
                let name = func.name.as_str();
                if is_test_function(name) {
                    let markers = extract_markers(&func.decorator_list);
                    let parametrize = extract_parametrize(&func.decorator_list);
                    let node_id = format!("{file_path_str}::{name}");
                    items.push(TestItem {
                        node_id,
                        markers,
                        parametrize,
                        is_method: false,
                        class_name: None,
                        function_name: name.to_string(),
                    });
                }
            }
            Stmt::ClassDef(class) => {
                let class_name = class.name.as_str();
                if is_test_class(class_name) {
                    for class_stmt in &class.body {
                        if let Stmt::FunctionDef(method) = class_stmt {
                            let method_name = method.name.as_str();
                            if is_test_function(method_name) {
                                let mut markers = extract_markers(&class.decorator_list);
                                markers.extend(extract_markers(&method.decorator_list));
                                let parametrize = {
                                    let mut p = extract_parametrize(&class.decorator_list);
                                    p.extend(extract_parametrize(&method.decorator_list));
                                    p
                                };
                                let node_id = format!(
                                    "{file_path_str}::{class_name}::{method_name}"
                                );
                                items.push(TestItem {
                                    node_id,
                                    markers,
                                    parametrize,
                                    is_method: true,
                                    class_name: Some(class_name.to_string()),
                                    function_name: method_name.to_string(),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    items
}

fn is_test_function(name: &str) -> bool {
    name.starts_with("test_") || name == "test"
}

fn is_test_class(name: &str) -> bool {
    name.starts_with("Test") && !name.contains("Mixin")
}

fn extract_markers(decorators: &[Decorator]) -> Vec<Marker> {
    let mut markers = Vec::new();

    for dec in decorators {
        if let Some(marker) = parse_marker_decorator(&dec.expression) {
            markers.push(marker);
        }
    }

    markers
}

fn parse_marker_decorator(expr: &Expr) -> Option<Marker> {
    match expr {
        // @pytest.mark.skip
        Expr::Attribute(attr) => {
            let chain = resolve_attribute_chain(expr);
            if chain.len() >= 3 && chain[0] == "pytest" && chain[1] == "mark" {
                return Some(Marker {
                    name: chain[2..].join("."),
                    args: vec![],
                });
            }
            // Single attribute like @mark.skip (less common but valid)
            if chain.len() >= 2 && chain[0] == "mark" {
                return Some(Marker {
                    name: chain[1..].join("."),
                    args: vec![],
                });
            }
            let _ = attr;
            None
        }
        // @pytest.mark.skip(reason="...")
        Expr::Call(call) => {
            let chain = resolve_attribute_chain(&call.func);
            if chain.len() >= 3 && chain[0] == "pytest" && chain[1] == "mark" {
                return Some(Marker {
                    name: chain[2..].join("."),
                    args: vec![], // We don't need arg values for collection
                });
            }
            if chain.len() >= 2 && chain[0] == "mark" {
                return Some(Marker {
                    name: chain[1..].join("."),
                    args: vec![],
                });
            }
            None
        }
        _ => None,
    }
}

fn resolve_attribute_chain(expr: &Expr) -> Vec<String> {
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

pub fn extract_parametrize(decorators: &[Decorator]) -> Vec<ParametrizeArgs> {
    let mut params = Vec::new();

    for dec in decorators {
        if let Expr::Call(call) = &dec.expression {
            let chain = resolve_attribute_chain(&call.func);
            if chain.len() >= 3
                && chain[0] == "pytest"
                && chain[1] == "mark"
                && chain[2] == "parametrize"
            {
                if let Some(p) = crate::parametrize::parse_parametrize_call(call) {
                    params.push(p);
                }
            }
        }
    }

    params
}
