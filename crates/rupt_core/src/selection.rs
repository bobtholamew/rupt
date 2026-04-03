use crate::config::PytestConfig;
use crate::parser::TestItem;

pub fn filter_items(
    items: Vec<TestItem>,
    k_expr: Option<&str>,
    m_expr: Option<&str>,
    _config: &PytestConfig,
) -> Vec<TestItem> {
    let mut result = items;

    if let Some(expr) = k_expr {
        let filter = parse_k_expression(expr);
        result = result.into_iter().filter(|item| filter.matches(item)).collect();
    }

    if let Some(expr) = m_expr {
        let filter = parse_m_expression(expr);
        result = result.into_iter().filter(|item| filter.matches_markers(item)).collect();
    }

    result
}

// -k keyword filter: matches against node_id and marker names
enum KFilter {
    Keyword(String),
    Not(Box<KFilter>),
    And(Box<KFilter>, Box<KFilter>),
    Or(Box<KFilter>, Box<KFilter>),
}

impl KFilter {
    fn matches(&self, item: &TestItem) -> bool {
        match self {
            KFilter::Keyword(kw) => {
                let kw_lower = kw.to_lowercase();
                let node_lower = item.node_id.to_lowercase();
                node_lower.contains(&kw_lower)
                    || item.markers.iter().any(|m| m.name.to_lowercase().contains(&kw_lower))
            }
            KFilter::Not(inner) => !inner.matches(item),
            KFilter::And(a, b) => a.matches(item) && b.matches(item),
            KFilter::Or(a, b) => a.matches(item) || b.matches(item),
        }
    }
}

// -m marker filter: matches only against marker names (exact match)
enum MFilter {
    Marker(String),
    Not(Box<MFilter>),
    And(Box<MFilter>, Box<MFilter>),
    Or(Box<MFilter>, Box<MFilter>),
}

impl MFilter {
    fn matches_markers(&self, item: &TestItem) -> bool {
        match self {
            MFilter::Marker(name) => {
                item.markers.iter().any(|m| m.name == *name)
            }
            MFilter::Not(inner) => !inner.matches_markers(item),
            MFilter::And(a, b) => a.matches_markers(item) && b.matches_markers(item),
            MFilter::Or(a, b) => a.matches_markers(item) || b.matches_markers(item),
        }
    }
}

fn parse_k_expression(expr: &str) -> KFilter {
    parse_k_or(expr.trim())
}

fn parse_m_expression(expr: &str) -> MFilter {
    parse_m_or(expr.trim())
}

// -k parser
fn parse_k_or(expr: &str) -> KFilter {
    if let Some(idx) = find_operator(expr, " or ") {
        let left = &expr[..idx];
        let right = &expr[idx + 4..];
        return KFilter::Or(Box::new(parse_k_or(left)), Box::new(parse_k_or(right)));
    }
    parse_k_and(expr)
}

fn parse_k_and(expr: &str) -> KFilter {
    if let Some(idx) = find_operator(expr, " and ") {
        let left = &expr[..idx];
        let right = &expr[idx + 5..];
        return KFilter::And(Box::new(parse_k_and(left)), Box::new(parse_k_and(right)));
    }
    parse_k_not(expr)
}

fn parse_k_not(expr: &str) -> KFilter {
    let trimmed = expr.trim();
    if let Some(rest) = trimmed.strip_prefix("not ") {
        return KFilter::Not(Box::new(parse_k_not(rest)));
    }
    parse_k_atom(trimmed)
}

fn parse_k_atom(expr: &str) -> KFilter {
    let trimmed = expr.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return parse_k_or(&trimmed[1..trimmed.len() - 1]);
    }
    KFilter::Keyword(trimmed.to_string())
}

// -m parser (same structure, different leaf type)
fn parse_m_or(expr: &str) -> MFilter {
    if let Some(idx) = find_operator(expr, " or ") {
        let left = &expr[..idx];
        let right = &expr[idx + 4..];
        return MFilter::Or(Box::new(parse_m_or(left)), Box::new(parse_m_or(right)));
    }
    parse_m_and(expr)
}

fn parse_m_and(expr: &str) -> MFilter {
    if let Some(idx) = find_operator(expr, " and ") {
        let left = &expr[..idx];
        let right = &expr[idx + 5..];
        return MFilter::And(Box::new(parse_m_and(left)), Box::new(parse_m_and(right)));
    }
    parse_m_not(expr)
}

fn parse_m_not(expr: &str) -> MFilter {
    let trimmed = expr.trim();
    if let Some(rest) = trimmed.strip_prefix("not ") {
        return MFilter::Not(Box::new(parse_m_not(rest)));
    }
    parse_m_atom(trimmed)
}

fn parse_m_atom(expr: &str) -> MFilter {
    let trimmed = expr.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return parse_m_or(&trimmed[1..trimmed.len() - 1]);
    }
    MFilter::Marker(trimmed.to_string())
}

fn find_operator(expr: &str, op: &str) -> Option<usize> {
    let mut depth = 0;
    let bytes = expr.as_bytes();
    let op_bytes = op.as_bytes();

    if bytes.len() < op_bytes.len() {
        return None;
    }

    for i in 0..=(bytes.len() - op_bytes.len()) {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && &bytes[i..i + op_bytes.len()] == op_bytes {
            return Some(i);
        }
    }
    None
}
