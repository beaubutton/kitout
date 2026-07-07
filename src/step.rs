use anyhow::Result;

/// Result of a read-only convergence check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Machine already matches the manifest for this step.
    Satisfied,
    /// Work is needed; the string is a one-line human summary.
    Pending(String),
}

/// One concrete change `apply` would make, for `kitout plan` output.
#[derive(Debug, Clone)]
pub struct Change {
    pub summary: String,
    /// Unified diff when the change rewrites an existing file.
    pub diff: Option<String>,
}

/// How conflicts (local edits to managed targets) are resolved.
/// Locked design decision: local edits are sacred — unattended runs never
/// destroy them unless `--force-replace` is passed explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Show the diff and ask (prompts are serialized by the executor).
    Interactive,
    /// `apply -y`: keep the local version, warn loudly.
    KeepLocal,
    /// `apply --force-replace`: the manifest wins (servers/CI).
    ForceReplace,
}

/// A typed, idempotent unit of machine convergence.
///
/// Contract: `check` and `plan` are read-only; `apply` is idempotent and
/// respects the conflict policy. Implementations must be `Send + Sync` so the
/// DAG executor can run independent steps in parallel.
/// What `apply` did, with a one-line human summary. Steps do not print their
/// own result lines — the executor renders these uniformly (ui.rs).
#[derive(Debug, Clone)]
pub enum Applied {
    /// Nothing needed doing.
    Unchanged(String),
    /// Work was done.
    Changed(String),
    /// Deliberately did NOT converge (kept local edits) — warn-worthy.
    Kept(String),
}

pub trait Step: Send + Sync {
    fn id(&self) -> &str;
    fn needs(&self) -> &[String];
    fn check(&self) -> Result<Status>;
    fn plan(&self) -> Result<Vec<Change>>;
    fn apply(&self, policy: ConflictPolicy) -> Result<Applied>;
    /// Soft-fail steps warn instead of failing the run (drain-and-report).
    fn warn_on_error(&self) -> bool {
        false
    }
    /// Steps that stream child output / prompt interactively announce
    /// themselves with a banner so their output is attributed.
    fn streams_output(&self) -> bool {
        false
    }
    /// Steps that mutate a shared resource (e.g. several block-in-file steps
    /// targeting the same file) return the same key here; the scheduler
    /// serializes them in manifest order so parallel waves can't race.
    fn resource(&self) -> Option<String> {
        None
    }
}
