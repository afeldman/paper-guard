//! Persistent ledger storage.

use std::path::{Path, PathBuf};

use paper_guard_core::{ContentHash, FindingStatus};

use crate::model::RunRecord;

/// A ledger error.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("run {0} not found")]
    NotFound(String),
}

/// A thin convenience alias for the store.
pub type Ledger = LedgerStore;

/// A persistent store for review runs.
#[derive(Debug, Clone)]
pub struct LedgerStore {
    root: PathBuf,
}

impl LedgerStore {
    /// Open (creating if needed) a ledger at the given directory.
    pub fn open<P: AsRef<Path>>(dir: P) -> Result<Self, LedgerError> {
        let root = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(LedgerStore { root })
    }

    /// The directory backing this ledger.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn run_path(&self, run_id: &str) -> PathBuf {
        self.root.join(format!("{run_id}.json"))
    }

    /// Save (create or overwrite) a run record.
    pub fn save_run(&self, run: &RunRecord) -> Result<(), LedgerError> {
        let path = self.run_path(&run.run_id);
        let json = serde_json::to_string_pretty(run)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a run record by id.
    pub fn load_run(&self, run_id: &str) -> Result<RunRecord, LedgerError> {
        let path = self.run_path(run_id);
        if !path.exists() {
            return Err(LedgerError::NotFound(run_id.to_string()));
        }
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// List all run ids, sorted ascending.
    pub fn list_runs(&self) -> Result<Vec<String>, LedgerError> {
        let mut runs = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                runs.push(name.trim_end_matches(".json").to_string());
            }
        }
        runs.sort();
        Ok(runs)
    }

    /// Load all run records.
    pub fn load_all(&self) -> Result<Vec<RunRecord>, LedgerError> {
        let mut out = Vec::new();
        for id in self.list_runs()? {
            if let Ok(run) = self.load_run(&id) {
                out.push(run);
            }
        }
        Ok(out)
    }

    /// Find the most recent run that used the same input hash, for change
    /// tracking across iterations.
    pub fn latest_for_input(
        &self,
        input_hash: &ContentHash,
    ) -> Result<Option<RunRecord>, LedgerError> {
        let mut best: Option<RunRecord> = None;
        for id in self.list_runs()? {
            if let Ok(run) = self.load_run(&id) {
                if &run.input_hash != input_hash {
                    continue;
                }
                let is_newer = match &best {
                    None => true,
                    Some(b) => {
                        // Prefer a later timestamp; break ties by run id so
                        // iterating runs yields the last one as "latest".
                        run.timestamp > b.timestamp
                            || (run.timestamp == b.timestamp && run.run_id > b.run_id)
                    }
                };
                if is_newer {
                    best = Some(run);
                }
            }
        }
        Ok(best)
    }

    /// Update the status of a tracked finding across runs.
    ///
    /// This sets the finding's lifecycle status and, for terminal states,
    /// records which run produced the change.
    pub fn update_finding_status(
        &self,
        run_id: &str,
        finding_id: &str,
        status: FindingStatus,
    ) -> Result<(), LedgerError> {
        let mut run = self.load_run(run_id)?;
        for f in &mut run.findings {
            if f.finding_id == finding_id {
                f.status = status;
                if matches!(
                    status,
                    FindingStatus::Resolved
                        | FindingStatus::Rejected
                        | FindingStatus::Regressed
                        | FindingStatus::Revised
                ) {
                    f.resolved_in = Some(run_id.to_string());
                }
            }
        }
        self.save_run(&run)
    }

    /// Compute whether a list of findings regressed relative to a prior run:
    /// a finding that was Resolved but reappears as Open indicates regression.
    pub fn detect_regressions(
        &self,
        prior_run_id: &str,
        current_finding_ids: &[String],
    ) -> Result<Vec<String>, LedgerError> {
        let prior = self.load_run(prior_run_id)?;
        let mut regressed = Vec::new();
        for f in &prior.findings {
            if f.status == FindingStatus::Resolved && current_finding_ids.contains(&f.finding_id)
            {
                regressed.push(f.finding_id.clone());
            }
        }
        Ok(regressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::ContentHash;

    fn sample_run(id: &str, hash: &str) -> RunRecord {
        RunRecord::shell(
            id.into(),
            None,
            ContentHash(hash.into()),
            "latex",
            "0.1.0",
            "0.1.0",
            ContentHash::default(),
            "{}",
            "v1",
            "2026-01-01T00:00:00Z",
        )
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pg-ledger-test-{}", std::process::id()));
        let store = LedgerStore::open(&dir).unwrap();
        let run = sample_run("run-001", "abc");
        store.save_run(&run).unwrap();
        let loaded = store.load_run("run-001").unwrap();
        assert_eq!(loaded.run_id, "run-001");
        assert_eq!(loaded.input_hash.as_str(), "abc");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn latest_for_input_finds_newest_run() {
        let dir = std::env::temp_dir().join(format!("pg-ledger-latest-{}", std::process::id()));
        let store = LedgerStore::open(&dir).unwrap();
        store.save_run(&sample_run("run-001", "abc")).unwrap();
        store.save_run(&sample_run("run-002", "abc")).unwrap();
        store.save_run(&sample_run("run-003", "xyz")).unwrap();
        let latest = store.latest_for_input(&ContentHash("abc".into())).unwrap();
        // Among the runs with input hash "abc", run-002 is the latest.
        let run = latest.expect("expected a match");
        assert_eq!(run.run_id, "run-002");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Regression coverage for the finding lifecycle:
    ///   Run 001: PG-001 OPEN
    ///   Run 002: PG-001 REVISED
    ///   Run 003: PG-001 RESOLVED
    ///   Run 004: PG-001 REGRESSED (the problem is reintroduced)
    ///
    /// A later run must not lose the history of earlier findings, and a
    /// previously-resolved finding that reappears as OPEN must be reported as
    /// REGRESSED via `detect_regressions`.
    #[test]
    fn finding_lifecycle_and_regression_detection() {
        use crate::model::FindingRecord;
        use paper_guard_core::FindingSeverity;
        let dir = std::env::temp_dir().join(format!("pg-ledger-lifecycle-{}", std::process::id()));
        let store = LedgerStore::open(&dir).unwrap();

        let mut run1 = sample_run("run-001", "abc");
        // PG-001 starts OPEN in run 001.
        let f = FindingRecord::new(
            "PG-001".into(),
            "adversarial".into(),
            "loc".into(),
            "unsupported_claim".into(),
            FindingSeverity::Major,
            0.9,
            None,
            "text".into(),
            vec![],
            "rec".into(),
            "run-001".into(),
        );
        run1.findings.push(f);
        store.save_run(&run1).unwrap();
        assert_eq!(store.load_run("run-001").unwrap().findings[0].status, paper_guard_core::FindingStatus::Open);

        // Run 002: the finding is revised (address assigned).
        let run2 = {
            let mut r = sample_run("run-002", "abc");
            // Carry the finding forward; mark REVISED.
            r.findings.push(store.load_run("run-001").unwrap().findings[0].clone());
            r
        };
        store.save_run(&run2).unwrap();
        store.update_finding_status("run-002", "PG-001", paper_guard_core::FindingStatus::Revised).unwrap();
        assert_eq!(store.load_run("run-002").unwrap().findings[0].status, paper_guard_core::FindingStatus::Revised);

        // Run 003: resolved.
        let run3 = {
            let mut r = sample_run("run-003", "abc");
            r.findings.push(store.load_run("run-002").unwrap().findings[0].clone());
            r
        };
        store.save_run(&run3).unwrap();
        store.update_finding_status("run-003", "PG-001", paper_guard_core::FindingStatus::Resolved).unwrap();
        assert_eq!(store.load_run("run-003").unwrap().findings[0].status, paper_guard_core::FindingStatus::Resolved);
        assert_eq!(store.load_run("run-003").unwrap().findings[0].resolved_in.as_deref(), Some("run-003"));

        // Run 004: the same finding reappears (reintroduced). Regression must
        // be detected relative to the run where it was Resolved.
        let run4 = {
            let mut r = sample_run("run-004", "abc");
            // PG-001 reappears as Open in run 004 (fresh problem reintroduced).
            let mut f = store.load_run("run-003").unwrap().findings[0].clone();
            f.status = paper_guard_core::FindingStatus::Open;
            f.opened_in = "run-004".into();
            f.resolved_in = None;
            r.findings.push(f);
            r
        };
        store.save_run(&run4).unwrap();
        let regressed = store
            .detect_regressions("run-003", &["PG-001".to_string()])
            .unwrap();
        assert_eq!(regressed, vec!["PG-001"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
