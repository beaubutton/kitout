//! `brewfile`: converge Homebrew packages on a Brewfile via `brew bundle`,
//! including the tap-trust pass (brew requires non-core taps to be trusted
//! before `brew bundle` will install from them).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct BrewfileStep {
    pub id: String,
    pub needs: Vec<String>,
    pub path: PathBuf,
}

impl BrewfileStep {
    fn bundle_check(&self) -> Result<bool> {
        let status = Command::new("brew")
            .args(["bundle", "check"])
            .arg(format!("--file={}", self.path.display()))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("running `brew bundle check` (is Homebrew installed?)")?;
        Ok(status.success())
    }

    /// `brew "user/tap/name"` / `cask "user/tap/name"` entries need trusting.
    fn trust_taps(&self) -> Result<()> {
        let raw = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        for line in raw.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            for (kw, flag) in [("brew", "--formula"), ("cask", "--cask")] {
                if let Some(rest) = line.strip_prefix(kw) {
                    let rest = rest.trim();
                    if let Some(name) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                        if name.matches('/').count() == 2 {
                            let _ = Command::new("brew")
                                .args(["trust", flag, name, "--quiet"])
                                .status();
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl Step for BrewfileStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn check(&self) -> Result<Status> {
        Ok(if self.bundle_check()? {
            Status::Satisfied
        } else {
            Status::Pending("brew bundle has work to do".into())
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(if self.bundle_check()? {
            vec![]
        } else {
            vec![Change {
                summary: format!("brew bundle --upgrade --file={}", self.path.display()),
                diff: None,
            }]
        })
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        if self.bundle_check()? {
            return Ok(Applied::Unchanged("all packages already current".into()));
        }
        self.trust_taps()?;
        // Capture brew's output: its per-package "Using foo" chatter is noise
        // at success and gold at failure.
        let (ok, output) = crate::ui::run_captured(
            Command::new("brew")
                .args(["bundle", "--upgrade"])
                .arg(format!("--file={}", self.path.display())),
        )?;
        let mut installed: Vec<&str> = Vec::new();
        let mut already = 0usize;
        for line in output.lines() {
            if let Some(name) = line.strip_prefix("Installing ").or(line.strip_prefix("Upgrading ")) {
                installed.push(name.split_whitespace().next().unwrap_or(name));
            } else if line.starts_with("Using ") {
                already += 1;
            }
        }
        if !ok {
            crate::ui::dump_tail(&output, 20);
            bail!("brew bundle failed (full output above)");
        }
        Ok(Applied::Changed(format!(
            "installed/upgraded {} ({}); {} already current",
            installed.len(),
            installed.join(", "),
            already
        )))
    }
}
