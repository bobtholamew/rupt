use crate::results::{Outcome, SessionResult, TestResult};

pub struct ReportConfig {
    pub verbosity: i32,    // -1=quiet, 0=normal, 1=verbose, 2=very verbose
    pub show_capture: bool, // -s flag
    pub tb_style: TbStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TbStyle {
    Short,
    Long,
    Line,
    No,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            verbosity: 0,
            show_capture: false,
            tb_style: TbStyle::Short,
        }
    }
}

pub fn print_header() {
    let version = env!("CARGO_PKG_VERSION");
    let platform = std::env::consts::OS;
    eprintln!(
        "========================== rupt {version} ({platform}) =========================="
    );
}

pub fn print_result_live(result: &TestResult, config: &ReportConfig) {
    if config.verbosity >= 1 {
        // Verbose: show full node_id and outcome
        let marker = outcome_marker(&result.outcome);
        eprintln!("{} {}", result.node_id, marker);
    } else if config.verbosity == 0 {
        // Normal: just a character per test
        let ch = outcome_char(&result.outcome);
        eprint!("{ch}");
    }
    // Quiet: nothing during execution
}

pub fn print_failures(session: &SessionResult, config: &ReportConfig) {
    let failures: Vec<&TestResult> = session
        .results
        .iter()
        .filter(|r| r.outcome == Outcome::Failed || r.outcome == Outcome::Error)
        .collect();

    if failures.is_empty() {
        return;
    }

    if config.tb_style == TbStyle::No {
        return;
    }

    eprintln!();
    eprintln!("================================= FAILURES =================================");

    for result in &failures {
        eprintln!(
            "_________________________________ {} _________________________________",
            short_id(&result.node_id)
        );

        match config.tb_style {
            TbStyle::Short => {
                // Show last frame + assertion
                if !result.longrepr.is_empty() {
                    let lines: Vec<&str> = result.longrepr.lines().collect();
                    // Show from the last "File" line to the end
                    let start = lines
                        .iter()
                        .rposition(|l| l.trim_start().starts_with("File "))
                        .unwrap_or(0);
                    for line in &lines[start..] {
                        eprintln!("{line}");
                    }
                }
            }
            TbStyle::Long => {
                if !result.longrepr.is_empty() {
                    eprintln!("{}", result.longrepr);
                }
            }
            TbStyle::Line => {
                // Just the last line of the traceback
                if !result.longrepr.is_empty() {
                    if let Some(last) = result.longrepr.lines().last() {
                        eprintln!("{last}");
                    }
                }
            }
            TbStyle::No => {}
        }

        if config.show_capture || config.verbosity >= 2 {
            if !result.stdout.is_empty() {
                eprintln!("--- Captured stdout ---");
                eprintln!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                eprintln!("--- Captured stderr ---");
                eprintln!("{}", result.stderr);
            }
        }
    }
}

pub fn print_summary(session: &SessionResult, config: &ReportConfig) {
    if config.verbosity == 0 {
        eprintln!(); // newline after the dots
    }

    let passed = session.passed();
    let failed = session.failed();
    let skipped = session.skipped();
    let errors = session.errors();
    let xfailed = session.xfailed();
    let duration = session.total_duration;

    let mut parts = Vec::new();
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if errors > 0 {
        parts.push(format!("{errors} error"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if xfailed > 0 {
        parts.push(format!("{xfailed} xfailed"));
    }

    let summary = parts.join(", ");
    eprintln!(
        "========================== {summary} in {duration:.2}s =========================="
    );
}

pub fn print_short_summary(session: &SessionResult) {
    let failures: Vec<&TestResult> = session
        .results
        .iter()
        .filter(|r| r.outcome == Outcome::Failed)
        .collect();

    if failures.is_empty() {
        return;
    }

    eprintln!("========================== short test summary info ==========================");
    for r in &failures {
        let msg = if r.message.is_empty() {
            "AssertionError"
        } else {
            &r.message
        };
        eprintln!("FAILED {} - {}", r.node_id, msg);
    }
}

fn outcome_char(outcome: &Outcome) -> char {
    match outcome {
        Outcome::Passed => '.',
        Outcome::Failed => 'F',
        Outcome::Skipped => 's',
        Outcome::Error => 'E',
        Outcome::Xfailed => 'x',
        Outcome::Xpassed => 'X',
    }
}

fn outcome_marker(outcome: &Outcome) -> &str {
    match outcome {
        Outcome::Passed => "PASSED",
        Outcome::Failed => "FAILED",
        Outcome::Skipped => "SKIPPED",
        Outcome::Error => "ERROR",
        Outcome::Xfailed => "XFAIL",
        Outcome::Xpassed => "XPASS",
    }
}

fn short_id(node_id: &str) -> &str {
    // Show just "test_file.py::test_func" without the full path
    node_id
}
