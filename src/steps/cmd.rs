//! `command-if-missing`: probe for a binary; run an installer when absent.
//! Covers the npm-global / dotnet-tool install pattern.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct CommandIfMissingStep {
    pub id: String,
    pub needs: Vec<String>,
    /// Binary name looked up on PATH.
    pub probe: String,
    /// Installer argv, spawned directly (no shell).
    pub install: Vec<String>,
}

/// Is `bin` present? A value containing '/' is a path (tilde-expanded); a bare
/// name is looked up on `PATH`. Shared with the `absent` step (its inverse).
pub(crate) fn on_path(bin: &str) -> bool {
    // A probe containing '/' is a path (tilde-expanded), not a PATH lookup —
    // covers binaries in dirs like ~/.dotnet/tools that only login shells see.
    if bin.contains('/') {
        let p = std::path::PathBuf::from(shellexpand::tilde(bin).into_owned());
        return p.is_file() || p.symlink_metadata().is_ok();
    }
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
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
                summary: format!(
                    "run `{}` (provides `{}`)",
                    self.install.join(" "),
                    self.probe
                ),
                diff: None,
            }]
        })
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        if on_path(&self.probe) {
            return Ok(Applied::Unchanged(format!(
                "`{}` already present",
                self.probe
            )));
        }
        if self.install.is_empty() {
            bail!(
                "command-if-missing '{}' has an empty install command",
                self.id
            );
        }
        let (ok, output) =
            crate::ui::run_captured(Command::new(&self.install[0]).args(&self.install[1..]))
                .with_context(|| format!("spawning {}", self.install[0]))?;
        if !ok {
            crate::ui::dump_tail(&output, 15);
            bail!("installer for `{}` failed (output above)", self.probe);
        }
        Ok(Applied::Changed(format!("installed `{}`", self.probe)))
    }
}
