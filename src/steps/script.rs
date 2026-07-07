use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::manifest::OnError;
use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

/// Escape hatch: run any executable. With an optional `check` command the
/// step becomes convergence-aware: check exits 0 → satisfied (plan shows
/// nothing, apply skips the script). Without one, scripts own their own
/// idempotency and always run.
pub struct ScriptStep {
    pub id: String,
    pub needs: Vec<String>,
    pub path: PathBuf,
    pub on_error: OnError,
    /// Optional cheap convergence probe (argv). Exit 0 = satisfied.
    pub check: Option<Vec<String>>,
}

impl ScriptStep {
    /// Some(true)=satisfied, Some(false)=needs running, None=no check command.
    fn check_passes(&self) -> Result<Option<bool>> {
        let Some(cmd) = &self.check else {
            return Ok(None);
        };
        if cmd.is_empty() {
            bail!("script '{}' has an empty check command", self.id);
        }
        let status = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("running check for '{}'", self.id))?;
        Ok(Some(status.success()))
    }
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
        Ok(match self.check_passes()? {
            Some(true) => Status::Satisfied,
            Some(false) => Status::Pending(format!("run {}", self.path.display())),
            None => Status::Pending(format!("run {} (no check command)", self.path.display())),
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        if self.check_passes()? == Some(true) {
            return Ok(vec![]);
        }
        Ok(vec![Change {
            summary: format!("run {}", self.path.display()),
            diff: None,
        }])
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        if self.check_passes()? == Some(true) {
            return Ok(Applied::Unchanged("check passed — script skipped".into()));
        }
        let status = Command::new(&self.path)
            .status()
            .with_context(|| format!("spawning {}", self.path.display()))?;
        if !status.success() {
            bail!("{} exited with {}", self.path.display(), status);
        }
        Ok(Applied::Changed("completed".into()))
    }
}
