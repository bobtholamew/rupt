use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

/// Aggregated coverage data across all workers.
#[derive(Debug, Default)]
pub struct CoverageData {
    /// file path (relative) -> set of covered line numbers
    pub files: BTreeMap<String, BTreeSet<usize>>,
}

impl CoverageData {
    pub fn merge(&mut self, other: &BTreeMap<String, Vec<usize>>) {
        for (file, lines) in other {
            let entry = self.files.entry(file.clone()).or_default();
            for &line in lines {
                entry.insert(line);
            }
        }
    }

    pub fn total_lines(&self) -> usize {
        self.files.values().map(|s| s.len()).sum()
    }

    pub fn total_files(&self) -> usize {
        self.files.len()
    }
}

pub fn print_coverage_report(data: &CoverageData, fail_under: Option<f64>) {
    if data.files.is_empty() {
        eprintln!("No coverage data collected.");
        return;
    }

    eprintln!();
    eprintln!("---------- coverage: rupt ----------");
    eprintln!("{:<60} {:>6}", "Name", "Lines");
    eprintln!("{}", "-".repeat(68));

    let mut total = 0;
    for (file, lines) in &data.files {
        let count = lines.len();
        total += count;
        eprintln!("{:<60} {:>6}", file, count);
    }

    eprintln!("{}", "-".repeat(68));
    eprintln!("{:<60} {:>6}", "TOTAL", total);

    if let Some(threshold) = fail_under {
        // Without knowing total possible lines, we can only report covered lines
        eprintln!("Coverage threshold: {threshold}%");
    }
}

pub fn write_json_report(data: &CoverageData, path: &Path) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(&data.files)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn write_lcov_report(data: &CoverageData, path: &Path) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;

    for (file, lines) in &data.files {
        writeln!(f, "SF:{file}")?;
        for &lineno in lines {
            writeln!(f, "DA:{lineno},1")?;
        }
        writeln!(f, "LF:{}", lines.len())?;
        writeln!(f, "LH:{}", lines.len())?;
        writeln!(f, "end_of_record")?;
    }

    Ok(())
}
