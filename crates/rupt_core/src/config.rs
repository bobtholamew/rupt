use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PytestConfig {
    pub testpaths: Vec<String>,
    pub python_files: Vec<String>,
    pub python_classes: Vec<String>,
    pub python_functions: Vec<String>,
    pub norecursedirs: Vec<String>,
    pub markers: Vec<String>,
    pub rootdir: PathBuf,
    pub collect_ignore: Vec<String>,
}

impl Default for PytestConfig {
    fn default() -> Self {
        Self {
            testpaths: vec![],
            python_files: vec!["test_*.py".into(), "*_test.py".into()],
            python_classes: vec!["Test*".into()],
            python_functions: vec!["test_*".into()],
            norecursedirs: vec![
                ".*".into(),
                "build".into(),
                "dist".into(),
                "_darcs".into(),
                "CVS".into(),
                "{arch}".into(),
                "*.egg".into(),
                "venv".into(),
                ".venv".into(),
                "__pycache__".into(),
                ".git".into(),
                "node_modules".into(),
                ".tox".into(),
                ".nox".into(),
                ".hg".into(),
            ],
            markers: vec![],
            rootdir: PathBuf::new(),
            collect_ignore: vec![],
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct PyprojectToml {
    tool: Option<ToolSection>,
}

#[derive(Debug, Deserialize, Default)]
struct ToolSection {
    pytest: Option<PytestSection>,
}

#[derive(Debug, Deserialize, Default)]
struct PytestSection {
    ini_options: Option<IniOptions>,
}

#[derive(Debug, Deserialize, Default)]
struct IniOptions {
    testpaths: Option<Vec<String>>,
    python_files: Option<Vec<String>>,
    python_classes: Option<Vec<String>>,
    python_functions: Option<Vec<String>>,
    norecursedirs: Option<String>,
    markers: Option<Vec<String>>,
    collect_ignore: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct PytestIni {
    pytest: Option<PytestIniSection>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct PytestIniSection {
    testpaths: Option<String>,
    python_files: Option<String>,
    python_classes: Option<String>,
    python_functions: Option<String>,
    norecursedirs: Option<String>,
    markers: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct SetupCfg {
    #[serde(rename = "tool:pytest")]
    tool_pytest: Option<SetupCfgPytest>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct SetupCfgPytest {
    testpaths: Option<String>,
    python_files: Option<String>,
    python_classes: Option<String>,
    python_functions: Option<String>,
    norecursedirs: Option<String>,
}

pub fn load_config(root: &Path) -> PytestConfig {
    let mut config = PytestConfig {
        rootdir: root.to_path_buf(),
        ..Default::default()
    };

    // Priority: pyproject.toml > pytest.ini > setup.cfg (matching pytest's resolution order)
    let pyproject_path = root.join("pyproject.toml");
    if pyproject_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject_path) {
            if let Ok(pyproject) = toml::from_str::<PyprojectToml>(&content) {
                if let Some(opts) = pyproject
                    .tool
                    .and_then(|t| t.pytest)
                    .and_then(|p| p.ini_options)
                {
                    apply_ini_options(&mut config, opts);
                    return config;
                }
            }
        }
    }

    let pytest_ini_path = root.join("pytest.ini");
    if pytest_ini_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&pytest_ini_path) {
            if let Ok(ini) = parse_ini_style(&content) {
                apply_ini_map(&mut config, &ini);
                return config;
            }
        }
    }

    let setup_cfg_path = root.join("setup.cfg");
    if setup_cfg_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&setup_cfg_path) {
            if let Ok(ini) = parse_setup_cfg(&content) {
                apply_ini_map(&mut config, &ini);
                return config;
            }
        }
    }

    config
}

fn apply_ini_options(config: &mut PytestConfig, opts: IniOptions) {
    if let Some(tp) = opts.testpaths {
        config.testpaths = tp;
    }
    if let Some(pf) = opts.python_files {
        config.python_files = pf;
    }
    if let Some(pc) = opts.python_classes {
        config.python_classes = pc;
    }
    if let Some(pfn) = opts.python_functions {
        config.python_functions = pfn;
    }
    if let Some(nr) = opts.norecursedirs {
        config.norecursedirs = split_whitespace_values(&nr);
    }
    if let Some(m) = opts.markers {
        config.markers = m;
    }
    if let Some(ci) = opts.collect_ignore {
        config.collect_ignore = ci;
    }
}

fn split_whitespace_values(s: &str) -> Vec<String> {
    s.split_whitespace().map(|s| s.to_string()).collect()
}

use std::collections::HashMap;

fn parse_ini_style(content: &str) -> Result<HashMap<String, String>, ()> {
    let mut map = HashMap::new();
    let mut in_pytest = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_pytest = trimmed == "[pytest]";
            continue;
        }
        if !in_pytest {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    if map.is_empty() && !in_pytest {
        return Err(());
    }
    Ok(map)
}

fn parse_setup_cfg(content: &str) -> Result<HashMap<String, String>, ()> {
    let mut map = HashMap::new();
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == "[tool:pytest]";
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    if map.is_empty() && !in_section {
        return Err(());
    }
    Ok(map)
}

fn apply_ini_map(config: &mut PytestConfig, map: &HashMap<String, String>) {
    if let Some(v) = map.get("testpaths") {
        config.testpaths = split_whitespace_values(v);
    }
    if let Some(v) = map.get("python_files") {
        config.python_files = split_whitespace_values(v);
    }
    if let Some(v) = map.get("python_classes") {
        config.python_classes = split_whitespace_values(v);
    }
    if let Some(v) = map.get("python_functions") {
        config.python_functions = split_whitespace_values(v);
    }
    if let Some(v) = map.get("norecursedirs") {
        config.norecursedirs = split_whitespace_values(v);
    }
}
