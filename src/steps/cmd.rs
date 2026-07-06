//! `command-if-missing`: probe for a binary; run an installer when absent.
//! Covers the npm-global / dotnet-tool pattern from Setup.sh.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Change, ConflictPolicy, Status, Step};

pub struct CommandIfMissingStep {
    pub id: String,
    pub needs: Vec<String>,
    /// Binary name looked up on PATH.
    pub probe: String,
    /// Installer argv, spawned directly (no shell).
    pub install: Vec<String>,
}

fn on_path(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else { return false };
    std::env::split_paths(&path).any(|d| {
        let p = d.join(bin);
        p.is_file() || p.symlink_metadata().is_ok()
    })
}

impl Step for CommandIfMissingStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn check(&self) -> Result<Status> {
        Ok(if on_path(&self.probe) {
            Status::Satisfied
        } else {
            Status::Pending(format!("install (provides `{}`)", self.probe))
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(if on_path(&self.probe) {
            vec![]
        } else {
            vec![Change {
                summary: format!("run `{}` (provides `{}`)", self.install.join(" "), self.probe),
                diff: None,
            }]
        })
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<()> {
        if on_path(&self.probe) {
            return Ok(());
        }
        if self.install.is_empty() {
            bail!("command-if-missing '{}' has an empty install command", self.id);
        }
        let status = Command::new(&self.install[0])
            .args(&self.install[1..])
            .status()
            .with_context(|| format!("spawning {}", self.install[0]))?;
        if !status.success() {
            bail!("installer for `{}` exited with {status}", self.probe);
        }
        println!("  + installed `{}`", self.probe);
        Ok(())
    }
}
