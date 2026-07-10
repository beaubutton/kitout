//! `brewfile` / `brew`: converge Homebrew packages via `brew bundle`.
//! `brewfile` runs an existing Brewfile by path; `brew` renders an inline
//! package list (taps/formulae/casks/vscode) into a Brewfile and runs the same
//! bundle — so a fragment can declare its tools without a sidecar Brewfile.
//! Both do the tap-trust pass (brew requires non-core taps to be trusted before
//! `brew bundle` will install from them). Homebrew is a shared resource, so
//! these steps are serialized — never run concurrently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

const HOMEBREW_RESOURCE: &str = "homebrew";

fn bundle_check(path: &Path) -> Result<bool> {
    let status = Command::new("brew")
        .args(["bundle", "check"])
        .arg(format!("--file={}", path.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("running `brew bundle check` (is Homebrew installed?)")?;
    Ok(status.success())
}

/// `brew "user/tap/name"` / `cask "user/tap/name"` entries need trusting.
fn trust_taps(path: &Path) -> Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
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

fn bundle_status(path: &Path) -> Result<Status> {
    Ok(if bundle_check(path)? {
        Status::Satisfied
    } else {
        Status::Pending("brew bundle has work to do".into())
    })
}

fn bundle_plan(path: &Path) -> Result<Vec<Change>> {
    Ok(if bundle_check(path)? {
        vec![]
    } else {
        vec![Change {
            summary: format!("brew bundle --upgrade --file={}", path.display()),
            diff: None,
        }]
    })
}

fn bundle_apply(path: &Path) -> Result<Applied> {
    if bundle_check(path)? {
        return Ok(Applied::Unchanged("all packages already current".into()));
    }
    trust_taps(path)?;
    // Capture brew's output: its per-package "Using foo" chatter is noise at
    // success and gold at failure.
    let (ok, output) = crate::ui::run_captured(
        Command::new("brew")
            .args(["bundle", "--upgrade"])
            .arg(format!("--file={}", path.display())),
    )?;
    let mut installed: Vec<&str> = Vec::new();
    let mut already = 0usize;
    for line in output.lines() {
        if let Some(name) = line
            .strip_prefix("Installing ")
            .or(line.strip_prefix("Upgrading "))
        {
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

// ------------------------------------------------------- brewfile (by path) --

pub struct BrewfileStep {
    pub id: String,
    pub needs: Vec<String>,
    pub path: PathBuf,
}

impl Step for BrewfileStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn resource(&self) -> Option<String> {
        Some(HOMEBREW_RESOURCE.into())
    }
    fn check(&self) -> Result<Status> {
        bundle_status(&self.path)
    }
    fn plan(&self) -> Result<Vec<Change>> {
        bundle_plan(&self.path)
    }
    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        bundle_apply(&self.path)
    }
}

// --------------------------------------------------------- brew (inline) -----

pub struct BrewStep {
    pub id: String,
    pub needs: Vec<String>,
    /// Rendered Brewfile content (taps/formulae/casks/vscode).
    pub content: String,
}

impl BrewStep {
    /// Render inline lists into Brewfile lines: taps first (so bundle can
    /// install from them), then formulae, casks, vscode.
    pub fn render(
        taps: &[String],
        formulae: &[String],
        casks: &[String],
        vscode: &[String],
    ) -> String {
        let mut s = String::new();
        for t in taps {
            s.push_str(&format!("tap \"{t}\"\n"));
        }
        for f in formulae {
            s.push_str(&format!("brew \"{f}\"\n"));
        }
        for c in casks {
            s.push_str(&format!("cask \"{c}\"\n"));
        }
        for v in vscode {
            s.push_str(&format!("vscode \"{v}\"\n"));
        }
        s
    }

    /// Write the rendered Brewfile to a temp file and run `f` against its path.
    fn with_temp<T>(&self, f: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
        use std::io::Write;
        let mut tmp = tempfile::Builder::new().suffix(".Brewfile").tempfile()?;
        tmp.write_all(self.content.as_bytes())?;
        tmp.flush()?;
        f(tmp.path())
    }

    /// Package/tap names, for the plan summary (the temp Brewfile path the
    /// shared machinery prints means nothing to a reader).
    fn names(&self) -> String {
        self.content
            .lines()
            .filter_map(|l| l.split('"').nth(1))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Step for BrewStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn resource(&self) -> Option<String> {
        Some(HOMEBREW_RESOURCE.into())
    }
    fn check(&self) -> Result<Status> {
        self.with_temp(bundle_status)
    }
    fn plan(&self) -> Result<Vec<Change>> {
        Ok(if self.with_temp(bundle_check)? {
            vec![]
        } else {
            vec![Change {
                summary: format!("brew bundle --upgrade: {}", self.names()),
                diff: None,
            }]
        })
    }
    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        self.with_temp(bundle_apply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_orders_taps_then_packages() {
        let s = BrewStep::render(
            &["hashicorp/tap".into()],
            &["go".into(), "hashicorp/tap/terraform".into()],
            &["godot-mono".into()],
            &["ms-python.pylance".into()],
        );
        assert_eq!(
            s,
            "tap \"hashicorp/tap\"\n\
             brew \"go\"\n\
             brew \"hashicorp/tap/terraform\"\n\
             cask \"godot-mono\"\n\
             vscode \"ms-python.pylance\"\n"
        );
    }
}
