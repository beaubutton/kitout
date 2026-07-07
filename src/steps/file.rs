use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use similar::TextDiff;

use crate::manifest::OnConflict;
use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

/// Install a file from the manifest repo to a target path, with the locked
/// conflict semantics: local edits are never destroyed silently.
pub struct FileStep {
    pub id: String,
    pub needs: Vec<String>,
    pub source: PathBuf,
    pub target: PathBuf,
    pub on_conflict: OnConflict,
}

enum State {
    Missing,
    Same,
    Differs,
}

impl FileStep {
    fn state(&self) -> Result<State> {
        let src = fs::read(&self.source)
            .with_context(|| format!("reading source {}", self.source.display()))?;
        match fs::read(&self.target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::Missing),
            Err(e) => Err(e).with_context(|| format!("reading target {}", self.target.display())),
            Ok(dst) if dst == src => Ok(State::Same),
            Ok(_) => Ok(State::Differs),
        }
    }

    fn diff(&self) -> Result<String> {
        let src = fs::read_to_string(&self.source).unwrap_or_else(|_| "<binary>".into());
        let dst = fs::read_to_string(&self.target).unwrap_or_else(|_| "<binary>".into());
        Ok(TextDiff::from_lines(&dst, &src)
            .unified_diff()
            .header(&self.target.display().to_string(), "manifest")
            .to_string())
    }

    fn install(&self) -> Result<()> {
        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&self.source, &self.target)
            .with_context(|| format!("installing {}", self.target.display()))?;
        Ok(())
    }
}

impl Step for FileStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn resource(&self) -> Option<String> {
        Some(self.target.display().to_string())
    }

    fn check(&self) -> Result<Status> {
        Ok(match self.state()? {
            State::Same => Status::Satisfied,
            State::Missing => Status::Pending("install".into()),
            State::Differs => Status::Pending("target differs from manifest".into()),
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(match self.state()? {
            State::Same => vec![],
            State::Missing => vec![Change {
                summary: format!("install {}", self.target.display()),
                diff: None,
            }],
            State::Differs => vec![Change {
                summary: format!("update {} (conflict policy applies)", self.target.display()),
                diff: Some(self.diff()?),
            }],
        })
    }

    fn apply(&self, policy: ConflictPolicy) -> Result<Applied> {
        match self.state()? {
            State::Same => Ok(Applied::Unchanged("already current".into())),
            State::Missing => {
                self.install()?;
                Ok(Applied::Changed(format!(
                    "installed {}",
                    self.target.display()
                )))
            }
            State::Differs => {
                // Per-step config narrows the policy first.
                let effective = match self.on_conflict {
                    OnConflict::Replace => ConflictPolicy::ForceReplace,
                    OnConflict::Keep => ConflictPolicy::KeepLocal,
                    OnConflict::PromptDiff => policy,
                };
                match effective {
                    ConflictPolicy::ForceReplace => {
                        self.install()?;
                        Ok(Applied::Changed(format!(
                            "replaced {} (local edits overwritten)",
                            self.target.display()
                        )))
                    }
                    ConflictPolicy::KeepLocal => Ok(Applied::Kept(format!(
                        "kept local {} — differs from manifest (run interactively or --force-replace)",
                        self.target.display()
                    ))),
                    // ui::sync suspends any active spinner for the diff+prompt.
                    ConflictPolicy::Interactive => crate::ui::sync(|| {
                        println!("{}", self.diff()?);
                        let overwrite = dialoguer::Confirm::new()
                            .with_prompt(format!("Overwrite {}?", self.target.display()))
                            .default(false)
                            .interact()?;
                        if overwrite {
                            self.install()?;
                            Ok(Applied::Changed(format!("overwrote {}", self.target.display())))
                        } else {
                            Ok(Applied::Kept(format!("kept local {}", self.target.display())))
                        }
                    }),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(dir: &std::path::Path, on_conflict: OnConflict) -> FileStep {
        FileStep {
            id: "t".into(),
            needs: vec![],
            source: dir.join("src.txt"),
            target: dir.join("out/dst.txt"),
            on_conflict,
        }
    }

    #[test]
    fn installs_when_missing_then_satisfied() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("src.txt"), "hello").unwrap();
        let s = step(dir.path(), OnConflict::PromptDiff);
        assert_eq!(s.check().unwrap(), Status::Pending("install".into()));
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        assert_eq!(s.check().unwrap(), Status::Satisfied);
    }

    #[test]
    fn keep_local_never_destroys_edits() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("src.txt"), "manifest").unwrap();
        let s = step(dir.path(), OnConflict::PromptDiff);
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        fs::write(dir.path().join("out/dst.txt"), "local edit").unwrap();
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        let kept = fs::read_to_string(dir.path().join("out/dst.txt")).unwrap();
        assert_eq!(kept, "local edit");
    }

    #[test]
    fn force_replace_converges() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("src.txt"), "manifest").unwrap();
        let s = step(dir.path(), OnConflict::PromptDiff);
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        fs::write(dir.path().join("out/dst.txt"), "local edit").unwrap();
        s.apply(ConflictPolicy::ForceReplace).unwrap();
        let converged = fs::read_to_string(dir.path().join("out/dst.txt")).unwrap();
        assert_eq!(converged, "manifest");
    }
}
