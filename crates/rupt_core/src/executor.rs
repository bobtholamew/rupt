use crate::coverage::CoverageData;
use crate::results::{Outcome, SessionResult, TestResult, WorkerMessage};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

pub struct ExecutorConfig {
    pub python: String,
    pub runner_path: String,
    pub workers: usize,
    pub fail_fast: bool,
    pub max_failures: Option<usize>,
    pub coverage_sources: Vec<String>,
    pub timeout: Option<f64>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            python: "python".to_string(),
            runner_path: String::new(),
            workers: 1,
            fail_fast: false,
            max_failures: None,
            coverage_sources: Vec::new(),
            timeout: None,
        }
    }
}

pub fn execute(
    rootdir: &Path,
    test_ids: &[String],
    config: &ExecutorConfig,
    on_result: &mut dyn FnMut(&TestResult),
) -> (SessionResult, CoverageData) {
    if config.workers <= 1 {
        return execute_single(rootdir, test_ids, config, on_result);
    }

    execute_parallel(rootdir, test_ids, config, on_result)
}

fn execute_single(
    rootdir: &Path,
    test_ids: &[String],
    config: &ExecutorConfig,
    on_result: &mut dyn FnMut(&TestResult),
) -> (SessionResult, CoverageData) {
    let start = Instant::now();
    let mut coverage = CoverageData::default();
    let results = spawn_worker(rootdir, test_ids, config, on_result, &mut coverage);
    (
        SessionResult {
            results,
            total_duration: start.elapsed().as_secs_f64(),
        },
        coverage,
    )
}

fn execute_parallel(
    rootdir: &Path,
    test_ids: &[String],
    config: &ExecutorConfig,
    on_result: &mut dyn FnMut(&TestResult),
) -> (SessionResult, CoverageData) {
    let start = Instant::now();
    let mut coverage = CoverageData::default();

    let buckets = partition_by_file(test_ids, config.workers);
    let mut all_results = Vec::new();

    let mut children: Vec<(std::process::Child, Vec<String>)> = buckets
        .into_iter()
        .map(|bucket| {
            let child = spawn_worker_process(rootdir, &bucket, config);
            (child, bucket)
        })
        .collect();

    for (child, bucket) in &mut children {
        let mut reported: HashSet<String> = HashSet::new();

        if let Some(ref mut stdout) = child.stdout {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<WorkerMessage>(&line) {
                    Ok(WorkerMessage::Result(r)) => {
                        reported.insert(r.node_id.clone());
                        on_result(&r);
                        all_results.push(r);

                        if should_stop(&all_results, config) {
                            break;
                        }
                    }
                    Ok(WorkerMessage::Coverage { data, .. }) => {
                        coverage.merge(&data);
                    }
                    Ok(WorkerMessage::Finished { .. }) => break,
                    Err(_) => {}
                }
            }
        }

        let _ = child.wait();

        for test_id in bucket {
            if !reported.contains(test_id) {
                let r = TestResult {
                    node_id: test_id.clone(),
                    outcome: Outcome::Error,
                    duration: 0.0,
                    stdout: String::new(),
                    stderr: String::new(),
                    longrepr: "worker process died before running this test".to_string(),
                    message: "worker crash".to_string(),
                };
                on_result(&r);
                all_results.push(r);
            }
        }
    }

    (
        SessionResult {
            results: all_results,
            total_duration: start.elapsed().as_secs_f64(),
        },
        coverage,
    )
}

fn spawn_worker(
    rootdir: &Path,
    test_ids: &[String],
    config: &ExecutorConfig,
    on_result: &mut dyn FnMut(&TestResult),
    coverage: &mut CoverageData,
) -> Vec<TestResult> {
    let mut child = spawn_worker_process(rootdir, test_ids, config);
    let mut results = Vec::new();
    let mut reported: HashSet<String> = HashSet::new();

    if let Some(ref mut stdout) = child.stdout {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<WorkerMessage>(&line) {
                Ok(WorkerMessage::Result(r)) => {
                    reported.insert(r.node_id.clone());
                    on_result(&r);
                    results.push(r);

                    if should_stop(&results, config) {
                        break;
                    }
                }
                Ok(WorkerMessage::Coverage { data, .. }) => {
                    coverage.merge(&data);
                }
                Ok(WorkerMessage::Finished { .. }) => break,
                Err(_) => {}
            }
        }
    }

    let status = child.wait();
    let crashed = status.map(|s| !s.success()).unwrap_or(true);

    if crashed || reported.len() < test_ids.len() {
        for test_id in test_ids {
            if !reported.contains(test_id) {
                let r = TestResult {
                    node_id: test_id.clone(),
                    outcome: Outcome::Error,
                    duration: 0.0,
                    stdout: String::new(),
                    stderr: String::new(),
                    longrepr: "worker process died before running this test".to_string(),
                    message: "worker crash".to_string(),
                };
                on_result(&r);
                results.push(r);
            }
        }
    }

    results
}

fn should_stop(results: &[TestResult], config: &ExecutorConfig) -> bool {
    if config.fail_fast {
        if let Some(last) = results.last() {
            if last.outcome == Outcome::Failed || last.outcome == Outcome::Error {
                return true;
            }
        }
    }
    if let Some(max) = config.max_failures {
        let fail_count = results
            .iter()
            .filter(|r| r.outcome == Outcome::Failed || r.outcome == Outcome::Error)
            .count();
        if fail_count >= max {
            return true;
        }
    }
    false
}

fn spawn_worker_process(
    rootdir: &Path,
    test_ids: &[String],
    config: &ExecutorConfig,
) -> std::process::Child {
    let mut manifest = serde_json::json!({
        "tests": test_ids,
        "rootdir": rootdir.to_string_lossy(),
    });
    if !config.coverage_sources.is_empty() {
        manifest["coverage_sources"] = serde_json::json!(config.coverage_sources);
    }
    if let Some(timeout) = config.timeout {
        manifest["timeout"] = serde_json::json!(timeout);
    }

    let runner_path = if config.runner_path.is_empty() {
        find_runner_path()
    } else {
        config.runner_path.clone()
    };

    let mut child = Command::new(&config.python)
        .args(["-m", "rupt_runner.worker"])
        .env("PYTHONPATH", &runner_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn python worker");

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(manifest.to_string().as_bytes())
            .expect("failed to write to worker stdin");
    }
    child.stdin.take();

    child
}

fn find_runner_path() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe.parent().unwrap_or(Path::new("."));

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

fn partition_by_file(test_ids: &[String], n: usize) -> Vec<Vec<String>> {
    if n <= 1 {
        return vec![test_ids.to_vec()];
    }

    let mut buckets: Vec<Vec<String>> = (0..n).map(|_| Vec::new()).collect();
    let mut file_groups: Vec<Vec<String>> = Vec::new();
    let mut current_file = String::new();

    for id in test_ids {
        let file = id.split("::").next().unwrap_or("").to_string();
        if file != current_file {
            file_groups.push(Vec::new());
            current_file = file;
        }
        file_groups.last_mut().unwrap().push(id.clone());
    }

    for (i, group) in file_groups.into_iter().enumerate() {
        buckets[i % n].extend(group);
    }

    buckets.into_iter().filter(|b| !b.is_empty()).collect()
}
