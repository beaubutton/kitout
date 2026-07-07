//! `defaults`: macOS preference writes with per-key change detection, and
//! `killall` only when something actually changed — the Setup.sh step-12
//! pattern as a first-class type.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct DefaultsStep {
    pub id: String,
    pub needs: Vec<String>,
    pub writes: Vec<DefaultsWrite>,
    /// Processes to `killall` when at least one key changed.
    pub kill: Vec<String>,
}

pub struct DefaultsWrite {
    pub domain: String,
    pub key: String,
    pub value: toml::Value,
}

impl DefaultsWrite {
    /// How `defaults read` renders the desired value.
    fn expected(&self) -> Result<String> {
        Ok(match &self.value {
            toml::Value::Boolean(b) => if *b { "1" } else { "0" }.to_string(),
            toml::Value::Integer(i) => i.to_string(),
            toml::Value::String(s) => s.clone(),
            other => bail!("defaults value for {}:{} must be bool, int, or string (got {other})", self.domain, self.key),
        })
    }

    fn type_flag(&self) -> (&'static str, String) {
        match &self.value {
            toml::Value::Boolean(b) => ("-bool", b.to_string()),
            toml::Value::Integer(i) => ("-int", i.to_string()),
            _ => ("-string", self.value.as_str().unwrap_or_default().to_string()),
        }
    }

    fn current(&self) -> Option<String> {
        let out = Command::new("defaults")
            .args(["read", &self.domain, &self.key])
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    }

    fn matches(&self) -> Result<bool> {
        Ok(self.current().as_deref() == Some(self.expected()?.as_str()))
    }
}

impl Step for DefaultsStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn check(&self) -> Result<Status> {
        let missing = self.writes.iter().filter(|w| !w.matches().unwrap_or(false)).count();
        Ok(if missing == 0 {
            Status::Satisfied
        } else {
            Status::Pending(format!("{missing} preference(s) need writing"))
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        let mut changes = Vec::new();
        for w in &self.writes {
            if !w.matches()? {
                changes.push(Change {
                    summary: format!(
                        "defaults write {} {} → {} (now: {})",
                        w.domain,
                        w.key,
                        w.expected()?,
                        w.current().unwrap_or_else(|| "<unset>".into())
                    ),
                    diff: None,
                });
            }
        }
        if !changes.is_empty() && !self.kill.is_empty() {
            changes.push(Change {
                summary: format!("restart {} (killall)", self.kill.join(", ")),
                diff: None,
            });
        }
        Ok(changes)
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        let mut written = 0usize;
        for w in &self.writes {
            if w.matches()? {
                continue;
            }
            let (flag, val) = w.type_flag();
            let status = Command::new("defaults")
                .args(["write", &w.domain, &w.key, flag, &val])
                .status()
                .context("running `defaults write`")?;
            if !status.success() {
                bail!("defaults write {} {} failed", w.domain, w.key);
            }
            written += 1;
        }
        if written == 0 {
            return Ok(Applied::Unchanged(format!(
                "all {} preference(s) already set",
                self.writes.len()
            )));
        }
        for app in &self.kill {
            let _ = Command::new("killall").arg(app).status();
        }
        let restarted = if self.kill.is_empty() {
            String::new()
        } else {
            format!("; restarted {}", self.kill.join(", "))
        };
        Ok(Applied::Changed(format!("wrote {written} preference(s){restarted}")))
    }
}
