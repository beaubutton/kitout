//! `absent`: probe for something installed; run a removal command when it's
//! present. The mirror of `command-if-missing` — stateless (presence is read
//! off the machine, never a state file) and idempotent (once it's gone the step
//! is Satisfied). Covers uninstalling anything you can name a removal command
//! for: a bundled app (`rm -rf /Applications/Pages.app`), a brew/npm/mas
//! package, etc.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};
use crate::steps::cmd::on_path;

pub struct AbsentStep {
    pub id: String,
    pub needs: Vec<String>,
    /// What to look for. A value with '/' is a path (tilde-expanded); a bare
    /// name is looked up on `PATH`. Present → remove; absent → Satisfied.
    pub probe: String,
    /// Removal argv, spawned directly (no shell).
    pub remove: Vec<String>,
}

impl Step for AbsentStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn check(&self) -> Result<Status> {
        Ok(if on_path(&self.probe) {
            Status::Pending(format!("remove (`{}` present)", self.probe))
        } else {
            Status::Satisfied
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        Ok(if on_path(&self.probe) {
            vec![Change {
                summary: format!("run `{}` (removes `{}`)", self.remove.join(" "), self.probe),
                diff: None,
            }]
        } else {
            vec![]
        })
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        if !on_path(&self.probe) {
            return Ok(Applied::Unchanged(format!(
                "`{}` already absent",
                self.probe
            )));
        }
        if self.remove.is_empty() {
            bail!("absent '{}' has an empty remove command", self.id);
        }
        let (ok, output) =
            crate::ui::run_captured(Command::new(&self.remove[0]).args(&self.remove[1..]))
                .with_context(|| format!("spawning {}", self.remove[0]))?;
        if !ok {
            crate::ui::dump_tail(&output, 15);
            bail!("remove for `{}` failed (output above)", self.probe);
        }
        // A removal that exits 0 but leaves the probe present (SIP-protected
        // path, wrong package name) would otherwise loop as pending forever —
        // surface it as a failure now rather than silently not-converging.
        if on_path(&self.probe) {
            bail!(
                "`{}` still present after `{}`",
                self.probe,
                self.remove.join(" ")
            );
        }
        Ok(Applied::Changed(format!("removed `{}`", self.probe)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step::ConflictPolicy;

    fn step(probe: &str, remove: Vec<&str>) -> AbsentStep {
        AbsentStep {
            id: "t".into(),
            needs: vec![],
            probe: probe.into(),
            remove: remove.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn absent_probe_is_satisfied() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        let s = step(missing.to_str().unwrap(), vec!["true"]);
        assert!(matches!(s.check().unwrap(), Status::Satisfied));
        assert!(s.plan().unwrap().is_empty());
    }

    #[test]
    fn present_probe_is_removed_then_satisfied() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("victim");
        std::fs::write(&target, "x").unwrap();
        let p = target.to_str().unwrap();
        let s = step(p, vec!["rm", "-f", p]);

        assert!(matches!(s.check().unwrap(), Status::Pending(_)));
        assert_eq!(s.plan().unwrap().len(), 1);
        assert!(matches!(
            s.apply(ConflictPolicy::KeepLocal).unwrap(),
            Applied::Changed(_)
        ));
        // Now gone: idempotent no-op.
        assert!(matches!(s.check().unwrap(), Status::Satisfied));
        assert!(matches!(
            s.apply(ConflictPolicy::KeepLocal).unwrap(),
            Applied::Unchanged(_)
        ));
    }

    #[test]
    fn remove_that_leaves_probe_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("stubborn");
        std::fs::write(&target, "x").unwrap();
        let p = target.to_str().unwrap();
        // `true` exits 0 but removes nothing → probe still present → error.
        let s = step(p, vec!["true"]);
        assert!(s.apply(ConflictPolicy::KeepLocal).is_err());
    }

    #[test]
    fn empty_remove_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f");
        std::fs::write(&target, "x").unwrap();
        let s = step(target.to_str().unwrap(), vec![]);
        assert!(s.apply(ConflictPolicy::KeepLocal).is_err());
    }
}
