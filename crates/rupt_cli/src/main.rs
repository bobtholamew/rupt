use clap::Parser;
use notify::{RecursiveMode, Watcher};
use rupt_core::config;
use rupt_core::executor::{self, ExecutorConfig};
use rupt_core::reporting::{self, ReportConfig, TbStyle};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::mpsc;

#[derive(Parser)]
#[command(name = "rupt", about = "A fast Python test runner")]
struct Cli {
    /// Only collect tests, don't execute
    #[arg(long, visible_alias = "co")]
    collect_only: bool,

    /// Filter tests by keyword expression
    #[arg(short)]
    k: Option<String>,

    /// Filter by marker expression
    #[arg(short)]
    m: Option<String>,

    /// Quiet output
    #[arg(short, long)]
    q: bool,

    /// Verbose output
    #[arg(short, long, action = clap::ArgAction::Count)]
    v: u8,

    /// Show captured output (disable capture)
    #[arg(short)]
    s: bool,

    /// Stop on first failure
    #[arg(short)]
    x: bool,

    /// Stop after N failures
    #[arg(long)]
    maxfail: Option<usize>,

    /// Number of parallel workers (0 or "auto" = CPU count)
    #[arg(short)]
    n: Option<String>,

    /// Traceback style: short, long, line, no
    #[arg(long, default_value = "short")]
    tb: String,

    /// Write JUnit XML report
    #[arg(long)]
    junit_xml: Option<PathBuf>,

    /// Timeout per test in seconds
    #[arg(long)]
    timeout: Option<f64>,

    /// Show N slowest tests
    #[arg(long)]
    durations: Option<usize>,

    /// Only show durations above this threshold (seconds)
    #[arg(long, default_value = "0.0")]
    durations_min: f64,

    /// Measure coverage for source directory
    #[arg(long = "cov")]
    cov: Option<Vec<String>>,

    /// Coverage report format: term, json, lcov
    #[arg(long = "cov-report", default_value = "term")]
    cov_report: String,

    /// Fail if coverage below threshold
    #[arg(long = "cov-fail-under")]
    cov_fail_under: Option<f64>,

    /// Watch mode — re-run on file changes
    #[arg(long)]
    watch: bool,

    /// Python executable to use
    #[arg(long, default_value = "python")]
    python: String,

    /// Paths to search for tests
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let root = if cli.paths.len() == 1 {
        dunce::canonicalize(&cli.paths[0]).unwrap_or_else(|_| cli.paths[0].clone())
    } else {
        dunce::canonicalize(std::env::current_dir().unwrap())
            .unwrap_or_else(|_| std::env::current_dir().unwrap())
    };

    let config = config::load_config(&root);
    let result = rupt_core::collect(&root, &config, cli.k.as_deref(), cli.m.as_deref());

    // Collect-only mode
    if cli.collect_only {
        if cli.q {
            for item in &result.items {
                println!("{}", item.node_id);
            }
            println!();
            println!("{} tests collected.", result.items.len());
        } else {
            println!("<Module>");
            let mut current_class: Option<&str> = None;
            for item in &result.items {
                if let Some(ref class) = item.class_name {
                    if current_class != Some(class.as_str()) {
                        println!("  <Class {class}>");
                        current_class = Some(class);
                    }
                    println!("    <Function {}>", item.function_name);
                } else {
                    current_class = None;
                    println!("  <Function {}>", item.function_name);
                }
            }
            println!();
            println!("{} tests collected.", result.items.len());
        }
        return;
    }

    // Execution mode
    let verbosity = if cli.q { -1 } else { cli.v as i32 };

    let report_config = ReportConfig {
        verbosity,
        show_capture: cli.s,
        tb_style: match cli.tb.as_str() {
            "long" => TbStyle::Long,
            "line" => TbStyle::Line,
            "no" => TbStyle::No,
            _ => TbStyle::Short,
        },
    };

    let workers = match cli.n.as_deref() {
        Some("auto") | Some("0") => num_cpus(),
        Some(n) => n.parse().unwrap_or(1),
        None => 1,
    };

    let coverage_sources = cli.cov.clone().unwrap_or_default();

    let exec_config = ExecutorConfig {
        python: cli.python.clone(),
        runner_path: find_runner_path(),
        workers,
        fail_fast: cli.x,
        max_failures: cli.maxfail,
        coverage_sources,
        timeout: cli.timeout,
    };

    let test_ids: Vec<String> = result.items.iter().map(|i| i.node_id.clone()).collect();

