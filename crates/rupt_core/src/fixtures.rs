use ruff_python_ast::{self as ast, Stmt, Expr};
use ruff_python_parser::{parse_unchecked, Mode, ParseOptions};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FixtureDef {
    pub name: String,
    pub scope: FixtureScope,
    pub params: bool,
    pub autouse: bool,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FixtureScope {
    Function,
    Class,
    Module,
    Package,
    Session,
}

pub fn extract_fixtures(path: &Path) -> Vec<FixtureDef> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let parsed = parse_unchecked(&source, ParseOptions::from(Mode::Module));
    let module = parsed.into_syntax();
    let file_str = path.to_string_lossy().to_string();

    let body = match module {
        ast::Mod::Module(m) => m.body,
        _ => return vec![],
    };

    let mut fixtures = Vec::new();
    extract_fixtures_from_body(&body, &file_str, &mut fixtures);
    fixtures
}

fn extract_fixtures_from_body(body: &[Stmt], file: &str, fixtures: &mut Vec<FixtureDef>) {
    for stmt in body {
        if let Stmt::FunctionDef(func) = stmt {
            if let Some(fixture) = parse_fixture_def(func, file) {
                fixtures.push(fixture);
            }
        }
    }
}

fn parse_fixture_def(func: &ast::StmtFunctionDef, file: &str) -> Option<FixtureDef> {
    for dec in &func.decorator_list {
        if is_fixture_decorator(&dec.expression) {
            let (scope, autouse) = extract_fixture_args(&dec.expression);
            return Some(FixtureDef {
                name: func.name.to_string(),
                scope,
                params: false,
                autouse,
                file: file.to_string(),
            });
        }
    }
    None
}

fn is_fixture_decorator(expr: &Expr) -> bool {
    match expr {
        Expr::Attribute(attr) => {
            if let Expr::Name(name) = &*attr.value {
                name.id.as_str() == "pytest" && attr.attr.as_str() == "fixture"
            } else {
                false
            }
        }
        Expr::Call(call) => is_fixture_decorator(&call.func),
        Expr::Name(name) => name.id.as_str() == "fixture",
        _ => false,
    }
}

fn extract_fixture_args(expr: &Expr) -> (FixtureScope, bool) {
    let mut scope = FixtureScope::Function;
    let mut autouse = false;

    if let Expr::Call(call) = expr {
        for kw in call.arguments.keywords.iter() {
            if let Some(ref arg) = kw.arg {
                match arg.as_str() {
                    "scope" => {
                        if let Expr::StringLiteral(s) = &kw.value {
                            scope = match s.value.to_str() {
                                "class" => FixtureScope::Class,
                                "module" => FixtureScope::Module,
                                "package" => FixtureScope::Package,
                                "session" => FixtureScope::Session,
                                _ => FixtureScope::Function,
                            };
                        }
                    }
                    "autouse" => {
                        if let Expr::BooleanLiteral(b) = &kw.value {
                            autouse = b.value;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    (scope, autouse)
}

/// Build fixture names available at a given decorator list
pub fn extract_fixture_dependencies(func: &ast::StmtFunctionDef) -> Vec<String> {
    func.parameters
        .args
        .iter()
        .filter_map(|param| {
            let name = param.parameter.name.as_str();
            if name == "self" || name == "cls" {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}
