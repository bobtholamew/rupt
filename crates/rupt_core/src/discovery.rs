use crate::config::PytestConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn discover_test_files(root: &Path, config: &PytestConfig) -> Vec<PathBuf> {
    let file_globs = build_globset(&config.python_files);
    let norecurse_globs = build_globset(&config.norecursedirs);
    let ignore_globs = build_globset(&config.collect_ignore);

    let search_roots: Vec<PathBuf> = if config.testpaths.is_empty() {
        vec![root.to_path_buf()]
    } else {
        config
            .testpaths
            .iter()
            .map(|p| root.join(p))
            .filter(|p| p.exists())
            .collect()
    };

    let mut files = Vec::new();

    for search_root in &search_roots {
        let walker = WalkDir::new(search_root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_string_lossy();
                    !should_skip_dir(&name, &norecurse_globs)
                } else {
                    true
                }
            });

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy();

            if !file_name.ends_with(".py") {
                continue;
            }

            if file_globs.is_match(file_name.as_ref()) {
                let rel = path.strip_prefix(root).unwrap_or(path);
                if !ignore_globs.is_match(rel.to_string_lossy().as_ref()) {
                    files.push(path.to_path_buf());
                }
            }
        }
    }

    files.sort();
    files
}

fn should_skip_dir(name: &str, norecurse: &GlobSet) -> bool {
    if matches!(name, ".git" | "__pycache__" | ".tox" | ".nox" | "node_modules") {
        return true;
    }
    norecurse.is_match(name)
}

fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| {
        GlobSetBuilder::new().build().unwrap()
    })
}
