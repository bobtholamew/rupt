use crate::config::PytestConfig;
use crate::fixtures::{FixtureDef, extract_fixtures};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Maps directory paths to the fixtures defined in conftest.py files at that level.
#[derive(Debug, Clone)]
pub struct ConftestMap {
    pub levels: BTreeMap<PathBuf, Vec<FixtureDef>>,
}

impl ConftestMap {
    pub fn fixtures_for(&self, test_file: &Path) -> Vec<&FixtureDef> {
        let mut result = Vec::new();
        let dir = test_file.parent().unwrap_or(test_file);

        for (conftest_dir, fixtures) in &self.levels {
            if dir.starts_with(conftest_dir) {
                result.extend(fixtures.iter());
            }
        }

        result
    }
}

pub fn build_conftest_map(root: &Path, _config: &PytestConfig) -> ConftestMap {
    let mut levels = BTreeMap::new();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy();
        if name != "conftest.py" {
            continue;
        }

        let path = entry.path();

        // Skip hidden/venv directories
        let rel = path.strip_prefix(root).unwrap_or(path);
        let skip = rel.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s.starts_with('.') || s == "__pycache__" || s == "node_modules"
                || s == ".venv" || s == "venv" || s == ".tox" || s == ".nox"
        });
        if skip {
            continue;
        }

        let fixtures = extract_fixtures(path);
        if let Some(parent) = path.parent() {
            levels.insert(parent.to_path_buf(), fixtures);
        }
    }

    ConftestMap { levels }
}