    if test_ids.is_empty() {
        eprintln!("no tests collected");
        return;
    }

    reporting::print_header();
    eprintln!("collected {} tests", test_ids.len());
    eprintln!();

    let (session, coverage) = executor::execute(&root, &test_ids, &exec_config, &mut |r| {
        reporting::print_result_live(r, &report_config);
    });

    reporting::print_failures(&session, &report_config);

    // --durations
    if let Some(n) = cli.durations {
        print_durations(&session, n, cli.durations_min);
    }

    reporting::print_short_summary(&session);
    reporting::print_summary(&session, &report_config);

    // Coverage reporting
    if cli.cov.is_some() && coverage.total_files() > 0 {
        match cli.cov_report.as_str() {
            "json" => {
                let path = root.join("coverage.json");
                if let Err(e) = rupt_core::coverage::write_json_report(&coverage, &path) {
                    eprintln!("error writing coverage json: {e}");
                } else {
                    eprintln!("Coverage JSON written to {}", path.display());
                }
            }
            "lcov" => {
                let path = root.join("coverage.lcov");
                if let Err(e) = rupt_core::coverage::write_lcov_report(&coverage, &path) {
                    eprintln!("error writing coverage lcov: {e}");
                } else {
                    eprintln!("Coverage LCOV written to {}", path.display());
                }
            }
            _ => {
                rupt_core::coverage::print_coverage_report(&coverage, cli.cov_fail_under);
            }
        }
    }

    if let Some(ref junit_path) = cli.junit_xml {
        if let Err(e) = rupt_core::junit::write_junit_xml(&session, junit_path) {
            eprintln!("error writing junit xml: {e}");
        }
    }

    if !cli.watch {
        process::exit(session.exit_code());
    }

    // Watch mode: wait for file changes and re-run
    watch_loop(&root, &cli, &report_config, &exec_config);
}

fn watch_loop(root: &Path, cli: &Cli, report_config: &ReportConfig, exec_config: &ExecutorConfig) {
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
        if let Ok(event) = res {
            if event.paths.iter().any(|p| {
                p.extension().is_some_and(|ext| ext == "py")
            }) {
                let _ = tx.send(());
            }
        }
    })
    .expect("failed to create file watcher");

    watcher
        .watch(root, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    eprintln!();
    eprintln!("Watching for changes... (Ctrl+C to stop)");

    loop {
        // Wait for a change
        let _ = rx.recv();
        // Debounce: drain any extra events that arrived
        while rx.try_recv().is_ok() {}
        std::thread::sleep(std::time::Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        // Clear screen
        eprint!("\x1b[2J\x1b[H");

        // Re-collect and re-run
        let config = config::load_config(root);
        let result = rupt_core::collect(root, &config, cli.k.as_deref(), cli.m.as_deref());
        let test_ids: Vec<String> = result.items.iter().map(|i| i.node_id.clone()).collect();

        if test_ids.is_empty() {
            eprintln!("no tests collected");
            continue;
        }

        reporting::print_header();
        eprintln!("collected {} tests", test_ids.len());
        eprintln!();

        let (session, _coverage) = executor::execute(root, &test_ids, exec_config, &mut |r| {
            reporting::print_result_live(r, report_config);
        });

        reporting::print_failures(&session, report_config);
        reporting::print_short_summary(&session);
        reporting::print_summary(&session, report_config);

        eprintln!();
        eprintln!("Watching for changes... (Ctrl+C to stop)");
    }
}

fn print_durations(session: &rupt_core::results::SessionResult, n: usize, min_secs: f64) {
    let mut sorted: Vec<_> = session
        .results
        .iter()
        .filter(|r| r.duration >= min_secs)
        .collect();
    sorted.sort_by(|a, b| b.duration.partial_cmp(&a.duration).unwrap());
    sorted.truncate(n);

    if sorted.is_empty() {
        return;
    }

    eprintln!();
    eprintln!("========================== slowest {n} durations ==========================");
    for r in &sorted {
        eprintln!("{:.4}s {}", r.duration, r.node_id);
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn find_runner_path() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));

    let candidates = [
        exe_dir.join("../../python"),
        exe_dir.join("../../../python"),
        exe_dir.join("../python"),
        exe_dir.join("python"),
        std::env::current_dir()
            .unwrap_or_default()
            .join("python"),
    ];

    for candidate in &candidates {
        if candidate.join("rupt_runner").exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    "python".to_string()
}
