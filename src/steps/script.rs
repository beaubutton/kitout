use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::manifest::OnError;
use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

/// Escape hatch: run any executable. Scripts own their own idempotency, so
/// `check` always reports pending and `plan` describes the invocation.
pub struct ScriptStep {
    pub id: String,
    pub needs: Vec<String>,
    pub path: PathBuf,
    pub on_error: OnError,
}

impl Step for ScriptStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn warn_on_error(&self) -> bool {
        self.on_error == OnError::Warn
    }
    fn streams_output(&self) -> bool {
        true
    }

    fn check(&self) -> Result<Status> {
        Ok(Status::Pending(format!("run {}", self.path.display())))
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(vec![Change {
            summary: format!("run {}", self.path.display()),
            diff: None,
        }])
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        let status = Command::new(&self.path)
            .status()
            .with_context(|| format!("spawning {}", self.path.display()))?;
        if !status.success() {
            bail!("{} exited with {}", self.path.display(), status);
        }
        Ok(Applied::Changed("completed".into()))
    }
}
