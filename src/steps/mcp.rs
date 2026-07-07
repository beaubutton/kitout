//! FLAGSHIP: idempotent MCP server registration for coding agents.
//! v0 supports Claude Code (`claude mcp get/add`); the `agent` field exists
//! so codex/gemini registration can join without a schema break.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct McpServerStep {
    pub id: String,
    pub needs: Vec<String>,
    pub name: String,
    /// Stdio transport: command + args. Mutually exclusive with `url`.
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// HTTP transport URL.
    pub url: Option<String>,
    /// HTTP headers, passed literally (no shell expansion — `${VAR}` reaches
    /// the agent config verbatim for its own runtime expansion).
    pub headers: BTreeMap<String, String>,
}

impl McpServerStep {
    fn registered(&self) -> Result<bool> {
        let status = Command::new("claude")
            .args(["mcp", "get", &self.name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("running `claude mcp get` (is claude installed?)")?;
        Ok(status.success())
    }
}

impl Step for McpServerStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn check(&self) -> Result<Status> {
        Ok(if self.registered()? {
            Status::Satisfied
        } else {
            Status::Pending("register with Claude Code".into())
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(if self.registered()? {
            vec![]
        } else {
            vec![Change {
                summary: format!("claude mcp add {} (user scope)", self.name),
                diff: None,
            }]
        })
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        if self.registered()? {
            return Ok(Applied::Unchanged("already registered with Claude Code".into()));
        }
        let mut cmd = Command::new("claude");
        cmd.args(["mcp", "add", "--scope", "user"]);
        if let Some(url) = &self.url {
            cmd.args(["--transport", "http"]);
            for (k, v) in &self.headers {
                cmd.arg("--header").arg(format!("{k}: {v}"));
            }
            cmd.arg(&self.name).arg(url);
        } else {
            if self.command.is_empty() {
                bail!("mcp-server '{}' has neither command nor url", self.name);
            }
            for (k, v) in &self.env {
                cmd.arg("-e").arg(format!("{k}={v}"));
            }
            cmd.arg(&self.name).arg("--").args(&self.command);
        }
        let (ok, output) = crate::ui::run_captured(&mut cmd).context("running `claude mcp add`")?;
        if !ok {
            crate::ui::dump_tail(&output, 10);
            bail!("claude mcp add {} failed (output above)", self.name);
        }
        Ok(Applied::Changed("registered with Claude Code (user scope)".into()))
    }
}
