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

    /// Args after `claude mcp` for registration. The `claude mcp add` CLI
    /// (v2.1) is `add [opts] <name> <commandOrUrl> [args...]` and treats
    /// `-e`/`--header` as VARIADIC — so the positionals must come BEFORE them
    /// or they get swallowed. The fresh-VM bootstrap test caught both the
    /// stdio (`-e` ate the name) and HTTP (`--header` ate name+url) forms.
    ///   stdio: add -s user <name> -e KEY=val -- <command> args
    ///   http:  add -s user --transport http <name> <url> --header "K: V"
    fn add_args(&self) -> Result<Vec<String>> {
        let mut a = vec!["add".to_string(), "--scope".to_string(), "user".to_string()];
        if let Some(url) = &self.url {
            a.push("--transport".into());
            a.push("http".into());
            a.push(self.name.clone());
            a.push(url.clone());
            for (k, v) in &self.headers {
                a.push("--header".into());
                a.push(format!("{k}: {v}"));
            }
        } else {
            if self.command.is_empty() {
                bail!("mcp-server '{}' has neither command nor url", self.name);
            }
            a.push(self.name.clone());
            for (k, v) in &self.env {
                a.push("-e".into());
                a.push(format!("{k}={v}"));
            }
            a.push("--".into());
            a.extend(self.command.iter().cloned());
        }
        Ok(a)
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
            return Ok(Applied::Unchanged(
                "already registered with Claude Code".into(),
            ));
        }
        let mut cmd = Command::new("claude");
        cmd.arg("mcp").args(self.add_args()?);
        let (ok, output) = crate::ui::run_captured(&mut cmd).context("running `claude mcp add`")?;
        if !ok {
            crate::ui::dump_tail(&output, 10);
            bail!("claude mcp add {} failed (output above)", self.name);
        }
        Ok(Applied::Changed(
            "registered with Claude Code (user scope)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(a: &[String], needle: &str) -> usize {
        a.iter().position(|x| x == needle).unwrap_or_else(|| {
            panic!("{needle:?} not found in {a:?}");
        })
    }

    #[test]
    fn stdio_name_precedes_variadic_env() {
        let step = McpServerStep {
            id: "mcp:godot".into(),
            needs: vec![],
            name: "godot".into(),
            command: vec!["node".into(), "/p/index.js".into()],
            env: BTreeMap::from([("GODOT_PATH".into(), "/g".into())]),
            url: None,
            headers: BTreeMap::new(),
        };
        let a = step.add_args().unwrap();
        // Name must come before -e, else the variadic flag swallows it.
        assert!(pos(&a, "godot") < pos(&a, "-e"), "{a:?}");
        // Subprocess command is handed off after `--`.
        let sep = pos(&a, "--");
        assert_eq!(a[sep + 1], "node", "{a:?}");
        assert_eq!(a[sep + 2], "/p/index.js", "{a:?}");
    }

    #[test]
    fn http_name_and_url_precede_variadic_header() {
        let step = McpServerStep {
            id: "mcp:runcomfy".into(),
            needs: vec![],
            name: "runcomfy".into(),
            command: vec![],
            env: BTreeMap::new(),
            url: Some("https://mcp.example/mcp".into()),
            headers: BTreeMap::from([("Authorization".into(), "Bearer T".into())]),
        };
        let a = step.add_args().unwrap();
        let (name, url, hdr) = (
            pos(&a, "runcomfy"),
            pos(&a, "https://mcp.example/mcp"),
            pos(&a, "--header"),
        );
        assert!(name < url && url < hdr, "{a:?}");
        assert_eq!(a[hdr + 1], "Authorization: Bearer T", "{a:?}");
    }

    #[test]
    fn stdio_without_command_errors() {
        let step = McpServerStep {
            id: "mcp:bad".into(),
            needs: vec![],
            name: "bad".into(),
            command: vec![],
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
        };
        assert!(step.add_args().is_err());
    }
}
