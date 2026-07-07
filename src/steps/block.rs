//! `block-in-file`: own a markered block inside a file kitout doesn't fully
//! manage (the ~/.zshrc pattern). The block lives between marker comments:
//!
//!   # >>> kitout:aliases >>>
//!   …content…
//!   # <<< kitout:aliases <<<
//!
//! Add/update is idempotent and position-preserving; content outside the
//! markers is never touched.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct BlockInFileStep {
    pub id: String,
    pub needs: Vec<String>,
    pub target: PathBuf,
    pub marker: String,
    pub block: String,
    pub comment_prefix: String,
}

enum State {
    FileMissing,
    BlockMissing(String),          // current content
    BlockDiffers(String, String),  // current content, current inner
    Same,
}

impl BlockInFileStep {
    fn begin(&self) -> String {
        format!("{} >>> {} >>>", self.comment_prefix, self.marker)
    }
    fn end(&self) -> String {
        format!("{} <<< {} <<<", self.comment_prefix, self.marker)
    }
    fn body(&self) -> String {
        self.block.trim_end_matches('\n').to_string()
    }
    fn rendered(&self) -> String {
        format!("{}\n{}\n{}", self.begin(), self.body(), self.end())
    }

    fn state(&self) -> Result<State> {
        let raw = match fs::read_to_string(&self.target) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::FileMissing),
            Err(e) => return Err(e).context("reading target")?,
        };
        let lines: Vec<&str> = raw.lines().collect();
        let begin = lines.iter().position(|l| l.trim() == self.begin());
        let end = lines.iter().position(|l| l.trim() == self.end());
        match (begin, end) {
            (None, None) => Ok(State::BlockMissing(raw)),
            (Some(b), Some(e)) if b < e => {
                let inner = lines[b + 1..e].join("\n");
                if inner == self.body() {
                    Ok(State::Same)
                } else {
                    Ok(State::BlockDiffers(raw, inner))
                }
            }
            _ => bail!(
                "unbalanced block markers for '{}' in {} — fix the file manually",
                self.marker,
                self.target.display()
            ),
        }
    }

    fn write_with_block(&self, raw: Option<&str>) -> Result<()> {
        let content = match raw {
            // Replace an existing block in place, preserving its position.
            Some(raw) => {
                let lines: Vec<&str> = raw.lines().collect();
                let b = lines.iter().position(|l| l.trim() == self.begin()).unwrap();
                let e = lines.iter().position(|l| l.trim() == self.end()).unwrap();
                let mut out: Vec<String> = Vec::new();
                out.extend(lines[..b].iter().map(|s| s.to_string()));
                out.push(self.rendered());
                out.extend(lines[e + 1..].iter().map(|s| s.to_string()));
                out.join("\n") + "\n"
            }
            None => String::new(),
        };
        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent)?;
        }
        if raw.is_some() {
            fs::write(&self.target, content)?;
        } else {
            // Append (creating the file if needed), separated by a blank line.
            let mut existing = fs::read_to_string(&self.target).unwrap_or_default();
            if !existing.is_empty() && !existing.ends_with('\n') {
                existing.push('\n');
            }
            if !existing.is_empty() {
                existing.push('\n');
            }
            existing.push_str(&self.rendered());
            existing.push('\n');
            fs::write(&self.target, existing)?;
        }
        Ok(())
    }
}

impl Step for BlockInFileStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn check(&self) -> Result<Status> {
        Ok(match self.state()? {
            State::Same => Status::Satisfied,
            State::FileMissing | State::BlockMissing(_) => Status::Pending("add managed block".into()),
            State::BlockDiffers(..) => Status::Pending("managed block differs".into()),
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(match self.state()? {
            State::Same => vec![],
            State::FileMissing | State::BlockMissing(_) => vec![Change {
                summary: format!("add block '{}' to {}", self.marker, self.target.display()),
                diff: None,
            }],
            State::BlockDiffers(_, inner) => {
                let body = self.body();
                let diff = similar::TextDiff::from_lines(&inner, &body)
                    .unified_diff()
                    .header(&format!("{} (block '{}')", self.target.display(), self.marker), "manifest")
                    .to_string();
                vec![Change {
                    summary: format!("update block '{}' in {}", self.marker, self.target.display()),
                    diff: Some(diff),
                }]
            }
        })
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        match self.state()? {
            State::Same => Ok(Applied::Unchanged("block already current".into())),
            State::FileMissing | State::BlockMissing(_) => {
                self.write_with_block(None)?;
                Ok(Applied::Changed(format!(
                    "added block '{}' to {}",
                    self.marker,
                    self.target.display()
                )))
            }
            State::BlockDiffers(raw, _) => {
                self.write_with_block(Some(&raw))?;
                Ok(Applied::Changed(format!(
                    "updated block '{}' in {}",
                    self.marker,
                    self.target.display()
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(dir: &std::path::Path, block: &str) -> BlockInFileStep {
        BlockInFileStep {
            id: "t".into(),
            needs: vec![],
            target: dir.join("rc"),
            marker: "kitout:test".into(),
            block: block.into(),
            comment_prefix: "#".into(),
        }
    }

    #[test]
    fn adds_then_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rc"), "existing content\n").unwrap();
        let s = step(dir.path(), "alias a=b\n");
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        assert_eq!(s.check().unwrap(), Status::Satisfied);
        let raw = fs::read_to_string(dir.path().join("rc")).unwrap();
        assert!(raw.starts_with("existing content\n"));
        assert!(raw.contains("# >>> kitout:test >>>\nalias a=b\n# <<< kitout:test <<<"));
    }

    #[test]
    fn updates_in_place_preserving_surroundings() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rc"), "before\n").unwrap();
        let s1 = step(dir.path(), "old\n");
        s1.apply(ConflictPolicy::KeepLocal).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("rc"))
            .unwrap();
        let mut raw = fs::read_to_string(dir.path().join("rc")).unwrap();
        raw.push_str("after\n");
        fs::write(dir.path().join("rc"), raw).unwrap();

        let s2 = step(dir.path(), "new\n");
        s2.apply(ConflictPolicy::KeepLocal).unwrap();
        let raw = fs::read_to_string(dir.path().join("rc")).unwrap();
        assert!(raw.contains("before\n"));
        assert!(raw.contains("after\n"));
        assert!(raw.contains("new"));
        assert!(!raw.contains("old"));
        assert_eq!(s2.check().unwrap(), Status::Satisfied);
    }

    #[test]
    fn unbalanced_markers_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rc"), "# >>> kitout:test >>>\nno end\n").unwrap();
        let s = step(dir.path(), "x\n");
        assert!(s.check().is_err());
    }
}
