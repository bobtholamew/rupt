use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TestResult {
    pub node_id: String,
    pub outcome: Outcome,
    pub duration: f64,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub longrepr: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Passed,
    Failed,
    Skipped,
    Error,
    Xfailed,
    Xpassed,
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Passed => write!(f, "PASSED"),
            Outcome::Failed => write!(f, "FAILED"),
            Outcome::Skipped => write!(f, "SKIPPED"),
            Outcome::Error => write!(f, "ERROR"),
            Outcome::Xfailed => write!(f, "XFAIL"),
            Outcome::Xpassed => write!(f, "XPASS"),
        }
    }
}

use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WorkerMessage {
    Result(TestResult),
    Coverage {
        r#type: String,
        data: BTreeMap<String, Vec<usize>>,
    },
    Finished {
        r#type: String,
    },
}

#[derive(Debug, Default)]
pub struct SessionResult {
    pub results: Vec<TestResult>,
    pub total_duration: f64,
}

impl SessionResult {
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.outcome == Outcome::Passed).count()
    }

    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| r.outcome == Outcome::Failed).count()
    }

    pub fn skipped(&self) -> usize {
        self.results.iter().filter(|r| r.outcome == Outcome::Skipped).count()
    }

    pub fn errors(&self) -> usize {
        self.results.iter().filter(|r| r.outcome == Outcome::Error).count()
    }

    pub fn xfailed(&self) -> usize {
        self.results.iter().filter(|r| r.outcome == Outcome::Xfailed).count()
    }

    pub fn exit_code(&self) -> i32 {
        if self.failed() > 0 || self.errors() > 0 {
            1
        } else {
            0
        }
    }
}
