//! `secret`: prompt-once, stash in the macOS login Keychain, reuse forever.
//! Typical use is an API token consumed by a later step. Never prints values;
//! unattended runs never prompt (they report Kept instead).

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct SecretStep {
    pub id: String,
    pub needs: Vec<String>,
    /// Keychain service name.
    pub service: String,
    /// Keychain account (defaults to $USER at build time).
    pub account: String,
    /// Human prompt shown when stashing interactively.
    pub prompt: String,
}

impl SecretStep {
    fn present(&self) -> Result<bool> {
        let status = Command::new("security")
            .args([
                "find-generic-password",
                "-a",
                &self.account,
                "-s",
                &self.service,
                "-w",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("running `security find-generic-password`")?;
        Ok(status.success())
    }
}

impl Step for SecretStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn streams_output(&self) -> bool {
        true // prompts
    }

    fn check(&self) -> Result<Status> {
        Ok(if self.present()? {
            Status::Satisfied
        } else {
            Status::Pending(format!("'{}' not in Keychain", self.service))
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(if self.present()? {
            vec![]
        } else {
            vec![Change {
                summary: format!(
                    "prompt for '{}' and stash in the login Keychain",
                    self.service
                ),
                diff: None,
            }]
        })
    }

    fn apply(&self, policy: ConflictPolicy) -> Result<Applied> {
        if self.present()? {
            return Ok(Applied::Unchanged("already in Keychain".into()));
        }
        if policy != ConflictPolicy::Interactive {
            return Ok(Applied::Kept(format!(
                "'{}' not stashed — run `kitout apply` interactively to be prompted",
                self.service
            )));
        }
        let value = crate::ui::sync(|| {
            dialoguer::Password::new()
                .with_prompt(format!("{} (Enter to skip)", self.prompt))
                .allow_empty_password(true)
                .interact()
        })?;
        if value.is_empty() {
            return Ok(Applied::Kept(
                "skipped — no value entered (re-run to add)".into(),
            ));
        }
        let status = Command::new("security")
            .args([
                "add-generic-password",
                "-a",
                &self.account,
                "-s",
                &self.service,
                "-U",
                "-w",
                &value,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("running `security add-generic-password`")?;
        if !status.success() {
            bail!("Keychain write for '{}' failed", self.service);
        }
        Ok(Applied::Changed("stashed in the login Keychain".into()))
    }
}
